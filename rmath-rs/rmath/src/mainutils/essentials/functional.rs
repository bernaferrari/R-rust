//! Essentials domain module `functional` — extracted verbatim from essentials.rs.

use super::*;
use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_int;

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
use crate::sexp::ffi::{
    FALSE, ISNAN, NA_INTEGER, NA_LOGICAL, NA_REAL, R_NA_BIT_PATTERN, R_xlen_t, Rcomplex, SEXP,
    SEXPTYPE, TRUE,
};
use crate::sexp::globals::{R_MissingArg, R_NilValue};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// lapply/sapply/Map/Filter/do.call — functional programming
// ---------------------------------------------------------------------------

/// R's `lapply(X, FUN)` — apply FUN to each element, return list.
pub unsafe fn do_lapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = eval_arg_by_name_or_position(args, &["X"], 0, rho);
        let fun = callable_arg_by_name_or_position(args, &["FUN"], 1);
        if x.is_null() || x == R_NilValue() || fun.is_null() {
            return Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        }
        // `x` and `fun` live across `apply_unary_value` -> `Rf_eval`, which
        // allocates and can run a deferred collection at an eval safe point.
        // Raw Rust locals are not GC roots, so an unprotected `x` can be
        // swept mid-loop and its slab slot recycled as another node type;
        // `extract_element` then reads `TYPEOF(x)` off the recycled node.
        // Protect both for the whole loop, like `result`.
        let _x_guard = protect(x);
        let _fun_guard = protect(fun);
        let n = list_apply_len(x);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            let elem = extract_element(x, i);
            let val = apply_unary_value(fun, elem, rho);
            crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, val);
        }
        let names = list_apply_names(x, n);
        if !names.is_null() && names != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_NamesSymbol(),
                names,
            );
        }
        result
    }
}

/// R's `sapply(X, FUN)` — like lapply but simplifies to vector.
pub unsafe fn do_sapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let list = do_lapply(_call, _op, args, rho);
        simplify_scalar_list(list)
    }
}

/// R's `vapply(X, FUN, FUN.VALUE)` — apply and simplify using FUN.VALUE's scalar type.
pub unsafe fn do_vapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let template_expr = arg_by_name_or_position(args, &["FUN.VALUE"], 2);
        let template_type = fun_value_type(template_expr, rho);
        let list = do_lapply(_call, _op, args, rho);
        simplify_scalar_list_as(list, template_type)
    }
}

/// R's `Map(f, ...)` — apply f element-wise.
pub unsafe fn do_map(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let fun = callable_arg_by_name_or_position(args, &["f", "FUN"], 0);
        let x = eval_arg_by_name_or_position(args, &[], 1, rho);
        if fun.is_null() || x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        // Protect across the eval-per-element loop (see do_lapply).
        let _x_guard = protect(x);
        let _fun_guard = protect(fun);
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            let elem = extract_element(x, i);
            let val = apply_unary_value(fun, elem, rho);
            crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, val);
        }
        result
    }
}

/// R's `Filter(f, x)` — keep elements where f returns TRUE.
pub unsafe fn do_filter(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let fun = callable_arg_by_name_or_position(args, &["f", "FUN"], 0);
        let x = eval_arg_by_name_or_position(args, &["x"], 1, rho);
        if fun.is_null() || x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        // Protect across the eval-per-element loop (see do_lapply).
        let _x_guard = protect(x);
        let _fun_guard = protect(fun);
        let n = XLENGTH(x);
        let mut kept: Vec<R_xlen_t> = Vec::new();
        for i in 0..n {
            let elem = extract_element(x, i);
            let val = apply_unary_value(fun, elem, rho);
            if !val.is_null() && TYPEOF(val) == SEXPTYPE::LGLSXP && *LOGICAL(val) != 0 {
                kept.push(i);
            }
        }
        let result = Rf_allocVector3(TYPEOF(x), kept.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (new_i, &old_i) in kept.iter().enumerate() {
            if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(result).add(new_i) = *REAL(x).add(old_i as usize);
            } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                *INTEGER(result).add(new_i) = *INTEGER(x).add(old_i as usize);
            }
        }
        result
    }
}

/// R's `do.call(what, args)` — call function with list of args.
pub unsafe fn do_do_call(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let fun = callable_arg_by_name_or_position(args, &["what"], 0);
        let arg_list = eval_arg_by_name_or_position(args, &["args"], 1, rho);
        if fun.is_null() || arg_list.is_null() {
            return R_NilValue();
        }
        let n = if TYPEOF(arg_list) == SEXPTYPE::VECSXP {
            XLENGTH(arg_list)
        } else {
            0
        };
        let names = if TYPEOF(arg_list) == SEXPTYPE::VECSXP {
            crate::sexp::attrib_core::getAttrib(arg_list, crate::sexp::attrib_core::R_NamesSymbol())
        } else {
            R_NilValue()
        };
        let mut call_args = R_NilValue();
        for i in (0..n).rev() {
            let cell = Rf_cons(
                crate::sexp::accessors::VECTOR_ELT(arg_list, i as i64),
                call_args,
            );
            if !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == SEXPTYPE::STRSXP
                && i < XLENGTH(names)
            {
                let name = STRING_ELT(names, i);
                if !name.is_null() {
                    let chars = CHAR(name);
                    if !chars.is_null() && *chars != 0 {
                        SETTAG(cell, Rf_install(chars));
                    }
                }
            }
            call_args = cell;
        }
        let call_sexp = Rf_cons(fun, call_args);
        if !call_sexp.is_null() {
            (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        crate::eval::eval::Rf_eval(call_sexp, rho)
    }
}

fn callable_arg_by_name_or_position(args: SEXP, names: &[&str], position: usize) -> SEXP {
    unsafe { callable_expr(arg_by_name_or_position(args, names, position)) }
}

fn eval_arg_by_name_or_position(args: SEXP, names: &[&str], position: usize, rho: SEXP) -> SEXP {
    unsafe {
        let expr = arg_by_name_or_position(args, names, position);
        if expr.is_null() || expr == R_NilValue() {
            R_NilValue()
        } else {
            crate::eval::eval::Rf_eval(expr, rho)
        }
    }
}

fn callable_expr(fun: SEXP) -> SEXP {
    unsafe {
        if fun.is_null() || fun == R_NilValue() {
            return fun;
        }
        if TYPEOF(fun) == SEXPTYPE::STRSXP && XLENGTH(fun) > 0 {
            let charsxp = STRING_ELT(fun, 0);
            if charsxp.is_null() || charsxp == crate::sexp::globals::R_NaString() {
                return R_NilValue();
            }
            let name = CHAR(charsxp);
            if name.is_null() {
                R_NilValue()
            } else {
                Rf_install(name)
            }
        } else {
            fun
        }
    }
}

fn fun_value_type(template_expr: SEXP, rho: SEXP) -> SEXPTYPE {
    unsafe {
        if !template_expr.is_null()
            && template_expr != R_NilValue()
            && TYPEOF(template_expr) == SEXPTYPE::LANGSXP
        {
            let head = CAR(template_expr);
            if TYPEOF(head) == SEXPTYPE::SYMSXP {
                if let Some(name) = symbol_name(head) {
                    if let Some(template_type) = match name.as_str() {
                        "integer" => Some(SEXPTYPE::INTSXP),
                        "numeric" | "double" => Some(SEXPTYPE::REALSXP),
                        "logical" => Some(SEXPTYPE::LGLSXP),
                        "character" => Some(SEXPTYPE::STRSXP),
                        _ => None,
                    } {
                        return template_type;
                    }
                }
            }
        }
        let template = if template_expr.is_null() || template_expr == R_NilValue() {
            R_NilValue()
        } else {
            crate::eval::eval::Rf_eval(template_expr, rho)
        };
        SEXPTYPE(TYPEOF(template))
    }
}

fn apply_unary_value(fun: SEXP, value: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if value == R_MissingArg()
            && TYPEOF(fun) == SEXPTYPE::SYMSXP
            && symbol_name(fun).as_deref() == Some("typeof")
        {
            let args = Rf_cons(value, R_NilValue());
            let _args_guard = protect(args);
            return crate::mainutils::essentials_basic::do_typeof(
                R_NilValue(),
                R_NilValue(),
                args,
                rho,
            );
        }
        let arg_sym = Rf_install(c"..rport_apply_value".as_ptr());
        let call_env = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), rho, R_NilValue());
        crate::sexp::envir::defineVar(arg_sym, value, call_env);

        let call_args = Rf_cons(arg_sym, R_NilValue());
        let call_sexp = Rf_cons(fun, call_args);
        if !call_sexp.is_null() {
            (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        crate::eval::eval::Rf_eval(call_sexp, call_env)
    }
}

fn simplify_scalar_list(list: SEXP) -> SEXP {
    unsafe {
        if list.is_null() || TYPEOF(list) != SEXPTYPE::VECSXP {
            return list;
        }
        let n = XLENGTH(list);
        if n == 0 {
            return list;
        }
        let first = VECTOR_ELT(list, 0);
        if first.is_null() || XLENGTH(first) != 1 {
            return list;
        }
        simplify_scalar_list_as(list, SEXPTYPE(TYPEOF(first)))
    }
}

