//! Tukey Median Smoothing
//! Port of r-source/src/library/stats/src/smooth.c

use std::os::raw::{c_double, c_int};

use crate::attrib_core::setAttrib;
use crate::main::coerce::asInteger;
use crate::main::errors::{Rf_error, Rf_error_cstr};
use crate::main::relop::R_NamesSymbol;
use crate::sexp::accessors::{
    INTEGER_ELT, LENGTH, REAL_ELT, SET_REAL_ELT, SET_STRING_ELT, SET_VECTOR_ELT, TYPEOF, XLENGTH,
};
use crate::sexp::constructors::{Rf_ScalarInteger, Rf_ScalarLogical, Rf_allocVector};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::protect::protect;

const sm_NO_ENDRULE: c_int = 0;
const sm_COPY_ENDRULE: c_int = 1;
const sm_TUKEY_ENDRULE: c_int = 2;

fn med3(u: c_double, v: c_double, w: c_double) -> c_double {
    if (u <= v && v <= w) || (u >= v && v >= w) {
        return v;
    }
    if (u <= w && w <= v) || (u >= w && w >= v) {
        return w;
    }
    u
}

fn imed3(u: c_double, v: c_double, w: c_double) -> c_int {
    if (u <= v && v <= w) || (u >= v && v >= w) {
        return 0;
    }
    if (u <= w && w <= v) || (u >= w && w >= v) {
        return 1;
    }
    -1
}

/// Apply end-rule to y[]. Returns updated chg.
fn sm_do_endrule(x: &[c_double], y: &mut [c_double], n: R_xlen_t, end_rule: c_int, chg: &mut bool) {
    match end_rule {
        sm_NO_ENDRULE => {
            // do nothing
        }
        sm_COPY_ENDRULE => {
            y[0] = x[0];
            y[(n - 1) as usize] = x[(n - 1) as usize];
        }
        sm_TUKEY_ENDRULE => {
            let y0 = med3(3.0 * y[1] - 2.0 * y[2], x[0], y[1]);
            y[0] = y0;
            *chg = *chg || (y0 != x[0]);
            let yn = med3(
                y[(n - 2) as usize],
                x[(n - 1) as usize],
                3.0 * y[(n - 2) as usize] - 2.0 * y[(n - 3) as usize],
            );
            y[(n - 1) as usize] = yn;
            *chg = *chg || (yn != x[(n - 1) as usize]);
        }
        _ => {
            let msg = std::ffi::CString::new(format!(
                "invalid end-rule for running median of 3: {}",
                end_rule
            ))
            .unwrap_or_default();
            Rf_error_cstr(&msg);
        }
    }
}

fn sm_3(x: &[c_double], y: &mut [c_double], n: R_xlen_t, end_rule: c_int) -> bool {
    if n <= 2 {
        for i in 0..n {
            y[i as usize] = x[i as usize];
        }
        return false;
    }

    let mut chg = false;
    let mut i: R_xlen_t = 1;
    while i < n - 1 {
        let j = imed3(x[(i - 1) as usize], x[i as usize], x[(i + 1) as usize]);
        y[i as usize] = x[(i + j as R_xlen_t) as usize];
        chg = chg || (j != 0);
        i += 1;
    }

    sm_do_endrule(x, y, n, end_rule, &mut chg);
    chg
}

