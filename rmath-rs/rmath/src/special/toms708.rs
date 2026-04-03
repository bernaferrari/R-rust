#![allow(unused_assignments)]
// Based on C translation of ACM TOMS 708
// Please do not change this, e.g. to use R's versions of the
// ancillary routines, without investigating the error analysis as we
// do need very high relative accuracy.  This version has about
// 14 digits accuracy.
//
// More specifically,  Brown & Levy (1994) "Certification of Algorithm 708" write
// "
//    The number of significant digits of accuracy [..] was calculated [..] as
//
//                       - log10 (2 RelativeError),
//    [....]
//    Accuracy ranged from 9.64 significant digits to 15.65 with a
//    median of 14.65 and a lower quartile of 13.81.
//    [...]
//    ... overall accuracy increases slightly as a/b moves away from 1.
//        Linear regression indicates that
//    (1) an average of 13.71 significant digits are obtained in cases in which a = b and
//    (2) the number increases 0.14 significant digits for each unit change in log10(a/b).
//   "
//
//      ALGORITHM 708, COLLECTED ALGORITHMS FROM ACM.
//      This work published in  Transactions On Mathematical Software,
//      vol. 18, no. 3, September 1992, pp. 360-373.
//
// Changes by R Core Team :
// add log_p  and work towards gaining precision in that case;
// work for very large (but finite) {a,b}

use crate::constants::*;
use crate::dpq::*;
use crate::utils::*;
use libm::*;
// Note: gammafn/lgammafn are available from crate::special::gamma if needed in future
// but the TOMS 708 implementation uses its own gamln/gamln1 internally.

const DBL_MIN: f64 = 2.2250738585072014e-308;
const DBL_MAX: f64 = 1.7976931348623157e+308;
const DBL_EPSILON: f64 = 2.220446049250313e-16;
const INT_MAX_I32: i32 = 2147483647;
const M_LN_SQRT_2PI: f64 = 0.918938533204672741780329736406;
const M_SQRT_PI: f64 = 1.772453850905516027298167483341;
const M_LN2: f64 = 0.693147180559945309417232121458;

// R_Log1_Exp(x)
fn r_log1_exp(x: f64) -> f64 {
    if x > -M_LN2 {
        log(-expm1(x))
    } else {
        log1p(-exp(x))
    }
}

// logspace_add(logx, logy) = log(exp(logx) + exp(logy))
#[inline]
fn logspace_add(logx: f64, logy: f64) -> f64 {
    fmax2(logx, logy) + log1p(exp(-fabs(logx - logy)))
}

// R_D_exp(z) when log_p is known: if log_p { z } else { exp(z) }
// We'll pass log_p explicitly

