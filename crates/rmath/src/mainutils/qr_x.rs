//! Public `qr.X` reconstruction of the original matrix from a QR object.
#![allow(non_snake_case)]

use crate::attrib_core::{R_ClassSymbol, R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib};
use crate::main::coerce::{asInteger, asLogical};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
use std::os::raw::c_int;

const REALSXP_C: c_int = 14;
const INTSXP_C: c_int = 13;
const LGLSXP_C: c_int = 10;
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

unsafe fn field(x: SEXP, name: &[u8]) -> SEXP {
    unsafe {
        let names = getAttrib(x, R_NamesSymbol());
        if TYPEOF(names) == STRSXP_C && XLENGTH(names) == XLENGTH(x) {
            for i in 0..XLENGTH(names) {
                if std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, i))).to_bytes() == name {
                    return VECTOR_ELT(x, i);
                }
            }
        }
        R_NilValue()
    }
}

unsafe fn match_args(args: SEXP) -> [Option<SEXP>; 3] {
    unsafe {
        let mut matched = [None; 3];
        let mut cell = args;
        while cell != R_NilValue() && !cell.is_null() {
            if TAG(cell) != R_NilValue() && !TAG(cell).is_null() {
                let tag = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(TAG(cell)))).to_string_lossy();
                let index = ["qr", "complete", "ncol"]
                    .iter()
                    .position(|name| !tag.is_empty() && name.starts_with(tag.as_ref()))
                    .unwrap_or_else(|| err("unused argument"));
                if matched[index].replace(CAR(cell)).is_some() {
                    err("formal argument matched by multiple actual arguments");
                }
            }
            cell = CDR(cell);
        }
        cell = args;
        while cell != R_NilValue() && !cell.is_null() {
            if TAG(cell) == R_NilValue() || TAG(cell).is_null() {
                let slot = matched
                    .iter_mut()
                    .find(|slot| slot.is_none())
                    .unwrap_or_else(|| err("unused argument"));
                *slot = Some(CAR(cell));
            }
            cell = CDR(cell);
        }
        matched
    }
}