fn simplify_scalar_list_as(list: SEXP, elem_type: SEXPTYPE) -> SEXP {
    unsafe {
        if list.is_null() || TYPEOF(list) != SEXPTYPE::VECSXP {
            return list;
        }
        if elem_type != SEXPTYPE::REALSXP
            && elem_type != SEXPTYPE::INTSXP
            && elem_type != SEXPTYPE::LGLSXP
            && elem_type != SEXPTYPE::STRSXP
        {
            return list;
        }
        let n = XLENGTH(list);
        let result = Rf_allocVector3(elem_type, n);
        if result.is_null() {
            return list;
        }
        let _result_guard = protect(result);
        for i in 0..n {
            let elem = VECTOR_ELT(list, i as i64);
            if elem.is_null() || TYPEOF(elem) != elem_type || XLENGTH(elem) != 1 {
                return list;
            }
            if elem_type == SEXPTYPE::REALSXP {
                *REAL(result).add(i as usize) = *REAL(elem);
            } else if elem_type == SEXPTYPE::INTSXP {
                *INTEGER(result).add(i as usize) = *INTEGER(elem);
            } else if elem_type == SEXPTYPE::LGLSXP {
                *LOGICAL(result).add(i as usize) = *LOGICAL(elem);
            } else if elem_type == SEXPTYPE::STRSXP {
                SET_STRING_ELT(result, i, STRING_ELT(elem, 0));
            }
        }
        let names =
            crate::sexp::attrib_core::getAttrib(list, crate::sexp::attrib_core::R_NamesSymbol());
        if !names.is_null()
            && names != R_NilValue()
            && TYPEOF(names) == SEXPTYPE::STRSXP
            && XLENGTH(names) == n
        {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_NamesSymbol(),
                names,
            );
        }
        result
    }
}

pub(crate) fn extract_element(x: SEXP, i: R_xlen_t) -> SEXP {
    unsafe {
        let t = TYPEOF(x);
        if t == SEXPTYPE::VECSXP {
            return crate::sexp::accessors::VECTOR_ELT(x, i as i64);
        }
        if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP {
            let mut current = x;
            let mut index = 0;
            while !current.is_null() && current != R_NilValue() {
                if index == i {
                    return CAR(current);
                }
                index += 1;
                current = CDR(current);
            }
            return R_NilValue();
        }
        // Only allocate a scalar element for atomic vector inputs.
        // `Rf_allocVector3(TYPEOF(x), 1)` with an arbitrary type would
        // fabricate a node whose union payload is a vector header
        // ({length, truelength}) but whose SEXPTYPE is non-vector — the GC
        // then traces e.g. a PROMSXP-typed node whose value/expr slots hold
        // the integers 1/1 and whose env slot is uninitialized memory.
        if t != SEXPTYPE::REALSXP
            && t != SEXPTYPE::INTSXP
            && t != SEXPTYPE::LGLSXP
            && t != SEXPTYPE::CPLXSXP
            && t != SEXPTYPE::RAWSXP
            && t != SEXPTYPE::STRSXP
        {
            return R_NilValue();
        }
        let elem = Rf_allocVector3(t, 1);
        if elem.is_null() {
            return R_NilValue();
        }
        if t == SEXPTYPE::REALSXP {
            *REAL(elem) = *REAL(x).add(i as usize);
        } else if t == SEXPTYPE::INTSXP {
            *INTEGER(elem) = *INTEGER(x).add(i as usize);
        } else if t == SEXPTYPE::LGLSXP {
            *LOGICAL(elem) = *LOGICAL(x).add(i as usize);
        } else if t == SEXPTYPE::CPLXSXP {
            *crate::sexp::accessors::COMPLEX(elem) =
                *crate::sexp::accessors::COMPLEX(x).add(i as usize);
        } else if t == SEXPTYPE::RAWSXP {
            *crate::sexp::accessors::RAW(elem) = *crate::sexp::accessors::RAW(x).add(i as usize);
        } else if t == SEXPTYPE::STRSXP {
            crate::sexp::accessors::SET_STRING_ELT(
                elem,
                0,
                crate::sexp::accessors::STRING_ELT(x, i as i64),
            );
        }
        elem
    }
}

fn list_apply_len(x: SEXP) -> R_xlen_t {
    unsafe {
        match TYPEOF(x) {
            t if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP => {
                let mut len = 0;
                let mut current = x;
                while !current.is_null() && current != R_NilValue() {
                    len += 1;
                    current = CDR(current);
                }
                len
            }
            _ => XLENGTH(x),
        }
    }
}

fn list_apply_names(x: SEXP, n: R_xlen_t) -> SEXP {
    unsafe {
        if n <= 0 {
            return R_NilValue();
        }
        match TYPEOF(x) {
            t if t == SEXPTYPE::VECSXP => {
                let names = crate::sexp::attrib_core::getAttrib(
                    x,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                );
                if !names.is_null()
                    && names != R_NilValue()
                    && TYPEOF(names) == SEXPTYPE::STRSXP
                    && XLENGTH(names) == n
                {
                    names
                } else {
                    R_NilValue()
                }
            }
            t if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP => {
                let names = Rf_allocVector3(SEXPTYPE::STRSXP, n);
                if names.is_null() {
                    return R_NilValue();
                }
                let _names_guard = protect(names);
                let mut current = x;
                let mut i = 0;
                let mut any_name = false;
                while !current.is_null() && current != R_NilValue() && i < n {
                    let tag = TAG(current);
                    if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) == SEXPTYPE::SYMSXP {
                        let printname = PRINTNAME(tag);
                        if !printname.is_null() && printname != R_NilValue() {
                            SET_STRING_ELT(names, i, printname);
                            any_name = true;
                        } else {
                            SET_STRING_ELT(names, i, Rf_mkChar(c"".as_ptr()));
                        }
                    } else {
                        SET_STRING_ELT(names, i, Rf_mkChar(c"".as_ptr()));
                    }
                    i += 1;
                    current = CDR(current);
                }
                if any_name { names } else { R_NilValue() }
            }
            _ => R_NilValue(),
        }
    }
}

// ---------------------------------------------------------------------------
// apply / tapply / mapply / outer / sweep — higher-order array functions
// ---------------------------------------------------------------------------

/// Extract a row from a matrix (column-major storage) as a length-ncol vector.
unsafe fn extract_matrix_row(x: SEXP, nrow: R_xlen_t, ncol: R_xlen_t, row: R_xlen_t) -> SEXP {
    unsafe {
        let t = TYPEOF(x);
        let result = Rf_allocVector3(t, ncol);
        if result.is_null() {
            return R_NilValue();
        }
        for j in 0..ncol {
            let src = (j * nrow + row) as usize;
            if t == SEXPTYPE::REALSXP {
                *REAL(result).add(j as usize) = *REAL(x).add(src);
            } else if t == SEXPTYPE::INTSXP {
                *INTEGER(result).add(j as usize) = *INTEGER(x).add(src);
            } else if t == SEXPTYPE::LGLSXP {
                *LOGICAL(result).add(j as usize) = *LOGICAL(x).add(src);
            }
        }
        result
    }
}

/// Extract a column from a matrix (column-major storage) as a length-nrow vector.
unsafe fn extract_matrix_col(x: SEXP, nrow: R_xlen_t, _ncol: R_xlen_t, col: R_xlen_t) -> SEXP {
    unsafe {
        let t = TYPEOF(x);
        let result = Rf_allocVector3(t, nrow);
        if result.is_null() {
            return R_NilValue();
        }
        let offset = (col * nrow) as usize;
        if t == SEXPTYPE::REALSXP {
            for i in 0..nrow {
                *REAL(result).add(i as usize) = *REAL(x).add(offset + i as usize);
            }
        } else if t == SEXPTYPE::INTSXP {
            for i in 0..nrow {
                *INTEGER(result).add(i as usize) = *INTEGER(x).add(offset + i as usize);
            }
        } else if t == SEXPTYPE::LGLSXP {
            for i in 0..nrow {
                *LOGICAL(result).add(i as usize) = *LOGICAL(x).add(offset + i as usize);
            }
        }
        result
    }
}

/// R's `apply(X, MARGIN, FUN)` — apply FUN over margins of array/matrix.
///
/// For a 2D matrix:
/// - MARGIN=1: apply FUN to each row, return vector of length nrow
/// - MARGIN=2: apply FUN to each column, return vector of length ncol
pub unsafe fn do_apply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = eval_arg_by_name_or_position(args, &["X"], 0, rho);
        let margin_arg = eval_arg_by_name_or_position(args, &["MARGIN"], 1, rho);
        let fun = callable_arg_by_name_or_position(args, &["FUN"], 2);
        if x.is_null() || x == R_NilValue() || fun.is_null() {
            return R_NilValue();
        }

        // Get dimensions
        let dim_attr = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"dim".as_ptr()));
        if dim_attr.is_null() || TYPEOF(dim_attr) != SEXPTYPE::INTSXP || LENGTH(dim_attr) < 2 {
            return R_NilValue(); // not a matrix/array
        }
        let nrow = *INTEGER(dim_attr) as R_xlen_t;
        let ncol = *INTEGER(dim_attr).add(1) as R_xlen_t;
        let margin = real_or_default(margin_arg, 1.0) as i64;

        if margin == 1 {
            // Apply over rows
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, nrow);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            for i in 0..nrow {
                let row_vec = extract_matrix_row(x, nrow, ncol, i);
                let call_args = Rf_cons(row_vec, R_NilValue());
                let call_sexp = Rf_cons(fun, call_args);
                if !call_sexp.is_null() {
                    (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                }
                let val = crate::eval::eval::Rf_eval(call_sexp, rho);
                crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, val);
            }
            simplify_scalar_list(result)
        } else if margin == 2 {
            // Apply over columns
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncol);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            for j in 0..ncol {
                let col_vec = extract_matrix_col(x, nrow, ncol, j);
                let call_args = Rf_cons(col_vec, R_NilValue());
                let call_sexp = Rf_cons(fun, call_args);
                if !call_sexp.is_null() {
                    (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                }
                let val = crate::eval::eval::Rf_eval(call_sexp, rho);
                crate::sexp::accessors::SET_VECTOR_ELT(result, j as i64, val);
            }
            simplify_scalar_list(result)
        } else {
            R_NilValue()
        }
    }
}

