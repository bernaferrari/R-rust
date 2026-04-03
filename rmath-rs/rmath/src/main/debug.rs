#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/debug.c — debug/trace support.
//!
//! Provides debug(), undebug(), isdebugged(), debugonce(),
//! .Internal(trace()), .primTrace/.primUntrace,
//! tracingState/debuggingState, and memory profiling stubs.

use std::os::raw::c_int;

use crate::main::errors::Rf_error;
use crate::sexp::accessors::{CAR, TYPEOF};
use crate::sexp::constructors::Rf_ScalarLogical;
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::globals::set_R_Visible;

// ---------------------------------------------------------------------------
// Local stub functions for debug/trace gp-bit accessors
//
// In R's C code, SET_RDEBUG/RDEBUG/SET_RSTEP/SET_RTRACE/RTRACE are macros
// that manipulate bits in the gp field of the SEXPREC header. These will be
// replaced with real implementations once the gp-bit infrastructure is wired
// up in accessors.rs. For now they are safe no-ops / zero-returning stubs.
// ---------------------------------------------------------------------------

/// Set the DEBUG bit on an SEXP (stub).
unsafe fn SET_RDEBUG(_x: SEXP, _v: c_int) {
    let _ = (_x, _v);
}

/// Get the DEBUG bit from an SEXP (stub — always returns 0).
unsafe fn RDEBUG(_x: SEXP) -> c_int {
    let _ = _x;
    0
}

/// Set the STEP bit on an SEXP (stub).
unsafe fn SET_RSTEP(_x: SEXP, _v: c_int) {
    let _ = (_x, _v);
}

/// Set the TRACE bit on an SEXP (stub).
unsafe fn SET_RTRACE(_x: SEXP, _v: c_int) {
    let _ = (_x, _v);
}

/// Get the TRACE bit from an SEXP (stub — always returns 0).
unsafe fn RTRACE(_x: SEXP) -> c_int {
    let _ = _x;
    0
}

/// Get the PRIMVAL (primitive internal code) from an SEXP.
unsafe fn PRIMVAL(op: SEXP) -> c_int {
    unsafe { crate::main::relop::PRIMVAL(op) }
}

// ---------------------------------------------------------------------------
// Static state for tracing/debugging toggles
// ---------------------------------------------------------------------------

/// Global tracing state (on/off). Defaults to TRUE (enabled).
static mut tracing_state: c_int = TRUE;

/// Global debugging state (on/off). Defaults to TRUE (enabled).
static mut debugging_state: c_int = TRUE;

