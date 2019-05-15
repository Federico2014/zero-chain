/// Defined in appendix B: THE GRAND PRODUCT ARGUMENT
use pairing::{Engine, Field, CurveAffine, CurveProjective};
use bellman::SynthesisError;
use merlin::Transcript;
use crate::{traits, transcript::ProvingTranscript};
use crate::srs::SRS;
use crate::utils::*;
use crate::polynomials::operations::mul_polynomials;
use crate::polynomials::commitment::poly_comm_opening;
use crate::traits::*;
use super::well_formed;

#[derive(Clone)]
pub struct ProductPolys<E: Engine>{
    u_poly: Vec<E::Fr>,
    v_poly: Vec<E::Fr>
}

impl<E: Engine> ProductPolys<E> {
    pub fn new(u_poly: Vec<E::Fr>, v_poly: Vec<E::Fr>) -> Self {
        ProductPolys {
            u_poly,
            v_poly,
        }
    }

    /// Create the Grand product arguments from the given two polynomials
    pub fn gen_arg<PE: PolyEngine<Pairing=E>>(&self, srs: &SRS<E>) -> gprodArg<E, PE> {
        // let (u_poly, v_poly) = polys;
        let n = self.u_poly.len();
        assert!(self.u_poly.len() == self.v_poly.len());

        let mut a_poly = Vec::with_capacity(2 * n + 1);
        let mut c_poly = Vec::with_capacity(2 * n + 1);
        let mut c_coeff = E::Fr::one();

        // c_1 = a_1 * b_1(=1)
        // c_2 = a_2 * b_2(=c_1) = a_2 * a_1 * 1
        // c_3 = a_3 * b_3(=c_2) = a_3 * a_2 * a_1 * 1
        // ...
        // c_n = a_n + c_{n-1} = \prod a_i
        for a in self.u_poly.iter() {
            c_coeff.mul_assign(a);
            c_poly.push(c_coeff);
        }

        // c_n_inv = a_{n+1} = c_{n}^-1
        let c_n_inv = c_poly[n - 1].inverse().unwrap();

        let mut c_coeff = E::Fr::one(); // re-set to one

        // (3) c_{n+1} = 1
        c_poly.push(c_coeff);

        for b in self.v_poly.iter() {
            c_coeff.mul_assign(b);
            c_poly.push(c_coeff);
        }
        assert_eq!(c_poly.len(), 2 * n + 1);

        // (4) c_{2n+1} == c_{n}
        assert_eq!(c_poly[2 * n], c_poly[n - 1]);

        // Define the a_i arguments
        // a_1, a_2, ..., a_n from U
        a_poly.extend(&self.u_poly);
        // a_{n+1} = 0
        a_poly.push(E::Fr::zero());
        // a_{n+2}, a_{n+3}, ..., a_{2n+1} from V
        a_poly.extend(&self.v_poly);

        let c_point = multiexp(
            srs.g_pos_x_alpha[0..n].iter(),
            c_poly.iter()
        ).into_affine();

        let c_comm = PE::Commitment::from_point(&c_point);

        gprodArg {
            a_poly,
            c_poly,
            // a_comm,
            c_comm,
            c_n_inv,
            n,
        }
    }
}

#[derive(Clone)]
pub struct gprodArg<E: Engine, PE: PolyEngine> {
    /// the coeffientis of two commitments U and V,
    /// where U and V are fully well-formed commitments to n-degree polynomials.
    /// U = g^{alpha * \sum\limits_{i=1}^n a_i x^i, V = g^{alpha * \sum\limits_{i=1}^n a_{i+n+1} x^i,
    a_poly: Vec<E::Fr>,

    /// g^{alpha * \sum\limits_{i=1}^{2n+1} c_i x^i
    /// Following the requirements.
    /// (1) a \cdot b = c
    /// (2) b = (1, c_1, ..., c_{2n+1})
    /// (3) c_{n+1} = 1
    /// (4) c_{2n+1} = c_n
    c_poly: Vec<E::Fr>,

    // a_comm: PE::Commitment,
    c_comm: PE::Commitment,

