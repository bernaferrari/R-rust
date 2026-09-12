//! Public `qr.coef` wrapper over LINPACK `dqrsl` and LAPACK `qr_coef_real`.
#![allow(non_snake_case)]

use crate::appl::linpack_qr::dqrsl;
use crate::attrib_core::{R_ClassSymbol, R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib};
use crate::main::coerce::coerceVector;
use crate::modules::lapack::backend::dtrtrs_;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
use std::os::raw::c_int;

const REALSXP_C: c_int = 14;
const INTSXP_C: c_int = 13;
const STRSXP_C: c_int = 16;
const VECSXP_C: c_int = 19;

fn err(s: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError { message: s.into() })
}

unsafe fn is_qr(x: SEXP) -> bool {
    unsafe {
        let c = getAttrib(x, R_ClassSymbol());
        if c == R_NilValue() || TYPEOF(c) != STRSXP_C {
            return false;
        }
        for i in 0..XLENGTH(c) {
            if std::ffi::CStr::from_ptr(CHAR(STRING_ELT(c, i))).to_bytes() == b"qr" {
                return true;
            }
        }
        false
    }
}

unsafe fn fields(x: SEXP) -> (SEXP, SEXP, SEXP, SEXP) {
    unsafe {
        let names = getAttrib(x, R_NamesSymbol());
        let mut qr = R_NilValue();
        let mut rank = R_NilValue();
        let mut qraux = R_NilValue();
        let mut pivot = R_NilValue();
        if TYPEOF(names) == STRSXP_C && XLENGTH(names) == XLENGTH(x) {
            for i in 0..XLENGTH(names) {
                match std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, i))).to_bytes() {
                    b"qr" => qr = VECTOR_ELT(x, i),
                    b"rank" => rank = VECTOR_ELT(x, i),
                    b"qraux" => qraux = VECTOR_ELT(x, i),
                    b"pivot" => pivot = VECTOR_ELT(x, i),
                    _ => {}
                }
            }
        }
        (qr, rank, qraux, pivot)
    }
}

