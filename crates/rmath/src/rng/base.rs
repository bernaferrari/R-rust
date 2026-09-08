// Standalone RNG: Marsaglia-MultiCarry
// Ported from standalone/sunif.c, with state owned by the active RInstance.
//
// Since the nmath split, the generator itself lives in `rmath-nmath`
// (`rmath_nmath::rng`); this facade delegates so the R-level stream and the
// nmath samplers keep sharing one session-owned Marsaglia-MultiCarry state.

pub use rmath_nmath::rng::{Rf_set_seed, Rf_unif_rand, get_seed, set_seed, unif_rand};
