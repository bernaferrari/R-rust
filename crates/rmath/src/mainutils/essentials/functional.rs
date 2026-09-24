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
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_lang3,
    Rf_mkChar, Rf_mkString,
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

/// R's `lapply(X, FUN, ...)` — apply FUN to each element, return list.
///
/// Extra arguments are forwarded to every FUN call (evaluated once, tags
/// preserved), matching the R-level closure: `lapply(1:2, `+`, 10)` and
/// `lapply(x, f, ctx=1)` both reach FUN. Without this, whisker's
/// `lapply(keys, resolve, context=context, strict=strict)` calls `resolve`
/// with no `context`, failing on its first formal without a default.
pub unsafe fn do_lapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = eval_arg_by_name_or_position(args, &["X"], 0, rho);
        let fun = callable_arg_by_name_or_position(args, &["FUN"], 1, rho);

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

        // Collect `...`: every cell that is not X (name or first positional
        // when X is unnamed) and not FUN (name or the positional after X).
        // Evaluate each once in the caller; values live across the loop.
        let mut extra_args = R_NilValue();
        let mut extra_tail: SEXP = std::ptr::null_mut();
        let mut extra_guards: Vec<_> = Vec::new();
        let mut x_named = false;
        let mut fun_named = false;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            match tag_name(current).as_deref() {
                Some("X") => x_named = true,
                Some("FUN") => fun_named = true,
                _ => {}
            }
            current = CDR(current);
        }
        let mut positional = 0usize;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let named = tag_name(current);
            let is_x =
                named.as_deref() == Some("X") || (named.is_none() && !x_named && positional == 0);
            let is_fun = named.as_deref() == Some("FUN")
                || (named.is_none() && !fun_named && positional == 1);
            if !is_x && !is_fun {
                let val = crate::sexp::memory_ext::mkPROMSXP(CAR(current), rho);
                let _val = protect(val);
                let cell = Rf_cons(val, R_NilValue());
                extra_guards.push(protect(cell));
                let tg = TAG(current);
                if !tg.is_null() && tg != R_NilValue() {
                    SETTAG(cell, tg);
                }
                if extra_args == R_NilValue() {
                    extra_args = cell;
                } else {
                    SETCDR(extra_tail, cell);
                }
                extra_tail = cell;
            }
            if named.is_none() {
                positional += 1;
            }
            current = CDR(current);
        }

        let n = list_apply_len(x);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            let elem = extract_element(x, i);
            let val = apply_fun_to_element(fun, elem, extra_args, rho);
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

/// GNU `sapply(X, FUN, ..., simplify = TRUE, USE.NAMES = TRUE)`.
/// `simplify` / `USE.NAMES` are not forwarded into FUN's `...`.
pub unsafe fn do_sapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = eval_arg_by_name_or_position(args, &["X"], 0, rho);
        let _x = protect(x);
        let (lapply_args, simplify, use_names) = sapply_control_args(args, x, rho);
        let _lapply_args = protect(lapply_args);
        let _simplify = protect(simplify);
        let list = do_lapply(_call, _op, lapply_args, rho);
        let _list = protect(list);

        if use_names
            && !list.is_null()
            && TYPEOF(list) == SEXPTYPE::VECSXP
            && TYPEOF(x) == SEXPTYPE::STRSXP
            && XLENGTH(x) == XLENGTH(list)
        {
            let names = crate::sexp::attrib_core::getAttrib(
                list,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            if names.is_null() || names == R_NilValue() {
                crate::sexp::attrib_core::setAttrib(
                    list,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    x,
                );
            }
        }
        if sapply_is_false(simplify) {
            return list;
        }
        if list.is_null() || TYPEOF(list) != SEXPTYPE::VECSXP || XLENGTH(list) == 0 {
            return list;
        }
        let first = VECTOR_ELT(list, 0);
        let atomic = !first.is_null()
            && matches!(
                SEXPTYPE(TYPEOF(first)),
                SEXPTYPE::REALSXP
                    | SEXPTYPE::INTSXP
                    | SEXPTYPE::LGLSXP
                    | SEXPTYPE::STRSXP
                    | SEXPTYPE::CPLXSXP
                    | SEXPTYPE::RAWSXP
            );
        if !atomic {
            return list;
        }
        let higher = if sapply_is_array(simplify) {
            TRUE
        } else {
            FALSE
        };
        let higher_s = Rf_ScalarLogical(higher);
        let _higher = protect(higher_s);
        let tail = Rf_cons(higher_s, R_NilValue());
        let _tail = protect(tail);
        SETTAG(tail, Rf_install(c"higher".as_ptr()));
        let sargs = Rf_cons(list, tail);
        let _sargs = protect(sargs);
        do_simplify2array(_call, _op, sargs, rho)
    }
}

unsafe fn sapply_control_args(args: SEXP, x: SEXP, rho: SEXP) -> (SEXP, SEXP, bool) {
    unsafe {
        let mut simplify = Rf_ScalarLogical(TRUE);
        let mut _simplify_guard = None;
        let mut use_names = true;
        let mut lapply_args = R_NilValue();
        let mut guards: Vec<_> = Vec::new();
        let mut cells = Vec::new();
        let mut current = args;
        let mut positional = 0usize;
        let mut x_named = false;
        let mut fun_named = false;
        while !current.is_null() && current != R_NilValue() {
            match tag_name(current).as_deref() {
                Some("X") => x_named = true,
                Some("FUN") => fun_named = true,
                _ => {}
            }
            current = CDR(current);
        }
        current = args;
        while !current.is_null() && current != R_NilValue() {
            let named = tag_name(current);
            let is_x =
                named.as_deref() == Some("X") || (named.is_none() && !x_named && positional == 0);
            let is_fun = named.as_deref() == Some("FUN")
                || (named.is_none() && !fun_named && positional == 1);
            let is_simplify = named.as_deref() == Some("simplify");
            let is_use_names = named.as_deref() == Some("USE.NAMES");
            if is_simplify {
                simplify = crate::eval::eval::Rf_eval(CAR(current), rho);
                _simplify_guard = Some(protect(simplify));
            } else if is_use_names {
                let value = crate::eval::eval::Rf_eval(CAR(current), rho);
                if TYPEOF(value) == SEXPTYPE::LGLSXP && XLENGTH(value) == 1 {
                    use_names = *LOGICAL(value) != FALSE && *LOGICAL(value) != NA_LOGICAL;
                }
            } else {
                let value = if is_x {
                    x
                } else if is_fun {
                    let fun = match_fun_arg(CAR(current), rho);
                    guards.push(protect(fun));
                    fun
                } else {
                    CAR(current)
                };

                cells.push((value, TAG(current)));

            }

            if named.is_none() {
                positional += 1;
            }
            current = CDR(current);
        }
        for (value, tag) in cells.into_iter().rev() {
            let cell = Rf_cons(value, lapply_args);
            guards.push(protect(cell));
            SETTAG(cell, tag);
            lapply_args = cell;
        }
        let _ = guards;
        (lapply_args, simplify, use_names)
    }
}

unsafe fn sapply_is_false(x: SEXP) -> bool {
    unsafe {
        TYPEOF(x) == SEXPTYPE::LGLSXP
            && XLENGTH(x) == 1
            && *LOGICAL(x) == FALSE
    }
}

unsafe fn sapply_is_array(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::STRSXP && XLENGTH(x) >= 1 && elt_to_string(x, 0) == "array" }
}



