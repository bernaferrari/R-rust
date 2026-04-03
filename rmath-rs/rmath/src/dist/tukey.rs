#![allow(unused_assignments)]
// Studentized range (Tukey HSD) distribution: ptukey, qtukey
// Ported from R's nmath/ptukey.c and nmath/qtukey.c
//
// Original by Copenhaver, Margaret Diponzio & Holland, Burt S.
// Journal of Statistical Computation and Simulation, Vol.30, pp.1-15, 1988.
//
// ptukey.c: based on AS70 (C) 1974 Royal Statistical Society
// qtukey.c: uses secant method from Copenhaver & Holland (1988)

use super::normal::pnorm5_inner;
use crate::constants::*;
use crate::dpq::*;
use crate::error::*;
use crate::special::gamma::lgammafn;
use crate::utils::*;
use libm::*;
use std::os::raw::{c_double, c_int};

// Constants
const M_LN2: f64 = 0.693147180559945309417232121458;
const M_1_SQRT_2PI: f64 = 0.398942280401432677939946059934; // 1/sqrt(2*pi)

// =====================================================================
// wprob: helper for ptukey
// =====================================================================

fn wprob(w: f64, rr: f64, cc: f64) -> f64 {
    // Constants
    const NLEG: usize = 12;
    const IHALF: usize = 6;

    const C1: f64 = -30.0;
    const C2: f64 = -50.0;
    const C3: f64 = 60.0;
    const BB: f64 = 8.0;
    const WLAR: f64 = 3.0;
    const WINCR1: f64 = 2.0;
    const WINCR2: f64 = 3.0;

    const XLEG: [f64; 6] = [
        0.981560634246719250690549090149,
        0.904117256370474856678465866119,
        0.769902674194304687036893833213,
        0.587317954286617447296702418941,
        0.367831498998180193752691536644,
        0.125233408511468915472441369464,
    ];

    const ALEG: [f64; 6] = [
        0.047175336386511827194615961485,
        0.106939325995318430960254718194,
        0.160078328543346226334652529543,
        0.203167426723065921749064455810,
        0.233492536538354808760849898925,
        0.249147045813402785000562436043,
    ];

    let qsqz = w * 0.5;

    // if w >= 16 then the integral lower bound (occurs for c=20) is
    // 0.99999999999995 so return a value of 1.
    if qsqz >= BB {
        return 1.0;
    }

    // find (f(w/2) - 1) ^ cc (first term in integral of hartley's form)
    let mut pr_w = 2.0 * pnorm5_inner(qsqz, 0.0, 1.0, true, false) - 1.0; // erf(qsqz / sqrt(2))
    // if pr_w ^ cc < 2e-22 then set pr_w = 0
    if pr_w >= exp(C2 / cc) {
        pr_w = pow(pr_w, cc);
    } else {
        pr_w = 0.0;
    }

    // if w is large then the second component of the integral is small,
    // so fewer intervals are needed.
    let wincr = if w > WLAR { WINCR1 } else { WINCR2 };

    // find the integral of second term of hartley's form
    // for equal-length intervals using legendre quadrature.
    // limits of integration are from (w/2, 8).

    let mut blb = qsqz;
    let binc = (BB - qsqz) / wincr;
    let mut bub = blb + binc;
    let mut einsum: f64 = 0.0;

    // integrate over each interval
    let cc1 = cc - 1.0;
    for _wi in 1..=(wincr as i32) {
        let mut elsum = 0.0;
        let a = 0.5 * (bub + blb);
        let b = 0.5 * (bub - blb);

        // legendre quadrature with order = nleg
        for jj in 1..=NLEG {
            let j = if IHALF < jj { NLEG - jj } else { jj - 1 };
            let xx = if IHALF < jj { XLEG[j] } else { -XLEG[j] };
            let c_val = b * xx;
            let ac = a + c_val;

            // if exp(-qexpo/2) < 9e-14, doesn't contribute to integral
            let qexpo = ac * ac;
            if qexpo > C3 {
                break;
            }

            let pplus = 2.0 * pnorm5_inner(ac, 0.0, 1.0, true, false);
            let pminus = 2.0 * pnorm5_inner(ac, w, 1.0, true, false);

            // if rinsum ^ (cc-1) < 9e-14, doesn't contribute to integral
            let rinsum = (pplus * 0.5) - (pminus * 0.5);
            if rinsum >= exp(C1 / cc1) {
                let val = ALEG[j] * exp(-0.5 * qexpo) * pow(rinsum, cc1);
                elsum += val;
            }
        }

        elsum *= (2.0 * b) * cc * M_1_SQRT_2PI;
        einsum += elsum;
        blb = bub;
        bub += binc;
    }

    // if pr_w ^ rr < 9e-14, then return 0
    pr_w += einsum;
    if pr_w <= exp(C1 / rr) {
        return 0.0;
    }

    pr_w = pow(pr_w, rr);
    if pr_w >= 1.0 {
        return 1.0;
    }
    pr_w
}

