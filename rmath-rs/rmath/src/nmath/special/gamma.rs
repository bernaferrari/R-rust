// Ported from R's nmath/gamma.c and nmath/pgamma.c (lgamma1p, log1pmx, logcf)
//
// gamma.c:
//   Original by W. Fullerton of Los Alamos Scientific Laboratory.
//   (e.g. http://www.netlib.org/slatec/fnlib/gamma.f)
//   MM specialized the case of n! for n < 50 - for even better precision
//
// lgamma1p, log1pmx, logcf:
//   Original by Catherine Loader, Morten Welinder

use crate::nmath::constants::*;
use crate::nmath::error::*;
use crate::nmath::special::chebyshev::chebyshev_eval;
use crate::nmath::special::cospi::sinpi;
use crate::nmath::special::lgammacor::lgammacor;
use crate::nmath::special::stirlerr::stirlerr;
use libm::*;

const GAMCS: [f64; 42] = [
    0.8571195590989331421920062399942e-2,
    0.4415381324841006757191315771652e-2,
    0.5685043681599363378632664588789e-1,
    -0.4219835396418560501012500186624e-2,
    0.1326808181212460220584006796352e-2,
    -0.1893024529798880432523947023886e-3,
    0.3606925327441245256578082217225e-4,
    -0.6056761904460864218485548290365e-5,
    0.1055829546302283344731823509093e-5,
    -0.1811967365542384048291855891166e-6,
    0.3117724964715322277790254593169e-7,
    -0.5354219639019687140874081024347e-8,
    0.9193275519859588946887786825940e-9,
    -0.1577941280288339761767423273953e-9,
    0.2707980622934954543266540433089e-10,
    -0.4646818653825730144081661058933e-11,
    0.7973350192007419656460767175359e-12,
    -0.1368078209830916025799499172309e-12,
    0.2347319486563800657233471771688e-13,
    -0.4027432614949066932766570534699e-14,
    0.6910051747372100912138336975257e-15,
    -0.1185584500221992907052387126192e-15,
    0.2034148542496373955201026051932e-16,
    -0.3490054341717405849274012949108e-17,
    0.5987993856485305567135051066026e-18,
    -0.1027378057872228074490069778431e-18,
    0.1762702816060529824942759660748e-19,
    -0.3024320653735306260958772112042e-20,
    0.5188914660218397839717833550506e-21,
    -0.8902770842456576692449251601066e-22,
    0.1527474068493342602274596891306e-22,
    -0.2620731256187362900257328332799e-23,
    0.4496464047830538670331046570666e-24,
    -0.7714712731336877911703901525333e-25,
    0.1323635453126044036486572714666e-25,
    -0.2270999412942928816702313813333e-26,
    0.3896418998003991449320816639999e-27,
    -0.6685198115125953327792127999999e-28,
    0.1146998663140024384347613866666e-28,
    -0.1967938586345134677295103999999e-29,
    0.3376448816585338090334890666666e-30,
    -0.5793070335782135784625493333333e-31,
];

// For IEEE double precision DBL_EPSILON = 2^-52 = 2.220446049250313e-16 :
// (xmin, xmax) are non-trivial, see ./gammalims.c
// xsml = exp(.01)*DBL_MIN
// dxrel = sqrt(DBL_EPSILON) = 2 ^ -26
const NGAM: i32 = 22;
const XMIN: f64 = -170.5674972726612;
const XMAX: f64 = 171.61447887182298;
const XSML: f64 = 2.2474362225598545e-308;
const DXREL: f64 = 1.490116119384765696e-8;

const M_LN_SQRT_2PI: f64 = 0.918938533204672741780329736406; // log(sqrt(2*pi))

// =====================================================================
// lgamma1p support functions (from pgamma.c)
// =====================================================================