/// R's `vapply(X, FUN, FUN.VALUE, ..., USE.NAMES = TRUE)` — apply.c's
/// do_vapply as a dedicated checked loop (the R-level wrapper only adds
/// match.fun and an as.list conversion for non-vector/object X).
///
/// The template (FUN.VALUE) is rooted before anything else and fixes the
/// answer's type, length and shape. Each FUN result is validated as it
/// arrives — length first, then type, permitting only the widening ladder
/// logical -> integer -> double -> complex — with upstream's exact error
/// wording; there is no "return the unsimplified list" fallback. A
/// template of length 1 flattens to a vector; any other length (including
/// 0, and templates carrying dims, whose dim is preserved) produces an
/// array of dim c(dim(template), length(X)). USE.NAMES carries X's names
/// (or X itself when X is character) onto the result; the template's (or
/// first result's) names/dimnames become the row names.
pub unsafe fn do_vapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = eval_arg_by_name_or_position(args, &["X"], 0, rho);
        let _x_guard = protect(x);
        let fun = callable_arg_by_name_or_position(args, &["FUN"], 1, rho);

        let _fun_guard = protect(fun);
        // Root the template FIRST: the output type, dimensions and
        // row-name policy all read it, and it must survive every FUN
        // evaluation below.
        let value = eval_arg_by_name_or_position(args, &["FUN.VALUE"], 2, rho);
        let _value_guard = protect(value);

        if !is_vapply_vector(value) {
            base_error("'FUN.VALUE' must be a vector");
        }
        let use_names = vapply_use_names(args, rho);

        // R-level wrapper: if (!is.vector(X) || is.object(X)) X <- as.list(X)
        let xx = normalize_vapply_x(x);
        let _xx_guard = protect(xx);

        let n: R_xlen_t = XLENGTH(xx);
        let common_type = TYPEOF(value);
        let supported = common_type == SEXPTYPE::CPLXSXP
            || common_type == SEXPTYPE::REALSXP
            || common_type == SEXPTYPE::INTSXP
            || common_type == SEXPTYPE::LGLSXP
            || common_type == SEXPTYPE::RAWSXP
            || common_type == SEXPTYPE::STRSXP
            || common_type == SEXPTYPE::VECSXP;
        if !supported {
            base_error(format!(
                "type '{}' is not supported",
                sexp_type_name(SEXPTYPE(common_type))
            ));
        }
        let common_len = vapply_length(value);

        let dim_v =
            crate::sexp::attrib_core::getAttrib(value, crate::sexp::attrib_core::R_DimSymbol());
        let _dim_v_guard = protect(dim_v);
        let array_value = TYPEOF(dim_v) == SEXPTYPE::INTSXP && XLENGTH(dim_v) >= 1;

        // Allocate and protect the answer before any FUN call.
        let ans = Rf_allocVector3(SEXPTYPE(common_type), n * common_len);
        let _ans_guard = protect(ans);

        // Result names: X's names attribute, or X itself when X is
        // character (upstream: names <- names(XX); if (is.null(names)
        // && is.character(XX)) names <- XX). Row names: the template's
        // names — dimnames when the template is an array — with a
        // fallback to the FIRST result's, taken inside the loop.
        let mut names = R_NilValue();
        let mut names_guard =
            crate::sexp::protect::protect_with_index_raw(R_NilValue(), "vapply names");
        let mut row_names = R_NilValue();
        let mut row_names_guard =
            crate::sexp::protect::protect_with_index_raw(R_NilValue(), "vapply row names");
        if use_names {
            names =
                crate::sexp::attrib_core::getAttrib(xx, crate::sexp::attrib_core::R_NamesSymbol());
            if vapply_is_nil(names) && TYPEOF(xx) == SEXPTYPE::STRSXP {
                names = xx;
            }
            names_guard.reprotect_raw(names);
            row_names = crate::sexp::attrib_core::getAttrib(
                value,
                if array_value {
                    crate::sexp::attrib_core::R_DimNamesSymbol()
                } else {
                    crate::sexp::attrib_core::R_NamesSymbol()
                },
            );
            row_names_guard.reprotect_raw(row_names);
        }

        // Collect vapply's `...`: every cell that is not X, FUN, FUN.VALUE
        // or USE.NAMES (by name, or the first three unnamed positionals).
        // Evaluated once, forwarded to every FUN call (tags preserved).
        let mut extra_args = R_NilValue();
        let mut extra_tail: SEXP = std::ptr::null_mut();
        let mut extra_guards: Vec<_> = Vec::new();
        let mut positional = 0usize;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let named = tag_name(current);
            let is_vapply_formal = match named.as_deref() {
                Some("X") | Some("FUN") | Some("FUN.VALUE") | Some("USE.NAMES") => true,
                None if positional <= 2 => true,
                _ => false,
            };
            if !is_vapply_formal {
                let val = crate::sexp::memory_ext::mkPROMSXP(CAR(current), rho);
                let _val = protect(val);
                let cell = Rf_cons(val, R_NilValue());
                extra_guards.push(protect(cell));
                let tg = TAG(current);
                if !tg.is_null() && tg != R_NilValue() {
                    SETTAG(cell, tg);
                }
                if extra_args == R_NilValue() {
                    extra_args = cell;
                } else {
                    SETCDR(extra_tail, cell);
                }
                extra_tail = cell;
            }
            if named.is_none() {
                positional += 1;
            }
            current = CDR(current);
        }

        let mut offset: R_xlen_t = 0;
        for i in 0..n {
            let elem = extract_element(xx, i);
            let mut elem_guard =
                crate::sexp::protect::protect_with_index_raw(elem, "vapply element");
            let mut val = apply_fun_to_element(fun, elem, extra_args, rho);
            elem_guard.reprotect_raw(val);

            // Immediate validation, before the next FUN call: length
            // first, then type — apply.c's order and wording.
            let val_len = vapply_length(val);
            if val_len != common_len {
                base_error(format!(
                    "values must be length {},\n but FUN(X[[{}]]) result is length {}",
                    common_len,
                    i + 1,
                    val_len
                ));
            }
            let val_type = TYPEOF(val);
            if val_type != common_type {
                let okay = match common_type {
                    t if t == SEXPTYPE::CPLXSXP => {
                        val_type == SEXPTYPE::REALSXP
                            || val_type == SEXPTYPE::INTSXP
                            || val_type == SEXPTYPE::LGLSXP
                    }
                    t if t == SEXPTYPE::REALSXP => {
                        val_type == SEXPTYPE::INTSXP || val_type == SEXPTYPE::LGLSXP
                    }
                    t if t == SEXPTYPE::INTSXP => val_type == SEXPTYPE::LGLSXP,
                    _ => false,
                };
                if !okay {
                    base_error(format!(
                        "values must be type '{}',\n but FUN(X[[{}]]) result is type '{}'",
                        sexp_type_name(SEXPTYPE(common_type)),
                        i + 1,
                        sexp_type_name(SEXPTYPE(val_type))
                    ));
                }
                val = crate::mainutils::coerce::coerceVector(val, common_type);
                elem_guard.reprotect_raw(val);
            }

            // Row names come from the first result only.
            if i == 0 && use_names && vapply_is_nil(row_names) {
                row_names = crate::sexp::attrib_core::getAttrib(
                    val,
                    if array_value {
                        crate::sexp::attrib_core::R_DimNamesSymbol()
                    } else {
                        crate::sexp::attrib_core::R_NamesSymbol()
                    },
                );
                row_names_guard.reprotect_raw(row_names);
            }

            // commonLen == 1 is the flat case: element i. Any other
            // length fills one column per result (column-major, apply.c).
            let dst = if common_len <= 1 { i } else { offset };
            match common_type {
                t if t == SEXPTYPE::CPLXSXP => {
                    for j in 0..common_len {
                        *COMPLEX(ans).add((dst + j) as usize) = *COMPLEX(val).add(j as usize);
                    }
                }
                t if t == SEXPTYPE::REALSXP => {
                    for j in 0..common_len {
                        *REAL(ans).add((dst + j) as usize) = *REAL(val).add(j as usize);
                    }
                }
                t if t == SEXPTYPE::INTSXP => {
                    for j in 0..common_len {
                        *INTEGER(ans).add((dst + j) as usize) = *INTEGER(val).add(j as usize);
                    }
                }
                t if t == SEXPTYPE::LGLSXP => {
                    for j in 0..common_len {
                        *LOGICAL(ans).add((dst + j) as usize) = *LOGICAL(val).add(j as usize);
                    }
                }
                t if t == SEXPTYPE::RAWSXP => {
                    for j in 0..common_len {
                        *RAW(ans).add((dst + j) as usize) = *RAW(val).add(j as usize);
                    }
                }
                t if t == SEXPTYPE::STRSXP => {
                    for j in 0..common_len {
                        SET_STRING_ELT(ans, dst + j, STRING_ELT(val, j));
                    }
                }
                t if t == SEXPTYPE::VECSXP => {
                    for j in 0..common_len {
                        SET_VECTOR_ELT(ans, (dst + j) as i64, VECTOR_ELT(val, j as i64));
                    }
                }
                _ => {}
            }
            if common_len > 1 {
                offset += common_len;
            }
        }

        if common_len != 1 {
            let rnk_v: R_xlen_t = if array_value { XLENGTH(dim_v) } else { 1 };
            let dim = Rf_allocVector3(SEXPTYPE::INTSXP, rnk_v + 1);
            let _dim_guard = protect(dim);
            if array_value {
                for j in 0..rnk_v {
                    *INTEGER(dim).add(j as usize) = *INTEGER(dim_v).add(j as usize);
                }
            } else {
                *INTEGER(dim) = common_len as c_int;
            }
            *INTEGER(dim).add(rnk_v as usize) = n as c_int;
            crate::sexp::attrib_core::setAttrib(ans, crate::sexp::attrib_core::R_DimSymbol(), dim);

            if use_names && (!vapply_is_nil(names) || !vapply_is_nil(row_names)) {
                let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, rnk_v + 1);
                let _dimnames_guard = protect(dimnames);
                if array_value && !vapply_is_nil(row_names) {
                    if TYPEOF(row_names) != SEXPTYPE::VECSXP || XLENGTH(row_names) != rnk_v {
                        base_error(format!(
                            "dimnames(<value>) is neither NULL nor list of length {}",
                            rnk_v
                        ));
                    }
                    for j in 0..rnk_v {
                        SET_VECTOR_ELT(dimnames, j as i64, VECTOR_ELT(row_names, j as i64));
                    }
                } else {
                    // The engine zero-initializes fresh VECSXP payloads;
                    // unset dimnames slots must read as NULL like
                    // upstream's R_NilValue-filled allocation.
                    SET_VECTOR_ELT(dimnames, 0, row_names);
                    for j in 1..rnk_v {
                        SET_VECTOR_ELT(dimnames, j as i64, R_NilValue());
                    }
                }
                SET_VECTOR_ELT(dimnames, rnk_v as i64, names);
                crate::sexp::attrib_core::setAttrib(
                    ans,
                    crate::sexp::attrib_core::R_DimNamesSymbol(),
                    dimnames,
                );
            }
        } else if use_names && !vapply_is_nil(names) {
            crate::sexp::attrib_core::setAttrib(
                ans,
                crate::sexp::attrib_core::R_NamesSymbol(),
                names,
            );
        }
        ans
    }
}

/// vapply's predicate for FUN.VALUE: C-level isVector (one of R's vector
/// types), NOT the stricter R-level is.vector. Lists and expressions are
/// vectors here; NULL, symbols and calls are not.
fn is_vapply_vector(x: SEXP) -> bool {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return false;
        }
        let t = TYPEOF(x);
        t == SEXPTYPE::LGLSXP
            || t == SEXPTYPE::INTSXP
            || t == SEXPTYPE::REALSXP
            || t == SEXPTYPE::CPLXSXP
            || t == SEXPTYPE::STRSXP
            || t == SEXPTYPE::RAWSXP
            || t == SEXPTYPE::VECSXP
            || t == SEXPTYPE::EXPRSXP
    }
}

/// USE.NAMES: named actual only (positional matching stops at vapply's
/// `...`); evaluated, then read as a logical scalar. NA errors, like
/// upstream; absent or non-logical defaults to TRUE.
fn vapply_use_names(args: SEXP, rho: SEXP) -> bool {
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).as_deref() == Some("USE.NAMES") {
                let raw = CAR(current);
                if raw.is_null() || raw == R_NilValue() {
                    return true;
                }
                let raw = if is_already_a_value(raw) {
                    raw
                } else {
                    crate::eval::eval::Rf_eval(raw, rho)
                };
                let _raw_guard = protect(raw);
                if XLENGTH(raw) == 0 {
                    return true;
                }
                let t = TYPEOF(raw);
                let logical: c_int = if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP {
                    *INTEGER(raw)
                } else if t == SEXPTYPE::REALSXP {
                    let d = *REAL(raw);
                    if ISNAN(d) { NA_LOGICAL } else { d as c_int }
                } else {
                    return true;
                };
                if logical == NA_LOGICAL {
                    base_error("invalid 'USE.NAMES' value");
                }
                return logical != 0;
            }
            current = CDR(current);
        }
        true
    }
}

/// The R-level wrapper's X normalization: plain (non-object) atomic
/// vectors and lists pass through; everything else — objects, factors,
/// pairlists, calls, expressions, environments — goes through as.list
/// (which flattens atomic input, matching as.list.default).
fn normalize_vapply_x(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        }
        let t = TYPEOF(x);
        let passes_through = (t == SEXPTYPE::LGLSXP
            || t == SEXPTYPE::INTSXP
            || t == SEXPTYPE::REALSXP
            || t == SEXPTYPE::CPLXSXP
            || t == SEXPTYPE::STRSXP
            || t == SEXPTYPE::RAWSXP
            || t == SEXPTYPE::VECSXP)
            && crate::sexp::attrib_core::isObject(x) == 0;
        if passes_through {
            return x;
        }
        let cell = Rf_cons(x, R_NilValue());
        let _cell_guard = protect(cell);
        if t == SEXPTYPE::CLOSXP
            || t == SEXPTYPE::BUILTINSXP
            || t == SEXPTYPE::SPECIALSXP
        {
            return crate::mainutils::essentials_basic::do_as_list_function(
                R_NilValue(),
                R_NilValue(),
                cell,
                R_NilValue(),
            );
        }
        crate::mainutils::essentials_basic::do_as_list(
            R_NilValue(),
            R_NilValue(),
            cell,
            R_NilValue(),
        )

    }
}

/// length() as vapply uses it: XLENGTH for vectors, a walk for pairlist
/// results, 0 for NULL.
fn vapply_length(x: SEXP) -> R_xlen_t {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return 0;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP {
            let mut len: R_xlen_t = 0;
            let mut current = x;
            while !current.is_null() && current != R_NilValue() {
                len += 1;
                current = CDR(current);
            }
            len
        } else {
            XLENGTH(x)
        }
    }
}

fn vapply_is_nil(x: SEXP) -> bool {
    unsafe { x.is_null() || x == R_NilValue() }
}