    /// c_n^{-1}
    c_n_inv: E::Fr,

    n: usize,
}

impl<E: Engine, PE: PolyEngine<Pairing=E>> gprodArg<E, PE> {
    pub fn commit_t_poly(
        &mut self,
        y: E::Fr, // challenge
        srs: &SRS<E>
    ) -> Result<(Vec<E::Fr>, PE::Commitment), SynthesisError> {
        let n = self.n;

        let mut a_xy = self.a_poly.clone();

        // r(X, y) + s(X, y) with powers [1, 2n+2]
        let r_plus_s = {

            // (y * \sum\limits_{i=1}^{2n+1} a_i y^i) * X^i
            let mut tmp = y;
            tmp.square();
            eval_bivar_poly::<E>(&mut a_xy[..], tmp, y);

            // (x_n^{-1}*y^{n+2} + y) * X^{n+1}
            let y_n_plus_two = y.pow(&[(n+2) as u64]);
            let mut x_n_plus_one = self.c_n_inv;
            x_n_plus_one.mul_assign(&y_n_plus_two);
            x_n_plus_one.add_assign(&y);
            a_xy[n].add_assign(&x_n_plus_one);

            // 1 * X^{n+2}
            a_xy[n+1].add_assign(&E::Fr::one());

            // (-y) * X^{2n+2}
            let mut neg_y = y;
            neg_y.negate();
            a_xy.push(neg_y);

            // Padding for negative powers
            let mut a_prime = vec![E::Fr::zero(); 2 * n + 3];
            a_prime.extend(a_xy);

            a_prime
        };

        // r'(X, y) with powers [-2n-3, -1]
        let r_prime = {
            let mut cx = self.c_poly.iter().rev().map(|e| *e).collect::<Vec<E::Fr>>();
            cx.push(E::Fr::one());
            cx.push(E::Fr::zero());

            cx
        };

        let mut t_xy = mul_polynomials::<E>(&r_plus_s, &r_prime)?;

        // (4n+5) + (2n+3) - 1
        assert_eq!(t_xy.len(), 6 * n + 7);

        // Remove the first powers due to the padding.
        t_xy.drain(0..(2*n+3));
        let last = t_xy.pop();
        assert_eq!(last.unwrap(), E::Fr::zero(), "last element should be zero");
        assert_eq!(t_xy.len(), 4 * n + 3);

        // k(y)
        let mut k_y = {
            let mut y_sq = y;
            y_sq.square();
            eval_univar_poly::<E>(&self.c_poly[..], y_sq, y)
        };
        k_y.add_assign(&E::Fr::one());

        // (r(X, y) + s(X, y))r'(X, y) - k(y)
        t_xy[2 * n + 1].sub_assign(&k_y);

        // mul_add_poly::<E>(&mut self.t_polys[..], &t_xy, *challenge);

        let t_comm = multiexp(
            srs.g_neg_x_alpha[..(2*n+1)].iter().rev()
                .chain_ext(srs.g_pos_x_alpha[..(2*n+1)].iter()),
            t_xy[..(2*n+1)].iter()
                .chain_ext(t_xy[(2*n+2)..].iter())
        ).into_affine();

        Ok((t_xy, PE::Commitment::from_point(&t_comm)))
    }
}



//     pub fn open(&self, y: E::Fr, z: E::Fr, srs: &SRS<E>) -> Vec<(E::Fr, E::G1Affine)> {
//         let n = self.n;
//         let mut yz = y;
//         yz.mul_assign(&z);

//         let mut acc = vec![];

//         for a_poly in self.a_polys.iter() {
//             let u = &a_poly[..n];
//             let v = &a_poly[(n+1)..];
//             assert_eq!(u.len(), v.len());

//             let mut val = eval_univar_poly::<E>(u, yz, yz);
//             let fp = yz.pow([(n+2) as u64]);
//             let val_v = eval_univar_poly::<E>(v, fp, yz);
//             val.add_assign(&val_v);

//             let mut constant_term = val;
//             constant_term.negate();