/// Continued fraction for calculation of
///   1/i + x/(i+d) + x^2/(i+2*d) + x^3/(i+3*d) + ...
/// auxiliary in log1pmx() and lgamma1p()
///
/// Also used by dist/gamma.rs — keep this as the single source of truth.
#[inline(always)]
pub(crate) fn logcf(x: f64, i: f64, d: f64, eps: f64) -> f64 {
    let scalefactor: f64 = {
        // (2^32)^8 = 2^256
        let s1: f64 = 4294967296.0;
        let s2 = s1 * s1;
        let s3 = s2 * s2;
        s3 * s3
    };

    let mut c1: f64 = 2.0 * d;
    let mut c2: f64 = i + d;
    let mut c4: f64 = c2 + d;
    let mut a1: f64 = c2;
    let mut b1: f64 = i * (c2 - i * x);
    let mut b2: f64 = d * d * x;
    let mut a2: f64 = c4 * c2 - b2;

    b2 = c4 * b1 - i * b2;

    let mut iterations = 0;
    const MAX_ITER_LOGCF: i32 = 10000;
    while fabs(a2 * b1 - a1 * b2) > fabs(eps * b1 * b2) {
        iterations += 1;
        if iterations > MAX_ITER_LOGCF {
            break;
        }
        let c3: f64 = c2 * c2 * x;
        c2 += d;
        c4 += d;
        a1 = c4 * a2 - c3 * a1;
        b1 = c4 * b2 - c3 * b1;

        let c3: f64 = c1 * c1 * x;
        c1 += d;
        c4 += d;
        a2 = c4 * a1 - c3 * a2;
        b2 = c4 * b1 - c3 * b2;

        if fabs(b2) > scalefactor {
            a1 /= scalefactor;
            b1 /= scalefactor;
            a2 /= scalefactor;
            b2 /= scalefactor;
        } else if fabs(b2) < 1.0 / scalefactor {
            a1 *= scalefactor;
            b1 *= scalefactor;
            a2 *= scalefactor;
            b2 *= scalefactor;
        }
    }

    a2 / b2
}

/// Accurate calculation of log(1+x)-x, particularly for small x.
#[inline]
pub(crate) fn log1pmx(x: f64) -> f64 {
    const MIN_LOG1_VALUE: f64 = -0.79149064;

    if x > 1.0 || x < MIN_LOG1_VALUE {
        log1p(x) - x
    } else {
        // -.791 <= x <= 1 -- expand in [x/(2+x)]^2 =: y :
        // log(1+x) - x = x/(2+x) * [ 2 * y * S(y) - x], with
        // S(y) = 1/3 + y/5 + y^2/7 + ... = sum_{k=0}^inf y^k / (2k + 3)
        let r = x / (2.0 + x);
        let y = r * r;
        if fabs(x) < 1e-2 {
            const TWO: f64 = 2.0;
            r * ((((TWO / 9.0 * y + TWO / 7.0) * y + TWO / 5.0) * y + TWO / 3.0) * y - x)
        } else {
            const TOL_LOGCF: f64 = 1e-14;
            r * (2.0 * y * logcf(y, 3.0, 2.0, TOL_LOGCF) - x)
        }
    }
}

/// Compute log(gamma(a+1)) accurately also for small a (0 < a < 0.5).
pub fn lgammafn1p(a: f64) -> f64 {
    if fabs(a) >= 0.5 {
        return lgammafn(a + 1.0);
    }

    const EULERS_CONST: f64 = 0.5772156649015328606065120900824024;

    // coeffs[i] holds (zeta(i+2)-1)/(i+2), i = 0:(N-1), N = 40
    const N: usize = 40;
    const COEFFS: [f64; 40] = [
        0.3224670334241132182362075833230126e-0, // (zeta(2)-1)/2
        0.6735230105319809513324605383715000e-1, // (zeta(3)-1)/3
        0.2058080842778454787900092413529198e-1,
        0.7385551028673985266273097291406834e-2,
        0.2890510330741523285752988298486755e-2,
        0.1192753911703260977113935692828109e-2,
        0.5096695247430424223356548135815582e-3,
        0.2231547584535793797614188036013401e-3,
        0.9945751278180853371459589003190170e-4,
        0.4492623673813314170020750240635786e-4,
        0.2050721277567069155316650397830591e-4,
        0.9439488275268395903987425104415055e-5,
        0.4374866789907487804181793223952411e-5,
        0.2039215753801366236781900709670839e-5,
        0.9551412130407419832857179772951265e-6,
        0.4492469198764566043294290331193655e-6,
        0.2120718480555466586923135901077628e-6,
        0.1004322482396809960872083050053344e-6,
        0.4769810169363980565760193417246730e-7,
        0.2271109460894316491031998116062124e-7,
        0.1083865921489695409107491757968159e-7,
        0.5183475041970046655121248647057669e-8,
        0.2483674543802478317185008663991718e-8,
        0.1192140140586091207442548202774640e-8,
        0.5731367241678862013330194857961011e-9,
        0.2759522885124233145178149692816341e-9,
        0.1330476437424448948149715720858008e-9,
        0.6422964563838100022082448087644648e-10,
        0.3104424774732227276239215783404066e-10,
        0.1502138408075414217093301048780668e-10,
        0.7275974480239079662504549924814047e-11,
        0.3527742476575915083615072228655483e-11,
        0.1711991790559617908601084114443031e-11,
        0.8315385841420284819798357793954418e-12,
        0.4042200525289440065536008957032896e-12,
        0.1966475631096616490411045679010286e-12,
        0.9573630387838555763782200936508615e-13,
        0.4664076026428374224576492565974577e-13,
        0.2273736960065972320633279596737272e-13,
        0.1109139947083452201658320007192334e-13, // (zeta(40+1)-1)/(40+1)
    ];

    const C: f64 = 0.2273736845824652515226821577978691e-12; // zeta(N+2)-1
    const TOL_LOGCF: f64 = 1e-14;

    // Abramowitz & Stegun 6.1.33: for |x| < 2,
    // log(gamma(1+x)) = -(log(1+x) - x) - gamma*x + x^2 * sum_{n=0}^inf c_n (-x)^n
    // where c_n := (Zeta(n+2) - 1)/(n+2) = coeffs[n]
    let mut lgam = C * logcf(-a / 2.0, (N + 2) as f64, 1.0, TOL_LOGCF);
    let mut idx = N - 1;
    loop {
        lgam = COEFFS[idx] - a * lgam;
        if idx == 0 {
            break;
        }
        idx -= 1;
    }

    (a * lgam - EULERS_CONST) * a - log1pmx(a)
}

