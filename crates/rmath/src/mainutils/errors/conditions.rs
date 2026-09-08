#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Condition handling: handler entries, condition signaling, condition
//! constructors (R_make*/R_signal*), tryCatch support, and initialization.

use super::helpers::translateChar;
use super::*;

// ---------------------------------------------------------------------------
// Condition handling infrastructure
// ---------------------------------------------------------------------------

/// Handler entry structure (mirrors R's mkHandlerEntry).
pub fn mkHandlerEntry(
    klass: SEXP,
    parentenv: SEXP,
    handler: SEXP,
    target: SEXP,
    result: SEXP,
    calling: c_int,
) -> SEXP {
    unsafe {
        let entry = Rf_allocVector(SEXPTYPE::VECSXP, 5);
        if !entry.is_null() {
            SET_VECTOR_ELT(entry, 0, klass);
            SET_VECTOR_ELT(entry, 1, parentenv);
            SET_VECTOR_ELT(entry, 2, handler);
            SET_VECTOR_ELT(entry, 3, target);
            SET_VECTOR_ELT(entry, 4, result);
            SETLEVELS(entry, calling);
        }
        entry
    }
}

/// IS_CALLING_ENTRY macro.
#[inline]
pub unsafe fn IS_CALLING_ENTRY(e: SEXP) -> c_int {
    unsafe { LEVELS(e) }
}

/// ENTRY_CLASS macro.
#[inline]
pub unsafe fn ENTRY_CLASS(e: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(e, 0) }
}

/// ENTRY_HANDLER macro.
#[inline]
pub unsafe fn ENTRY_HANDLER(e: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(e, 2) }
}

/// ENTRY_TARGET_ENVIR macro.
#[inline]
pub unsafe fn ENTRY_TARGET_ENVIR(e: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(e, 3) }
}

/// ENTRY_RETURN_RESULT macro.
#[inline]
pub unsafe fn ENTRY_RETURN_RESULT(e: SEXP) -> SEXP {
    unsafe { VECTOR_ELT(e, 4) }
}

/// CLEAR_ENTRY_CALLING_ENVIR macro.
#[inline]
pub unsafe fn CLEAR_ENTRY_CALLING_ENVIR(e: SEXP) {
    unsafe {
        SET_VECTOR_ELT(e, 1, globals::R_NilValue());
    }
}

/// CLEAR_ENTRY_TARGET_ENVIR macro.
#[inline]
pub unsafe fn CLEAR_ENTRY_TARGET_ENVIR(e: SEXP) {
    unsafe {
        SET_VECTOR_ELT(e, 3, globals::R_NilValue());
    }
}

/// RESULT_SIZE for handler results.
pub const RESULT_SIZE: usize = 4;

/// do_addCondHands — add condition handlers to the stack.
pub unsafe fn do_addCondHands(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let classes = CAR(args);
        let mut rest = CDR(args);
        let handlers = CAR(rest);
        rest = CDR(rest);
        let parentenv = CAR(rest);
        rest = CDR(rest);
        let target = CAR(rest);
        rest = CDR(rest);
        let calling = asLogical(CAR(rest));

        if isNull(classes) != 0 || isNull(handlers) != 0 {
            return handler_stack();
        }

        let n = LENGTH(handlers);

        {
            let oldstack = handler_stack();

            let result = Rf_allocVector(SEXPTYPE::VECSXP, RESULT_SIZE as c_int);
            let mut newstack = oldstack;

            for i in (0..n).rev() {
                let klass = STRING_ELT(classes, i as R_xlen_t);
                let handler = VECTOR_ELT(handlers, i as R_xlen_t);
                let entry = mkHandlerEntry(klass, parentenv, handler, target, result, calling);
                newstack = Rf_cons(entry, newstack);
            }

            set_handler_stack(newstack);
            oldstack
        }
    }
}

/// do_resetCondHands — reset condition handlers to a previous state.
pub unsafe fn do_resetCondHands(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let old = CAR(args);
        set_handler_stack(old);
        globals::R_NilValue()
    }
}

// ---------------------------------------------------------------------------

// Condition signaling
// ---------------------------------------------------------------------------

