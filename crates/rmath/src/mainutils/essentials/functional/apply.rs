//! lapply/sapply/vapply/Map/Filter/replicate family.

use super::*;

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

pub(super) unsafe fn sapply_control_args(args: SEXP, x: SEXP, rho: SEXP) -> (SEXP, SEXP, bool) {
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

pub(super) unsafe fn sapply_is_false(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::LGLSXP && XLENGTH(x) == 1 && *LOGICAL(x) == FALSE }
}

pub(super) unsafe fn sapply_is_array(x: SEXP) -> bool {
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
pub(super) fn is_vapply_vector(x: SEXP) -> bool {
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
pub(super) fn vapply_use_names(args: SEXP, rho: SEXP) -> bool {
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
pub(super) fn normalize_vapply_x(x: SEXP) -> SEXP {
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
        if t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
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
pub(super) fn vapply_length(x: SEXP) -> R_xlen_t {
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

pub(super) fn vapply_is_nil(x: SEXP) -> bool {
    unsafe { x.is_null() || x == R_NilValue() }
}

/// Upstream type names as they appear in vapply's error messages
/// (R_typeToChar).
pub(super) fn sexp_type_name(t: SEXPTYPE) -> &'static str {
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
        let raw = callable_arg_by_name_or_position(args, &["f", "FUN"], 0, rho);
        let matched = Rf_cons(raw, R_NilValue());
        let _m = protect(matched);
        let call = Rf_cons(
            crate::sexp::symbol::Rf_install(c"match.fun".as_ptr()),
            matched,
        );
        crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
        let _c = protect(call);
        let fun = crate::eval::eval::Rf_eval(call, rho);

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
            include_str!("../../base_wrappers/filter.R"),
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
            include_str!("../../base_wrappers/replicate.R"),
            args,
            rho,
            false,
        )
    }
}
