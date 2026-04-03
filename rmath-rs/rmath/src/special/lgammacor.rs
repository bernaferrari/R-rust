// Ported from R's nmath/lgammacor.c
// Compute the log gamma correction factor for x >= 10
//
// This routine is a translation into C of a Fortran subroutine
// written by W. Fullerton of Los Alamos Scientific Laboratory.

use crate::error::*;
use crate::special::chebyshev::chebyshev_eval;

const ALGMCS: [f64; 15] = [
    // below, nalgm = 5 ==> only the first 5 are used!
    0.1666389480451863247205729650822e+0,
    -0.1384948176067563840732986059135e-4,
    0.9810825646924729426157171547487e-8,
    -0.1809129475572494194263306266719e-10,
    0.6221098041892605227126015543416e-13,
    -0.3399615005417721944303330599666e-15,
    0.2683181998482698748957538846666e-17,
    -0.2868042435334643284144622399999e-19,
    0.3962837061046434803679306666666e-21,
    -0.6831888753985766870111999999999e-23,
    0.1429227355942498147573333333333e-24,
    -0.3547598158101070547199999999999e-26,
    0.1025680058010470912000000000000e-27,
    -0.3401102254316748799999999999999e-29,
    0.1276642195630062933333333333333e-30,
];

// NB: -- we'd need nalgm = 6 terms for full precision, but the result is
//      always used in +/- terms of considerably larger size ~ x*log(x)
// (we could even *decrease* nalgm for larger y)
const NALGM: i32 = 5;

// For IEEE double precision DBL_EPSILON = 2^-52 = 2.220446049250313e-16 :
//   xbig = 2 ^ 26.5
const XBIG: f64 = 94906265.62425156;

/// Compute the log gamma correction factor for x >= 10 so that
///
///   log(gamma(x)) = .5*log(2*pi) + (x-.5)*log(x) - x + lgammacor(x)
///
/// [ lgammacor(x) is called Del(x) in other contexts (e.g. dcdflib), or stirlerr(x) ]
pub(crate) fn lgammacor(x: f64) -> f64 {
    if x < 10.0 {
        // possibly consider stirlerr()
        return ml_warn_return_nan();
    }

    // IEEE 754 is always assumed in Rust
    if x < XBIG {
        let tmp: f64 = 10.0 / x;
        return chebyshev_eval(tmp * tmp * 2.0 - 1.0, &ALGMCS, NALGM) / x;
    }
    // x >= xbig
    1.0 / (x * 12.0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_lgammacor(x: f64) -> f64 {
    lgammacor(x)
}
