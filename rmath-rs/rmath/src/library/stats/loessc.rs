#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_assignments,
    non_camel_case_types,
    unsafe_op_in_unsafe_fn
)]

/*
 * Copyright (C) 1998--2025  The R Core Team
 *
 * The authors of this software are Cleveland, Grosse, and Shyu.
 * Copyright (c) 1989, 1992 by AT&T.
 * Permission to use, copy, modify, and distribute this software for any
 * purpose without fee is hereby granted, provided that this entire notice
 * is included in all copies of any software which is or includes a copy
 * or modification of this software and in all copies of the supporting
 * documentation for such software.
 * THIS SOFTWARE IS BEING PROVIDED "AS IS", WITHOUT ANY EXPRESS OR IMPLIED
 * WARRANTY.  IN PARTICULAR, NEITHER THE AUTHORS NOR AT&T MAKE ANY
 * REPRESENTATION OR WARRANTY OF ANY KIND CONCERNING THE MERCHANTABILITY
 * OF THIS SOFTWARE OR ITS FITNESS FOR A PARTICULAR PURPOSE.
 *
 * Ported from r-source/src/library/stats/src/loessc.c
 */

use std::cmp;
use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_int};

use crate::sexp::ffi::*;

const GAUSSIAN: c_int = 1;
const SYMMETRIC: c_int = 0;

// Global variables (static in C)
static mut IV: *mut c_int = std::ptr::null_mut();
static mut LIV: c_int = 0;
static mut LV: c_int = 0;
static mut TAU: c_int = 0;
static mut V: *mut c_double = std::ptr::null_mut();

fn r_min<T: Ord>(a: T, b: T) -> T {
    if a < b { a } else { b }
}
fn r_max<T: Ord>(a: T, b: T) -> T {
    if a > b { a } else { b }
}

unsafe fn loess_free() {
    if !V.is_null() {
        let _ = Vec::from_raw_parts(V, LV as usize, LV as usize);
        V = std::ptr::null_mut();
    }
    if !IV.is_null() {
        let _ = Vec::from_raw_parts(IV, LIV as usize, LIV as usize);
        IV = std::ptr::null_mut();
    }
}

unsafe extern "C" {
    fn lowesd(
        iv: *mut c_int,
        liv: *mut c_int,
        lv: *mut c_int,
        v: *mut c_double,
        d: *mut c_int,
        n: *mut c_int,
        f: *mut c_double,
        ideg: *mut c_int,
        nf: *mut c_int,
        nvmax: *mut c_int,
        setlf: *mut c_int,
    );
    fn lowesa(
        trL: *mut c_double,
        n: *mut c_int,
        d: *mut c_int,
        tau: *mut c_int,
        nsing: *mut c_int,
        one_delta: *mut c_double,
        two_delta: *mut c_double,
    );
    fn lowesb(
        x: *mut c_double,
        y: *mut c_double,
        robust: *mut c_double,
        diagonal: *mut c_double,
        i1: *mut c_int,
        iv: *mut c_int,
        v: *mut c_double,
    );
    fn lowese(
        iv: *mut c_int,
        v: *mut c_double,
        n: *mut c_int,
        x: *mut c_double,
        surface: *mut c_double,
    );
    fn lowesf(
        x: *mut c_double,
        y: *mut c_double,
        weights: *mut c_double,
        iv: *mut c_int,
        v: *mut c_double,
        m: *mut c_int,
        x_evaluate: *mut c_double,
        diagonal: *mut c_double,
        i2: *mut c_int,
        surface: *mut c_double,
    );
    fn lowesl(iv: *mut c_int, v: *mut c_double, m: *mut c_int, x: *mut c_double, L: *mut c_double);
    fn lowesc(
        n: *mut c_int,
        hat_matrix: *mut c_double,
        LL: *mut c_double,
        trL: *mut c_double,
        one_delta: *mut c_double,
        two_delta: *mut c_double,
    );
    fn ehg169(
        d: *mut c_int,
        vc: *mut c_int,
        nc: *mut c_int,
        nc2: *mut c_int,
        nv: *mut c_int,
        nv2: *mut c_int,
        vert: *mut c_double,
        a: *mut c_int,
        xi: *mut c_double,
        lv1: *mut c_int,
        lv2: *mut c_int,
        lv3: *mut c_int,
    );
    fn ehg196(tau: *mut c_int, d: *mut c_int, span: *mut c_double, trL: *mut c_double);
}