/// R's `tapply(X, INDEX, FUN)` — apply FUN to each group defined by INDEX.
///
/// Iterates unique values of INDEX, collects matching elements from X, calls FUN on each group.
pub unsafe fn do_tapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let index = CAR(CDR(args));
        let fun = CAR(CDR(CDR(args)));
        if x.is_null() || x == R_NilValue() || index.is_null() || fun.is_null() {
            return R_NilValue();
        }
        if let Some(result) = tapply_numeric_array(x, index, fun, _call) {
            return result;
        }

        let n = XLENGTH(x);
        let idx_n = XLENGTH(index);

        // Collect unique index values and group membership
        let mut group_keys: Vec<i64> = Vec::new();
        let mut group_map: std::collections::BTreeMap<i64, usize> =
            std::collections::BTreeMap::new();
        let mut groups: Vec<Vec<R_xlen_t>> = Vec::new();

        let idx_t = TYPEOF(index);
        for i in 0..n {
            let idx_i = if idx_n == 0 { 0 } else { i % idx_n };
            let key = if idx_t == SEXPTYPE::INTSXP || idx_t == SEXPTYPE::LGLSXP {
                *INTEGER(index).add(idx_i as usize) as i64
            } else if idx_t == SEXPTYPE::REALSXP {
                (*REAL(index).add(idx_i as usize)).to_bits() as i64
            } else {
                idx_i as i64
            };

            if let Some(&g) = group_map.get(&key) {
                groups[g].push(i);
            } else {
                let g = groups.len();
                group_map.insert(key, g);
                group_keys.push(key);
                groups.push(vec![i]);
            }
        }

        let num_groups = groups.len() as R_xlen_t;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, num_groups);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for (g, indices) in groups.iter().enumerate() {
            let group_vec = Rf_allocVector3(TYPEOF(x), indices.len() as R_xlen_t);
            if !group_vec.is_null() {
                let t = TYPEOF(x);
                for (j, &src_i) in indices.iter().enumerate() {
                    if t == SEXPTYPE::REALSXP {
                        *REAL(group_vec).add(j) = *REAL(x).add(src_i as usize);
                    } else if t == SEXPTYPE::INTSXP {
                        *INTEGER(group_vec).add(j) = *INTEGER(x).add(src_i as usize);
                    } else if t == SEXPTYPE::LGLSXP {
                        *LOGICAL(group_vec).add(j) = *LOGICAL(x).add(src_i as usize);
                    }
                }
            }
            let call_args = Rf_cons(group_vec, R_NilValue());
            let call_sexp = Rf_cons(fun, call_args);
            if !call_sexp.is_null() {
                (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            }
            let val = crate::eval::eval::Rf_eval(call_sexp, rho);
            crate::sexp::accessors::SET_VECTOR_ELT(result, g as i64, val);
        }

        result
    }
}

struct TapplyIndex {
    labels: Vec<String>,
    row_codes: Vec<Option<usize>>,
    dim_name: Option<String>,
}

pub(crate) unsafe fn tapply_numeric_array(
    x: SEXP,
    index: SEXP,
    fun: SEXP,
    call: SEXP,
) -> Option<SEXP> {
    unsafe {
        let summary = aggregate_summary_fun(fun, call)?;
        let x_type = TYPEOF(x);
        if x_type != SEXPTYPE::INTSXP && x_type != SEXPTYPE::REALSXP {
            return None;
        }
        let n = XLENGTH(x);
        let indexes = tapply_indexes(index, n)?;
        if indexes.is_empty() || indexes.iter().any(|index| index.labels.is_empty()) {
            return None;
        }

        let dims = indexes
            .iter()
            .map(|index| index.labels.len())
            .collect::<Vec<_>>();
        let total_len = dims.iter().product::<usize>() as R_xlen_t;
        let mut states = vec![AggregateGroupState::new(); total_len as usize];

        for row in 0..n {
            let mut offset = 0_usize;
            let mut stride = 1_usize;
            let mut keep = true;
            for (index, dim) in indexes.iter().zip(dims.iter()) {
                match index.row_codes[row as usize] {
                    Some(code) => {
                        offset += code * stride;
                        stride *= *dim;
                    }
                    None => {
                        keep = false;
                        break;
                    }
                }
            }
            if keep {
                states[offset].record(tapply_value_at(x, x_type, row), summary);
            }
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP, total_len);
        if result.is_null() {
            return Some(result);
        }
        let _result_guard = protect(result);
        for (i, state) in states.into_iter().enumerate() {
            *REAL(result).add(i) = state.summarize(summary);
        }
        set_tapply_dim_attrs(result, &indexes);
        Some(result)
    }
}

unsafe fn tapply_indexes(index: SEXP, n: R_xlen_t) -> Option<Vec<TapplyIndex>> {
    unsafe {
        if TYPEOF(index) == SEXPTYPE::VECSXP {
            let mut indexes = Vec::with_capacity(XLENGTH(index) as usize);
            let names = crate::sexp::attrib_core::getAttrib(
                index,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            for i in 0..XLENGTH(index) {
                let dim_name = if !names.is_null()
                    && names != R_NilValue()
                    && TYPEOF(names) == SEXPTYPE::STRSXP
                    && XLENGTH(names) > i
                {
                    let name = string_at_or_empty(names, i);
                    if name.is_empty() { None } else { Some(name) }
                } else {
                    None
                };
                indexes.push(tapply_one_index(VECTOR_ELT(index, i), n, dim_name)?);
            }
            Some(indexes)
        } else {
            Some(vec![tapply_one_index(index, n, None)?])
        }
    }
}

unsafe fn tapply_one_index(
    index: SEXP,
    n: R_xlen_t,
    dim_name: Option<String>,
) -> Option<TapplyIndex> {
    unsafe {
        if index.is_null() || index == R_NilValue() || XLENGTH(index) == 0 {
            return None;
        }
        let index_type = TYPEOF(index);
        if index_type == SEXPTYPE::INTSXP {
            if let Some(levels) = aggregate_factor_levels(index) {
                let row_codes = (0..n)
                    .map(|row| {
                        let value = *INTEGER(index).add((row % XLENGTH(index)) as usize);
                        if value == NA_INTEGER || value <= 0 || value as usize > levels.len() {
                            None
                        } else {
                            Some((value - 1) as usize)
                        }
                    })
                    .collect();
                return Some(TapplyIndex {
                    labels: levels,
                    row_codes,
                    dim_name,
                });
            }
            let mut labels = std::collections::BTreeSet::<i32>::new();
            let mut values = Vec::with_capacity(n as usize);
            for row in 0..n {
                let value = *INTEGER(index).add((row % XLENGTH(index)) as usize);
                if value == NA_INTEGER {
                    values.push(None);
                } else {
                    labels.insert(value);
                    values.push(Some(value));
                }
            }
            let labels = labels.into_iter().collect::<Vec<_>>();
            let positions = labels
                .iter()
                .enumerate()
                .map(|(i, value)| (*value, i))
                .collect::<BTreeMap<_, _>>();
            let row_codes = values
                .into_iter()
                .map(|value| value.and_then(|value| positions.get(&value).copied()))
                .collect();
            return Some(TapplyIndex {
                labels: labels.into_iter().map(|value| value.to_string()).collect(),
                row_codes,
                dim_name,
            });
        }

        if index_type == SEXPTYPE::STRSXP {
            let mut labels = std::collections::BTreeSet::<String>::new();
            let mut values = Vec::with_capacity(n as usize);
            for row in 0..n {
                let elt = STRING_ELT(index, row % XLENGTH(index));
                if elt.is_null() || elt == crate::sexp::globals::R_NaString() {
                    values.push(None);
                } else {
                    let value = CStr::from_ptr(CHAR(elt)).to_string_lossy().into_owned();
                    labels.insert(value.clone());
                    values.push(Some(value));
                }
            }
            let labels = labels.into_iter().collect::<Vec<_>>();
            let positions = labels
                .iter()
                .enumerate()
                .map(|(i, value)| (value.clone(), i))
                .collect::<BTreeMap<_, _>>();
            let row_codes = values
                .into_iter()
                .map(|value| value.and_then(|value| positions.get(&value).copied()))
                .collect();
            return Some(TapplyIndex {
                labels,
                row_codes,
                dim_name,
            });
        }

        None
    }
}

unsafe fn tapply_value_at(x: SEXP, x_type: c_int, i: R_xlen_t) -> f64 {
    unsafe {
        if x_type == SEXPTYPE::REALSXP {
            *REAL(x).add(i as usize)
        } else {
            let value = *INTEGER(x).add(i as usize);
            if value == NA_INTEGER {
                NA_REAL
            } else {
                value as f64
            }
        }
    }
}

unsafe fn set_tapply_dim_attrs(result: SEXP, indexes: &[TapplyIndex]) {
    unsafe {
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, indexes.len() as R_xlen_t);
        if dim.is_null() {
            return;
        }
        let _dim_guard = protect(dim);
        for (i, index) in indexes.iter().enumerate() {
            *INTEGER(dim).add(i) = index.labels.len() as i32;
        }
        crate::sexp::attrib_core::setAttrib(result, crate::sexp::attrib_core::R_DimSymbol(), dim);

        let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, indexes.len() as R_xlen_t);
        if dimnames.is_null() {
            return;
        }
        let _dimnames_guard = protect(dimnames);
        for (i, index) in indexes.iter().enumerate() {
            let names = Rf_allocVector3(SEXPTYPE::STRSXP, index.labels.len() as R_xlen_t);
            if names.is_null() {
                return;
            }
            let _names_guard = protect(names);
            for (j, label) in index.labels.iter().enumerate() {
                let label_c = CString::new(label.as_str()).unwrap_or_default();
                SET_STRING_ELT(names, j as R_xlen_t, Rf_mkChar(label_c.as_ptr()));
            }
            SET_VECTOR_ELT(dimnames, i as R_xlen_t, names);
        }
        let dimname_names = Rf_allocVector3(SEXPTYPE::STRSXP, indexes.len() as R_xlen_t);
        if !dimname_names.is_null() {
            let _dimname_names_guard = protect(dimname_names);
            for (i, index) in indexes.iter().enumerate() {
                let label_c =
                    CString::new(index.dim_name.as_deref().unwrap_or("")).unwrap_or_default();
                SET_STRING_ELT(dimname_names, i as R_xlen_t, Rf_mkChar(label_c.as_ptr()));
            }
            crate::sexp::attrib_core::setAttrib(
                dimnames,
                crate::sexp::attrib_core::R_NamesSymbol(),
                dimname_names,
            );
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dimnames,
        );
    }
}

