// Normal distribution: dnorm, pnorm, qnorm, rnorm, norm_rand
// Ported from dnorm.c, pnorm.c, qnorm.c, rnorm.c, snorm.c

use crate::nmath::constants::*;
use crate::nmath::dpq::*;
use crate::nmath::error::*;
use crate::nmath::rng::*;
use crate::nmath::utils::*;
use libm::{exp, fabs, ldexp, log, log1p, sqrt, trunc};

// ---- dnorm ----

pub fn dnorm4_inner(x: f64, mu: f64, sigma: f64, give_log: bool) -> f64 {
    // IEEE_754
    if isnan(x) || isnan(mu) || isnan(sigma) {
        return x + mu + sigma;
    }
    if sigma < 0.0 {
        return ml_warn_return_nan();
    }
    if !r_finite(sigma) {
        return r_d__0(give_log);
    }
    if !r_finite(x) && mu == x {
        return ML_NAN; /* x-mu is NaN */
    }
    if sigma == 0.0 {
        return if x == mu { ML_POSINF } else { r_d__0(give_log) };
    }
    let mut x = (x - mu) / sigma;

    if !r_finite(x) {
        return r_d__0(give_log);
    }

    x = fabs(x);
    if x >= 2.0 * sqrt(DBL_MAX) {
        return r_d__0(give_log);
    }
    if give_log {
        return -(M_LN_SQRT_2PI + 0.5 * x * x + log(sigma));
    }
    // M_1_SQRT_2PI = 1 / sqrt(2 * pi)
    // not MATHLIB_FAST_dnorm:
    if x < 5.0 {
        return M_1_SQRT_2PI * exp(-0.5 * x * x) / sigma;
    }

    /* ELSE:
     * x*x  may lose upto about two digits accuracy for "large" x
     * Morten Welinder's proposal for PR#15620
     *
     * -- 1 --  No hoop jumping when we underflow to zero anyway:
     *
     *  -x^2/2 <         log(2)*.Machine$double.min.exp  <==>
     *     x   > sqrt(-2*log(2)*.Machine$double.min.exp) =IEEE= 37.64031
     * but "thanks" to denormalized numbers, underflow happens a bit later,
     *  effective.D.MIN.EXP <- with(.Machine, double.min.exp + double.ulp.digits)
     * for IEEE, DBL_MIN_EXP is -1022 but "effective" is -1074
     * ==> boundary = sqrt(-2*log(2)*(.Machine$double.min.exp + .Machine$double.ulp.digits))
     *              =IEEE=  38.58601
     */
    if x > sqrt(-2.0 * M_LN2 * ((DBL_MIN_EXP + 1 - DBL_MANT_DIG) as f64)) {
        return 0.0;
    }

    /* Now, to get full accuracy, split x into two parts,
     *  x = x1+x2, such that |x2| <= 2^-16.
     * Assuming that we are using IEEE doubles, that means that
     * x1*x1 is error free for x<1024 (but we have x < 38.6 anyway).
     *
     * If we do not have IEEE this is still an improvement over the naive formula.
     */
    let x1 = ldexp(r_forceint(ldexp(x, 16)), -16);
    let x2 = x - x1;
    return M_1_SQRT_2PI / sigma * (exp(-0.5 * x1 * x1) * exp((-0.5 * x2 - x1) * x2));
}

// ---- pnorm ----