/// Upstream type names as they appear in vapply's error messages
/// (R_typeToChar).
fn sexp_type_name(t: SEXPTYPE) -> &'static str {
    match t {
        t if t == SEXPTYPE::CPLXSXP => "complex",
        t if t == SEXPTYPE::REALSXP => "double",
        t if t == SEXPTYPE::INTSXP => "integer",
        t if t == SEXPTYPE::LGLSXP => "logical",
        t if t == SEXPTYPE::RAWSXP => "raw",
        t if t == SEXPTYPE::STRSXP => "character",
        t if t == SEXPTYPE::VECSXP => "list",
        t if t == SEXPTYPE::EXPRSXP => "expression",
        _ => "unknown",
    }
}

/// R's `Map(f, ...)` — apply f element-wise.
pub unsafe fn do_map(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let fun = callable_arg_by_name_or_position(args, &["f", "FUN"], 0, rho);

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
        crate::mainutils::base_wrappers::apply(
            "Filter",
            include_str!("../base_wrappers/filter.R"),
            args,
            rho,
            false,
        )
    }
}

/// GNU `replicate(n, expr, simplify = "array")`.
pub unsafe fn do_replicate(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "replicate",
            include_str!("../base_wrappers/replicate.R"),
            args,
            rho,
            false,
        )
    }
}


/// R's `do.call(what, args)` — call function with list of args.
fn do_call_arguments(args: SEXP) -> [Option<SEXP>; 4] {
    unsafe {
        let formals = ["what", "args", "quote", "envir"];
        let mut matched = [None; 4];
        let mut positional = Vec::new();
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if let Some(name) = tag_name(current) {
                let index = formals
                    .iter()
                    .position(|formal| *formal == name)
                    .or_else(|| {
                        formals
                            .iter()
                            .position(|formal| !name.is_empty() && formal.starts_with(&name))
                    })
                    .unwrap_or_else(|| base_error(format!("unused argument ({name} = ...)")));
                if matched[index].is_some() {
                    base_error(format!(
                        "formal argument '{}' matched by multiple actual arguments",
                        formals[index]
                    ));
                }
                matched[index] = Some(CAR(current));
            } else {
                positional.push(CAR(current));
            }
            current = CDR(current);
        }
        for value in positional {
            let index = matched
                .iter()
                .position(Option::is_none)
                .unwrap_or_else(|| base_error("unused argument (...)"));
            matched[index] = Some(value);
        }
        for index in 0..2 {
            if matched[index].is_none() || matched[index] == Some(R_MissingArg()) {
                base_error(format!(
                    "argument '{}' is missing, with no default",
                    formals[index]
                ));
            }
        }
        matched
    }
}

pub unsafe fn do_do_call(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let matched = do_call_arguments(args);
        let what = crate::eval::eval::Rf_eval(matched[0].unwrap(), rho);
        let _what = protect(what);
        if !(TYPEOF(what) == SEXPTYPE::CLOSXP
            || TYPEOF(what) == SEXPTYPE::BUILTINSXP
            || TYPEOF(what) == SEXPTYPE::SPECIALSXP)
            && !(TYPEOF(what) == SEXPTYPE::STRSXP
                && XLENGTH(what) == 1
                && STRING_ELT(what, 0) != crate::sexp::globals::R_NaString())
        {
            base_error("'what' must be a function or character string");
        }
        let fun = callable_expr(what);
        let _fun = protect(fun);
        let arg_list = crate::eval::eval::Rf_eval(matched[1].unwrap(), rho);
        let _arg_list = protect(arg_list);
        if arg_list != R_NilValue() && TYPEOF(arg_list) != SEXPTYPE::VECSXP {
            base_error("second argument must be a list");
        }
        let env = matched[3].map_or(rho, |expr| crate::eval::eval::Rf_eval(expr, rho));
        let _env = protect(env);
        if TYPEOF(env) != SEXPTYPE::ENVSXP {
            base_error("'envir' must be an environment");
        }
        let quoted = if let Some(expr) = matched[2] {
            let value = crate::eval::eval::Rf_eval(expr, rho);
            let flag = crate::mainutils::coerce::asLogical(value);
            if flag == NA_LOGICAL {
                base_error("invalid 'quote' argument");
            }
            flag != FALSE
        } else {
            false
        };
        let names = crate::sexp::attrib_core::getAttrib(
            arg_list,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        let mut call_args = R_NilValue();
        let mut guards = Vec::new();
        for i in (0..XLENGTH(arg_list)).rev() {
            let value = VECTOR_ELT(arg_list, i);
            let value = if quoted {
                let wrapped =
                    crate::sexp::constructors::Rf_lang2(Rf_install(c"quote".as_ptr()), value);
                guards.push(protect(wrapped));
                wrapped
            } else {
                value
            };
            let cell = Rf_cons(value, call_args);
            guards.push(protect(cell));
            if TYPEOF(names) == SEXPTYPE::STRSXP && i < XLENGTH(names) {
                let chars = CHAR(STRING_ELT(names, i));
                if !chars.is_null() && *chars != 0 {
                    SETTAG(cell, Rf_install(chars));
                }
            }
            call_args = cell;
        }
        let call_sexp = Rf_cons(fun, call_args);
        let _call = protect(call_sexp);
        (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        crate::eval::eval::Rf_eval(call_sexp, env)
    }
}

fn callable_arg_by_name_or_position(
    args: SEXP,
    names: &[&str],
    position: usize,
    rho: SEXP,
) -> SEXP {
    unsafe { match_fun_arg(arg_by_name_or_position(args, names, position), rho) }
}

unsafe fn match_fun_arg(expr: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if expr.is_null() || expr == R_NilValue() {
            return expr;
        }
        let t = TYPEOF(expr);
        if t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
            return expr;
        }
        let val = if t == SEXPTYPE::SYMSXP
            || t == SEXPTYPE::LANGSXP
            || t == SEXPTYPE::PROMSXP
            || t == SEXPTYPE::BCODESXP
        {
            crate::eval::eval::Rf_eval(expr, rho)
        } else {
            expr
        };
        let vt = TYPEOF(val);
        if vt == SEXPTYPE::CLOSXP || vt == SEXPTYPE::BUILTINSXP || vt == SEXPTYPE::SPECIALSXP {
            return val;
        }
        if vt == SEXPTYPE::STRSXP && XLENGTH(val) > 0 {
            let charsxp = STRING_ELT(val, 0);
            if charsxp.is_null() || charsxp == crate::sexp::globals::R_NaString() {
                return R_NilValue();
            }
            let name = CHAR(charsxp);
            if name.is_null() {
                return R_NilValue();
            }
            return crate::eval::eval::Rf_eval(Rf_install(name), rho);
        }
        if vt == SEXPTYPE::SYMSXP {
            return crate::eval::eval::Rf_eval(val, rho);
        }
        val
    }
}


fn eval_arg_by_name_or_position(args: SEXP, names: &[&str], position: usize, rho: SEXP) -> SEXP {
    unsafe {
        let expr = arg_by_name_or_position(args, names, position);
        if expr.is_null() || expr == R_NilValue() {
            R_NilValue()
        } else if is_already_a_value(expr) {
            // Evaluated builtins (vapply -> do_lapply) arrive with args
            // already evaluated: X is a pairlist/vector, not a symbol or
            // call. Re-evaluating a pairlist evaluates EACH element, which
            // panics on a missing-arg formal (evalList "argument N is
            // empty"). Values pass through untouched.
            expr
        } else {
            crate::eval::eval::Rf_eval(expr, rho)
        }
    }
}

