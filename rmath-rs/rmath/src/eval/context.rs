#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Context management for the evaluator — ports parts of context.c.
//!
//! This module provides the evaluator-specific context operations:
//! - R_run_onexits: run on.exit handlers
//! - R_findParentContext: find parent function context
//! - R_CleanUp: cleanup on error/exit
//! - R_jump_to_top: jump to top-level context

use std::os::raw::c_int;

use crate::sexp::context as sexp_context;
use crate::sexp::ffi::SEXP;
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// R_run_onexits — run on.exit handlers
// ---------------------------------------------------------------------------

/// Run all on.exit handlers registered in contexts above the given one.
///
/// This is the equivalent of R's `R_run_onexits()` in context.c.
pub unsafe fn R_run_onexits() {
    // In the full implementation, this walks the context stack
    // and calls any registered on.exit handlers.
    // For now, this is a stub.
}

// ---------------------------------------------------------------------------
// R_findParentContext — find the nearest parent function context
// ---------------------------------------------------------------------------

/// Find the parent context of a given type.
///
/// This is the equivalent of R's `R_findParentContext()` in context.c.
pub unsafe fn R_findParentContext(_ctxt: SEXP, _which: c_int) -> SEXP {
    unsafe {
        // Simplified: walk up the context stack looking for function contexts
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// R_findExecContext — find the executing function context
// ---------------------------------------------------------------------------

/// Find the context of the currently executing function.
pub unsafe fn R_findExecContext(_rho: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// R_CleanUp — cleanup on error or normal exit
// ---------------------------------------------------------------------------

/// Clean up the evaluator state (context-specific cleanup).
///
/// This is the evaluator-specific cleanup called by R_CleanUp.
pub unsafe fn eval_CleanUp(_sa: c_int, _status: c_int, _RunLast: c_int) {
    unsafe {
        // In the full implementation:
        // 1. Run on.exit handlers
        // 2. Close connections
        // 3. Print warnings
        // 4. etc.
        R_run_onexits();
    }
}

// ---------------------------------------------------------------------------
// R_jumpctxt — jump to a specific context (panic-based)
// ---------------------------------------------------------------------------

/// Jump to a specific context, running on.exit handlers.
///
/// This is the equivalent of R's `R_jumpctxt()` in context.c.
/// In C, this uses longjmp. In Rust, we panic with RError.
pub unsafe fn R_jumpctxt(_ctxt: *mut sexp_context::RCNTXT, _retval: c_int) {
    std::panic::panic_any(sexp_context::RError {
        message: "jump_to_context".to_string(),
    });
}

// ---------------------------------------------------------------------------
// R_jump_to_top — jump to the top-level context
// ---------------------------------------------------------------------------

/// Jump to the top-level context (used for error recovery).
///
/// This is the equivalent of R's `R_jump_to_top()` in context.c.
pub unsafe fn R_jump_to_top() {
    // In C, this uses longjmp to the top-level context
    // In Rust, we panic with a special error
    std::panic::panic_any(sexp_context::RError {
        message: "jump_to_top".to_string(),
    });
}

// ---------------------------------------------------------------------------
// R_InsertRestartHandlers — manage restart handlers
// ---------------------------------------------------------------------------

/// Insert restart handlers into the context stack.
pub unsafe fn R_InsertRestartHandlers(_call: SEXP, _rho: SEXP) {
    // Stub
}