/// Internal function: pnorm_both
/// i_tail in {0,1,2} means: "lower", "upper", or "both"
/// Returns (cum, ccum)
pub(crate) fn pnorm_both(x: f64, i_tail: i32, log_p: bool) -> (f64, f64) {
    const A: [f64; 5] = [
        2.2352520354606839287,
        161.02823106855587881,
        1067.6894854603709582,
        18154.981253343561249,
        0.065682337918207449113,
    ];
    const B: [f64; 4] = [
        47.20258190468824187,
        976.09855173777669322,
        10260.932208618978205,
        45507.789335026729956,
    ];
    const C: [f64; 9] = [
        0.39894151208813466764,
        8.8831497943883759412,
        93.506656132177855979,
        597.27027639480026226,
        2494.5375852903726711,
        6848.1904505362823326,
        11602.651437647350124,
        9842.7148383839780218,
        1.0765576773720192317e-8,
    ];
    const D: [f64; 8] = [
        22.266688044328115691,
        235.38790178262499861,
        1519.377599407554805,
        6485.558298266760755,
        18615.571640885098091,
        34900.952721145977266,
        38912.003286093271411,
        19685.429676859990727,
    ];
    const P: [f64; 6] = [
        0.21589853405795699,
        0.1274011611602473639,
        0.022235277870649807,
        0.001421619193227893466,
        2.9112874951168792e-5,
        0.02307344176494017303,
    ];
    const Q: [f64; 5] = [
        1.28426009614491121,
        0.468238212480865118,
        0.0659881378689285515,
        0.00378239633202758244,
        7.29751555083966205e-5,
    ];

    let mut cum: f64 = 0.0;
    let mut ccum: f64 = 0.0;

    // IEEE_754
    if isnan(x) {
        cum = x;
        ccum = x;
        return (cum, ccum);
    }

    /* Consider changing these : */
    #[allow(unused_variables)]
    let eps = DBL_EPSILON * 0.5;

    /* i_tail in {0,1,2} =^= {lower, upper, both} */
    let lower = i_tail != 1;
    let upper = i_tail != 0;

    let y = fabs(x);
    if y <= 0.67448975 {
        /* qnorm(3/4) = .6744.... -- earlier had 0.66291 */
        let (mut xnum, mut xden): (f64, f64);
        if y > eps {
            let xsq = x * x;
            xnum = A[4] * xsq;
            xden = xsq;
            for i in 0..3 {
                xnum = (xnum + A[i]) * xsq;
                xden = (xden + B[i]) * xsq;
            }
        } else {
            xnum = 0.0;
            xden = 0.0;
        }

        let temp = x * (xnum + A[3]) / (xden + B[3]);
        if lower {
            cum = 0.5 + temp;
        }
        if upper {
            ccum = 0.5 - temp;
        }
        if log_p {
            if lower {
                cum = log(cum);
            }
            if upper {
                ccum = log(ccum);
            }
        }
    } else if y <= M_SQRT_32 {
        /* Evaluate pnorm for 0.674.. = qnorm(3/4) < |x| <= sqrt(32) ~= 5.657 */

        let mut xnum = C[8] * y;
        let mut xden = y;
        for i in 0..7 {
            xnum = (xnum + C[i]) * y;
            xden = (xden + D[i]) * y;
        }
        let temp = (xnum + C[7]) / (xden + D[7]);

        // do_del(y):
        let xsq = ldexp(trunc(ldexp(y, 4)), -4);
        let del = (y - xsq) * (y + xsq);
        if log_p {
            cum = (-xsq * ldexp(xsq, -1)) - ldexp(del, -1) + log(temp);
            if (lower && x > 0.0) || (upper && x <= 0.0) {
                ccum = log1p(-exp(-xsq * ldexp(xsq, -1)) * exp(-ldexp(del, -1)) * temp);
            }
        } else {
            cum = exp(-xsq * ldexp(xsq, -1)) * exp(-ldexp(del, -1)) * temp;
            ccum = 1.0 - cum;
        }

        // swap_tail:
        if x > 0.0 {
            /* swap  ccum <--> cum */
            let temp2 = cum;
            if lower {
                cum = ccum;
            }
            ccum = temp2;
        }
    } else if (log_p && y < 1e170) /* avoid underflow below */
        || (lower && -38.4674 < x && x < 8.2924)
        || (upper && -8.2924 < x && x < 38.4674)
    {
        /* Evaluate pnorm for x in (-37.5, -5.657) union (5.657, 37.5) */
        let xsq = 1.0 / (x * x); /* (1./x)*(1./x) might be better */
        let mut xnum = P[5] * xsq;
        let mut xden = xsq;
        for i in 0..4 {
            xnum = (xnum + P[i]) * xsq;
            xden = (xden + Q[i]) * xsq;
        }
        let mut temp = xsq * (xnum + P[4]) / (xden + Q[4]);
        temp = (M_1_SQRT_2PI - temp) / y;

        // do_del(x):
        let xsq = ldexp(trunc(ldexp(x, 4)), -4);
        let del = (x - xsq) * (x + xsq);
        if log_p {
            cum = (-xsq * ldexp(xsq, -1)) - ldexp(del, -1) + log(temp);
            if (lower && x > 0.0) || (upper && x <= 0.0) {
                ccum = log1p(-exp(-xsq * ldexp(xsq, -1)) * exp(-ldexp(del, -1)) * temp);
            }
        } else {
            cum = exp(-xsq * ldexp(xsq, -1)) * exp(-ldexp(del, -1)) * temp;
            ccum = 1.0 - cum;
        }

        // swap_tail:
        if x > 0.0 {
            /* swap  ccum <--> cum */
            let temp2 = cum;
            if lower {
                cum = ccum;
            }
            ccum = temp2;
        }
    } else {
        /* large |x| such that probs are 0 or 1 */
        if x > 0.0 {
            cum = r_d__1(log_p);
            ccum = r_d__0(log_p);
        } else {
            cum = r_d__0(log_p);
            ccum = r_d__1(log_p);
        }
    }

    // NO_DENORMS is not defined (we follow R's behavior of returning denormalized)

    (cum, ccum)
}

