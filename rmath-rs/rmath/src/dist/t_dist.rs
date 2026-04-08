// t distribution: dt, pt, qt, rt
// Ported from dt.c, pt.c, qt.c, rt.c

use crate::constants::*;
use crate::dpq::*;
use crate::error::*;
use crate::special::cospi::tanpi;
use libm::*;

// Constants
const M_LN_SQRT_2PI: f64 = 0.918938533204672741780329736406; // log(sqrt(2*pi))
const M_1_SQRT_2PI: f64 = 0.398942280401432677939946059934; // 1/sqrt(2*pi)
const M_LN2: f64 = 0.693147180559945309417232121458; // log(2)
const M_PI: f64 = 3.14159265358979323846264338328; // pi
const M_1_PI: f64 = 0.318309886183790671537767526745; // 1/pi
const M_PI_2: f64 = 1.57079632679489661923132169164; // pi/2
const DBL_EPSILON: f64 = 2.220446049250313e-16;
const DBL_MAX: f64 = 1.7976931348623157e+308;
const DBL_MIN: f64 = 2.2250738585072014e-308;
const DBL_MANT_DIG: i32 = 53;

// ---- dt ----

#[must_use]
pub fn dt_inner(x: f64, n: f64, give_log: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(n) {
        return x + n;
    }
    if n <= 0.0 {
        return ml_warn_return_nan();
    }
    if !r_finite(x) {
        return r_d__0(give_log);
    }
    if !r_finite(n) {
        return crate::dist::normal::dnorm4_inner(x, 0.0, 1.0, give_log);
    }

    let t = -crate::special::bd0::bd0(n / 2.0, (n + 1.0) / 2.0)
        + crate::special::stirlerr::stirlerr((n + 1.0) / 2.0)
        - crate::special::stirlerr::stirlerr(n / 2.0);
    let x2n = x * x / n; // in [0, Inf]
    let mut ax = 0.0;
    let lrg_x2n = x2n > 1.0 / DBL_EPSILON;

    let l_x2n: f64;
    let u: f64;

    if lrg_x2n {
        // large x^2/n :
        ax = fabs(x);
        l_x2n = log(ax) - log(n) / 2.0; // = log(x2n)/2 = 1/2 * log(x^2 / n)
        u = n * l_x2n;
    } else if x2n > 0.2 {
        l_x2n = log(1.0 + x2n) / 2.0;
        u = n * l_x2n;
    } else {
        l_x2n = log1p(x2n) / 2.0;
        u = -crate::special::bd0::bd0(n / 2.0, (n + x * x) / 2.0) + x * x / 2.0;
    }

    // R_D_fexp(f,x) := (give_log ? -0.5*log(f)+(x) : exp(x)/sqrt(f))
    // f = 2pi*(1+x2n)
    //  ==> 0.5*log(f) = log(2pi)/2 + log(1+x2n)/2 = log(2pi)/2 + l_x2n
    //       1/sqrt(f) = 1/sqrt(2pi * (1+ x^2 / n))

    if give_log {
        return t - u - (M_LN_SQRT_2PI + l_x2n);
    }

    // else: if(lrg_x2n) : sqrt(1 + 1/x2n) ='= sqrt(1) = 1
    let i_sqrt_ = if lrg_x2n { sqrt(n) / ax } else { exp(-l_x2n) };
    exp(t - u) * M_1_SQRT_2PI * i_sqrt_
}

// ---- pt ----

