#[macro_use]
extern crate lazy_static;

use scrypto::jubjub::JubjubBls12;
use bellman::SynthesisError;
pub mod circuit_transfer;
pub mod circuit_mimc;
pub mod prover;
pub mod circuit_test;
pub mod keys;
pub mod elgamal;



lazy_static! {
    pub static ref PARAMS: JubjubBls12 = { JubjubBls12::new() };
}

// TODO: This should probably be removed and we
// should use existing helper methods on `Option`
// for mapping with an error.
/// This basically is just an extension to `Option`
/// which allows for a convenient mapping to an
/// error on `None`.
trait Assignment<T> {
    fn get(&self) -> Result<&T, SynthesisError>;
}

impl<T> Assignment<T> for Option<T> {
    fn get(&self) -> Result<&T, SynthesisError> {
        match *self {
            Some(ref v) => Ok(v),
            None => Err(SynthesisError::AssignmentMissing)
        }
    }
}
