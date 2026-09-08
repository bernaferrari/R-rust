use super::*;

// ---------------------------------------------------------------------------
// as.function, str2lang, as.call
// ---------------------------------------------------------------------------

/// do_asfunction — convert a list to a function (closure).
/// Matches C's `do_asfunction()` in coerce.c line 1605.
pub unsafe fn do_asfunction(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let arglist = CAR(args);
        if TYPEOF(arglist) != SEXPTYPE::VECSXP {
            error("list argument expected");
        }
        let envir = CADR(args);
        if isNull(envir) {
            error("use of NULL environment is defunct");
        }
        if !isEnvironment(envir) {
            error("invalid environment");
        }
        let n = LENGTH(arglist);
        if n < 1 {
            error("argument must have length at least 1");
        }
        let names = getAttrib(arglist, R_NamesSymbol());
        let _names_guard = protect(names);
        let pargs = crate::sexp::constructors::Rf_allocList(n - 1);
        let _pargs_guard = protect(pargs);
        let mut current = pargs;
        for i in 0..n - 1 {
            SETCAR(current, VECTOR_ELT(arglist, i as R_xlen_t));
            if names != R_NilValue() {
                let name_elt = STRING_ELT(names, i as R_xlen_t);
                if name_elt != R_NilValue() {
                    let c = CHAR(name_elt);
                    if !c.is_null() && *c != 0 {
                        SETTAG(current, crate::mainutils::subset::installTrChar(name_elt));
                    }
                }
            }
            current = CDR(current);
        }
        let body = VECTOR_ELT(arglist, (n - 1) as R_xlen_t);
        let _body_guard = protect(body);
        let bt = TYPEOF(body);
        if bt == SEXPTYPE::LISTSXP
            || bt == SEXPTYPE::LANGSXP
            || bt == SEXPTYPE::SYMSXP
            || bt == SEXPTYPE::EXPRSXP
            || bt == SEXPTYPE::VECSXP
            || bt == SEXPTYPE::RAWSXP
            || bt == SEXPTYPE::INTSXP
            || bt == SEXPTYPE::REALSXP
            || bt == SEXPTYPE::STRSXP
            || bt == SEXPTYPE::LGLSXP
        {
            crate::mainutils::dstruct::mkCLOSXP(pargs, body, envir)
        } else {
            error("invalid body for function");
        }
    }
}

/// do_str2lang — convert a string to a language/call object.
/// Matches C's `do_str2lang()` in coerce.c line 1668.
pub unsafe fn do_str2lang(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let s = CAR(args);
        if TYPEOF(s) != SEXPTYPE::STRSXP {
            error("argument must be character");
        }
        if LENGTH(s) != 1 {
            error("argument must be a single character string");
        }
        let mut status: c_int = 0;
        let srcfile = Rf_mkString(b"<text>\0".as_ptr() as *const c_char);
        let _srcfile_guard = protect(srcfile);
        let parsed = crate::mainutils::gram_main::R_ParseVector(s, -1, &mut status, srcfile);
        let _parsed_guard = protect(parsed);
        if status != 1 {
            error("parse error in str2lang");
        }
        if LENGTH(parsed) != 1 {
            error("parsing result not of length one");
        }
        let result = VECTOR_ELT(parsed, 0);
        result
    }
}