#[must_use]
pub fn pt_inner(x: f64, n: f64, lower_tail: bool, log_p: bool) -> f64 {
    let mut lower_tail = lower_tail;

    // IEEE_754
    if isnan(x) || isnan(n) {
        return x + n;
    }
    if n <= 0.0 {
        return ml_warn_return_nan();
    }

    if !r_finite(x) {
        return if x < 0.0 {
            r_dt_0(lower_tail, log_p)
        } else {
            r_dt_1(lower_tail, log_p)
        };
    }
    if !r_finite(n) {
        return crate::dist::normal::pnorm5_inner(x, 0.0, 1.0, lower_tail, log_p);
    }

    let nx = 1.0 + (x / n) * x;
    let val: f64;

    if nx > 1e100 {
        /* Danger of underflow. So use Abramowitz & Stegun 26.5.4
        pbeta(z, a, b) ~ z^a(1-z)^b / aB(a,b) ~ z^a / aB(a,b),
        with z = 1/nx,  a = n/2,  b= 1/2 :
        lbeta(a,b) = lgammafn(a) + lgammafn(b) - lgammafn(a+b) */
        let lbeta_val = crate::special::gamma::lgammafn(0.5 * n)
            + crate::special::gamma::lgammafn(0.5)
            - crate::special::gamma::lgammafn(0.5 * n + 0.5);
        let lval = -0.5 * n * (2.0 * log(fabs(x)) - log(n)) - lbeta_val - log(0.5 * n);
        val = if log_p { lval } else { exp(lval) };
    } else if n > x * x {
        val = crate::dist::beta::pbeta_inner(
            x * x / (n + x * x),
            0.5,
            n / 2.0,
            false, /* lower_tail = 0 */
            log_p,
        );
    } else {
        val = crate::dist::beta::pbeta_inner(
            1.0 / nx,
            n / 2.0,
            0.5,
            true, /* lower_tail = 1 */
            log_p,
        );
    }

    /* Use "1 - v"  if lower_tail  and  x > 0 (but not both): */
    if x <= 0.0 {
        lower_tail = !lower_tail;
    }

    if log_p {
        if lower_tail {
            log1p(-0.5 * exp(val))
        } else {
            val - M_LN2 /* = log(.5* pbeta(....)) */
        }
    } else {
        let val = val / 2.0;
        r_d_cval(val, lower_tail)
    }
}

// ---- qt ----

