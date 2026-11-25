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