/// R's `mapply(FUN, ...)` — multivariate sapply. Applies FUN element-wise across multiple vectors with recycling.
pub unsafe fn do_mapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let fun = CAR(args);
        let vec_args = CDR(args);
        if fun.is_null() {
            return R_NilValue();
        }

        // Collect vector args, find max length
        let mut arg_vecs: Vec<SEXP> = Vec::new();
        let mut max_len: R_xlen_t = 0;
        let mut current = vec_args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                arg_vecs.push(arg);
                let n = XLENGTH(arg);
                if n > max_len {
                    max_len = n;
                }
            }
            current = CDR(current);
        }
        if max_len == 0 {
            return R_NilValue();
        }

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, max_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..max_len {
            // Build call: FUN(arg1[i], arg2[i], ...) with recycling
            let mut call_args = R_NilValue();
            for &arg in arg_vecs.iter().rev() {
                let n = XLENGTH(arg);
                let idx = if n == 0 { 0 } else { i % n };
                let elem = extract_element(arg, idx);
                call_args = Rf_cons(elem, call_args);
            }
            let call_sexp = Rf_cons(fun, call_args);
            if !call_sexp.is_null() {
                (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            }
            let val = crate::eval::eval::Rf_eval(call_sexp, rho);
            crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, val);
        }

        result
    }
}

/// R's `outer(X, Y, FUN="*")` — outer product. Returns a matrix of length(X) x length(Y).
///
/// For each pair (x_i, y_j), computes FUN(x_i, y_j).
pub unsafe fn do_outer(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        let fun_arg = CAR(CDR(CDR(args)));
        if x.is_null() || x == R_NilValue() || y.is_null() || y == R_NilValue() {
            return R_NilValue();
        }

        let nx = XLENGTH(x);
        let ny = XLENGTH(y);

        // Determine if FUN is a symbol (operator name) or a function object
        let use_multiply = if fun_arg.is_null() || fun_arg == R_NilValue() {
            true
        } else if TYPEOF(fun_arg) == SEXPTYPE::STRSXP {
            elt_to_string(fun_arg, 0) == "*"
        } else if TYPEOF(fun_arg) == SEXPTYPE::SYMSXP {
            let pname = crate::sexp::accessors::PRINTNAME(fun_arg);
            if !pname.is_null() {
                let s = crate::sexp::accessors::CHAR(pname);
                if !s.is_null() {
                    std::ffi::CStr::from_ptr(s).to_str().unwrap_or("") == "*"
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        let result = Rf_allocVector3(SEXPTYPE::REALSXP, nx * ny);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);

        if use_multiply {
            // Fast path: multiply
            for i in 0..nx {
                let xi = elt_real_safe(x, i);
                for j in 0..ny {
                    let yj = elt_real_safe(y, j);
                    *dst.add((j * nx + i) as usize) = xi * yj;
                }
            }
        } else {
            // General path: call FUN(x_i, y_j) for each pair
            for i in 0..nx {
                let xi = extract_element(x, i);
                for j in 0..ny {
                    let yj = extract_element(y, j);
                    let call_args = Rf_cons(xi, Rf_cons(yj, R_NilValue()));
                    let call_sexp = Rf_cons(fun_arg, call_args);
                    if !call_sexp.is_null() {
                        (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                    }
                    let val = crate::eval::eval::Rf_eval(call_sexp, rho);
                    let v = if !val.is_null() && TYPEOF(val) == SEXPTYPE::REALSXP {
                        *REAL(val)
                    } else if !val.is_null()
                        && (TYPEOF(val) == SEXPTYPE::INTSXP || TYPEOF(val) == SEXPTYPE::LGLSXP)
                    {
                        let iv = *INTEGER(val);
                        if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
                    } else {
                        NA_REAL
                    };
                    *dst.add((j * nx + i) as usize) = v;
                }
            }
        }

        // Set dim attribute: c(nx, ny)
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            *INTEGER(dim) = nx as c_int;
            *INTEGER(dim).add(1) = ny as c_int;
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"dim".as_ptr()), dim);
        }

        result
    }
}

/// R's `sweep(x, MARGIN, STATS, FUN="-")` — sweep out statistics from array.
///
/// For each row/column, applies FUN(x, STATS) element-wise.
pub unsafe fn do_sweep(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let margin_arg = CAR(CDR(args));
        let stats = CAR(CDR(CDR(args)));
        let fun_arg = CAR(CDR(CDR(CDR(args))));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        // Determine operation
        let op_str = if fun_arg.is_null() || fun_arg == R_NilValue() {
            "-".to_string()
        } else if TYPEOF(fun_arg) == SEXPTYPE::STRSXP {
            elt_to_string(fun_arg, 0)
        } else if TYPEOF(fun_arg) == SEXPTYPE::SYMSXP {
            let pname = crate::sexp::accessors::PRINTNAME(fun_arg);
            if !pname.is_null() {
                let s = crate::sexp::accessors::CHAR(pname);
                if !s.is_null() {
                    std::ffi::CStr::from_ptr(s)
                        .to_str()
                        .unwrap_or("-")
                        .to_string()
                } else {
                    "-".to_string()
                }
            } else {
                "-".to_string()
            }
        } else {
            String::new()
        };

        let margin = if margin_arg.is_null() || margin_arg == R_NilValue() {
            1
        } else {
            real_or_default(margin_arg, 1.0) as i64
        };

        let t = TYPEOF(x);
        let n = XLENGTH(x);

        // Get dimensions
        let dim_attr = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"dim".as_ptr()));
        let (nrow, ncol) =
            if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2
            {
                (
                    *INTEGER(dim_attr) as R_xlen_t,
                    *INTEGER(dim_attr).add(1) as R_xlen_t,
                )
            } else {
                (n, 1)
            };

        let result = Rf_allocVector3(t, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        // Fast path for common ops
        let apply_binary = |src_val: f64, stat_val: f64| -> f64 {
            match op_str.as_str() {
                "-" => src_val - stat_val,
                "+" => src_val + stat_val,
                "*" => src_val * stat_val,
                "/" => {
                    if stat_val != 0.0 {
                        src_val / stat_val
                    } else {
                        NA_REAL
                    }
                }
                _ => src_val - stat_val,
            }
        };

        if margin == 1 {
            // Sweep across rows: subtract STATS from each row
            let stats_len = if stats.is_null() || stats == R_NilValue() {
                0
            } else {
                XLENGTH(stats)
            };
            for i in 0..nrow {
                for j in 0..ncol {
                    let src_idx = (j * nrow + i) as usize;
                    let stat_idx = if stats_len == 0 { 0 } else { j % stats_len };
                    let src_val = if t == SEXPTYPE::REALSXP {
                        *REAL(x).add(src_idx)
                    } else if t == SEXPTYPE::INTSXP {
                        let v = *INTEGER(x).add(src_idx);
                        if v == NA_INTEGER { NA_REAL } else { v as f64 }
                    } else {
                        NA_REAL
                    };
                    let stat_val = if stats.is_null() || stats == R_NilValue() {
                        0.0
                    } else {
                        elt_real_safe(stats, stat_idx)
                    };
                    let res = apply_binary(src_val, stat_val);
                    if t == SEXPTYPE::REALSXP {
                        *REAL(result).add(src_idx) = res;
                    } else if t == SEXPTYPE::INTSXP {
                        *INTEGER(result).add(src_idx) = if res.is_nan() || res == NA_REAL {
                            NA_INTEGER
                        } else {
                            res as c_int
                        };
                    }
                }
            }
        } else if margin == 2 {
            // Sweep across columns: subtract STATS from each column
            let stats_len = if stats.is_null() || stats == R_NilValue() {
                0
            } else {
                XLENGTH(stats)
            };
            for j in 0..ncol {
                for i in 0..nrow {
                    let src_idx = (j * nrow + i) as usize;
                    let stat_idx = if stats_len == 0 { 0 } else { i % stats_len };
                    let src_val = if t == SEXPTYPE::REALSXP {
                        *REAL(x).add(src_idx)
                    } else if t == SEXPTYPE::INTSXP {
                        let v = *INTEGER(x).add(src_idx);
                        if v == NA_INTEGER { NA_REAL } else { v as f64 }
                    } else {
                        NA_REAL
                    };
                    let stat_val = if stats.is_null() || stats == R_NilValue() {
                        0.0
                    } else {
                        elt_real_safe(stats, stat_idx)
                    };
                    let res = apply_binary(src_val, stat_val);
                    if t == SEXPTYPE::REALSXP {
                        *REAL(result).add(src_idx) = res;
                    } else if t == SEXPTYPE::INTSXP {
                        *INTEGER(result).add(src_idx) = if res.is_nan() || res == NA_REAL {
                            NA_INTEGER
                        } else {
                            res as c_int
                        };
                    }
                }
            }
        }

        // Copy dim attribute if present
        if !dim_attr.is_null() {
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"dim".as_ptr()), dim_attr);
        }

        result
    }
}

