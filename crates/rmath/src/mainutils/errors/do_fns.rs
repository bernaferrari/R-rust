#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! R-level do_* .Internal/.External entry points for stop, warning,
//! geterrmessage, seterrmessage and interruptsSuspended.

use super::helpers::diagnostic_cstring;
use super::*;

// ---------------------------------------------------------------------------
// R-level do_* functions (called from the evaluator)
// ---------------------------------------------------------------------------

/// do_stop — R's stop() function.
/// Ported from errors.c do_stop().
pub fn do_stop(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    checkArity(op, args);

    let mut c_call: SEXP = ptr::null_mut();
    if asLogical(CAR(args)) != 0 {
        c_call = findCall();
    }
    let args = CDR(args);

    if isNull(CAR(args)) == 0 {
        // Has a message
        SETCAR(args, coerceVector(CAR(args), SEXPTYPE::STRSXP.as_c_int()));
        if isValidString(CAR(args)) == 0 {
            let c_msg = c" [invalid string in stop(.)]";
            implicit_errorcall_cstr(c_call, &c_msg);
        }
        // Pre-format: in C this is errorcall(c_call, "%s", translateChar(...))
        // In Rust, we pre-format the string
        let msg = diagnostic_cstring(STRING_ELT(CAR(args), 0));
        implicit_errorcall_str(c_call, msg.to_str().unwrap_or(""))
    } else {
        implicit_errorcall_cstr(c_call, c"");
        ptr::null_mut()
    }
}

/// Direct .Internal(stop(...)) handler for structured embedding boundaries.
pub fn do_stop_internal(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    checkArity(op, args);

    let args = CDR(args);
    if isNull(CAR(args)) != 0 {
        implicit_errorcall_str(globals::R_NilValue(), "");
    }

    SETCAR(args, coerceVector(CAR(args), SEXPTYPE::STRSXP.as_c_int()));
    if isValidString(CAR(args)) == 0 {
        implicit_errorcall_str(globals::R_NilValue(), " [invalid string in stop(.)]");
    }

    let msg = diagnostic_cstring(STRING_ELT(CAR(args), 0));
    // Like upstream do_stop, the call comes from the context stack, not
    // the .Internal expression; render explicitly (bare at top level) so
    // the .Internal-dispatch attribution wrapper does not add a call.
    implicit_errorcall_str(R_getCurrentCall(), msg.to_str().unwrap_or(""))
}

/// do_warning — R's warning() function.
/// Ported from errors.c do_warning().
pub fn do_warning(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
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
    if isNull(message) == 0 {
        SETCAR(args, coerceVector(message, SEXPTYPE::STRSXP.as_c_int()));
        let message = CAR(args);
        if isValidString(message) == 0 {
            let c_msg = c" [invalid string in warning(.)]";
            warningcall_cstr(c_call, &c_msg);
        } else {
            // Pre-format: in C this is warningcall(c_call, "%s", translateChar(...))
            let msg = diagnostic_cstring(STRING_ELT(message, 0));
            let msg = if msg.to_str().is_ok() { msg.as_c_str() } else { c"" };
            warningcall_cstr(c_call, msg);
        }
    } else {
        warningcall_cstr(c_call, c"");
    }

    set_immediate_warning(false);
    set_no_break_warning(false);

    CAR(args)
}

/// do_geterrmessage — geterrmessage().
pub fn do_geterrmessage(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    super::native::error_buffer_entry(op, args, false)
}

/// do_seterrmessage — seterrmessage().
pub fn do_seterrmessage(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    super::native::error_buffer_entry(op, args, true)
}

/// do_interruptsSuspended — get/set interrupts suspended flag.
pub fn do_interruptsSuspended(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    {
        let orig = interrupts_suspended();
        ScalarLogical(orig as c_int)
    }
}
