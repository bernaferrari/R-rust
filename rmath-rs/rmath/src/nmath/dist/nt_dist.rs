// Noncentral t distribution: dnt, pnt, qnt, rnt
// Ported from dnt.c, pnt.c, qnt.c
//
// dnt.c: Claus Ekstrom, July 15, 2003.
//   From Johnson, Kotz and Balakrishnan (1995) [2nd ed.; formula (31.15), p.516]
// pnt.c: Algorithm AS 243, Lenth,R.V. (1989). Appl. Statist., Vol.38, 185-189.
// qnt.c: Inverts pnt via bisection
// rnt: R generates noncentral t as (rnchisq(1, ncp^2) + norm_rand()) / sqrt(rchisq(df) / df)

use crate::nmath::constants::*;
use crate::nmath::dpq::*;
use crate::nmath::error::*;
use crate::nmath::special::gamma::lgammafn;
use crate::nmath::utils::*;
use libm::*;
use std::os::raw::{c_double, c_int};

use super::beta::pbeta_inner;
use super::chisq::rchisq_inner;
use super::nchisq::rnchisq_inner;
use super::normal::{dnorm4_inner, norm_rand, pnorm5_inner};
use super::t_dist::{dt_inner, pt_inner, qt_inner};

// Constants
const M_SQRT_2dPI: f64 = 0.79788456080286535587989211986876; // sqrt(2/pi)
const M_LN_SQRT_PI: f64 = 0.572364942924700087071713667651; // ln(sqrt(pi)) = ln(pi)/2
const M_LN2: f64 = 0.693147180559945309417232121458; // log(2)
const DBL_EPSILON: f64 = 2.220446049250313e-16;
const DBL_MAX: f64 = 1.7976931348623157e+308;
const DBL_MIN_EXP: i32 = -1022;

// ---- dnt ----

#[must_use]
pub fn dnt_inner(x: f64, df: f64, ncp: f64, log_p: bool) -> f64 {
    let u: f64;

    // IEEE_754
    if isnan(x) || isnan(df) {
        return x + df;
    }

    /* If non-positive df then error */
    if df <= 0.0 {
        return ml_warn_return_nan();
    }

    if ncp == 0.0 {
        return dt_inner(x, df, log_p);
    }

    /* If x is infinite then return 0 */
    if !r_finite(x) {
        return r_d__0(log_p);
    }

    /* If infinite df then the density is identical to a
       normal distribution with mean = ncp.  However, the formula
       loses a lot of accuracy around df=1e9 // FIXME?
    */
    if !r_finite(df) || df > 1e8 {
        return dnorm4_inner(x, ncp, 1.0, log_p);
    }

    /* Do calculations on log scale to stabilize */

    /* Consider two cases: x ~= 0 or not */
    if fabs(x) > sqrt(df * DBL_EPSILON) {
        // |x| > eps * sqrt(df)
        u = log(df) - log(fabs(x))
            + log(fabs(
                pnt_inner(x * sqrt((df + 2.0) / df), df + 2.0, ncp, true, false)
                    - pnt_inner(x, df, ncp, true, false),
            ));
        /* FIXME: the above still suffers from cancellation (but not horribly) */
    } else {
        /* x ~= 0 : -> same value as for  x = 0 */
        u = lgammafn((df + 1.0) / 2.0)
            - lgammafn(df / 2.0)
            - (M_LN_SQRT_PI + 0.5 * (log(df) + ncp * ncp));
    }

    if log_p { u } else { exp(u) }
}

// ---- pnt ----

