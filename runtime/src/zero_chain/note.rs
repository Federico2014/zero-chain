extern crate substrate_primitives as primitives;
extern crate parity_crypto as crypto;
extern crate crypto as rcrypto;
extern crate rand;
extern crate substrate_keystore;

use self::rand::{Rng, OsRng};
use primitives::{hashing::blake2_256, ed25519::{Pair, Public, PKCS_LEN}};
use self::rcrypto::ed25519::exchange;
// use {untrusted, pkcs8, error, der};

pub type SecretKey = [u8; PKCS_LEN];
pub type Publickey = Public;


fn to_array(slice: &[u8]) -> [u8; 16] {
    let mut array = [0u8; 16];
    for (&x, p) in slice.iter().zip(array.iter_mut()) {
        *p = x;
    }
    array
}

// TODO: looping for hashes
/// Get keys for encrypt and mac through the key derivation function
fn concat_kdf(key_material: [u8; 32]) -> ([u8; 16], [u8; 16]) {
    // const SHA256BlockSize: usize = 64;
    // const reps: usize = (32 + 7) * 8 / (SHA256BlockSize * 8);

    // let mut buffers: Vec<u8> = Vec::new();
    // for counter in 0..(reps+1) {
    //     let mut sha256 = Sha256::new();
    //     let mut tmp: Vec<u8> = Vec::new();
    //     tmp.write_u32::<BigEndian>((counter + 1) as u32).unwrap();
    //     sha256.input(&tmp);
    //     sha256.input(&key_material);
    //     buffers.append(&mut sha256.result().as_ref().into());
    // }

    let hash = blake2_256(&key_material);
    let (left_hash, right_hash) = hash.split_at(16);
    (to_array(left_hash), to_array(right_hash))    
}


// trait GetPrivateScalar {
//     fn get_private_scalar(&self) -> [u8; 32];
// }

// struct LinkPair(Pair);
// struct LinkKeyPair(Ed25519KeyPair);

// trait LinkPair {
//     fn get_ed25519_pair(&self) -> Ed25519KeyPair;
// }

// trait LinkKeyPair {
//     fn get_private_scalar(&self) -> [u8; 32];
// }

// impl LinkPair for Pair {
//     fn get_ed25519_pair(&self) -> &Ed25519KeyPair {
//         &self.0
//     }
// }

// impl LinkKeyPair for Ed25519KeyPair {
//     fn get_private_scalar(&self) -> [u8; 32] {
//         &self.private_scalar
//     }
// }

// impl LinkPair {
//     fn get_private_scalar(&self) -> [u8; 32] {
//         LinkKeyPair.get_ed25519_private_scalar()
//     }
// }

// impl LinkKeyPair {
//     fn get_ed25519_private_scalar(&self) -> [u8; 32] {
//         self.private_scalar
//     }
// }

// impl GetPrivateScalar for Pair {
//     // fn new(pair: Pair) -> LinkPair {
//     //     LinkPair(pair)
//     // }

//     fn get_private_scalar(&self) -> [u8; 32] {
//         self.0.private_scalar
//     }
// }

// fn unwrap_pkcs8(version: pkcs8::Version, input: untrusted::Input)
//         -> Result<(untrusted::Input, Option<untrusted::Input>),
//                   error::Unspecified> {
//     let (private_key, public_key) =
//         pkcs8::unwrap_key(&PKCS8_TEMPLATE, version, input)?;
//     let private_key = private_key.read_all(error::Unspecified, |input| {
//         der::expect_tag_and_get_value(input, der::Tag::OctetString)
//     })?;
//     Ok((private_key, public_key))
// }

pub struct Note {
    pub value: u64,
    pub public_key: Publickey,
    // pub E::Fs, // the commitment randomness
}

pub struct EncryptedNote {
    ciphertext: Vec<u8>,
    iv: [u8; 16],
    mac: [u8; 32],
    ephemeral_public: Public, 
}

impl EncryptedNote {  
    // TODO: fix type of plain_note 
    /// Encrypt a Note with public key
    pub fn encrypt_note(&self, plain_note: &[u8; PKCS_LEN], public_key: [u8; 32]) -> Self {
        let mut rng = OsRng::new().expect("OS Randomness available on all supported platforms; qed");        

        let ephemeral_secret: [u8; 32] = rng.gen();       

        // Make a new key pair from a seed phrase.
	    // NOTE: prefer pkcs#8 unless security doesn't matter -- this is used primarily for tests. 
        // https://github.com/paritytech/substrate/issues/1063
        let pair = Pair::from_seed(&ephemeral_secret);                    
        let ephemeral_public = pair.public();
            
        let shared_secret = exchange(&public_key, &ephemeral_secret);
                
		// [ DK[0..15] DK[16..31] ] = [derived_left_bits, derived_right_bits]        
        let (derived_left_bits, derived_right_bits) = concat_kdf(shared_secret);            

        // an initialisation vector
        let iv: [u8; 16] = rng.gen();                
        let mut ciphertext = vec![0u8; PKCS_LEN];

        crypto::aes::encrypt_128_ctr(&derived_left_bits, &iv, plain_note, &mut *ciphertext)
            .expect("input lengths of key and iv are both 16; qed");
        
        // Blake2_256(DK[16..31] ++ <ciphertext>), where DK[16..31] - derived_right_bits
        let mac = blake2_256(&crypto::derive_mac(&derived_right_bits, &*ciphertext));

        EncryptedNote {
            ciphertext,
            iv,
            mac,
            ephemeral_public,
        }

    }    
    

    /// Decrypt a Note with secret key
    pub fn decrypt_note(&self, secret_key: &[u8; 32]) -> Result<[u8; PKCS_LEN], ()> {
        let shared_secret = exchange(&self.ephemeral_public.0, secret_key);

        // [ DK[0..15] DK[16..31] ] = [derived_left_bits, derived_right_bits]        
        let (derived_left_bits, derived_right_bits) = concat_kdf(shared_secret); 

        // Blake2_256(DK[16..31] ++ <ciphertext>), where DK[16..31] - derived_right_bits
        let mac = blake2_256(&crypto::derive_mac(&derived_right_bits, &*self.ciphertext));

        // TODO: ref: https://github.com/rust-lang/rust/issues/16913
        if !(&mac[..] == &self.mac[..]) {
            // TODO: elaborate error handling
			panic!("Not match macs");
		}

        let mut plain = [0; PKCS_LEN];
        crypto::aes::decrypt_128_ctr(&derived_left_bits, &self.iv, &self.ciphertext, &mut plain[..])
            .expect("input lengths of key and iv are both 16; qed");
        Ok(plain)
    }
}

#[cfg(test)]
    use super::*;

    #[test]
    fn ok() {
        assert_eq!(4, 2+2);
    }