/// do_ascall — convert an object to a call object.
/// Matches C's `do_ascall()` in coerce.c line 1732.
pub unsafe fn do_ascall(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        match TYPEOF(x) {
            t if t == SEXPTYPE::LANGSXP => x,
            t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                let n = LENGTH(x);
                if n == 0 {
                    error("invalid length 0 argument");
                }
                let names = getAttrib(x, R_NamesSymbol());
                let _names_guard = protect(names);
                let ans = crate::sexp::constructors::Rf_allocList(n);
                let _ans_guard = protect(ans);
                let mut ap = ans;
                for i in 0..n {
                    SETCAR(ap, VECTOR_ELT(x, i as R_xlen_t));
                    if names != R_NilValue() {
                        let name_elt = STRING_ELT(names, i as R_xlen_t);
                        if name_elt != R_NilValue() {
                            let c = CHAR(name_elt);
                            if !c.is_null() && *c != 0 {
                                SETTAG(ap, crate::mainutils::subset::installTrChar(name_elt));
                            }
                        }
                    }
                    ap = CDR(ap);
                }
                SET_TYPEOF(ans, SEXPTYPE::LANGSXP.into());
                SETTAG(ans, R_NilValue());
                ans
            }
            t if t == SEXPTYPE::LISTSXP => {
                let ans = crate::mainutils::duplicate::Rf_duplicate(x);
                SET_TYPEOF(ans, SEXPTYPE::LANGSXP.into());
                SETTAG(ans, R_NilValue());
                ans
            }
            t if t == SEXPTYPE::STRSXP => {
                error("as.call(<character>) not feasible; consider str2lang()");
            }
            _ => {
                error("invalid argument list");
            }
        }
    }
}
pub unsafe fn do_isfinite(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = xlength(x);
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let _ans_guard = protect(ans);
        let pa = LOGICAL(ans);

        match TYPEOF(x) {
            t if t == SEXPTYPE::STRSXP || t == SEXPTYPE::RAWSXP || t == SEXPTYPE::NILSXP => {
                for i in 0..n {
                    *pa.add(i as usize) = 0;
                }
            }
            t if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP => {
                for i in 0..n {
                    *pa.add(i as usize) = (INTEGER_ELT(x, i as c_int) != NA_INTEGER) as c_int;
                }
            }
            t if t == SEXPTYPE::REALSXP => {
                for i in 0..n {
                    *pa.add(i as usize) = R_FINITE(REAL_ELT(x, i as c_int)) as c_int;
                }
            }
            t if t == SEXPTYPE::CPLXSXP => {
                for i in 0..n {
                    let v = COMPLEX_ELT(x, i as c_int);
                    *pa.add(i as usize) = (R_FINITE(v.r) && R_FINITE(v.i)) as c_int;
                }
            }
            _ => {
                error("default method not implemented for type");
            }
        }

        if isVector(x) {
            let dims = getAttrib(x, R_DimSymbol());
            if !isNull(dims) {
                setAttrib(ans, R_DimSymbol(), dims);
            }
            let names = if isArray(x) {
                getAttrib(x, R_DimNamesSymbol())
            } else {
                getAttrib(x, R_NamesSymbol())
            };
            if !isNull(names) {
                if isArray(x) {
                    setAttrib(ans, R_DimNamesSymbol(), names);
                } else {
                    setAttrib(ans, R_NamesSymbol(), names);
                }
            }
        }

        ans
    }
}

