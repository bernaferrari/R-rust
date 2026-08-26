// Normal distribution: dnorm, pnorm, qnorm, rnorm, norm_rand
// Ported from dnorm.c, pnorm.c, qnorm.c, rnorm.c, snorm.c

use crate::constants::*;
use crate::dpq::*;
use crate::error::*;
use crate::rng::*;
use crate::utils::*;
use libm::{exp, fabs, ldexp, log, log1p, sqrt, trunc};

// ---- dnorm ----

#[must_use]
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

#[must_use]
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

#[must_use]
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

#[must_use]
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

/// norm_rand: random variate from the STANDARD normal distribution N(0,1).
#[must_use]
/// Uses INVERSION method (default for standalone mode).
pub fn norm_rand() -> f64 {
    const BIG: f64 = 134217728.0; /* 2^27 */

    // INVERSION method (default for standalone)
    /* unif_rand() alone is not of high enough precision */
    let mut u1 = unif_rand();
    u1 = (BIG * u1) as i64 as f64 + unif_rand();
    qnorm5_inner(u1 / BIG, 0.0, 1.0, true, false)
}

#[must_use]
pub fn Rf_norm_rand() -> f64 {
    norm_rand()
}

// ---- FFI shims ----

#[must_use]
pub fn Rf_dnorm(x: f64, mu: f64, sigma: f64, give_log: i32) -> f64 {
    dnorm4_inner(x, mu, sigma, give_log != 0)
}

#[must_use]
pub fn dnorm(x: f64, mu: f64, sigma: f64, give_log: i32) -> f64 {
    dnorm4_inner(x, mu, sigma, give_log != 0)
}

