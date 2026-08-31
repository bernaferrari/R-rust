//! Transpose/dim helpers: t(), drop(), dim/dimnames<- , nrow/ncol, tsp, aperm-adjacent dimension utilities — extracted verbatim from the former single-file module.
use super::*;
use std::ffi::{CStr, CString};
use std::os::raw::c_int;
use std::path::Path;

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
use crate::sexp::context::RError;
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_REAL, R_NA_BIT_PATTERN, R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use crate::sexp::attrib_core::{R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol};

/// R's `drop(x)` — remove extent-one dimensions from arrays and matrices.
pub unsafe fn do_drop(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        if dim.is_null() || dim == R_NilValue() || TYPEOF(dim) != SEXPTYPE::INTSXP {
            return x;
        }

        let dim_count = XLENGTH(dim);
        let mut kept_axes = Vec::new();
        for i in 0..dim_count {
            if *INTEGER(dim).add(i as usize) != 1 {
                kept_axes.push(i);
            }
        }
        if kept_axes.len() == dim_count as usize {
            return x;
        }

        let result = crate::mainutils::duplicate::shallow_duplicate(x);
        if result.is_null() || result == R_NilValue() {
            return result;
        }
        let _result_guard = protect(result);

        let dimnames =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimNamesSymbol());

        match kept_axes.len() {
            0 => {
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_DimSymbol(),
                    R_NilValue(),
                );
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_DimNamesSymbol(),
                    R_NilValue(),
                );
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    R_NilValue(),
                );
            }
            1 => {
                let names = retained_dimname(dimnames, kept_axes[0]);
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_DimSymbol(),
                    R_NilValue(),
                );
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_DimNamesSymbol(),
                    R_NilValue(),
                );
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    names,
                );
            }
            _ => {
                let new_dim = Rf_allocVector3(SEXPTYPE::INTSXP, kept_axes.len() as R_xlen_t);
                if new_dim.is_null() {
                    return R_NilValue();
                }
                let _dim_guard = protect(new_dim);
                for (out_i, axis) in kept_axes.iter().enumerate() {
                    *INTEGER(new_dim).add(out_i) = *INTEGER(dim).add(*axis as usize);
                }
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_DimSymbol(),
                    new_dim,
                );

                let new_dimnames = retained_dimnames(dimnames, &kept_axes);
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_DimNamesSymbol(),
                    new_dimnames,
                );
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    R_NilValue(),
                );
            }
        }

        result
    }
}

