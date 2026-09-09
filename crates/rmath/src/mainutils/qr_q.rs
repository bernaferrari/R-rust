//! R-facing extraction of the orthogonal Q factor from a `qr` object.

#![allow(non_snake_case)]

use crate::attrib_core::{R_ClassSymbol, R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib};
use crate::main::coerce::{asLogical, coerceVector};
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
fn qr_q_error(s: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError { message: s.into() })
}
unsafe fn match_args(args: SEXP) -> [Option<SEXP>; 3] {
    unsafe {
        let mut out = [None; 3];
        let mut cell = args;
        while cell != R_NilValue() && !cell.is_null() {
            if TAG(cell) != R_NilValue() && !TAG(cell).is_null() {
                let t = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(TAG(cell)))).to_string_lossy();
                let i = ["qr", "complete", "Dvec"]
                    .iter()
                    .position(|n| !t.is_empty() && n.starts_with(t.as_ref()))
                    .unwrap_or_else(|| qr_q_error("unused argument"));
                if out[i].replace(CAR(cell)).is_some() {
                    qr_q_error("formal argument matched by multiple actual arguments")
                }
            }
            cell = CDR(cell);
        }
        cell = args;
        while cell != R_NilValue() && !cell.is_null() {
            if TAG(cell) == R_NilValue() || TAG(cell).is_null() {
                let s = out
                    .iter_mut()
                    .find(|x| x.is_none())
                    .unwrap_or_else(|| qr_q_error("unused argument"));
                *s = Some(CAR(cell));
            }
            cell = CDR(cell);
        }
        out
    }
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
/// Extract Q using the stored Householder vectors, without calling raw dqrsl.
pub unsafe fn do_qr_Q(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let [obj, complete_arg, dvec_arg] = match_args(args);
        let obj = obj.unwrap_or_else(|| qr_q_error("argument 'qr' is missing, with no default"));
        if TYPEOF(obj) != VECSXP_C || !is_qr(obj) {
            qr_q_error("argument is not a QR decomposition")
        }
        let names = getAttrib(obj, R_NamesSymbol());
        let mut factor = R_NilValue();
        let mut aux = R_NilValue();
        let mut rank = R_NilValue();
        if TYPEOF(names) == STRSXP_C && XLENGTH(names) == XLENGTH(obj) {
            for i in 0..XLENGTH(names) {
                let n = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, i))).to_bytes();
                if n == b"qr" {
                    factor = VECTOR_ELT(obj, i)
                } else if n == b"rank" {
                    rank = VECTOR_ELT(obj, i)
                } else if n == b"qraux" {
                    aux = VECTOR_ELT(obj, i)
                }
            }
        }
        if TYPEOF(factor) != REALSXP_C || TYPEOF(aux) != REALSXP_C {
            qr_q_error("invalid QR decomposition")
        }
        let dim = getAttrib(factor, R_DimSymbol());
        if TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            qr_q_error("invalid QR matrix dimensions")
        }
        let m = *INTEGER(dim) as isize;
        let n = *INTEGER(dim).add(1) as isize;
        if m < 0 || n < 0 {
            qr_q_error("invalid QR matrix dimensions")
        }
        let m = m as usize;
        let n = n as usize;
        if m.checked_mul(n) != Some(XLENGTH(factor) as usize) {
            qr_q_error("invalid QR matrix length")
        }
        let k = m.min(n);
        if XLENGTH(aux) < k as i64 {
            qr_q_error("invalid QR decomposition fields")
        }
        let lapack_flag = getAttrib(obj, crate::sexp::symbol::Rf_install(c"useLAPACK".as_ptr()));
        let lapack = TYPEOF(lapack_flag) == SEXPTYPE::LGLSXP
            && XLENGTH(lapack_flag) == 1
            && LOGICAL_ELT(lapack_flag, 0) == 1;
        let transforms = if lapack {
            k
        } else {
            if TYPEOF(rank) != INTSXP_C || XLENGTH(rank) != 1 {
                qr_q_error("invalid QR rank")
            }
            let rank = INTEGER_ELT(rank, 0);
            if rank < 0 || rank as usize > k {
                qr_q_error("invalid QR rank")
            }
            (rank as usize).min(m.saturating_sub(1))
        };
        let complete = complete_arg.is_some_and(|x| {
            let v = asLogical(x);
            if v == NA_LOGICAL {
                qr_q_error("invalid 'complete' argument")
            }
            v != 0
        });
        let cols = if complete { m } else { k };
        let dvec = dvec_arg.map(|x| {
            if TYPEOF(x) == REALSXP_C {
                x
            } else {
                coerceVector(x, REALSXP_C)
            }
        });
        let _dvec_root = dvec.map(protect);
        if let Some(d) = dvec {
            if cols > 0 && (XLENGTH(d) == 0 || (!complete && XLENGTH(d) < cols as i64)) {
                qr_q_error("Dvec has insufficient length")
            }
        }
        let count = m
            .checked_mul(cols)
            .filter(|&n| n <= isize::MAX as usize / std::mem::size_of::<f64>())
            .unwrap_or_else(|| qr_q_error("QR result is too large"));
        let out = Rf_allocVector3(SEXPTYPE::REALSXP, count as R_xlen_t);
        let _out_root = protect(out);
        for j in 0..cols {
            crate::eval::limits::poll_computation();
            let diagonal = dvec.map_or(1.0, |d| REAL(d).add(j % XLENGTH(d) as usize).read());
            if !diagonal.is_finite() {
                qr_q_error("invalid 'Dvec'")
            }
            for i in 0..m {
                *REAL(out).add(i + j * m) = if i == j { diagonal } else { 0.0 };
            }
        }
        for j in (0..transforms).rev() {
            crate::eval::limits::poll_computation();
            let tau = REAL(aux).add(j).read();
            if !tau.is_finite() {
                qr_q_error("invalid QR decomposition fields")
            }
            if tau == 0.0 {
                continue;
            }
            // LINPACK stores u with u[0]=qraux and H=I-u*u'/qraux.
            // LAPACK stores v with v[0]=1 and H=I-tau*v*v'.
            let tail_scale = if lapack { 1.0 } else { 1.0 / tau };
            for col in 0..cols {
                crate::eval::limits::poll_computation();
                let mut dot = REAL(out).add(j + col * m).read();
                for i in j + 1..m {
                    dot += REAL(factor).add(i + j * m).read()
                        * tail_scale
                        * REAL(out).add(i + col * m).read();
                }
                let scale = tau * dot;
                *REAL(out).add(j + col * m) -= scale;
                for i in j + 1..m {
                    *REAL(out).add(i + col * m) -=
                        scale * REAL(factor).add(i + j * m).read() * tail_scale;
                }
            }
        }
        let od = Rf_allocVector(INTSXP_C, 2);
        let _dd = protect(od);
        *INTEGER(od) = m as c_int;
        *INTEGER(od).add(1) = cols as c_int;
        setAttrib(out, R_DimSymbol(), od);
        out
    }
}
