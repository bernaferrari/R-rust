//! Public `qr` wrapper.
//!
//! This keeps the R-facing validation and result construction separate from
//! the lower-level LAPACK entry points, which intentionally have raw-pointer
//! contracts.

#![allow(non_snake_case)]

use std::os::raw::{c_char, c_int};

use crate::appl::linpack_qr::dqrdc2;
use crate::attrib_core::{
    R_ClassSymbol, R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib,
};
use crate::main::coerce::{asLogical, coerceVector};
use crate::modules::lapack::backend::dgeqp3_;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

const REALSXP_C: c_int = 14;
const INTSXP_C: c_int = 13;
const LGLSXP_C: c_int = 10;
const CPLXSXP_C: c_int = 15;
const VECSXP_C: c_int = 19;
const STRSXP_C: c_int = 16;

fn qr_error(message: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: message.into(),
    });
}

/// Implementation of the public `qr(x, tol = 1e-7, LAPACK = FALSE)` builtin.
///
/// The dispatcher supplies evaluated arguments. Complex input always
/// decomposes through LAPACK `zgeqp3`, matching GNU `qr.default`.
unsafe fn match_qr_args(args: SEXP) -> [Option<SEXP>; 3] {
    unsafe {
        let names = ["x", "tol", "LAPACK"];
        let mut matched = [None; 3];
        let mut cell = args;
        while cell != R_NilValue() && !cell.is_null() {
            if TAG(cell) != R_NilValue() && !TAG(cell).is_null() {
                let tag = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(TAG(cell)))).to_string_lossy();
                if let Some(index) = names
                    .iter()
                    .position(|name| !tag.is_empty() && name.starts_with(tag.as_ref()))
                {
                    if matched[index].replace(CAR(cell)).is_some() {
                        qr_error("formal argument matched by multiple actual arguments")
                    }
                }
            }
            cell = CDR(cell);
        }
        cell = args;
        while cell != R_NilValue() && !cell.is_null() {
            if TAG(cell) == R_NilValue() || TAG(cell).is_null() {
                if let Some(slot) = matched.iter_mut().find(|slot| slot.is_none()) {
                    *slot = Some(CAR(cell));
                }
            }
            cell = CDR(cell);
        }
        matched
    }
}

pub unsafe fn do_qr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // GNU qr is a generic (UseMethod); dispatch registered qr.<class>
        // closure methods before the default decomposition. qr.default
        // itself never dispatches.
        if let Some(result) =
            crate::mainutils::essentials::apply_s3_closure_method("qr", _call, args, _rho)
        {
            return result;
        }
        do_qr_default(_call, _op, args, _rho)
    }
}

