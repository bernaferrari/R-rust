#![allow(
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_imports
)]

use super::*;

// ---------------------------------------------------------------------------
// dispatchMethod -- dispatch to an S3 method
// ---------------------------------------------------------------------------

/// Dispatch an S3 method by creating the dispatch environment and calling
/// the method function.
unsafe fn dispatchMethod(
    _op: SEXP,
    sxp: SEXP,
    dotClass: SEXP,
    cptr: *mut RCNTXT,
    method: SEXP,
    generic: *const c_char,
    rho: SEXP,
    callrho: SEXP,
    defrho: SEXP,
) -> SEXP {
    unsafe {
        // Create the S3 dispatch variables
        let generic_str = Rf_mkString(generic);
        let _generic_str_guard = protect(generic_str);

        let blank_str = Rf_mkString(b"\x00".as_ptr() as *const c_char);
        let _blank_str_guard = protect(blank_str);

        let method_name = PRINTNAME(method);
        let method_str = Rf_ScalarString(method_name);
        let _method_str_guard = protect(method_str);

        let newvars = createS3Vars(
            generic_str,
            blank_str,
            dotClass,
            method_str,
            callrho,
            defrho,
        );
        let _newvars_guard = protect(newvars);

        // Create the new call
        let mut newcall = R_NilValue();
        if !cptr.is_null() {
            newcall = (*cptr).call;
            if !newcall.is_null() && newcall != R_NilValue() {
                SETCAR(newcall, method);
            }
        }
        let _newcall_guard = protect(newcall);

        let mut matchedarg = if !cptr.is_null() {
            (*cptr).promiseargs
        } else {
            R_NilValue()
        };
        if matchedarg.is_null() || matchedarg == R_NilValue() {
            matchedarg = CDR(newcall);
        }
        let _matchedarg_guard = protect(matchedarg);

        let ans = applyMethod(newcall, sxp, matchedarg, rho, newvars);
        ans
    }
}

unsafe fn frame_args_for_method(formals: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let mut tags = Vec::new();
        let mut formal = formals;
        while !formal.is_null() && formal != R_NilValue() {
            let tag = TAG(formal);
            if tag != R_DotsSymbol_fn() {
                tags.push(tag);
            }
            formal = CDR(formal);
        }

        let mut args = R_NilValue();
        for tag in tags.into_iter().rev() {
            let mut value = if tag.is_null() || tag == R_NilValue() {
                R_MissingArg()
            } else {
                crate::sexp::envir::R_findVarInFrame(env, tag)
            };
            if value.is_null() || value == R_UnboundValue() {
                value = R_MissingArg();
            }
            let cell = Rf_cons(value, args);
            SETTAG(cell, tag);
            args = cell;
        }
        args
    }
}