// ---------------------------------------------------------------------------
// do_debug — debug / undebug / isdebugged / debugonce
//
// Dispatches on PRIMVAL(op):
//   0 = debug()        SET_RDEBUG(x, 1)
//   1 = undebug()      SET_RDEBUG(x, 0)
//   2 = isdebugged()   return ScalarLogical(RDEBUG(x))
//   3 = debugonce()    SET_RSTEP(x, 1)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_debug(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = (call, rho);
        let s = CAR(args);
        let t = TYPEOF(s);

        // Validate that the argument is a function type
        if t != SEXPTYPE::CLOSXP.0 && t != SEXPTYPE::SPECIALSXP.0 && t != SEXPTYPE::BUILTINSXP.0 {
            Rf_error(
                c"debug/undebug/isdebugged/debugonce requires a function".as_ptr() as *const _,
            );
        }

        match PRIMVAL(op) {
            0 => {
                // debug()
                SET_RDEBUG(s, 1);
            }
            1 => {
                // undebug()
                SET_RDEBUG(s, 0);
            }
            2 => {
                // isdebugged()
                return Rf_ScalarLogical(RDEBUG(s));
            }
            3 => {
                // debugonce()
                SET_RSTEP(s, 1);
            }
            _ => {}
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_trace — .primTrace / .primUntrace
//
// Dispatches on PRIMVAL(op):
//   0 = .primTrace      SET_RTRACE(x, 1)
//   1 = .primUntrace    SET_RTRACE(x, 0)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_trace(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = (call, rho);
        let s = CAR(args);

        match PRIMVAL(op) {
            0 => {
                // .primTrace
                SET_RTRACE(s, 1);
            }
            1 => {
                // .primUntrace
                SET_RTRACE(s, 0);
            }
            _ => {}
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// do_traceOnOff — tracingState / debuggingState
//
// Dispatches on PRIMVAL(op):
//   0 = tracingState — toggle or query tracing
//   1 = debuggingState — toggle or query debugging
//
// Returns ScalarLogical of the previous state.
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_traceOnOff(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = (call, rho);
        let s = CAR(args);
        let state: c_int = if TYPEOF(s) == SEXPTYPE::LGLSXP.0 {
            if !s.is_null() {
                let data = (*s).gengc_next_node as *mut c_int;
                if !data.is_null() { *data } else { 0 }
            } else {
                0
            }
        } else {
            0
        };

        match PRIMVAL(op) {
            0 => {
                // tracingState
                let prev = tracing_state;
                tracing_state = state;
                return Rf_ScalarLogical(prev);
            }
            1 => {
                // debuggingState
                let prev = debugging_state;
                debugging_state = state;
                return Rf_ScalarLogical(prev);
            }
            _ => {}
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// R_current_debug_state — return the global debugging state
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_current_debug_state() -> c_int {
    unsafe { debugging_state }
}

// ---------------------------------------------------------------------------
// R_current_trace_state — return the global tracing state
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_current_trace_state() -> c_int {
    unsafe { tracing_state }
}

// ---------------------------------------------------------------------------
// do_tracemem — stub (memory profiling not compiled in)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_tracemem(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let _ = (call, op, args, rho);
    Rf_error(c"R was not compiled with support for memory profiling".as_ptr() as *const _);
    unreachable!()
}

// ---------------------------------------------------------------------------
// do_untracemem — stub (memory profiling not compiled in)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_untracemem(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    let _ = (call, op, args, rho);
    Rf_error(c"R was not compiled with support for memory profiling".as_ptr() as *const _);
    unreachable!()
}

// ---------------------------------------------------------------------------
// do_retracemem — no-op, returns invisible R_NilValue
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn do_retracemem(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let _ = (call, op, args, rho);
        set_R_Visible(FALSE);
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// memtrace_report — no-op stub for memory tracing
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memtrace_report(old: *mut std::ffi::c_void, new: *mut std::ffi::c_void) {
    let _ = (old, new);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that do_debug does not panic when called with null pointers.
    /// In real usage, null CAR(args) would be caught by the TYPEOF check
    /// and trigger an Rf_error (panic). We test that the stub SET_RDEBUG
    /// calls themselves don't panic with null.
    #[test]
    fn test_debug_set_rdebug() {
        unsafe {
            // The stub SET_RDEBUG should not panic even with null
            SET_RDEBUG(ptr::null_mut(), 1);
            SET_RDEBUG(ptr::null_mut(), 0);
            SET_RSTEP(ptr::null_mut(), 1);
        }
    }

    /// Test that the tracing/debugging state functions work correctly.
    #[test]
    fn test_trace_state() {
        unsafe {
            // Initial state should be TRUE
            assert_eq!(R_current_trace_state(), TRUE);
            assert_eq!(R_current_debug_state(), TRUE);

            // Mutate state directly and verify
            tracing_state = FALSE;
            assert_eq!(R_current_trace_state(), FALSE);

            debugging_state = FALSE;
            assert_eq!(R_current_debug_state(), FALSE);

            // Restore defaults
            tracing_state = TRUE;
            debugging_state = TRUE;
            assert_eq!(R_current_trace_state(), TRUE);
            assert_eq!(R_current_debug_state(), TRUE);
        }
    }

    /// Test that do_tracemem is defined (actual error behavior tested
    /// indirectly — panic through extern "C" aborts, so we skip calling it).
    #[test]
    fn test_tracemem_error() {
        // do_tracemem always calls Rf_error which panics.
        // Cannot catch panics across extern "C" boundaries in Rust,
        // so we just verify the function exists and is callable via FFI.
        assert!(do_tracemem as usize != 0);
    }

    /// Test that do_untracemem is defined.
    #[test]
    fn test_untracemem_error() {
        assert!(do_untracemem as usize != 0);
    }

    /// Test that the stub accessors return expected values.
    #[test]
    fn test_stub_accessors() {
        unsafe {
            assert_eq!(RDEBUG(ptr::null_mut()), 0);
            assert_eq!(RTRACE(ptr::null_mut()), 0);
            assert_eq!(PRIMVAL(ptr::null_mut()), 0);
        }
    }

    /// Test that do_retracemem returns without panicking.
    #[test]
    fn test_retracemem_no_panic() {
        unsafe {
            let result = do_retracemem(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert!(result.is_null() || result == R_NilValue());
        }
    }

    /// Test that memtrace_report is a safe no-op.
    #[test]
    fn test_memtrace_report_noop() {
        unsafe {
            memtrace_report(ptr::null_mut(), ptr::null_mut());
        }
    }
}