fn R_pow_di(x: c_double, n: c_int) -> c_double {
    crate::nmath::special::mlutils::R_pow_di(x, n)
}

fn strcmp_c(s1: &str, s2: &str) -> bool {
    s1 == s2
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loess_raw(
    y: *mut c_double,
    x: *mut c_double,
    weights: *mut c_double,
    robust: *mut c_double,
    d: *mut c_int,
    n: *mut c_int,
    span: *mut c_double,
    degree: *mut c_int,
    nonparametric: *mut c_int,
    drop_square: *mut c_int,
    sum_drop_sqr: *mut c_int,
    cell: *mut c_double,
    surf_stat: *mut *mut c_char,
    surface: *mut c_double,
    parameter: *mut c_int,
    a: *mut c_int,
    xi: *mut c_double,
    vert: *mut c_double,
    vval: *mut c_double,
    diagonal: *mut c_double,
    trL: *mut c_double,
    one_delta: *mut c_double,
    two_delta: *mut c_double,
    setLf: *mut c_int,
) {
    use crate::main::errors::Rf_error;

    let mut i0: c_int = 0;
    let mut one: c_int = 1;
    let mut two: c_int = 2;
    let mut d0: c_double = 0.0;

    *trL = 0.0;

    loess_workspace(
        *d,
        *n,
        *span,
        *degree,
        *nonparametric,
        drop_square,
        *sum_drop_sqr,
        *setLf != 0,
    );
    *V.add(1) = *cell;

    let surf = CStr::from_ptr(*surf_stat).to_str().unwrap_or("");

    if strcmp_c(surf, "interpolate/none") {
        lowesb(
            x,
            y,
            robust,
            std::ptr::addr_of_mut!(d0),
            std::ptr::addr_of_mut!(i0),
            IV,
            V,
        );
        lowese(IV, V, n, x, surface);
        loess_prune(parameter, a, xi, vert, vval);
    } else if strcmp_c(surf, "direct/none") {
        lowesf(
            x,
            y,
            robust,
            IV,
            V,
            n,
            x,
            std::ptr::addr_of_mut!(d0),
            std::ptr::addr_of_mut!(i0),
            surface,
        );
    } else if strcmp_c(surf, "interpolate/1.approx") {
        lowesb(x, y, weights, diagonal, std::ptr::addr_of_mut!(one), IV, V);
        lowese(IV, V, n, x, surface);
        let mut nsing = *IV.add(29);
        for i in 0..(*n as usize) {
            *trL = *trL + *diagonal.add(i);
        }
        lowesa(
            trL,
            n,
            d,
            std::ptr::addr_of_mut!(TAU),
            std::ptr::addr_of_mut!(nsing),
            one_delta,
            two_delta,
        );
        loess_prune(parameter, a, xi, vert, vval);
    } else if strcmp_c(surf, "interpolate/2.approx") {
        lowesb(
            x,
            y,
            weights,
            std::ptr::addr_of_mut!(d0),
            std::ptr::addr_of_mut!(i0),
            IV,
            V,
        );
        lowese(IV, V, n, x, surface);
        let _nsing = *IV.add(29);
        ehg196(std::ptr::addr_of_mut!(TAU), d, span, trL);
        let mut nsing = *IV.add(29);
        lowesa(
            trL,
            n,
            d,
            std::ptr::addr_of_mut!(TAU),
            std::ptr::addr_of_mut!(nsing),
            one_delta,
            two_delta,
        );
        loess_prune(parameter, a, xi, vert, vval);
    } else if strcmp_c(surf, "direct/approximate") {
        lowesf(
            x,
            y,
            weights,
            IV,
            V,
            n,
            x,
            diagonal,
            std::ptr::addr_of_mut!(one),
            surface,
        );
        let mut nsing = *IV.add(29);
        for i in 0..(*n as usize) {
            *trL = *trL + *diagonal.add(i);
        }
        lowesa(
            trL,
            n,
            d,
            std::ptr::addr_of_mut!(TAU),
            std::ptr::addr_of_mut!(nsing),
            one_delta,
            two_delta,
        );
    } else if strcmp_c(surf, "interpolate/exact") {
        let hat_matrix = vec![0.0f64; (*n as usize) * (*n as usize)];
        let mut ll = vec![0.0f64; (*n as usize) * (*n as usize)];
        lowesb(x, y, weights, diagonal, std::ptr::addr_of_mut!(one), IV, V);
        lowesl(IV, V, n, x, hat_matrix.as_ptr() as *mut c_double);
        lowesc(
            n,
            hat_matrix.as_ptr() as *mut c_double,
            ll.as_mut_ptr(),
            trL,
            one_delta,
            two_delta,
        );
        lowese(IV, V, n, x, surface);
        loess_prune(parameter, a, xi, vert, vval);
    } else if strcmp_c(surf, "direct/exact") {
        let mut hat_matrix = vec![0.0f64; (*n as usize) * (*n as usize)];
        let mut ll = vec![0.0f64; (*n as usize) * (*n as usize)];
        lowesf(
            x,
            y,
            weights,
            IV,
            V,
            n,
            x,
            hat_matrix.as_mut_ptr(),
            std::ptr::addr_of_mut!(two),
            surface,
        );
        lowesc(
            n,
            hat_matrix.as_mut_ptr(),
            ll.as_mut_ptr(),
            trL,
            one_delta,
            two_delta,
        );
        let k = (*n + 1) as usize;
        for i in 0..(*n as usize) {
            *diagonal.add(i) = *hat_matrix.as_ptr().add(i * k);
        }
    } else {
        Rf_error(b"invalid surface statistic type\0".as_ptr() as *const i8);
    }
    loess_free();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loess_dfit(
    y: *mut c_double,
    x: *mut c_double,
    x_evaluate: *mut c_double,
    weights: *mut c_double,
    span: *mut c_double,
    degree: *mut c_int,
    nonparametric: *mut c_int,
    drop_square: *mut c_int,
    sum_drop_sqr: *mut c_int,
    d: *mut c_int,
    n: *mut c_int,
    m: *mut c_int,
    fit: *mut c_double,
) {
    let mut i0: c_int = 0;
    let mut d0: c_double = 0.0;

    loess_workspace(
        *d,
        *n,
        *span,
        *degree,
        *nonparametric,
        drop_square,
        *sum_drop_sqr,
        false,
    );
    lowesf(
        x,
        y,
        weights,
        IV,
        V,
        m,
        x_evaluate,
        std::ptr::addr_of_mut!(d0),
        std::ptr::addr_of_mut!(i0),
        fit,
    );
    loess_free();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loess_dfitse(
    y: *mut c_double,
    x: *mut c_double,
    x_evaluate: *mut c_double,
    weights: *mut c_double,
    robust: *mut c_double,
    family: *mut c_int,
    span: *mut c_double,
    degree: *mut c_int,
    nonparametric: *mut c_int,
    drop_square: *mut c_int,
    sum_drop_sqr: *mut c_int,
    d: *mut c_int,
    n: *mut c_int,
    m: *mut c_int,
    fit: *mut c_double,
    L: *mut c_double,
) {
    loess_workspace(
        *d,
        *n,
        *span,
        *degree,
        *nonparametric,
        drop_square,
        *sum_drop_sqr,
        false,
    );

    let mut i2: c_int = 2;
    if *family == GAUSSIAN {
        lowesf(
            x,
            y,
            weights,
            IV,
            V,
            m,
            x_evaluate,
            L,
            std::ptr::addr_of_mut!(i2),
            fit,
        );
    } else if *family == SYMMETRIC {
        let mut i0: c_int = 0;
        let mut d0: c_double = 0.0;
        lowesf(
            x,
            y,
            weights,
            IV,
            V,
            m,
            x_evaluate,
            L,
            std::ptr::addr_of_mut!(i2),
            fit,
        );
        lowesf(
            x,
            y,
            robust,
            IV,
            V,
            m,
            x_evaluate,
            std::ptr::addr_of_mut!(d0),
            std::ptr::addr_of_mut!(i0),
            fit,
        );
    }
    loess_free();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loess_ifit(
    parameter: *mut c_int,
    a: *mut c_int,
    xi: *mut c_double,
    vert: *mut c_double,
    vval: *mut c_double,
    m: *mut c_int,
    x_evaluate: *mut c_double,
    fit: *mut c_double,
) {
    loess_grow(parameter, a, xi, vert, vval);
    lowese(IV, V, m, x_evaluate, fit);
    loess_free();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loess_ise(
    y: *mut c_double,
    x: *mut c_double,
    x_evaluate: *mut c_double,
    weights: *mut c_double,
    span: *mut c_double,
    degree: *mut c_int,
    nonparametric: *mut c_int,
    drop_square: *mut c_int,
    sum_drop_sqr: *mut c_int,
    cell: *mut c_double,
    d: *mut c_int,
    n: *mut c_int,
    m: *mut c_int,
    fit: *mut c_double,
    L: *mut c_double,
) {
    loess_workspace(
        *d,
        *n,
        *span,
        *degree,
        *nonparametric,
        drop_square,
        *sum_drop_sqr,
        true,
    );

    let mut i0: c_int = 0;
    let mut d0: c_double = 0.0;
    *V.add(1) = *cell;
    lowesb(
        x,
        y,
        weights,
        std::ptr::addr_of_mut!(d0),
        std::ptr::addr_of_mut!(i0),
        IV,
        V,
    );
    lowesl(IV, V, m, x_evaluate, L);
    loess_free();
}

/// Set global variables tau, lv, liv, and allocate global arrays v[1..lv], iv[1..liv]
unsafe fn loess_workspace(
    d: c_int,
    n: c_int,
    span: c_double,
    degree: c_int,
    nonparametric: c_int,
    drop_square: *const c_int,
    sum_drop_sqr: c_int,
    set_lf: bool,
) {
    use crate::main::errors::Rf_error;

    let nvmax = r_max(200, n);
    let nf = r_min(n, (n as f64 * span + 1e-5).floor() as c_int);
    if nf <= 0 {
        Rf_error(b"span is too small\0".as_ptr() as *const i8);
    }

    let tau0 = if degree > 1 {
        ((d + 2) * (d + 1)) / 2
    } else {
        d + 1
    };
    TAU = tau0 - sum_drop_sqr;

    let dlv = 50.0 + (3 * d + 3) as f64 * nvmax as f64 + n as f64 + (tau0 as f64 + 2.0) * nf as f64;
    let mut dliv = 50.0 + (R_pow_di(2.0, d) + 4.0) * nvmax as f64 + 2.0 * n as f64;

    if set_lf {
        // dlv is f64, we need to compute with it
        let dlv_extra = (d + 1) as f64 * nf as f64 * nvmax as f64;
        let dliv_extra = nf as f64 * nvmax as f64;
        let total_dlv = dlv + dlv_extra;
        let total_dliv = dliv + dliv_extra;

        if total_dlv < c_int::MAX as f64 && total_dliv < c_int::MAX as f64 {
            LV = total_dlv as c_int;
            LIV = total_dliv as c_int;
        } else {
            Rf_error(b"workspace required is too large\0".as_ptr() as *const i8);
        }
    } else {
        if dlv < c_int::MAX as f64 && dliv < c_int::MAX as f64 {
            LV = dlv as c_int;
            LIV = dliv as c_int;
        } else {
            Rf_error(b"workspace required is too large\0".as_ptr() as *const i8);
        }
    }

    let mut iv_vec = vec![0i32; LIV as usize];
    let mut v_vec = vec![0.0f64; LV as usize];
    IV = iv_vec.as_mut_ptr();
    V = v_vec.as_mut_ptr();
    std::mem::forget(iv_vec);
    std::mem::forget(v_vec);

    let mut iset_lf = if set_lf { 1 } else { 0 };
    let mut d_out = d;
    let mut n_out = n;
    let mut span_out = span;
    let mut degree_out = degree;
    let mut nf_out = nf;
    let mut nvmax_out = nvmax;
    lowesd(
        IV,
        std::ptr::addr_of_mut!(LIV),
        std::ptr::addr_of_mut!(LV),
        V,
        std::ptr::addr_of_mut!(d_out),
        std::ptr::addr_of_mut!(n_out),
        std::ptr::addr_of_mut!(span_out),
        std::ptr::addr_of_mut!(degree_out),
        std::ptr::addr_of_mut!(nf_out),
        std::ptr::addr_of_mut!(nvmax_out),
        std::ptr::addr_of_mut!(iset_lf),
    );
    *IV.add(32) = nonparametric;
    for i in 0..(d as usize) {
        *IV.add(40 + i) = *drop_square.add(i);
    }
}

unsafe fn loess_prune(
    parameter: *mut c_int,
    a: *mut c_int,
    xi: *mut c_double,
    vert: *mut c_double,
    vval: *mut c_double,
) {
    let d = *IV.add(1);
    let vc = *IV.add(3) - 1;
    let nc = *IV.add(4);
    let nv = *IV.add(5);
    let a1 = *IV.add(6) - 1;
    let v1 = *IV.add(10) - 1;
    let xi1 = *IV.add(11) - 1;
    let vv1 = *IV.add(12) - 1;
    let nvmax = *IV.add(13);

    for i in 0..5 {
        *parameter.add(i) = *IV.add(1 + i);
    }
    *parameter.add(5) = *IV.add(21) - 1;
    *parameter.add(6) = *IV.add(14) - 1;

    for i in 0..(d as usize) {
        let k = nvmax as usize * i;
        *vert.add(i) = *V.add((v1 + k as c_int) as usize);
        *vert.add(i + d as usize) = *V.add((v1 + vc + k as c_int) as usize);
    }
    for i in 0..(nc as usize) {
        *xi.add(i) = *V.add(xi1 as usize + i);
        *a.add(i) = *IV.add(a1 as usize + i);
    }
    let k = (d + 1) * nv;
    for i in 0..(k as usize) {
        *vval.add(i) = *V.add(vv1 as usize + i);
    }
}

unsafe fn loess_grow(
    parameter: *mut c_int,
    a: *mut c_int,
    xi: *mut c_double,
    vert: *mut c_double,
    vval: *mut c_double,
) {
    let mut d = *parameter.add(0);
    let mut vc = *parameter.add(2);
    let mut nc = *parameter.add(3);
    let mut nv = *parameter.add(4);
    LIV = *parameter.add(5);
    LV = *parameter.add(6);

    let mut iv_vec = vec![0i32; LIV as usize];
    let mut v_vec = vec![0.0f64; LV as usize];
    IV = iv_vec.as_mut_ptr();
    V = v_vec.as_mut_ptr();
    std::mem::forget(iv_vec);
    std::mem::forget(v_vec);

    *IV.add(1) = d;
    *IV.add(2) = *parameter.add(1);
    *IV.add(3) = vc;
    *IV.add(5) = nv;
    *IV.add(13) = nv;
    *IV.add(4) = nc;
    *IV.add(16) = nc;
    *IV.add(6) = 50;
    *IV.add(7) = 50 + nc;
    *IV.add(8) = 50 + nc + vc * nc;
    *IV.add(9) = 50 + nc + vc * nc + nc;
    *IV.add(10) = 50;
    *IV.add(12) = 50 + nv * d;
    *IV.add(11) = 50 + nv * d + (d + 1) * nv;
    *IV.add(27) = 173;

    let v1 = *IV.add(10) - 1;
    let xi1 = *IV.add(11) - 1;
    let a1 = *IV.add(6) - 1;
    let vv1 = *IV.add(12) - 1;

    for i in 0..(d as usize) {
        let k = nv as usize * i;
        *V.add((v1 + k as c_int) as usize) = *vert.add(i);
        *V.add((v1 + vc - 1 + k as c_int) as usize) = *vert.add(i + d as usize);
    }
    for i in 0..(nc as usize) {
        *V.add(xi1 as usize + i) = *xi.add(i);
        *IV.add(a1 as usize + i) = *a.add(i);
    }
    let k = (d + 1) * nv;
    for i in 0..(k as usize) {
        *V.add(vv1 as usize + i) = *vval.add(i);
    }

    ehg169(
        std::ptr::addr_of_mut!(d),
        std::ptr::addr_of_mut!(vc),
        std::ptr::addr_of_mut!(nc),
        std::ptr::addr_of_mut!(nc),
        std::ptr::addr_of_mut!(nv),
        std::ptr::addr_of_mut!(nv),
        V.add(v1 as usize),
        IV.add(a1 as usize),
        V.add(xi1 as usize),
        IV.add(*IV.add(7) as usize - 1),
        IV.add(*IV.add(8) as usize - 1),
        IV.add(*IV.add(9) as usize - 1),
    );
}

/* begin ehg's FORTRAN-callable C-codes */

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loesswarn(i: *mut c_int) {
    let msg = match *i {
        100 => "wrong version number in lowesd.   Probably typo in caller.",
        101 => "d>dMAX in ehg131.  Need to recompile with increased dimensions.",
        102 => "liv too small.    (Discovered by lowesd)",
        103 => "lv too small.     (Discovered by lowesd)",
        104 => "span too small.   fewer data values than degrees of freedom.",
        105 => "k>d2MAX in ehg136.  Need to recompile with increased dimensions.",
        106 => "lwork too small",
        107 => "invalid value for kernel",
        108 => "invalid value for ideg",
        109 => "lowstt only applies when kernel=1.",
        110 => "not enough extra workspace for robustness calculation",
        120 => "zero-width neighborhood. make span bigger",
        121 => "all data on boundary of neighborhood. make span bigger",
        122 => "extrapolation not allowed with blending",
        123 => "ihat=1 (diag L) in l2fit only makes sense if z=x (eval=data).",
        171 => "lowesd must be called first.",
        172 => "lowesf must not come between lowesb and lowese, lowesr, or lowesl.",
        173 => "lowesb must come before lowese, lowesr, or lowesl.",
        174 => "lowesb need not be called twice.",
        175 => "need setLf=.true. for lowesl.",
        180 => "nv>nvmax in cpvert.",
        181 => "nt>20 in eval.",
        182 => "svddc failed in l2fit.",
        183 => "didn't find edge in vleaf.",
        184 => "zero-width cell found in vleaf.",
        185 => "trouble descending to leaf in vleaf.",
        186 => "insufficient workspace for lowesf.",
        187 => "insufficient stack space",
        188 => "lv too small for computing explicit L",
        191 => "computed trace L was negative; something is wrong!",
        192 => "computed delta was negative; something is wrong!",
        193 => "workspace in loread appears to be corrupted",
        194 => "trouble in l2fit/l2tr",
        195 => "only constant, linear, or quadratic local models allowed",
        196 => "degree must be at least 1 for vertex influence matrix",
        999 => "not yet implemented",
        _ => {
            // snprintf(msg2, 50, "Assert failed; error code %d\n", *i);
            "Assert failed"
        }
    };
    crate::main::errors::Rf_warning(format!("{}\0", msg).as_ptr() as *const i8);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ehg183a(
    s: *mut c_char,
    nc: *mut c_int,
    i: *mut c_int,
    n: *mut c_int,
    inc: *mut c_int,
) {
    let nnc = *nc as usize;
    let s_slice = std::slice::from_raw_parts(s as *const u8, nnc);
    let s_str = std::str::from_utf8_unchecked(s_slice);
    let mut mess = String::with_capacity(4000);
    mess.push_str(s_str);
    for j in 0..(*n as usize) {
        mess.push_str(&format!(" {}", *i.add(j * (*inc as usize))));
    }
    mess.push('\n');
    crate::main::errors::Rf_warning(format!("{}\0", mess).as_ptr() as *const i8);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ehg184a(
    s: *mut c_char,
    nc: *mut c_int,
    x: *mut c_double,
    n: *mut c_int,
    inc: *mut c_int,
) {
    let nnc = *nc as usize;
    let s_slice = std::slice::from_raw_parts(s as *const u8, nnc);
    let s_str = std::str::from_utf8_unchecked(s_slice);
    let mut mess = String::with_capacity(4000);
    mess.push_str(s_str);
    for j in 0..(*n as usize) {
        mess.push_str(&format!(" {:.5}", *x.add(j * (*inc as usize))));
    }
    mess.push('\n');
    crate::main::errors::Rf_warning(format!("{}\0", mess).as_ptr() as *const i8);
}