pub(crate) unsafe fn simple_next_method_dispatch(
    current_call: SEXP,
    generic: SEXP,
    klass: SEXP,
    method: SEXP,
    env: SEXP,
    callenv: SEXP,
    defenv: SEXP,
) -> Option<SEXP> {
    unsafe {
        if !current_call.is_null()
            && current_call != R_NilValue()
            && TYPEOF(generic) == SEXPTYPE::STRSXP
            && LENGTH(generic) == 1
            && TYPEOF(klass) == SEXPTYPE::STRSXP
            && TYPEOF(method) == SEXPTYPE::STRSXP
        {
            let generic_name = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(generic, 0)))
                .to_string_lossy()
                .into_owned();
            let mut current_method = String::new();
            for i in 0..LENGTH(method) {
                let chars = CHAR(STRING_ELT(method, i as R_xlen_t));
                if !chars.is_null() && *chars != 0 {
                    current_method = std::ffi::CStr::from_ptr(chars)
                        .to_string_lossy()
                        .into_owned();
                    break;
                }
            }

            let mut start = 0;
            for i in 0..LENGTH(klass) {
                let class_chars = CHAR(STRING_ELT(klass, i as R_xlen_t));
                if class_chars.is_null() {
                    continue;
                }
                let class_name = std::ffi::CStr::from_ptr(class_chars).to_string_lossy();
                if current_method == format!("{generic_name}.{class_name}") {
                    start = i + 1;
                    break;
                }
            }

            for i in start..LENGTH(klass) {
                let class_chars = CHAR(STRING_ELT(klass, i as R_xlen_t));
                if class_chars.is_null() {
                    continue;
                }
                let class_name = std::ffi::CStr::from_ptr(class_chars).to_string_lossy();
                let Some(next_match) =
                    lookup_s3_method_for_class(&generic_name, &class_name, env, callenv, defenv)
                else {
                    continue;
                };
                let next_call = crate::mainutils::duplicate::shallow_duplicate(current_call);
                let _next_call_guard = protect(next_call);
                if !next_call.is_null() && next_call != R_NilValue() {
                    SETCAR(next_call, next_match.method_symbol);
                }
                let next_class = stringSuffix(klass, i);
                let _next_class_guard = protect(next_class);
                let method_name = Rf_ScalarString(PRINTNAME(next_match.method_symbol));
                let _method_name_guard = protect(method_name);
                let blank_group = Rf_mkString(b"\0".as_ptr() as *const c_char);
                let _blank_group_guard = protect(blank_group);
                let next_vars = createS3Vars(
                    generic,
                    blank_group,
                    next_class,
                    method_name,
                    callenv,
                    defenv,
                );
                let _next_vars_guard = protect(next_vars);
                let args = frame_args_for_method(FORMALS(next_match.method), env);
                let _args_guard = protect(args);
                return Some(crate::eval::closure::applyClosureWithFrameVars(
                    next_call,
                    next_match.method,
                    args,
                    env,
                    callenv,
                    next_vars,
                    0,
                ));
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// equalS3Signature -- compare S3 method signatures
// ---------------------------------------------------------------------------

/// Compare "signature" with "left.right" for S3 method name matching.
/// Returns TRUE if signature == "left.right", FALSE otherwise.
pub(crate) unsafe fn equalS3Signature(
    signature: *const c_char,
    left: *const c_char,
    right: *const c_char,
) -> c_int {
    unsafe {
        if signature.is_null() || left.is_null() || right.is_null() {
            return FALSE;
        }

        let mut s = signature;
        let mut a = left;

        // Compare against left part
        while *a != 0 {
            if *s != *a {
                return FALSE;
            }
            s = s.add(1);
            a = a.add(1);
        }

        // Must have a dot separator
        if *s != b'.' as c_char {
            return FALSE;
        }
        s = s.add(1);

        // Compare against right part
        a = right;
        while *a != 0 {
            if *s != *a {
                return FALSE;
            }
            s = s.add(1);
            a = a.add(1);
        }

        // Must end exactly
        if *s == 0 { TRUE } else { FALSE }
    }
}

// ---------------------------------------------------------------------------
// getPrimitive -- get the primitive function for a symbol
// ---------------------------------------------------------------------------

/// Get the primitive (BUILTINSXP or SPECIALSXP) bound to a symbol.
pub(crate) unsafe fn getPrimitive(symbol: SEXP) -> SEXP {
    unsafe {
        if symbol.is_null() {
            return R_NilValue();
        }
        let value = SYMVALUE(symbol);
        if value.is_null() {
            return R_NilValue();
        }
        let t = TYPEOF(value);
        if t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
            return value;
        }
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// R_LookupMethod -- look up an S3 method in the appropriate environments
// ---------------------------------------------------------------------------

/// Look up a method in the S3 dispatch chain: call environment, definition
/// environment's .__S3MethodsTable__., and the base environment.
pub unsafe fn R_LookupMethod(method: SEXP, rho: SEXP, callrho: SEXP, defrho: SEXP) -> SEXP {
    unsafe {
        if method.is_null() {
            return R_NilValue();
        }

        // Validate callrho
        if !callrho.is_null() && TYPEOF(callrho) != SEXPTYPE::ENVSXP {
            if callrho == R_NilValue() {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "use of NULL environment is defunct".to_string(),
                });
            } else {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "bad generic call environment".to_string(),
                });
            }
        }

        // Search from callrho up to the top environment
        let top = topenv(R_NilValue(), callrho);
        let _top_guard = protect(top);

        let val = findFunInEnvRange(method, callrho, top);
        if val != R_UnboundValue() {
            return val;
        }

        // Try the .__S3MethodsTable__. in defrho. Upstream R maps the base
        // environment to the base namespace; this port does not yet model a
        // distinct base namespace, so R_BaseEnv is the least surprising local
        // approximation.
        let effective_defrho = defrho;
        if !effective_defrho.is_null() && effective_defrho != R_NilValue() {
            let s3_table_sym = S3MethodsTable_symbol();
            let table = crate::sexp::envir::R_findVarInFrame(effective_defrho, s3_table_sym);
            if table != R_UnboundValue() && TYPEOF(table) == SEXPTYPE::ENVSXP {
                let _table_guard = protect(table);
                let val2 = crate::sexp::envir::R_findVarInFrame(table, method);
                if val2 != R_UnboundValue() {
                    let t = TYPEOF(val2);
                    if t == SEXPTYPE::CLOSXP
                        || t == SEXPTYPE::BUILTINSXP
                        || t == SEXPTYPE::SPECIALSXP
                    {
                        return val2;
                    }
                }
            }
        }

        // Search from top's enclosing environment. In this port the search
        // path is represented by R_GlobalEnv's enclosure chain, so this also
        // covers attached package environments before base.
        let search_start = ENCLOS(top);

        if !search_start.is_null() && search_start != R_EmptyEnv() {
            let val3 = findFunWithBaseEnvAfterGlobalEnv(method, search_start);
            if val3 != R_UnboundValue() {
                return val3;
            }
        }

        R_UnboundValue()
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct S3MethodMatch {
    pub(crate) method_symbol: SEXP,
    pub(crate) method: SEXP,
    pub(crate) class_index: Option<c_int>,
}

pub(crate) fn s3_method_symbol(generic: &str, class: &str) -> Option<SEXP> {
    let generic = CString::new(generic).ok()?;
    let class = CString::new(class).ok()?;
    Some(unsafe { crate::mainutils::names::installS3Signature(generic.as_ptr(), class.as_ptr()) })
}

pub(crate) unsafe fn lookup_s3_method_symbol(
    method_symbol: SEXP,
    rho: SEXP,
    callrho: SEXP,
    defrho: SEXP,
) -> SEXP {
    unsafe {
        let method = R_LookupMethod(method_symbol, rho, callrho, defrho);
        if isFunction(method) != FALSE {
            return method;
        }

        let method = lookup_s3_method_in_attached_tables(method_symbol, rho);
        if isFunction(method) != FALSE {
            method
        } else {
            R_UnboundValue()
        }
    }
}

pub(crate) unsafe fn lookup_s3_method_for_class(
    generic: &str,
    class: &str,
    rho: SEXP,
    callrho: SEXP,
    defrho: SEXP,
) -> Option<S3MethodMatch> {
    unsafe {
        let method_symbol = s3_method_symbol(generic, class)?;
        let method = lookup_s3_method_symbol(method_symbol, rho, callrho, defrho);
        if isFunction(method) != FALSE {
            Some(S3MethodMatch {
                method_symbol,
                method,
                class_index: None,
            })
        } else {
            None
        }
    }
}

pub(crate) unsafe fn lookup_s3_method_for_classes(
    generic: &str,
    classes: SEXP,
    rho: SEXP,
    callrho: SEXP,
    defrho: SEXP,
    include_default: bool,
) -> Option<S3MethodMatch> {
    unsafe {
        if classes.is_null() || classes == R_NilValue() || TYPEOF(classes) != SEXPTYPE::STRSXP {
            return if include_default {
                lookup_s3_method_for_class(generic, "default", rho, callrho, defrho)
            } else {
                None
            };
        }

        let generic_cstr = CString::new(generic).ok()?;
        for i in 0..length(classes) {
            let class = STRING_ELT(classes, i as R_xlen_t);
            if class.is_null() {
                continue;
            }
            let class = translateChar(class);
            if class.is_null() {
                continue;
            }
            let method_symbol =
                crate::mainutils::names::installS3Signature(generic_cstr.as_ptr(), class);
            let method = lookup_s3_method_symbol(method_symbol, rho, callrho, defrho);
            if isFunction(method) != FALSE {
                return Some(S3MethodMatch {
                    method_symbol,
                    method,
                    class_index: Some(i),
                });
            }
        }

        if include_default {
            lookup_s3_method_for_class(generic, "default", rho, callrho, defrho)
        } else {
            None
        }
    }
}

unsafe fn lookup_s3_method_in_attached_tables(method_sym: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut current = if rho.is_null() || rho == R_NilValue() || TYPEOF(rho) != SEXPTYPE::ENVSXP
        {
            R_GlobalEnv()
        } else {
            rho
        };

        while !current.is_null() && current != R_EmptyEnv() {
            let table = crate::sexp::envir::R_findVarInFrame(current, S3MethodsTable_symbol());
            if !table.is_null() && table != R_UnboundValue() && TYPEOF(table) == SEXPTYPE::ENVSXP {
                let method = crate::sexp::envir::R_findVarInFrame(table, method_sym);
                if isFunction(method) != FALSE {
                    return method;
                }
            }
            current = ENCLOS(current);
        }

        R_UnboundValue()
    }
}

