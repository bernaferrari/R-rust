//! Direct application of QR Householder factors (`qr.qy`/`qr.qty`).
#![allow(non_snake_case)]
use crate::attrib_core::{
    R_ClassSymbol, R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib,
};
use crate::main::coerce::coerceVector;
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
unsafe fn fields(x: SEXP) -> (SEXP, SEXP, SEXP) {
    unsafe {
        let names = getAttrib(x, R_NamesSymbol());
        let mut f = R_NilValue();
        let mut a = R_NilValue();
        let mut r = R_NilValue();
        if TYPEOF(names) == STRSXP_C && XLENGTH(names) == XLENGTH(x) {
            for i in 0..XLENGTH(names) {
                match std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, i))).to_bytes() {
                    b"qr" => f = VECTOR_ELT(x, i),
                    b"qraux" => a = VECTOR_ELT(x, i),
                    b"rank" => r = VECTOR_ELT(x, i),
                    _ => {}
                }
            }
        }
        (f, a, r)
    }
}
unsafe fn apply(call: SEXP, op: SEXP, args: SEXP, rho: SEXP, transpose: bool) -> SEXP {
    unsafe {
        let _ = (call, op, rho);
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
            err("'qr' must be a QR decomposition")
        }
        let (f, a, r) = fields(q);
        if TYPEOF(f) != REALSXP_C || TYPEOF(a) != REALSXP_C {
            err("invalid QR decomposition")
        }
        let d = getAttrib(f, R_DimSymbol());
        if TYPEOF(d) != INTSXP_C || XLENGTH(d) != 2 {
            err("invalid QR dimensions")
        }
        let m = *INTEGER(d) as isize;
        let n = *INTEGER(d).add(1) as isize;
        if m < 0 || n < 0 {
            err("invalid QR dimensions")
        }
        let m = m as usize;
        let n = n as usize;
        if m.checked_mul(n) != Some(XLENGTH(f) as usize) {
            err("invalid QR matrix length")
        }
        let k = m.min(n);
        if XLENGTH(a) < k as R_xlen_t {
            err("invalid QR decomposition fields");
        }
        let lap = getAttrib(q, crate::sexp::symbol::Rf_install(c"useLAPACK".as_ptr()));
        let lap = TYPEOF(lap) == SEXPTYPE::LGLSXP && XLENGTH(lap) == 1 && LOGICAL_ELT(lap, 0) == 1;
        let t = if lap {
            k
        } else {
            if TYPEOF(r) != INTSXP_C || XLENGTH(r) != 1 {
                err("invalid QR rank")
            }
            let rank = INTEGER_ELT(r, 0);
            if rank < 0 || rank as usize > k {
                err("invalid QR rank")
            }
            (rank as usize).min(m.saturating_sub(1))
        };
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
            err("invalid 'y' dimensions");
        }
        let cols = if matrix {
            let rows = *INTEGER(yd);
            if rows < 0 || rows as usize != m {
                err("'qr' and 'y' must have the same number of rows")
            }
            let cols = *INTEGER(yd).add(1);
            if cols < 0 {
                err("invalid 'y' dimensions");
            }
            cols as usize
        } else {
            if XLENGTH(yy) as usize != m {
                err("'qr' and 'y' must have the same number of rows")
            }
            1
        };
        let count = m
            .checked_mul(cols)
            .filter(|&count| count <= isize::MAX as usize / std::mem::size_of::<f64>())
            .unwrap_or_else(|| err("result too large"));
        if XLENGTH(yy) as usize != count {
            err("invalid 'y' dimensions")
        }
        // GNU's LINPACK .Fortran path rejects nonfinite input before arithmetic.
        if !lap {
            for field in [f, a, yy] {
                for i in 0..XLENGTH(field) as usize {
                    if i % 4096 == 0 {
                        crate::eval::limits::poll_computation();
                    }
                    if !REAL(field).add(i).read().is_finite() {
                        err("NA/NaN/Inf in foreign function call");
                    }
                }
            }
        }
        let out = Rf_allocVector3(SEXPTYPE::REALSXP, count as R_xlen_t);
        let _og = protect(out);
        for i in 0..count {
            if i % 4096 == 0 {
                crate::eval::limits::poll_computation();
            }
            *REAL(out).add(i) = *REAL(yy).add(i)
        }
        for step in 0..t {
            let j = if transpose { step } else { t - 1 - step };
            crate::eval::limits::poll_computation();
            let tau = *REAL(a).add(j);
            if !lap && !tau.is_finite() {
                err("invalid QR decomposition")
            }
            if tau == 0.0 {
                continue;
            }
            let scale = if lap { 1.0 } else { 1.0 / tau };
            for col in 0..cols {
                crate::eval::limits::poll_computation();
                let mut dot = *REAL(out).add(j + col * m);
                for i in j + 1..m {
                    if i % 4096 == 0 {
                        crate::eval::limits::poll_computation();
                    }
                    dot += *REAL(f).add(i + j * m) * scale * *REAL(out).add(i + col * m)
                }
                let z = tau * dot;
                *REAL(out).add(j + col * m) -= z;
                for i in j + 1..m {
                    if i % 4096 == 0 {
                        crate::eval::limits::poll_computation();
                    }
                    *REAL(out).add(i + col * m) -= z * scale * *REAL(f).add(i + j * m)
                }
            }
        }
        if matrix {
            setAttrib(
                out,
                R_DimSymbol(),
                crate::mainutils::duplicate::Rf_duplicate(yd),
            );
            let dn = getAttrib(yy, R_DimNamesSymbol());
            if dn != R_NilValue() {
                setAttrib(
                    out,
                    R_DimNamesSymbol(),
                    crate::mainutils::duplicate::Rf_duplicate(dn),
                )
            }
        } else if lap {
            let dims = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            let _dims = protect(dims);
            *INTEGER(dims) = m as c_int;
            *INTEGER(dims).add(1) = 1;
            setAttrib(out, R_DimSymbol(), dims);
            let names = getAttrib(yy, R_NamesSymbol());
            if names != R_NilValue() {
                let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
                let _dn = protect(dn);
                SET_VECTOR_ELT(dn, 0, names);
                SET_VECTOR_ELT(dn, 1, R_NilValue());
                setAttrib(out, R_DimNamesSymbol(), dn);
            }
        } else {
            let nm = getAttrib(yy, R_NamesSymbol());
            if nm != R_NilValue() {
                setAttrib(
                    out,
                    R_NamesSymbol(),
                    crate::mainutils::duplicate::Rf_duplicate(nm),
                )
            }
        }
        out
    }
}
pub unsafe fn do_qr_qy(c: SEXP, o: SEXP, a: SEXP, r: SEXP) -> SEXP {
    unsafe { apply(c, o, a, r, false) }
}
pub unsafe fn do_qr_qty(c: SEXP, o: SEXP, a: SEXP, r: SEXP) -> SEXP {
    unsafe { apply(c, o, a, r, true) }
}