// ---------------------------------------------------------------------------
// List / data.frame operations
// ---------------------------------------------------------------------------

/// R's `list(...)` — create a VECSXP (list) from arguments.
pub unsafe fn do_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut n: R_xlen_t = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            n += 1;
            current = CDR(current);
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let mut i: R_xlen_t = 0;
        current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            SET_VECTOR_ELT(result, i as i64, arg);
            i += 1;
            current = CDR(current);
        }
        // Copy names from the pairlist tags if present
        let mut name_parts: Vec<String> = Vec::new();
        let mut has_names = false;
        current = args;
        while !current.is_null() && current != R_NilValue() {
            let tag = (*current).data.listsxp.tagval;
            if !tag.is_null() && tag != R_NilValue() {
                let pname = crate::sexp::accessors::PRINTNAME(tag);
                if !pname.is_null() {
                    let s = crate::sexp::accessors::CHAR(pname);
                    if !s.is_null() {
                        name_parts.push(
                            std::ffi::CStr::from_ptr(s)
                                .to_str()
                                .unwrap_or("")
                                .to_string(),
                        );
                        has_names = true;
                    } else {
                        name_parts.push(String::new());
                    }
                } else {
                    name_parts.push(String::new());
                }
            } else {
                name_parts.push(String::new());
            }
            current = CDR(current);
        }
        if has_names {
            let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            if !names_vec.is_null() {
                let _names_guard = protect(names_vec);
                for (j, name) in name_parts.iter().enumerate() {
                    let cstr = CString::new(name.as_str()).unwrap_or_default();
                    let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                    if !charsxp.is_null() {
                        let data = (*names_vec).gengc_next_node as *mut SEXP;
                        *data.add(j) = charsxp;
                    }
                }
                crate::sexp::attrib_core::setAttrib(
                    result,
                    Rf_install(c"names".as_ptr()),
                    names_vec,
                );
            }
        }
        result
    }
}

pub(crate) unsafe fn string_at_or_empty(x: SEXP, index: R_xlen_t) -> String {
    unsafe {
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::STRSXP || index >= XLENGTH(x)
        {
            return String::new();
        }
        let value = STRING_ELT(x, index);
        if value.is_null() || value == crate::sexp::globals::R_NaString() {
            return String::new();
        }
        CStr::from_ptr(CHAR(value)).to_string_lossy().into_owned()
    }
}

pub(crate) unsafe fn set_string_names(x: SEXP, names: &[String]) {
    unsafe {
        let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as R_xlen_t);
        if names_vec.is_null() {
            return;
        }
        let _names_guard = protect(names_vec);
        for (i, name) in names.iter().enumerate() {
            let cstr = CString::new(name.as_str()).unwrap_or_default();
            SET_STRING_ELT(names_vec, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names_vec,
        );
    }
}

pub(crate) unsafe fn set_compact_row_names(x: SEXP, nrow: R_xlen_t) {
    unsafe {
        let rn = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if rn.is_null() {
            return;
        }
        let _row_names_guard = protect(rn);
        *INTEGER(rn) = NA_INTEGER;
        *INTEGER(rn).add(1) = -(nrow as i32);
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_RowNamesSymbol(), rn);
    }
}

pub(crate) unsafe fn set_data_frame_class(x: SEXP) {
    unsafe {
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if class_vec.is_null() {
            return;
        }
        let _class_guard = protect(class_vec);
        SET_STRING_ELT(class_vec, 0, Rf_mkChar(c"data.frame".as_ptr()));
        crate::sexp::attrib_core::setAttrib(x, Rf_install(c"class".as_ptr()), class_vec);
    }
}

pub(crate) unsafe fn set_summary_default_class(x: SEXP) {
    unsafe {
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        if class_vec.is_null() {
            return;
        }
        let _class_guard = protect(class_vec);
        SET_STRING_ELT(class_vec, 0, Rf_mkChar(c"summaryDefault".as_ptr()));
        SET_STRING_ELT(class_vec, 1, Rf_mkChar(c"table".as_ptr()));
        crate::sexp::attrib_core::setAttrib(x, Rf_install(c"class".as_ptr()), class_vec);
    }
}

fn repair_data_frame_names(names: &mut [String]) {
    let mut used: BTreeMap<String, usize> = BTreeMap::new();
    for (i, name) in names.iter_mut().enumerate() {
        if name.is_empty() {
            *name = format!("X{}", i + 1);
        }
        let base = name.clone();
        let mut suffix = *used.get(&base).unwrap_or(&0);
        if suffix == 0 && !used.contains_key(&base) {
            used.insert(base, 1);
            continue;
        }
        loop {
            let candidate = format!("{base}.{suffix}");
            suffix += 1;
            if !used.contains_key(&candidate) {
                used.insert(base.clone(), suffix);
                used.insert(candidate.clone(), 1);
                *name = candidate;
                break;
            }
        }
    }
}

unsafe fn recycle_column_if_needed(x: SEXP, target_len: R_xlen_t) -> SEXP {
    unsafe {
        let len = XLENGTH(x);
        if len == target_len || target_len == 0 {
            return x;
        }
        if len != 1 {
            base_error(format!(
                "arguments imply differing number of rows: {target_len}, {len}"
            ));
        }
        let ty = TYPEOF(x);
        let out = Rf_allocVector3(ty, target_len);
        if out.is_null() {
            return out;
        }
        let _out_guard = protect(out);
        for i in 0..target_len {
            match ty {
                t if t == SEXPTYPE::REALSXP => *REAL(out).add(i as usize) = *REAL(x),
                t if t == SEXPTYPE::INTSXP => *INTEGER(out).add(i as usize) = *INTEGER(x),
                t if t == SEXPTYPE::LGLSXP => *LOGICAL(out).add(i as usize) = *LOGICAL(x),
                t if t == SEXPTYPE::STRSXP => SET_STRING_ELT(out, i, STRING_ELT(x, 0)),
                _ => return x,
            }
        }
        out
    }
}

/// R's `data.frame(...)`: build a data-frame list while expanding data-frame arguments.
pub unsafe fn do_data_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let initial = do_list(_call, _op, args, _rho);
        if initial.is_null() || initial == R_NilValue() {
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
            if !result.is_null() {
                let _result_guard = protect(result);
                set_string_names(result, &[]);
                set_compact_row_names(result, 0);
                set_data_frame_class(result);
            }
            return result;
        }
        let _initial_guard = protect(initial);
        let arg_names =
            crate::sexp::attrib_core::getAttrib(initial, crate::sexp::attrib_core::R_NamesSymbol());
        let mut columns: Vec<SEXP> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        let mut nrow: Option<R_xlen_t> = None;

        for i in 0..XLENGTH(initial) {
            let value = VECTOR_ELT(initial, i);
            let arg_name = string_at_or_empty(arg_names, i);
            if sexp_has_class(value, "data.frame") && TYPEOF(value) == SEXPTYPE::VECSXP {
                let inner_names = crate::sexp::attrib_core::getAttrib(
                    value,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                );
                for j in 0..XLENGTH(value) {
                    let column = VECTOR_ELT(value, j);
                    let len = XLENGTH(column);
                    match nrow {
                        Some(existing) if len != existing => base_error(format!(
                            "arguments imply differing number of rows: {existing}, {len}"
                        )),
                        None => nrow = Some(len),
                        _ => {}
                    }
                    columns.push(column);
                    let child_name = string_at_or_empty(inner_names, j);
                    names.push(if arg_name.is_empty() {
                        child_name
                    } else if child_name.is_empty() {
                        arg_name.clone()
                    } else {
                        format!("{arg_name}.{child_name}")
                    });
                }
            } else {
                let len = XLENGTH(value);
                match nrow {
                    Some(existing) if len != existing && len != 1 => base_error(format!(
                        "arguments imply differing number of rows: {existing}, {len}"
                    )),
                    None => nrow = Some(len),
                    _ => {}
                }
                columns.push(value);
                names.push(arg_name);
            }
        }

        repair_data_frame_names(&mut names);
        let row_count = nrow.unwrap_or(0);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, columns.len() as R_xlen_t);
        if result.is_null() {
            return result;
        }
        let _result_guard = protect(result);
        for (i, column) in columns.iter().enumerate() {
            SET_VECTOR_ELT(
                result,
                i as R_xlen_t,
                recycle_column_if_needed(*column, row_count),
            );
        }
        set_string_names(result, &names);
        set_compact_row_names(result, row_count);
        set_data_frame_class(result);

        result
    }
}

// ---------------------------------------------------------------------------
// List operations
// ---------------------------------------------------------------------------

/// R's `lengths(x)` alias — lengths of list elements.
/// Wrapper that delegates to do_lengths (already registered separately).
pub unsafe fn do_length_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_lengths(_call, _op, args, _rho) }
}

/// R's `names(x)` for lists — names of list elements.
/// Wrapper that delegates to do_names.
pub unsafe fn do_names_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_names(_call, _op, args, _rho) }
}