/// Main entry point for the incomplete beta function.
///
/// Evaluates I_x(a,b) and 1 - I_x(a,b).
///
/// # Returns
/// (w, w1, ierr) where:
/// - w = I_x(a,b)  (or log(I_x(a,b)) when log_p is true)
/// - w1 = 1 - I_x(a,b)  (or log(1-I_x(a,b)) when log_p is true)
/// - ierr: 0 = success, nonzero = error code
pub fn bratio(a: f64, b: f64, x: f64, y: f64, log_p: bool) -> (f64, f64, i32) {
    let mut ierr: i32 = 0;
    let mut ierr1: i32 = 0;
    let mut w: f64 = 0.0;
    let mut w1: f64 = 0.0;

    // eps is a machine dependent constant: the smallest
    // floating point number for which   1. + eps > 1.
    // NOTE: for almost all purposes it is replaced by 1e-15 (~= 4.5 times larger) below
    let mut eps = 2.0 * DBL_EPSILON;

    w = if log_p { ML_NEGINF } else { 0.0 };
    w1 = if log_p { ML_NEGINF } else { 0.0 };

    // IEEE_754: safeguard, preventing infinite loops further down
    if x.is_nan() || y.is_nan() || a.is_nan() || b.is_nan() {
        ierr = 9;
        return (w, w1, ierr);
    }
    if a < 0.0 || b < 0.0 {
        ierr = 1;
        return (w, w1, ierr);
    }
    if a == 0.0 && b == 0.0 {
        ierr = 2;
        return (w, w1, ierr);
    }
    if x < 0.0 || x > 1.0 {
        ierr = 3;
        return (w, w1, ierr);
    }
    if y < 0.0 || y > 1.0 {
        ierr = 4;
        return (w, w1, ierr);
    }

    // check that  'y == 1 - x' :
    let z = x + y - 0.5 - 0.5;

    if fabs(z) > eps * 3.0 {
        ierr = 5;
        return (w, w1, ierr);
    }

    ierr = 0;

    // L200:
    if x == 0.0 {
        if a == 0.0 {
            ierr = 6;
            return (w, w1, ierr);
        }
        // L201:
        w = if log_p { ML_NEGINF } else { 0.0 };
        w1 = if log_p { 0.0 } else { 1.0 };
        return (w, w1, ierr);
    }

    // L210:
    if y == 0.0 {
        if b == 0.0 {
            ierr = 7;
            return (w, w1, ierr);
        }
        // L211:
        w = if log_p { 0.0 } else { 1.0 };
        w1 = if log_p { ML_NEGINF } else { 0.0 };
        return (w, w1, ierr);
    }

    if a == 0.0 {
        // L211:
        w = if log_p { 0.0 } else { 1.0 };
        w1 = if log_p { ML_NEGINF } else { 0.0 };
        return (w, w1, ierr);
    }
    if b == 0.0 {
        // L201:
        w = if log_p { ML_NEGINF } else { 0.0 };
        w1 = if log_p { 0.0 } else { 1.0 };
        return (w, w1, ierr);
    }

    eps = if eps > 1e-15 { eps } else { 1e-15 };
    let a_lt_b = a < b;

    if (if a_lt_b { b } else { a }) < eps * 0.001 {
        // L230: result *independent* of x (!)
        // w = a/(a+b) and w1 = b/(a+b):
        if log_p {
            if a_lt_b {
                w = log1p(-a / (a + b));
                w1 = log(a / (a + b));
            } else {
                w = log(b / (a + b));
                w1 = log1p(-b / (a + b));
            }
        } else {
            w = b / (a + b);
            w1 = a / (a + b);
        }
        return (w, w1, ierr);
    }

    let do_swap: bool;
    let mut a0: f64;
    let mut b0: f64;
    let x0: f64;
    let y0: f64;
    let mut lambda: f64;
    let mut n: i32 = 0;

    if a.min(b) <= 1.0 {
        // min(a,b) <= 1
        do_swap = x > 0.5;
        if do_swap {
            a0 = b;
            x0 = y;
            b0 = a;
            y0 = x;
        } else {
            a0 = a;
            x0 = x;
            b0 = b;
            y0 = y;
        }
        // now have  x0 <= 1/2 <= y0  (still  x0+y0 == 1)

        // L80:
        if b0 < eps.min(eps * a0) {
            w = fpser(a0, b0, x0, eps, log_p);
            w1 = if log_p { r_log1_exp(w) } else { 0.5 - w + 0.5 };
            // L_end:
            if do_swap {
                let t = w;
                w = w1;
                w1 = t;
            }
            return (w, w1, ierr);
        }

        // L90:
        if a0 < eps.min(eps * b0) && b0 * x0 <= 1.0 {
            w1 = apser(a0, b0, x0, eps);
            // L_end_from_w1:
            if log_p {
                w = log1p(-w1);
                w1 = log(w1);
            } else {
                w = 0.5 - w1 + 0.5;
            }
            // L_end:
            if do_swap {
                let t = w;
                w = w1;
                w1 = t;
            }
            return (w, w1, ierr);
        }

        let mut did_bup = false;
        if a0.max(b0) > 1.0 {
            // L20: min(a,b) <= 1 < max(a,b)
            if b0 <= 1.0 {
                // L_w_bpser:
                w = bpser(a0, b0, x0, eps, log_p);
                w1 = if log_p { r_log1_exp(w) } else { 0.5 - w + 0.5 };
                // L_end:
                if do_swap {
                    let t = w;
                    w = w1;
                    w1 = t;
                }
                return (w, w1, ierr);
            }

            if x0 >= 0.29 {
                // L_w1_bpser:
                w1 = bpser(b0, a0, y0, eps, log_p);
                w = if log_p {
                    r_log1_exp(w1)
                } else {
                    0.5 - w1 + 0.5
                };
                // L_end:
                if do_swap {
                    let t = w;
                    w = w1;
                    w1 = t;
                }
                return (w, w1, ierr);
            }

            if x0 < 0.1 && pow(x0 * b0, a0) <= 0.7 {
                // L_w_bpser:
                w = bpser(a0, b0, x0, eps, log_p);
                w1 = if log_p { r_log1_exp(w) } else { 0.5 - w + 0.5 };
                // L_end:
                if do_swap {
                    let t = w;
                    w = w1;
                    w1 = t;
                }
                return (w, w1, ierr);
            }

            if b0 > 15.0 {
                w1 = 0.0;
                // goto L131
            } else {
                // L20: b0 <= 15, need bup + bgrat
                // This branch handles min(a,b)<=1 < max(a,b), b0 > 1, x0 < 0.29, b0 <= 15
                // Fall through to the bup+bgrat path below
                n = 20;
                w1 = bup(b0, a0, y0, x0, n, eps, false);
                did_bup = true;
                b0 += n as f64;
                // L131:
                bgrat(b0, a0, y0, x0, &mut w1, 15.0 * eps, &mut ierr1, false);
                if w1 == 0.0 || (0.0 < w1 && w1 < DBL_MIN) {
                    // denormalized or underflow (?) -> retrying
                    if did_bup {
                        w1 = bup(b0 - n as f64, a0, y0, x0, n, eps, true);
                    } else {
                        w1 = ML_NEGINF;
                    }
                    bgrat(b0, a0, y0, x0, &mut w1, 15.0 * eps, &mut ierr1, true);
                    if ierr1 != 0 {
                        ierr = 10 + ierr1;
                    }
                    // L_end_from_w1_log:
                    if log_p {
                        w = r_log1_exp(w1);
                    } else {
                        w = -expm1(w1);
                        w1 = exp(w1);
                    }
                    // L_end:
                    if do_swap {
                        let t = w;
                        w = w1;
                        w1 = t;
                    }
                    return (w, w1, ierr);
                }
                if ierr1 != 0 {
                    ierr = 10 + ierr1;
                }
                // L_end_from_w1:
                if log_p {
                    w = log1p(-w1);
                    w1 = log(w1);
                } else {
                    w = 0.5 - w1 + 0.5;
                }
                // L_end:
                if do_swap {
                    let t = w;
                    w = w1;
                    w1 = t;
                }
                return (w, w1, ierr);
            }
        } else {
            // a, b <= 1
            if a0 >= 0.2_f64.min(b0) {
                // L_w_bpser:
                w = bpser(a0, b0, x0, eps, log_p);
                w1 = if log_p { r_log1_exp(w) } else { 0.5 - w + 0.5 };
                // L_end:
                if do_swap {
                    let t = w;
                    w = w1;
                    w1 = t;
                }
                return (w, w1, ierr);
            }

            if pow(x0, a0) <= 0.9 {
                // L_w_bpser:
                w = bpser(a0, b0, x0, eps, log_p);
                w1 = if log_p { r_log1_exp(w) } else { 0.5 - w + 0.5 };
                // L_end:
                if do_swap {
                    let t = w;
                    w = w1;
                    w1 = t;
                }
                return (w, w1, ierr);
            }

            if x0 >= 0.3 {
                // L_w1_bpser:
                w1 = bpser(b0, a0, y0, eps, log_p);
                w = if log_p {
                    r_log1_exp(w1)
                } else {
                    0.5 - w1 + 0.5
                };
                // L_end:
                if do_swap {
                    let t = w;
                    w = w1;
                    w1 = t;
                }
                return (w, w1, ierr);
            }

            n = 20;
            w1 = bup(b0, a0, y0, x0, n, eps, false);
            did_bup = true;
            b0 += n as f64;
        }

        // L131:
        bgrat(b0, a0, y0, x0, &mut w1, 15.0 * eps, &mut ierr1, false);
        if w1 == 0.0 || (0.0 < w1 && w1 < DBL_MIN) {
            // denormalized or underflow (?) -> retrying
            if did_bup {
                w1 = bup(b0 - n as f64, a0, y0, x0, n, eps, true);
            } else {
                w1 = ML_NEGINF;
            }
            bgrat(b0, a0, y0, x0, &mut w1, 15.0 * eps, &mut ierr1, true);
            if ierr1 != 0 {
                ierr = 10 + ierr1;
            }
            // L_end_from_w1_log:
            if log_p {
                w = r_log1_exp(w1);
            } else {
                w = -expm1(w1);
                w1 = exp(w1);
            }
            // L_end:
            if do_swap {
                let t = w;
                w = w1;
                w1 = t;
            }
            return (w, w1, ierr);
        }
        if ierr1 != 0 {
            ierr = 10 + ierr1;
        }
        // L_end_from_w1:
        if log_p {
            w = log1p(-w1);
            w1 = log(w1);
        } else {
            w = 0.5 - w1 + 0.5;
        }
        // L_end:
        if do_swap {
            let t = w;
            w = w1;
            w1 = t;
        }
        return (w, w1, ierr);
    } else {
        // L30: both a, b > 1 {a0 > 1  &  b0 > 1}

        // lambda := a y - b x  =  (a + b)y - b  =  a - (a+b)x    {using x + y == 1},
        // using the numerically best version :
        lambda = if (a + b).is_finite() {
            if a > b {
                (a + b) * y - b
            } else {
                a - (a + b) * x
            }
        } else {
            a * y - b * x
        };

        do_swap = lambda < 0.0;
        if do_swap {
            lambda = -lambda;
            a0 = b;
            x0 = y;
            b0 = a;
            y0 = x;
        } else {
            a0 = a;
            x0 = x;
            b0 = b;
            y0 = y;
        }

        if b0 < 40.0 {
            if b0 * x0 <= 0.7 || (log_p && lambda > 650.0) {
                // L_w_bpser:
                w = bpser(a0, b0, x0, eps, log_p);
                w1 = if log_p { r_log1_exp(w) } else { 0.5 - w + 0.5 };
            } else {
                // L140:
                n = b0 as i32;
                b0 -= n as f64;
                if b0 == 0.0 {
                    n -= 1;
                    b0 = 1.0;
                }

                w = bup(b0, a0, y0, x0, n, eps, false);

                if w < DBL_MIN && log_p {
                    // do not believe it; try bpser() :
                    b0 += n as f64;
                    // which is only valid if b0 <= 1 || b0*x0 <= 0.7
                    // L_w_bpser:
                    w = bpser(a0, b0, x0, eps, log_p);
                    w1 = if log_p { r_log1_exp(w) } else { 0.5 - w + 0.5 };
                } else {
                    if x0 <= 0.7 {
                        w += bpser(a0, b0, x0, eps, false);
                        // L_end_from_w:
                        if log_p {
                            w1 = log1p(-w);
                            w = log(w);
                        } else {
                            w1 = 0.5 - w + 0.5;
                        }
                    } else {
                        // L150:
                        if a0 <= 15.0 {
                            n = 20;
                            w += bup(a0, b0, x0, y0, n, eps, false);
                            a0 += n as f64;
                        }
                        bgrat(a0, b0, x0, y0, &mut w, 15.0 * eps, &mut ierr1, false);
                        if ierr1 != 0 {
                            ierr = 10 + ierr1;
                        }
                        // L_end_from_w:
                        if log_p {
                            w1 = log1p(-w);
                            w = log(w);
                        } else {
                            w1 = 0.5 - w + 0.5;
                        }
                    }
                }
            }
        } else if a0 > b0 {
            // a0 > b0 >= 40
            if b0 <= 100.0 || lambda > b0 * 0.03 {
                // L_bfrac:
                w = bfrac(a0, b0, x0, y0, lambda, eps * 15.0, log_p);
                w1 = if log_p { r_log1_exp(w) } else { 0.5 - w + 0.5 };
            } else {
                // L180: basym
                w = basym(a0, b0, lambda, eps * 100.0, log_p);
                w1 = if log_p { r_log1_exp(w) } else { 0.5 - w + 0.5 };
            }
        } else if a0 <= 100.0 {
            // a0 <= 100; a0 <= b0 >= 40
            // L_bfrac:
            w = bfrac(a0, b0, x0, y0, lambda, eps * 15.0, log_p);
            w1 = if log_p { r_log1_exp(w) } else { 0.5 - w + 0.5 };
        } else if lambda > a0 * 0.03 {
            // b0 >= a0 > 100; lambda > a0 * 0.03
            // L_bfrac:
            w = bfrac(a0, b0, x0, y0, lambda, eps * 15.0, log_p);
            w1 = if log_p { r_log1_exp(w) } else { 0.5 - w + 0.5 };
        } else {
            // L180: basym
            w = basym(a0, b0, lambda, eps * 100.0, log_p);
            w1 = if log_p { r_log1_exp(w) } else { 0.5 - w + 0.5 };
        }

        // L_end:
        if do_swap {
            let t = w;
            w = w1;
            w1 = t;
        }
        return (w, w1, ierr);
    }
}

fn fpser(a: f64, b: f64, x: f64, eps: f64, log_p: bool) -> f64 {
    let mut ans: f64;
    let mut c: f64;
    let mut s: f64;
    let mut t: f64;
    let mut an: f64;
    let tol: f64;

    // SET  ans := x^a :
    if log_p {
        ans = a * log(x);
    } else if a > eps * 0.001 {
        t = a * log(x);
        if t < exparg(1) {
            // exp(t) would underflow
            return 0.0;
        }
        ans = exp(t);
    } else {
        ans = 1.0;
    }

    // NOTE THAT 1/B(A,B) = B

    if log_p {
        ans += log(b) - log(a);
    } else {
        ans *= b / a;
    }

    tol = eps / a;
    an = a + 1.0;
    t = x;
    s = t / an;
    loop {
        an += 1.0;
        t = x * t;
        c = t / an;
        s += c;
        if !(fabs(c) > tol) {
            break;
        }
    }

    if log_p {
        ans += log1p(a * s);
    } else {
        ans *= a * s + 1.0;
    }
    ans
}