/// R-level `is.infinite()` entry point.
///
/// This is the `do_isinfinite()` function from coerce.c.
pub unsafe fn do_isinfinite(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = xlength(x);
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let _ans_guard = protect(ans);
        let pa = LOGICAL(ans);

        match TYPEOF(x) {
            t if t == SEXPTYPE::STRSXP
                || t == SEXPTYPE::RAWSXP
                || t == SEXPTYPE::NILSXP
                || t == SEXPTYPE::LGLSXP
                || t == SEXPTYPE::INTSXP =>
            {
                for i in 0..n {
                    *pa.add(i as usize) = 0;
                }
            }
            t if t == SEXPTYPE::REALSXP => {
                for i in 0..n {
                    let xr = REAL_ELT(x, i as c_int);
                    *pa.add(i as usize) = if ISNAN(xr) || R_FINITE(xr) { 0 } else { 1 };
                }
            }
            t if t == SEXPTYPE::CPLXSXP => {
                for i in 0..n {
                    let v = COMPLEX_ELT(x, i as c_int);
                    *pa.add(i as usize) =
                        if (ISNAN(v.r) || R_FINITE(v.r)) && (ISNAN(v.i) || R_FINITE(v.i)) {
                            0
                        } else {
                            1
                        };
                }
            }
            _ => {
                error("default method not implemented for type");
            }
        }

        if isVector(x) {
            let dims = getAttrib(x, R_DimSymbol());
            if !isNull(dims) {
                setAttrib(ans, R_DimSymbol(), dims);
            }
            let names = if isArray(x) {
                getAttrib(x, R_DimNamesSymbol())
            } else {
                getAttrib(x, R_NamesSymbol())
            };
            if !isNull(names) {
                if isArray(x) {
                    setAttrib(ans, R_DimNamesSymbol(), names);
                } else {
                    setAttrib(ans, R_NamesSymbol(), names);
                }
            }
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// do_coerce -- R-level coercion entry point
// ---------------------------------------------------------------------------

/// R-level coercion entry point (`do_coerce`).
///
/// This dispatches to `ascommon` for the actual coercion, matching R's
/// behavior for `as.vector()`, `as.expression()`, `as.list()`, etc.
pub unsafe fn do_coerce(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let args_s =
            Sexp::try_from_raw(args).unwrap_or_else(|err| errorcall(call, &err.to_string()));
        let x = args_s
            .clone()
            .try_pairlist_arg(0)
            .clone()
            .unwrap_or_else(|err| errorcall(call, &err.to_string()));
        let mode_str = match args_s.try_pairlist_arg(1) {
            Ok(s) => s,
            Err(_) => return x.as_raw(),
        };
        coerce_vector_safe(x, mode_str).unwrap_or_else(|message| errorcall(call, &message))
    }
}

// ---------------------------------------------------------------------------
// strtod wrapper (C lib)
// ---------------------------------------------------------------------------

/// Parse a full Rust string as a double via libc `strtod` (the same
/// conversion family as R's `R_strtod`: correctly rounded decimals, C99
/// hex floats such as "0x1p3", surrounding C whitespace allowed).
///
/// Returns `None` when the (whitespace-trimmed) string is not fully
/// consumed, mirroring the "rest is not blank" NA rule in
/// `RealFromString`/`ComplexFromString`.
pub unsafe fn parse_double_str(s: &str) -> Option<c_double> {
    let trimmed =
        s.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r'));
    let Ok(cstr) = std::ffi::CString::new(trimmed) else {
        return None;
    };
    let mut endp: *mut c_char = ptr::null_mut();
    let v = unsafe { strtod(cstr.as_ptr(), &mut endp) };
    if endp.is_null() || unsafe { *endp } != 0 {
        return None;
    }
    Some(v)
}

// ---------------------------------------------------------------------------
// anyNA — recursive NA detection
// ---------------------------------------------------------------------------

/// Check if any element of a vector contains NA values.
///
/// Ported from R's `anyNA()` in coerce.c.
pub fn any_na_impl(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> bool {
    use crate::sexp::accessors::{
        CADR, CAR, CDR, COMPLEX_ELT, INTEGER_ELT, LENGTH, LOGICAL_ELT, OBJECT, REAL_ELT,
        STRING_ELT, TYPEOF, VECTOR_ELT, XLENGTH,
    };
    use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, SEXPTYPE};
    use crate::sexp::globals::{R_NaString, R_NilValue};

    unsafe {
        let x = CAR(args);
        let xT = TYPEOF(x);
        let is_list = xT == SEXPTYPE::VECSXP || xT == SEXPTYPE::LISTSXP;

        let recursive = if is_list && LENGTH(args) > 1 {
            let r = CADR(args);
            asRbool(r, _call) != 0
        } else {
            false
        };

        // For objects or non-recursive lists, fall back to is.na + any
        if OBJECT(x) != 0 || (is_list && !recursive) {
            // Simplified: just check vector elements directly for non-objects
            if OBJECT(x) != 0 {
                // For S4/S3 objects, we'd need eval(dispatch) — skip for now
                return false;
            }
        }

        let n = XLENGTH(x);
        match xT {
            t if t == SEXPTYPE::REALSXP => {
                for i in 0..n as usize {
                    let v = REAL_ELT(x, i as c_int);
                    if v.is_nan() {
                        return true;
                    }
                }
                false
            }
            t if t == SEXPTYPE::INTSXP => {
                for i in 0..n as usize {
                    let v = INTEGER_ELT(x, i as c_int);
                    if v == NA_INTEGER {
                        return true;
                    }
                }
                false
            }
            t if t == SEXPTYPE::LGLSXP => {
                for i in 0..n as usize {
                    let v = LOGICAL_ELT(x, i as c_int);
                    if v == NA_LOGICAL {
                        return true;
                    }
                }
                false
            }
            t if t == SEXPTYPE::CPLXSXP => {
                for i in 0..n as usize {
                    let v = COMPLEX_ELT(x, i as c_int);
                    if v.r.is_nan() || v.i.is_nan() {
                        return true;
                    }
                }
                false
            }
            t if t == SEXPTYPE::STRSXP => {
                for i in 0..n as R_xlen_t {
                    if STRING_ELT(x, i) == R_NaString() {
                        return true;
                    }
                }
                false
            }
            t if t == SEXPTYPE::RAWSXP => false,
            t if t == SEXPTYPE::NILSXP => false,
            t if t == SEXPTYPE::VECSXP && recursive => {
                for i in 0..n as usize {
                    let elt = VECTOR_ELT(x, i as R_xlen_t);
                    // Recursively check each element
                    let inner_args = Rf_cons(elt, R_NilValue());
                    let _inner_args_guard = protect(inner_args);
                    let found = any_na_impl(_call, _op, inner_args, _env);
                    if found {
                        return true;
                    }
                }
                false
            }
            t if t == SEXPTYPE::LISTSXP && recursive => {
                let mut node = x;
                while !node.is_null() && node != R_NilValue() {
                    let elt = CAR(node);
                    let inner_args = Rf_cons(elt, R_NilValue());
                    let _inner_args_guard = protect(inner_args);
                    let found = any_na_impl(_call, _op, inner_args, _env);
                    if found {
                        return true;
                    }
                    node = CDR(node);
                }
                false
            }
            _ => false,
        }
    }
}

/// R-level entry point for `anyNA()`.
///
/// Ported from R's `do_anyNA()` in coerce.c.
pub unsafe fn do_anyNA(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    use crate::sexp::constructors::Rf_ScalarLogical;
    use crate::sexp::ffi::FALSE;

    unsafe {
        let nargs = LENGTH(args);
        if nargs < 1 || nargs > 2 {
            crate::mainutils::errors::Rf_error(
                b"anyNA takes 1 or 2 arguments\0".as_ptr() as *const c_char
            );
        }

        // Simplified: skip DispatchOrEval for now, call any_na_impl directly
        if nargs == 1 {
            Rf_ScalarLogical(if any_na_impl(call, op, args, rho) {
                1
            } else {
                FALSE
            })
        } else {
            // Two args: x and recursive (default FALSE)
            // Ensure second arg exists and is logical
            let recursive_val = CADR(args);
            let full_args = args;
            if recursive_val.is_null() || recursive_val == crate::sexp::globals::R_MissingArg() {
                // Append ScalarLogical(FALSE) as second arg
                let with_rec = Rf_cons(CAR(args), Rf_cons(Rf_ScalarLogical(FALSE), R_NilValue()));
                let _with_rec_guard = protect(with_rec);
                let result = Rf_ScalarLogical(if any_na_impl(call, op, with_rec, rho) {
                    1
                } else {
                    FALSE
                });
                result
            } else {
                Rf_ScalarLogical(if any_na_impl(call, op, args, rho) {
                    1
                } else {
                    FALSE
                })
            }
        }
    }
}
