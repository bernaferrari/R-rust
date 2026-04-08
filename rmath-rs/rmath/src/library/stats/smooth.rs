#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    non_camel_case_types,
    unsafe_op_in_unsafe_fn
)]

//! Tukey Median Smoothing
//! Port of r-source/src/library/stats/src/smooth.c

use std::os::raw::{c_double, c_int};

use crate::attrib_core::setAttrib;
use crate::main::coerce::asInteger;
use crate::main::errors::Rf_error;
use crate::main::relop::R_NamesSymbol;
use crate::sexp::accessors::{LENGTH, REAL, SET_STRING_ELT, SET_VECTOR_ELT, TYPEOF, XLENGTH};
use crate::sexp::constructors::{Rf_ScalarInteger, Rf_ScalarLogical, Rf_allocVector, Rf_mkChar};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

const sm_NO_ENDRULE: c_int = 0;
const sm_COPY_ENDRULE: c_int = 1;
const sm_TUKEY_ENDRULE: c_int = 2;

unsafe fn med3(u: c_double, v: c_double, w: c_double) -> c_double {
    if (u <= v && v <= w) || (u >= v && v >= w) {
        return v;
    }
    if (u <= w && w <= v) || (u >= w && w >= v) {
        return w;
    }
    u
}

unsafe fn imed3(u: c_double, v: c_double, w: c_double) -> c_int {
    if (u <= v && v <= w) || (u >= v && v >= w) {
        return 0;
    }
    if (u <= w && w <= v) || (u >= w && w >= v) {
        return 1;
    }
    -1
}

/// Apply end-rule to y[]. Returns updated chg.
unsafe fn sm_do_endrule(
    x: *const c_double,
    y: *mut c_double,
    n: R_xlen_t,
    end_rule: c_int,
    chg: &mut bool,
) {
    match end_rule {
        sm_NO_ENDRULE => {
            // do nothing
        }
        sm_COPY_ENDRULE => {
            *y.add(0) = *x.add(0);
            *y.add((n - 1) as usize) = *x.add((n - 1) as usize);
        }
        sm_TUKEY_ENDRULE => {
            let y0 = med3(3.0 * *y.add(1) - 2.0 * *y.add(2), *x.add(0), *y.add(1));
            *y.add(0) = y0;
            *chg = *chg || (y0 != *x.add(0));
            let yn = med3(
                *y.add((n - 2) as usize),
                *x.add((n - 1) as usize),
                3.0 * *y.add((n - 2) as usize) - 2.0 * *y.add((n - 3) as usize),
            );
            *y.add((n - 1) as usize) = yn;
            *chg = *chg || (yn != *x.add((n - 1) as usize));
        }
        _ => {
            let msg = std::ffi::CString::new(format!(
                "invalid end-rule for running median of 3: {}",
                end_rule
            ))
            .unwrap_or_default();
            Rf_error(msg.as_ptr());
        }
    }
}

unsafe fn sm_3(x: *const c_double, y: *mut c_double, n: R_xlen_t, end_rule: c_int) -> bool {
    if n <= 2 {
        for i in 0..n {
            *y.add(i as usize) = *x.add(i as usize);
        }
        return false;
    }

    let mut chg = false;
    let mut i: R_xlen_t = 1;
    while i < n - 1 {
        let j = imed3(
            *x.add((i - 1) as usize),
            *x.add(i as usize),
            *x.add((i + 1) as usize),
        );
        *y.add(i as usize) = *x.add((i + j as R_xlen_t) as usize);
        chg = chg || (j != 0);
        i += 1;
    }

    sm_do_endrule(x, y, n, end_rule, &mut chg);
    chg
}