#[must_use]
pub fn qt_inner(p: f64, ndf: f64, lower_tail: bool, log_p: bool) -> f64 {
    let eps: f64 = 1.0e-12;

    // IEEE_754
    if isnan(p) || isnan(ndf) {
        return p + ndf;
    }

    // R_Q_P01_boundaries(p, ML_NEGINF, ML_POSINF);
    if log_p {
        if p > 0.0 {
            return ml_warn_return_nan();
        }
        if p == 0.0 {
            return if lower_tail { ML_NEGINF } else { ML_POSINF };
        }
        if p == ML_NEGINF {
            return if lower_tail { ML_POSINF } else { ML_NEGINF };
        }
    } else {
        if p < 0.0 || p > 1.0 {
            return ml_warn_return_nan();
        }
        if p == 0.0 {
            return if lower_tail { ML_NEGINF } else { ML_POSINF };
        }
        if p == 1.0 {
            return if lower_tail { ML_POSINF } else { ML_NEGINF };
        }
    }

    if ndf <= 0.0 {
        return ml_warn_return_nan();
    }

    let mut p = p;

    if ndf < 1.0 {
        /* based on qnt */
        let accu: f64 = 1e-13;
        let q_eps: f64 = 1e-11; /* must be > accu */

        let mut ux: f64;
        let mut lx: f64;

        let mut iter: i32 = 0;

        p = r_dt_qiv(p, lower_tail, log_p);

        /* Invert pt(.) :
         * 1. finding an upper and lower bound */
        if p > 1.0 - DBL_EPSILON {
            return ML_POSINF;
        }
        let pp = fmin(1.0 - DBL_EPSILON, p * (1.0 + q_eps));
        ux = 1.0;
        while ux < DBL_MAX && pt_inner(ux, ndf, true, false) < pp {
            ux *= 2.0;
        }
        let pp = p * (1.0 - q_eps);
        lx = -1.0;
        while lx > -DBL_MAX && pt_inner(lx, ndf, true, false) > pp {
            lx *= 2.0;
        }

        /* 2. interval (lx,ux) halving
        regula falsi failed on qt(0.1, 0.1) */
        loop {
            let nx = 0.5 * (lx + ux);
            if pt_inner(nx, ndf, true, false) > p {
                ux = nx;
            } else {
                lx = nx;
            }
            if (ux - lx) / fabs(nx) <= accu {
                break;
            }
            iter += 1;
            if iter >= 1000 {
                break;
            }
        }

        if iter >= 1000 {
            crate::error::ml_warning(ME_PRECISION, "qt");
        }

        return 0.5 * (lx + ux);
    }

    /* ndf >= 1 */
    if ndf > 1e20 {
        return crate::dist::normal::qnorm5_inner(p, 0.0, 1.0, lower_tail, log_p);
    }

    let P = r_d_qiv(p, log_p); /* if exp(p) underflows, we fix below */

    let neg: bool = (!lower_tail || P < 0.5) && (lower_tail || P > 0.5);
    let is_neg_lower: bool = lower_tail == neg; /* both TRUE or FALSE == !xor */

    let P = if neg {
        2.0 * if log_p {
            if lower_tail { P } else { -expm1(p) }
        } else {
            r_d_lval(p, lower_tail)
        }
    } else {
        2.0 * if log_p {
            if lower_tail { -expm1(p) } else { P }
        } else {
            r_d_cval(p, lower_tail)
        }
    };
    /* 0 <= P <= 1 ; P = 2*min(P', 1 - P')  in all cases */

    let mut q: f64;

    if fabs(ndf - 2.0) < eps {
        /* df ~= 2 */
        if P > DBL_MIN {
            if 3.0 * P < DBL_EPSILON {
                /* P ~= 0 */
                q = 1.0 / sqrt(P);
            } else if P > 0.9 {
                /* P ~= 1 */
                q = (1.0 - P) * sqrt(2.0 / (P * (2.0 - P)));
            } else {
                /* eps/3 <= P <= 0.9 */
                q = sqrt(2.0 / (P * (2.0 - P)) - 2.0);
            }
        } else {
            /* P << 1, q = 1/sqrt(P) = ... */
            if log_p {
                q = if is_neg_lower {
                    exp(-p / 2.0) / (2.0_f64).sqrt()
                } else {
                    1.0 / sqrt(-expm1(p))
                };
            } else {
                q = ML_POSINF;
            }
        }
    } else if ndf < 1.0 + eps {
        /* df ~= 1  (df < 1 excluded above): Cauchy */
        if P == 1.0 {
            q = 0.0;
        } else if P > 0.0 {
            q = 1.0 / tanpi(P / 2.0);
        } else {
            /* P = 0, but maybe = 2*exp(p) ! */
            if log_p {
                /* 1/tan(e) ~ 1/e */
                q = if is_neg_lower {
                    M_1_PI * exp(-p)
                } else {
                    -1.0 / (M_PI * expm1(p))
                };
            } else {
                q = ML_POSINF;
            }
        }
    } else {
        /*-- usual case;  including, e.g.,  df = 1.1 */
        let mut x = 0.0_f64;
        let mut y: f64 = 0.0;
        let mut log_p2 = 0.0_f64; /* -Wall */
        let a = 1.0 / (ndf - 0.5);
        let b = 48.0 / (a * a);
        let c = ((20700.0 * a / b - 98.0) * a - 16.0) * a + 96.36;
        let d = ((94.5 / (b + c) - 3.0) / b + 1.0) * sqrt(a * M_PI_2) * ndf;

        let p_ok1: bool = P > DBL_MIN || !log_p;
        let mut p_ok: bool = p_ok1;

        if p_ok1 {
            y = pow(d * P, 2.0 / ndf);
            p_ok = y >= DBL_EPSILON;
        }
        if !p_ok {
            // log.p && P very.small  ||  (d*P)^(2/df) =: y < eps_c
            log_p2 = if is_neg_lower {
                r_d_log(p, log_p)
            } else {
                r_d_lexp(p, log_p)
            }; /* == log(P / 2) */
            x = (log(d) + M_LN2 + log_p2) / ndf;
            y = exp(2.0 * x);
        }

        if (ndf < 2.1 && P > 0.5) || y > 0.05 + a {
            /* P > P0(df) */
            /* Asymptotic inverse expansion about normal */
            if p_ok {
                x = crate::dist::normal::qnorm5_inner(
                    0.5 * P,
                    0.0,
                    1.0,
                    true,  /* lower_tail */
                    false, /* log_p */
                );
            } else {
                /* log_p && P underflowed */
                x = crate::dist::normal::qnorm5_inner(
                    log_p2, 0.0, 1.0, lower_tail, true, /* log_p */
                );
            }

            y = x * x;
            let mut c = c;
            if ndf < 5.0 {
                c += 0.3 * (ndf - 4.5) * (x + 0.6);
            }
            c += (((0.05 * d * x - 5.0) * x - 7.0) * x - 2.0) * x + b;
            y = (((((0.4 * y + 6.3) * y + 36.0) * y + 94.5) / c - y - 3.0) / b + 1.0) * x;
            y = expm1(a * y * y);
            q = sqrt(ndf * y);
        } else if !p_ok && x < -M_LN2 * (DBL_MANT_DIG as f64) {
            /* 0.5* log(DBL_EPSILON) */
            /* y above might have underflown */
            q = sqrt(ndf) * exp(-x);
        } else {
            /* re-use 'y' from above */
            y = ((1.0 / (((ndf + 6.0) / (ndf * y) - 0.089 * d - 0.822) * (ndf + 2.0) * 3.0)
                + 0.5 / (ndf + 4.0))
                * y
                - 1.0)
                * (ndf + 1.0)
                / (ndf + 2.0)
                + 1.0 / y;
            q = sqrt(ndf * y);
        }

        /* Now apply 2-term Taylor expansion improvement (1-term = Newton):
         * as by Hill (1981) [ref.above] */

        if p_ok1 {
            let m_val = fabs(sqrt(DBL_MAX / 2.0) - ndf);
            let mut it: i32 = 0;
            while it < 10 {
                it += 1;
                let y_val = dt_inner(q, ndf, false);
                if y_val <= 0.0 {
                    break;
                }
                let x_val = (pt_inner(q, ndf, false, false) - P / 2.0) / y_val;
                if !r_finite(x_val) || fabs(x_val) <= 1e-14 * fabs(q) {
                    break;
                }
                let f_val = if fabs(q) < m_val {
                    q * (ndf + 1.0) / (2.0 * (q * q + ndf))
                } else {
                    (ndf + 1.0) / (2.0 * (q + ndf / q))
                };
                let del_q = x_val * (1.0 + x_val * f_val);
                if r_finite(del_q) && r_finite(q + del_q) {
                    q += del_q;
                } else if r_finite(x_val) && r_finite(q + x_val) {
                    q += x_val;
                } else {
                    break;
                }
            }
        }
    }

    if neg { -q } else { q }
}