pub unsafe fn do_qr_X(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let [object, complete_arg, ncol_arg] = match_args(args);
        let object = object.unwrap_or_else(|| err("argument 'qr' is missing, with no default"));
        if object == R_NilValue() || TYPEOF(object) != VECSXP_C || !is_qr(object) {
            err("argument is not a QR decomposition")
        }
        let complete = complete_arg.is_some_and(|value| {
            let flag = asLogical(value);
            if flag == NA_LOGICAL {
                err("invalid 'complete' argument")
            }
            flag != 0
        });
        let true_s = Rf_allocVector3(SEXPTYPE::LGLSXP, 1);
        let _true = protect(true_s);
        *LOGICAL(true_s) = 1;
        let r_tail = Rf_cons(true_s, R_NilValue());
        let _r_tail = protect(r_tail);
        let r_args = Rf_cons(object, r_tail);
        let _r_args = protect(r_args);
        let r = crate::mainutils::qr_extract::do_qr_R(R_NilValue(), R_NilValue(), r_args, R_NilValue());
        let _r = protect(r);
        let rdim = getAttrib(r, R_DimSymbol());
        if TYPEOF(rdim) != INTSXP_C || XLENGTH(rdim) != 2 {
            err("invalid NCOL(R)")
        }
        let nrow_r = *INTEGER(rdim);
        let p = *INTEGER(rdim).add(1);
        if p < 0 || nrow_r < 0 {
            err("invalid NCOL(R)")
        }
        let default_ncol = if complete {
            nrow_r
        } else if nrow_r < p {
            nrow_r
        } else {
            p
        };
        let ncol = ncol_arg.map_or(default_ncol, |value| {
            let n = asInteger(value);
            if n == NA_INTEGER || n < 0 {
                err("invalid 'ncol' argument")
            }
            n
        });
        let pivot = field(object, b"pivot");
        let pivoted = TYPEOF(pivot) == INTSXP_C
            && (0..XLENGTH(pivot)).any(|i| *INTEGER(pivot).add(i as usize) != (i as c_int + 1));
        if pivoted && ncol < XLENGTH(pivot) as c_int {
            err("need larger value of 'ncol' as pivoting occurred")
        }
        let r_use = if ncol == p {
            r
        } else if ncol < p {
            let out = Rf_allocVector3(SEXPTYPE::REALSXP, (nrow_r * ncol) as R_xlen_t);
            let _out = protect(out);
            for j in 0..ncol as usize {
                for i in 0..nrow_r as usize {
                    *REAL(out).add(i + j * nrow_r as usize) =
                        *REAL(r).add(i + j * nrow_r as usize);
                }
            }
            let dims = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            let _dims = protect(dims);
            *INTEGER(dims) = nrow_r;
            *INTEGER(dims).add(1) = ncol;
            setAttrib(out, R_DimSymbol(), dims);
            out
        } else {
            let out = Rf_allocVector3(SEXPTYPE::REALSXP, (nrow_r * ncol) as R_xlen_t);
            let _out = protect(out);
            for i in 0..(nrow_r * ncol) as usize {
                *REAL(out).add(i) = 0.0;
            }
            for j in 0..p as usize {
                for i in 0..nrow_r as usize {
                    *REAL(out).add(i + j * nrow_r as usize) =
                        *REAL(r).add(i + j * nrow_r as usize);
                }
            }
            for i in 0..nrow_r as usize {
                if i < ncol as usize {
                    *REAL(out).add(i + i * nrow_r as usize) = 1.0;
                }
            }
            // The extra-column identity is on the new columns only; GNU does
            // diag(..., nrow(R), ncol) then overwrites the first p columns.
            // Rebuild that explicitly.
            for i in 0..(nrow_r * ncol) as usize {
                *REAL(out).add(i) = 0.0;
            }
            for i in 0..nrow_r.min(ncol) as usize {
                *REAL(out).add(i + i * nrow_r as usize) = 1.0;
            }
            for j in 0..p as usize {
                for i in 0..nrow_r as usize {
                    *REAL(out).add(i + j * nrow_r as usize) =
                        *REAL(r).add(i + j * nrow_r as usize);
                }
            }
            let dims = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            let _dims = protect(dims);
            *INTEGER(dims) = nrow_r;
            *INTEGER(dims).add(1) = ncol;
            setAttrib(out, R_DimSymbol(), dims);
            out
        };
        let qy_tail = Rf_cons(r_use, R_NilValue());
        let _qy_tail = protect(qy_tail);
        let qy_args = Rf_cons(object, qy_tail);
        let _qy_args = protect(qy_args);
        let res = crate::mainutils::qr_apply::do_qr_qy(
            R_NilValue(),
            R_NilValue(),
            qy_args,
            R_NilValue(),
        );
        let _res = protect(res);
        if pivoted {
            let pvt_len = XLENGTH(pivot) as usize;
            let rdim = getAttrib(res, R_DimSymbol());
            let rows = if TYPEOF(rdim) == INTSXP_C && XLENGTH(rdim) == 2 {
                *INTEGER(rdim) as usize
            } else {
                err("invalid qr.X result")
            };
            let cols = if TYPEOF(rdim) == INTSXP_C && XLENGTH(rdim) == 2 {
                *INTEGER(rdim).add(1) as usize
            } else {
                0
            };
            if cols < pvt_len {
                err("need larger value of 'ncol' as pivoting occurred")
            }
            let mut copy = vec![0.0f64; rows * cols];
            for i in 0..rows * cols {
                copy[i] = *REAL(res).add(i);
            }
            for i in 0..pvt_len {
                let dest = *INTEGER(pivot).add(i);
                if dest <= 0 || dest as usize > cols {
                    err("invalid QR pivot")
                }
                let src = i;
                for row in 0..rows {
                    *REAL(res).add(row + (dest as usize - 1) * rows) = copy[row + src * rows];
                }
            }
        }
        res
    }
}