fn apser(a: f64, b: f64, x: f64, eps: f64) -> f64 {
    let g: f64 = 0.577215664901533;

    let tol: f64;
    let c: f64;
    let mut j: f64;
    let mut s: f64;
    let mut t: f64;
    let mut aj: f64;
    let bx = b * x;

    t = x - bx;
    if b * eps <= 0.02 {
        c = log(x) + psi(b) + g + t;
    } else {
        // b > 2e13 : psi(b) ~= log(b)
        c = log(bx) + g + t;
    }

    tol = eps * 5.0 * fabs(c);
    j = 1.0;
    s = 0.0;
    loop {
        j += 1.0;
        t *= x - bx / j;
        aj = t / j;
        s += aj;
        if !(fabs(aj) > tol) {
            break;
        }
    }

    -a * (c + s)
}

fn bpser(a: f64, b: f64, x: f64, eps: f64, log_p: bool) -> f64 {
    if x == 0.0 {
        return if log_p { ML_NEGINF } else { 0.0 };
    }

    // compute the factor  x^a/(a*Beta(a,b))
    let a0 = a.min(b);
    let mut ans: f64;

    if a0 >= 1.0 {
        // 1 <= a0 <= b0
        let z = a * log(x) - betaln(a, b);
        ans = if log_p { z - log(a) } else { exp(z) / a };
    } else {
        let t: f64;
        let mut u: f64;
        let apb: f64;
        let mut b0 = a.max(b);
        if b0 < 8.0 {
            if b0 <= 1.0 {
                // a0 < 1  and  a0 <= b0 <= 1
                if log_p {
                    ans = a * log(x);
                } else {
                    ans = pow(x, a);
                    if ans == 0.0 {
                        // once underflow, always underflow ..
                        return ans;
                    }
                }
                apb = a + b;
                let c: f64;
                if apb > 1.0 {
                    u = a + b - 1.0;
                    let z = (gam1(u) + 1.0) / apb;
                    c = z;
                } else {
                    c = gam1(apb) + 1.0;
                }
                let c_val = (gam1(a) + 1.0) * (gam1(b) + 1.0) / c;
                if log_p {
                    ans += log(c_val * (b / apb));
                } else {
                    ans *= c_val * (b / apb);
                }
            } else {
                // a0 < 1 < b0 < 8
                u = gamln1(a0);
                let m = b0 as i32 - 1;
                if m >= 1 {
                    let mut cc = 1.0;
                    for _i in 1..=m {
                        b0 += -1.0;
                        cc *= b0 / (a0 + b0);
                    }
                    u += log(cc);
                }

                let z = a * log(x) - u;
                b0 += -1.0; // => b0 in (0, 7)
                let apb = a0 + b0;
                if apb > 1.0 {
                    let uu = a0 + b0 - 1.0;
                    t = (gam1(uu) + 1.0) / apb;
                } else {
                    t = gam1(apb) + 1.0;
                }

                if log_p {
                    ans = z + log(a0 / a) + log1p(gam1(b0)) - log(t);
                } else {
                    ans = exp(z) * (a0 / a) * (gam1(b0) + 1.0) / t;
                }
            }
        } else {
            // a0 < 1 < 8 <= b0
            u = gamln1(a0) + algdiv(a0, b0);
            let z = a * log(x) - u;
            if log_p {
                ans = z + log(a0 / a);
            } else {
                ans = a0 / a * exp(z);
            }
        }
    }

    if ans == (if log_p { ML_NEGINF } else { 0.0 }) || (!log_p && a <= eps * 0.1) {
        return ans;
    }

    // COMPUTE THE SERIES
    let tol = eps / a;
    let mut n: f64 = 0.0;
    let mut sum: f64 = 0.0;
    let mut ww: f64;
    let mut c = 1.0;
    loop {
        n += 1.0;
        c *= (0.5 - b / n + 0.5) * x;
        ww = c / (a + n);
        sum += ww;
        if !(n < 1e7 && fabs(ww) > tol) {
            break;
        }
    }
    // the series may not have converged in time, but we proceed

    if log_p {
        if a * sum > -1.0 {
            ans += log1p(a * sum);
        } else {
            ans = ML_NEGINF;
        }
    } else if a * sum > -1.0 {
        ans *= a * sum + 1.0;
    } else {
        ans = 0.0;
    }
    ans
}

fn bup(a: f64, b: f64, x: f64, y: f64, n: i32, eps: f64, give_log: bool) -> f64 {
    let mut ret_val: f64;
    let _i: i32;
    let _k: i32;
    let _mu: i32;
    let _d: f64;
    let _l: f64;

    // Obtain the scaling factor exp(-mu) and exp(mu)*(x^a * y^b / beta(a,b))/a
    let apb = a + b;
    let ap1 = a + 1.0;

    let (mu, d) = if n > 1 && a >= 1.0 && apb >= ap1 * 1.1 {
        let mu_val = fabs(exparg(1)) as i32;
        let k_val = exparg(0) as i32;
        let mu_final = if mu_val > k_val { k_val } else { mu_val };
        (mu_final, exp(-(mu_final as f64)))
    } else {
        (0, 1.0)
    };

    // L10:
    ret_val = if give_log {
        brcmp1(mu, a, b, x, y, true) - log(a)
    } else {
        brcmp1(mu, a, b, x, y, false) / a
    };

    if n == 1 || (give_log && ret_val == ML_NEGINF) || (!give_log && ret_val == 0.0) {
        return ret_val;
    }

    let nm1 = n - 1;
    let mut dd = d;

    // LET K BE THE INDEX OF THE MAXIMUM TERM
    let mut k = 0;
    if b > 1.0 {
        if y > 1e-4 {
            let r = (b - 1.0) * x / y - a;
            if r >= 1.0 {
                k = if r < nm1 as f64 { r as i32 } else { nm1 };
            }
        } else {
            k = nm1;
        }

        // ADD THE INCREASING TERMS OF THE SERIES - if k > 0
        // L30:
        for i in 0..k {
            let l = i as f64;
            dd *= (apb + l) / (ap1 + l) * x;
            dd += dd - dd + dd; // just to suppress unused warning; we use dd below
            // Actually we want: w += d but we're computing dd
        }
        // Redo properly:
        dd = d;
        for i in 0..k {
            let l = i as f64;
            dd *= (apb + l) / (ap1 + l) * x;
        }
        let mut w = dd;
        // L40: ADD THE REMAINING TERMS OF THE SERIES
        for i in k..nm1 {
            let l = i as f64;
            dd *= (apb + l) / (ap1 + l) * x;
            w += dd;
            if dd <= eps * w {
                break;
            }
        }

        // L50: TERMINATE THE PROCEDURE
        if give_log {
            ret_val += log(w);
        } else {
            ret_val *= w;
        }
    } else {
        // b <= 1
        let mut w = dd;
        for i in 0..nm1 {
            let l = i as f64;
            dd *= (apb + l) / (ap1 + l) * x;
            w += dd;
            if dd <= eps * w {
                break;
            }
        }
        if give_log {
            ret_val += log(w);
        } else {
            ret_val *= w;
        }
    }

    ret_val
}

fn bfrac(a: f64, b: f64, x: f64, y: f64, lambda: f64, eps: f64, log_p: bool) -> f64 {
    if !lambda.is_finite() {
        return ML_NAN;
    }

    let brc = brcomp(a, b, x, y, log_p);
    if brc.is_nan() {
        return ML_NAN;
    }
    if !log_p && brc == 0.0 {
        return 0.0;
    }

    let c = lambda + 1.0;
    let c0 = b / a;
    let c1 = 1.0 / a + 1.0;
    let yp1 = y + 1.0;
    let mut n = 0.0_f64;
    let mut p = 1.0_f64;
    let s = a + 1.0;
    let mut an = 0.0_f64;
    let mut bn = 1.0_f64;
    let mut anp1 = 1.0_f64;
    let mut bnp1 = c / c1;
    let mut r = c1 / c;
    let mut r0: f64;

    // CONTINUED FRACTION CALCULATION
    let bfrac_maxit = 1000u32;
    let mut iter = 0u32;
    loop {
        n += 1.0;
        iter += 1;
        let mut w = n * x * (b - n); // overflows when b is almost DBL_MAX !
        let rescale = !w.is_finite();
        if rescale {
            w = n * x * ldexp(b - n, -20);
        }
        let t = n / a;
        let e = a / s;
        let alpha = p * (p + c0) * e * e * (w * x);
        let e2 = (t + 1.0) / (c1 + t + t);
        let beta = w / s
            + (if rescale {
                ldexp(n + e2 * (c + n * yp1), -20)
            } else {
                n + e2 * (c + n * yp1)
            });
        p = t + 1.0;

        // update an, bn, anp1, and bnp1
        let tt = alpha * an + beta * anp1;
        an = anp1;
        anp1 = tt;
        let tt = alpha * bn + beta * bnp1;
        bn = bnp1;
        bnp1 = tt;

        r0 = r;
        r = anp1 / bnp1;
        if fabs(r - r0) <= eps * r {
            break;
        }

        // rescale an, bn, anp1, and bnp1
        an /= bnp1;
        bn /= bnp1;
        anp1 = r;
        bnp1 = 1.0;

        if iter >= bfrac_maxit {
            break;
        }
    }

    if log_p { brc + log(r) } else { brc * r }
}