// =====================================================================
// ptukey
// =====================================================================

pub fn ptukey_inner(
    q: f64,
    nranges: f64,
    nmeans: f64,
    df: f64,
    lower_tail: bool,
    log_p: bool,
) -> f64 {
    // Legendre quadrature constants
    const NLEGQ: usize = 16;
    const IHALFQ: usize = 8;

    const EPS1: f64 = -30.0;
    const EPS2: f64 = 1.0e-14;
    const DHAF: f64 = 100.0;
    const DQUAR: f64 = 800.0;
    const DEIGH: f64 = 5000.0;
    const DLARG: f64 = 25000.0;
    const ULEN1: f64 = 1.0;
    const ULEN2: f64 = 0.5;
    const ULEN3: f64 = 0.25;
    const ULEN4: f64 = 0.125;

    const XLEGQ: [f64; 8] = [
        0.989400934991649932596154173450,
        0.944575023073232576077988415535,
        0.865631202387831743880467897712,
        0.755404408355003033895101194847,
        0.617876244402643748446671764049,
        0.458016777657227386342419442984,
        0.281603550779258913230460501460,
        0.0950125098376374401853193354250,
    ];

    const ALEGQ: [f64; 8] = [
        0.0271524594117540948517805724560,
        0.0622535239386478928628438369944,
        0.0951585116824927848099251076022,
        0.124628971255533872052476282192,
        0.149595988816576732081501730547,
        0.169156519395002538189312079030,
        0.182603415044923588866763667969,
        0.189450610455068496285396723208,
    ];

    // IEEE_754
    if isnan(q) || isnan(nranges) || isnan(nmeans) || isnan(df) {
        return ml_warn_return_nan();
    }

    if q <= 0.0 {
        return r_dt_0(lower_tail, log_p);
    }

    // df must be > 1; there must be at least two values
    if df < 2.0 || nranges < 1.0 || nmeans < 2.0 {
        return ml_warn_return_nan();
    }

    if !r_finite(q) {
        return r_dt_1(lower_tail, log_p);
    }

    if df > DLARG {
        return r_dt_val(wprob(q, nranges, nmeans), lower_tail, log_p);
    }

    // calculate leading constant
    let f2 = df * 0.5;
    let mut f2lf = (f2 * log(df)) - (df * M_LN2) - lgammafn(f2);
    let f21 = f2 - 1.0;

    // integral is divided into unit, half-unit, quarter-unit, or
    // eighth-unit length intervals depending on the value of the
    // degrees of freedom.
    let ff4 = df * 0.25;
    let ulen = if df <= DHAF {
        ULEN1
    } else if df <= DQUAR {
        ULEN2
    } else if df <= DEIGH {
        ULEN3
    } else {
        ULEN4
    };

    f2lf += log(ulen);

    // integrate over each subinterval
    let mut ans = 0.0;
    let mut otsum_last = 0.0;

    for i in 1..=50 {
        let mut otsum = 0.0;

        // legendre quadrature with order = nlegq
        let twa1 = (2 * i - 1) as f64 * ulen;

        for jj in 1..=NLEGQ {
            let j = if IHALFQ < jj { jj - IHALFQ - 1 } else { jj - 1 };

            let t1 = if IHALFQ < jj {
                (f2lf + (f21 * log(twa1 + (XLEGQ[j] * ulen)))) - (((XLEGQ[j] * ulen) + twa1) * ff4)
            } else {
                (f2lf + (f21 * log(twa1 - (XLEGQ[j] * ulen)))) + (((XLEGQ[j] * ulen) - twa1) * ff4)
            };

            // if exp(t1) < 9e-14, then doesn't contribute to integral
            if t1 >= EPS1 {
                let qsqz = if IHALFQ < jj {
                    q * sqrt(((XLEGQ[j] * ulen) + twa1) * 0.5)
                } else {
                    q * sqrt(((-(XLEGQ[j] * ulen)) + twa1) * 0.5)
                };

                // call wprob to find integral of range portion
                let wprb = wprob(qsqz, nranges, nmeans);
                let rotsum = (wprb * ALEGQ[j]) * exp(t1);
                otsum += rotsum;
            }
        }

        // if integral for interval i < 1e-14, then stop.
        // However, at least 1/ulen intervals are calculated.
        if (i as f64) * ulen >= 1.0 && otsum <= EPS2 {
            ans += otsum;
            break;
        }

        otsum_last = otsum;
        ans += otsum;
    }

    if otsum_last > EPS2 {
        // not converged
        ml_warning(ME_PRECISION, "ptukey");
    }
    if ans > 1.0 {
        ans = 1.0;
    }
    r_dt_val(ans, lower_tail, log_p)
}

// =====================================================================
// qinv: initial estimate for qtukey
// =====================================================================