unsafe fn findConditionHandler(cond: SEXP) -> SEXP {
    unsafe {
        let classes = getAttrib(cond, R_ClassSymbol());
        if TYPEOF(classes) != SEXPTYPE::STRSXP {
            return globals::R_NilValue();
        }
        let n_classes = LENGTH(classes);
        let mut list = handler_stack();
        while !list.is_null() && list != globals::R_NilValue() {
            let entry = CAR(list);
            let entry_class = ENTRY_CLASS(entry);
            if !entry_class.is_null() {
                let entry_bytes = CHAR(entry_class);
                if !entry_bytes.is_null() {
                    let entry_str = CStr::from_ptr(entry_bytes).to_bytes();
                    for i in 0..n_classes {
                        let cls = STRING_ELT(classes, i as R_xlen_t);
                        if !cls.is_null() {
                            let cls_bytes = CHAR(cls);
                            if !cls_bytes.is_null() {
                                let cls_str = CStr::from_ptr(cls_bytes).to_bytes();
                                if entry_str == cls_str {
                                    return list;
                                }
                            }
                        }
                    }
                }
            }
            list = CDR(list);
        }
        globals::R_NilValue()
    }
}

/// do_signalCondition — signal a condition through the handler stack.
pub unsafe fn do_signalCondition(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let cond = CAR(args);
        let msg = CADR(args);
        let ecall = CADDR(args);

        let oldstack = handler_stack();
        let _oldstack_guard = protect(oldstack);

        let mut list = findConditionHandler(cond);
        while !list.is_null() && list != globals::R_NilValue() {
            let entry = CAR(list);
            set_handler_stack(CDR(list));
            if IS_CALLING_ENTRY(entry) != 0 {
                let h = ENTRY_HANDLER(entry);
                if h == globals::R_RestartToken() {
                    let msgstr = if TYPEOF(msg) == SEXPTYPE::STRSXP && LENGTH(msg) > 0 {
                        let c = translateChar(STRING_ELT(msg, 0));
                        CStr::from_ptr(c).to_str().unwrap_or("error")
                    } else {
                        "error message not a string"
                    };
                    let cmsg = std::ffi::CString::new(msgstr).unwrap_or_default();
                    verrorcall_dflt(ecall, cmsg.as_ptr(), ptr::null_mut());
                } else {
                    let hcall = Rf_lang2(h, cond);
                    let _hcall_guard = protect(hcall);
                    let _ = crate::eval::eval::Rf_eval(hcall, globals::R_GlobalEnv());
                }
            } else {
                gotoExitingHandler(cond, ecall, entry);
            }
            list = findConditionHandler(cond);
        }

        set_handler_stack(oldstack);
        globals::R_NilValue()
    }
}

/// do_dfltWarn — default warning handler.
pub unsafe fn do_dfltWarn(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        if TYPEOF(CAR(args)) != SEXPTYPE::STRSXP || LENGTH(CAR(args)) != 1 {
            errorcall(call, b"bad error message\x00".as_ptr() as *const c_char);
        }
        let msg = translateChar(STRING_ELT(CAR(args), 0));
        let ecall = CADR(args);
        vwarningcall_dflt(ecall, msg, ptr::null_mut());
        globals::R_NilValue()
    }
}

/// do_dfltStop — default error handler.
pub unsafe fn do_dfltStop(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        if TYPEOF(CAR(args)) != SEXPTYPE::STRSXP || LENGTH(CAR(args)) != 1 {
            errorcall(call, b"bad error message\x00".as_ptr() as *const c_char);
        }
        let msg = translateChar(STRING_ELT(CAR(args), 0));
        let message = CStr::from_ptr(msg).to_str().unwrap_or("").to_string();
        errorcall_str(globals::R_NilValue(), &message)
    }
}

// ---------------------------------------------------------------------------
// Condition creation helpers
// ---------------------------------------------------------------------------

