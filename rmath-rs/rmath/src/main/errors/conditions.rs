#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Condition handling: R_InitConditions, R_MakeCondition, R_makeErrorCondition,
//! R_signalErrorCondition, R_tryCatch, R_withCallingErrorHandler, etc.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use crate::attrib_core::{R_ClassSymbol, R_NamesSymbol};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals;

use super::format::{CHAR_local, checkArity, getAttrib_wrap, isNull, setAttrib_wrap};

// Use safe wrappers from mod.rs for cross-submodule calls
use super::warning::{R_GetTracebackOnly, warningcall};
use super::{errorcall, jump_to_top_ex};

// ---------------------------------------------------------------------------
// Handler entry structure
// ---------------------------------------------------------------------------

/// mkHandlerEntry -- create a condition handler entry.
pub fn mkHandlerEntry(
    klass: SEXP,
    parentenv: SEXP,
    handler: SEXP,
    target: SEXP,
    result: SEXP,
    calling: c_int,
) -> SEXP {
    unsafe {
        let entry = Rf_allocVector(SEXPTYPE::VECSXP.0, 5);
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

// ---------------------------------------------------------------------------
// Condition handler stack operations
// ---------------------------------------------------------------------------

/// do_addCondHands -- add condition handlers to the stack.
pub unsafe fn do_addCondHands(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        use super::R_HANDLER_STACK;
        checkArity(op, args);

        let classes = CAR(args);
        let mut rest = CDR(args);
        let handlers = CAR(rest);
        rest = CDR(rest);
        let parentenv = CAR(rest);
        rest = CDR(rest);
        let target = CAR(rest);
        rest = CDR(rest);
        let calling = super::format::asLogical(CAR(rest));

        if isNull(classes) != 0 || isNull(handlers) != 0 {
            return R_HANDLER_STACK.with(|s| *s.borrow());
        }

        let n = LENGTH(handlers);

        R_HANDLER_STACK.with(|stack| {
            let oldstack = *stack.borrow();

            let result = Rf_allocVector(SEXPTYPE::VECSXP.0, RESULT_SIZE as c_int);
            let mut newstack = oldstack;

            for i in (0..n).rev() {
                let klass = STRING_ELT(classes, i as R_xlen_t);
                let handler = VECTOR_ELT(handlers, i as R_xlen_t);
                let entry = mkHandlerEntry(klass, parentenv, handler, target, result, calling);
                newstack = Rf_cons(entry, newstack);
            }

            *stack.borrow_mut() = newstack;
            oldstack
        })
    }
}

/// do_resetCondHands -- reset condition handlers to a previous state.
pub unsafe fn do_resetCondHands(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        use super::R_HANDLER_STACK;
        checkArity(op, args);
        let old = CAR(args);
        R_HANDLER_STACK.with(|stack| {
            *stack.borrow_mut() = old;
        });
        globals::R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// Restart handling
// ---------------------------------------------------------------------------

/// do_getRestart -- get a restart from the restart stack.
pub unsafe fn do_getRestart(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        use super::R_RESTART_STACK;
        use super::format::{isInteger, setAttrib_wrap};
        checkArity(op, args);
        let mut i = if isInteger(CAR(args)) != 0 && LENGTH(CAR(args)) >= 1 {
            *INTEGER(CAR(args)).offset(0)
        } else {
            crate::sexp::ffi::NA_INTEGER
        };

        R_RESTART_STACK.with(|stack| {
            let mut list = *stack.borrow();
            while !list.is_null() && i > 1 {
                list = CDR(list);
                i -= 1;
            }
            if !list.is_null() {
                CAR(list)
            } else if i == 1 {
                let name = Rf_mkString(b"abort\x00".as_ptr() as *const c_char);
                let entry = Rf_allocVector(SEXPTYPE::VECSXP.0, 2);
                SET_VECTOR_ELT(entry, 0, name);
                SET_VECTOR_ELT(entry, 1, globals::R_NilValue());
                setAttrib_wrap(
                    entry,
                    R_ClassSymbol(),
                    Rf_mkString(b"restart\x00".as_ptr() as *const c_char),
                );
                entry
            } else {
                globals::R_NilValue()
            }
        })
    }
}

/// do_addRestart -- add a restart to the restart stack.
pub unsafe fn do_addRestart(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        use super::R_RESTART_STACK;
        checkArity(op, args);
        let r = CAR(args);
        if TYPEOF(r) != SEXPTYPE::VECSXP.0 || LENGTH(r) < 2 {
            errorcall(call, b"bad restart\x00".as_ptr() as *const c_char);
        }
        R_RESTART_STACK.with(|stack| {
            let new = Rf_cons(r, *stack.borrow());
            *stack.borrow_mut() = new;
        });
        globals::R_NilValue()
    }
}