fn brcomp(a: f64, b: f64, x: f64, y: f64, log_p: bool) -> f64 {
    if x == 0.0 || y == 0.0 {
        return if log_p { ML_NEGINF } else { 0.0 };
    }
    let a0 = a.min(b);
    if a0 < 8.0 {
        let lnx: f64;
        let lny: f64;
        if x <= 0.375 {
            lnx = log(x);
            lny = alnrel(-x);
        } else if y > 0.375 {
            lnx = log(x);
            lny = log(y);
        } else {
            lnx = alnrel(-y);
            lny = log(y);
        }

        let mut z = a * lnx + b * lny;
        if a0 >= 1.0 {
            z -= betaln(a, b);
            return r_d_exp(z, log_p);
        }
        // else : PROCEDURE FOR a < 1 OR b < 1
        let mut b0 = a.max(b);
        if b0 >= 8.0 {
            // L80:
            let u = gamln1(a0) + algdiv(a0, b0);
            return if log_p {
                log(a0) + (z - u)
            } else {
                a0 * exp(z - u)
            };
        }
        // else :
        if b0 <= 1.0 {
            // algorithm for max(a,b) = b0 <= 1
            let e_z = r_d_exp(z, log_p);
            if !log_p && e_z == 0.0 {
                // exp() underflow
                return 0.0;
            }

            let apb = a + b;
            if apb > 1.0 {
                z = (gam1(apb - 1.0) + 1.0) / apb;
            } else {
                z = gam1(apb) + 1.0;
            }
            let c = (gam1(a) + 1.0) * (gam1(b) + 1.0) / z;
            return if log_p {
                e_z + log(a0 * c) - log1p(a0 / b0)
            } else {
                e_z * (a0 * c) / (a0 / b0 + 1.0)
            };
        }

        // else : ALGORITHM FOR 1 < b0 < 8
        let mut u = gamln1(a0);
        let nn = b0 as i32 - 1;
        if nn >= 1 {
            let mut cc = 1.0;
            for _i in 1..=nn {
                b0 += -1.0;
                cc *= b0 / (a0 + b0);
            }
            u = log(cc) + u;
        }
        z -= u;
        b0 += -1.0;
        let apb = a0 + b0;
        let t = if apb > 1.0 {
            let uu = a0 + b0 - 1.0;
            (gam1(uu) + 1.0) / apb
        } else {
            gam1(apb) + 1.0
        };

        return if log_p {
            log(a0) + z + log1p(gam1(b0)) - log(t)
        } else {
            a0 * exp(z) * (gam1(b0) + 1.0) / t
        };
    } else {
        // PROCEDURE FOR a >= 8 AND b >= 8
        let const__ = 0.398942280401433_f64; // == 1/sqrt(2*pi);
        let h: f64;
        let x0: f64;
        let y0: f64;
        let apb = a + b;
        let lambda = if apb.is_finite() {
            if a <= b { a - apb * x } else { apb * y - b }
        } else {
            a * y - b * x
        };

        if a <= b {
            h = a / b;
            x0 = h / (h + 1.0);
            y0 = 1.0 / (h + 1.0);
        } else {
            h = b / a;
            x0 = 1.0 / (h + 1.0);
            y0 = h / (h + 1.0);
        }

        let e = -lambda / a;
        let u: f64;
        let v: f64;
        let z: f64;
        if fabs(e) > 0.6 {
            u = e - log(x / x0);
        } else {
            u = rlog1(e);
        }

        let e = lambda / b;
        if fabs(e) <= 0.6 {
            v = rlog1(e);
        } else {
            v = e - log(y / y0);
        }

        z = if log_p {
            -(a * u + b * v)
        } else {
            exp(-(a * u + b * v))
        };

        return if log_p {
            -M_LN_SQRT_2PI + 0.5 * log(b * x0) + z - bcorr(a, b)
        } else {
            const__ * sqrt(b * x0) * z * exp(-bcorr(a, b))
        };
    }
}

fn brcmp1(mu: i32, a: f64, b: f64, x: f64, y: f64, give_log: bool) -> f64 {
    let a0 = a.min(b);
    if a0 < 8.0 {
        let lnx: f64;
        let lny: f64;
        if x <= 0.375 {
            lnx = log(x);
            lny = alnrel(-x);
        } else if y > 0.375 {
            lnx = log(x);
            lny = log(y);
        } else {
            lnx = alnrel(-y);
            lny = log(y);
        }

        // L20:
        let mut z = a * lnx + b * lny;
        if a0 >= 1.0 {
            z -= betaln(a, b);
            return esum(mu, z, give_log);
        }
        // else : PROCEDURE FOR a < 1 OR b < 1
        // L30:
        let mut b0 = a.max(b);
        if b0 >= 8.0 {
            // L80:
            let u = gamln1(a0) + algdiv(a0, b0);
            return if give_log {
                log(a0) + esum(mu, z - u, true)
            } else {
                a0 * esum(mu, z - u, false)
            };
        } else if b0 <= 1.0 {
            // a0 < 1, b0 <= 1
            let ans = esum(mu, z, give_log);
            if ans == (if give_log { ML_NEGINF } else { 0.0 }) {
                return ans;
            }

            let apb = a + b;
            let z_val = if apb > 1.0 {
                (gam1(apb - 1.0) + 1.0) / apb
            } else {
                gam1(apb) + 1.0
            };
            // L50:
            let c = if give_log {
                log1p(gam1(a)) + log1p(gam1(b)) - log(z_val)
            } else {
                (gam1(a) + 1.0) * (gam1(b) + 1.0) / z_val
            };
            return if give_log {
                ans + log(a0) + c - log1p(a0 / b0)
            } else {
                ans * (a0 * c) / (a0 / b0 + 1.0)
            };
        }
        // else: algorithm for a0 < 1 < b0 < 8
        // L60:
        let mut u = gamln1(a0);
        let nn = b0 as i32 - 1;
        if nn >= 1 {
            let mut cc = 1.0;
            for _i in 1..=nn {
                b0 += -1.0;
                cc *= b0 / (a0 + b0);
            }
            u += log(cc);
        }
        // L70:
        z -= u;
        b0 += -1.0;
        let apb = a0 + b0;
        let t = if apb > 1.0 {
            (gam1(apb - 1.0) + 1.0) / apb
        } else {
            gam1(apb) + 1.0
        };
        // L72:
        return if give_log {
            log(a0) + esum(mu, z, true) + log1p(gam1(b0)) - log(t)
        } else {
            a0 * esum(mu, z, false) * (gam1(b0) + 1.0) / t
        };
    } else {
        // PROCEDURE FOR A >= 8 AND B >= 8
        let const__ = 0.398942280401433_f64; // == 1/sqrt(2*pi);
        // L100:
        let apb = a + b;
        let lambda = if apb.is_finite() {
            if a <= b { a - apb * x } else { apb * y - b }
        } else {
            a * y - b * x
        };

        let h: f64;
        let x0: f64;
        let y0: f64;
        if a > b {
            // L101:
            h = b / a;
            x0 = 1.0 / (h + 1.0);
            y0 = h / (h + 1.0);
        } else {
            h = a / b;
            x0 = h / (h + 1.0);
            y0 = 1.0 / (h + 1.0);
        }
        let lx0 = -log1p(b / a); // in both cases

        // L110:
        let e = -lambda / a;
        let u: f64;
        let v: f64;
        let z: f64;
        if fabs(e) > 0.6 {
            u = e - log(x / x0);
        } else {
            u = rlog1(e);
        }

        // L120:
        let e = lambda / b;
        if fabs(e) > 0.6 {
            v = e - log(y / y0);
        } else {
            v = rlog1(e);
        }

        // L130:
        z = esum(mu, -(a * u + b * v), give_log);
        return if give_log {
            log(const__) + (log(b) + lx0) / 2.0 + z - bcorr(a, b)
        } else {
            const__ * sqrt(b * x0) * z * exp(-bcorr(a, b))
        };
    }
}

fn bgrat(a: f64, b: f64, x: f64, y: f64, w: &mut f64, eps: f64, ierr: &mut i32, log_w: bool) {
    let n_terms_bgrat = 30usize;
    let mut c = [0.0_f64; 30];
    let mut d = [0.0_f64; 30];
    let bm1 = b - 0.5 - 0.5;
    let nu = a + bm1 * 0.5;
    let lnx = if y > 0.375 { log(x) } else { alnrel(-y) };
    let z = -nu * lnx;

    if b * z == 0.0 {
        // THE EXPANSION CANNOT BE COMPUTED
        *ierr = 1;
        return;
    }

    // COMPUTATION OF THE EXPANSION
    let log_r = log(b) + log1p(gam1(b)) + b * log(z) + nu * lnx;
    let log_u = log_r - (algdiv(b, a) + b * log(nu));
    let u = exp(log_u);

    if log_u == ML_NEGINF {
        // THE EXPANSION CANNOT BE COMPUTED
        *ierr = 2;
        return;
    }

    let u_0 = u == 0.0;
    let l = if log_w {
        if *w == ML_NEGINF {
            0.0
        } else {
            exp(*w - log_u)
        }
    } else {
        if *w == 0.0 { 0.0 } else { exp(log(*w) - log_u) }
    };

    let q_r = grat_r(b, z, log_r, eps);
    let v = 0.25 / (nu * nu);
    let t2 = lnx * 0.25 * lnx;
    let mut j = q_r;
    let mut sum = j;
    let mut t = 1.0;
    let mut cn = 1.0;
    let mut n2 = 0.0_f64;

    for n in 1..=n_terms_bgrat {
        let bp2n = b + n2;
        j = (bp2n * (bp2n + 1.0) * j + (z + bp2n + 1.0) * t) * v;
        n2 += 2.0;
        t *= t2;
        cn /= n2 * (n2 + 1.0);
        let nm1 = n - 1;
        c[nm1] = cn;
        let mut s = 0.0;

        if n > 1 {
            let mut coef = b - n as f64;
            for i in 1..=nm1 {
                s += coef * c[i - 1] * d[nm1 - i];
                coef += b;
            }
        }
        d[nm1] = bm1 * cn + s / n as f64;
        let dj = d[nm1] * j;
        sum += dj;
        if sum <= 0.0 {
            // THE EXPANSION CANNOT BE COMPUTED
            *ierr = 3;
            return;
        }
        if fabs(dj) <= eps * (sum + l) {
            *ierr = 0;
            break;
        } else if n == n_terms_bgrat {
            *ierr = 4;
        }
    }

    // ADD THE RESULTS TO W
    if log_w {
        *w = logspace_add(*w, log_u + log(sum));
    } else {
        *w += if u_0 { exp(log_u + log(sum)) } else { u * sum };
    }
}