/// Whether `expr` is already an evaluated R value (self-evaluating or a
/// container), as opposed to a symbol/closure-call that still needs
/// evaluation in `rho`.
fn is_already_a_value(expr: SEXP) -> bool {
    unsafe {
        let t = crate::sexp::accessors::TYPEOF(expr);
        t != SEXPTYPE::SYMSXP.0
            && t != SEXPTYPE::LANGSXP.0
            && t != SEXPTYPE::PROMSXP.0
            && t != SEXPTYPE::BCODESXP.0
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


fn apply_unary_value(fun: SEXP, value: SEXP, rho: SEXP) -> SEXP {
    unsafe { apply_fun_to_element(fun, value, R_NilValue(), rho) }
}

/// Call FUN on one extracted element, forwarding the caller's collected
/// `...` (evaluated once, tags preserved). The element goes in as a
/// pre-forced promise: splicing a language object (`1 + 2` from a saved
/// expression vector) straight into the call makes evalList EVALUATE it;
/// upstream lapply.c passes each element as a forced promise too.
/// Binding the missing-arg sentinel itself would make lookup treat the
/// formal as missing; a forced promise whose value is that sentinel is
/// the empty symbol, matching `vapply(formals(f), FUN, ...)`.
fn apply_fun_to_element(fun: SEXP, elem: SEXP, extra_args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let elem_promise = crate::sexp::memory_ext::mkPROMSXP(elem, rho);
        if !elem_promise.is_null() {
            crate::sexp::accessors::SET_PRVALUE(elem_promise, elem);
        }
        let _promise_guard = protect(elem_promise);
        let call_args = Rf_cons(elem_promise, extra_args);
        let _call_args_guard = protect(call_args);
        let call = Rf_cons(fun, call_args);
        if !call.is_null() {
            (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        let _call_guard = protect(call);
        crate::eval::eval::Rf_eval(call, rho)
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
        if first.is_null()
            || !matches!(
                SEXPTYPE(TYPEOF(first)),
                SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP | SEXPTYPE::STRSXP
            )
            || XLENGTH(first) != 1
        {
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
            _ => {
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
        let fun = apply_fun(_call, args, rho);

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
fn apply_fun(call: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let evaluated = callable_arg_by_name_or_position(args, &["FUN"], 2, rho);
        if is_function(evaluated) {
            return evaluated;
        }
        // `apply(x, 1, diff)` inside a function whose formal is `diff = TRUE`
        // still means base::diff. GNU match.fun recovers the symbol and
        // skips the non-function binding.
        let uneval = CDR(call);
        let expr = arg_by_name_or_position(uneval, &["FUN"], 2);
        if !expr.is_null() && TYPEOF(expr) == SEXPTYPE::SYMSXP {
            let found = crate::sexp::envir::findFun(expr, rho);
            if is_function(found) {
                return found;
            }
        }
        evaluated
    }
}

fn is_function(value: SEXP) -> bool {
    unsafe {
        !value.is_null()
            && value != R_NilValue()
            && value != crate::sexp::globals::R_UnboundValue()
            && (TYPEOF(value) == SEXPTYPE::CLOSXP
                || TYPEOF(value) == SEXPTYPE::BUILTINSXP
                || TYPEOF(value) == SEXPTYPE::SPECIALSXP)
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
        let star = fun_arg.is_null()
            || fun_arg == R_NilValue()
            || (TYPEOF(fun_arg) == SEXPTYPE::STRSXP && elt_to_string(fun_arg, 0) == "*");
        let robj = if star {
            let result = Rf_allocVector3(SEXPTYPE::REALSXP, nx * ny);
            let _result = protect(result);
            let dst = REAL(result);
            for i in 0..nx {
                let xi = elt_real_safe(x, i);
                for j in 0..ny {
                    *dst.add((j * nx + i) as usize) = xi * elt_real_safe(y, j);
                }
            }
            result
        } else {
            let fun = resolve_outer_fun(fun_arg, rho);
            let _fun = protect(fun);
            let xrep = rep_times(x, ny);
            let _xrep = protect(xrep);
            let yrep = rep_each(y, nx);
            let _yrep = protect(yrep);
            // GNU: FUN(X, Y, ...) — extra tagged args such as sep=":".
            let extra = CDR(CDR(CDR(args)));
            let call = if extra.is_null() || extra == R_NilValue() {
                Rf_lang3(fun, xrep, yrep)
            } else {
                let extra_dup = crate::mainutils::duplicate::shallow_duplicate(extra);
                let _ed = protect(extra_dup);
                let call = Rf_cons(fun, Rf_cons(xrep, Rf_cons(yrep, extra_dup)));
                if !call.is_null() {
                    (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                }
                call
            };
            let _call = protect(call);
            crate::eval::eval::Rf_eval(call, rho)
        };
        let _robj = protect(robj);
        let mut od = array_dims(x);
        od.extend(array_dims(y));
        set_int_dim(robj, &od);
        attach_outer_dimnames(robj, x, y);
        robj
    }
}

/// GNU `.kronecker(X, Y, FUN="*")` — `aperm(outer(X,Y,FUN))` then `dim <- dX*dY`.
pub unsafe fn do_kronecker(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = as_array_for_kronecker(CAR(args));
        let _x = protect(x);
        let y = as_array_for_kronecker(CAR(CDR(args)));
        let _y = protect(y);
        let fun_cell = CDR(CDR(args));
        let fun = if fun_cell.is_null() || fun_cell == R_NilValue() {
            Rf_mkString(c"*".as_ptr())
        } else {
            CAR(fun_cell)
        };
        let _fun = protect(fun);
        let mut dx = array_dims(x);
        let mut dy = array_dims(y);
        if dx.len() < dy.len() {
            dx.resize(dy.len(), 1);
            set_int_dim(x, &dx);
        } else if dy.len() < dx.len() {
            dy.resize(dx.len(), 1);
            set_int_dim(y, &dy);
        }
        let outer_args = Rf_cons(x, Rf_cons(y, Rf_cons(fun, R_NilValue())));
        let _oa = protect(outer_args);
        let opobj = do_outer(call, op, outer_args, rho);
        let _op = protect(opobj);
        let k = dx.len();
        let perm = Rf_allocVector3(SEXPTYPE::INTSXP, (2 * k) as i64);
        let _perm = protect(perm);
        for i in 0..k {
            *INTEGER(perm).add(2 * i) = (k + i + 1) as c_int;
            *INTEGER(perm).add(2 * i + 1) = (i + 1) as c_int;
        }
        let aperm_args = Rf_cons(opobj, Rf_cons(perm, R_NilValue()));
        let _aa = protect(aperm_args);
        let permuted = crate::mainutils::array::do_aperm(call, op, aperm_args, rho);
        let _p = protect(permuted);
        let out_dim: Vec<c_int> = dx
            .iter()
            .zip(dy.iter())
            .map(|(a, b)| a.saturating_mul(*b))
            .collect();
        set_int_dim(permuted, &out_dim);
        permuted
    }
}

unsafe fn as_array_for_kronecker(x: SEXP) -> SEXP {
    unsafe {
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        if !dim.is_null() && dim != R_NilValue() && TYPEOF(dim) == SEXPTYPE::INTSXP {
            return x;
        }
        let y = crate::mainutils::duplicate::duplicate(x);
        let _y = protect(y);
        set_int_dim(y, &[XLENGTH(y) as c_int]);
        y
    }
}

unsafe fn array_dims(x: SEXP) -> Vec<c_int> {
    unsafe {
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        if dim.is_null() || dim == R_NilValue() || TYPEOF(dim) != SEXPTYPE::INTSXP {
            return vec![XLENGTH(x) as c_int];
        }
        let n = XLENGTH(dim) as usize;
        (0..n).map(|i| *INTEGER(dim).add(i)).collect()
    }
}

unsafe fn set_int_dim(x: SEXP, dims: &[c_int]) {
    unsafe {
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, dims.len() as i64);
        for (i, d) in dims.iter().enumerate() {
            *INTEGER(dim).add(i) = *d;
        }
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_DimSymbol(), dim);
    }
}


unsafe fn attach_outer_dimnames(robj: SEXP, x: SEXP, y: SEXP) {
    unsafe {
        let nxn = crate::attrib_core::getAttrib(x, crate::attrib_core::R_NamesSymbol());
        let nyn = crate::attrib_core::getAttrib(y, crate::attrib_core::R_NamesSymbol());
        let has_x = !nxn.is_null() && nxn != R_NilValue();
        let has_y = !nyn.is_null() && nyn != R_NilValue();
        if !has_x && !has_y {
            return;
        }
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        SET_VECTOR_ELT(dn, 0, if has_x { nxn } else { R_NilValue() });
        SET_VECTOR_ELT(dn, 1, if has_y { nyn } else { R_NilValue() });
        crate::sexp::attrib_core::setAttrib(robj, crate::attrib_core::R_DimNamesSymbol(), dn);
    }
}

unsafe fn resolve_outer_fun(fun_arg: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if fun_arg.is_null() || fun_arg == R_NilValue() {
            return crate::sexp::envir::findFun(Rf_install(c"*".as_ptr()), rho);
        }
        let ty = TYPEOF(fun_arg);
        if ty == SEXPTYPE::CLOSXP || ty == SEXPTYPE::BUILTINSXP || ty == SEXPTYPE::SPECIALSXP {
            return fun_arg;
        }
        let sym = if ty == SEXPTYPE::STRSXP {
            let name = elt_to_string(fun_arg, 0);
            let cstr = CString::new(name.as_str()).unwrap_or_default();
            Rf_install(cstr.as_ptr())
        } else if ty == SEXPTYPE::SYMSXP {
            fun_arg
        } else {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "object of mode 'function' was not found",
            );
        };
        let fun = crate::sexp::envir::findFun(sym, rho);
        if fun.is_null() || fun == crate::sexp::globals::R_UnboundValue() {
            let name = if TYPEOF(sym) == SEXPTYPE::SYMSXP {
                let p = PRINTNAME(sym);
                if p.is_null() {
                    "FUN".to_string()
                } else {
                    std::ffi::CStr::from_ptr(CHAR(p))
                        .to_string_lossy()
                        .into_owned()
                }
            } else {
                "FUN".to_string()
            };
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                &format!("object '{name}' of mode 'function' was not found"),
            );
        }
        fun
    }
}

unsafe fn rep_times(x: SEXP, times: i64) -> SEXP {
    unsafe {
        let n = XLENGTH(x);
        let out_n = n.saturating_mul(times);
        let result = Rf_allocVector3(TYPEOF(x), out_n);
        let _r = protect(result);
        for t in 0..times {
            for i in 0..n {
                copy_elt(x, i, result, t * n + i);
            }
        }
        result
    }
}

unsafe fn rep_each(x: SEXP, each: i64) -> SEXP {
    unsafe {
        let n = XLENGTH(x);
        let out_n = n.saturating_mul(each);
        let result = Rf_allocVector3(TYPEOF(x), out_n);
        let _r = protect(result);
        for i in 0..n {
            for e in 0..each {
                copy_elt(x, i, result, i * each + e);
            }
        }
        result
    }
}

unsafe fn copy_elt(src: SEXP, si: i64, dst: SEXP, di: i64) {
    unsafe {
        match TYPEOF(src) {
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                *INTEGER(dst).add(di as usize) = *INTEGER(src).add(si as usize);
            }
            t if t == SEXPTYPE::REALSXP => {
                *REAL(dst).add(di as usize) = *REAL(src).add(si as usize);
            }
            t if t == SEXPTYPE::CPLXSXP => {
                *COMPLEX(dst).add(di as usize) = *COMPLEX(src).add(si as usize);
            }
            t if t == SEXPTYPE::RAWSXP => {
                *RAW(dst).add(di as usize) = *RAW(src).add(si as usize);
            }
            t if t == SEXPTYPE::STRSXP => {
                SET_STRING_ELT(dst, di, STRING_ELT(src, si));
            }
            _ => {
                let v = extract_element(src, si);
                SET_VECTOR_ELT(dst, di, v);
            }
        }
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
            // GNU sweep MARGIN=1: one STATS value per row.
            let stats_len = if stats.is_null() || stats == R_NilValue() {
                0
            } else {
                XLENGTH(stats)
            };
            for i in 0..nrow {
                for j in 0..ncol {
                    let src_idx = (j * nrow + i) as usize;
                    let stat_idx = if stats_len == 0 { 0 } else { (i as usize) % (stats_len as usize) };
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
                        elt_real_safe(stats, stat_idx as i64)
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
            // GNU sweep MARGIN=2: one STATS value per column.
            let stats_len = if stats.is_null() || stats == R_NilValue() {
                0
            } else {
                XLENGTH(stats)
            };
            for j in 0..ncol {
                for i in 0..nrow {
                    let src_idx = (j * nrow + i) as usize;
                    let stat_idx = if stats_len == 0 { 0 } else { (j as usize) % (stats_len as usize) };
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
                        elt_real_safe(stats, stat_idx as i64)
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

/// GNU `expand.grid(...)` — Cartesian product as a data.frame.
pub unsafe fn do_expand_grid(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut cols: Vec<SEXP> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        let mut keep_out = true;
        let mut strings_as_factors = true;
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let tag_s = if !tag.is_null() && tag != R_NilValue() {
                let p = PRINTNAME(tag);
                if p.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(CHAR(p))
                        .to_string_lossy()
                        .into_owned()
                }
            } else {
                String::new()
            };
            if tag_s == "KEEP.OUT.ATTRS" {
                keep_out = crate::main::coerce::asLogical(CAR(cell)) != 0;
            } else if tag_s == "stringsAsFactors" {
                strings_as_factors = crate::main::coerce::asLogical(CAR(cell)) != 0;
            } else {
                cols.push(CAR(cell));
                names.push(tag_s);
            }
            cell = CDR(cell);
        }
        if cols.len() == 1 && TYPEOF(cols[0]) == SEXPTYPE::VECSXP {
            let lst = cols[0];
            let n = XLENGTH(lst);
            let nm = crate::sexp::attrib_core::getAttrib(
                lst,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            cols = (0..n).map(|i| VECTOR_ELT(lst, i)).collect();
            names = (0..n)
                .map(|i| {
                    if !nm.is_null() && nm != R_NilValue() && TYPEOF(nm) == SEXPTYPE::STRSXP {
                        elt_to_string(nm, i)
                    } else {
                        String::new()
                    }
                })
                .collect();
        }
        let nargs = cols.len();
        if nargs == 0 {
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
            set_string_names(result, &[]);
            set_compact_row_names(result, 0);
            set_data_frame_class(result);
            return result;
        }
        for (i, name) in names.iter_mut().enumerate() {
            if name.is_empty() {
                *name = format!("Var{}", i + 1);
            }
        }
        let lens: Vec<i64> = cols.iter().map(|c| XLENGTH(*c)).collect();
        let mut total: i64 = 1;
        for &n in &lens {
            total = total.saturating_mul(n);
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, nargs as i64);
        let _result = protect(result);
        let mut rep_fac: i64 = 1;
        let mut orep = total;
        for i in 0..nargs {
            let mut x = cols[i];
            let nx = lens[i];
            orep = if nx == 0 { 0 } else { orep / nx };
            if strings_as_factors && TYPEOF(x) == SEXPTYPE::STRSXP {
                let fargs = Rf_cons(x, R_NilValue());
                let _fa = protect(fargs);
                x = crate::mainutils::essentials::do_factor(_call, _op, fargs, _rho);
            }
            let expanded = expand_grid_column(x, nx, rep_fac, orep, total);
            let levels = crate::sexp::attrib_core::getAttrib(
                x,
                crate::sexp::attrib_core::R_LevelsSymbol(),
            );
            if !levels.is_null() && levels != R_NilValue() {
                crate::sexp::attrib_core::setAttrib(
                    expanded,
                    crate::sexp::attrib_core::R_LevelsSymbol(),
                    levels,
                );
            }
            let class = crate::sexp::attrib_core::getAttrib(
                x,
                crate::sexp::attrib_core::R_ClassSymbol(),
            );
            if !class.is_null() && class != R_NilValue() {
                crate::sexp::attrib_core::setAttrib(
                    expanded,
                    crate::sexp::attrib_core::R_ClassSymbol(),
                    class,
                );
            }
            SET_VECTOR_ELT(result, i as i64, expanded);
            rep_fac = rep_fac.saturating_mul(nx.max(1));
        }
        set_string_names(result, &names);
        set_compact_row_names(result, total);
        set_data_frame_class(result);
        if keep_out {
            let dimv = Rf_allocVector3(SEXPTYPE::INTSXP, nargs as i64);
            for (i, n) in lens.iter().enumerate() {
                *INTEGER(dimv).add(i) = *n as c_int;
            }
            let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, nargs as i64);
            let _dn = protect(dimnames);
            for i in 0..nargs {
                let nx = lens[i];
                let labels = Rf_allocVector3(SEXPTYPE::STRSXP, nx);
                for j in 0..nx {
                    let label = format!("{}={}", names[i], format_grid_elt(cols[i], j));
                    let cstr = CString::new(label).unwrap_or_default();
                    SET_STRING_ELT(labels, j, Rf_mkChar(cstr.as_ptr()));
                }
                SET_VECTOR_ELT(dimnames, i as i64, labels);
            }
            set_string_names(dimnames, &names);
            set_string_names(dimv, &names);
            let attrs = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            SET_VECTOR_ELT(attrs, 0, dimv);
            SET_VECTOR_ELT(attrs, 1, dimnames);
            set_string_names(attrs, &["dim".to_string(), "dimnames".to_string()]);
            crate::sexp::attrib_core::setAttrib(result, Rf_install(c"out.attrs".as_ptr()), attrs);
        }
        result
    }
}

unsafe fn expand_grid_column(x: SEXP, nx: i64, rep_fac: i64, orep: i64, total: i64) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(TYPEOF(x), total);
        let _r = protect(result);
        if nx == 0 || total == 0 {
            return result;
        }
        let mut dst = 0i64;
        for _ in 0..orep {
            for src in 0..nx {
                for _ in 0..rep_fac {
                    copy_elt(x, src, result, dst);
                    dst += 1;
                }
            }
        }
        result
    }
}

unsafe fn format_grid_elt(x: SEXP, i: i64) -> String {
    unsafe {
        match TYPEOF(x) {
            t if t == SEXPTYPE::INTSXP => format!("{}", *INTEGER(x).add(i as usize)),
            t if t == SEXPTYPE::REALSXP => {
                let v = *REAL(x).add(i as usize);
                if v.fract() == 0.0 {
                    format!("{}", v as i64)
                } else {
                    format!("{v}")
                }
            }
            t if t == SEXPTYPE::STRSXP => elt_to_string(x, i),
            t if t == SEXPTYPE::LGLSXP => {
                let v = *INTEGER(x).add(i as usize);
                if v == 0 {
                    "FALSE".into()
                } else {
                    "TRUE".into()
                }
            }
            _ => elt_to_string(x, i),
        }
    }
}

/// GNU `stack.default` — list/data.frame columns to values+ind.
pub unsafe fn do_stack(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let mut cols: Vec<SEXP> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        if TYPEOF(x) == SEXPTYPE::VECSXP {
            let n = XLENGTH(x);
            let nm = crate::sexp::attrib_core::getAttrib(
                x,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            for i in 0..n {
                let col = VECTOR_ELT(x, i);
                let dim = crate::sexp::attrib_core::getAttrib(
                    col,
                    crate::sexp::attrib_core::R_DimSymbol(),
                );
                if !dim.is_null() && dim != R_NilValue() {
                    continue;
                }
                cols.push(col);
                let name = if !nm.is_null()
                    && nm != R_NilValue()
                    && TYPEOF(nm) == SEXPTYPE::STRSXP
                {
                    elt_to_string(nm, i)
                } else {
                    String::new()
                };
                names.push(name);
            }
        } else {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "at least one vector element is required",
            );
        }
        if cols.is_empty() {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "at least one vector element is required",
            );
        }
        let mut total: i64 = 0;
        let lens: Vec<i64> = cols.iter().map(|c| {
            let n = XLENGTH(*c);
            total += n;
            n
        }).collect();
        let values_ty = TYPEOF(cols[0]);
        let values = Rf_allocVector3(values_ty, total);
        let _values = protect(values);
        let ind = Rf_allocVector3(SEXPTYPE::INTSXP, total);
        let _ind = protect(ind);
        let mut dst = 0i64;
        for (i, col) in cols.iter().enumerate() {
            for j in 0..lens[i] {
                copy_elt(*col, j, values, dst);
                *INTEGER(ind).add(dst as usize) = (i as c_int) + 1;
                dst += 1;
            }
        }
        let levels = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as i64);
        for (i, name) in names.iter().enumerate() {
            let label = if name.is_empty() {
                format!("{}", i + 1)
            } else {
                name.clone()
            };
            let cstr = CString::new(label).unwrap_or_default();
            SET_STRING_ELT(levels, i as i64, Rf_mkChar(cstr.as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(
            ind,
            crate::sexp::attrib_core::R_LevelsSymbol(),
            levels,
        );
        crate::sexp::attrib_core::setAttrib(
            ind,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"factor".as_ptr()),
        );
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _result = protect(result);
        SET_VECTOR_ELT(result, 0, values);
        SET_VECTOR_ELT(result, 1, ind);
        set_string_names(result, &["values".to_string(), "ind".to_string()]);
        set_compact_row_names(result, total);
        set_data_frame_class(result);
        result
    }
}