// =====================================================================
// gammafn
// =====================================================================

/// This function computes the value of the gamma function.
///
/// This function is a translation into C of a Fortran subroutine
/// by W. Fullerton of Los Alamos Scientific Laboratory.
/// (e.g. http://www.netlib.org/slatec/fnlib/gamma.f)
///
/// The accuracy of this routine compares (very) favourably
/// with those of the Sun Microsystems portable mathematical library.
///
/// MM specialized the case of n! for n < 50 - for even better precision
pub fn gammafn(x: f64) -> f64 {
    if isnan(x) {
        return x;
    }

    // If the argument is exactly zero or a negative integer
    // then return NaN.
    if x == 0.0 || (x < 0.0 && x == round(x)) {
        ml_warning(ME_DOMAIN, "gammafn");
        return ML_NAN;
    }

    let y = fabs(x);
    let mut value: f64;

    if y <= 10.0 {
        // Compute gamma(x) for -10 <= x <= 10
        // Reduce the interval and find gamma(1 + y) for 0 <= y < 1
        // first of all.
        let mut n = x as i32;
        if x < 0.0 {
            n -= 1;
        }
        let y_loc = x - (n as f64); // n = floor(x) ==> y in [0, 1)
        n -= 1;
        value = chebyshev_eval(y_loc * 2.0 - 1.0, &GAMCS, NGAM) + 0.9375;
        if n == 0 {
            return value; // x = 1.dddd = 1+y
        }

        if n < 0 {
            // compute gamma(x) for -10 <= x < 1
            // exact 0 or "-n" checked already above

            // The answer is less than half precision
            // because x too near a negative integer.
            if x < -0.5 && fabs(x - ((x - 0.5) as i32) as f64 / x) < DXREL {
                ml_warning(ME_PRECISION, "gammafn");
            }

            // The argument is so close to 0 that the result would overflow.
            if y_loc < XSML {
                ml_warning(ME_RANGE, "gammafn");
                if x > 0.0 {
                    return ML_POSINF;
                } else {
                    return ML_NEGINF;
                }
            }

            let n_pos = -n;
            for i in 0..n_pos {
                value /= x + i as f64;
            }
            return value;
        } else {
            // gamma(x) for 2 <= x <= 10
            for i in 1..=n {
                value *= y_loc + i as f64;
            }
            return value;
        }
    } else {
        // gamma(x) for y = |x| > 10.

        if x > XMAX {
            // Overflow
            // No warning: +Inf is the best answer
            return ML_POSINF;
        }

        if x < XMIN {
            // Underflow
            // No warning: 0 is the best answer
            return 0.0;
        }

        if y <= 50.0 && y == (y as i32) as f64 {
            // compute (n - 1)!
            value = 1.0;
            let mut i: i32 = 2;
            while i < (y as i32) {
                value *= i as f64;
                i += 1;
            }
        } else {
            // normal case
            if 2.0 * y == ((2.0 * y) as i32) as f64 {
                value = exp((y - 0.5) * log(y) - y + M_LN_SQRT_2PI + stirlerr(y));
            } else {
                value = exp((y - 0.5) * log(y) - y + M_LN_SQRT_2PI + lgammacor(y));
            }
        }

        if x > 0.0 {
            return value;
        }
        // else: x < 0, not an integer :

        if fabs((x - ((x - 0.5) as i32) as f64) / x) < DXREL {
            // The answer is less than half precision because
            // the argument is too near a negative integer.
            ml_warning(ME_PRECISION, "gammafn");
        }

        let sinpiy = sinpi(y);
        if sinpiy == 0.0 {
            // Negative integer arg - overflow
            ml_warning(ME_RANGE, "gammafn");
            return ML_POSINF;
        }

        -(std::f64::consts::PI) / (y * sinpiy * value)
    }
}