/// R_makeErrorCondition — create an error condition object.
pub unsafe fn R_makeErrorCondition(
    call: SEXP,
    classname: *const c_char,
    subclassname: *const c_char,
    nextra: c_int,
    format: *const c_char,
) -> SEXP {
    unsafe {
        let class = if classname.is_null() {
            ""
        } else {
            CStr::from_ptr(classname).to_str().unwrap_or("")
        };
        let sub = if subclassname.is_null() {
            ""
        } else {
            CStr::from_ptr(subclassname).to_str().unwrap_or("")
        };
        let fmt = if format.is_null() {
            ""
        } else {
            CStr::from_ptr(format).to_str().unwrap_or("")
        };

        let nelem = nextra + 2;
        let cond = Rf_allocVector(SEXPTYPE::VECSXP, nelem);

        // Element 0: message
        SET_VECTOR_ELT(cond, 0, Rf_mkString(fmt.as_ptr() as *const c_char));
        // Element 1: call
        SET_VECTOR_ELT(cond, 1, call);

        // Names attribute
        let names = Rf_allocVector(SEXPTYPE::STRSXP, nelem);
        setAttrib_wrap(cond, R_NamesSymbol(), names);
        SET_STRING_ELT(
            names,
            0,
            Rf_mkChar(b"message\x00".as_ptr() as *const c_char),
        );
        SET_STRING_ELT(names, 1, Rf_mkChar(b"call\x00".as_ptr() as *const c_char));

        // Class attribute
        let nclass = if sub.is_empty() { 3 } else { 4 };
        let klass = Rf_allocVector(SEXPTYPE::STRSXP, nclass);
        setAttrib_wrap(cond, R_ClassSymbol(), klass);

        if sub.is_empty() {
            SET_STRING_ELT(klass, 0, Rf_mkChar(class.as_ptr() as *const c_char));
            SET_STRING_ELT(klass, 1, Rf_mkChar(b"error\x00".as_ptr() as *const c_char));
            SET_STRING_ELT(
                klass,
                2,
                Rf_mkChar(b"condition\x00".as_ptr() as *const c_char),
            );
        } else {
            SET_STRING_ELT(klass, 0, Rf_mkChar(sub.as_ptr() as *const c_char));
            SET_STRING_ELT(klass, 1, Rf_mkChar(class.as_ptr() as *const c_char));
            SET_STRING_ELT(klass, 2, Rf_mkChar(b"error\x00".as_ptr() as *const c_char));
            SET_STRING_ELT(
                klass,
                3,
                Rf_mkChar(b"condition\x00".as_ptr() as *const c_char),
            );
        }

        cond
    }
}

/// R_signalErrorCondition — signal an error condition.
pub unsafe fn R_signalErrorCondition(cond: SEXP, call: SEXP) {
    unsafe {
        // Extract message from condition and call errorcall_dflt
        if TYPEOF(cond) != SEXPTYPE::VECSXP || LENGTH(cond) == 0 {
            errorcall(
                call,
                b"condition object must be a VECSXP of length at least one\x00".as_ptr()
                    as *const c_char,
            );
        }
        let elt = VECTOR_ELT(cond, 0);
        if TYPEOF(elt) != SEXPTYPE::STRSXP || LENGTH(elt) != 1 {
            errorcall(
                call,
                b"first element of condition object must be a scalar string\x00".as_ptr()
                    as *const c_char,
            );
        }
        let msg = translateChar(STRING_ELT(elt, 0));
        errorcall(call, msg);
    }
}

/// R_signalErrorConditionEx — signal an error condition with exitOnly flag.
pub unsafe fn R_signalErrorConditionEx(cond: SEXP, call: SEXP, exitOnly: c_int) {
    unsafe {
        R_signalErrorCondition(cond, call);
    }
}

/// R_setConditionField — set a field in a condition object.
pub unsafe fn R_setConditionField(cond: SEXP, idx: R_xlen_t, name: *const c_char, val: SEXP) {
    unsafe {
        if TYPEOF(cond) != SEXPTYPE::VECSXP {
            return;
        }
        let len = XLENGTH(cond);
        if idx < 0 || idx >= len {
            return;
        }
        SET_VECTOR_ELT(cond, idx, val);
        let names = getAttrib_wrap(cond, R_NamesSymbol());
        if !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP && XLENGTH(names) == len {
            SET_STRING_ELT(names, idx, Rf_mkChar(name));
        }
    }
}

// ---------------------------------------------------------------------------
// tryCatch support (simplified)
// ---------------------------------------------------------------------------