/// R's `[[i]]` — get element i from a list (1-indexed).
pub unsafe fn do_list_get(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let i = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || i.is_null() || i == R_NilValue() {
            return R_NilValue();
        }
        let idx = real_or_default(i, 0.0) as i64;
        if idx < 1 || TYPEOF(x) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let n = XLENGTH(x) as i64;
        if idx > n {
            return R_NilValue();
        }
        VECTOR_ELT(x, idx - 1)
    }
}

/// R's `[[i]] <- value` — set element i in a list (1-indexed).
pub unsafe fn do_list_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let i = CAR(CDR(args));
        let value = CAR(CDR(CDR(args)));
        if x.is_null() || x == R_NilValue() || i.is_null() || i == R_NilValue() {
            return R_NilValue();
        }
        let idx = real_or_default(i, 0.0) as i64;
        if idx < 1 || TYPEOF(x) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let n = XLENGTH(x) as i64;
        if idx > n {
            return R_NilValue();
        }
        SET_VECTOR_ELT(x, idx - 1, value);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `c(...)` for lists — concatenate lists together.
/// If all args are VECSXP, result is a flattened VECSXP.
pub unsafe fn do_c_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut total_len: R_xlen_t = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                total_len += XLENGTH(arg);
            }
            current = CDR(current);
        }
        if total_len == 0 {
            return Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, total_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let mut offset: R_xlen_t = 0;
        current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                let n = XLENGTH(arg);
                if TYPEOF(arg) == SEXPTYPE::VECSXP {
                    for i in 0..n {
                        SET_VECTOR_ELT(result, (offset + i) as i64, VECTOR_ELT(arg, i as i64));
                    }
                } else {
                    // Wrap scalar/vector in a single slot
                    SET_VECTOR_ELT(result, offset as i64, arg);
                }
                offset += n;
            }
            current = CDR(current);
        }
        result
    }
}

/// R's `unlist(x)` — flatten nested list to a vector.
/// Simplified: if list elements are all numeric, return REALSXP.
pub unsafe fn do_unlist(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        if TYPEOF(x) != SEXPTYPE::VECSXP {
            return x;
        }
        let recursive = logical_arg_by_name_or_position(args, "recursive", 1)
            .or_else(|| logical_from_raw_arg(args, 1))
            .unwrap_or(true);
        let use_names = logical_arg_by_name_or_position(args, "use.names", 2)
            .or_else(|| logical_from_raw_arg(args, 2))
            .unwrap_or(true);
        let mut entries = Vec::new();
        collect_unlist_entries(x, None, recursive, use_names, &mut entries);
        let result_type = unlist_result_type(&entries);
        let total = entries.len() as R_xlen_t;

        let result = Rf_allocVector3(result_type, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for (idx, entry) in entries.iter().enumerate() {
            match result_type {
                t if t == SEXPTYPE::STRSXP => {
                    let cstr = CString::new(entry.value.as_string()).unwrap_or_default();
                    let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                    if !charsxp.is_null() {
                        let data = (*result).gengc_next_node as *mut SEXP;
                        *data.add(idx) = charsxp;
                    }
                }
                t if t == SEXPTYPE::CPLXSXP => {
                    *COMPLEX(result).add(idx) = entry.value.as_complex();
                }
                t if t == SEXPTYPE::REALSXP => {
                    *REAL(result).add(idx) = entry.value.as_real();
                }
                t if t == SEXPTYPE::VECSXP => {
                    SET_VECTOR_ELT(result, idx as R_xlen_t, entry.value.as_sexp());
                }
                _ => {
                    *INTEGER(result).add(idx) = entry.value.as_integer();
                }
            }
        }

        if use_names && entries.iter().any(|entry| entry.name.is_some()) {
            let names = Rf_allocVector3(SEXPTYPE::STRSXP, total);
            if !names.is_null() {
                let _names_guard = protect(names);
                for (idx, entry) in entries.iter().enumerate() {
                    let cstr =
                        CString::new(entry.name.as_deref().unwrap_or("")).unwrap_or_default();
                    SET_STRING_ELT(names, idx as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
                }
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    names,
                );
            }
        }

        result
    }
}

unsafe fn logical_from_raw_arg(args: SEXP, position: usize) -> Option<bool> {
    unsafe {
        let mut current = args;
        for _ in 0..position {
            if current.is_null() || current == R_NilValue() {
                return None;
            }
            current = CDR(current);
        }
        if current.is_null() || current == R_NilValue() {
            return None;
        }
        let value = CAR(current);
        if value.is_null() || value == R_NilValue() || XLENGTH(value) == 0 {
            return None;
        }
        let raw = if TYPEOF(value) == SEXPTYPE::LGLSXP || TYPEOF(value) == SEXPTYPE::INTSXP {
            *INTEGER(value)
        } else if TYPEOF(value) == SEXPTYPE::REALSXP {
            let value = *REAL(value);
            if ISNAN(value) {
                NA_LOGICAL
            } else {
                value as c_int
            }
        } else {
            return None;
        };
        (raw != NA_INTEGER).then_some(raw != 0)
    }
}

struct UnlistEntry {
    value: UnlistValue,
    name: Option<String>,
}

enum UnlistValue {
    Logical(i32),
    Integer(i32),
    Real(f64),
    Complex(Rcomplex),
    String(String),
    Object(SEXP),
}

impl UnlistValue {
    fn as_integer(&self) -> i32 {
        match self {
            Self::Logical(value) | Self::Integer(value) => *value,
            Self::Real(value) => {
                if value.to_bits() == R_NA_BIT_PATTERN || value.is_nan() {
                    NA_INTEGER
                } else {
                    *value as i32
                }
            }
            Self::Complex(_) | Self::String(_) | Self::Object(_) => NA_INTEGER,
        }
    }

    fn as_real(&self) -> f64 {
        match self {
            Self::Logical(value) | Self::Integer(value) => {
                if *value == NA_INTEGER {
                    NA_REAL
                } else {
                    *value as f64
                }
            }
            Self::Real(value) => *value,
            Self::Complex(value) => value.r,
            Self::String(_) | Self::Object(_) => NA_REAL,
        }
    }

    fn as_complex(&self) -> Rcomplex {
        match self {
            Self::Logical(value) | Self::Integer(value) => Rcomplex {
                r: if *value == NA_INTEGER {
                    NA_REAL
                } else {
                    *value as f64
                },
                i: 0.0,
            },
            Self::Real(value) => Rcomplex { r: *value, i: 0.0 },
            Self::Complex(value) => *value,
            Self::String(_) | Self::Object(_) => Rcomplex {
                r: NA_REAL,
                i: NA_REAL,
            },
        }
    }

    fn as_string(&self) -> String {
        match self {
            Self::Logical(value) => match *value {
                TRUE => "TRUE".to_string(),
                FALSE => "FALSE".to_string(),
                _ => "NA".to_string(),
            },
            Self::Integer(value) => {
                if *value == NA_INTEGER {
                    "NA".to_string()
                } else {
                    value.to_string()
                }
            }
            Self::Real(value) => {
                if value.to_bits() == R_NA_BIT_PATTERN || value.is_nan() {
                    "NA".to_string()
                } else {
                    value.to_string()
                }
            }
            Self::Complex(value) => format!(
                "{}{}{}i",
                value.r,
                if value.i < 0.0 { "" } else { "+" },
                value.i
            ),
            Self::String(value) => value.clone(),
            Self::Object(value) => elt_to_string(*value, 0),
        }
    }

    fn as_sexp(&self) -> SEXP {
        match self {
            Self::Object(value) => *value,
            _ => unsafe { R_NilValue() },
        }
    }
}

fn unlist_result_type(entries: &[UnlistEntry]) -> SEXPTYPE {
    if entries
        .iter()
        .any(|entry| matches!(entry.value, UnlistValue::Object(_)))
    {
        SEXPTYPE::VECSXP
    } else if entries
        .iter()
        .any(|entry| matches!(entry.value, UnlistValue::String(_)))
    {
        SEXPTYPE::STRSXP
    } else if entries
        .iter()
        .any(|entry| matches!(entry.value, UnlistValue::Complex(_)))
    {
        SEXPTYPE::CPLXSXP
    } else if entries
        .iter()
        .any(|entry| matches!(entry.value, UnlistValue::Real(_)))
    {
        SEXPTYPE::REALSXP
    } else {
        SEXPTYPE::INTSXP
    }
}

unsafe fn collect_unlist_entries(
    x: SEXP,
    prefix: Option<String>,
    recursive: bool,
    use_names: bool,
    out: &mut Vec<UnlistEntry>,
) {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return;
        }
        if TYPEOF(x) == SEXPTYPE::VECSXP || TYPEOF(x) == SEXPTYPE::EXPRSXP {
            let names =
                crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
            if !recursive && prefix.is_some() {
                for i in 0..XLENGTH(x) {
                    let child_name = if use_names {
                        unlist_element_name(prefix.as_deref(), names, i, XLENGTH(x))
                    } else {
                        None
                    };
                    out.push(UnlistEntry {
                        value: UnlistValue::Object(VECTOR_ELT(x, i)),
                        name: child_name,
                    });
                }
                return;
            }
            for i in 0..XLENGTH(x) {
                let child_name = if use_names {
                    unlist_element_name(prefix.as_deref(), names, i, XLENGTH(x))
                } else {
                    None
                };
                collect_unlist_entries(VECTOR_ELT(x, i), child_name, recursive, use_names, out);
            }
            return;
        }
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        for i in 0..XLENGTH(x) {
            let name = if use_names {
                unlist_element_name(prefix.as_deref(), names, i, XLENGTH(x))
            } else {
                None
            };
            let value = match TYPEOF(x) {
                t if t == SEXPTYPE::LGLSXP => UnlistValue::Logical(*LOGICAL(x).add(i as usize)),
                t if t == SEXPTYPE::INTSXP => UnlistValue::Integer(*INTEGER(x).add(i as usize)),
                t if t == SEXPTYPE::REALSXP => UnlistValue::Real(*REAL(x).add(i as usize)),
                t if t == SEXPTYPE::CPLXSXP => UnlistValue::Complex(*COMPLEX(x).add(i as usize)),
                t if t == SEXPTYPE::STRSXP => {
                    let string = STRING_ELT(x, i);
                    if string.is_null() || string == crate::sexp::globals::R_NaString() {
                        UnlistValue::String("NA".to_string())
                    } else {
                        UnlistValue::String(
                            CStr::from_ptr(CHAR(string)).to_string_lossy().into_owned(),
                        )
                    }
                }
                _ => UnlistValue::String(elt_to_string(x, i)),
            };
            out.push(UnlistEntry { value, name });
        }
    }
}