/// R's `t(x)` — transpose a matrix.
///
/// Ported from R's `do_transpose` in array.c (line 1569). Plain vectors
/// (dim of length 0 or 1) transpose as column vectors; dimnames are swapped
/// (including their names), and all other attributes are copied via
/// copyMostAttrib.
pub unsafe fn do_transpose(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let t = TYPEOF(x);
        if !supported_matrix_type(t) {
            not_matrix();
        }

        let dim_attr = crate::sexp::attrib_core::getAttrib(x, R_DimSymbol());
        let ldim = if !dim_attr.is_null()
            && dim_attr != R_NilValue()
            && TYPEOF(dim_attr) == SEXPTYPE::INTSXP
        {
            LENGTH(dim_attr) as R_xlen_t
        } else {
            0
        };

        // (nrow, ncol, rnames, cnames, names(dimnames), whether to attach
        // dimnames at all) as in the C ldim switch.
        let (nrow, ncol, rnames, cnames, dimnames_names, have_dimnames) = match ldim {
            0 => {
                let rnames = crate::sexp::attrib_core::getAttrib(x, R_NamesSymbol());
                let have = !rnames.is_null() && rnames != R_NilValue();
                (XLENGTH(x), 1, rnames, R_NilValue(), R_NilValue(), have)
            }
            1 | 2 => {
                let dimnames = crate::sexp::attrib_core::getAttrib(x, R_DimNamesSymbol());
                let have = !dimnames.is_null() && dimnames != R_NilValue();
                if !have {
                    if ldim == 2 {
                        (
                            *INTEGER(dim_attr) as R_xlen_t,
                            *INTEGER(dim_attr).add(1) as R_xlen_t,
                            R_NilValue(),
                            R_NilValue(),
                            R_NilValue(),
                            false,
                        )
                    } else {
                        (
                            XLENGTH(x),
                            1,
                            R_NilValue(),
                            R_NilValue(),
                            R_NilValue(),
                            false,
                        )
                    }
                } else {
                    let (nrow, ncol, rnames, cnames) = if ldim == 2 {
                        (
                            *INTEGER(dim_attr) as R_xlen_t,
                            *INTEGER(dim_attr).add(1) as R_xlen_t,
                            VECTOR_ELT(dimnames, 0),
                            VECTOR_ELT(dimnames, 1),
                        )
                    } else {
                        (XLENGTH(x), 1, VECTOR_ELT(dimnames, 0), R_NilValue())
                    };
                    let names = crate::sexp::attrib_core::getAttrib(dimnames, R_NamesSymbol());
                    (nrow, ncol, rnames, cnames, names, true)
                }
            }
            _ => not_matrix(),
        };

        let len = nrow * ncol;
        let result = Rf_allocVector3(t, len);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        // Source and destination are both column-major:
        // src(row, col) = row + col * nrow
        // dst(col, row) = col + row * ncol
        for row in 0..nrow {
            for col in 0..ncol {
                let src = row + col * nrow;
                let dst = col + row * ncol;
                copy_matrix_element(result, dst, x, src);
            }
        }

        // Set transposed dim attribute
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            *INTEGER(dim) = ncol as c_int;
            *INTEGER(dim).add(1) = nrow as c_int;
            crate::sexp::attrib_core::setAttrib(result, R_DimSymbol(), dim);
        }

        if have_dimnames {
            let new_dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            if !new_dimnames.is_null() {
                let _dimnames_guard = protect(new_dimnames);
                SET_VECTOR_ELT(new_dimnames, 0, cnames);
                SET_VECTOR_ELT(new_dimnames, 1, rnames);
                if !dimnames_names.is_null() && dimnames_names != R_NilValue() {
                    let new_names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
                    if !new_names.is_null() {
                        let _names_guard = protect(new_names);
                        SET_STRING_ELT(new_names, 1, STRING_ELT(dimnames_names, 0));
                        SET_STRING_ELT(
                            new_names,
                            0,
                            if ldim == 2 {
                                STRING_ELT(dimnames_names, 1)
                            } else {
                                // R_BlankString
                                Rf_mkChar(c"".as_ptr())
                            },
                        );
                        crate::sexp::attrib_core::setAttrib(
                            new_dimnames,
                            R_NamesSymbol(),
                            new_names,
                        );
                    }
                }
                crate::sexp::attrib_core::setAttrib(result, R_DimNamesSymbol(), new_dimnames);
            }
        }

        crate::mainutils::array::copyMostAttrib(x, result);
        result
    }
}

pub fn not_matrix() -> ! {
    std::panic::panic_any(RError {
        message: "argument is not a matrix".to_string(),
    });
}

/// R's `nrow(x)` — number of rows.
pub unsafe fn do_nrow(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
        }
        if is_data_frame_object(x) {
            return Rf_ScalarInteger(data_frame_row_count(x) as c_int);
        }
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 1 {
            Rf_ScalarInteger(*INTEGER(dim_attr))
        } else {
            Rf_ScalarInteger(XLENGTH(x) as c_int)
        }
    }
}

/// R's `ncol(x)` — number of columns.
pub unsafe fn do_ncol(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
        }
        if is_data_frame_object(x) {
            return Rf_ScalarInteger(XLENGTH(x) as c_int);
        }
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
            Rf_ScalarInteger(*INTEGER(dim_attr).add(1))
        } else {
            Rf_ScalarInteger(1)
        }
    }
}