fn grat_r(a: f64, x: f64, log_r: f64, eps: f64) -> f64 {
    // Scaled complement of incomplete gamma ratio function
    // grat_r(a,x,r) := Q(a,x) / r
    // It is assumed that a <= 1.  eps is the tolerance to be used.

    if a * x == 0.0 {
        if x <= a {
            return exp(-log_r);
        } else {
            return 0.0;
        }
    } else if a == 0.5 {
        // e.g. when called from pt()
        if x < 0.25 {
            let p = erf__(sqrt(x));
            return (0.5 - p + 0.5) * exp(-log_r);
        } else {
            // improvement for "large" x: direct computation of q/r:
            let sx = sqrt(x);
            let q_r = erfc1(1, sx) / sx * M_SQRT_PI;
            return q_r;
        }
    } else if x < 1.1 {
        // L10: Taylor series for P(a,x)/x^a
        let mut an = 3.0;
        let mut cc = x;
        let mut sum = x / (a + 3.0);
        let tol = eps * 0.1 / (a + 1.0);
        let mut t: f64;
        loop {
            an += 1.0;
            cc *= -(x / an);
            t = cc / (a + an);
            sum += t;
            if !(fabs(t) > tol) {
                break;
            }
        }

        let j = a * x * ((sum / 6.0 - 0.5 / (a + 2.0)) * x + 1.0 / (a + 1.0));
        let z = a * log(x);
        let h = gam1(a);
        let g = h + 1.0;

        if (x >= 0.25 && (a < x / 2.59)) || (z > -0.13394) {
            // L40:
            let l = rexpm1(z);
            let q = ((l + 0.5 + 0.5) * j - l) * g - h;
            if q <= 0.0 {
                return 0.0;
            } else {
                return q * exp(-log_r);
            }
        } else {
            let p = exp(z) * g * (0.5 - j + 0.5);
            return (0.5 - p + 0.5) * exp(-log_r);
        }
    } else {
        // L50: (x >= 1.1) Continued Fraction Expansion
        let mut a2n_1 = 1.0;
        let mut a2n = 1.0;
        let mut b2n_1 = x;
        let mut b2n = x + (1.0 - a);
        let mut cc = 1.0;
        let mut am0: f64;
        let mut an0: f64;

        loop {
            a2n_1 = x * a2n + cc * a2n_1;
            b2n_1 = x * b2n + cc * b2n_1;
            am0 = a2n_1 / b2n_1;
            cc += 1.0;
            let c_a = cc - a;
            a2n = a2n_1 + c_a * a2n;
            b2n = b2n_1 + c_a * b2n;
            an0 = a2n / b2n;
            if !(fabs(an0 - am0) >= eps * an0) {
                break;
            }
        }

        an0
    }
}

fn basym(a: f64, b: f64, lambda: f64, eps: f64, log_p: bool) -> f64 {
    // ASYMPTOTIC EXPANSION FOR I_x(A,B) FOR LARGE A AND B.
    // lambda := a y - b x = (a + b)y - b = a - (a+b)x  {using x + y == 1},
    // and eps is the tolerance used.
    // It is assumed that lambda >= 0, i.e., x <= a/(a+b), and both a, b >= 15

    let num_it = 20usize;
    let e0 = 1.12837916709551_f64; // e0 == 2/sqrt(pi)
    let e1 = 0.353553390593274_f64; // e1 == 2^(-3/2)
    let ln_e0 = 0.120782237635245_f64; // == ln(e0)

    let mut a0 = [0.0_f64; 21];
    let mut b0 = [0.0_f64; 21];
    let mut cc = [0.0_f64; 21];
    let mut dd = [0.0_f64; 21];

    let f = a * rlog1(-lambda / a) + b * rlog1(lambda / b);
    let t: f64;
    if log_p {
        t = -f;
    } else {
        t = exp(-f);
        if t == 0.0 {
            return 0.0; // once underflow, always underflow ..
        }
    }
    let z0 = sqrt(f);
    let z = z0 / e1 * 0.5;
    let z2 = f + f;
    let h: f64;
    let r0: f64;
    let r1: f64;
    let w0: f64;

    if a < b {
        h = a / b;
        r0 = 1.0 / (h + 1.0);
        r1 = (b - a) / b;
        w0 = 1.0 / sqrt(a * (h + 1.0));
    } else {
        h = b / a;
        r0 = 1.0 / (h + 1.0);
        r1 = (b - a) / a;
        w0 = 1.0 / sqrt(b * (h + 1.0));
    }

    a0[0] = r1 * 0.66666666666666663;
    cc[0] = a0[0] * -0.5;
    dd[0] = -cc[0];
    let mut j0 = 0.5 / e0 * erfc1(1, z0);
    let mut j1 = e1;
    let mut sum = j0 + dd[0] * w0 * j1;

    let mut s = 1.0;
    let h2 = h * h;
    let mut hn = 1.0;
    let mut w = w0;
    let mut znm1 = z;
    let mut zn = z2;

    let mut n: usize = 2;
    while n <= num_it {
        hn *= h2;
        a0[n - 1] = r0 * 2.0 * (h * hn + 1.0) / ((n + 2) as f64);
        let np1 = n + 1;
        s += hn;
        a0[np1 - 1] = r1 * 2.0 * s / ((n + 3) as f64);

        for i in n..=np1 {
            let r = ((i + 1) as f64) * -0.5;
            b0[0] = r * a0[0];
            for m in 2..=i {
                let mut bsum = 0.0;
                for jj in 1..=(m - 1) {
                    let mmj = m - jj;
                    bsum += (jj as f64 * r - mmj as f64) * a0[jj - 1] * b0[mmj - 1];
                }
                b0[m - 1] = r * a0[m - 1] + bsum / m as f64;
            }
            cc[i - 1] = b0[i - 1] / ((i + 1) as f64);

            let mut dsum = 0.0;
            for jj in 1..=(i - 1) {
                dsum += dd[i - jj - 1] * cc[jj - 1];
            }
            dd[i - 1] = -(dsum + cc[i - 1]);
        }

        j0 = e1 * znm1 + ((n - 1) as f64) * j0;
        j1 = e1 * zn + (n as f64) * j1;
        znm1 = z2 * znm1;
        zn = z2 * zn;
        w *= w0;
        let t0 = dd[n - 1] * w * j0;
        w *= w0;
        let t1 = dd[np1 - 1] * w * j1;
        sum += t0 + t1;
        if fabs(t0) + fabs(t1) <= eps * sum {
            break;
        }
        n += 2;
    }

    if log_p {
        ln_e0 + t - bcorr(a, b) + log(sum)
    } else {
        let u = exp(-bcorr(a, b));
        e0 * t * u * sum
    }
}

fn exparg(l: i32) -> f64 {
    // If l = 0 then  exparg(l) = The largest positive W for which
    // exp(W) can be computed.
    // if l = 1 (nonzero) then  exparg(l) = the largest negative W for
    // which the computed value of exp(W) is nonzero.
    // Note... only an approximate value for exparg(L) is needed.

    let lnb = 0.69314718055995_f64;
    // Rf_i1mach(16) = max exponent for double (1024 on IEEE 754)
    // Rf_i1mach(15) = min exponent for double (-1021 on IEEE 754)
    let m = if l == 0 { 1024i32 } else { -1021i32 - 1 };
    m as f64 * lnb * 0.99999
}

fn esum(mu: i32, x: f64, give_log: bool) -> f64 {
    // EVALUATION OF EXP(MU + X)
    if give_log {
        return x + mu as f64;
    }

    // else :
    let w: f64;
    if x > 0.0 {
        if mu > 0 {
            return exp(mu as f64) * exp(x);
        }
        w = mu as f64 + x;
        if w < 0.0 {
            return exp(mu as f64) * exp(x);
        }
    } else {
        if mu < 0 {
            return exp(mu as f64) * exp(x);
        }
        w = mu as f64 + x;
        if w > 0.0 {
            return exp(mu as f64) * exp(x);
        }
    }
    exp(w)
}

fn rexpm1(x: f64) -> f64 {
    // EVALUATION OF THE FUNCTION EXP(X) - 1

    let p1 = 9.14041914819518e-10_f64;
    let p2 = 0.0238082361044469_f64;
    let q1 = -0.499999999085958_f64;
    let q2 = 0.107141568980644_f64;
    let q3 = -0.0119041179760821_f64;
    let q4 = 5.95130811860248e-4_f64;

    if fabs(x) <= 0.15 {
        x * (((p2 * x + p1) * x + 1.0) / ((((q4 * x + q3) * x + q2) * x + q1) * x + 1.0))
    } else {
        let w = exp(x);
        if x > 0.0 {
            w * (0.5 - 1.0 / w + 0.5)
        } else {
            w - 0.5 - 0.5
        }
    }
}