fn qinv(p: f64, c: f64, v: f64) -> f64 {
    const P0: f64 = 0.322232421088;
    const Q0: f64 = 0.0993484626060;
    const P1: f64 = -1.0;
    const Q1: f64 = 0.588581570495;
    const P2: f64 = -0.342242088547;
    const Q2: f64 = 0.531103462366;
    const P3: f64 = -0.204231210125;
    const Q3: f64 = 0.103537752850;
    const P4: f64 = -0.0000453642210148;
    const Q4: f64 = 0.00385607006340e-2;
    const C1: f64 = 0.8832;
    const C2: f64 = 0.2368;
    const C3: f64 = 1.214;
    const C4: f64 = 1.208;
    const C5: f64 = 1.4142;
    const VMAX: f64 = 120.0;

    let ps = 0.5 - 0.5 * p;
    let yi = sqrt(log(1.0 / (ps * ps)));
    let mut t = yi
        + ((((yi * P4 + P3) * yi + P2) * yi + P1) * yi + P0)
            / ((((yi * Q4 + Q3) * yi + Q2) * yi + Q1) * yi + Q0);
    if v < VMAX {
        t += (t * t * t + t) / v / 4.0;
    }
    let mut q_val = C1 - C2 * t;
    if v < VMAX {
        q_val += -C3 / v + C4 * t / v;
    }
    t * (q_val * log(c - 1.0) + C5)
}

// =====================================================================
// qtukey
// =====================================================================

pub fn qtukey_inner(
    p: f64,
    nranges: f64,
    nmeans: f64,
    df: f64,
    lower_tail: bool,
    log_p: bool,
) -> f64 {
    const EPS: f64 = 0.0001;
    const MAXITER: i32 = 50;

    // IEEE_754
    if isnan(p) || isnan(nranges) || isnan(nmeans) || isnan(df) {
        return p + nranges + nmeans + df;
    }

    // df must be > 1; there must be at least two values
    if df < 2.0 || nranges < 1.0 || nmeans < 2.0 {
        return ml_warn_return_nan();
    }

    // R_Q_P01_boundaries(p, 0, ML_POSINF)
    if log_p {
        if p > 0.0 {
            return ml_warn_return_nan();
        }
        if p == 0.0 {
            return if lower_tail { ML_POSINF } else { 0.0 };
        }
        if p == ML_NEGINF {
            return if lower_tail { 0.0 } else { ML_POSINF };
        }
    } else {
        if p < 0.0 || p > 1.0 {
            return ml_warn_return_nan();
        }
        if p == 0.0 {
            return if lower_tail { 0.0 } else { ML_POSINF };
        }
        if p == 1.0 {
            return if lower_tail { ML_POSINF } else { 0.0 };
        }
    }

    let p = r_dt_qiv(p, lower_tail, log_p); // lower_tail, non-log "p"

    // Initial value
    let x0 = qinv(p, nmeans, df);

    // Find prob(value < x0)
    let valx0 = ptukey_inner(x0, nranges, nmeans, df, true, false) - p;

    // Find the second iterate and prob(value < x1).
    // If the first iterate has probability value exceeding p
    // then second iterate is 1 less than first iterate;
    // otherwise it is 1 greater.
    let x1 = if valx0 > 0.0 {
        fmax2(0.0, x0 - 1.0)
    } else {
        x0 + 1.0
    };
    let mut valx1 = ptukey_inner(x1, nranges, nmeans, df, true, false) - p;

    // Find new iterate
    let mut x0 = x0;
    let mut valx0 = valx0;
    let mut x1 = x1;
    let mut ans = 0.0_f64;

    for _iter in 1..MAXITER {
        ans = x1 - ((valx1 * (x1 - x0)) / (valx1 - valx0));
        valx0 = valx1;

        // New iterate must be >= 0
        x0 = x1;
        if ans < 0.0 {
            ans = 0.0;
            valx1 = -p;
        }

        // Find prob(value < new iterate)
        valx1 = ptukey_inner(ans, nranges, nmeans, df, true, false) - p;
        x1 = ans;

        // If the difference between two successive iterates is less than eps, stop
        let xabs = fabs(x1 - x0);
        if xabs < EPS {
            return ans;
        }
    }

    // The process did not converge in 'maxiter' iterations
    ml_warning(ME_NOCONV, "qtukey");
    ans
}

// =====================================================================
// FFI shims
// =====================================================================

#[unsafe(no_mangle)]
pub extern "C" fn Rf_ptukey(
    q: c_double,
    nranges: c_double,
    nmeans: c_double,
    df: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    ptukey_inner(q, nranges, nmeans, df, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn ptukey(
    q: c_double,
    nranges: c_double,
    nmeans: c_double,
    df: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    ptukey_inner(q, nranges, nmeans, df, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn Rf_qtukey(
    p: c_double,
    nranges: c_double,
    nmeans: c_double,
    df: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    qtukey_inner(p, nranges, nmeans, df, lower_tail != 0, log_p != 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn qtukey(
    p: c_double,
    nranges: c_double,
    nmeans: c_double,
    df: c_double,
    lower_tail: c_int,
    log_p: c_int,
) -> c_double {
    qtukey_inner(p, nranges, nmeans, df, lower_tail != 0, log_p != 0)
}