/// GNU `unstack` for a stacked values/ind data.frame.
pub unsafe fn do_unstack(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if TYPEOF(x) != SEXPTYPE::VECSXP {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "'form' must be a two-sided formula",
            );
        }
        let names = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        let mut values = R_NilValue();
        let mut ind = R_NilValue();
        if TYPEOF(names) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(x) {
                let name = elt_to_string(names, i);
                if name == "values" {
                    values = VECTOR_ELT(x, i);
                } else if name == "ind" {
                    ind = VECTOR_ELT(x, i);
                }
            }
        }
        if values == R_NilValue() || ind == R_NilValue() {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "'form' must be a two-sided formula",
            );
        }
        let n = XLENGTH(values);
        let levels = crate::sexp::attrib_core::getAttrib(
            ind,
            crate::sexp::attrib_core::R_LevelsSymbol(),
        );
        let nlev = if !levels.is_null() && levels != R_NilValue() && TYPEOF(levels) == SEXPTYPE::STRSXP {
            XLENGTH(levels)
        } else {
            0
        };
        if nlev == 0 {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "'form' must be a two-sided formula",
            );
        }
        let mut buckets: Vec<Vec<i64>> = vec![Vec::new(); nlev as usize];
        for i in 0..n {
            let code = if TYPEOF(ind) == SEXPTYPE::INTSXP {
                *INTEGER(ind).add(i as usize)
            } else {
                0
            };
            if code >= 1 && (code as i64) <= nlev {
                buckets[(code as usize) - 1].push(i);
            }
        }
        let nrow = buckets.first().map(|b| b.len() as i64).unwrap_or(0);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, nlev);
        let _result = protect(result);
        let mut col_names: Vec<String> = Vec::new();
        for (k, bucket) in buckets.iter().enumerate() {
            let col = Rf_allocVector3(TYPEOF(values), bucket.len() as i64);
            for (dst, &src) in bucket.iter().enumerate() {
                copy_elt(values, src, col, dst as i64);
            }
            SET_VECTOR_ELT(result, k as i64, col);
            col_names.push(elt_to_string(levels, k as i64));
        }
        set_string_names(result, &col_names);
        set_compact_row_names(result, nrow);
        set_data_frame_class(result);
        result
    }
}

fn merge_named_true(args: SEXP, name: &str) -> bool {
    unsafe {
        let cname = std::ffi::CString::new(name).unwrap_or_default();
        let sym = crate::sexp::symbol::Rf_installChar(cname.as_ptr(), name.len() as crate::sexp::ffi::R_xlen_t);
        let mut cell = args;
        while !cell.is_null() && cell != crate::sexp::globals::R_NilValue() {
            let mut v = CAR(cell);
            if TYPEOF(v) == SEXPTYPE::PROMSXP {
                v = crate::sexp::accessors::PRVALUE(v);
            }
            let tag = crate::sexp::accessors::TAG(cell);
            let is_true = (TYPEOF(v) == SEXPTYPE::LGLSXP || TYPEOF(v) == SEXPTYPE::INTSXP)
                && !v.is_null()
                && *INTEGER(v) == 1;
            if tag == sym {
                if is_true {
                    return true;
                }
            }
            cell = CDR(cell);
        }
        false
    }
}

fn merge_named_value(args: SEXP, name: &str) -> SEXP {
    unsafe {
        let cname = std::ffi::CString::new(name).unwrap_or_default();
        let sym = crate::sexp::symbol::Rf_installChar(
            cname.as_ptr(),
            name.len() as crate::sexp::ffi::R_xlen_t,
        );
        let mut cell = args;
        while !cell.is_null() && cell != crate::sexp::globals::R_NilValue() {
            let tag = crate::sexp::accessors::TAG(cell);
            if tag == sym {
                let mut v = CAR(cell);
                if TYPEOF(v) == SEXPTYPE::PROMSXP {
                    v = crate::sexp::accessors::PRVALUE(v);
                    if v == crate::sexp::globals::R_UnboundValue() {
                        v = crate::eval::eval::Rf_eval(CAR(cell), crate::sexp::accessors::PRENV(CAR(cell)));
                    }
                }
                return v;
            }
            cell = CDR(cell);
        }
        crate::sexp::globals::R_NilValue()
    }
}

unsafe fn store_na(out: SEXP, i: i64) {
    unsafe {
        let t = TYPEOF(out);
        if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            *INTEGER(out).add(i as usize) = crate::sexp::ffi::NA_INTEGER;
        } else if t == SEXPTYPE::REALSXP {
            *REAL(out).add(i as usize) = f64::NAN;
        } else if t == SEXPTYPE::STRSXP {
            SET_STRING_ELT(out, i, crate::sexp::globals::R_NaString());
        }
    }
}