//             let opening = poly_comm_opening(
//                 0,
//                 2 * n + 1,
//                 srs,
//                 Some(constant_term).iter() // f(x)-f(yz)
//                     .chain_ext(u.iter())
//                     .chain_ext(Some(E::Fr::zero()).iter())
//                     .chain_ext(v.iter()),
//                 yz,
//             );

//             acc.push((val, opening));
//         }

//         acc
//     }

pub fn create_gprod_proof<E: Engine, PE: PolyEngine<Pairing=E>>(
    polys: &ProductPolys<E>,
    srs: &SRS<E>
) -> Result<GrandProductProof<E>, SynthesisError> {
    let mut transcript = Transcript::new(&[]);

    // gprodP_1
    let mut args = polys.gen_arg::<PE>(srs);
    transcript.commit_point::<PE>(&args.c_comm);

    let n = args.n;

    // gprodV -> gprodP:
    let y: E::Fr = transcript.challenge_scalar();

    // gprod_2(y) -> T:
    let (mut t_xy, t_comm) = args.commit_t_poly(y, srs)?;
    transcript.commit_point::<PE>(&t_comm);

    // gprodV -> gprodP:
    let z: E::Fr = transcript.challenge_scalar();
    let z_inv = z.inverse().ok_or(SynthesisError::DivisionByZero)?;

    let mut yz = y;
    yz.mul_assign(&z);

    // gprod_3(z) -> T:
    let mut c_z_inv = eval_univar_poly::<E>(&args.c_poly[..], z_inv, z_inv);
    c_z_inv.negate();

    let c_opening = poly_comm_opening(
        0,
        2 * n + 1,
        srs,
        Some(c_z_inv).iter()
            .chain_ext(args.c_poly.iter()),
        z_inv
    );
    c_z_inv.negate();

    let mut k_y = eval_univar_poly::<E>(&args.c_poly[..], y, y);
    k_y.negate();

    let k_opening = poly_comm_opening(
        0,
        2 * n + 1,
        srs,
        Some(k_y).iter()
            .chain_ext(args.c_poly.iter()),
        y
    );
    k_y.negate();

    let t_zy = {
        let first_power = z_inv.pow([(2 * n + 1) as u64]);
        eval_univar_poly::<E>(&t_xy, first_power, z)
    };
    t_xy[2 * n + 1].sub_assign(&t_zy);

    let t_opening = poly_comm_opening(
        2 * n + 1,
        2 * n + 1,
        srs,
        t_xy.iter(),
        z
    );

    Ok(GrandProductProof::<E> {
        // a_yz: E::Fr,
        // a_opening: E::G1Affine,
        c_z_inv,
        c_opening,
        k_y,
        k_opening,
        t_opening,
    })
}

// #[derive(Clone)]
// pub struct CPoly<E: Engine>(Vec<Vec<E::Fr>>);

// impl<E: Engine> CPoly<E> {
//     pub fn new(polys: Vec<(Vec<E::Fr>, Vec<E::Fr>)>) -> Self {

//         unimplemented!();
//     }

//     pub fn commit(&self, srs: &SRS<E>) -> CPolyComm<E> {
//         let mut res = vec![];
//         let n = self.0.len();

//         for ()
//         unimplemented!();
//     }
// }

#[derive(Clone)]
pub struct GrandProductProof<E: Engine> {
    // a_yz: E::Fr,
    // a_opening: E::G1Affine,
    c_z_inv: E::Fr,
    c_opening: E::G1Affine,
    k_y: E::Fr,
    k_opening: E::G1Affine,
    t_opening: E::G1Affine,
}

impl<E: Engine> GrandProductProof<E> {
    pub fn verify(
        &self,
        n: usize,
        randomness: &Vec<E::Fr>,
        t_commitment: E::G1Affine,
        c_commitments: &Vec<(E::G1Affine, E::Fr)>,
        y: E::Fr,
        z: E::Fr,
        srs: &SRS<E>
    ) -> bool {

        // Re-calculate t(z, y)

        //

        unimplemented!();
    }
}