pub fn pnorm5_inner(x: f64, mu: f64, sigma: f64, lower_tail: bool, log_p: bool) -> f64 {
    /* Note: The structure of these checks has been carefully thought through.
     * For example, if x == mu and sigma == 0, we get the correct answer 1.
     */
    // IEEE_754
    if isnan(x) || isnan(mu) || isnan(sigma) {
        return x + mu + sigma;
    }
    if !r_finite(x) && mu == x {
        return ML_NAN; /* x-mu is NaN */
    }
    if sigma <= 0.0 {
        if sigma < 0.0 {
            return ml_warn_return_nan();
        }
        /* sigma = 0 : */
        return if x < mu {
            r_dt_0(lower_tail, log_p)
        } else {
            r_dt_1(lower_tail, log_p)
        };
    }
    let p = (x - mu) / sigma;
    if !r_finite(p) {
        return if x < mu {
            r_dt_0(lower_tail, log_p)
        } else {
            r_dt_1(lower_tail, log_p)
        };
    }
    let x = p;

    let i_tail: i32 = if lower_tail { 0 } else { 1 };
    let (p, cp) = pnorm_both(x, i_tail, log_p);

    if lower_tail { p } else { cp }
}

// ---- qnorm ----

pub fn qnorm5_inner(p: f64, mu: f64, sigma: f64, lower_tail: bool, log_p: bool) -> f64 {
    // IEEE_754
    if isnan(p) || isnan(mu) || isnan(sigma) {
        return p + mu + sigma;
    }
    // R_Q_P01_boundaries(p, ML_NEGINF, ML_POSINF);
    if log_p {
        if p > 0.0 {
            return ml_warn_return_nan();
        }
        if p == 0.0 {
            /* upper bound */
            return if lower_tail { ML_POSINF } else { ML_NEGINF };
        }
        if p == ML_NEGINF {
            return if lower_tail { ML_NEGINF } else { ML_POSINF };
        }
    } else {
        /* !log_p */
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

    if sigma < 0.0 {
        return ml_warn_return_nan();
    }
    if sigma == 0.0 {
        return mu;
    }

    let p_ = r_dt_qiv(p, lower_tail, log_p); /* real lower_tail prob. p */
    let q = p_ - 0.5;

    /*-- use AS 241 --- */
    if fabs(q) <= 0.425 {
        /* |p~ - 0.5| <= .425  <==> 0.075 <= p~ <= 0.925 */
        let r = 0.180625 - q * q; // = .425^2 - q^2  >= 0
        let val = q
            * (((((((r * 2509.0809287301226727 + 33430.575583588128105) * r
                + 67265.770927008700853)
                * r
                + 45921.953931549871457)
                * r
                + 13731.693765509461125)
                * r
                + 1971.5909503065514427)
                * r
                + 133.14166789178437745)
                * r
                + 3.387132872796366608)
            / (((((((r * 5226.495278852854561 + 28729.085735721942674) * r
                + 39307.89580009271061)
                * r
                + 21213.794301586595867)
                * r
                + 5394.1960214247511077)
                * r
                + 687.1870074920579083)
                * r
                + 42.313330701600911252)
                * r
                + 1.0);
        return mu + sigma * val;
    } else {
        /* closer than 0.075 from {0,1} boundary :
         *  r := log(p~);  p~ = min(p, 1-p) < 0.075 :  */
        let lp: f64;
        if log_p && ((lower_tail && q <= 0.0) || (!lower_tail && q > 0.0)) {
            lp = p;
        } else {
            lp = log(if q > 0.0 {
                r_dt_civ(p, lower_tail, log_p) /* 1-p */
            } else {
                p_ /* = R_DT_Iv(p) ^=  p */
            });
        }
        // r = sqrt( - log(min(p,1-p)) )  <==>  min(p, 1-p) = exp( - r^2 ) :
        let mut r = sqrt(-lp);

        let val: f64;
        if r <= 5.0 {
            /* <==> min(p,1-p) >= exp(-25) ~= 1.3888e-11 */
            r += -1.6;
            val = (((((((r * 7.7454501427834140764e-4 + 0.0227238449892691845833) * r
                + 0.24178072517745061177)
                * r
                + 1.27045825245236838258)
                * r
                + 3.64784832476320460504)
                * r
                + 5.7694972214606914055)
                * r
                + 4.6303378461565452959)
                * r
                + 1.42343711074968357734)
                / (((((((r * 1.05075007164441684324e-9 + 5.475938084995344946e-4) * r
                    + 0.0151986665636164571966)
                    * r
                    + 0.14810397642748007459)
                    * r
                    + 0.68976733498510000455)
                    * r
                    + 1.6763848301838038494)
                    * r
                    + 2.05319162663775882187)
                    * r
                    + 1.0);
        } else if r <= 27.0 {
            /* p is very close to  0 or 1: r in (5, 27] */
            r += -5.0;
            val = (((((((r * 2.01033439929228813265e-7 + 2.71155556874348757815e-5) * r
                + 0.0012426609473880784386)
                * r
                + 0.026532189526576123093)
                * r
                + 0.29656057182850489123)
                * r
                + 1.7848265399172913358)
                * r
                + 5.4637849111641143699)
                * r
                + 6.6579046435011037772)
                / (((((((r * 2.04426310338993978564e-15 + 1.4215117583164458887e-7) * r
                    + 1.8463183175100546818e-5)
                    * r
                    + 7.868691311456132591e-4)
                    * r
                    + 0.0148753612908506148525)
                    * r
                    + 0.13692988092273580531)
                    * r
                    + 0.59983220655588793769)
                    * r
                    + 1.0);
        } else {
            // r > 27: p is *really* close to 0 or 1 .. practically only when log_p = TRUE
            if r >= 6.4e8 {
                // Using the asymptotical formula ("0-th order"): qn = sqrt(2*s)
                val = r * M_SQRT2;
            } else {
                let s2 = -ldexp(lp, 1); // = -2*lp = 2s
                let mut x2 = s2 - log(M_2PI * s2); // = xs_1
                if r < 36000.0 {
                    x2 = s2 - log(M_2PI * x2) - 2.0 / (2.0 + x2); // == xs_2
                    if r < 840.0 {
                        // 27 < r < 840
                        x2 = s2 - log(M_2PI * x2)
                            + 2.0 * log1p(-(1.0 - 1.0 / (4.0 + x2)) / (2.0 + x2)); // == xs_3
                        if r < 109.0 {
                            // 27 < r < 109
                            x2 = s2 - log(M_2PI * x2)
                                + 2.0
                                    * log1p(
                                        -(1.0 - (1.0 - 5.0 / (6.0 + x2)) / (4.0 + x2)) / (2.0 + x2),
                                    ); // == xs_4
                            if r < 55.0 {
                                // 27 < r < 55
                                x2 = s2 - log(M_2PI * x2)
                                    + 2.0
                                        * log1p(
                                            -(1.0
                                                - (1.0 - (5.0 - 9.0 / (8.0 + x2)) / (6.0 + x2))
                                                    / (4.0 + x2))
                                                / (2.0 + x2),
                                        ); // == xs_5
                            }
                        }
                    }
                }
                val = sqrt(x2);
            }
        }
        let val = if q < 0.0 { -val } else { val };
        return mu + sigma * val;
    }
}

// ---- rnorm ----

pub fn rnorm_inner(mu: f64, sigma: f64) -> f64 {
    if isnan(mu) || !r_finite(sigma) || sigma < 0.0 {
        return ml_warn_return_nan();
    }
    if sigma == 0.0 || !r_finite(mu) {
        return mu; /* includes mu = +/- Inf with finite sigma */
    } else {
        return mu + sigma * norm_rand();
    }
}

// ---- norm_rand (snorm.c) ----

use std::cell::Cell;

// Thread-local Box-Muller cached value.
// Used by BOX_MULLER method (not default; INVERSION is default in standalone).
thread_local! {
    static BM_NORM_KEEP: Cell<u64> = Cell::new(0);
}

#[allow(dead_code)]
fn bm_norm_keep_load() -> f64 {
    BM_NORM_KEEP.with(|c| f64::from_bits(c.get()))
}

#[allow(dead_code)]
fn bm_norm_keep_store(val: f64) {
    BM_NORM_KEEP.with(|c| c.set(val.to_bits()))
}

/// norm_rand: random variate from the STANDARD normal distribution N(0,1).
/// Uses INVERSION method (default for standalone mode).
#[unsafe(no_mangle)]
pub extern "C" fn norm_rand() -> f64 {
    const BIG: f64 = 134217728.0; /* 2^27 */

    // INVERSION method (default for standalone)
    /* unif_rand() alone is not of high enough precision */
    let mut u1 = unif_rand();
    u1 = (BIG * u1) as i64 as f64 + unif_rand();
    qnorm5_inner(u1 / BIG, 0.0, 1.0, true, false)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_norm_rand() -> f64 {
    norm_rand()
}

// ---- FFI shims ----

#[unsafe(no_mangle)]
pub extern "C" fn Rf_dnorm(x: f64, mu: f64, sigma: f64, give_log: i32) -> f64 {
    dnorm4_inner(x, mu, sigma, give_log != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn dnorm(x: f64, mu: f64, sigma: f64, give_log: i32) -> f64 {
    dnorm4_inner(x, mu, sigma, give_log != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_pnorm(x: f64, mu: f64, sigma: f64, lower_tail: i32, log_p: i32) -> f64 {
    pnorm5_inner(x, mu, sigma, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn pnorm(x: f64, mu: f64, sigma: f64, lower_tail: i32, log_p: i32) -> f64 {
    pnorm5_inner(x, mu, sigma, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_qnorm(p: f64, mu: f64, sigma: f64, lower_tail: i32, log_p: i32) -> f64 {
    qnorm5_inner(p, mu, sigma, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn qnorm(p: f64, mu: f64, sigma: f64, lower_tail: i32, log_p: i32) -> f64 {
    qnorm5_inner(p, mu, sigma, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_rnorm(mu: f64, sigma: f64) -> f64 {
    rnorm_inner(mu, sigma)
}

#[unsafe(no_mangle)]
pub extern "C" fn rnorm(mu: f64, sigma: f64) -> f64 {
    rnorm_inner(mu, sigma)
}