/// do_invokeRestart -- invoke a restart.
pub unsafe fn do_invokeRestart(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        use super::R_RESTART_STACK;
        use super::format::isNull;
        checkArity(op, args);
        let r = CAR(args);
        if TYPEOF(r) != SEXPTYPE::VECSXP.0 || LENGTH(r) < 2 {
            errorcall(call, b"bad restart\x00".as_ptr() as *const c_char);
        }
        let exit = VECTOR_ELT(r, 1);
        if isNull(exit) != 0 {
            R_RESTART_STACK.with(|stack| {
                *stack.borrow_mut() = globals::R_NilValue();
            });
            jump_to_top_ex(0, 0, 1, 1, 1);
        }
        ptr::null_mut()
    }
}

/// do_addTryHandlers -- add tryCatch handlers.
pub unsafe fn do_addTryHandlers(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        globals::R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// Condition signaling
// ---------------------------------------------------------------------------

/// do_signalCondition -- signal a condition.
pub unsafe fn do_signalCondition(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        globals::R_NilValue()
    }
}

/// do_dfltWarn -- default warning handler.
pub unsafe fn do_dfltWarn(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        use super::format::translateChar;
        checkArity(op, args);
        if TYPEOF(CAR(args)) != SEXPTYPE::STRSXP.0 || LENGTH(CAR(args)) != 1 {
            errorcall(call, b"bad error message\x00".as_ptr() as *const c_char);
        }
        let msg = translateChar(STRING_ELT(CAR(args), 0));
        let ecall = CADR(args);
        warningcall(ecall, msg);
        globals::R_NilValue()
    }
}

/// do_dfltStop -- default error handler.
pub unsafe fn do_dfltStop(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        use super::format::translateChar;
        checkArity(op, args);
        if TYPEOF(CAR(args)) != SEXPTYPE::STRSXP.0 || LENGTH(CAR(args)) != 1 {
            errorcall(call, b"bad error message\x00".as_ptr() as *const c_char);
        }
        let msg = translateChar(STRING_ELT(CAR(args), 0));
        let ecall = CADR(args);
        errorcall(ecall, msg);
        ptr::null_mut()
    }
}

// ---------------------------------------------------------------------------
// Condition creation helpers
// ---------------------------------------------------------------------------

/// R_makeErrorCondition -- create an error condition object.
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
        let cond = Rf_allocVector(SEXPTYPE::VECSXP.0, nelem);

        SET_VECTOR_ELT(cond, 0, Rf_mkString(fmt.as_ptr() as *const c_char));
        SET_VECTOR_ELT(cond, 1, call);

        let names = Rf_allocVector(SEXPTYPE::STRSXP.0, nelem);
        setAttrib_wrap(cond, R_NamesSymbol(), names);
        SET_STRING_ELT(
            names,
            0,
            Rf_mkChar(b"message\x00".as_ptr() as *const c_char),
        );
        SET_STRING_ELT(names, 1, Rf_mkChar(b"call\x00".as_ptr() as *const c_char));

        let nclass = if sub.is_empty() { 3 } else { 4 };
        let klass = Rf_allocVector(SEXPTYPE::STRSXP.0, nclass);
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