unsafe fn unlist_element_name(
    prefix: Option<&str>,
    names: SEXP,
    index: R_xlen_t,
    len: R_xlen_t,
) -> Option<String> {
    unsafe {
        let own = if !names.is_null()
            && names != R_NilValue()
            && TYPEOF(names) == SEXPTYPE::STRSXP
            && index < XLENGTH(names)
        {
            let value = string_at_or_empty(names, index);
            (!value.is_empty()).then_some(value)
        } else {
            None
        };

        match (prefix, own) {
            (Some(prefix), Some(own)) => Some(format!("{prefix}.{own}")),
            (None, Some(own)) => Some(own),
            (Some(prefix), None) if len > 1 => Some(format!("{}{}", prefix, index + 1)),
            (Some(prefix), None) => Some(prefix.to_string()),
            (None, None) => None,
        }
    }
}

/// R's `is.atomic(x)` — TRUE for non-recursive types (not list, pairlist, etc.).
pub unsafe fn do_is_atomic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(TRUE);
        }
        let t = TYPEOF(x);
        let is_atomic = t == SEXPTYPE::LGLSXP
            || t == SEXPTYPE::INTSXP
            || t == SEXPTYPE::REALSXP
            || t == SEXPTYPE::CPLXSXP
            || t == SEXPTYPE::STRSXP
            || t == SEXPTYPE::RAWSXP
            || t == SEXPTYPE::CHARSXP
            || t == SEXPTYPE::NILSXP;
        Rf_ScalarLogical(if is_atomic { TRUE } else { FALSE })
    }
}

/// R's `is.recursive(x)` — TRUE for recursive types (list, pairlist, language, etc.).
pub unsafe fn do_is_recursive(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        let is_rec = t == SEXPTYPE::VECSXP
            || t == SEXPTYPE::LISTSXP
            || t == SEXPTYPE::LANGSXP
            || t == SEXPTYPE::CLOSXP
            || t == SEXPTYPE::BUILTINSXP
            || t == SEXPTYPE::SPECIALSXP
            || t == SEXPTYPE::ENVSXP
            || t == SEXPTYPE::EXPRSXP;
        Rf_ScalarLogical(if is_rec { TRUE } else { FALSE })
    }
}

/// R's `is.object(x)` — TRUE if x has a "class" attribute.
pub unsafe fn do_is_object(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let class = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"class".as_ptr()));
        Rf_ScalarLogical(if !class.is_null() && class != R_NilValue() {
            TRUE
        } else {
            FALSE
        })
    }
}

// ---------------------------------------------------------------------------
// List operations
// ---------------------------------------------------------------------------

/// R-like `list.append(x, ...)` — append elements to a list.
pub unsafe fn do_list_append(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let rest = CDR(args);
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let mut extra_count: R_xlen_t = 0;
        let mut cur = rest;
        while !cur.is_null() && cur != R_NilValue() {
            extra_count += 1;
            cur = CDR(cur);
        }

        let total = n + extra_count;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        // Copy original elements
        for i in 0..n {
            SET_VECTOR_ELT(result, i as i64, VECTOR_ELT(x, i));
        }

        // Append new elements
        let mut offset = n;
        cur = rest;
        while !cur.is_null() && cur != R_NilValue() {
            let elem = CAR(cur);
            SET_VECTOR_ELT(result, offset as i64, elem);
            offset += 1;
            cur = CDR(cur);
        }
        result
    }
}

/// R-like `list.prepend(x, ...)` — prepend elements to a list.
pub unsafe fn do_list_prepend(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let rest = CDR(args);
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let mut extra_count: R_xlen_t = 0;
        let mut cur = rest;
        while !cur.is_null() && cur != R_NilValue() {
            extra_count += 1;
            cur = CDR(cur);
        }

        let total = n + extra_count;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        // Prepend new elements
        let mut offset: R_xlen_t = 0;
        cur = rest;
        while !cur.is_null() && cur != R_NilValue() {
            let elem = CAR(cur);
            SET_VECTOR_ELT(result, offset as i64, elem);
            offset += 1;
            cur = CDR(cur);
        }

        // Copy original elements
        for i in 0..n {
            SET_VECTOR_ELT(result, (offset + i) as i64, VECTOR_ELT(x, i));
        }
        result
    }
}

/// R-like `compact(x)` — remove NULL elements from a list.
pub unsafe fn do_compact(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::VECSXP {
            return x;
        }

        let n = XLENGTH(x);
        let mut kept: Vec<R_xlen_t> = Vec::new();
        for i in 0..n {
            let elem = VECTOR_ELT(x, i);
            if !elem.is_null() && elem != R_NilValue() {
                kept.push(i);
            }
        }

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, kept.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        for (j, &i) in kept.iter().enumerate() {
            SET_VECTOR_ELT(result, j as i64, VECTOR_ELT(x, i));
        }
        result
    }
}

/// R-like `keep(x, i)` — keep elements at 1-based indices from a list/vector.
pub unsafe fn do_keep(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let i_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || i_arg.is_null() || i_arg == R_NilValue() {
            return x;
        }

        let t = TYPEOF(x);
        let n_i = XLENGTH(i_arg);
        let result = Rf_allocVector3(t, n_i);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        if t == SEXPTYPE::VECSXP {
            for j in 0..n_i {
                let idx = (*INTEGER(i_arg).add(j as usize) - 1) as R_xlen_t; // 1-based to 0-based
                if idx >= 0 {
                    let elem = VECTOR_ELT(x, idx);
                    SET_VECTOR_ELT(result, j as i64, elem);
                }
            }
        } else if t == SEXPTYPE::REALSXP {
            let dst = REAL(result);
            for j in 0..n_i {
                let idx = (*INTEGER(i_arg).add(j as usize) - 1) as R_xlen_t;
                if idx >= 0 {
                    *dst.add(j as usize) = *REAL(x).add(idx as usize);
                } else {
                    *dst.add(j as usize) = NA_REAL;
                }
            }
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let dst = INTEGER(result);
            for j in 0..n_i {
                let idx = (*INTEGER(i_arg).add(j as usize) - 1) as R_xlen_t;
                if idx >= 0 {
                    *dst.add(j as usize) = *INTEGER(x).add(idx as usize);
                } else {
                    *dst.add(j as usize) = NA_INTEGER;
                }
            }
        }
        result
    }
}

/// R-like `discard(x, i)` — discard elements at 1-based indices from a list/vector.
pub unsafe fn do_discard(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let i_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || i_arg.is_null() || i_arg == R_NilValue() {
            return x;
        }

        let n = XLENGTH(x);
        let n_i = XLENGTH(i_arg);

        // Collect which indices to discard (0-based)
        let mut discard_set: std::collections::HashSet<R_xlen_t> = std::collections::HashSet::new();
        for j in 0..n_i {
            let idx = (*INTEGER(i_arg).add(j as usize) - 1) as R_xlen_t;
            if idx >= 0 && idx < n {
                discard_set.insert(idx);
            }
        }

        let t = TYPEOF(x);
        let new_len = n - discard_set.len() as R_xlen_t;
        let result = Rf_allocVector3(t, new_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let mut out_idx: R_xlen_t = 0;
        if t == SEXPTYPE::VECSXP {
            for i in 0..n {
                if !discard_set.contains(&i) {
                    SET_VECTOR_ELT(result, out_idx as i64, VECTOR_ELT(x, i));
                    out_idx += 1;
                }
            }
        } else if t == SEXPTYPE::REALSXP {
            let dst = REAL(result);
            for i in 0..n {
                if !discard_set.contains(&i) {
                    *dst.add(out_idx as usize) = *REAL(x).add(i as usize);
                    out_idx += 1;
                }
            }
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let dst = INTEGER(result);
            for i in 0..n {
                if !discard_set.contains(&i) {
                    *dst.add(out_idx as usize) = *INTEGER(x).add(i as usize);
                    out_idx += 1;
                }
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Complete list/data.frame — checking
// ---------------------------------------------------------------------------

/// R's `is.data.frame(x)` — check if x has "data.frame" class.
pub unsafe fn do_is_data_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let class = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"class".as_ptr()));
        if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP && XLENGTH(class) > 0 {
            let cls = elt_to_string(class, 0);
            return Rf_ScalarLogical(if cls == "data.frame" { TRUE } else { FALSE });
        }
        Rf_ScalarLogical(FALSE)
    }
}

// ---------------------------------------------------------------------------
// Complete list operations — modifyList, splice, flatten, split, melt, cast
// ---------------------------------------------------------------------------