// ---- rt ----

#[must_use]
pub fn rt_inner(df: f64) -> f64 {
    if isnan(df) || df <= 0.0 {
        return ml_warn_return_nan();
    }

    if !r_finite(df) {
        return crate::dist::normal::norm_rand();
    } else {
        let num = crate::dist::normal::norm_rand();
        num / sqrt(crate::dist::chisq::rchisq_inner(df) / df)
    }
}

// ---- FFI shims ----

#[must_use]
pub extern "C" fn Rf_dt(x: f64, n: f64, give_log: i32) -> f64 {
    dt_inner(x, n, give_log != 0)
}

#[must_use]
pub extern "C" fn dt(x: f64, n: f64, give_log: i32) -> f64 {
    dt_inner(x, n, give_log != 0)
}

#[must_use]
pub extern "C" fn Rf_pt(x: f64, n: f64, lower_tail: i32, log_p: i32) -> f64 {
    pt_inner(x, n, lower_tail != 0, log_p != 0)
}

#[must_use]
pub extern "C" fn pt(x: f64, n: f64, lower_tail: i32, log_p: i32) -> f64 {
    pt_inner(x, n, lower_tail != 0, log_p != 0)
}

#[must_use]
pub extern "C" fn Rf_qt(p: f64, ndf: f64, lower_tail: i32, log_p: i32) -> f64 {
    qt_inner(p, ndf, lower_tail != 0, log_p != 0)
}

#[must_use]
pub extern "C" fn qt(p: f64, ndf: f64, lower_tail: i32, log_p: i32) -> f64 {
    qt_inner(p, ndf, lower_tail != 0, log_p != 0)
}

#[must_use]
pub extern "C" fn Rf_rt(df: f64) -> f64 {
    rt_inner(df)
}

#[must_use]
pub extern "C" fn rt(df: f64) -> f64 {
    rt_inner(df)
}