#[must_use]
pub fn Rf_pnorm(x: f64, mu: f64, sigma: f64, lower_tail: i32, log_p: i32) -> f64 {
    pnorm5_inner(x, mu, sigma, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn pnorm(x: f64, mu: f64, sigma: f64, lower_tail: i32, log_p: i32) -> f64 {
    pnorm5_inner(x, mu, sigma, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn Rf_qnorm(p: f64, mu: f64, sigma: f64, lower_tail: i32, log_p: i32) -> f64 {
    qnorm5_inner(p, mu, sigma, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn qnorm(p: f64, mu: f64, sigma: f64, lower_tail: i32, log_p: i32) -> f64 {
    qnorm5_inner(p, mu, sigma, lower_tail != 0, log_p != 0)
}

#[must_use]
pub fn Rf_rnorm(mu: f64, sigma: f64) -> f64 {
    rnorm_inner(mu, sigma)
}

#[must_use]
pub fn rnorm(mu: f64, sigma: f64) -> f64 {
    rnorm_inner(mu, sigma)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-10;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    // -----------------------------------------------------------------------
    // Normal distribution invariants
    // -----------------------------------------------------------------------

    /// pnorm(qnorm(p)) ≈ p  — CDF/quantile round-trip
    #[test]
    fn normal_pq_roundtrip() {
        let probs = [
            0.001, 0.01, 0.05, 0.1, 0.25, 0.5, 0.75, 0.9, 0.95, 0.99, 0.999,
        ];
        for &p in &probs {
            let q = qnorm5_inner(p, 0.0, 1.0, true, false);
            let p_back = pnorm5_inner(q, 0.0, 1.0, true, false);
            assert!(
                approx_eq(p, p_back, TOL),
                "pnorm(qnorm({p})) = {p_back}, expected {p}"
            );
        }
    }

    /// qnorm(pnorm(x)) ≈ x  — quantile/CDF round-trip
    #[test]
    fn normal_qp_roundtrip() {
        let xs = [-3.0, -2.0, -1.0, -0.5, 0.0, 0.5, 1.0, 2.0, 3.0];
        for &x in &xs {
            let p = pnorm5_inner(x, 0.0, 1.0, true, false);
            let x_back = qnorm5_inner(p, 0.0, 1.0, true, false);
            assert!(
                approx_eq(x, x_back, 1e-8),
                "qnorm(pnorm({x})) = {x_back}, expected {x}"
            );
        }
    }

    /// dnorm(x) ≥ 0 for all x — density is non-negative
    #[test]
    fn normal_density_non_negative() {
        let xs = [
            -1e10,
            -100.0,
            -10.0,
            -1.0,
            0.0,
            1.0,
            10.0,
            100.0,
            1e10,
            f64::NEG_INFINITY,
            f64::INFINITY,
        ];
        for &x in &xs {
            let d = dnorm4_inner(x, 0.0, 1.0, false);
            assert!(d >= 0.0, "dnorm({x}) = {d}, expected non-negative");
        }
    }

    /// pnorm(-Inf) = 0, pnorm(Inf) = 1
    #[test]
    fn normal_cdf_boundary() {
        let p_neg_inf = pnorm5_inner(f64::NEG_INFINITY, 0.0, 1.0, true, false);
        let p_pos_inf = pnorm5_inner(f64::INFINITY, 0.0, 1.0, true, false);
        assert_eq!(p_neg_inf, 0.0, "pnorm(-Inf) should be 0");
        assert_eq!(p_pos_inf, 1.0, "pnorm(Inf) should be 1");
    }

    /// pnorm(0, 0, 1) = 0.5 — standard normal median
    #[test]
    fn normal_cdf_at_zero() {
        let p = pnorm5_inner(0.0, 0.0, 1.0, true, false);
        assert!(approx_eq(p, 0.5, TOL), "pnorm(0) should be 0.5, got {p}");
    }

    /// dnorm(x, mu, sigma) with negative sigma returns NaN
    #[test]
    fn normal_negative_sigma_is_nan() {
        let d = dnorm4_inner(0.0, 0.0, -1.0, false);
        assert!(d.is_nan(), "dnorm with sigma<0 should be NaN, got {d}");
    }

    /// Upper-tail: pnorm(x, lower_tail=false) = 1 - pnorm(x, lower_tail=true)
    #[test]
    fn normal_upper_tail_complement() {
        let xs = [-2.0, -1.0, 0.0, 1.0, 2.0];
        for &x in &xs {
            let lower = pnorm5_inner(x, 0.0, 1.0, true, false);
            let upper = pnorm5_inner(x, 0.0, 1.0, false, false);
            assert!(
                approx_eq(lower + upper, 1.0, TOL),
                "pnorm({x},lower) + pnorm({x},upper) = {}, expected 1.0",
                lower + upper
            );
        }
    }

    /// Shifted normal: pnorm(mu, mu, sigma) = 0.5
    #[test]
    fn normal_shifted_cdf_at_mean() {
        let params = [(5.0, 2.0), (-3.0, 0.5), (0.0, 10.0), (100.0, 0.01)];
        for &(mu, sigma) in &params {
            let p = pnorm5_inner(mu, mu, sigma, true, false);
            assert!(
                approx_eq(p, 0.5, TOL),
                "pnorm({mu}, {mu}, {sigma}) = {p}, expected 0.5"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Exponential distribution invariants
    // -----------------------------------------------------------------------

    #[test]
    fn exponential_pq_roundtrip() {
        use crate::dist::exponential::{pexp_inner, qexp_inner};
        let probs = [0.01, 0.1, 0.25, 0.5, 0.75, 0.9, 0.99];
        for &p in &probs {
            let q = qexp_inner(p, 1.0, true, false);
            let p_back = pexp_inner(q, 1.0, true, false);
            assert!(approx_eq(p, p_back, TOL), "pexp(qexp({p})) = {p_back}");
        }
    }

    #[test]
    fn exponential_density_non_negative() {
        use crate::dist::exponential::dexp_inner;
        let xs = [-1.0, 0.0, 0.001, 1.0, 10.0, 100.0];
        for &x in &xs {
            let d = dexp_inner(x, 1.0, false);
            assert!(d >= 0.0, "dexp({x}) = {d}");
        }
    }

    #[test]
    fn exponential_cdf_boundary() {
        use crate::dist::exponential::pexp_inner;
        let p0 = pexp_inner(0.0, 1.0, true, false);
        let pinf = pexp_inner(f64::INFINITY, 1.0, true, false);
        assert_eq!(p0, 0.0);
        assert_eq!(pinf, 1.0);
    }

    // -----------------------------------------------------------------------
    // Uniform distribution invariants
    // -----------------------------------------------------------------------

    #[test]
    fn uniform_pq_roundtrip() {
        use crate::dist::uniform::{punif_inner, qunif_inner};
        let probs = [0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0];
        for &p in &probs {
            let q = qunif_inner(p, 0.0, 1.0, true, false);
            let p_back = punif_inner(q, 0.0, 1.0, true, false);
            assert!(approx_eq(p, p_back, TOL), "punif(qunif({p})) = {p_back}");
        }
    }

    #[test]
    fn uniform_density_constant_inside() {
        use crate::dist::uniform::dunif_inner;
        let d1 = dunif_inner(0.2, 0.0, 1.0, false);
        let d2 = dunif_inner(0.8, 0.0, 1.0, false);
        assert_eq!(d1, d2, "uniform density should be constant");
        assert_eq!(d1, 1.0, "U(0,1) density should be 1.0");
    }

    #[test]
    fn uniform_density_zero_outside() {
        use crate::dist::uniform::dunif_inner;
        assert_eq!(dunif_inner(-0.1, 0.0, 1.0, false), 0.0);
        assert_eq!(dunif_inner(1.1, 0.0, 1.0, false), 0.0);
    }

    // -----------------------------------------------------------------------
    // Cauchy distribution invariants
    // -----------------------------------------------------------------------

    #[test]
    fn cauchy_pq_roundtrip() {
        use crate::dist::cauchy::{pcauchy_inner, qcauchy_inner};
        let probs = [0.01, 0.1, 0.25, 0.5, 0.75, 0.9, 0.99];
        for &p in &probs {
            let q = qcauchy_inner(p, 0.0, 1.0, true, false);
            let p_back = pcauchy_inner(q, 0.0, 1.0, true, false);
            assert!(
                approx_eq(p, p_back, 1e-8),
                "pcauchy(qcauchy({p})) = {p_back}"
            );
        }
    }

    #[test]
    fn cauchy_density_non_negative() {
        use crate::dist::cauchy::dcauchy_inner;
        let xs = [-1e10, -1.0, 0.0, 1.0, 1e10];
        for &x in &xs {
            let d = dcauchy_inner(x, 0.0, 1.0, false);
            assert!(d >= 0.0, "dcauchy({x}) = {d}");
        }
    }

    #[test]
    fn cauchy_cdf_at_location() {
        use crate::dist::cauchy::pcauchy_inner;
        let p = pcauchy_inner(0.0, 0.0, 1.0, true, false);
        assert!(approx_eq(p, 0.5, TOL), "pcauchy(0,0,1) = {p}");
    }

    // -----------------------------------------------------------------------
    // Gamma distribution invariants
    // -----------------------------------------------------------------------

    #[test]
    fn gamma_pq_roundtrip() {
        use crate::dist::gamma::{pgamma_inner, qgamma_inner};
        let probs = [0.01, 0.1, 0.25, 0.5, 0.75, 0.9, 0.99];
        for &p in &probs {
            let q = qgamma_inner(p, 2.0, 1.0, true, false);
            let p_back = pgamma_inner(q, 2.0, 1.0, true, false);
            assert!(
                approx_eq(p, p_back, 1e-6),
                "pgamma(qgamma({p}, shape=2)) = {p_back}"
            );
        }
    }

    #[test]
    fn gamma_density_non_negative() {
        use crate::dist::gamma::dgamma_inner;
        let xs = [-1.0, 0.0, 0.001, 1.0, 5.0, 100.0];
        for &x in &xs {
            let d = dgamma_inner(x, 2.0, 1.0, false);
            assert!(d >= 0.0, "dgamma({x}) = {d}");
        }
    }

    #[test]
    fn gamma_cdf_boundary() {
        use crate::dist::gamma::pgamma_inner;
        let p0 = pgamma_inner(0.0, 2.0, 1.0, true, false);
        let pinf = pgamma_inner(f64::INFINITY, 2.0, 1.0, true, false);
        assert_eq!(p0, 0.0);
        assert_eq!(pinf, 1.0);
    }

    // -----------------------------------------------------------------------
    // Binomial distribution invariants
    // -----------------------------------------------------------------------

    #[test]
    fn binomial_density_sums_to_one() {
        use crate::dist::binomial::dbinom_inner;
        // Sum dbinom(k, n, p) for k = 0..n should be approximately 1.0
        let cases = [(10, 0.5), (20, 0.3), (5, 0.9), (30, 0.1)];
        for &(n, p) in &cases {
            let sum: f64 = (0..=n)
                .map(|k| dbinom_inner(k as f64, n as f64, p, false))
                .sum();
            assert!(
                approx_eq(sum, 1.0, 1e-8),
                "sum(dbinom(0..{n}, {n}, {p})) = {sum}, expected ~1.0"
            );
        }
    }

    #[test]
    fn binomial_density_non_negative() {
        use crate::dist::binomial::dbinom_inner;
        for k in 0..=10 {
            let d = dbinom_inner(k as f64, 10.0, 0.5, false);
            assert!(d >= 0.0, "dbinom({k}, 10, 0.5) = {d}");
        }
    }

    #[test]
    fn binomial_pq_roundtrip_at_median() {
        use crate::dist::binomial::{pbinom_inner, qbinom_inner};
        // For n=20, p=0.5: the CDF/quantile should round-trip for non-boundary probs
        let probs = [0.1, 0.25, 0.5, 0.75, 0.9];
        for &p in &probs {
            let q = qbinom_inner(p, 20.0, 0.5, true, false);
            let p_back = pbinom_inner(q, 20.0, 0.5, true, false);
            // For discrete distributions: p_back >= p is the quantile contract
            assert!(
                p_back >= p - 1e-10,
                "pbinom(qbinom({p})) = {p_back}, should be >= {p}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Poisson distribution invariants
    // -----------------------------------------------------------------------

    #[test]
    fn poisson_density_sums_to_one() {
        use crate::dist::poisson::dpois_inner;
        // Sum for large enough range should approximate 1.0
        let lambdas = [0.5, 1.0, 5.0, 10.0];
        for &lam in &lambdas {
            let upper = (lam + 10.0 * libm::sqrt(lam)).ceil() as usize;
            let upper = upper.max(50);
            let sum: f64 = (0..=upper).map(|k| dpois_inner(k as f64, lam, false)).sum();
            assert!(
                approx_eq(sum, 1.0, 1e-6),
                "sum(dpois(0..{upper}, {lam})) = {sum}"
            );
        }
    }

    #[test]
    fn poisson_density_non_negative() {
        use crate::dist::poisson::dpois_inner;
        for k in 0..20 {
            let d = dpois_inner(k as f64, 5.0, false);
            assert!(d >= 0.0, "dpois({k}, 5) = {d}");
        }
    }

    // -----------------------------------------------------------------------
    // Chi-squared distribution invariants
    // -----------------------------------------------------------------------

    #[test]
    fn chisq_pq_roundtrip() {
        use crate::dist::chisq::{pchisq_inner, qchisq_inner};
        let probs = [0.01, 0.1, 0.5, 0.9, 0.99];
        for &p in &probs {
            let q = qchisq_inner(p, 5.0, true, false);
            let p_back = pchisq_inner(q, 5.0, true, false);
            assert!(
                approx_eq(p, p_back, 1e-6),
                "pchisq(qchisq({p}, df=5)) = {p_back}"
            );
        }
    }

    #[test]
    fn chisq_density_non_negative() {
        use crate::dist::chisq::dchisq_inner;
        let xs = [-1.0, 0.0, 1.0, 5.0, 20.0];
        for &x in &xs {
            let d = dchisq_inner(x, 5.0, false);
            assert!(d >= 0.0, "dchisq({x}, df=5) = {d}");
        }
    }

    // -----------------------------------------------------------------------
    // Weibull distribution invariants
    // -----------------------------------------------------------------------

    #[test]
    fn weibull_pq_roundtrip() {
        use crate::dist::weibull::{pweibull_inner, qweibull_inner};
        let probs = [0.01, 0.1, 0.5, 0.9, 0.99];
        for &p in &probs {
            let q = qweibull_inner(p, 2.0, 1.0, true, false);
            let p_back = pweibull_inner(q, 2.0, 1.0, true, false);
            assert!(
                approx_eq(p, p_back, TOL),
                "pweibull(qweibull({p})) = {p_back}"
            );
        }
    }

    #[test]
    fn weibull_density_non_negative() {
        use crate::dist::weibull::dweibull_inner;
        let xs = [-1.0, 0.0, 0.5, 1.0, 5.0];
        for &x in &xs {
            let d = dweibull_inner(x, 2.0, 1.0, false);
            assert!(d >= 0.0, "dweibull({x}) = {d}");
        }
    }

    // -----------------------------------------------------------------------
    // Log-normal distribution invariants
    // -----------------------------------------------------------------------

    #[test]
    fn lognormal_pq_roundtrip() {
        use crate::dist::lnorm::{plnorm_inner, qlnorm_inner};
        let probs = [0.01, 0.1, 0.25, 0.5, 0.75, 0.9, 0.99];
        for &p in &probs {
            let q = qlnorm_inner(p, 0.0, 1.0, true, false);
            let p_back = plnorm_inner(q, 0.0, 1.0, true, false);
            assert!(approx_eq(p, p_back, 1e-8), "plnorm(qlnorm({p})) = {p_back}");
        }
    }

    #[test]
    fn lognormal_density_non_negative() {
        use crate::dist::lnorm::dlnorm_inner;
        let xs = [-1.0, 0.0, 0.001, 1.0, 10.0, 100.0];
        for &x in &xs {
            let d = dlnorm_inner(x, 0.0, 1.0, false);
            assert!(d >= 0.0, "dlnorm({x}) = {d}");
        }
    }

    // -----------------------------------------------------------------------
    // Beta distribution invariants
    // -----------------------------------------------------------------------

    #[test]
    fn beta_pq_roundtrip() {
        use crate::dist::beta::{pbeta_inner, qbeta_inner};
        let probs = [0.01, 0.1, 0.25, 0.5, 0.75, 0.9, 0.99];
        for &p in &probs {
            let q = qbeta_inner(p, 2.0, 5.0, true, false);
            let p_back = pbeta_inner(q, 2.0, 5.0, true, false);
            assert!(
                approx_eq(p, p_back, 1e-6),
                "pbeta(qbeta({p}, 2, 5)) = {p_back}"
            );
        }
    }

    #[test]
    fn beta_density_non_negative() {
        use crate::dist::beta::dbeta_inner;
        let xs = [-0.1, 0.0, 0.25, 0.5, 0.75, 1.0, 1.1];
        for &x in &xs {
            let d = dbeta_inner(x, 2.0, 5.0, false);
            assert!(d >= 0.0, "dbeta({x}, 2, 5) = {d}");
        }
    }

    #[test]
    fn beta_cdf_boundary() {
        use crate::dist::beta::pbeta_inner;
        let p0 = pbeta_inner(0.0, 2.0, 5.0, true, false);
        let p1 = pbeta_inner(1.0, 2.0, 5.0, true, false);
        assert_eq!(p0, 0.0, "pbeta(0) should be 0");
        assert_eq!(p1, 1.0, "pbeta(1) should be 1");
    }

    // -----------------------------------------------------------------------
    // Log-space consistency: give_log flag
    // -----------------------------------------------------------------------

    #[test]
    fn normal_log_density_consistent() {
        let xs = [-2.0, -1.0, 0.0, 1.0, 2.0];
        for &x in &xs {
            let d = dnorm4_inner(x, 0.0, 1.0, false);
            let log_d = dnorm4_inner(x, 0.0, 1.0, true);
            assert!(
                approx_eq(d.ln(), log_d, 1e-12),
                "log(dnorm({x})) = {}, dnorm({x}, log=T) = {log_d}",
                d.ln()
            );
        }
    }

    #[test]
    fn normal_log_cdf_consistent() {
        let xs = [-2.0, -1.0, 0.0, 1.0, 2.0];
        for &x in &xs {
            let p = pnorm5_inner(x, 0.0, 1.0, true, false);
            let log_p = pnorm5_inner(x, 0.0, 1.0, true, true);
            assert!(
                approx_eq(p.ln(), log_p, 1e-12),
                "log(pnorm({x})) = {}, pnorm({x},log_p=T) = {log_p}",
                p.ln()
            );
        }
    }
}