#[must_use]
pub fn pnt_inner(t: f64, df: f64, ncp: f64, lower_tail: bool, log_p: bool) -> f64 {
    let mut lower_tail = lower_tail;

    let albeta: f64;
    let mut a: f64;
    let b: f64;
    let mut errbd: f64;
    let lambda: f64;
    let mut rxb: f64;
    let mut x: f64;

    let mut geven: f64;
    let mut godd: f64;
    let mut p: f64;
    let mut q: f64;
    let mut s: f64;
    let mut tnc: f64;
    let mut xeven: f64;
    let mut xodd: f64;

    /* note - itrmax and errmax may be changed to suit one's needs. */
    let itrmax: i32 = 1000;
    let errmax: f64 = 1.0e-12;

    if df <= 0.0 {
        return ml_warn_return_nan();
    }
    if ncp == 0.0 {
        return pt_inner(t, df, lower_tail, log_p);
    }

    if !r_finite(t) {
        return if t < 0.0 {
            r_dt_0(lower_tail, log_p)
        } else {
            r_dt_1(lower_tail, log_p)
        };
    }

    let (negdel, tt, del) = if t >= 0.0 {
        (false, t, ncp)
    } else {
        /* We deal quickly with left tail if extreme,
        since pt(q, df, ncp) <= pt(0, df, ncp) = \Phi(-ncp) */
        if ncp > 40.0 && (!log_p || !lower_tail) {
            return r_dt_0(lower_tail, log_p);
        }
        (true, -t, -ncp)
    };

    if df > 4e5 || del * del > 2.0 * M_LN2 * (-(DBL_MIN_EXP as f64)) {
        /*-- 2nd part: if del > 37.62, then p=0 below
        FIXME: test should depend on `df', `tt' AND `del' ! */
        /* Approx. from Abramowitz & Stegun 26.7.10 (p.949) */
        let s = 1.0 / (4.0 * df);

        return pnorm5_inner(
            tt * (1.0 - s),
            del,
            sqrt(1.0 + tt * tt * 2.0 * s),
            lower_tail != negdel,
            log_p,
        );
    }

    /* initialize twin series */
    /* Guenther, J. (1978). Statist. Computn. Simuln. vol.6, 199. */

    x = t * t;
    rxb = df / (x + df); /* := (1 - x) {x below} -- but more accurately */
    x = x / (x + df); /* in [0,1) */
    if x > 0.0 {
        /* <==>  t != 0 */
        lambda = del * del;
        p = 0.5 * exp(-0.5 * lambda);
        if p == 0.0 {
            /* underflow! */
            /*========== really use an other algorithm for this case !!! */
            crate::nmath::error::ml_warning(crate::nmath::constants::ME_UNDERFLOW, "pnt");
            crate::nmath::error::ml_warning(crate::nmath::constants::ME_RANGE, "pnt");
            return r_dt_0(lower_tail, log_p);
        }
        q = M_SQRT_2dPI * p * del;
        s = 0.5 - p;
        /* s = 0.5 - p = 0.5*(1 - exp(-.5 L)) =  -0.5*expm1(-.5 L)) */
        if s < 1e-7 {
            s = -0.5 * expm1(-0.5 * lambda);
        }
        a = 0.5;
        b = 0.5 * df;
        /* rxb = (1 - x) ^ b   [ ~= 1 - b*x for tiny x --> see 'xeven' below]
         *       where '(1 - x)' =: rxb {accurately!} above */
        rxb = pow(rxb, b);
        albeta = M_LN_SQRT_PI + lgammafn(b) - lgammafn(0.5 + b);
        xodd = pbeta_inner(x, a, b, true, false); /* lower=TRUE, log_p=FALSE */
        godd = 2.0 * rxb * exp(a * log(x) - albeta);
        let tnc_val = b * x;
        xeven = if tnc_val < DBL_EPSILON {
            tnc_val
        } else {
            1.0 - rxb
        };
        geven = tnc_val * rxb;
        tnc = p * xodd + q * xeven;

        /* repeat until convergence or iteration limit */
        let mut it: i32 = 1;
        while it <= itrmax {
            it += 1;
            a += 1.0;
            xodd -= godd;
            xeven -= geven;
            godd *= x * (a + b - 1.0) / a;
            geven *= x * (a + b - 0.5) / (a + 0.5);
            p *= lambda / (2.0 * (it as f64));
            q *= lambda / (2.0 * (it as f64) + 1.0);
            tnc += p * xodd + q * xeven;
            s -= p;
            /* R 2.4.0 added test for rounding error here. */
            if s < -1.0e-10 {
                /* happens e.g. for (t,df,ncp)=(40,10,38.5), after 799 it.*/
                crate::nmath::error::ml_warning(crate::nmath::constants::ME_PRECISION, "pnt");
                break;
            }
            if s <= 0.0 && it > 2 {
                break;
            }
            errbd = 2.0 * s * (xodd - godd);
            if fabs(errbd) < errmax {
                break; /*convergence*/
            }
        }
        // if it > itrmax: non-convergence
        // (In C there's a warning, we just silently continue)
    } else {
        /* x = t = 0 */
        tnc = 0.0;
    }
    // finis:
    tnc += pnorm5_inner(-del, 0.0, 1.0, true, false); /* lower=TRUE, log_p=FALSE */

    lower_tail = lower_tail != negdel; /* xor */
    if tnc > 1.0 - 1e-10 && lower_tail {
        crate::nmath::error::ml_warning(crate::nmath::constants::ME_PRECISION, "pnt{final}");
    }

    r_dt_val(fmin2(tnc, 1.0), lower_tail, log_p) /* Precaution */
}