pub unsafe fn do_qr_coef(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut matched = [None; 2];
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            if !tag.is_null() && tag != R_NilValue() {
                let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag))).to_bytes();
                let index = [b"qr".as_slice(), b"y".as_slice()]
                    .iter()
                    .position(|formal| !name.is_empty() && formal.starts_with(name))
                    .unwrap_or_else(|| err("unused argument"));
                if matched[index].replace(CAR(cell)).is_some() {
                    err("formal argument matched by multiple actual arguments");
                }
            }
            cell = CDR(cell);
        }
        cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            if tag.is_null() || tag == R_NilValue() {
                let slot = matched
                    .iter_mut()
                    .find(|x| x.is_none())
                    .unwrap_or_else(|| err("unused argument"));
                *slot = Some(CAR(cell));
            }
            cell = CDR(cell);
        }
        let q = matched[0].unwrap_or_else(|| err("argument 'qr' is missing, with no default"));
        let y = matched[1].unwrap_or_else(|| err("argument 'y' is missing, with no default"));
        if TYPEOF(q) != VECSXP_C || !is_qr(q) {
            err("first argument must be a QR decomposition")
        }
        let (f, rank_s, qraux, pivot) = fields(q);
        if TYPEOF(f) != REALSXP_C {
            err("complex matrices are not supported by qr.coef in this runtime")
        }
        let d = getAttrib(f, R_DimSymbol());
        if TYPEOF(d) != INTSXP_C || XLENGTH(d) != 2 {
            err("invalid nrow(qr$qr)")
        }
        let n = *INTEGER(d);
        let p = *INTEGER(d).add(1);
        if n < 0 {
            err("invalid nrow(qr$qr)")
        }
        if p < 0 {
            err("invalid ncol(qr$qr)")
        }
        let n_us = n as usize;
        let p_us = p as usize;
        if n_us.checked_mul(p_us) != Some(XLENGTH(f) as usize) {
            err("invalid QR matrix length")
        }
        if TYPEOF(rank_s) != INTSXP_C || XLENGTH(rank_s) != 1 {
            err("invalid ncol(qr$rank)")
        }
        let k = *INTEGER(rank_s);
        if k < 0 || k as usize > n_us.min(p_us) {
            err("invalid ncol(qr$rank)")
        }
        if TYPEOF(qraux) != REALSXP_C || XLENGTH(qraux) < k as R_xlen_t {
            err("invalid QR decomposition")
        }
        if TYPEOF(pivot) != INTSXP_C || XLENGTH(pivot) != p as R_xlen_t {
            err("invalid QR pivot")
        }
        let yy = if TYPEOF(y) == REALSXP_C {
            y
        } else if matches!(TYPEOF(y), INTSXP_C | STRSXP_C | 10) {
            coerceVector(y, REALSXP_C)
        } else {
            err("'y' must be numeric")
        };
        let _yg = protect(yy);
        let yd = getAttrib(yy, R_DimSymbol());
        let matrix = yd != R_NilValue() && !yd.is_null();
        if matrix && (TYPEOF(yd) != INTSXP_C || XLENGTH(yd) != 2) {
            err("invalid ncol(y)")
        }
        let ny = if matrix {
            let rows = *INTEGER(yd);
            if rows != n {
                err("'qr' and 'y' must have the same number of rows")
            }
            let cols = *INTEGER(yd).add(1);
            if cols < 0 {
                err("invalid ncol(y)")
            }
            cols as usize
        } else {
            if XLENGTH(yy) != n as R_xlen_t {
                err("'qr' and 'y' must have the same number of rows")
            }
            1
        };
        let Some(out_len) = p_us.checked_mul(ny) else {
            err("result too large")
        };
        let coef = Rf_allocVector3(SEXPTYPE::REALSXP, out_len as R_xlen_t);
        let _cg = protect(coef);
        for i in 0..out_len {
            *REAL(coef).add(i) = NA_REAL;
        }
        if p == 0 || k == 0 {
            return finish(coef, p, ny, matrix);
        }
        let lap = getAttrib(q, crate::sexp::symbol::Rf_install(c"useLAPACK".as_ptr()));
        let lap = TYPEOF(lap) == SEXPTYPE::LGLSXP && XLENGTH(lap) == 1 && LOGICAL_ELT(lap, 0) == 1;
        if lap {
            let kk = XLENGTH(qraux) as c_int;
            if kk < 0 || kk as usize > n_us.min(p_us) {
                err("invalid QR decomposition")
            }
            let qty_tail = Rf_cons(yy, R_NilValue());
            let _qty_tail = protect(qty_tail);
            let qty_args = Rf_cons(q, qty_tail);
            let _qty_args = protect(qty_args);
            let qty = crate::mainutils::qr_apply::do_qr_qty(
                R_NilValue(),
                R_NilValue(),
                qty_args,
                R_NilValue(),
            );
            let _qty = protect(qty);
            if XLENGTH(qty) as usize != n_us * ny {
                err("invalid QR coefficient workspace")
            }
            let mut b = vec![0.0f64; n_us * ny];
            for i in 0..n_us * ny {
                b[i] = *REAL(qty).add(i);
            }
            let ny_i = ny as c_int;
            let mut info: c_int = 0;
            dtrtrs_(
                b"U".as_ptr(),
                b"N".as_ptr(),
                b"N".as_ptr(),
                &kk,
                &ny_i,
                REAL(f),
                &n,
                b.as_mut_ptr(),
                &n,
                &mut info,
            );
            if info != 0 {
                err("error code from Lapack routine 'dtrtrs'");
            }
            for j in 0..ny {
                for i in 0..kk as usize {
                    let dest = *INTEGER(pivot).add(i);
                    if dest <= 0 || dest as usize > p_us {
                        err("invalid QR pivot")
                    }
                    *REAL(coef).add((dest as usize - 1) + j * p_us) = b[i + j * n_us];
                }
            }
        } else {
            for field in [f, qraux, yy] {
                for i in 0..XLENGTH(field) as usize {
                    if i % 4096 == 0 {
                        crate::eval::limits::poll_computation();
                    }
                    if !REAL(field).add(i).read().is_finite() {
                        err("NA/NaN/Inf in foreign function call");
                    }
                }
            }
            let Some(y_len) = n_us.checked_mul(ny) else {
                err("result too large")
            };
            let mut y_copy = vec![0.0f64; y_len];
            for i in 0..y_len {
                y_copy[i] = *REAL(yy).add(i);
            }
            let mut b = vec![0.0f64; k as usize * ny];
            let mut dummy = 0.0f64;
            for j in 0..ny {
                crate::eval::limits::poll_computation();
                let yj = y_copy.as_mut_ptr().add(j * n_us);
                let bj = b.as_mut_ptr().add(j * k as usize);
                let mut info: c_int = 0;
                dqrsl(
                    REAL(f),
                    n,
                    n,
                    k,
                    REAL(qraux),
                    yj,
                    &mut dummy,
                    yj,
                    bj,
                    &mut dummy,
                    &mut dummy,
                    100,
                    &mut info,
                );
                if info != 0 {
                    err("exact singularity in 'qr.coef'");
                }
            }
            if (k as usize) < p_us {
                for j in 0..ny {
                    for i in 0..k as usize {
                        let dest = *INTEGER(pivot).add(i);
                        if dest <= 0 || dest as usize > p_us {
                            err("invalid QR pivot")
                        }
                        *REAL(coef).add((dest as usize - 1) + j * p_us) = b[i + j * k as usize];
                    }
                }
            } else {
                for i in 0..out_len {
                    *REAL(coef).add(i) = b[i];
                }
            }
        }
        finish(coef, p, ny, matrix)
    }
}

unsafe fn finish(coef: SEXP, p: c_int, ny: usize, matrix: bool) -> SEXP {
    unsafe {
        if matrix {
            let dims = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            let _dims = protect(dims);
            *INTEGER(dims) = p;
            *INTEGER(dims).add(1) = ny as c_int;
            setAttrib(coef, R_DimSymbol(), dims);
            coef
        } else if p == 1 {
            coef
        } else {
            coef
        }
    }
}
