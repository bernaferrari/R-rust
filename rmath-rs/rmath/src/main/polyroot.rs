#![allow(
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unsafe_op_in_unsafe_fn
)]

//! Port of R's src/main/complex.c — polynomial root-finding (Jenkins-Traub).
//!
//! This module ports the complete polyroot() implementation including:
//!   - do_polyroot: SEXP interface for polyroot()
//!   - R_cpolyroot: Jenkins-Traub algorithm for complex polynomial roots
//!   - Supporting functions: calct, fxshft, vrshft, nexth, noshft
//!
//! Also ports the standalone complex polynomial utility functions:
//!   cdivid, polyev, errev, cpoly_cauchy, cpoly_scale

use std::os::raw::{c_double, c_int};
use std::ptr::{self, addr_of_mut};

use crate::nmath::special::mlutils::R_pow_di;
use crate::sexp::accessors::{CAR, COMPLEX, LENGTH, REAL, TYPEOF, XLENGTH};
use crate::sexp::constructors::Rf_allocVector3;
use crate::sexp::ffi::{R_FINITE, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const ETA: c_double = f64::EPSILON;
const ARE: c_double = f64::EPSILON;
const MRE: c_double = 2.0 * 1.41421356237309504880_f64 * f64::EPSILON;
const INFIN: c_double = f64::MAX;
const SMALNO: c_double = f64::MIN;
const BASE: c_double = f32::RADIX as c_double;
const COSR: c_double = -0.06975647374412529990;
const SINR: c_double = 0.99756405025982424767;

/// R's positive infinity.
pub const R_PosInf: c_double = f64::INFINITY;

// ---------------------------------------------------------------------------
// Global state for the Jenkins-Traub algorithm
// ---------------------------------------------------------------------------

static mut NN: c_int = 0;
static mut G_PR: *mut c_double = ptr::null_mut();
static mut G_PI: *mut c_double = ptr::null_mut();
static mut G_HR: *mut c_double = ptr::null_mut();
static mut G_HI: *mut c_double = ptr::null_mut();
static mut G_QPR: *mut c_double = ptr::null_mut();
static mut G_QPI: *mut c_double = ptr::null_mut();
static mut G_QHR: *mut c_double = ptr::null_mut();
static mut G_QHI: *mut c_double = ptr::null_mut();
static mut G_SHR: *mut c_double = ptr::null_mut();
static mut G_SHI: *mut c_double = ptr::null_mut();
static mut G_SR: c_double = 0.0;
static mut G_SI: c_double = 0.0;
static mut G_TR: c_double = 0.0;
static mut G_TI: c_double = 0.0;
static mut G_PVR: c_double = 0.0;
static mut G_PVI: c_double = 0.0;

// ---------------------------------------------------------------------------
// cdivid -- complex division avoiding overflow
// ---------------------------------------------------------------------------

/// Complex division `c = a / b`, avoiding overflow.
fn cdivid(ar: c_double, ai: c_double, br: c_double, bi: c_double) -> (c_double, c_double) {
    if br == 0.0 && bi == 0.0 {
        return (R_PosInf, R_PosInf);
    }
    if br.abs() >= bi.abs() {
        let r = bi / br;
        let d = br + r * bi;
        ((ar + ai * r) / d, (ai - ar * r) / d)
    } else {
        let r = br / bi;
        let d = bi + r * br;
        ((ar * r + ai) / d, (ai * r - ar) / d)
    }
}

// ---------------------------------------------------------------------------
// polyev -- polynomial evaluation (Horner) using raw pointers
// ---------------------------------------------------------------------------

unsafe fn polyev(
    n: c_int,
    s_r: c_double,
    s_i: c_double,
    p_r: *const c_double,
    p_i: *const c_double,
    q_r: *mut c_double,
    q_i: *mut c_double,
    v_r: *mut c_double,
    v_i: *mut c_double,
) {
    *q_r.offset(0) = *p_r.offset(0);
    *q_i.offset(0) = *p_i.offset(0);
    *v_r = *q_r.offset(0);
    *v_i = *q_i.offset(0);
    let mut i: c_int = 1;
    while i < n {
        let t = *v_r * s_r - *v_i * s_i + *p_r.offset(i as isize);
        *q_i.offset(i as isize) = *v_r * s_i + *v_i * s_r + *p_i.offset(i as isize);
        *v_i = *q_i.offset(i as isize);
        *q_r.offset(i as isize) = t;
        *v_r = t;
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// errev -- error estimation for Horner polynomial evaluation
// ---------------------------------------------------------------------------

unsafe fn errev(
    n: c_int,
    qr: *const c_double,
    qi: *const c_double,
    ms: c_double,
    mp: c_double,
    a_re: c_double,
    m_re: c_double,
) -> c_double {
    let mut e = (*qr.offset(0)).hypot(*qi.offset(0)) * m_re / (a_re + m_re);
    let mut i: c_int = 0;
    while i < n {
        e = e * ms + (*qr.offset(i as isize)).hypot(*qi.offset(i as isize));
        i += 1;
    }
    e * (a_re + m_re) - mp * m_re
}

// ---------------------------------------------------------------------------
// cpoly_cauchy -- Cauchy lower bound on root moduli
// ---------------------------------------------------------------------------

unsafe fn cpoly_cauchy(n: c_int, pot: *mut c_double, q: *mut c_double) -> c_double {
    let n1 = (n - 1) as usize;

    *pot.add(n1) = -*pot.add(n1);

    // compute upper estimate of bound
    let mut x = ((-*pot.add(n1)).ln() - (*pot.offset(0)).ln() / (n1 as c_double)).exp();

    // if newton step at the origin is better, use it
    if *pot.add(n1 - 1) != 0.0 {
        let xm = -*pot.add(n1) / *pot.add(n1 - 1);
        if xm < x {
            x = xm;
        }
    }

    // chop the interval (0,x) until f le 0
    loop {
        let xm = x * 0.1;
        let mut f = *pot.offset(0);
        let mut i = 1;
        while i < n as usize {
            f = f * xm + *pot.add(i);
            i += 1;
        }
        if f <= 0.0 {
            break;
        }
        x = xm;
    }

    let mut dx = x;

    // do Newton iteration until x converges to two decimal places
    while (dx / x).abs() > 0.005 {
        *q.offset(0) = *pot.offset(0);
        let mut i = 1;
        while i < n as usize {
            *q.add(i) = *q.add(i - 1) * x + *pot.add(i);
            i += 1;
        }
        let f = *q.add(n1);
        let mut delf = *q.offset(0);
        let mut i = 1;
        while i < n1 {
            delf = delf * x + *q.add(i);
            i += 1;
        }
        dx = f / delf;
        x -= dx;
    }

    x
}

// ---------------------------------------------------------------------------
// cpoly_scale -- compute scaling factor for polynomial coefficients
// ---------------------------------------------------------------------------

unsafe fn cpoly_scale(
    n: c_int,
    pot: *const c_double,
    eps: c_double,
    big: c_double,
    small: c_double,
    base: c_double,
) -> c_double {
    let high = big.sqrt();
    let lo = small / eps;
    let mut max_: c_double = 0.0;
    let mut min_: c_double = big;

    let mut i: c_int = 0;
    while i < n {
        let x = *pot.offset(i as isize);
        if x > max_ {
            max_ = x;
        }
        if x != 0.0 && x < min_ {
            min_ = x;
        }
        i += 1;
    }

    if min_ < lo || max_ > high {
        let x = lo / min_;
        let sc = if x <= 1.0 {
            1.0 / (max_.sqrt() * min_.sqrt())
        } else {
            let mut s = x;
            if big / s > max_ {
                s = 1.0;
            }
            s
        };
        let ell = (sc.ln() / base.ln() + 0.5) as i32;
        R_pow_di(base, ell)
    } else {
        1.0
    }
}

// ---------------------------------------------------------------------------
// calct -- computes t = -p(s)/h(s)
// ---------------------------------------------------------------------------

unsafe fn calct(h_s_0: *mut bool) {
    let n = *addr_of_mut!(NN) - 1;

    // evaluate h(s)
    let mut hvr: c_double = 0.0;
    let mut hvi: c_double = 0.0;
    polyev(
        n,
        *addr_of_mut!(G_SR),
        *addr_of_mut!(G_SI),
        *addr_of_mut!(G_HR),
        *addr_of_mut!(G_HI),
        *addr_of_mut!(G_QHR),
        *addr_of_mut!(G_QHI),
        &mut hvr,
        &mut hvi,
    );

    let hvr_val = hvr;
    let hvi_val = hvi;
    let hr_n1 = *(*addr_of_mut!(G_HR)).offset((n - 1) as isize);
    let hi_n1 = *(*addr_of_mut!(G_HI)).offset((n - 1) as isize);

    *h_s_0 = hvr_val.hypot(hvi_val) <= ARE * 10.0 * hr_n1.hypot(hi_n1);
    if !*h_s_0 {
        let (tr_val, ti_val) = cdivid(
            -*addr_of_mut!(G_PVR),
            -*addr_of_mut!(G_PVI),
            hvr_val,
            hvi_val,
        );
        *addr_of_mut!(G_TR) = tr_val;
        *addr_of_mut!(G_TI) = ti_val;
    } else {
        *addr_of_mut!(G_TR) = 0.0;
        *addr_of_mut!(G_TI) = 0.0;
    }
}

// ---------------------------------------------------------------------------
// nexth -- calculates the next shifted h polynomial
// ---------------------------------------------------------------------------

unsafe fn nexth(h_s_0: bool) {
    let n = *addr_of_mut!(NN) - 1;

    if !h_s_0 {
        let tr_val = *addr_of_mut!(G_TR);
        let ti_val = *addr_of_mut!(G_TI);
        let mut j: c_int = 1;
        while j < n {
            let t1 = *(*addr_of_mut!(G_QHR)).offset((j - 1) as isize);
            let t2 = *(*addr_of_mut!(G_QHI)).offset((j - 1) as isize);
            *(*addr_of_mut!(G_HR)).offset(j as isize) =
                tr_val * t1 - ti_val * t2 + *(*addr_of_mut!(G_QPR)).offset(j as isize);
            *(*addr_of_mut!(G_HI)).offset(j as isize) =
                tr_val * t2 + ti_val * t1 + *(*addr_of_mut!(G_QPI)).offset(j as isize);
            j += 1;
        }
        *(*addr_of_mut!(G_HR)).offset(0) = *(*addr_of_mut!(G_QPR)).offset(0);
        *(*addr_of_mut!(G_HI)).offset(0) = *(*addr_of_mut!(G_QPI)).offset(0);
    } else {
        // if h(s) is zero replace h with qh
        let mut j: c_int = 1;
        while j < n {
            *(*addr_of_mut!(G_HR)).offset(j as isize) =
                *(*addr_of_mut!(G_QHR)).offset((j - 1) as isize);
            *(*addr_of_mut!(G_HI)).offset(j as isize) =
                *(*addr_of_mut!(G_QHI)).offset((j - 1) as isize);
            j += 1;
        }
        *(*addr_of_mut!(G_HR)).offset(0) = 0.0;
        *(*addr_of_mut!(G_HI)).offset(0) = 0.0;
    }
}

// ---------------------------------------------------------------------------
// noshft -- computes l1 no-shift h polynomials
// ---------------------------------------------------------------------------

unsafe fn noshft(l1: c_int) {
    let n = *addr_of_mut!(NN) - 1;
    let nm1 = n - 1;

    // compute derivative polynomial as initial h
    let mut i: c_int = 0;
    while i < n {
        let xni = (*addr_of_mut!(NN) - i - 1) as c_double;
        *(*addr_of_mut!(G_HR)).offset(i as isize) =
            xni * *(*addr_of_mut!(G_PR)).offset(i as isize) / n as c_double;
        *(*addr_of_mut!(G_HI)).offset(i as isize) =
            xni * *(*addr_of_mut!(G_PI)).offset(i as isize) / n as c_double;
        i += 1;
    }

    let mut jj: c_int = 1;
    while jj <= l1 {
        let hr_n1 = *(*addr_of_mut!(G_HR)).offset((n - 1) as isize);
        let hi_n1 = *(*addr_of_mut!(G_HI)).offset((n - 1) as isize);
        let pr_n1 = *(*addr_of_mut!(G_PR)).offset((n - 1) as isize);
        let pi_n1 = *(*addr_of_mut!(G_PI)).offset((n - 1) as isize);

        if hr_n1.hypot(hi_n1) <= ETA * 10.0 * pr_n1.hypot(pi_n1) {
            // shift h coefficients
            let mut i: c_int = 1;
            while i <= nm1 {
                let j = *addr_of_mut!(NN) - i;
                *(*addr_of_mut!(G_HR)).offset((j - 1) as isize) =
                    *(*addr_of_mut!(G_HR)).offset((j - 2) as isize);
                *(*addr_of_mut!(G_HI)).offset((j - 1) as isize) =
                    *(*addr_of_mut!(G_HI)).offset((j - 2) as isize);
                i += 1;
            }
            *(*addr_of_mut!(G_HR)).offset(0) = 0.0;
            *(*addr_of_mut!(G_HI)).offset(0) = 0.0;
        } else {
            let (tr_val, ti_val) = cdivid(-pr_n1, -pi_n1, hr_n1, hi_n1);
            *addr_of_mut!(G_TR) = tr_val;
            *addr_of_mut!(G_TI) = ti_val;

            let mut i: c_int = 1;
            while i <= nm1 {
                let j = *addr_of_mut!(NN) - i;
                let t1 = *(*addr_of_mut!(G_HR)).offset((j - 2) as isize);
                let t2 = *(*addr_of_mut!(G_HI)).offset((j - 2) as isize);
                *(*addr_of_mut!(G_HR)).offset((j - 1) as isize) =
                    tr_val * t1 - ti_val * t2 + *(*addr_of_mut!(G_PR)).offset((j - 1) as isize);
                *(*addr_of_mut!(G_HI)).offset((j - 1) as isize) =
                    tr_val * t2 + ti_val * t1 + *(*addr_of_mut!(G_PI)).offset((j - 1) as isize);
                i += 1;
            }
            *(*addr_of_mut!(G_HR)).offset(0) = *(*addr_of_mut!(G_PR)).offset(0);
            *(*addr_of_mut!(G_HI)).offset(0) = *(*addr_of_mut!(G_PI)).offset(0);
        }
        jj += 1;
    }
}

// ---------------------------------------------------------------------------
// vrshft -- third stage iteration (variable-shift)
// ---------------------------------------------------------------------------

unsafe fn vrshft(l3: c_int, zr: *mut c_double, zi: *mut c_double) -> bool {
    let mut b = false;
    *addr_of_mut!(G_SR) = *zr;
    *addr_of_mut!(G_SI) = *zi;

    let mut omp: c_double = 0.0;
    let mut relstp: c_double = 0.0;

    let mut i: c_int = 1;
    while i <= l3 {
        // evaluate p at s and test for convergence
        polyev(
            *addr_of_mut!(NN),
            *addr_of_mut!(G_SR),
            *addr_of_mut!(G_SI),
            *addr_of_mut!(G_PR),
            *addr_of_mut!(G_PI),
            *addr_of_mut!(G_QPR),
            *addr_of_mut!(G_QPI),
            addr_of_mut!(G_PVR),
            addr_of_mut!(G_PVI),
        );

        let mp = (*addr_of_mut!(G_PVR)).hypot(*addr_of_mut!(G_PVI));
        let ms = (*addr_of_mut!(G_SR)).hypot(*addr_of_mut!(G_SI));
        if mp
            <= 20.0
                * errev(
                    *addr_of_mut!(NN),
                    *addr_of_mut!(G_QPR),
                    *addr_of_mut!(G_QPI),
                    ms,
                    mp,
                    ETA,
                    MRE,
                )
        {
            // convergence
            *zr = *addr_of_mut!(G_SR);
            *zi = *addr_of_mut!(G_SI);
            return true;
        }

        if i != 1 {
            if !b && mp >= omp && relstp < 0.05 {
                // iteration has stalled. probably a cluster of zeros.
                let tp = if relstp < ETA { ETA } else { relstp };
                b = true;
                let r1 = tp.sqrt();
                let r2 = *addr_of_mut!(G_SR) * (r1 + 1.0) - *addr_of_mut!(G_SI) * r1;
                *addr_of_mut!(G_SI) = *addr_of_mut!(G_SR) * r1 + *addr_of_mut!(G_SI) * (r1 + 1.0);
                *addr_of_mut!(G_SR) = r2;
                polyev(
                    *addr_of_mut!(NN),
                    *addr_of_mut!(G_SR),
                    *addr_of_mut!(G_SI),
                    *addr_of_mut!(G_PR),
                    *addr_of_mut!(G_PI),
                    *addr_of_mut!(G_QPR),
                    *addr_of_mut!(G_QPI),
                    addr_of_mut!(G_PVR),
                    addr_of_mut!(G_PVI),
                );
                let mut j: c_int = 1;
                while j <= 5 {
                    let mut h_s_0 = false;
                    calct(&mut h_s_0);
                    nexth(h_s_0);
                    j += 1;
                }
                omp = INFIN;
            } else {
                // exit if polynomial value increases significantly
                if mp * 0.1 > omp {
                    return false;
                }
            }
        }
        omp = mp;

        // calculate next iterate
        let mut h_s_0 = false;
        calct(&mut h_s_0);
        nexth(h_s_0);
        calct(&mut h_s_0);
        if !h_s_0 {
            let tr_val = *addr_of_mut!(G_TR);
            let ti_val = *addr_of_mut!(G_TI);
            relstp = tr_val.hypot(ti_val) / (*addr_of_mut!(G_SR)).hypot(*addr_of_mut!(G_SI));
            *addr_of_mut!(G_SR) += tr_val;
            *addr_of_mut!(G_SI) += ti_val;
        }

        i += 1;
    }

    false
}

// ---------------------------------------------------------------------------
// fxshft -- second stage: fixed-shift h polynomials and convergence test
// ---------------------------------------------------------------------------

unsafe fn fxshft(l2: c_int, zr: *mut c_double, zi: *mut c_double) -> bool {
    let n = *addr_of_mut!(NN) - 1;

    // evaluate p at s
    polyev(
        *addr_of_mut!(NN),
        *addr_of_mut!(G_SR),
        *addr_of_mut!(G_SI),
        *addr_of_mut!(G_PR),
        *addr_of_mut!(G_PI),
        *addr_of_mut!(G_QPR),
        *addr_of_mut!(G_QPI),
        addr_of_mut!(G_PVR),
        addr_of_mut!(G_PVI),
    );

    let mut test = true;
    let mut pasd = false;

    // calculate first t = -p(s)/h(s)
    let mut h_s_0 = false;
    calct(&mut h_s_0);

    // main loop for one second stage step
    let mut j: c_int = 1;
    while j <= l2 {
        let otr = *addr_of_mut!(G_TR);
        let oti = *addr_of_mut!(G_TI);

        // compute next h polynomial and new t
        nexth(h_s_0);
        calct(&mut h_s_0);
        *zr = *addr_of_mut!(G_SR) + *addr_of_mut!(G_TR);
        *zi = *addr_of_mut!(G_SI) + *addr_of_mut!(G_TI);

        // test for convergence unless stage 3 has failed once
        if !h_s_0 && test && j != l2 {
            let tr_val = *addr_of_mut!(G_TR);
            let ti_val = *addr_of_mut!(G_TI);
            if (tr_val - otr).hypot(ti_val - oti) >= (*zr).hypot(*zi) * 0.5 {
                pasd = false;
            } else if !pasd {
                pasd = true;
            } else {
                // weak convergence test passed twice, start third stage
                let mut i: c_int = 0;
                while i < n {
                    *(*addr_of_mut!(G_SHR)).offset(i as isize) =
                        *(*addr_of_mut!(G_HR)).offset(i as isize);
                    *(*addr_of_mut!(G_SHI)).offset(i as isize) =
                        *(*addr_of_mut!(G_HI)).offset(i as isize);
                    i += 1;
                }
                let svsr = *addr_of_mut!(G_SR);
                let svsi = *addr_of_mut!(G_SI);
                if vrshft(10, zr, zi) {
                    return true;
                }

                // iteration failed to converge, turn off testing, restore h, s, pv, t
                test = false;
                let mut i: c_int = 1;
                while i <= n {
                    *(*addr_of_mut!(G_HR)).offset((i - 1) as isize) =
                        *(*addr_of_mut!(G_SHR)).offset((i - 1) as isize);
                    *(*addr_of_mut!(G_HI)).offset((i - 1) as isize) =
                        *(*addr_of_mut!(G_SHI)).offset((i - 1) as isize);
                    i += 1;
                }
                *addr_of_mut!(G_SR) = svsr;
                *addr_of_mut!(G_SI) = svsi;
                polyev(
                    *addr_of_mut!(NN),
                    *addr_of_mut!(G_SR),
                    *addr_of_mut!(G_SI),
                    *addr_of_mut!(G_PR),
                    *addr_of_mut!(G_PI),
                    *addr_of_mut!(G_QPR),
                    *addr_of_mut!(G_QPI),
                    addr_of_mut!(G_PVR),
                    addr_of_mut!(G_PVI),
                );
                calct(&mut h_s_0);
            }
        }

        j += 1;
    }

    // attempt an iteration with final h polynomial from second stage
    vrshft(10, zr, zi)
}

// ---------------------------------------------------------------------------
// R_cpolyroot -- main Jenkins-Traub algorithm entry point
// ---------------------------------------------------------------------------

unsafe fn R_cpolyroot(
    opr: *const c_double,
    opi: *const c_double,
    degree: *mut c_int,
    zeror: *mut c_double,
    zeroi: *mut c_double,
    fail: *mut bool,
) {
    let mut xx: c_double = std::f64::consts::FRAC_1_SQRT_2; // M_SQRT1_2 = 1/sqrt(2)
    let mut yy: c_double = -xx;
    *fail = false;

    *addr_of_mut!(NN) = *degree;
    let d1 = *addr_of_mut!(NN) - 1;

    // algorithm fails if the leading coefficient is zero
    if *opr.offset(0) == 0.0 && *opi.offset(0) == 0.0 {
        *fail = true;
        return;
    }

    // remove the zeros at the origin if any
    while *opr.offset(*addr_of_mut!(NN) as isize) == 0.0
        && *opi.offset(*addr_of_mut!(NN) as isize) == 0.0
    {
        let d_n = d1 - *addr_of_mut!(NN) + 1;
        *zeror.offset(d_n as isize) = 0.0;
        *zeroi.offset(d_n as isize) = 0.0;
        *addr_of_mut!(NN) -= 1;
    }
    *addr_of_mut!(NN) += 1;
    // Now, NN = #{coefficients} = (relevant degree) + 1

    if *addr_of_mut!(NN) == 1 {
        return;
    }

    // Allocate temporary arrays
    let nn_val = *addr_of_mut!(NN) as usize;
    let tmp = std::alloc::alloc(std::alloc::Layout::array::<c_double>(10 * nn_val).unwrap())
        as *mut c_double;

    *addr_of_mut!(G_PR) = tmp;
    *addr_of_mut!(G_PI) = tmp.add(nn_val);
    *addr_of_mut!(G_HR) = tmp.add(2 * nn_val);
    *addr_of_mut!(G_HI) = tmp.add(3 * nn_val);
    *addr_of_mut!(G_QPR) = tmp.add(4 * nn_val);
    *addr_of_mut!(G_QPI) = tmp.add(5 * nn_val);
    *addr_of_mut!(G_QHR) = tmp.add(6 * nn_val);
    *addr_of_mut!(G_QHI) = tmp.add(7 * nn_val);
    *addr_of_mut!(G_SHR) = tmp.add(8 * nn_val);
    *addr_of_mut!(G_SHI) = tmp.add(9 * nn_val);

    // make a copy of the coefficients and shr[] = |p[]|
    let mut i: c_int = 0;
    while i < *addr_of_mut!(NN) {
        *(*addr_of_mut!(G_PR)).offset(i as isize) = *opr.offset(i as isize);
        *(*addr_of_mut!(G_PI)).offset(i as isize) = *opi.offset(i as isize);
        *(*addr_of_mut!(G_SHR)).offset(i as isize) =
            (*opr.offset(i as isize)).hypot(*opi.offset(i as isize));
        i += 1;
    }

    // scale the polynomial with factor 'bnd'
    let bnd = cpoly_scale(
        *addr_of_mut!(NN),
        *addr_of_mut!(G_SHR),
        ETA,
        INFIN,
        SMALNO,
        BASE,
    );
    if bnd != 1.0 {
        let mut i: c_int = 0;
        while i < *addr_of_mut!(NN) {
            *(*addr_of_mut!(G_PR)).offset(i as isize) *= bnd;
            *(*addr_of_mut!(G_PI)).offset(i as isize) *= bnd;
            i += 1;
        }
    }

    // start the algorithm for one zero
    while *addr_of_mut!(NN) > 2 {
        // calculate bnd, a lower bound on the modulus of the zeros
        let mut i: c_int = 0;
        while i < *addr_of_mut!(NN) {
            *(*addr_of_mut!(G_SHR)).offset(i as isize) = (*(*addr_of_mut!(G_PR))
                .offset(i as isize))
            .hypot(*(*addr_of_mut!(G_PI)).offset(i as isize));
            i += 1;
        }
        let bnd = cpoly_cauchy(
            *addr_of_mut!(NN),
            *addr_of_mut!(G_SHR),
            *addr_of_mut!(G_SHI),
        );

        // outer loop to control 2 major passes with different sequences of shifts
        let mut i1: c_int = 1;
        'outer: while i1 <= 2 {
            // first stage calculation, no shift
            noshft(5);

            // inner loop to select a shift
            let mut i2: c_int = 1;
            while i2 <= 9 {
                // shift is chosen with modulus bnd and amplitude rotated by 94 deg
                let xxx = COSR * xx - SINR * yy;
                yy = SINR * xx + COSR * yy;
                xx = xxx;
                *addr_of_mut!(G_SR) = bnd * xx;
                *addr_of_mut!(G_SI) = bnd * yy;

                // second stage calculation, fixed shift
                let mut zr: c_double = 0.0;
                let mut zi: c_double = 0.0;
                let conv = fxshft(i2 * 10, &mut zr, &mut zi);
                if conv {
                    // found a zero - store and deflate
                    let d_n = d1 + 2 - *addr_of_mut!(NN);
                    *zeror.offset(d_n as isize) = zr;
                    *zeroi.offset(d_n as isize) = zi;
                    *addr_of_mut!(NN) -= 1;
                    let mut i: c_int = 0;
                    while i < *addr_of_mut!(NN) {
                        *(*addr_of_mut!(G_PR)).offset(i as isize) =
                            *(*addr_of_mut!(G_QPR)).offset(i as isize);
                        *(*addr_of_mut!(G_PI)).offset(i as isize) =
                            *(*addr_of_mut!(G_QPI)).offset(i as isize);
                        i += 1;
                    }
                    continue 'outer; // back to while nn > 2
                }
                i2 += 1;
            }
            i1 += 1;
        }

        // the zerofinder has failed on two major passes
        *fail = true;

        // free tmp
        std::alloc::dealloc(
            tmp as *mut u8,
            std::alloc::Layout::array::<c_double>(10 * nn_val).unwrap(),
        );
        return;
    }

    // calculate the final zero and return
    let (zr_val, zi_val) = cdivid(
        -*(*addr_of_mut!(G_PR)).offset(1),
        -*(*addr_of_mut!(G_PI)).offset(1),
        *(*addr_of_mut!(G_PR)).offset(0),
        *(*addr_of_mut!(G_PI)).offset(0),
    );
    *zeror.offset(d1 as isize) = zr_val;
    *zeroi.offset(d1 as isize) = zi_val;

    // free tmp
    std::alloc::dealloc(
        tmp as *mut u8,
        std::alloc::Layout::array::<c_double>(10 * nn_val).unwrap(),
    );
}

// ---------------------------------------------------------------------------
// do_polyroot -- SEXP interface for polyroot()
// ---------------------------------------------------------------------------

/// polyroot() SEXP interface.
///
/// Ported from lines 798-865 of complex.c.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_polyroot(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let z = CAR(args);

    // coerce to complex
    let z = match TYPEOF(z) {
        t if t == SEXPTYPE::CPLXSXP.0 => {
            Rf_protect(z);
            z
        }
        t if t == SEXPTYPE::REALSXP.0 || t == SEXPTYPE::INTSXP.0 || t == SEXPTYPE::LGLSXP.0 => {
            let coerced = Rf_allocVector3(SEXPTYPE::CPLXSXP.0, XLENGTH(z));
            Rf_protect(coerced);
            // Simple coercion: copy real values, set imaginary to 0
            if TYPEOF(z) == SEXPTYPE::REALSXP.0 {
                let pr = REAL(z);
                let pc = COMPLEX(coerced);
                for i in 0..XLENGTH(z) as usize {
                    (*pc.add(i)).r = *pr.add(i);
                    (*pc.add(i)).i = 0.0;
                }
            } else if TYPEOF(z) == SEXPTYPE::INTSXP.0 {
                let pi = crate::sexp::accessors::INTEGER(z);
                let pc = COMPLEX(coerced);
                for i in 0..XLENGTH(z) as usize {
                    (*pc.add(i)).r = *pi.add(i) as c_double;
                    (*pc.add(i)).i = 0.0;
                }
            }
            coerced
        }
        _ => {
            return R_NilValue();
        }
    };

    let n = LENGTH(z);
    let pz = COMPLEX(z);

    // find degree = max{i; z[i] != 0}
    let mut degree: c_int = 0;
    let mut i: c_int = 0;
    while i < n {
        let pzi = *pz.offset(i as isize);
        if pzi.r != 0.0 || pzi.i != 0.0 {
            degree = i;
        }
        i += 1;
    }
    let nn = degree + 1; // omit trailing zeroes

    if degree >= 1 {
        let rr = Rf_protect(Rf_allocVector3(SEXPTYPE::REALSXP.0, nn as R_xlen_t));
        let ri = Rf_protect(Rf_allocVector3(SEXPTYPE::REALSXP.0, nn as R_xlen_t));
        let zr = Rf_protect(Rf_allocVector3(SEXPTYPE::REALSXP.0, nn as R_xlen_t));
        let zi = Rf_protect(Rf_allocVector3(SEXPTYPE::REALSXP.0, nn as R_xlen_t));

        let p_rr = REAL(rr);
        let p_ri = REAL(ri);
        let p_zr = REAL(zr);
        let p_zi = REAL(zi);

        // reverse coefficients
        let mut i: c_int = 0;
        while i < nn {
            let pzi = *pz.offset(i as isize);
            if !R_FINITE(pzi.r) || !R_FINITE(pzi.i) {
                Rf_unprotect(4);
                Rf_unprotect(1); // z
                return R_NilValue();
            }
            *p_zr.offset((degree - i) as isize) = pzi.r;
            *p_zi.offset((degree - i) as isize) = pzi.i;
            i += 1;
        }

        let mut degree_mut = degree;
        let mut fail = false;
        R_cpolyroot(p_zr, p_zi, &mut degree_mut, p_rr, p_ri, &mut fail);

        Rf_unprotect(2); // zr, zi

        if fail {
            Rf_unprotect(2); // rr, ri
            Rf_unprotect(1); // z
            return R_NilValue();
        }

        let r = Rf_allocVector3(SEXPTYPE::CPLXSXP.0, degree_mut as R_xlen_t);
        let pr = COMPLEX(r);
        let mut i: c_int = 0;
        while i < degree_mut {
            (*pr.offset(i as isize)).r = *p_rr.offset(i as isize);
            (*pr.offset(i as isize)).i = *p_ri.offset(i as isize);
            i += 1;
        }

        Rf_unprotect(3); // rr, ri, z
        r
    } else {
        Rf_unprotect(1); // z
        Rf_allocVector3(SEXPTYPE::CPLXSXP.0, 0)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cdivid_basic() {
        // (3+4i) / (1+2i) = (11-2i)/5 = 2.2-0.4i
        let (cr, ci) = cdivid(3.0, 4.0, 1.0, 2.0);
        assert!((cr - 2.2).abs() < 1e-10);
        assert!((ci - (-0.4)).abs() < 1e-10);
    }

    #[test]
    fn test_cdivid_real() {
        let (cr, ci) = cdivid(6.0, 0.0, 2.0, 0.0);
        assert!((cr - 3.0).abs() < 1e-10);
        assert!(ci.abs() < 1e-10);
    }

    #[test]
    fn test_cdivid_zero() {
        let (cr, ci) = cdivid(1.0, 0.0, 0.0, 0.0);
        assert!(cr.is_infinite());
        assert!(ci.is_infinite());
    }

    #[test]
    fn test_cdivid_by_imaginary() {
        let (cr, ci) = cdivid(0.0, 2.0, 0.0, 1.0);
        assert!((cr - 2.0).abs() < 1e-10);
        assert!(ci.abs() < 1e-10);
    }

    #[test]
    fn test_cpoly_scale_no_scale() {
        let pot = [1.0, 2.0, 3.0];
        let scale = unsafe { cpoly_scale(3, pot.as_ptr(), 1e-10, 1e20, 1e-20, 2.0) };
        assert!((scale - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_cpoly_scale_wide_range() {
        let pot = [1e-30, 1.0, 1e30];
        let scale = unsafe { cpoly_scale(3, pot.as_ptr(), 1e-10, 1e20, 1e-20, 2.0) };
        assert!(scale != 1.0);
    }
}