// ---- qnt ----

#[must_use]
pub fn qnt_inner(p: f64, df: f64, ncp: f64, lower_tail: bool, log_p: bool) -> f64 {
    let accu: f64 = 1e-13;
    let eps: f64 = 1e-11; /* must be > accu */

    let mut ux: f64;
    let mut lx: f64;
    let mut pp: f64;

    // IEEE_754
    if isnan(p) || isnan(df) || isnan(ncp) {
        return p + df + ncp;
    }

    if df <= 0.0 {
        return ml_warn_return_nan();
    }

    if ncp == 0.0 && df >= 1.0 {
        return qt_inner(p, df, lower_tail, log_p);
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

    if !r_finite(df) {
        // df = Inf ==> limit N(ncp,1)
        return pnorm5_inner(p, ncp, 1.0, lower_tail, log_p);
    }

    let p = r_dt_qiv(p, lower_tail, log_p);

    /* Invert pnt(.) :
     * 1. finding an upper and lower bound */
    if p > 1.0 - DBL_EPSILON {
        return ML_POSINF;
    }
    pp = fmin2(1.0 - DBL_EPSILON, p * (1.0 + eps));
    ux = fmax2(1.0, ncp);
    while ux < DBL_MAX && pnt_inner(ux, df, ncp, true, false) < pp {
        ux *= 2.0;
    }
    pp = p * (1.0 - eps);
    lx = fmin2(-1.0, -ncp);
    while lx > -DBL_MAX && pnt_inner(lx, df, ncp, true, false) > pp {
        lx *= 2.0;
    }

    /* 2. interval (lx,ux)  halving : */
    loop {
        let nx = 0.5 * (lx + ux); // could be zero
        if pnt_inner(nx, df, ncp, true, false) > p {
            ux = nx;
        } else {
            lx = nx;
        }
        if !((ux - lx) > accu * fmax2(fabs(lx), fabs(ux))) {
            break;
        }
    }

    0.5 * (lx + ux)
}

// ---- rnt ----
// R generates noncentral t as:
//   (rnchisq(1, ncp^2) + norm_rand()) / sqrt(rchisq(df) / df)

#[must_use]
pub fn rnt_inner(df: f64, ncp: f64) -> f64 {
    if isnan(df) || isnan(ncp) || df <= 0.0 {
        return ml_warn_return_nan();
    }
    // R's implementation:
    // rnt = (rnchisq(1, ncp^2) + norm_rand()) / sqrt(rchisq(df) / df)
    let num = rnchisq_inner(1.0, ncp * ncp) + norm_rand();
    num / sqrt(rchisq_inner(df) / df)
}

// ---- FFI shims ----

#[must_use]
pub extern "C" fn Rf_dnt(x: c_double, df: c_double, ncp: c_double, give_log: c_int) -> c_double {
    dnt_inner(x, df, ncp, give_log != 0)
}

#[must_use]
pub extern "C" fn dnt(x: c_double, df: c_double, ncp: c_double, give_log: c_int) -> c_double {
    dnt_inner(x, df, ncp, give_log != 0)
}

pub extern "C" fn Rf_pnt(
    t: c_double,
    df: c_double,
    ncp: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    pnt_inner(t, df, ncp, lower_tail != 0, log_p != 0)
}

pub extern "C" fn pnt(
    t: c_double,
    df: c_double,
    ncp: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    pnt_inner(t, df, ncp, lower_tail != 0, log_p != 0)
}

pub extern "C" fn Rf_qnt(
    p: c_double,
    df: c_double,
    ncp: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    qnt_inner(p, df, ncp, lower_tail != 0, log_p != 0)
}

pub extern "C" fn qnt(
    p: c_double,
    df: c_double,
    ncp: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    qnt_inner(p, df, ncp, lower_tail != 0, log_p != 0)
}

#[must_use]
pub extern "C" fn Rf_rnt(df: c_double, ncp: c_double) -> c_double {
    rnt_inner(df, ncp)
}

#[must_use]
pub extern "C" fn rnt(df: c_double, ncp: c_double) -> c_double {
    rnt_inner(df, ncp)
}