fn sm_3R(
    x: &[c_double],
    y: &mut [c_double],
    z: &mut [c_double],
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
                y[i as usize] = z[i as usize];
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

fn sptest(x: &[c_double], i: R_xlen_t) -> bool {
    if x[i as usize] != x[(i + 1) as usize] {
        return false;
    }
    if (x[(i - 1) as usize] <= x[i as usize] && x[(i + 1) as usize] <= x[(i + 2) as usize])
        || (x[(i - 1) as usize] >= x[i as usize] && x[(i + 1) as usize] >= x[(i + 2) as usize])
    {
        return false;
    }
    true
}

fn sm_split3(x: &[c_double], y: &mut [c_double], n: R_xlen_t, do_ends: bool) -> bool {
    let mut i: R_xlen_t = 0;
    while i < n {
        y[i as usize] = x[i as usize];
        i += 1;
    }
    if n <= 4 {
        return false;
    }

    let mut chg = false;

    if do_ends && sptest(x, 1) {
        chg = true;
        y[1] = x[0];
        y[2] = med3(x[2], x[3], 3.0 * x[3] - 2.0 * x[4]);
    }

    let mut i: R_xlen_t = 2;
    while i < n - 3 {
        if sptest(x, i) {
            // plateau at x[i] == x[i+1]
            // at left
            let j = imed3(
                x[i as usize],
                x[(i - 1) as usize],
                3.0 * x[(i - 1) as usize] - 2.0 * x[(i - 2) as usize],
            );
            if j > -1 {
                let val = if j == 0 {
                    x[(i - 1) as usize]
                } else {
                    3.0 * x[(i - 1) as usize] - 2.0 * x[(i - 2) as usize]
                };
                y[i as usize] = val;
                chg = chg || (val != x[i as usize]);
            }
            // at right
            let j2 = imed3(
                x[(i + 1) as usize],
                x[(i + 2) as usize],
                3.0 * x[(i + 2) as usize] - 2.0 * x[(i + 3) as usize],
            );
            if j2 > -1 {
                let val = if j2 == 0 {
                    x[(i + 2) as usize]
                } else {
                    3.0 * x[(i + 2) as usize] - 2.0 * x[(i + 3) as usize]
                };
                y[(i + 1) as usize] = val;
                chg = chg || (val != x[(i + 1) as usize]);
            }
        }
        i += 1;
    }

    if do_ends && sptest(x, n - 3) {
        chg = true;
        y[(n - 2) as usize] = x[(n - 1) as usize];
        y[(n - 3) as usize] = med3(
            x[(n - 3) as usize],
            x[(n - 4) as usize],
            3.0 * x[(n - 4) as usize] - 2.0 * x[(n - 5) as usize],
        );
    }

    chg
}

fn sm_3RS3R(
    x: &[c_double],
    y: &mut [c_double],
    z: &mut [c_double],
    w: &mut [c_double],
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

fn sm_3RSS(
    x: &[c_double],
    y: &mut [c_double],
    z: &mut [c_double],
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

fn sm_3RSR(
    x: &[c_double],
    y: &mut [c_double],
    z: &mut [c_double],
    w: &mut [c_double],
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
            z[i as usize] = x[i as usize] - y[i as usize];
            i += 1;
        }
    }

    iter
}

pub fn Rsm(x: SEXP, stype: SEXP, send: SEXP) -> SEXP {
    let iend = asInteger(send);
    let type_ = asInteger(stype);
    let n = XLENGTH(x);

    let ans = Rf_allocVector(SEXPTYPE::VECSXP, 2);
    let _ans_guard = protect(ans);
    let y = Rf_allocVector(SEXPTYPE::REALSXP, n as c_int);
    SET_VECTOR_ELT(ans, 0, y);
    let xs = crate::library::stats::checked::real_vec(x);
    let mut ys = vec![0.0f64; n.max(0) as usize];

    let second_name = if type_ <= 5 {
        let mut iter: c_int = 0;
        match type_ {
            1 => {
                let mut z = vec![0.0f64; n as usize];
                let mut w = vec![0.0f64; n as usize];
                iter = sm_3RS3R(&xs, &mut ys, &mut z, &mut w, n, iend.abs(), iend < 0);
            }
            2 => {
                let mut z = vec![0.0f64; n as usize];
                iter = sm_3RSS(&xs, &mut ys, &mut z, n, iend.abs(), iend < 0);
            }
            3 => {
                let mut z = vec![0.0f64; n as usize];
                let mut w = vec![0.0f64; n as usize];
                iter = sm_3RSR(&xs, &mut ys, &mut z, &mut w, n, iend.abs(), iend < 0);
            }
            4 => {
                // "3R"
                let mut z = vec![0.0f64; n as usize];
                iter = sm_3R(&xs, &mut ys, &mut z, n, iend);
            }
            5 => {
                // "3"
                let chg = sm_3(&xs, &mut ys, n, iend);
                iter = if chg { 1 } else { 0 };
            }
            _ => {} // intentionally unhandled: unknown smooth method
        }
        SET_VECTOR_ELT(ans, 1, Rf_ScalarInteger(iter));
        "iter"
    } else {
        // type > 5: ~ "S"
        let changed = sm_split3(&xs, &mut ys, n, iend != 0);
        SET_VECTOR_ELT(ans, 1, Rf_ScalarLogical(if changed { 1 } else { 0 }));
        "changed"
    };

    crate::library::stats::checked::set_reals(y, &ys);

    let names = Rf_allocVector(SEXPTYPE::STRSXP, 2);
    let _names_guard = protect(names);
    SET_STRING_ELT(names, 0, crate::library::stats::checked::mk_char("y"));
    SET_STRING_ELT(
        names,
        1,
        crate::library::stats::checked::mk_char(second_name),
    );
    setAttrib(ans, R_NamesSymbol(), names);
    ans
}

/// GNU `smooth(x)` default 3RS3R / Tukey / no do.ends.
pub fn do_smooth(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    use crate::sexp::accessors::{CAR, VECTOR_ELT};
    use crate::sexp::constructors::Rf_allocVector3;
    use crate::sexp::ffi::NA_INTEGER;
    use crate::sexp::globals::R_NilValue;
    let x = CAR(args);
    let xt = TYPEOF(x);
    if xt != SEXPTYPE::INTSXP && xt != SEXPTYPE::REALSXP && xt != SEXPTYPE::LGLSXP {
        crate::mainutils::errors::errorcall_str(
            crate::mainutils::errors::R_getCurrentCall(),
            "attempt to smooth non-numeric values",
        );
    }
    let n = XLENGTH(x);
    let xd = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    let _xd = protect(xd);
    for i in 0..n {
        let v = if xt == SEXPTYPE::REALSXP {
            REAL_ELT(x, i as i32)
        } else {
            let iv = INTEGER_ELT(x, i as i32);
            if iv == NA_INTEGER {
                crate::mainutils::errors::errorcall_str(
                    crate::mainutils::errors::R_getCurrentCall(),
                    "attempt to smooth NA values",
                );
            }
            iv as f64
        };
        if !v.is_finite() {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "attempt to smooth NA values",
            );
        }
        SET_REAL_ELT(xd, i as i32, v);
    }
    let stype = crate::sexp::constructors::Rf_ScalarInteger(1);
    let _st = protect(stype);
    let send = crate::sexp::constructors::Rf_ScalarInteger(-2);
    let _se = protect(send);
    let smo = Rsm(xd, stype, send);
    let _smo = protect(smo);
    let y = VECTOR_ELT(smo, 0);
    let iter = VECTOR_ELT(smo, 1);
    crate::sexp::attrib_core::setAttrib(
        y,
        crate::sexp::symbol::install_str("kind"),
        crate::library::stats::checked::mk_string("3RS3R"),
    );
    crate::sexp::attrib_core::setAttrib(
        y,
        crate::sexp::symbol::install_str("twiced"),
        crate::sexp::constructors::Rf_ScalarLogical(0),
    );
    if !iter.is_null() && iter != R_NilValue() {
        crate::sexp::attrib_core::setAttrib(y, crate::sexp::symbol::install_str("iter"), iter);
    }
    crate::sexp::attrib_core::setAttrib(
        y,
        crate::sexp::symbol::install_str("endrule"),
        crate::library::stats::checked::mk_string("copy"),
    );
    crate::sexp::attrib_core::setAttrib(
        y,
        crate::sexp::attrib_core::R_ClassSymbol(),
        crate::library::stats::checked::mk_string("tukeysmooth"),
    );
    y
}
pub unsafe fn c_rsm(x: SEXP, stype: SEXP, send: SEXP) -> SEXP {
    Rsm(x, stype, send)
}