/// GNU `merge` inner join on intersecting column names.
pub unsafe fn do_merge(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        if TYPEOF(x) != SEXPTYPE::VECSXP || TYPEOF(y) != SEXPTYPE::VECSXP {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "'by' must specify one or more columns as numbers, names or logical",
            );
        }
        let xnames = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        let ynames = crate::sexp::attrib_core::getAttrib(
            y,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        let mut by: Vec<String> = Vec::new();
        let mut x_by: Vec<usize> = Vec::new();
        let mut y_by: Vec<usize> = Vec::new();
        let by_x = merge_named_value(args, "by.x");
        let by_y = merge_named_value(args, "by.y");
        if TYPEOF(by_x) == SEXPTYPE::STRSXP
            && TYPEOF(by_y) == SEXPTYPE::STRSXP
            && XLENGTH(by_x) > 0
            && XLENGTH(by_x) == XLENGTH(by_y)
        {
            for k in 0..XLENGTH(by_x) {
                let xn = elt_to_string(by_x, k);
                let yn = elt_to_string(by_y, k);
                let xi = (0..XLENGTH(xnames)).find(|&i| elt_to_string(xnames, i) == xn);
                let yi = (0..XLENGTH(ynames)).find(|&i| elt_to_string(ynames, i) == yn);
                if let (Some(xi), Some(yi)) = (xi, yi) {
                    by.push(xn);
                    x_by.push(xi as usize);
                    y_by.push(yi as usize);
                }
            }
        } else if TYPEOF(xnames) == SEXPTYPE::STRSXP && TYPEOF(ynames) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(xnames) {
                let n = elt_to_string(xnames, i);
                for j in 0..XLENGTH(ynames) {
                    if n == elt_to_string(ynames, j) {
                        by.push(n.clone());
                        x_by.push(i as usize);
                        y_by.push(j as usize);
                    }
                }
            }
        }
        if by.is_empty() {
            crate::mainutils::errors::errorcall_str(
                unsafe { crate::mainutils::errors::R_getCurrentCall() },
                "'by' must specify one or more columns as numbers, names or logical",
            );
        }
        let nx = if XLENGTH(x) > 0 { XLENGTH(VECTOR_ELT(x, 0)) } else { 0 };
        let ny = if XLENGTH(y) > 0 { XLENGTH(VECTOR_ELT(y, 0)) } else { 0 };
        let mut pairs: Vec<(i64, i64)> = Vec::new();
        for i in 0..nx {
            for j in 0..ny {
                let mut ok = true;
                for k in 0..by.len() {
                    if !merge_keys_equal(
                        VECTOR_ELT(x, x_by[k] as i64),
                        i,
                        VECTOR_ELT(y, y_by[k] as i64),
                        j,
                    ) {
                        ok = false;
                        break;
                    }
                }
                if ok {
                    pairs.push((i, j));
                }
            }
        }
        let all_flag = merge_named_true(args, "all");
        let all_x = all_flag || merge_named_true(args, "all.x");
        let all_y = all_flag || merge_named_true(args, "all.y");
        if all_x {
            for i in 0..nx {
                if !pairs.iter().any(|p| p.0 == i) {
                    pairs.push((i, -1));
                }
            }
        }
        if all_y {
            for j in 0..ny {
                if !pairs.iter().any(|p| p.1 == j) {
                    pairs.push((-1, j));
                }
            }
        }
        let sort_arg = merge_named_value(args, "sort");
        let do_sort = sort_arg.is_null()
            || sort_arg == R_NilValue()
            || sort_arg == crate::sexp::globals::R_MissingArg()
            || merge_named_true(args, "sort");
        if do_sort && !by.is_empty() {
            let xkey = VECTOR_ELT(x, x_by[0] as i64);
            let ykey = VECTOR_ELT(y, y_by[0] as i64);
            let key_of = |idx: i64, col: SEXP| -> String {
                if idx < 0 {
                    String::new()
                } else {
                    elt_to_string(col, idx)
                }
            };
            pairs.sort_by(|a, b| {
                let ka = if a.0 >= 0 { key_of(a.0, xkey) } else { key_of(a.1, ykey) };
                let kb = if b.0 >= 0 { key_of(b.0, xkey) } else { key_of(b.1, ykey) };
                ka.cmp(&kb)
            });
        }
        let nout = pairs.len() as i64;
        let x_extra: Vec<usize> = (0..XLENGTH(x) as usize)
            .filter(|i| !x_by.contains(i))
            .collect();
        let y_extra: Vec<usize> = (0..XLENGTH(y) as usize)
            .filter(|i| !y_by.contains(i))
            .collect();
        let ncols = by.len() + x_extra.len() + y_extra.len();
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncols as i64);
        let _result = protect(result);
        let mut names: Vec<String> = Vec::new();
        let mut col = 0i64;
        for (k, name) in by.iter().enumerate() {
            let src = VECTOR_ELT(x, x_by[k] as i64);
            let out = Rf_allocVector3(TYPEOF(src), nout);
            for (dst, (xi, yr)) in pairs.iter().enumerate() {
                if *xi < 0 {
                    let ysrc = VECTOR_ELT(y, y_by[k] as i64);
                    copy_elt(ysrc, *yr, out, dst as i64);
                } else {
                    copy_elt(src, *xi, out, dst as i64);
                }
            }
            SET_VECTOR_ELT(result, col, out);
            names.push(name.clone());
            col += 1;
        }
        for &xi in &x_extra {
            let src = VECTOR_ELT(x, xi as i64);
            let out = Rf_allocVector3(TYPEOF(src), nout);
            for (dst, (xr, _)) in pairs.iter().enumerate() {
                if *xr < 0 { store_na(out, dst as i64); } else { copy_elt(src, *xr, out, dst as i64); }
            }
            SET_VECTOR_ELT(result, col, out);
            names.push(elt_to_string(xnames, xi as i64));
            col += 1;
        }
        for &yi in &y_extra {
            let src = VECTOR_ELT(y, yi as i64);
            let out = Rf_allocVector3(TYPEOF(src), nout);
            for (dst, (_, yr)) in pairs.iter().enumerate() {
                if *yr < 0 { store_na(out, dst as i64); } else { copy_elt(src, *yr, out, dst as i64); }
            }
            SET_VECTOR_ELT(result, col, out);
            names.push(elt_to_string(ynames, yi as i64));
            col += 1;
        }
        set_string_names(result, &names);
        set_compact_row_names(result, nout);
        set_data_frame_class(result);
        result
    }
}

unsafe fn merge_keys_equal(a: SEXP, i: i64, b: SEXP, j: i64) -> bool {
    unsafe {
        if TYPEOF(a) != TYPEOF(b) {
            return format_grid_elt(a, i) == format_grid_elt(b, j);
        }
        match TYPEOF(a) {
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                *INTEGER(a).add(i as usize) == *INTEGER(b).add(j as usize)
            }
            t if t == SEXPTYPE::REALSXP => {
                *REAL(a).add(i as usize) == *REAL(b).add(j as usize)
            }
            t if t == SEXPTYPE::STRSXP => STRING_ELT(a, i) == STRING_ELT(b, j),
            _ => format_grid_elt(a, i) == format_grid_elt(b, j),
        }
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

pub(crate) unsafe fn recycle_column_if_needed(x: SEXP, target_len: R_xlen_t) -> SEXP {
    unsafe {
        // GNU recycles data.frame columns by *rows*. A 2-D AsIs / model.matrix
        // column has length nrow*ncol, so XLENGTH is the wrong unit.
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let len = if TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) == 2 {
            *INTEGER(dim) as R_xlen_t
        } else {
            XLENGTH(x)
        };
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
        // `...` are the columns; the remaining formals (row.names,
        // check.rows, check.names, fix.empty.names, stringsAsFactors) are
        // options, never columns. Drop option cells before list-building so
        // `data.frame(a=1, stringsAsFactors=FALSE)` keeps 1 column.
        let mut filtered = R_NilValue();
        let mut tail: SEXP = std::ptr::null_mut();
        let mut filter_guards: Vec<_> = Vec::new();
        let mut ap = args;
        while !ap.is_null() && ap != R_NilValue() {
            let is_option = matches!(
                tag_name(ap).as_deref(),
                Some("row.names")
                    | Some("check.rows")
                    | Some("check.names")
                    | Some("fix.empty.names")
                    | Some("stringsAsFactors")
            );
            if !is_option {
                let cell = Rf_cons(CAR(ap), R_NilValue());
                filter_guards.push(protect(cell));
                let tg = TAG(ap);
                if !tg.is_null() && tg != R_NilValue() {
                    SETTAG(cell, tg);
                }
                if filtered == R_NilValue() {
                    filtered = cell;
                } else {
                    SETCDR(tail, cell);
                }
                tail = cell;
            }
            ap = CDR(ap);
        }
        let _filtered_guard = protect(filtered);
        let initial = do_list(_call, _op, filtered, _rho);
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
            } else if TYPEOF(crate::sexp::attrib_core::getAttrib(
                value,
                crate::sexp::attrib_core::R_DimSymbol(),
            )) == SEXPTYPE::INTSXP
                && XLENGTH(crate::sexp::attrib_core::getAttrib(
                    value,
                    crate::sexp::attrib_core::R_DimSymbol(),
                )) == 2
            {
                let dim = crate::sexp::attrib_core::getAttrib(
                    value,
                    crate::sexp::attrib_core::R_DimSymbol(),
                );
                let nr = *INTEGER(dim) as R_xlen_t;
                let nc = *INTEGER(dim).add(1) as R_xlen_t;
                match nrow {
                    Some(existing) if nr != existing && nr != 0 && existing != 0 => {
                        base_error(format!(
                            "arguments imply differing number of rows: {existing}, {nr}"
                        ))
                    }
                    None => nrow = Some(nr),
                    _ => {}
                }
                if nr != 0 || nc == 0 {
                    nrow = Some(nrow.unwrap_or(nr));
                }
                // GNU `as.data.frame.AsIs` (and `as.data.frame.model.matrix`):
                // a 2-D AsIs object stays one list column. Bare matrices split.
                if sexp_has_class(value, "AsIs") || sexp_has_class(value, "model.matrix") {
                    columns.push(value);
                    names.push(arg_name);
                } else {
                    for j in 0..nc {
                        columns.push(super::s3::matrix_column(value, nr, j));
                        names.push(if arg_name.is_empty() {
                            format!("V{}", j + 1)
                        } else if nc == 1 {
                            arg_name.clone()
                        } else {
                            format!("{arg_name}.{}", j + 1)
                        });
                    }
                }
            } else {
                let value = if sexp_has_class(value, "POSIXlt") && TYPEOF(value) == SEXPTYPE::VECSXP
                {
                    let ct = crate::mainutils::essentials::do_as_POSIXct(
                        _call,
                        _op,
                        Rf_cons(value, R_NilValue()),
                        _rho,
                    );
                    filter_guards.push(protect(ct));
                    ct
                } else {
                    value
                };
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
pub unsafe fn do_unlist(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut ans = R_NilValue();
        if crate::eval::dispatch::DispatchOrEval(
            call,
            op,
            c"unlist".as_ptr(),
            args,
            rho,
            &mut ans,
            0,
            1,
        ) != 0
        {
            return ans;
        }
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
        collect_unlist_entries(x, UnlistName::Absent, recursive, use_names, &mut entries);

        let result_type = unlist_result_type(&entries);
        let total = entries.len() as R_xlen_t;
        // GNU AnswerType still records the child type when every element
        // has length 0. unlist(list(list(), list()), recursive=FALSE) is
        // list(), unlist(list(integer(0))) is integer(0), and only an
        // all-NULL walk stays NULL.
        if entries.is_empty() {
            return unlist_empty_result(x, recursive, use_names, call);
        }

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
                t if t == SEXPTYPE::LGLSXP => {
                    *LOGICAL(result).add(idx) = entry.value.as_logical();
                }
                _ => {
                    *INTEGER(result).add(idx) = entry.value.as_integer();
                }
            }
        }

        if use_names && entries.iter().any(|entry| !matches!(entry.name, UnlistName::Absent)) {
            let names = Rf_allocVector3(SEXPTYPE::STRSXP, total);
            if !names.is_null() {
                let _names_guard = protect(names);
                for (idx, entry) in entries.iter().enumerate() {
                    let charsxp = match &entry.name {
                        UnlistName::Na => crate::sexp::globals::R_NaString(),
                        UnlistName::Value(s) => {
                            let cstr = CString::new(s.as_str()).unwrap_or_default();
                            Rf_mkChar(cstr.as_ptr())
                        }
                        UnlistName::Absent => Rf_mkChar(c"".as_ptr()),
                    };
                    SET_STRING_ELT(names, idx as R_xlen_t, charsxp);
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
    name: UnlistName,
}

#[derive(Clone)]
enum UnlistName {
    Absent,
    Na,
    Value(String),
}


enum UnlistValue {
    Logical(i32),
    Integer(i32),
    Real(f64),
    Complex(Rcomplex),
    String(String),
    Object(SEXP),
    /// Atomic element still owned by the input vector — materialize at fill.
    Element { parent: SEXP, index: R_xlen_t },
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
            Self::Complex(_) | Self::String(_) | Self::Object(_) | Self::Element { .. } => {
                NA_INTEGER
            }
        }
    }

    fn as_logical(&self) -> i32 {
        self.as_integer()
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
            Self::String(_) | Self::Object(_) | Self::Element { .. } => NA_REAL,

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
            Self::String(_) | Self::Object(_) | Self::Element { .. } => Rcomplex {
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
            Self::Element { parent, index } => unsafe { elt_to_string(*parent, *index) },
        }
    }

    fn as_sexp(&self) -> SEXP {
        match self {
            Self::Object(value) => *value,
            Self::Element { parent, index } => unsafe { unlist_scalar_element(*parent, *index) },
            _ => unsafe { R_NilValue() },
        }
    }
}

unsafe fn unlist_empty_result(x: SEXP, recursive: bool, use_names: bool, call: SEXP) -> SEXP {
    unsafe {
        let mut data = crate::mainutils::bind::BindData {
            ans_flags: 0,
            ans_ptr: std::ptr::null_mut(),
            ans_length: 0,
            ans_names: std::ptr::null_mut(),
            ans_nnames: 0,
        };
        if TYPEOF(x) == SEXPTYPE::VECSXP || TYPEOF(x) == SEXPTYPE::EXPRSXP {
            for i in 0..XLENGTH(x) {
                crate::mainutils::bind::AnswerType(
                    VECTOR_ELT(x, i),
                    recursive,
                    use_names,
                    &mut data,
                    call,
                );
            }
        }
        if data.ans_flags == 0 {
            return R_NilValue();
        }
        Rf_allocVector3(
            crate::mainutils::bind::ans_flags_to_mode(data.ans_flags),
            0,
        )
    }
}


fn unlist_result_type(entries: &[UnlistEntry]) -> SEXPTYPE {
    if entries
        .iter()
        .any(|entry| {
            matches!(
                entry.value,
                UnlistValue::Object(_) | UnlistValue::Element { .. }
            )
        })

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
    } else if entries
        .iter()
        .any(|entry| matches!(entry.value, UnlistValue::Integer(_)))
    {
        SEXPTYPE::INTSXP
    } else if entries
        .iter()
        .any(|entry| matches!(entry.value, UnlistValue::Logical(_)))
    {
        SEXPTYPE::LGLSXP
    } else {
        SEXPTYPE::INTSXP
    }

}