/// R's `dim(x)` — dimensions as integer vector.
pub unsafe fn do_dim(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        if is_data_frame_object(x) {
            let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            if dim.is_null() {
                return R_NilValue();
            }
            *INTEGER(dim) = data_frame_row_count(x) as c_int;
            *INTEGER(dim).add(1) = XLENGTH(x) as c_int;
            return dim;
        }
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if !dim_attr.is_null() {
            dim_attr
        } else {
            R_NilValue()
        }
    }
}

/// R's `tsp(x)` — time-series parameter attribute.
pub unsafe fn do_tsp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_TspSymbol())
    }
}

/// R's `dim(x) <- value` — replace an object's dimension attribute.
pub unsafe fn do_dim_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        if value.is_null() || value == R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                x,
                crate::sexp::attrib_core::R_DimSymbol(),
                R_NilValue(),
            );
            crate::sexp::attrib_core::setAttrib(
                x,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
                R_NilValue(),
            );
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }

        let dim = match dimension_attribute(value, XLENGTH(x)) {
            Ok(dim) => dim,
            Err(message) => {
                std::panic::panic_any(RError { message });
            }
        };

        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::attrib_core::R_NamesSymbol(),
            R_NilValue(),
        );
        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            R_NilValue(),
        );
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_DimSymbol(), dim);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `tsp(x) <- value` — replace or remove time-series parameters.
pub unsafe fn do_tsp_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        if value.is_null() || value == R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                x,
                crate::sexp::attrib_core::R_TspSymbol(),
                R_NilValue(),
            );
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }

        let tsp = match tsp_attribute(value) {
            Ok(tsp) => tsp,
            Err(message) => {
                std::panic::panic_any(RError { message });
            }
        };
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_TspSymbol(), tsp);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

pub unsafe fn tsp_attribute(value: SEXP) -> Result<SEXP, String> {
    unsafe {
        if XLENGTH(value) != 3
            || (TYPEOF(value) != SEXPTYPE::INTSXP && TYPEOF(value) != SEXPTYPE::REALSXP)
        {
            return Err("'tsp' attribute must be numeric of length three".to_string());
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
        if result.is_null() {
            return Ok(R_NilValue());
        }
        let _result_guard = protect(result);
        for i in 0..3 {
            *REAL(result).add(i) = if TYPEOF(value) == SEXPTYPE::INTSXP {
                let n = INTEGER_ELT(value, i as c_int);
                if n == NA_INTEGER { NA_REAL } else { n as f64 }
            } else {
                REAL_ELT(value, i as c_int)
            };
        }

        let start = *REAL(result);
        let end = *REAL(result).add(1);
        let frequency = *REAL(result).add(2);
        if frequency.is_infinite() || start.is_infinite() || end.is_infinite() {
            return Err("invalid time series parameters specified (1)".to_string());
        }
        if !frequency.is_nan() && frequency <= 0.0 {
            return Err("invalid time series parameters specified (0)".to_string());
        }
        if start.is_finite() && end.is_finite() && frequency.is_finite() && end < start {
            return Err("invalid time series parameters specified (1)".to_string());
        }
        Ok(result)
    }
}

pub unsafe fn dimension_attribute(value: SEXP, object_len: R_xlen_t) -> Result<SEXP, String> {
    unsafe {
        if !is_atomic_vector_type(TYPEOF(value)) {
            return Err("invalid second argument, must be vector or NULL".to_string());
        }

        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, XLENGTH(value));
        let _dim_guard = protect(dim);
        for i in 0..XLENGTH(value) {
            *INTEGER(dim).add(i as usize) = dimension_component(value, i);
        }

        // dimgets(): dims are validated and totaled by array.c's dim2total()
        // (trunk attrib.c delegates there — it errors on length-0/missing/
        // negative dims itself).
        let total = crate::mainutils::array::dim2total(dim)
            .map_err(|()| "too many elements specified".to_string())?;

        if total != object_len {
            return Err(format!(
                "dims [product {total}] do not match the length of object [{object_len}]"
            ));
        }

        Ok(dim)
    }
}
/// R's `dimnames(x)` — get dimension names of a matrix/array.
pub unsafe fn do_dimnames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr()),
        )
    }
}