/// R_tryCatchError — C-level tryCatch for error conditions.
pub unsafe fn R_tryCatchError(
    body: Option<unsafe extern "C" fn(*mut c_void) -> SEXP>,
    bdata: *mut c_void,
    handler: Option<unsafe extern "C" fn(SEXP, *mut c_void) -> SEXP>,
    hdata: *mut c_void,
) -> SEXP {
    unsafe {
        let klass = Rf_mkChar(b"error\0".as_ptr() as *const c_char);
        let _klass_guard = protect(klass);
        let handler_fn = if handler.is_some() {
            Rf_mkString(b"tryCatchError\0".as_ptr() as *const c_char)
        } else {
            globals::R_NilValue()
        };
        let entry = mkHandlerEntry(
            klass,
            globals::R_GlobalEnv(),
            handler_fn,
            globals::R_NilValue(),
            globals::R_NilValue(),
            0,
        );
        let _entry_guard = protect(entry);

        let old_stack = handler_stack();
        let _old_stack_guard = protect(old_stack);
        let new_top = Rf_cons(entry, old_stack);
        set_handler_stack(new_top);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Some(f) = body {
                f(bdata)
            } else {
                globals::R_NilValue()
            }
        }));

        set_handler_stack(old_stack);

        match result {
            Ok(val) => val,
            Err(payload) => {
                // Check for ExitingHandler signal — targeted context jump.
                // If our context stack has the target environment, consume the signal
                // and return the result vector. Otherwise re-panic to continue unwinding.
                if let Some(signal) = payload.downcast_ref::<crate::sexp::context::RSignal>() {
                    match signal {
                        crate::sexp::context::RSignal::ExitingHandler { target_env, result } => {
                            let target = *target_env;
                            let res = *result;
                            if crate::sexp::context::context_env_exists(target) {
                                res
                            } else {
                                std::panic::panic_any(
                                    crate::sexp::context::RSignal::ExitingHandler {
                                        target_env: target,
                                        result: res,
                                    },
                                );
                            }
                        }
                        _ => {
                            if handler.is_some() {
                                let cond = crate::sexp::constructors::Rf_allocVector(
                                    SEXPTYPE::STRSXP.as_c_int(),
                                    1,
                                );
                                if !cond.is_null() {
                                    let msg = Rf_mkString(b"error\0".as_ptr() as *const c_char);
                                    crate::sexp::accessors::SET_STRING_ELT(cond, 0, msg);
                                }
                                if let Some(h) = handler {
                                    h(cond, hdata)
                                } else {
                                    globals::R_NilValue()
                                }
                            } else {
                                std::panic::resume_unwind(payload)
                            }
                        }
                    }
                } else if handler.is_some() {
                    let cond = crate::sexp::constructors::Rf_allocVector(SEXPTYPE::STRSXP, 1);
                    if !cond.is_null() {
                        let msg = Rf_mkString(b"error\0".as_ptr() as *const c_char);
                        crate::sexp::accessors::SET_STRING_ELT(cond, 0, msg);
                    }
                    if let Some(h) = handler {
                        h(cond, hdata)
                    } else {
                        globals::R_NilValue()
                    }
                } else {
                    std::panic::resume_unwind(payload)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// R_InitConditions — initialize error/warning condition objects.
pub unsafe fn R_InitConditions() {
    unsafe {
        // Create and preserve condition objects for stack overflow errors
        let protect_so = R_makeErrorCondition(
            globals::R_NilValue(),
            b"stackOverflowError\x00".as_ptr() as *const c_char,
            b"protectStackOverflowError\x00".as_ptr() as *const c_char,
            0,
            b"protect(): protection stack overflow\x00".as_ptr() as *const c_char,
        );
        crate::sexp::protect::R_PreserveObject(protect_so);

        let expr_so = R_makeErrorCondition(
            globals::R_NilValue(),
            b"stackOverflowError\x00".as_ptr() as *const c_char,
            b"expressionStackOverflowError\x00".as_ptr() as *const c_char,
            0,
            b"evaluation nested too deeply: infinite recursion / options(expressions=)?\x00"
                .as_ptr() as *const c_char,
        );
        crate::sexp::protect::R_PreserveObject(expr_so);

        let node_so = R_makeErrorCondition(
            globals::R_NilValue(),
            b"stackOverflowError\x00".as_ptr() as *const c_char,
            b"nodeStackOverflowError\x00".as_ptr() as *const c_char,
            0,
            b"node stack overflow\x00".as_ptr() as *const c_char,
        );
        crate::sexp::protect::R_PreserveObject(node_so);
    }
}

/// R_MissingArgError_c — report a missing argument error.
/// Matches C's `void R_MissingArgError_c(const char* arg, SEXP call, const char* subclass)`
pub unsafe fn R_MissingArgError_c(arg: *const c_char, call: SEXP, subclass: *const c_char) {
    unsafe {
        let arg_str = if arg.is_null() {
            ""
        } else {
            CStr::from_ptr(arg).to_str().unwrap_or("")
        };
        let _call_guard = protect(call);
        let msg = if !arg_str.is_empty() {
            format!("argument \"{}\" is missing, with no default", arg_str)
        } else {
            "argument is missing, with no default".to_string()
        };
        let c_msg = std::ffi::CString::new(msg.clone()).unwrap_or_default();
        let cond = R_makeErrorCondition(
            call,
            b"missingArgError\0".as_ptr() as *const c_char,
            subclass,
            0,
            c_msg.as_ptr(),
        );
        let _cond_guard = protect(cond);
        R_signalErrorCondition(cond, call);
    }
}

/// R_MissingArgError — report a missing argument error from a symbol.
/// Matches C's `void R_MissingArgError(SEXP symbol, SEXP call, const char* subclass)`
pub unsafe fn R_MissingArgError(symbol: SEXP, call: SEXP, subclass: *const c_char) {
    unsafe {
        let arg = if symbol.is_null() || TYPEOF(symbol) != SEXPTYPE::SYMSXP {
            b"\0".as_ptr() as *const c_char
        } else {
            let name = CHAR_local(PRINTNAME(symbol));
            if name.is_null() {
                b"\0".as_ptr() as *const c_char
            } else {
                name
            }
        };
        R_MissingArgError_c(arg, call, subclass);
    }
}

/// R_signalWarningCondition — signal a warning condition object.
/// Matches C's `void R_signalWarningCondition(SEXP cond)`.
pub unsafe fn R_signalWarningCondition(cond: SEXP) {
    unsafe {
        if cond.is_null() || TYPEOF(cond) != SEXPTYPE::VECSXP || LENGTH(cond) < 1 {
            return;
        }
        let elt = VECTOR_ELT(cond, 0);
        if TYPEOF(elt) != SEXPTYPE::STRSXP || LENGTH(elt) != 1 {
            return;
        }
        let msg = translateChar(STRING_ELT(elt, 0));
        let call = if LENGTH(cond) > 1 {
            VECTOR_ELT(cond, 1)
        } else {
            ptr::null_mut()
        };
        warningcall(call, msg);
    }
}

/// Apply the default warning policy to an already-signaled condition.
///
/// This is the post-handler half of `R_signalWarningCondition`: callers that
/// preserve a concrete warning subclass signal it first, then collect/print it
/// here only when no handler muffled the warning.
pub(crate) unsafe fn warning_condition_default(cond: SEXP) {
    unsafe {
        if cond.is_null() || TYPEOF(cond) != SEXPTYPE::VECSXP || LENGTH(cond) < 1 {
            return;
        }
        let elt = VECTOR_ELT(cond, 0);
        if TYPEOF(elt) != SEXPTYPE::STRSXP || LENGTH(elt) != 1 {
            return;
        }
        let msg = translateChar(STRING_ELT(elt, 0));
        let call = if LENGTH(cond) > 1 {
            VECTOR_ELT(cond, 1)
        } else {
            ptr::null_mut()
        };
        vwarningcall_dflt(call, msg, ptr::null_mut());
    }
}

/// R_makeWarningCondition — create a warning condition object.
/// Matches C's `SEXP R_makeWarningCondition(SEXP call, const char *classname,
/// const char *subclassname, int nextra, const char *format, ...)`
pub unsafe fn R_makeWarningCondition(
    call: SEXP,
    classname: *const c_char,
    subclassname: *const c_char,
    nextra: c_int,
    format: *const c_char,
) -> SEXP {
    unsafe {
        let class = if classname.is_null() {
            "simpleWarning"
        } else {
            CStr::from_ptr(classname)
                .to_str()
                .unwrap_or("simpleWarning")
        };
        let sub = if subclassname.is_null() {
            ""
        } else {
            CStr::from_ptr(subclassname).to_str().unwrap_or("")
        };
        let fmt = if format.is_null() {
            ""
        } else {
            CStr::from_ptr(format).to_str().unwrap_or("")
        };

        let nelem = nextra + 2;
        let cond = Rf_allocVector(SEXPTYPE::VECSXP, nelem);

        // Element 0: message
        SET_VECTOR_ELT(cond, 0, Rf_mkString(fmt.as_ptr() as *const c_char));
        // Element 1: call
        SET_VECTOR_ELT(
            cond,
            1,
            if call.is_null() {
                globals::R_NilValue()
            } else {
                call
            },
        );

        // Names attribute
        let names = Rf_allocVector(SEXPTYPE::STRSXP, nelem);
        setAttrib_wrap(cond, R_NamesSymbol(), names);
        SET_STRING_ELT(names, 0, Rf_mkChar(b"message\0".as_ptr() as *const c_char));
        SET_STRING_ELT(names, 1, Rf_mkChar(b"call\0".as_ptr() as *const c_char));

        // Class attribute: with a subclass,
        // [subclass, class, "warning", "condition"]; without,
        // [class, "warning", "condition"].
        let nclass = if sub.is_empty() { 3 } else { 4 };
        let klass = Rf_allocVector(SEXPTYPE::STRSXP, nclass);
        setAttrib_wrap(cond, R_ClassSymbol(), klass);

        if sub.is_empty() {
            SET_STRING_ELT(klass, 0, Rf_mkChar(class.as_ptr() as *const c_char));
            SET_STRING_ELT(klass, 1, Rf_mkChar(b"warning\0".as_ptr() as *const c_char));
            SET_STRING_ELT(
                klass,
                2,
                Rf_mkChar(b"condition\0".as_ptr() as *const c_char),
            );
        } else {
            SET_STRING_ELT(klass, 0, Rf_mkChar(sub.as_ptr() as *const c_char));
            SET_STRING_ELT(klass, 1, Rf_mkChar(class.as_ptr() as *const c_char));
            SET_STRING_ELT(klass, 2, Rf_mkChar(b"warning\0".as_ptr() as *const c_char));
            SET_STRING_ELT(
                klass,
                3,
                Rf_mkChar(b"condition\0".as_ptr() as *const c_char),
            );
        }

        cond
    }
}

/// Message text for a partial-match warning operand: PRINTNAME for a
/// symbol, translateChar for a CHARSXP string (upstream errors.c passes
/// these straight into the format string).
unsafe fn partial_match_text(x: SEXP) -> String {
    unsafe {
        if !x.is_null() && TYPEOF(x) == SEXPTYPE::SYMSXP {
            CStr::from_ptr(CHAR_local(PRINTNAME(x)))
                .to_string_lossy()
                .into_owned()
        } else if !x.is_null() {
            CStr::from_ptr(translateChar(x))
                .to_string_lossy()
                .into_owned()
        } else {
            "?".to_string()
        }
    }
}

/// R_makePartialMatchWarningCondition — create a partial match warning condition.
/// Matches C's `SEXP R_makePartialMatchWarningCondition(SEXP call, SEXP input, SEXP target)`
/// where input/target are symbols or CHARSXP strings.
pub unsafe fn R_makePartialMatchWarningCondition(call: SEXP, input: SEXP, target: SEXP) -> SEXP {
    unsafe {
        let msg = format!(
            "partial match of '{}' to '{}'",
            partial_match_text(input),
            partial_match_text(target),
        );
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();

        let cond = R_makeWarningCondition(
            call,
            b"partialMatchWarning\0".as_ptr() as *const c_char,
            ptr::null(),
            2,
            c_msg.as_ptr(),
        );
        let _cond_guard = protect(cond);
        R_setConditionField(
            cond,
            2,
            b"input\0".as_ptr() as *const c_char,
            if !input.is_null() && TYPEOF(input) == SEXPTYPE::SYMSXP {
                input
            } else {
                Rf_ScalarString(input)
            },
        );
        R_setConditionField(
            cond,
            3,
            b"target\0".as_ptr() as *const c_char,
            if !target.is_null() && TYPEOF(target) == SEXPTYPE::SYMSXP {
                target
            } else {
                Rf_ScalarString(target)
            },
        );
        // ideally we would want the function/object in a field also
        cond
    }
}

/// R_makePartialArgumentMatchWarningCondition — create a partial argument
/// match warning condition (supplied argument tag vs function formal).
/// Matches C's `SEXP R_makePartialArgumentMatchWarningCondition(SEXP call,
/// SEXP argument, SEXP formal)`
pub unsafe fn R_makePartialArgumentMatchWarningCondition(
    call: SEXP,
    argument: SEXP,
    formal: SEXP,
) -> SEXP {
    unsafe {
        let msg = format!(
            "partial argument match of '{}' to '{}'",
            partial_match_text(argument),
            partial_match_text(formal),
        );
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();

        let cond = R_makeWarningCondition(
            call,
            b"partialMatchWarning\0".as_ptr() as *const c_char,
            b"partialArgumentMatchWarning\0".as_ptr() as *const c_char,
            2,
            c_msg.as_ptr(),
        );
        let _cond_guard = protect(cond);
        R_setConditionField(cond, 2, b"argument\0".as_ptr() as *const c_char, argument);
        R_setConditionField(cond, 3, b"formal\0".as_ptr() as *const c_char, formal);
        // ideally we would want the function/object in a field also
        cond
    }
}

/// R_makeNotSubsettableError — create a "not subsettable" error condition.
/// Matches C's `SEXP R_makeNotSubsettableError(SEXP x, SEXP call)`
pub unsafe fn R_makeNotSubsettableError(x: SEXP, call: SEXP) -> SEXP {
    unsafe {
        let class_str = if !x.is_null() {
            let klass = getAttrib_wrap(x, R_ClassSymbol());
            if !klass.is_null() && TYPEOF(klass) == SEXPTYPE::STRSXP && LENGTH(klass) >= 1 {
                let s = CHAR_local(STRING_ELT(klass, 0));
                CStr::from_ptr(s).to_str().unwrap_or("object")
            } else {
                "object"
            }
        } else {
            "object"
        };
        let msg = format!("object of type '{}' is not subsettable", class_str);
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();

        R_makeErrorCondition(
            call,
            b"simpleError\0".as_ptr() as *const c_char,
            b"notSubsettableError\0".as_ptr() as *const c_char,
            0,
            c_msg.as_ptr(),
        )
    }
}

/// R_makeMissingSubscriptError — create a missing subscript error condition.
/// Matches C's `SEXP R_makeMissingSubscriptError(SEXP x, SEXP call)`
pub unsafe fn R_makeMissingSubscriptError(x: SEXP, call: SEXP) -> SEXP {
    unsafe {
        let class_str = if !x.is_null() {
            let klass = getAttrib_wrap(x, R_ClassSymbol());
            if !klass.is_null() && TYPEOF(klass) == SEXPTYPE::STRSXP && LENGTH(klass) >= 1 {
                let s = CHAR_local(STRING_ELT(klass, 0));
                CStr::from_ptr(s).to_str().unwrap_or("object")
            } else {
                "object"
            }
        } else {
            "object"
        };
        let msg = format!("subscript out of bounds for {}", class_str);
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();

        R_makeErrorCondition(
            call,
            b"simpleError\0".as_ptr() as *const c_char,
            b"missingSubscriptError\0".as_ptr() as *const c_char,
            0,
            c_msg.as_ptr(),
        )
    }
}

/// R_makeMissingSubscriptError1 — create a missing subscript error condition (no x).
/// Matches C's `SEXP R_makeMissingSubscriptError1(SEXP call)`
pub unsafe fn R_makeMissingSubscriptError1(call: SEXP) -> SEXP {
    unsafe {
        let msg = "subscript out of bounds";
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();

        R_makeErrorCondition(
            call,
            b"simpleError\0".as_ptr() as *const c_char,
            b"missingSubscriptError\0".as_ptr() as *const c_char,
            0,
            c_msg.as_ptr(),
        )
    }
}

/// R_makeOutOfBoundsError — create an out-of-bounds error condition.
/// Matches C's `SEXP R_makeOutOfBoundsError(SEXP x, int subscript, SEXP sindex, SEXP call)`
pub unsafe fn R_makeOutOfBoundsError(x: SEXP, subscript: c_int, sindex: SEXP, call: SEXP) -> SEXP {
    unsafe {
        let idx_str = if !sindex.is_null() && TYPEOF(sindex) == SEXPTYPE::REALSXP {
            format!("{}", *REAL(sindex))
        } else if !sindex.is_null() && TYPEOF(sindex) == SEXPTYPE::INTSXP {
            format!("{}", *INTEGER(sindex))
        } else {
            format!("{}", subscript)
        };
        let msg = format!("subscript out of bounds (index {} too large)", idx_str);
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();

        R_makeErrorCondition(
            call,
            b"simpleError\0".as_ptr() as *const c_char,
            b"outOfBoundsError\0".as_ptr() as *const c_char,
            0,
            c_msg.as_ptr(),
        )
    }
}

/// R_makeCStackOverflowError — create a C stack overflow error condition.
/// Matches C's `SEXP R_makeCStackOverflowError(SEXP call, intptr_t usage)`
pub unsafe fn R_makeCStackOverflowError(call: SEXP, usage: isize) -> SEXP {
    unsafe {
        let msg = format!("C stack usage {} is too close to the limit", usage);
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();

        R_makeErrorCondition(
            call,
            b"stackOverflowError\0".as_ptr() as *const c_char,
            b"cStackOverflowError\0".as_ptr() as *const c_char,
            0,
            c_msg.as_ptr(),
        )
    }
}

/// R_getProtectStackOverflowError — get the preserved protect stack overflow condition.
pub unsafe fn R_getProtectStackOverflowError() -> SEXP {
    unsafe {
        // Would return a preserved condition; for now return nil
        globals::R_NilValue()
    }
}

/// R_getExpressionStackOverflowError — get the preserved expression stack overflow condition.
pub unsafe fn R_getExpressionStackOverflowError() -> SEXP {
    unsafe {
        // Would return a preserved condition; for now return nil
        globals::R_NilValue()
    }
}

/// R_getNodeStackOverflowError — get the preserved node stack overflow condition.
pub unsafe fn R_getNodeStackOverflowError() -> SEXP {
    unsafe {
        // Would return a preserved condition; for now return nil
        globals::R_NilValue()
    }
}

/// R_tryCatch — C-level tryCatch.
/// Matches C's `SEXP R_tryCatch(SEXP (*body)(void *), void *bdata,
///                              SEXP (*handler)(void *, SEXP), void *hdata)`
pub unsafe fn R_tryCatch(
    body: Option<unsafe extern "C" fn(*mut c_void) -> SEXP>,
    bdata: *mut c_void,
    handler: Option<unsafe extern "C" fn(*mut c_void, SEXP) -> SEXP>,
    hdata: *mut c_void,
) -> SEXP {
    unsafe {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Some(f) = body {
                f(bdata)
            } else {
                globals::R_NilValue()
            }
        })) {
            Ok(result) => result,
            Err(panic_payload) => {
                // ExitingHandler: targeted context jump — pass through unless we are the target
                if let Some(signal) = panic_payload.downcast_ref::<crate::sexp::context::RSignal>()
                    && let crate::sexp::context::RSignal::ExitingHandler { target_env, result } =
                        signal
                {
                    let target = *target_env;
                    let res = *result;
                    if crate::sexp::context::context_env_exists(target) {
                        return res;
                    } else {
                        std::panic::panic_any(crate::sexp::context::RSignal::ExitingHandler {
                            target_env: target,
                            result: res,
                        });
                    }
                }
                // Try to extract the error condition
                let cond = if let Some(ref e) = panic_payload.downcast_ref::<RError>() {
                    R_makeErrorCondition(
                        ptr::null_mut(),
                        b"simpleError\0".as_ptr() as *const c_char,
                        ptr::null_mut(),
                        0,
                        std::ffi::CString::new(e.message.clone())
                            .unwrap_or_default()
                            .as_ptr(),
                    )
                } else {
                    R_makeErrorCondition(
                        ptr::null_mut(),
                        b"simpleError\0".as_ptr() as *const c_char,
                        ptr::null_mut(),
                        0,
                        b"unknown error\0".as_ptr() as *const c_char,
                    )
                };
                if let Some(f) = handler {
                    f(hdata, cond)
                } else {
                    globals::R_NilValue()
                }
            }
        }
    }
}