unsafe fn sm_3R(
    x: *const c_double,
    y: *mut c_double,
    z: *mut c_double,
    n: R_xlen_t,
    end_rule: c_int,
) -> c_int {
    let chg0 = sm_3(x, y, n, sm_COPY_ENDRULE);
    let mut iter: c_int = if chg0 { 1 } else { 0 };
    let mut chg = chg0;

    while chg {
        let chg2 = sm_3(y, z, n, sm_NO_ENDRULE);
        chg = chg2;
        if chg {
            iter += 1;
            let mut i: R_xlen_t = 1;
            while i < n - 1 {
                *y.add(i as usize) = *z.add(i as usize);
                i += 1;
            }
        }
    }

    if n > 2 {
        let mut dummy_chg = false;
        sm_do_endrule(x, y, n, end_rule, &mut dummy_chg);
    }

    if iter != 0 {
        iter
    } else if chg0 {
        1
    } else {
        0
    }
}

unsafe fn sptest(x: *const c_double, i: R_xlen_t) -> bool {
    if *x.add(i as usize) != *x.add((i + 1) as usize) {
        return false;
    }
    if (*x.add((i - 1) as usize) <= *x.add(i as usize)
        && *x.add((i + 1) as usize) <= *x.add((i + 2) as usize))
        || (*x.add((i - 1) as usize) >= *x.add(i as usize)
            && *x.add((i + 1) as usize) >= *x.add((i + 2) as usize))
    {
        return false;
    }
    true
}

unsafe fn sm_split3(x: *const c_double, y: *mut c_double, n: R_xlen_t, do_ends: bool) -> bool {
    let mut i: R_xlen_t = 0;
    while i < n {
        *y.add(i as usize) = *x.add(i as usize);
        i += 1;
    }
    if n <= 4 {
        return false;
    }

    let mut chg = false;

    if do_ends && sptest(x, 1) {
        chg = true;
        *y.add(1) = *x.add(0);
        *y.add(2) = med3(*x.add(2), *x.add(3), 3.0 * *x.add(3) - 2.0 * *x.add(4));
    }

    let mut i: R_xlen_t = 2;
    while i < n - 3 {
        if sptest(x, i) {
            // plateau at x[i] == x[i+1]
            // at left
            let j = imed3(
                *x.add(i as usize),
                *x.add((i - 1) as usize),
                3.0 * *x.add((i - 1) as usize) - 2.0 * *x.add((i - 2) as usize),
            );
            if j > -1 {
                let val = if j == 0 {
                    *x.add((i - 1) as usize)
                } else {
                    3.0 * *x.add((i - 1) as usize) - 2.0 * *x.add((i - 2) as usize)
                };
                *y.add(i as usize) = val;
                chg = chg || (val != *x.add(i as usize));
            }
            // at right
            let j2 = imed3(
                *x.add((i + 1) as usize),
                *x.add((i + 2) as usize),
                3.0 * *x.add((i + 2) as usize) - 2.0 * *x.add((i + 3) as usize),
            );
            if j2 > -1 {
                let val = if j2 == 0 {
                    *x.add((i + 2) as usize)
                } else {
                    3.0 * *x.add((i + 2) as usize) - 2.0 * *x.add((i + 3) as usize)
                };
                *y.add((i + 1) as usize) = val;
                chg = chg || (val != *x.add((i + 1) as usize));
            }
        }
        i += 1;
    }

    if do_ends && sptest(x, n - 3) {
        chg = true;
        *y.add((n - 2) as usize) = *x.add((n - 1) as usize);
        *y.add((n - 3) as usize) = med3(
            *x.add((n - 3) as usize),
            *x.add((n - 4) as usize),
            3.0 * *x.add((n - 4) as usize) - 2.0 * *x.add((n - 5) as usize),
        );
    }

    chg
}

unsafe fn sm_3RS3R(
    x: *const c_double,
    y: *mut c_double,
    z: *mut c_double,
    w: *mut c_double,
    n: R_xlen_t,
    end_rule: c_int,
    split_ends: bool,
) -> c_int {
    let iter = sm_3R(x, y, z, n, end_rule);
    let chg = sm_split3(y, z, n, split_ends);
    if chg {
        iter + sm_3R(z, y, w, n, end_rule) + 1
    } else {
        iter + (chg as c_int)
    }
}

