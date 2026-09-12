//! Public GNU qr.fitted / qr.resid.
//!
//! These compute the column space projection Q[,1:k] Q[,1:k]^T y via the
//! existing qr.qty / qr.qy Householder path, matching dqrxb / dqrrsd.
#![allow(non_snake_case)]

use crate::attrib_core::{R_ClassSymbol, R_DimSymbol, R_NamesSymbol, getAttrib};
use crate::main::coerce::{asInteger, coerceVector};
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
const LGLSXP_C: c_int = 10;

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

unsafe fn fields(x: SEXP) -> (SEXP, SEXP, SEXP) {
    unsafe {
        let names = getAttrib(x, R_NamesSymbol());
        let mut qr = R_NilValue();
        let mut rank = R_NilValue();
        let mut qraux = R_NilValue();
        if TYPEOF(names) == STRSXP_C && XLENGTH(names) == XLENGTH(x) {
            for i in 0..XLENGTH(names) {
                match std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, i))).to_bytes() {
                    b"qr" => qr = VECTOR_ELT(x, i),
                    b"rank" => rank = VECTOR_ELT(x, i),
                    b"qraux" => qraux = VECTOR_ELT(x, i),
                    _ => {}
                }
            }
        }
        (qr, rank, qraux)
    }
}

unsafe fn match_args<const N: usize>(args: SEXP, formals: [&[u8]; N]) -> [Option<SEXP>; N] {
    unsafe {
        let mut matched = [None; N];
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            if !tag.is_null() && tag != R_NilValue() {
                let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag))).to_bytes();
                let index = formals
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
        matched
    }
}

#[derive(Clone, Copy)]
enum Job {
    Fitted,
    Resid,
}

unsafe fn apply(args: SEXP, job: Job) -> SEXP {
    unsafe {
        let q;
        let y;
        let k_arg;
        match job {
            Job::Fitted => {
                let matched = match_args(args, [b"qr".as_slice(), b"y".as_slice(), b"k".as_slice()]);
                q = matched[0].unwrap_or_else(|| err("argument 'qr' is missing, with no default"));
                y = matched[1].unwrap_or_else(|| err("argument 'y' is missing, with no default"));
                k_arg = matched[2];
            }
            Job::Resid => {
                let matched = match_args(args, [b"qr".as_slice(), b"y".as_slice()]);
                q = matched[0].unwrap_or_else(|| err("argument 'qr' is missing, with no default"));
                y = matched[1].unwrap_or_else(|| err("argument 'y' is missing, with no default"));
                k_arg = None;
            }
        }
        if TYPEOF(q) != VECSXP_C || !is_qr(q) {
            err("argument is not a QR decomposition")
        }
        let (f, rank_s, qraux) = fields(q);
        if TYPEOF(f) != REALSXP_C {
            err("not implemented for complex 'qr'")
        }
        let lap = getAttrib(q, crate::sexp::symbol::Rf_install(c"useLAPACK".as_ptr()));
        let lap = TYPEOF(lap) == SEXPTYPE::LGLSXP && XLENGTH(lap) == 1 && LOGICAL_ELT(lap, 0) == 1;
        if lap {
            err("not supported for LAPACK QR")
        }
        let d = getAttrib(f, R_DimSymbol());
        if TYPEOF(d) != INTSXP_C || XLENGTH(d) != 2 {
            err("invalid nrow(qr)")
        }
        let n = *INTEGER(d);
        if n < 0 {
            err("invalid nrow(qr)")
        }
        let p = *INTEGER(d).add(1);
        if p < 0 {
            err("invalid ncol(qr)")
        }
        let n_us = n as usize;
        let p_us = p as usize;
        if n_us.checked_mul(p_us) != Some(XLENGTH(f) as usize) {
            err("invalid QR matrix length")
        }
        if TYPEOF(rank_s) != INTSXP_C || XLENGTH(rank_s) != 1 {
            err("invalid ncol(qr)")
        }
        let rank = *INTEGER(rank_s);
        if rank == NA_INTEGER || rank < 0 || rank as usize > n_us.min(p_us) {
            err("invalid ncol(qr)")
        }
        let k = match k_arg {
            Some(arg) => {
                let value = asInteger(arg);
                if value == NA_INTEGER {
                    err("invalid 'k'")
                }
                if value > rank {
                    err("'k' is too large")
                }
                if value < 0 {
                    err("invalid 'k'")
                }
                value
            }
            None => rank,
        };
        if matches!(job, Job::Resid) && k == 0 {
            return y;
        }
        if TYPEOF(qraux) != REALSXP_C || XLENGTH(qraux) < k as R_xlen_t {
            err("invalid QR decomposition")
        }
        let yy = if TYPEOF(y) == REALSXP_C {
            y
        } else if matches!(TYPEOF(y), INTSXP_C | STRSXP_C | LGLSXP_C) {
            coerceVector(y, REALSXP_C)
        } else {
            err("'y' must be numeric")
        };
        let _yg = protect(yy);
        let yd = getAttrib(yy, R_DimSymbol());
        let matrix = yd != R_NilValue() && !yd.is_null();
        if matrix && (TYPEOF(yd) != INTSXP_C || XLENGTH(yd) != 2) {
            err("invalid NCOL(y)")
        }
        let ny = if matrix {
            let rows = *INTEGER(yd);
            if rows != n {
                err("'qr' and 'y' must have the same number of rows")
            }
            let cols = *INTEGER(yd).add(1);
            if cols < 0 {
                err("invalid NCOL(y)")
            }
            cols as usize
        } else {
            if XLENGTH(yy) != n as R_xlen_t {
                err("'qr' and 'y' must have the same number of rows")
            }
            1
        };
        let Some(y_len) = n_us.checked_mul(ny) else {
            err("result too large")
        };
        if XLENGTH(yy) as usize != y_len {
            err("invalid NCOL(y)")
        }
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
        let out = crate::mainutils::duplicate::Rf_duplicate(yy);
        let _og = protect(out);
        if ny == 0 {
            return out;
        }
        if k == 0 {
            if matches!(job, Job::Fitted) {
                for i in 0..y_len {
                    *REAL(out).add(i) = 0.0;
                }
            }
            return out;
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
        for j in 0..ny {
            for i in k as usize..n_us {
                *REAL(qty).add(i + j * n_us) = 0.0;
            }
        }
        let qy_tail = Rf_cons(qty, R_NilValue());
        let _qy_tail = protect(qy_tail);
        let qy_args = Rf_cons(q, qy_tail);
        let _qy_args = protect(qy_args);
        let fitted = crate::mainutils::qr_apply::do_qr_qy(
            R_NilValue(),
            R_NilValue(),
            qy_args,
            R_NilValue(),
        );
        let _fitted = protect(fitted);
        match job {
            Job::Fitted => {
                for i in 0..y_len {
                    *REAL(out).add(i) = *REAL(fitted).add(i);
                }
            }
            Job::Resid => {
                for i in 0..y_len {
                    *REAL(out).add(i) = *REAL(yy).add(i) - *REAL(fitted).add(i);
                }
            }
        }
        out
    }
}

pub unsafe fn do_qr_fitted(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { apply(args, Job::Fitted) }
}

pub unsafe fn do_qr_resid(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { apply(args, Job::Resid) }
}