fn alnrel(a: f64) -> f64 {
    // Evaluation of the function ln(1 + a)

    if fabs(a) > 0.375 {
        return log(1.0 + a);
    }
    // else : |a| <= 0.375
    let p1 = -1.29418923021993_f64;
    let p2 = 0.405303492862024_f64;
    let p3 = -0.0178874546012214_f64;
    let q1 = -1.62752256355323_f64;
    let q2 = 0.747811014037616_f64;
    let q3 = -0.0845104217945565_f64;
    let t = a / (a + 2.0);
    let t2 = t * t;
    let w = (((p3 * t2 + p2) * t2 + p1) * t2 + 1.0) / (((q3 * t2 + q2) * t2 + q1) * t2 + 1.0);
    t * 2.0 * w
}

fn rlog1(x: f64) -> f64 {
    // Evaluation of the function  x - ln(1 + x)

    let a = 0.0566749439387324_f64;
    let b = 0.0456512608815524_f64;
    let p0 = 0.333333333333333_f64;
    let p1 = -0.224696413112536_f64;
    let p2 = 0.00620886815375787_f64;
    let q1 = -1.27408923933623_f64;
    let q2 = 0.354508718369557_f64;

    let _h: f64;
    let _r: f64;
    let _t: f64;
    let w: f64;
    let _w1: f64;
    if x < -0.39 || x > 0.57 {
        // direct evaluation
        w = x + 0.5 + 0.5;
        return x - log(w);
    }
    // else
    let (h, w1) = if x < -0.18 {
        // L10:
        let h_val = x + 0.3;
        let h_scaled = h_val / 0.7;
        let w1_val = a - h_scaled * 0.3;
        (h_scaled, w1_val)
    } else if x > 0.18 {
        // L20:
        let h_val = x * 0.75 - 0.25;
        let w1_val = b + h_val / 3.0;
        (h_val, w1_val)
    } else {
        // Argument Reduction
        (x, 0.0)
    };

    // L30: Series Expansion
    let r = h / (h + 2.0);
    let t = r * r;
    let w = ((p2 * t + p1) * t + p0) / ((q2 * t + q1) * t + 1.0);
    t * 2.0 * (1.0 / (1.0 - r) - r * w) + w1
}

fn erf__(x: f64) -> f64 {
    // EVALUATION OF THE REAL ERROR FUNCTION

    let c = 0.564189583547756_f64;
    let a = [
        7.7105849500132e-5_f64,
        -0.00133733772997339_f64,
        0.0323076579225834_f64,
        0.0479137145607681_f64,
        0.128379167095513_f64,
    ];
    let b = [
        0.00301048631703895_f64,
        0.0538971687740286_f64,
        0.375795757275549_f64,
    ];
    let p = [
        -1.36864857382717e-7_f64,
        0.564195517478974_f64,
        7.21175825088309_f64,
        43.1622272220567_f64,
        152.98928504694_f64,
        339.320816734344_f64,
        451.918953711873_f64,
        300.459261020162_f64,
    ];
    let q = [
        1.0_f64,
        12.7827273196294_f64,
        77.0001529352295_f64,
        277.585444743988_f64,
        638.980264465631_f64,
        931.35409485061_f64,
        790.950925327898_f64,
        300.459260956983_f64,
    ];
    let r = [
        2.10144126479064_f64,
        26.2370141675169_f64,
        21.3688200555087_f64,
        4.6580782871847_f64,
        0.282094791773523_f64,
    ];
    let s = [
        94.153775055546_f64,
        187.11481179959_f64,
        99.0191814623914_f64,
        18.0124575948747_f64,
    ];

    let ax = fabs(x);
    if ax <= 0.5 {
        let t = x * x;
        let top = (((a[0] * t + a[1]) * t + a[2]) * t + a[3]) * t + a[4] + 1.0;
        let bot = ((b[0] * t + b[1]) * t + b[2]) * t + 1.0;
        return x * (top / bot);
    }

    // else: |x| > 0.5
    if ax <= 4.0 {
        let top = ((((((p[0] * ax + p[1]) * ax + p[2]) * ax + p[3]) * ax + p[4]) * ax + p[5]) * ax
            + p[6])
            * ax
            + p[7];
        let bot = ((((((q[0] * ax + q[1]) * ax + q[2]) * ax + q[3]) * ax + q[4]) * ax + q[5]) * ax
            + q[6])
            * ax
            + q[7];
        let r_val = 0.5 - exp(-x * x) * top / bot + 0.5;
        return if x < 0.0 { -r_val } else { r_val };
    }

    // else: |x| > 4
    if ax >= 5.8 {
        return if x > 0.0 { 1.0 } else { -1.0 };
    }

    // else: 4 < |x| < 5.8
    let x2 = x * x;
    let t = 1.0 / x2;
    let top = (((r[0] * t + r[1]) * t + r[2]) * t + r[3]) * t + r[4];
    let bot = (((s[0] * t + s[1]) * t + s[2]) * t + s[3]) * t + 1.0;
    let t = (c - top / (x2 * bot)) / ax;
    let r_val = 0.5 - exp(-x2) * t + 0.5;
    if x < 0.0 { -r_val } else { r_val }
}

fn erfc1(ind: i32, x: f64) -> f64 {
    // EVALUATION OF THE COMPLEMENTARY ERROR FUNCTION
    // ERFC1(ind,X) = ERFC(X)            if ind = 0
    // ERFC1(ind,X) = EXP(X*X)*ERFC(X)   otherwise, the *scaled* erfc()

    let c = 0.564189583547756_f64;
    let a = [
        7.7105849500132e-5_f64,
        -0.00133733772997339_f64,
        0.0323076579225834_f64,
        0.0479137145607681_f64,
        0.128379167095513_f64,
    ];
    let b = [
        0.00301048631703895_f64,
        0.0538971687740286_f64,
        0.375795757275549_f64,
    ];
    let p = [
        -1.36864857382717e-7_f64,
        0.564195517478974_f64,
        7.21175825088309_f64,
        43.1622272220567_f64,
        152.98928504694_f64,
        339.320816734344_f64,
        451.918953711873_f64,
        300.459261020162_f64,
    ];
    let q = [
        1.0_f64,
        12.7827273196294_f64,
        77.0001529352295_f64,
        277.585444743988_f64,
        638.980264465631_f64,
        931.35409485061_f64,
        790.950925327898_f64,
        300.459260956983_f64,
    ];
    let r = [
        2.10144126479064_f64,
        26.2370141675169_f64,
        21.3688200555087_f64,
        4.6580782871847_f64,
        0.282094791773523_f64,
    ];
    let s = [
        94.153775055546_f64,
        187.11481179959_f64,
        99.0191814623914_f64,
        18.0124575948747_f64,
    ];

    let mut ret_val: f64;
    let e: f64;
    let t: f64;
    let w: f64;
    let _bot: f64;
    let _top: f64;

    let ax = fabs(x);
    // |X| <= 0.5
    if ax <= 0.5 {
        let t = x * x;
        let top = (((a[0] * t + a[1]) * t + a[2]) * t + a[3]) * t + a[4] + 1.0;
        let bot = ((b[0] * t + b[1]) * t + b[2]) * t + 1.0;
        ret_val = 0.5 - x * (top / bot) + 0.5;
        if ind != 0 {
            ret_val = exp(t) * ret_val;
        }
        return ret_val;
    }
    // else (L10:): 0.5 < |X| <= 4
    if ax <= 4.0 {
        let top = ((((((p[0] * ax + p[1]) * ax + p[2]) * ax + p[3]) * ax + p[4]) * ax + p[5]) * ax
            + p[6])
            * ax
            + p[7];
        let bot = ((((((q[0] * ax + q[1]) * ax + q[2]) * ax + q[3]) * ax + q[4]) * ax + q[5]) * ax
            + q[6])
            * ax
            + q[7];
        ret_val = top / bot;
    } else {
        // |X| > 4
        if x <= -5.6 {
            // L50: LIMIT VALUE FOR "LARGE" NEGATIVE X
            ret_val = 2.0;
            if ind != 0 {
                ret_val = exp(x * x) * 2.0;
            }
            return ret_val;
        }
        if ind == 0 && (x > 100.0 || x * x > -exparg(1)) {
            // Underflow to limit for large positive x when ind = 0
            return 0.0;
        }

        // L30: -5.6 < x < -4  or  4 < x <= 26.6286..
        let t = 1.0 / (x * x);
        let top = (((r[0] * t + r[1]) * t + r[2]) * t + r[3]) * t + r[4];
        let bot = (((s[0] * t + s[1]) * t + s[2]) * t + s[3]) * t + 1.0;
        ret_val = (c - t * top / bot) / ax;
    }

    // L40: FINAL ASSEMBLY
    if ind != 0 {
        if x < 0.0 {
            ret_val = exp(x * x) * 2.0 - ret_val;
        }
    } else {
        // L41: ind == 0 :
        w = x * x;
        t = w;
        e = w - t; // should be 0.0, but for C compatibility
        ret_val = (0.5 - e + 0.5) * exp(-t) * ret_val;
        if x < 0.0 {
            ret_val = 2.0 - ret_val;
        }
    }
    ret_val
}