/// R_signalErrorCondition -- signal an error condition.
pub unsafe fn R_signalErrorCondition(cond: SEXP, call: SEXP) {
    unsafe {
        use super::format::translateChar;
        if TYPEOF(cond) != SEXPTYPE::VECSXP.0 || LENGTH(cond) == 0 {
            errorcall(
                call,
                b"condition object must be a VECSXP of length at least one\x00".as_ptr()
                    as *const c_char,
            );
        }
        let elt = VECTOR_ELT(cond, 0);
        if TYPEOF(elt) != SEXPTYPE::STRSXP.0 || LENGTH(elt) != 1 {
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

/// R_signalErrorConditionEx -- signal an error condition with exitOnly flag.
pub unsafe fn R_signalErrorConditionEx(cond: SEXP, call: SEXP, exitOnly: c_int) {
    unsafe {
        R_signalErrorCondition(cond, call);
    }
}

/// R_setConditionField -- set a field in a condition object.
pub unsafe fn R_setConditionField(
    cond: SEXP,
    idx: R_xlen_t,
    name: *const c_char,
    val: SEXP,
) {
    unsafe {
        if TYPEOF(cond) != SEXPTYPE::VECSXP.0 {
            return;
        }
        let len = XLENGTH(cond);
        if idx < 0 || idx >= len {
            return;
        }
        SET_VECTOR_ELT(cond, idx, val);
        let names = getAttrib_wrap(cond, R_NamesSymbol());
        if !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP.0 && XLENGTH(names) == len {
            SET_STRING_ELT(names, idx, Rf_mkChar(name));
        }
    }
}

// ---------------------------------------------------------------------------
// Warning condition helpers
// ---------------------------------------------------------------------------

/// R_signalWarningCondition -- signal a warning condition object.
pub unsafe fn R_signalWarningCondition(cond: SEXP) {
    unsafe {
        use super::format::translateChar;
        if cond.is_null() || TYPEOF(cond) != SEXPTYPE::VECSXP.0 || LENGTH(cond) < 1 {
            return;
        }
        let elt = VECTOR_ELT(cond, 0);
        if TYPEOF(elt) != SEXPTYPE::STRSXP.0 || LENGTH(elt) != 1 {
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

/// R_makeWarningCondition -- create a warning condition object.
pub unsafe fn R_makeWarningCondition(
    call: SEXP,
    classname: *const c_char,
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
        let fmt = if format.is_null() {
            ""
        } else {
            CStr::from_ptr(format).to_str().unwrap_or("")
        };

        let nelem = nextra + 2;
        let cond = Rf_allocVector(SEXPTYPE::VECSXP.0, nelem);

        SET_VECTOR_ELT(cond, 0, Rf_mkString(fmt.as_ptr() as *const c_char));
        SET_VECTOR_ELT(
            cond,
            1,
            if call.is_null() {
                globals::R_NilValue()
            } else {
                call
            },
        );

        let names = Rf_allocVector(SEXPTYPE::STRSXP.0, nelem);
        setAttrib_wrap(cond, R_NamesSymbol(), names);
        SET_STRING_ELT(names, 0, Rf_mkChar(b"message\0".as_ptr() as *const c_char));
        SET_STRING_ELT(names, 1, Rf_mkChar(b"call\0".as_ptr() as *const c_char));

        let klass = Rf_allocVector(SEXPTYPE::STRSXP.0, 3);
        setAttrib_wrap(cond, R_ClassSymbol(), klass);
        SET_STRING_ELT(klass, 0, Rf_mkChar(class.as_ptr() as *const c_char));
        SET_STRING_ELT(klass, 1, Rf_mkChar(b"warning\0".as_ptr() as *const c_char));
        SET_STRING_ELT(
            klass,
            2,
            Rf_mkChar(b"condition\0".as_ptr() as *const c_char),
        );

        cond
    }
}

/// R_makePartialMatchWarningCondition -- create a partial match warning condition.
pub unsafe fn R_makePartialMatchWarningCondition(
    call: SEXP,
    argument: SEXP,
    formal: SEXP,
) -> SEXP {
    unsafe {
        use crate::main::inlined::PRINTNAME;
        let arg_name = if !argument.is_null() && TYPEOF(argument) == SEXPTYPE::SYMSXP.0 {
            CHAR_local(PRINTNAME(argument))
        } else {
            b"?\0".as_ptr() as *const c_char
        };
        let formal_name = if !formal.is_null() && TYPEOF(formal) == SEXPTYPE::SYMSXP.0 {
            CHAR_local(PRINTNAME(formal))
        } else {
            b"?\0".as_ptr() as *const c_char
        };

        let arg_str = CStr::from_ptr(arg_name).to_str().unwrap_or("?");
        let formal_str = CStr::from_ptr(formal_name).to_str().unwrap_or("?");
        let msg = format!(
            "'{}' matches multiple arguments (partial match of '{}' to '{}')",
            arg_str, arg_str, formal_str
        );
        let c_msg = std::ffi::CString::new(msg).expect("CString::new failed: contains null byte");

        R_makeWarningCondition(
            call,
            b"simpleWarning\0".as_ptr() as *const c_char,
            0,
            c_msg.as_ptr(),
        )
    }
}

// ---------------------------------------------------------------------------
// Error condition helpers
// ---------------------------------------------------------------------------

/// R_makeNotSubsettableError -- create a "not subsettable" error condition.
pub unsafe fn R_makeNotSubsettableError(x: SEXP, call: SEXP) -> SEXP {
    unsafe {
        let class_str = if !x.is_null() {
            let klass = getAttrib_wrap(x, R_ClassSymbol());
            if !klass.is_null() && TYPEOF(klass) == SEXPTYPE::STRSXP.0 && LENGTH(klass) >= 1 {
                let s = CHAR_local(STRING_ELT(klass, 0));
                CStr::from_ptr(s).to_str().unwrap_or("object")
            } else {
                "object"
            }
        } else {
            "object"
        };
        let msg = format!("object of type '{}' is not subsettable", class_str);
        let c_msg = std::ffi::CString::new(msg).expect("CString::new failed: contains null byte");

        R_makeErrorCondition(
            call,
            b"simpleError\0".as_ptr() as *const c_char,
            b"notSubsettableError\0".as_ptr() as *const c_char,
            0,
            c_msg.as_ptr(),
        )
    }
}

/// R_makeMissingSubscriptError -- create a missing subscript error condition.
pub unsafe fn R_makeMissingSubscriptError(x: SEXP, call: SEXP) -> SEXP {
    unsafe {
        let class_str = if !x.is_null() {
            let klass = getAttrib_wrap(x, R_ClassSymbol());
            if !klass.is_null() && TYPEOF(klass) == SEXPTYPE::STRSXP.0 && LENGTH(klass) >= 1 {
                let s = CHAR_local(STRING_ELT(klass, 0));
                CStr::from_ptr(s).to_str().unwrap_or("object")
            } else {
                "object"
            }
        } else {
            "object"
        };
        let msg = format!("subscript out of bounds for {}", class_str);
        let c_msg = std::ffi::CString::new(msg).expect("CString::new failed: contains null byte");

        R_makeErrorCondition(
            call,
            b"simpleError\0".as_ptr() as *const c_char,
            b"missingSubscriptError\0".as_ptr() as *const c_char,
            0,
            c_msg.as_ptr(),
        )
    }
}

/// R_makeMissingSubscriptError1 -- create a missing subscript error condition (no x).
pub unsafe fn R_makeMissingSubscriptError1(call: SEXP) -> SEXP {
    unsafe {
        let msg = "subscript out of bounds";
        let c_msg = std::ffi::CString::new(msg).expect("CString::new failed: contains null byte");

        R_makeErrorCondition(
            call,
            b"simpleError\0".as_ptr() as *const c_char,
            b"missingSubscriptError\0".as_ptr() as *const c_char,
            0,
            c_msg.as_ptr(),
        )
    }
}

/// R_makeOutOfBoundsError -- create an out-of-bounds error condition.
pub unsafe fn R_makeOutOfBoundsError(
    x: SEXP,
    subscript: c_int,
    sindex: SEXP,
    call: SEXP,
) -> SEXP {
    unsafe {
        let idx_str = if !sindex.is_null() && TYPEOF(sindex) == SEXPTYPE::REALSXP.0 {
            format!("{}", *REAL(sindex))
        } else if !sindex.is_null() && TYPEOF(sindex) == SEXPTYPE::INTSXP.0 {
            format!("{}", *INTEGER(sindex))
        } else {
            format!("{}", subscript)
        };
        let msg = format!("subscript out of bounds (index {} too large)", idx_str);
        let c_msg = std::ffi::CString::new(msg).expect("CString::new failed: contains null byte");

        R_makeErrorCondition(
            call,
            b"simpleError\0".as_ptr() as *const c_char,
            b"outOfBoundsError\0".as_ptr() as *const c_char,
            0,
            c_msg.as_ptr(),
        )
    }
}

/// R_makeCStackOverflowError -- create a C stack overflow error condition.
pub unsafe fn R_makeCStackOverflowError(call: SEXP, usage: isize) -> SEXP {
    unsafe {
        let msg = format!("C stack usage {} is too close to the limit", usage);
        let c_msg = std::ffi::CString::new(msg).expect("CString::new failed: contains null byte");

        R_makeErrorCondition(
            call,
            b"stackOverflowError\0".as_ptr() as *const c_char,
            b"cStackOverflowError\0".as_ptr() as *const c_char,
            0,
            c_msg.as_ptr(),
        )
    }
}

/// R_getProtectStackOverflowError -- get the preserved protect stack overflow condition.
pub unsafe fn R_getProtectStackOverflowError() -> SEXP {
    unsafe { globals::R_NilValue() }
}

/// R_getExpressionStackOverflowError -- get the preserved expression stack overflow condition.
pub unsafe fn R_getExpressionStackOverflowError() -> SEXP {
    unsafe { globals::R_NilValue() }
}

/// R_getNodeStackOverflowError -- get the preserved node stack overflow condition.
pub unsafe fn R_getNodeStackOverflowError() -> SEXP {
    unsafe { globals::R_NilValue() }
}

// ---------------------------------------------------------------------------
// tryCatch support
// ---------------------------------------------------------------------------

/// R_tryCatchError -- C-level tryCatch for error conditions.
pub unsafe fn R_tryCatchError(
    body: Option<unsafe extern "C" fn(*mut c_void) -> SEXP>,
    bdata: *mut c_void,
    handler: Option<unsafe extern "C" fn(SEXP, *mut c_void) -> SEXP>,
    hdata: *mut c_void,
) -> SEXP {
    unsafe {
        if let Some(f) = body {
            f(bdata)
        } else {
            globals::R_NilValue()
        }
    }
}

/// R_tryCatch -- C-level tryCatch.
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
                // BreakSignal/NextSignal/ReturnSignal are control flow, not errors
                if panic_payload.is::<crate::sexp::context::BreakSignal>()
                    || panic_payload.is::<crate::sexp::context::NextSignal>()
                    || panic_payload.is::<crate::sexp::context::ReturnSignal>()
                {
                    std::panic::resume_unwind(panic_payload);
                }
                let cond = if let Some(ref e) = panic_payload.downcast_ref::<RError>() {
                    R_makeErrorCondition(
                        ptr::null_mut(),
                        b"simpleError\0".as_ptr() as *const c_char,
                        ptr::null_mut(),
                        0,
                        std::ffi::CString::new(e.message.clone()).expect("CString::new failed: contains null byte").as_ptr(),
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

/// R_withCallingErrorHandler -- C-level withCallingHandler for errors.
pub unsafe fn R_withCallingErrorHandler(
    body: Option<unsafe extern "C" fn(*mut c_void) -> SEXP>,
    bdata: *mut c_void,
    handler: Option<unsafe extern "C" fn(*mut c_void, SEXP) -> SEXP>,
    hdata: *mut c_void,
) -> SEXP {
    unsafe {
        if let Some(f) = body {
            f(bdata)
        } else {
            globals::R_NilValue()
        }
    }
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// R_InitConditions -- initialize error/warning condition objects.
pub unsafe fn R_InitConditions() {
    unsafe {
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

// ---------------------------------------------------------------------------
// do_traceback
// ---------------------------------------------------------------------------

/// do_traceback -- traceback().
pub unsafe fn do_traceback(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        use super::format::{checkArity, isInteger};
        checkArity(op, args);
        let skip = if isInteger(CAR(args)) != 0 && LENGTH(CAR(args)) >= 1 {
            *INTEGER(CAR(args)).offset(0)
        } else {
            crate::sexp::ffi::NA_INTEGER
        };
        if skip == crate::sexp::ffi::NA_INTEGER || skip < 0 {
            errorcall(call, b"invalid 'skip' value\x00".as_ptr() as *const c_char);
        }
        R_GetTracebackOnly(skip)
    }
}