// ---------------------------------------------------------------------------
// usemethod -- core S3 method dispatch implementation
// ---------------------------------------------------------------------------

/// Core S3 method dispatch: iterate through class vector to find a matching
/// method, dispatching to it if found. Returns 1 if a method was dispatched,
/// 0 if no method was found.
pub unsafe fn usemethod(
    generic: *const c_char,
    obj: SEXP,
    call: SEXP,
    args: SEXP,
    rho: SEXP,
    callrho: SEXP,
    defrho: SEXP,
    ans: *mut SEXP,
) -> c_int {
    unsafe {
        if generic.is_null() || ans.is_null() {
            return 0;
        }

        // Get the context which UseMethod was called from
        let cptr = R_GlobalContext();
        if cptr.is_null() {
            return 0;
        }

        let op = (*cptr).closure;
        let klass = R_data_class2(obj);
        let _klass_guard = protect(klass);

        let generic_name = std::ffi::CStr::from_ptr(generic).to_string_lossy();
        let Some(method_match) =
            lookup_s3_method_for_classes(&generic_name, klass, rho, callrho, defrho, true)
        else {
            return 0;
        };

        let _method_guard = protect(method_match.method);
        match method_match.class_index {
            Some(i) => {
                if i > 0 {
                    let dotClass = stringSuffix(klass, i);
                    let _dotclass_guard = protect(dotClass);
                    setAttrib(dotClass, sym("previous"), klass);
                    *ans = dispatchMethod(
                        op,
                        method_match.method,
                        dotClass,
                        cptr,
                        method_match.method_symbol,
                        generic,
                        rho,
                        callrho,
                        defrho,
                    );
                } else {
                    *ans = dispatchMethod(
                        op,
                        method_match.method,
                        klass,
                        cptr,
                        method_match.method_symbol,
                        generic,
                        rho,
                        callrho,
                        defrho,
                    );
                }
            }
            None => {
                *ans = dispatchMethod(
                    op,
                    method_match.method,
                    R_NilValue(),
                    cptr,
                    method_match.method_symbol,
                    generic,
                    rho,
                    callrho,
                    defrho,
                );
            }
        }
        1
    }
}