unsafe fn sm_3RSS(
    x: *const c_double,
    y: *mut c_double,
    z: *mut c_double,
    n: R_xlen_t,
    end_rule: c_int,
    split_ends: bool,
) -> c_int {
    let iter = sm_3R(x, y, z, n, end_rule);
    let chg = sm_split3(y, z, n, split_ends);
    if chg {
        sm_split3(z, y, n, split_ends);
    }
    iter + (chg as c_int)
}

unsafe fn sm_3RSR(
    x: *const c_double,
    y: *mut c_double,
    z: *mut c_double,
    w: *mut c_double,
    n: R_xlen_t,
    end_rule: c_int,
    split_ends: bool,
) -> c_int {
    let mut iter = sm_3R(x, y, z, n, end_rule);

    loop {
        iter += 1;
        let mut chg = sm_split3(y, z, n, split_ends);
        let ch2 = sm_3R(z, y, w, n, end_rule) != 0;
        chg = chg || ch2;

        if !chg {
            break;
        }
        if iter > 2 * (n as c_int) {
            break;
        }
        let mut i: R_xlen_t = 0;
        while i < n {
            *z.add(i as usize) = *x.add(i as usize) - *y.add(i as usize);
            i += 1;
        }
    }

    iter
}

pub unsafe fn Rsm(x: SEXP, stype: SEXP, send: SEXP) -> SEXP {
    let iend = asInteger(send);
    let type_ = asInteger(stype);
    let n = XLENGTH(x);

    let ans = Rf_protect(Rf_allocVector(SEXPTYPE::VECSXP.0, n as c_int));
    let y = Rf_allocVector(SEXPTYPE::REALSXP.0, n as c_int);
    SET_VECTOR_ELT(ans, 0, y);
    let nm = Rf_allocVector(SEXPTYPE::STRSXP.0, 2);
    setAttrib(ans, R_NamesSymbol(), nm);
    SET_STRING_ELT(nm, 0, Rf_mkChar(b"y\0".as_ptr() as *const _));

    if type_ <= 5 {
        let mut iter: c_int = 0;
        match type_ {
            1 => {
                let mut z = vec![0.0f64; n as usize];
                let mut w = vec![0.0f64; n as usize];
                iter = sm_3RS3R(
                    REAL(x),
                    REAL(y),
                    z.as_mut_ptr(),
                    w.as_mut_ptr(),
                    n,
                    iend.abs(),
                    iend < 0,
                );
            }
            2 => {
                let mut z = vec![0.0f64; n as usize];
                iter = sm_3RSS(REAL(x), REAL(y), z.as_mut_ptr(), n, iend.abs(), iend < 0);
            }
            3 => {
                let mut z = vec![0.0f64; n as usize];
                let mut w = vec![0.0f64; n as usize];
                iter = sm_3RSR(
                    REAL(x),
                    REAL(y),
                    z.as_mut_ptr(),
                    w.as_mut_ptr(),
                    n,
                    iend.abs(),
                    iend < 0,
                );
            }
            4 => {
                // "3R"
                let mut z = vec![0.0f64; n as usize];
                iter = sm_3R(REAL(x), REAL(y), z.as_mut_ptr(), n, iend);
            }
            5 => {
                // "3"
                let chg = sm_3(REAL(x), REAL(y), n, iend);
                iter = if chg { 1 } else { 0 };
            }
            _ => {}
        }
        SET_VECTOR_ELT(ans, 1, Rf_ScalarInteger(iter));
        SET_STRING_ELT(nm, 1, Rf_mkChar(b"iter\0".as_ptr() as *const _));
    } else {
        // type > 5: ~ "S"
        let changed = sm_split3(REAL(x), REAL(y), n, iend != 0);
        SET_VECTOR_ELT(ans, 1, Rf_ScalarLogical(if changed { 1 } else { 0 }));
        SET_STRING_ELT(nm, 1, Rf_mkChar(b"changed\0".as_ptr() as *const _));
    }

    Rf_unprotect(1);
    ans
}