/// R's `modifyList(old, new)` — merge new into old (simplified: shallow merge).
pub unsafe fn do_modify_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let old = CAR(args);
        let new_list = CAR(CDR(args));
        if old.is_null() || old == R_NilValue() {
            return new_list;
        }
        if new_list.is_null() || new_list == R_NilValue() {
            return old;
        }
        // Simplified: if both are VECSXP, return new_list (shallow overlay)
        let t_old = TYPEOF(old);
        let t_new = TYPEOF(new_list);
        if t_old == SEXPTYPE::VECSXP && t_new == SEXPTYPE::VECSXP {
            // Return a copy of old with elements from new overlaid
            let n_old = XLENGTH(old);
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, n_old);
            if result.is_null() {
                return new_list;
            }
            let _p = protect(result);
            for i in 0..n_old {
                let elem = VECTOR_ELT(old, i);
                crate::sexp::accessors::SET_VECTOR_ELT(result, i, elem);
            }
            // Overlay elements from new (simplified: by index)
            let n_new = XLENGTH(new_list);
            for i in 0..n_new.min(n_old) {
                let elem = VECTOR_ELT(new_list, i);
                crate::sexp::accessors::SET_VECTOR_ELT(result, i, elem);
            }
            return result;
        }
        new_list
    }
}

/// R's `splice(x, i, value)` — splice value into list at position i (simplified).
pub unsafe fn do_splice(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let i_arg = CAR(CDR(args));
        let value = CAR(CDR(CDR(args)));
        if x.is_null() || x == R_NilValue() {
            return x;
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::VECSXP {
            return x;
        }
        let n = XLENGTH(x);
        let pos = real_or_default(i_arg, 1.0) as i64;
        // Insert value at position pos (1-indexed)
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n + 1);
        if result.is_null() {
            return x;
        }
        let _p = protect(result);
        let pos = ((pos - 1).max(0).min(n as i64)) as usize;
        for i in 0..pos {
            crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, VECTOR_ELT(x, i as i64));
        }
        crate::sexp::accessors::SET_VECTOR_ELT(result, pos as i64, value);
        for i in pos..(n as usize) {
            crate::sexp::accessors::SET_VECTOR_ELT(result, (i + 1) as i64, VECTOR_ELT(x, i as i64));
        }
        result
    }
}

/// R's `flatten(x)` — flatten a nested list (simplified: one level deep).
pub unsafe fn do_flatten(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return x;
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::VECSXP {
            return x;
        }
        // Count total elements after flattening
        let n = XLENGTH(x);
        let mut total: R_xlen_t = 0;
        for i in 0..n {
            let elem = VECTOR_ELT(x, i);
            if !elem.is_null() && TYPEOF(elem) == SEXPTYPE::VECSXP {
                let sub_n = XLENGTH(elem);
                total += sub_n;
            } else {
                total += 1;
            }
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, total);
        if result.is_null() {
            return x;
        }
        let _p = protect(result);
        let mut idx: R_xlen_t = 0;
        for i in 0..n {
            let elem = VECTOR_ELT(x, i);
            if !elem.is_null() && TYPEOF(elem) == SEXPTYPE::VECSXP {
                let sub_n = XLENGTH(elem);
                for j in 0..sub_n {
                    crate::sexp::accessors::SET_VECTOR_ELT(result, idx, VECTOR_ELT(elem, j));
                    idx += 1;
                }
            } else {
                crate::sexp::accessors::SET_VECTOR_ELT(result, idx, elem);
                idx += 1;
            }
        }
        result
    }
}

/// R's `split(x, f)` — split vector `x` into groups defined by `f`.
pub unsafe fn do_split(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let f = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || f.is_null() || f == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let nf = XLENGTH(f);
        if nf == 0 && n > 0 {
            base_error("group length is 0 but data length > 0");
        }

        let factor_levels = split_factor_levels(f);
        let mut labels = factor_levels.clone().unwrap_or_default();
        let mut groups: Vec<Vec<R_xlen_t>> = vec![Vec::new(); labels.len()];
        let mut label_index: BTreeMap<String, usize> = labels
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, label)| (label, index))
            .collect();

        for i in 0..n {
            let f_index = i % nf;
            let Some(label) = split_group_label(f, f_index) else {
                continue;
            };
            let group_index = if let Some(index) = label_index.get(&label).copied() {
                index
            } else {
                let index = labels.len();
                label_index.insert(label.clone(), index);
                labels.push(label);
                groups.push(Vec::new());
                index
            };
            groups[group_index].push(i);
        }

        if factor_levels.is_none() {
            let mut ordered: Vec<(String, Vec<R_xlen_t>)> = labels
                .iter()
                .filter_map(|label| {
                    label_index
                        .get(label)
                        .map(|&index| (label.clone(), groups[index].clone()))
                })
                .collect();
            ordered.sort_by(|left, right| split_label_cmp(TYPEOF(f), &left.0, &right.0));
            labels = ordered.iter().map(|(label, _)| label.clone()).collect();
            groups = ordered.into_iter().map(|(_, group)| group).collect();
        }

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, labels.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let result_names = Rf_allocVector3(SEXPTYPE::STRSXP, labels.len() as R_xlen_t);
        let _names_guard = protect(result_names);
        let x_names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        let have_x_names = !x_names.is_null()
            && x_names != R_NilValue()
            && TYPEOF(x_names) == SEXPTYPE::STRSXP
            && XLENGTH(x_names) >= n;

        for (group_index, (label, indices)) in labels.iter().zip(groups.iter()).enumerate() {
            let sub = Rf_allocVector3(TYPEOF(x), indices.len() as R_xlen_t);
            let _sub_guard = protect(sub);
            for (dst, &src) in indices.iter().enumerate() {
                copy_matrix_element(sub, dst as R_xlen_t, x, src);
            }
            if have_x_names {
                let names = Rf_allocVector3(SEXPTYPE::STRSXP, indices.len() as R_xlen_t);
                let _group_names_guard = protect(names);
                for (dst, &src) in indices.iter().enumerate() {
                    SET_STRING_ELT(names, dst as R_xlen_t, STRING_ELT(x_names, src));
                }
                crate::sexp::attrib_core::setAttrib(
                    sub,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    names,
                );
            }
            SET_VECTOR_ELT(result, group_index as R_xlen_t, sub);
            let label_c = CString::new(label.as_str()).unwrap_or_default();
            SET_STRING_ELT(
                result_names,
                group_index as R_xlen_t,
                Rf_mkChar(label_c.as_ptr()),
            );
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            result_names,
        );
        result
    }
}

unsafe fn split_factor_levels(f: SEXP) -> Option<Vec<String>> {
    unsafe {
        let levels =
            crate::sexp::attrib_core::getAttrib(f, crate::sexp::attrib_core::R_LevelsSymbol());
        if levels.is_null() || levels == R_NilValue() || TYPEOF(levels) != SEXPTYPE::STRSXP {
            return None;
        }
        let mut out = Vec::with_capacity(XLENGTH(levels) as usize);
        for i in 0..XLENGTH(levels) {
            out.push(elt_to_string(levels, i));
        }
        Some(out)
    }
}

unsafe fn split_group_label(f: SEXP, index: R_xlen_t) -> Option<String> {
    unsafe {
        if let Some(levels) = split_factor_levels(f) {
            if TYPEOF(f) != SEXPTYPE::INTSXP {
                return None;
            }
            let raw = *INTEGER(f).add(index as usize);
            if raw == NA_INTEGER || raw < 1 || raw as usize > levels.len() {
                return None;
            }
            return Some(levels[(raw - 1) as usize].clone());
        }

        match TYPEOF(f) {
            t if t == SEXPTYPE::INTSXP => {
                let value = *INTEGER(f).add(index as usize);
                (value != NA_INTEGER).then(|| value.to_string())
            }
            t if t == SEXPTYPE::LGLSXP => {
                let value = *LOGICAL(f).add(index as usize);
                match value {
                    TRUE => Some("TRUE".to_string()),
                    FALSE => Some("FALSE".to_string()),
                    _ => None,
                }
            }
            t if t == SEXPTYPE::REALSXP => {
                let value = *REAL(f).add(index as usize);
                if value.to_bits() == R_NA_BIT_PATTERN || value.is_nan() {
                    None
                } else {
                    Some(format!("{value}"))
                }
            }
            t if t == SEXPTYPE::STRSXP => {
                let value = STRING_ELT(f, index);
                if value.is_null() || value == crate::sexp::globals::R_NaString() {
                    None
                } else {
                    Some(elt_to_string(f, index))
                }
            }
            _ => Some(elt_to_string(f, index)),
        }
    }
}

fn split_label_cmp(t: c_int, left: &str, right: &str) -> std::cmp::Ordering {
    if t == SEXPTYPE::LGLSXP {
        return split_logical_rank(left).cmp(&split_logical_rank(right));
    }
    if t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP {
        let left_num = left.parse::<f64>().ok();
        let right_num = right.parse::<f64>().ok();
        if let (Some(left_num), Some(right_num)) = (left_num, right_num)
            && let Some(ordering) = left_num.partial_cmp(&right_num)
        {
            return ordering;
        }
    }
    left.cmp(right)
}

fn split_logical_rank(value: &str) -> u8 {
    match value {
        "FALSE" => 0,
        "TRUE" => 1,
        _ => 2,
    }
}

/// R's `melt(x)` — melt a data.frame to long format (simplified).
pub unsafe fn do_melt(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Simplified: return the input as-is
        // A full implementation would reshape the data.frame
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        x
    }
}

/// R's `cast(x, formula)` — cast melted data (simplified).
pub unsafe fn do_cast(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Simplified: return the input as-is
        // A full implementation would reshape using the formula
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        x
    }
}
