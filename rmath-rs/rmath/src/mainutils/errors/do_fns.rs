#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! R-level do_* .Internal/.External entry points for stop, warning,
//! geterrmessage, seterrmessage and interruptsSuspended.

use super::helpers::translateChar;
use super::*;

// ---------------------------------------------------------------------------
// R-level do_* functions (called from the evaluator)
// ---------------------------------------------------------------------------

/// do_stop — R's stop() function.
/// Ported from errors.c do_stop().
pub unsafe fn do_stop(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let mut c_call: SEXP = ptr::null_mut();
        if asLogical(CAR(args)) != 0 {
            c_call = findCall();
        }
        let args = CDR(args);

        if !isNull(CAR(args)) != 0 {
            // Has a message
            SETCAR(args, coerceVector(CAR(args), SEXPTYPE::STRSXP.as_c_int()));
            if isValidString(CAR(args)) == 0 {
                let c_msg =
                    std::ffi::CString::new(" [invalid string in stop(.)]").unwrap_or_default();
                errorcall(c_call, c_msg.as_ptr());
            }
            // Pre-format: in C this is errorcall(c_call, "%s", translateChar(...))
            // In Rust, we pre-format the string
            let msg = translateChar(STRING_ELT(CAR(args), 0));
            let msg_str = CStr::from_ptr(msg).to_str().unwrap_or("");
            let c_msg = std::ffi::CString::new(msg_str).unwrap_or_default();
            errorcall(c_call, c_msg.as_ptr());
            // errorcall doesn't return, but we need a return type
            ptr::null_mut()
        } else {
            errorcall(c_call, b"\0".as_ptr() as *const c_char);
            ptr::null_mut()
        }
    }
}

/// Direct .Internal(stop(...)) handler for structured embedding boundaries.
pub unsafe fn do_stop_internal(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let args = CDR(args);
        if isNull(CAR(args)) != 0 {
            errorcall_str(globals::R_NilValue(), "");
        }

        SETCAR(args, coerceVector(CAR(args), SEXPTYPE::STRSXP.as_c_int()));
        if isValidString(CAR(args)) == 0 {
            errorcall_str(globals::R_NilValue(), " [invalid string in stop(.)]");
        }

        let msg = translateChar(STRING_ELT(CAR(args), 0));
        let message = CStr::from_ptr(msg).to_str().unwrap_or("").to_string();
        // Like upstream do_stop, the call comes from the context stack, not
        // the .Internal expression; render explicitly (bare at top level) so
        // the .Internal-dispatch attribution wrapper does not add a call.
        errorcall_str(R_getCurrentCall(), &message)
    }
}

/// do_warning — R's warning() function.
/// Ported from errors.c do_warning().
pub unsafe fn do_warning(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);

        let mut c_call: SEXP = ptr::null_mut();
        if asLogical(CAR(args)) != 0 {
            c_call = findCall();
        }
        let mut args = CDR(args);

        if asLogical(CAR(args)) != 0 {
            set_immediate_warning(true);
        } else {
            set_immediate_warning(false);
        }
        args = CDR(args);

        if asLogical(CAR(args)) != 0 {
            set_no_break_warning(true);
        } else {
            set_no_break_warning(false);
        }
        args = CDR(args);

        let message = CAR(args);
        if !isNull(message) != 0 {
            SETCAR(args, coerceVector(message, SEXPTYPE::STRSXP.as_c_int()));
            let message = CAR(args);
            if isValidString(message) == 0 {
                let c_msg =
                    std::ffi::CString::new(" [invalid string in warning(.)]").unwrap_or_default();
                warningcall(c_call, c_msg.as_ptr());
            } else {
                // Pre-format: in C this is warningcall(c_call, "%s", translateChar(...))
                let msg = translateChar(STRING_ELT(message, 0));
                let msg_str = CStr::from_ptr(msg).to_str().unwrap_or("");
                let c_msg = std::ffi::CString::new(msg_str).unwrap_or_default();
                warningcall(c_call, c_msg.as_ptr());
            }
        } else {
            warningcall(c_call, b"\0".as_ptr() as *const c_char);
        }

        set_immediate_warning(false);
        set_no_break_warning(false);

        CAR(args)
    }
}

/// do_geterrmessage — geterrmessage().
pub unsafe fn do_geterrmessage(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let msg = R_GetErrorBuf();
        let msg = std::ffi::CString::new(msg).unwrap_or_default();
        Rf_mkString(msg.as_ptr())
    }
}

/// do_seterrmessage — seterrmessage().
pub unsafe fn do_seterrmessage(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        checkArity(op, args);
        let msg = CAR(args);
        if isString(msg) == 0 || LENGTH(msg) != 1 {
            errorcall(
                call,
                b"error message must be a character string\x00".as_ptr() as *const c_char,
            );
        }
        let s = CHAR_local(STRING_ELT(msg, 0));
        R_SetErrmessage_c(s);
        globals::R_NilValue()
    }
}

/// do_interruptsSuspended — get/set interrupts suspended flag.
pub unsafe fn do_interruptsSuspended(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let orig = interrupts_suspended();
        ScalarLogical(orig as c_int)
    }
}