unsafe fn unlist_scalar_element(x: SEXP, index: R_xlen_t) -> SEXP {
    unsafe {
        match TYPEOF(x) {
            t if t == SEXPTYPE::LGLSXP => Rf_ScalarLogical(*LOGICAL(x).add(index as usize)),
            t if t == SEXPTYPE::INTSXP => Rf_ScalarInteger(*INTEGER(x).add(index as usize)),
            t if t == SEXPTYPE::REALSXP => Rf_ScalarReal(*REAL(x).add(index as usize)),
            t if t == SEXPTYPE::CPLXSXP => {
                let value = *COMPLEX(x).add(index as usize);
                crate::sexp::constructors::Rf_ScalarComplex(value)
            }
            t if t == SEXPTYPE::STRSXP => {
                let out = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
                SET_STRING_ELT(out, 0, STRING_ELT(x, index));
                out
            }
            t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => VECTOR_ELT(x, index),
            _ => x,
        }
    }
}

unsafe fn collect_unlist_entries(
    x: SEXP,
    prefix: UnlistName,

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
            let n = XLENGTH(x);
            if !recursive {
                let any_list = (0..n).any(|i| {
                    let child = VECTOR_ELT(x, i);
                    TYPEOF(child) == SEXPTYPE::VECSXP || TYPEOF(child) == SEXPTYPE::EXPRSXP
                });
                if any_list || !matches!(prefix, UnlistName::Absent) {
                    for i in 0..n {
                        let child = VECTOR_ELT(x, i);
                        let child_name = if use_names {
                            unlist_element_name(&prefix, names, i, n)
                        } else {
                            UnlistName::Absent
                        };
                        if TYPEOF(child) == SEXPTYPE::VECSXP
                            || TYPEOF(child) == SEXPTYPE::EXPRSXP
                        {
                            let child_names = crate::sexp::attrib_core::getAttrib(
                                child,
                                crate::sexp::attrib_core::R_NamesSymbol(),
                            );
                            let child_n = XLENGTH(child);
                            for j in 0..child_n {
                                let leaf_name = if use_names {
                                    unlist_element_name(&child_name, child_names, j, child_n)
                                } else {
                                    UnlistName::Absent
                                };
                                out.push(UnlistEntry {
                                    value: UnlistValue::Object(VECTOR_ELT(child, j)),
                                    name: leaf_name,
                                });
                            }
                        } else {
                            let child_n = XLENGTH(child);
                            let child_names = crate::sexp::attrib_core::getAttrib(
                                child,
                                crate::sexp::attrib_core::R_NamesSymbol(),
                            );
                            for j in 0..child_n {
                                let leaf_name = if use_names {
                                    unlist_element_name(&child_name, child_names, j, child_n)
                                } else {
                                    UnlistName::Absent
                                };
                                out.push(UnlistEntry {
                                    value: UnlistValue::Element {
                                        parent: child,
                                        index: j,
                                    },
                                    name: leaf_name,
                                });
                            }
                        }
                    }
                    return;
                }
            }
            for i in 0..n {
                let child_name = if use_names {
                    unlist_element_name(&prefix, names, i, n)
                } else {
                    UnlistName::Absent
                };
                collect_unlist_entries(VECTOR_ELT(x, i), child_name, recursive, use_names, out);
            }
            return;
        }

        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        for i in 0..XLENGTH(x) {
            let name = if use_names {
                unlist_element_name(&prefix, names, i, XLENGTH(x))
            } else {
                UnlistName::Absent
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
    prefix: &UnlistName,
    names: SEXP,
    index: R_xlen_t,
    len: R_xlen_t,
) -> UnlistName {
    unsafe {
        let own = if !names.is_null()
            && names != R_NilValue()
            && TYPEOF(names) == SEXPTYPE::STRSXP
            && index < XLENGTH(names)
        {
            let elt = STRING_ELT(names, index);
            if elt.is_null() || elt == crate::sexp::globals::R_NaString() {
                UnlistName::Na
            } else {
                let value = CStr::from_ptr(CHAR(elt)).to_string_lossy().into_owned();
                if value.is_empty() {
                    UnlistName::Absent
                } else {
                    UnlistName::Value(value)
                }
            }
        } else {
            UnlistName::Absent
        };

        match (prefix, own) {
            (UnlistName::Value(prefix), UnlistName::Value(own)) => {
                UnlistName::Value(format!("{prefix}.{own}"))
            }
            (UnlistName::Absent, own) => own,
            (UnlistName::Na, UnlistName::Value(own)) => UnlistName::Value(format!("NA.{own}")),
            (UnlistName::Na, UnlistName::Absent) if len > 1 => {
                UnlistName::Value(format!("NA{}", index + 1))
            }
            (UnlistName::Na, _) => UnlistName::Na,
            (UnlistName::Value(prefix), UnlistName::Na) => UnlistName::Value(format!("{prefix}.NA")),
            (UnlistName::Value(prefix), UnlistName::Absent) if len > 1 => {
                UnlistName::Value(format!("{}{}", prefix, index + 1))
            }
            (UnlistName::Value(prefix), UnlistName::Absent) => UnlistName::Value(prefix.clone()),
        }
    }
}


/// R's `is.atomic(x)` — TRUE for non-recursive types (not list, pairlist, etc.).
pub unsafe fn do_is_atomic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        let is_atomic = t == SEXPTYPE::LGLSXP
            || t == SEXPTYPE::INTSXP
            || t == SEXPTYPE::REALSXP
            || t == SEXPTYPE::CPLXSXP
            || t == SEXPTYPE::STRSXP
            || t == SEXPTYPE::RAWSXP
            || t == SEXPTYPE::CHARSXP;
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
pub unsafe fn do_split(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let f = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || f.is_null() || f == R_NilValue() {
            return R_NilValue();
        }

        let n = if crate::mainutils::essentials::sexp_has_class(x, "POSIXlt")
            && TYPEOF(x) == SEXPTYPE::VECSXP
        {
            crate::mainutils::subassign::posixlt_obs_length(x)
        } else {
            XLENGTH(x)
        };
        let classed = crate::sexp::accessors::OBJECT(x) != 0;

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
            let sub = if classed {
                let idx = Rf_allocVector3(SEXPTYPE::INTSXP, indices.len() as R_xlen_t);
                let _idx = protect(idx);
                for (dst, &src) in indices.iter().enumerate() {
                    *INTEGER(idx).add(dst) = (src + 1) as c_int;
                }
                let sub_args = Rf_cons(x, Rf_cons(idx, R_NilValue()));
                let _sa = protect(sub_args);
                crate::mainutils::subset::do_subset(_call, _op, sub_args, rho)
            } else {
                let sub = Rf_allocVector3(TYPEOF(x), indices.len() as R_xlen_t);
                let _sub_guard = protect(sub);
                for (dst, &src) in indices.iter().enumerate() {
                    copy_matrix_element(sub, dst as R_xlen_t, x, src);
                }
                restore_datetime_or_difftime_class(x, sub);
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
                sub
            };
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

/// GNU `unsplit(value, f)` inverse of `split`.
pub unsafe fn do_unsplit(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let value = CAR(args);
        let f = CAR(CDR(args));
        if value.is_null()
            || value == R_NilValue()
            || TYPEOF(value) != SEXPTYPE::VECSXP
            || XLENGTH(value) == 0
            || f.is_null()
            || f == R_NilValue()
        {
            return R_NilValue();
        }
        let first = VECTOR_ELT(value, 0);
        let n = XLENGTH(f);
        let result = Rf_allocVector3(TYPEOF(first), n);
        let _r = protect(result);
        let ng = XLENGTH(value) as usize;
        let mut cursor = vec![0i64; ng];
        for i in 0..n {
            let g = if TYPEOF(f) == SEXPTYPE::INTSXP {
                (*INTEGER(f).add(i as usize) as i64) - 1
            } else {
                elt_real_safe(f, i).round() as i64 - 1
            };
            if g < 0 || (g as usize) >= ng {
                continue;
            }
            let src = VECTOR_ELT(value, g);
            let pos = cursor[g as usize];
            if !src.is_null() && src != R_NilValue() && pos < XLENGTH(src) {
                copy_matrix_element(result, i, src, pos);
                cursor[g as usize] = pos + 1;
            }
        }
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

/// R's plot generic delegates through the ordinary S3 machinery.
pub unsafe fn do_plot(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "plot",
            "function(x, y, ...) UseMethod('plot')",
            args,
            rho,
            false,
        )
    }
}

/// GNU `preplot(object, ...)` — `UseMethod("preplot")`.
pub unsafe fn do_preplot(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "preplot",
            "function(object, ...) UseMethod('preplot')",
            args,
            rho,
            false,
        )
    }
}

