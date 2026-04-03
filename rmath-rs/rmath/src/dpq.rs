// dpq.h macros translated to inline Rust functions.
// Each takes explicit bool parameters matching C's int (0/1) parameters.

use libm::*;

use crate::constants::*;

const M_LN2: f64 = 0.693147180559945309417232121458;

// R_D__0 : "0"
#[inline(always)]
pub fn r_d__0(log_p: bool) -> f64 {
    if log_p { ML_NEGINF } else { 0.0 }
}

// R_D__1 : "1"
#[inline(always)]
pub fn r_d__1(log_p: bool) -> f64 {
    if log_p { 0.0 } else { 1.0 }
}

// R_DT_0 : "0" (tail-aware)
#[inline(always)]
pub fn r_dt_0(lower_tail: bool, log_p: bool) -> f64 {
    if lower_tail {
        r_d__0(log_p)
    } else {
        r_d__1(log_p)
    }
}

// R_DT_1 : "1" (tail-aware)
#[inline(always)]
pub fn r_dt_1(lower_tail: bool, log_p: bool) -> f64 {
    if lower_tail {
        r_d__1(log_p)
    } else {
        r_d__0(log_p)
    }
}

// R_D_half : 1/2
#[inline(always)]
pub fn r_d_half(log_p: bool) -> f64 {
    if log_p { -M_LN2 } else { 0.5 }
}

// R_D_Lval(p) : p
#[inline(always)]
pub fn r_d_lval(p: f64, lower_tail: bool) -> f64 {
    if lower_tail { p } else { 0.5 - p + 0.5 }
}

// R_D_Cval(p) : 1 - p
#[inline(always)]
pub fn r_d_cval(p: f64, lower_tail: bool) -> f64 {
    if lower_tail { 0.5 - p + 0.5 } else { p }
}

// R_D_val(x) : x in pF(x,..)
#[inline(always)]
pub fn r_d_val(x: f64, log_p: bool) -> f64 {
    if log_p { log(x) } else { x }
}

// R_D_qIv(p) : p in qF(p,..)
#[inline(always)]
pub fn r_d_qiv(p: f64, log_p: bool) -> f64 {
    if log_p { exp(p) } else { p }
}

// R_D_exp(x) : exp(x)
#[inline(always)]
pub fn r_d_exp(x: f64, log_p: bool) -> f64 {
    if log_p { x } else { exp(x) }
}

// R_D_log(p) : log(p)
#[inline(always)]
pub fn r_d_log(p: f64, log_p: bool) -> f64 {
    if log_p { p } else { log(p) }
}

// R_D_Clog(p) : [log](1-p)
#[inline(always)]
pub fn r_d_clog(p: f64, log_p: bool) -> f64 {
    if log_p { log1p(-p) } else { 0.5 - p + 0.5 }
}

// R_Log1_Exp(x) : log(1 - exp(x)) in more stable form
#[inline(always)]
pub fn r_log1_exp(x: f64) -> f64 {
    if x > -M_LN2 {
        log(-expm1(x))
    } else {
        log1p(-exp(x))
    }
}

// R_D_LExp(x) : log(1-exp(x)) tail-aware
#[inline(always)]
pub fn r_d_lexp(x: f64, log_p: bool) -> f64 {
    if log_p { r_log1_exp(x) } else { log1p(-x) }
}

// R_DT_val(x) : x in pF (tail-aware)
#[inline(always)]
pub fn r_dt_val(x: f64, lower_tail: bool, log_p: bool) -> f64 {
    if lower_tail {
        r_d_val(x, log_p)
    } else {
        r_d_clog(x, log_p)
    }
}

// R_DT_Cval(x) : 1-x in pF (tail-aware)
#[inline(always)]
pub fn r_dt_cval(x: f64, lower_tail: bool, log_p: bool) -> f64 {
    if lower_tail {
        r_d_clog(x, log_p)
    } else {
        r_d_val(x, log_p)
    }
}

// R_DT_qIv(p) : p in qF (tail-aware)
#[inline(always)]
pub fn r_dt_qiv(p: f64, lower_tail: bool, log_p: bool) -> f64 {
    if log_p {
        if lower_tail { exp(p) } else { -expm1(p) }
    } else {
        r_d_lval(p, lower_tail)
    }
}

// R_DT_CIv(p) : 1-p in qF (tail-aware)
#[inline(always)]
pub fn r_dt_civ(p: f64, lower_tail: bool, log_p: bool) -> f64 {
    if log_p {
        if lower_tail { -expm1(p) } else { exp(p) }
    } else {
        r_d_cval(p, lower_tail)
    }
}

// R_DT_exp(x) : exp(x) (tail-aware)
#[inline(always)]
pub fn r_dt_exp(x: f64, lower_tail: bool, log_p: bool) -> f64 {
    r_d_exp(r_d_lval(x, lower_tail), log_p)
}

// R_DT_Cexp(x) : exp(1-x) (tail-aware)
#[inline(always)]
pub fn r_dt_cexp(x: f64, lower_tail: bool, log_p: bool) -> f64 {
    r_d_exp(r_d_cval(x, lower_tail), log_p)
}

// R_DT_log(p) : log(p) in qF (tail-aware)
#[inline(always)]
pub fn r_dt_log(p: f64, lower_tail: bool, log_p: bool) -> f64 {
    if lower_tail {
        r_d_log(p, log_p)
    } else {
        r_d_lexp(p, log_p)
    }
}

// R_DT_Clog(p) : log(1-p) in qF (tail-aware)
#[inline(always)]
pub fn r_dt_clog(p: f64, lower_tail: bool, log_p: bool) -> f64 {
    if lower_tail {
        r_d_lexp(p, log_p)
    } else {
        r_d_log(p, log_p)
    }
}

// R_DT_Log(p) : log(p) when we know log_p == TRUE (tail-aware)
#[inline(always)]
pub fn r_dt_log_known(p: f64, lower_tail: bool) -> f64 {
    if lower_tail { p } else { r_log1_exp(p) }
}