pub unsafe fn do_qr_default(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let [x_arg, tol_arg, lapack_arg] = match_qr_args(args);
        let x0 = x_arg.unwrap_or_else(|| qr_error("argument 'x' is missing, with no default"));
        if x0.is_null() || x0 == R_NilValue() {
            qr_error("'data' must be of a vector type, was 'NULL'")
        }
        // GNU qr.default tests is.complex(x) before `tol` or `LAPACK` are
        // ever forced, and complex input always decomposes through LAPACK
        // zgeqp3 regardless of those arguments.
        if TYPEOF(x0) == CPLXSXP_C {
            let dim0 = getAttrib(x0, R_DimSymbol());
            let x = if dim0.is_null() || dim0 == R_NilValue() {
                // as.matrix(): a bare complex vector becomes an n x 1 matrix,
                // keeping its names as row dimnames.
                if XLENGTH(x0) > c_int::MAX as i64 {
                    qr_error("vector too large for QR")
                }
                let duplicated = crate::mainutils::duplicate::Rf_duplicate(x0);
                let _duplicated_guard = protect(duplicated);
                let dimensions = Rf_allocVector(INTSXP_C, 2);
                let _dimensions_guard = protect(dimensions);
                INTEGER(dimensions).write(XLENGTH(x0) as c_int);
                INTEGER(dimensions).add(1).write(1);
                setAttrib(duplicated, R_DimSymbol(), dimensions);
                let names = getAttrib(x0, R_NamesSymbol());
                if names != R_NilValue() && !names.is_null() {
                    let dimnames = Rf_allocVector(VECSXP_C, 2);
                    let _dimnames_guard = protect(dimnames);
                    SET_VECTOR_ELT(dimnames, 0, names);
                    SET_VECTOR_ELT(dimnames, 1, R_NilValue());
                    setAttrib(duplicated, R_DimNamesSymbol(), dimnames);
                }
                duplicated
            } else {
                x0
            };
            let _x_guard = protect(x);
            let decomposed = crate::modules::lapack::lapack_impl::La_qr_cmplx(x);
            let _decomposed_guard = protect(decomposed);
            setAttrib(
                decomposed,
                R_ClassSymbol(),
                Rf_mkString(b"qr\0".as_ptr() as *const c_char),
            );
            return decomposed;
        }
        let lapack = lapack_arg.map_or(false, |value| {
            let flag = asLogical(value);
            if flag == NA_LOGICAL {
                qr_error("invalid 'LAPACK' argument")
            }
            flag != 0
        });
        let tolerance = tol_arg.map_or(1.0e-7, |value| crate::main::coerce::asReal(value));
        if !lapack && !tolerance.is_finite() {
            qr_error("invalid 'tol' argument")
        }
        if TYPEOF(x0) != REALSXP_C
            && TYPEOF(x0) != INTSXP_C
            && TYPEOF(x0) != LGLSXP_C
            && TYPEOF(x0) != STRSXP_C
        {
            qr_error("'x' must be a numeric matrix");
        }

        let dim = getAttrib(x0, R_DimSymbol());
        let dim_i = if dim.is_null() || dim == R_NilValue() {
            if XLENGTH(x0) > c_int::MAX as i64 {
                qr_error("vector too large for QR")
            }
            let dimensions = Rf_allocVector(INTSXP_C, 2);
            INTEGER(dimensions).write(XLENGTH(x0) as c_int);
            INTEGER(dimensions).add(1).write(1);
            dimensions
        } else {
            if TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
                qr_error("'x' must be a matrix")
            }
            dim
        };
        let _dim_guard = protect(dim_i);
        let m_i = INTEGER(dim_i).read();
        let n_i = INTEGER(dim_i).add(1).read();
        if m_i < 0 || n_i < 0 {
            qr_error("invalid matrix dimensions");
        }
        let m = m_i as usize;
        let n = n_i as usize;
        if m > c_int::MAX as usize || n > c_int::MAX as usize {
            qr_error("matrix dimensions exceed QR limits");
        }
        let len = match m.checked_mul(n) {
            Some(v) => v,
            None => qr_error("invalid matrix dimensions"),
        };
        if XLENGTH(x0) as usize != len {
            qr_error("'x' has invalid dimensions");
        }
        if !lapack && len > c_int::MAX as usize {
            qr_error("too large a matrix for LINPACK")
        }

        // Keep a real, mutable factor matrix in the arena. This avoids an
        // extra LAPACK/LINPACK copy of the input.
        let xr = if TYPEOF(x0) == REALSXP_C {
            x0
        } else {
            // GNU qr.default assigns storage.mode(x) <- "double" in R;
            // attribute the coercion warning to that call.
            let _warn_call = if TYPEOF(x0) == STRSXP_C {
                // The parser represents `storage.mode(x) <- "double"` as
                // `<-`(storage.mode(x), "double"); build exactly that so
                // warning rendering matches GNU byte for byte.
                let lhs = crate::sexp::constructors::Rf_lang2(
                    crate::sexp::symbol::Rf_install(c"storage.mode".as_ptr()),
                    crate::sexp::symbol::Rf_install(c"x".as_ptr()),
                );
                let call = crate::sexp::constructors::Rf_lang3(
                    crate::sexp::symbol::Rf_install(c"<-".as_ptr()),
                    lhs,
                    {
                        let value = crate::sexp::constructors::Rf_allocVector(SEXPTYPE::STRSXP, 1);
                        crate::sexp::accessors::SET_STRING_ELT(
                            value,
                            0,
                            crate::sexp::constructors::Rf_mkChar(c"double".as_ptr()),
                        );
                        value
                    },
                );
                crate::main::coerce::set_coercion_warning_call(call);
                Some(crate::main::coerce::CoercionWarningCallGuard)
            } else {
                None
            };
            coerceVector(x0, REALSXP_C)
        };
        let _xr_guard = (TYPEOF(x0) != REALSXP_C).then(|| protect(xr));
        for i in 0..len {
            if i % 4096 == 0 {
                crate::eval::limits::poll_computation();
            }
            if !(*REAL(xr).add(i)).is_finite() {
                qr_error("NA/NaN/Inf in foreign function call (arg 1)")
            }
        }
        let qr = crate::mainutils::duplicate::Rf_duplicate(xr);
        let _qr_guard = protect(qr);
        setAttrib(qr, R_DimSymbol(), dim_i);
        let dimnames = getAttrib(x0, R_DimNamesSymbol());
        if dimnames != R_NilValue() {
            setAttrib(qr, R_DimNamesSymbol(), dimnames);
        }

        let min_mn = m.min(n);
        let qraux = Rf_allocVector(REALSXP_C, if lapack { min_mn } else { n } as c_int);
        let _qraux_guard = protect(qraux);
        let pivot = Rf_allocVector(INTSXP_C, n as c_int);
        let _pivot_guard = protect(pivot);
        for i in 0..n {
            INTEGER(pivot)
                .add(i)
                .write(if lapack { 0 } else { (i + 1) as c_int });
        }
        for i in 0..if lapack { min_mn } else { n } {
            REAL(qraux).add(i).write(0.0);
        }
        let rank: c_int;

        if lapack {
            // Query first; the query itself does not require a workspace
            // allocation.  The backend reserves its native temporary buffers
            // around the actual call and reports budget denial via `info`.
            let mut query = 0.0f64;
            let mut lwork: c_int = -1;
            let mut info: c_int = 0;
            let m_c = m as c_int;
            let n_c = n as c_int;
            dgeqp3_(
                &m_c,
                &n_c,
                REAL(qr),
                &m_c,
                INTEGER(pivot),
                REAL(qraux),
                &mut query,
                &lwork,
                &mut info,
            );
            if info != 0 || !query.is_finite() || query < 1.0 || query > c_int::MAX as f64 {
                qr_error("LAPACK routine 'DGEQP3' query failed");
            }
            let work_len = query.ceil() as c_int;
            let work = Rf_allocVector(REALSXP_C, work_len);
            let _work_guard = protect(work);
            lwork = work_len;
            dgeqp3_(
                &m_c,
                &n_c,
                REAL(qr),
                &m_c,
                INTEGER(pivot),
                REAL(qraux),
                REAL(work),
                &lwork,
                &mut info,
            );
            if info != 0 {
                qr_error("LAPACK routine 'DGEQP3' failed");
            }
            rank = min_mn as c_int;
        } else {
            let work_len = match n.checked_mul(2).filter(|&v| v <= c_int::MAX as usize) {
                Some(v) => v as c_int,
                None => qr_error("matrix is too large for QR workspace"),
            };
            let work = Rf_allocVector(REALSXP_C, work_len);
            let _work_guard = protect(work);
            let mut k = 0 as c_int;
            // The pivot SEXP is also dqrdc2's integer workspace, so every
            // buffer remains tracked by the arena for the whole kernel.
            dqrdc2(
                REAL(qr),
                m as c_int,
                m as c_int,
                n as c_int,
                tolerance,
                &mut k,
                REAL(qraux),
                INTEGER(pivot),
                REAL(work),
            );
            rank = k;
        }

        if TYPEOF(dimnames) == SEXPTYPE::VECSXP && LENGTH(dimnames) == 2 {
            let columns = VECTOR_ELT(dimnames, 1);
            if TYPEOF(columns) == SEXPTYPE::STRSXP && XLENGTH(columns) == n as i64 {
                let copied_names = crate::mainutils::duplicate::Rf_duplicate(dimnames);
                let _copied_names_root = protect(copied_names);
                let ordered = Rf_allocVector(STRSXP_C, n as c_int);
                let _ordered_root = protect(ordered);
                for i in 0..n {
                    let original = *INTEGER(pivot).add(i) - 1;
                    if original < 0 || original as usize >= n {
                        qr_error("invalid QR pivot")
                    }
                    SET_STRING_ELT(ordered, i as i64, STRING_ELT(columns, original as i64));
                }
                SET_VECTOR_ELT(copied_names, 1, ordered);
                setAttrib(qr, R_DimNamesSymbol(), copied_names);
            }
        }

        let ret = Rf_allocVector(VECSXP_C, 4);
        let _ret_guard = protect(ret);
        let names = Rf_allocVector(STRSXP_C, 4);
        let _names_guard = protect(names);
        for (i, name) in [c"qr", c"rank", c"qraux", c"pivot"].iter().enumerate() {
            SET_STRING_ELT(names, i as i64, Rf_mkChar(name.as_ptr()));
        }
        SET_VECTOR_ELT(ret, 0, qr);
        SET_VECTOR_ELT(ret, 1, Rf_ScalarInteger(rank));
        SET_VECTOR_ELT(ret, 2, qraux);
        SET_VECTOR_ELT(ret, 3, pivot);
        setAttrib(ret, R_NamesSymbol(), names);
        setAttrib(
            ret,
            R_ClassSymbol(),
            Rf_mkString(b"qr\0".as_ptr() as *const c_char),
        );
        if lapack {
            setAttrib(
                ret,
                Rf_install(b"useLAPACK\0".as_ptr() as *const c_char),
                Rf_ScalarLogical(1),
            );
        }
        ret
    }
}