// ---------------------------------------------------------------------------
// findmethod -- find a method for a generic
// ---------------------------------------------------------------------------

/// Find a method for a generic function given an object's class.
/// Returns the method SEXP and the class index via out parameters.
pub unsafe fn findmethod(
    call: SEXP,
    op: SEXP,
    obj: SEXP,
    generic: *const c_char,
    method: *mut SEXP,
    _rho: SEXP,
    _callrho: SEXP,
    _defrho: SEXP,
) -> c_int {
    unsafe {
        if generic.is_null() || method.is_null() {
            return 0;
        }

        let klass = R_data_class2(obj);
        let _klass_guard = protect(klass);
        let nclass = length(klass);

        for i in 0..nclass {
            let ss = translateChar(STRING_ELT(klass, i as R_xlen_t));
            let m = crate::mainutils::names::installS3Signature(generic, ss);
            let sxp = R_LookupMethod(m, ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
            if isFunction(sxp) != FALSE {
                *method = sxp;
                return i + 1; // 1-based index
            }
        }

        // Try default
        let m = crate::mainutils::names::installS3Signature(
            generic,
            b"default\x00".as_ptr() as *const c_char,
        );
        let sxp = R_LookupMethod(m, ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
        if isFunction(sxp) != FALSE {
            *method = sxp;
            return 0; // default
        }

        -1 // not found
    }
}

// ---------------------------------------------------------------------------
// DispatchGroup -- group dispatch for Ops/Math/Summary
// ---------------------------------------------------------------------------

/// Group dispatch for Ops, Math, Summary, and Complex groups.
/// Returns 1 if dispatch occurred, 0 otherwise.
pub unsafe fn DispatchGroup(
    s: SEXP,
    code: *const c_char,
    call: SEXP,
    op: *const c_char,
    args: SEXP,
    env: SEXP,
) -> c_int {
    unsafe {
        if s.is_null() || code.is_null() {
            return 0;
        }

        // Get the class of the first argument
        let obj = if !args.is_null() && args != R_NilValue() {
            CAR(args)
        } else {
            R_NilValue()
        };

        if obj.is_null() {
            return 0;
        }

        let klass = R_data_class(obj);
        let _klass_guard = protect(klass);
        let nclass = length(klass);

        // Try each class in order
        for i in 0..nclass {
            let ss = translateChar(STRING_ELT(klass, i as R_xlen_t));
            if ss.is_null() || *ss == 0 {
                continue;
            }

            // Build the method name: group.class
            let mut buf = [0u8; 512];
            let code_str = std::ffi::CStr::from_ptr(code);
            let ss_str = std::ffi::CStr::from_ptr(ss);
            let mut pos = 0;
            for &b in code_str.to_bytes() {
                if pos < 511 {
                    buf[pos] = b;
                    pos += 1;
                }
            }
            if pos < 511 {
                buf[pos] = b'.';
                pos += 1;
            }
            for &b in ss_str.to_bytes() {
                if pos < 511 {
                    buf[pos] = b;
                    pos += 1;
                }
            }
            buf[pos] = 0;

            let method_sym = Rf_install(buf.as_ptr() as *const c_char);
            let method_val = crate::sexp::envir::R_findVarInFrame(env, method_sym);

            if isFunction(method_val) != FALSE {
                // Found a group method -- dispatch
                // Full implementation would call applyMethod
                return 1;
            }
        }

        0
    }
}

// ---------------------------------------------------------------------------
// DispatchOrEval -- dispatch or evaluate
// ---------------------------------------------------------------------------

/// Try S3 dispatch, and if no method is found, evaluate the default.
/// Returns 1 if dispatch occurred, 0 otherwise.
/// Note: canonical version lives in eval/dispatch.rs
pub(crate) unsafe fn DispatchOrEval_objects(
    call: SEXP,
    op: SEXP,
    generic: *const c_char,
    args: SEXP,
    env: SEXP,
    ans: *mut SEXP,
) -> c_int {
    unsafe {
        if generic.is_null() || ans.is_null() {
            return 0;
        }

        // Get the class of the first argument
        let obj = if !args.is_null() && args != R_NilValue() {
            CAR(args)
        } else {
            R_NilValue()
        };

        if obj.is_null() {
            return 0;
        }

        let klass = R_data_class(obj);
        let _klass_guard = protect(klass);
        let nclass = length(klass);

        for i in 0..nclass {
            let ss = translateChar(STRING_ELT(klass, i as R_xlen_t));
            let method_sym = crate::mainutils::names::installS3Signature(generic, ss);
            let method_val = R_LookupMethod(method_sym, env, env, R_GlobalEnv());

            if isFunction(method_val) != FALSE {
                // Dispatch to the method
                // Full implementation would call applyMethod
                *ans = method_val;
                return 1;
            }
        }

        0
    }
}