/// R_withCallingErrorHandler — C-level withCallingHandler for errors.
/// Matches C's `SEXP R_withCallingErrorHandler(...)`
pub unsafe fn R_withCallingErrorHandler(
    body: Option<unsafe extern "C" fn(*mut c_void) -> SEXP>,
    bdata: *mut c_void,
    handler: Option<unsafe extern "C" fn(*mut c_void, SEXP) -> SEXP>,
    hdata: *mut c_void,
) -> SEXP {
    unsafe {
        let klass = Rf_mkChar(b"error\0".as_ptr() as *const c_char);
        let _klass_guard = protect(klass);
        let handler_fn = if let Some(h) = handler {
            Rf_mkString(b"withCallingErrorHandler\0".as_ptr() as *const c_char)
        } else {
            globals::R_NilValue()
        };
        let entry = mkHandlerEntry(
            klass,
            globals::R_GlobalEnv(),
            handler_fn,
            globals::R_NilValue(),
            globals::R_NilValue(),
            1,
        );
        let _entry_guard = protect(entry);

        let old_stack = handler_stack();
        let _old_stack_guard = protect(old_stack);
        let new_top = Rf_cons(entry, old_stack);
        set_handler_stack(new_top);

        let val = if let Some(f) = body {
            f(bdata)
        } else {
            globals::R_NilValue()
        };

        set_handler_stack(old_stack);
        val
    }
}