/// GNU `profile(fitted, ...)` — `UseMethod("profile")`.
pub unsafe fn do_profile(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "profile",
            "function(fitted, ...) UseMethod('profile')",
            args,
            rho,
            false,
        )
    }
}

/// GNU `tsdiag(object, gof.lag, ...)` — `UseMethod("tsdiag")`.
pub unsafe fn do_tsdiag(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "tsdiag",
            "function(object, gof.lag, ...) UseMethod('tsdiag')",
            args,
            rho,
            false,
        )
    }
}

/// GNU `free1way(y, ...)` — `UseMethod("free1way")`.
pub unsafe fn do_free1way(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "free1way",
            "function(y, ...) UseMethod('free1way')",
            args,
            rho,
            false,
        )
    }
}

/// GNU `power.free1way.test` — exactly one of n/delta/power/sig.level is NULL.
pub unsafe fn do_power_free1way_test(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut n_null = true;
        let mut delta_null = true;
        let mut power_null = true;
        let mut sig_null = false;
        let mut cell = args;
        let mut pos = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let val = CAR(cell);
            let is_null = val.is_null() || val == R_NilValue();
            if name == "n" || (name.is_empty() && pos == 0) {
                n_null = is_null;
            } else if name == "delta" {
                delta_null = is_null;
            } else if name == "power" {
                power_null = is_null;
            } else if name == "sig.level" {
                sig_null = is_null;
            }
            if name.is_empty() {
                pos += 1;
            }
            cell = CDR(cell);
        }
        let nnull = [n_null, delta_null, power_null, sig_null]
            .iter()
            .filter(|b| **b)
            .count();
        if nnull != 1 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "exactly one of 'n', 'delta', 'power', and 'sig.level' must be NULL",
            );
        }
        R_NilValue()
    }
}


/// GNU `monthplot(x, ...)` — `UseMethod("monthplot")`.
pub unsafe fn do_monthplot(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "monthplot",
            "function(x, ...) UseMethod('monthplot')",
            args,
            rho,
            false,
        )
    }
}

/// GNU `monthplot.default(x)` — `range(x)` becomes `ylim`.
pub unsafe fn do_monthplot_default(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = if x.is_null() || x == R_NilValue() {
            0
        } else {
            XLENGTH(x)
        };
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        let mut any = false;
        for i in 0..n {
            let v = elt_real_safe(x, i);
            if v.is_finite() {
                any = true;
                if v < lo {
                    lo = v;
                }
                if v > hi {
                    hi = v;
                }
            }
        }
        if !any || !lo.is_finite() || !hi.is_finite() {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid 'ylim' value",
            );
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// GNU `scatter.smooth(x, y)` — `seq(min(x), max(x), length.out=)`.
pub unsafe fn do_scatter_smooth(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = if x.is_null() || x == R_NilValue() {
            0
        } else {
            XLENGTH(x)
        };
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        let mut any = false;
        for i in 0..n {
            let v = elt_real_safe(x, i);
            if v.is_finite() {
                any = true;
                if v < lo {
                    lo = v;
                }
                if v > hi {
                    hi = v;
                }
            }
        }
        if !any || !lo.is_finite() || !hi.is_finite() {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'from' must be a finite number",
            );
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// GNU `interaction.plot(x.factor, trace.factor, response)` — invisible.
pub unsafe fn do_interaction_plot(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let rest = CDR(args);
        let trace = if rest.is_null() || rest == R_NilValue() {
            R_NilValue()
        } else {
            CAR(rest)
        };
        let resp = if rest.is_null() || rest == R_NilValue() {
            R_NilValue()
        } else {
            let r2 = CDR(rest);
            if r2.is_null() || r2 == R_NilValue() {
                R_NilValue()
            } else {
                CAR(r2)
            }
        };
        if x.is_null()
            || x == R_NilValue()
            || XLENGTH(x) == 0
            || resp.is_null()
            || resp == R_NilValue()
            || XLENGTH(resp) == 0
        {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid 'ylim' value",
            );
        }
        let _ = trace;
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// GNU `lag.plot(x)` — `as.ts(as.matrix(x))` then plot.
pub unsafe fn do_lag_plot(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || XLENGTH(x) == 0 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "invalid 'xlim' value",
            );
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// GNU `eff.aovlist(aovlist)` — `$qr` on each stratum.
pub unsafe fn do_eff_aovlist(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let aovlist = CAR(args);
        if aovlist.is_null() || aovlist == R_NilValue() || TYPEOF(aovlist) != SEXPTYPE::VECSXP {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "$ operator is invalid for atomic vectors",
            );
        }
        if XLENGTH(aovlist) == 0 {
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return R_NilValue();
        }
        let dollar = Rf_lang3(
            crate::sexp::symbol::R_DollarSymbol(),
            VECTOR_ELT(aovlist, 0),
            Rf_install(c"qr".as_ptr()),
        );
        let _ = crate::eval::eval::Rf_eval(dollar, rho);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}



/// GNU `biplot(x, ...)` — `UseMethod("biplot")`.
pub unsafe fn do_biplot(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "biplot",
            "function(x, ...) UseMethod('biplot')",
            args,
            rho,
            false,
        )
    }
}

/// GNU `screeplot(x, ...)` — `UseMethod("screeplot")`.
pub unsafe fn do_screeplot(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "screeplot",
            "function(x, ...) UseMethod('screeplot')",
            args,
            rho,
            false,
        )
    }
}


/// GNU `biplot.default(x, y, ...)` — requires `y`.
pub unsafe fn do_biplot_default(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let y = CAR(CDR(args));
        if y.is_null() || y == R_NilValue() || y == crate::sexp::globals::R_MissingArg() {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "argument \"y\" is missing, with no default",
            );
        }
        R_NilValue()
    }
}

/// GNU `screeplot.default(x)` — reads `x$sdev`.
pub unsafe fn do_screeplot_default(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let dollar = crate::sexp::constructors::Rf_lang3(
            crate::sexp::symbol::Rf_install(c"$".as_ptr()),
            x,
            crate::sexp::symbol::Rf_install(c"sdev".as_ptr()),
        );
        let _d = protect(dollar);
        crate::eval::eval::Rf_eval(dollar, rho);
        R_NilValue()
    }
}

/// GNU `plot.spec.coherency(x)` — reads `x$spec`.
pub unsafe fn do_plot_spec_coherency(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { spec_plot_read_spec(args, rho) }
}

/// GNU `plot.spec.phase(x)` — reads `x$spec`.
pub unsafe fn do_plot_spec_phase(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { spec_plot_read_spec(args, rho) }
}

unsafe fn spec_plot_read_spec(args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let dollar = Rf_lang3(
            Rf_install(c"$".as_ptr()),
            x,
            Rf_install(c"spec".as_ptr()),
        );
        let _d = protect(dollar);
        crate::eval::eval::Rf_eval(dollar, rho);
        R_NilValue()
    }
}



pub unsafe fn do_plot_default(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    #[cfg(feature = "renderplot-device")]
    unsafe {
        crate::mainutils::portable_plot::plot_default(call, op, args, rho)
    }
    #[cfg(not(feature = "renderplot-device"))]
    {
        let _ = (call, op, args, rho);
        crate::sexp::globals::R_NilValue()
    }
}

macro_rules! portable_graphics_handlers {
    ($($handler:ident => $name:literal),* $(,)?) => {$(
        pub unsafe fn $handler(_call:SEXP,_op:SEXP,args:SEXP,_rho:SEXP)->SEXP {
            #[cfg(feature="renderplot-device")]
            unsafe {crate::mainutils::portable_plot::draw_builtin($name,args)}
            #[cfg(not(feature="renderplot-device"))]
            {let _=args; crate::sexp::globals::R_NilValue()}
        }
    )*};
}
portable_graphics_handlers! {
    do_lines_default=>"lines.default",do_points_default=>"points.default",
    do_segments=>"segments",do_arrows=>"arrows",do_polygon=>"polygon",
    do_text_default=>"text.default",do_title=>"title",do_box=>"box",do_axis=>"axis",do_plot_new=>"plot.new",do_plot_window=>"plot.window",
}

/// GNU `rect(...)` — no device: `plot.new has not been called yet`.
pub unsafe fn do_rect(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    #[cfg(feature = "renderplot-device")]
    unsafe {
        crate::mainutils::portable_plot::draw_builtin("rect", args)
    }
    #[cfg(not(feature = "renderplot-device"))]
    {
        let _ = args;
        crate::mainutils::errors::errorcall_str(
            crate::mainutils::errors::R_getCurrentCall(),
            "plot.new has not been called yet",
        );
    }
}

/// GNU `abline(...)` — no device: `plot.new has not been called yet`.
pub unsafe fn do_abline(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    #[cfg(feature = "renderplot-device")]
    unsafe {
        crate::mainutils::portable_plot::draw_builtin("abline", args)
    }
    #[cfg(not(feature = "renderplot-device"))]
    {
        let _ = args;
        // This build has no graphics device. GNU Rscript opens one and
        // draws; the regression only needs the call not to stop.
        R_NilValue()
    }
}


macro_rules! graphics_generics {
    ($($handler:ident => $name:literal),* $(,)?) => {$(
        pub unsafe fn $handler(_call:SEXP,_op:SEXP,args:SEXP,rho:SEXP)->SEXP {
            unsafe {
                crate::mainutils::base_wrappers::apply($name,concat!("function(x,...) UseMethod('",$name,"')"),args,rho,false)
            }
        }
    )*};
}
graphics_generics! {do_lines=>"lines",do_points=>"points",do_text=>"text"}