fn gam1(a: f64) -> f64 {
    // COMPUTATION OF 1/GAMMA(A+1) - 1  FOR -0.5 <= A <= 1.5

    let d = a - 0.5;
    // t := if(a > 1/2)  a-1  else  a  ==>  in [-0.5, 0.5]  <==>  |t| <= 0.5
    let t = if d > 0.0 { d - 0.5 } else { a };

    let w: f64;
    let bot: f64;
    let top: f64;
    if t < 0.0 {
        // L30:
        let r = [
            -0.422784335098468_f64,
            -0.771330383816272_f64,
            -0.244757765222226_f64,
            0.118378989872749_f64,
            9.30357293360349e-4_f64,
            -0.0118290993445146_f64,
            0.00223047661158249_f64,
            2.66505979058923e-4_f64,
            -1.32674909766242e-4_f64,
        ];
        let s1 = 0.273076135303957_f64;
        let s2 = 0.0559398236957378_f64;

        top = (((((((r[8] * t + r[7]) * t + r[6]) * t + r[5]) * t + r[4]) * t + r[3]) * t + r[2])
            * t
            + r[1])
            * t
            + r[0];
        bot = (s2 * t + s1) * t + 1.0;
        w = top / bot;
        if d > 0.0 {
            t * w / a
        } else {
            a * (w + 0.5 + 0.5)
        }
    } else if t == 0.0 {
        // L10: a in {0, 1}
        0.0
    } else {
        // t > 0; L20:
        let p = [
            0.577215664901533_f64,
            -0.409078193005776_f64,
            -0.230975380857675_f64,
            0.0597275330452234_f64,
            0.0076696818164949_f64,
            -0.00514889771323592_f64,
            5.89597428611429e-4_f64,
        ];
        let q = [
            1.0_f64,
            0.427569613095214_f64,
            0.158451672430138_f64,
            0.0261132021441447_f64,
            0.00423244297896961_f64,
        ];

        top = (((((p[6] * t + p[5]) * t + p[4]) * t + p[3]) * t + p[2]) * t + p[1]) * t + p[0];
        bot = (((q[4] * t + q[3]) * t + q[2]) * t + q[1]) * t + 1.0;
        w = top / bot;
        if d > 0.0 {
            // L21:
            t / a * (w - 0.5 - 0.5)
        } else {
            a * w
        }
    }
}

fn gamln1(a: f64) -> f64 {
    // EVALUATION OF LN(GAMMA(1 + A)) FOR -0.2 <= A <= 1.25

    let w: f64;
    if a < 0.6 {
        let p0 = 0.577215664901533_f64;
        let p1 = 0.844203922187225_f64;
        let p2 = -0.168860593646662_f64;
        let p3 = -0.780427615533591_f64;
        let p4 = -0.402055799310489_f64;
        let p5 = -0.0673562214325671_f64;
        let p6 = -0.00271935708322958_f64;
        let q1 = 2.88743195473681_f64;
        let q2 = 3.12755088914843_f64;
        let q3 = 1.56875193295039_f64;
        let q4 = 0.361951990101499_f64;
        let q5 = 0.0325038868253937_f64;
        let q6 = 6.67465618796164e-4_f64;
        w = ((((((p6 * a + p5) * a + p4) * a + p3) * a + p2) * a + p1) * a + p0)
            / ((((((q6 * a + q5) * a + q4) * a + q3) * a + q2) * a + q1) * a + 1.0);
        return -(a) * w;
    } else {
        // 0.6 <= a <= 1.25
        let r0 = 0.422784335098467_f64;
        let r1 = 0.848044614534529_f64;
        let r2 = 0.565221050691933_f64;
        let r3 = 0.156513060486551_f64;
        let r4 = 0.017050248402265_f64;
        let r5 = 4.97958207639485e-4_f64;
        let s1 = 1.24313399877507_f64;
        let s2 = 0.548042109832463_f64;
        let s3 = 0.10155218743983_f64;
        let s4 = 0.00713309612391_f64;
        let s5 = 1.16165475989616e-4_f64;
        let x = a - 0.5 - 0.5;
        w = (((((r5 * x + r4) * x + r3) * x + r2) * x + r1) * x + r0)
            / (((((s5 * x + s4) * x + s3) * x + s2) * x + s1) * x + 1.0);
        return x * w;
    }
}

fn psi(x: f64) -> f64 {
    // Evaluation of the Digamma function psi(x)

    let piov4 = 0.785398163397448_f64; // == pi / 4
    let dx0 = 1.461632144968362341262659542325721325_f64;
    // zero of psi() to extended precision

    // COEFFICIENTS FOR RATIONAL APPROXIMATION OF
    // PSI(X) / (X - X0),  0.5 <= X <= 3.
    let p1 = [
        0.0089538502298197_f64,
        4.77762828042627_f64,
        142.441585084029_f64,
        1186.45200713425_f64,
        3633.51846806499_f64,
        4138.10161269013_f64,
        1305.60269827897_f64,
    ];
    let q1 = [
        44.8452573429826_f64,
        520.752771467162_f64,
        2210.0079924783_f64,
        3641.27349079381_f64,
        1908.310765963_f64,
        6.91091682714533e-6_f64,
    ];

    // COEFFICIENTS FOR RATIONAL APPROXIMATION OF
    // PSI(X) - LN(X) + 1 / (2*X),  X > 3.
    let p2 = [
        -2.12940445131011_f64,
        -7.01677227766759_f64,
        -4.48616543918019_f64,
        -0.648157123766197_f64,
    ];
    let q2 = [
        32.2703493791143_f64,
        89.2920700481861_f64,
        54.6117738103215_f64,
        7.77788548522962_f64,
    ];

    let w: f64;
    let z: f64;
    let _den: f64;
    let _upper: f64;
    let mut aug: f64;
    let _sgn: f64;
    let xmx0: f64;
    let mut xmax1: f64;
    let xsmall: f64;
    let mut x = x;

    // XMAX1 = THE SMALLEST POSITIVE FLOATING POINT CONSTANT
    // WITH ENTIRELY INT REPRESENTATION.
    xmax1 = INT_MAX_I32 as f64;
    let d2 = 0.5 / DBL_EPSILON; // = 0.5 / (0.5 * DBL_EPS) = 1/DBL_EPSILON = 2^52
    if xmax1 > d2 {
        xmax1 = d2;
    }

    // XSMALL = ABSOLUTE ARGUMENT BELOW WHICH PI*COTAN(PI*X)
    // MAY BE REPRESENTED BY 1/X.
    xsmall = 1e-9;

    aug = 0.0;
    if x < 0.5 {
        // X < 0.5, USE REFLECTION FORMULA
        // PSI(1-X) = PSI(X) + PI * COTAN(PI*X)
        if fabs(x) <= xsmall {
            if x == 0.0 {
                return 0.0; // L_err
            }
            // 0 < |X| <= XSMALL. USE 1/X AS A SUBSTITUTE FOR PI*COTAN(PI*X)
            aug = -1.0 / x;
        } else {
            // |x| > xsmall
            // L100: REDUCTION OF ARGUMENT FOR COTAN
            let mut w_val = -x;
            let mut sgn = piov4;
            if w_val <= 0.0 {
                w_val = -w_val;
                sgn = -sgn;
            }
            // MAKE AN ERROR EXIT IF |X| >= XMAX1
            if w_val >= xmax1 {
                return 0.0; // L_err
            }
            let nq = w_val as i32;
            w_val -= nq as f64;
            let nq2 = (w_val * 4.0) as i32;
            w_val = (w_val - nq2 as f64 * 0.25) * 4.0;

            // W IS NOW RELATED TO THE FRACTIONAL PART OF 4. * X.
            // ADJUST ARGUMENT TO CORRESPOND TO VALUES IN FIRST QUADRANT AND DETERMINE SIGN
            let n = nq2 / 2;
            let w_adj = if n + n != nq2 { 1.0 - w_val } else { w_val };
            z = piov4 * w_adj;
            let m = n / 2;
            let mut sgn2 = sgn;
            if m + m != n {
                sgn2 = -sgn;
            }

            // DETERMINE FINAL VALUE FOR -PI*COTAN(PI*X)
            let n = (nq2 + 1) / 2;
            let m = n / 2;
            let m2 = m + m;
            if m2 == n {
                // CHECK FOR SINGULARITY
                if z == 0.0 {
                    return 0.0; // L_err
                }
                // USE COS/SIN AS A SUBSTITUTE FOR COTAN
                aug = sgn2 * (cos(z) / sin(z) * 4.0);
            } else {
                // L140:
                aug = sgn2 * (sin(z) / cos(z) * 4.0);
            }
        }
        x = 1.0 - x;
    }
    // L200:
    if x <= 3.0 {
        // 0.5 <= X <= 3.
        let mut den = x;
        let mut upper = p1[0] * x;

        for i in 1..=5 {
            den = (den + q1[i - 1]) * x;
            upper = (upper + p1[i]) * x;
        }

        den = (upper + p1[6]) / (den + q1[5]);
        xmx0 = x - dx0;
        return den * xmx0 + aug;
    }

    // IF X >= XMAX1, PSI = LN(X)
    if x < xmax1 {
        // 3. < X < XMAX1
        w = 1.0 / (x * x);
        let mut den = w;
        let mut upper = p2[0] * w;

        for i in 1..=3 {
            den = (den + q2[i - 1]) * w;
            upper = (upper + p2[i]) * w;
        }

        aug = upper / (den + q2[3]) - 0.5 / x + aug;
    }
    aug + log(x)
}