/// C FFI wrapper for gammafn
pub extern "C" fn gammafn_c(x: f64) -> f64 {
    gammafn(x)
}

/// C FFI wrapper for lgammafn1p
pub extern "C" fn lgammafn1p_c(a: f64) -> f64 {
    lgammafn1p(a)
}

// =====================================================================
// lgammafn (from lgamma.c)
// =====================================================================

// For IEEE double precision:
// xmax = DBL_MAX / log(DBL_MAX) = 2^1024 / (1024 * log(2)) = 2^1014 / log(2)
// dxrel = sqrt(DBL_EPSILON) = 2^-26
const LG_XMAX: f64 = 2.5327372760800758e+305;
const LG_DXREL: f64 = 1.490116119384765625e-8;
const M_LN_SQRT_PId2: f64 = 0.225791352644727432363097614947; // log(sqrt(pi/2))

/// Compute log|gamma(x)|.
pub fn lgammafn(x: f64) -> f64 {
    if isnan(x) {
        return x;
    }

    if x <= 0.0 && x == trunc(x) {
        // Negative integer argument
        // No warning: this is the best answer
        return ML_POSINF; // +Inf, since lgamma(x) = log|gamma(x)|
    }

    let y = fabs(x);

    if y < 1e-306 {
        return -log(y); // denormalized range, R change
    }
    if y <= 10.0 {
        return log(fabs(gammafn(x)));
    }
    // ELSE y = |x| > 10

    if y > LG_XMAX {
        // No warning: +Inf is the best answer
        return ML_POSINF;
    }

    if x > 0.0 {
        // i.e. y = x > 10
        if x > 1e17 {
            return x * (log(x) - 1.0);
        } else if x > 4934720.0 {
            return M_LN_SQRT_2PI + (x - 0.5) * log(x) - x;
        } else {
            return M_LN_SQRT_2PI + (x - 0.5) * log(x) - x + lgammacor(x);
        }
    }
    // else: x < -10; y = -x > 10
    let sinpiy = fabs(sinpi(y));

    if sinpiy == 0.0 {
        // Negative integer argument === Now UNNECESSARY: caught above
        return f64::NAN;
    }

    let ans = M_LN_SQRT_PId2 + (x - 0.5) * log(y) - x - log(sinpiy) - lgammacor(y);

    if fabs((x - trunc(x - 0.5)) * ans / x) < LG_DXREL {
        // The answer is less than half precision because
        // the argument is too near a negative integer
        ml_warning(ME_PRECISION, "lgamma");
    }

    ans
}

/// C FFI wrapper for lgammafn
pub extern "C" fn lgammafn_c(x: f64) -> f64 {
    lgammafn(x)
}

/// Compute log|gamma(x)| and optionally the sign of gamma(x).
/// Ported from R's lgammafn_sign() in lgamma.c.
///
/// If `sgn` is non-null, stores the sign of gamma(x) there.
/// Returns log|gamma(x)|.
pub fn lgammafn_sign(x: f64, sgn: Option<&mut i32>) -> f64 {
    if isnan(x) {
        return x;
    }

    // Determine sign before we might move sgn
    let neg_sign = x < 0.0 && libm::floor(-x) % 2.0 == 0.0;

    if let Some(s) = sgn {
        *s = if neg_sign { -1 } else { 1 };
    }

    if x <= 0.0 && x == trunc(x) {
        return ML_POSINF;
    }

    lgammafn(x)
}

// =====================================================================
// Rf_ prefixed FFI shims (R-compatible symbol names)
// =====================================================================

pub extern "C" fn Rf_gammafn(x: f64) -> f64 {
    gammafn(x)
}

pub extern "C" fn Rf_lgammafn(x: f64) -> f64 {
    lgammafn(x)
}

pub extern "C" fn Rf_lgammafn1p(a: f64) -> f64 {
    lgammafn1p(a)
}

/// C FFI for lgammafn_sign: returns log|gamma(x)|, stores sign in *sgn if sgn is non-null.
pub extern "C" fn Rf_lgammafn_sign(x: f64, sgn: *mut i32) -> f64 {
    let sgn_opt = if sgn.is_null() {
        None
    } else {
        Some(unsafe { &mut *sgn })
    };
    lgammafn_sign(x, sgn_opt)
}