fn betaln(a0: f64, b0: f64) -> f64 {
    // Evaluation of the logarithm of the beta function ln(beta(a0,b0))

    let mut a = a0.min(b0);
    let mut b = a0.max(b0);

    if a < 8.0 {
        if a < 1.0 {
            // A < 1
            if b < 8.0 {
                return gamln(a) + (gamln(b) - gamln(a + b));
            } else {
                return gamln(a) + algdiv(a, b);
            }
        }
        // else: 1 <= A < 8
        if a < 2.0 {
            if b <= 2.0 {
                return gamln(a) + gamln(b) - gsumln(a, b);
            }
            if b < 8.0 {
                let w = 0.0;
                // L40: 1 < A <= B < 8 : reduction of B
                let n = b as i32 - 1;
                let mut z = 1.0;
                for _i in 1..=n {
                    b += -1.0;
                    z *= b / (a + b);
                }
                return w + log(z) + (gamln(a) + (gamln(b) - gsumln(a, b)));
            }
            return gamln(a) + algdiv(a, b);
        }
        // else L30: REDUCTION OF A WHEN B <= 1000
        if b <= 1e3 {
            let n = a as i32 - 1;
            let mut w = 1.0;
            for _i in 1..=n {
                a += -1.0;
                let h = a / b;
                w *= h / (h + 1.0);
            }
            let w = log(w);

            if b >= 8.0 {
                return w + gamln(a) + algdiv(a, b);
            }

            // L40: 1 < A <= B < 8 : reduction of B
            let n = b as i32 - 1;
            let mut z = 1.0;
            for _i in 1..=n {
                b += -1.0;
                z *= b / (a + b);
            }
            w + log(z) + (gamln(a) + (gamln(b) - gsumln(a, b)))
        } else {
            // L50: reduction of A when B > 1000
            let n = a as i32 - 1;
            let mut w = 1.0;
            for _i in 1..=n {
                a += -1.0;
                w *= a / (a / b + 1.0);
            }
            log(w) - (n as f64) * log(b) + (gamln(a) + algdiv(a, b))
        }
    } else {
        // L60: A >= 8
        let e = 0.918938533204673_f64; // e == 0.5*LN(2*PI)
        let w = bcorr(a, b);
        let h = a / b;
        let u = -(a - 0.5) * log(h / (h + 1.0));
        let v = b * alnrel(h);
        if u > v {
            log(b) * -0.5 + e + w - v - u
        } else {
            log(b) * -0.5 + e + w - u - v
        }
    }
}

fn gsumln(a: f64, b: f64) -> f64 {
    // EVALUATION OF THE FUNCTION LN(GAMMA(A + B))
    // FOR 1 <= A <= 2  AND  1 <= B <= 2

    let x = a + b - 2.0; // in [0, 2]

    if x <= 0.25 {
        return gamln1(x + 1.0);
    }

    if x <= 1.25 {
        return gamln1(x) + alnrel(x);
    }
    // else x > 1.25 :
    gamln1(x - 1.0) + log(x * (x + 1.0))
}

fn bcorr(a0: f64, b0: f64) -> f64 {
    // EVALUATION OF  DEL(A0) + DEL(B0) - DEL(A0 + B0)  WHERE
    // LN(GAMMA(A)) = (A - 0.5)*LN(A) - A + 0.5*LN(2*PI) + DEL(A).
    // IT IS ASSUMED THAT A0 >= 8 AND B0 >= 8.

    let c0 = 0.0833333333333333_f64;
    let c1 = -0.00277777777760991_f64;
    let c2 = 7.9365066682539e-4_f64;
    let c3 = -5.9520293135187e-4_f64;
    let c4 = 8.37308034031215e-4_f64;
    let c5 = -0.00165322962780713_f64;

    let a = a0.min(b0);
    let b = a0.max(b0);

    let h = a / b;
    let c = h / (h + 1.0);
    let x = 1.0 / (h + 1.0);
    let x2 = x * x;

    // SET s<n> := (1 - x^n)/(1 - x)
    let s3 = x + x2 + 1.0;
    let s5 = x + x2 * s3 + 1.0;
    let s7 = x + x2 * s5 + 1.0;
    let s9 = x + x2 * s7 + 1.0;
    let s11 = x + x2 * s9 + 1.0;

    // SET W = DEL(B) - DEL(A + B)
    let mut t = 1.0 / b;
    t *= t; // t := 1 / b^2
    let w = ((((c5 * s11 * t + c4 * s9) * t + c3 * s7) * t + c2 * s5) * t + c1 * s3) * t + c0;
    let w = w * c / b;

    // COMPUTE  DEL(A) + W
    t = 1.0 / a;
    t *= t; // t := 1 / a^2
    (((((c5 * t + c4) * t + c3) * t + c2) * t + c1) * t + c0) / a + w
}

fn algdiv(a: f64, b: f64) -> f64 {
    // COMPUTATION OF LN(GAMMA(B)/GAMMA(A+B)) WHEN B >= 8
    // IN THIS ALGORITHM, DEL(X) IS THE FUNCTION DEFINED BY
    // LN(GAMMA(X)) = (X - 0.5)*LN(X) - X + 0.5*LN(2*PI) + DEL(X).

    let c0 = 0.0833333333333333_f64;
    let c1 = -0.00277777777760991_f64;
    let c2 = 7.9365066682539e-4_f64;
    let c3 = -5.9520293135187e-4_f64;
    let c4 = 8.37308034031215e-4_f64;
    let c5 = -0.00165322962780713_f64;

    let _c: f64;
    let _d: f64;
    let _h: f64;
    let _t: f64;
    let _u: f64;
    let _v: f64;
    let _w: f64;
    let _x: f64;
    let _s3: f64;
    let _s5: f64;
    let _x2: f64;
    let _s7: f64;
    let _s9: f64;
    let _s11: f64;

    let (_h, c, x, d) = if a > b {
        let h_val = b / a;
        let c_val = 1.0 / (h_val + 1.0);
        let x_val = h_val / (h_val + 1.0);
        let d_val = a + (b - 0.5);
        (h_val, c_val, x_val, d_val)
    } else {
        let h_val = a / b;
        let c_val = h_val / (h_val + 1.0);
        let x_val = 1.0 / (h_val + 1.0);
        let d_val = b + (a - 0.5);
        (h_val, c_val, x_val, d_val)
    };

    // Set s<n> = (1 - x^n)/(1 - x) :
    let x2 = x * x;
    let s3 = x + x2 + 1.0;
    let s5 = x + x2 * s3 + 1.0;
    let s7 = x + x2 * s5 + 1.0;
    let s9 = x + x2 * s7 + 1.0;
    let s11 = x + x2 * s9 + 1.0;

    // w := Del(b) - Del(a + b)
    let t = 1.0 / (b * b);
    let w = ((((c5 * s11 * t + c4 * s9) * t + c3 * s7) * t + c2 * s5) * t + c1 * s3) * t + c0;
    let w = w * c / b;

    // COMBINE THE RESULTS
    let u = d * alnrel(a / b);
    let v = a * (log(b) - 1.0);
    if u > v { w - v - u } else { w - u - v }
}

fn gamln(a: f64) -> f64 {
    // Evaluation of  ln(gamma(a))  for positive a

    let d = 0.418938533204673_f64; // d == 0.5*(LN(2*PI) - 1)

    let c0 = 0.0833333333333333_f64;
    let c1 = -0.00277777777760991_f64;
    let c2 = 7.9365066682539e-4_f64;
    let c3 = -5.9520293135187e-4_f64;
    let c4 = 8.37308034031215e-4_f64;
    let c5 = -0.00165322962780713_f64;

    if a <= 0.8 {
        gamln1(a) - log(a)
    } else if a <= 2.25 {
        gamln1(a - 0.5 - 0.5)
    } else if a < 10.0 {
        let n = a as i32 - 1;
        let mut t = a;
        let mut w = 1.0;
        for _i in 1..=n {
            t += -1.0;
            w *= t;
        }
        gamln1(t - 1.0) + log(w)
    } else {
        // a >= 10
        let t = 1.0 / (a * a);
        let w = (((((c5 * t + c4) * t + c3) * t + c2) * t + c1) * t + c0) / a;
        d + w + (a - 0.5) * (log(a) - 1.0)
    }
}

// =====================================================================
// C FFI shims
// =====================================================================

/// C-compatible bratio: stores results via pointers.
#[unsafe(no_mangle)]
pub extern "C" fn Rf_bratio(
    a: f64,
    b: f64,
    x: f64,
    y: f64,
    w: *mut f64,
    w1: *mut f64,
    ierr: *mut std::os::raw::c_int,
    log_p: std::os::raw::c_int,
) {
    let (w_val, w1_val, ierr_val) = bratio(a, b, x, y, log_p != 0);
    if !w.is_null() {
        unsafe {
            *w = w_val;
        }
    }
    if !w1.is_null() {
        unsafe {
            *w1 = w1_val;
        }
    }
    if !ierr.is_null() {
        unsafe {
            *ierr = ierr_val;
        }
    }
}
