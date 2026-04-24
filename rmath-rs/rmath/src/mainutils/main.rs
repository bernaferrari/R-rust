#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/main.c — main REPL and global variable definitions.
//!
//! Provides Rf_mainloop(), R_ReplFile(), Rf_ReplIteration(), and other
//! core REPL functions. Currently stubbed since it depends on the parser,
//! readline, and the full evaluation system.

use std::os::raw::c_int;

use crate::sexp::ffi::{FALSE, SEXP, TRUE};
use crate::sexp::globals::{R_NilValue, R_Visible, set_R_Visible};
use crate::sexp::instance::with_required_current_instance;
use crate::sexp::symbol::Rf_install;

/// Console buffer size.
pub const CONSOLE_BUFFER_SIZE: usize = 1024;

// ---------------------------------------------------------------------------
// Global symbols (initialized lazily)
// ---------------------------------------------------------------------------

/// Get the .Last.value symbol.
pub unsafe fn R_LastvalueSymbol() -> SEXP {
    unsafe {
        Rf_install(
            std::ffi::CString::new(".Last.value")
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

/// Get the .Random.seed symbol.
pub unsafe fn R_SeedsSymbol() -> SEXP {
    unsafe {
        Rf_install(
            std::ffi::CString::new(".Random.seed")
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

// ---------------------------------------------------------------------------
// R_Visible accessor
// ---------------------------------------------------------------------------

/// Get R_Visible flag.
pub unsafe fn R_GetVisible() -> c_int {
    unsafe { if R_Visible() != 0 { TRUE } else { FALSE } }
}

/// Set R_Visible flag.
pub unsafe fn R_SetVisible(v: c_int) {
    unsafe {
        set_R_Visible(v);
    }
}

/// Get R_Interactive flag.
pub unsafe fn R_Interactive() -> c_int {
    with_required_current_instance(|inst| inst.eval_state.interactive)
}

/// Set R_Interactive flag.
pub unsafe fn R_SetInteractive(v: c_int) {
    with_required_current_instance(|inst| inst.eval_state.interactive = v);
}

/// Get R_Quiet flag.
pub unsafe fn R_Quiet() -> c_int {
    with_required_current_instance(|inst| inst.eval_state.quiet)
}

/// Set R_Quiet flag.
pub unsafe fn R_SetQuiet(v: c_int) {
    with_required_current_instance(|inst| inst.eval_state.quiet = v);
}

/// Get R_NoEcho flag.
pub unsafe fn R_NoEcho() -> c_int {
    with_required_current_instance(|inst| inst.eval_state.no_echo)
}

/// Get R_Verbose flag.
pub unsafe fn R_Verbose() -> c_int {
    with_required_current_instance(|inst| inst.eval_state.verbose)
}

// ---------------------------------------------------------------------------
/// Get evaluation depth.
pub unsafe fn R_GetEvalDepth() -> c_int {
    with_required_current_instance(|inst| inst.eval_state.eval_depth)
}

/// Set evaluation depth.
pub unsafe fn R_SetEvalDepth(v: c_int) {
    with_required_current_instance(|inst| inst.eval_state.eval_depth = v);
}

// ---------------------------------------------------------------------------
/// Get protection stack top.
pub unsafe fn R_PPStackTop() -> c_int {
    with_required_current_instance(|inst| inst.eval_state.pp_stack_top)
}

/// Set protection stack top.
pub unsafe fn R_SetPPStackTop(v: c_int) {
    with_required_current_instance(|inst| inst.eval_state.pp_stack_top = v);
}

// ---------------------------------------------------------------------------
/// Get warnings collection flag.
pub unsafe fn R_GetCollectWarnings() -> c_int {
    with_required_current_instance(|inst| inst.eval_state.collect_warnings)
}

/// Set warnings collection flag.
pub unsafe fn R_SetCollectWarnings(v: c_int) {
    with_required_current_instance(|inst| inst.eval_state.collect_warnings = v);
}

// ---------------------------------------------------------------------------
// Time limits (stubs)
// ---------------------------------------------------------------------------

pub unsafe fn resetTimeLimits() {
    // Unimplemented
}

pub unsafe fn checkTimeLimits() {
    // Unimplemented
}

// ---------------------------------------------------------------------------
// SrcRef state (stubs)
// ---------------------------------------------------------------------------

pub unsafe fn R_InitSrcRefState(_cntxt: *mut std::ffi::c_void) {
    // Unimplemented
}

pub unsafe fn R_FinalizeSrcRefState() {
    // Unimplemented
}

// ---------------------------------------------------------------------------
// Parse status
// ---------------------------------------------------------------------------

pub const PARSE_OK: c_int = 0;
pub const PARSE_INCOMPLETE: c_int = 1;
pub const PARSE_ERROR: c_int = 2;
pub const PARSE_EOF: c_int = 3;
pub const PARSE_NULL: c_int = 4;

pub unsafe fn R_GetParseErrorMsg() -> *const std::os::raw::c_char {
    with_required_current_instance(|inst| {
        inst.eval_state.parse_error_msg.as_ptr() as *const std::os::raw::c_char
    })
}

// ---------------------------------------------------------------------------
// R_ReplFile — REPL from file (stub)
// ---------------------------------------------------------------------------

/// Run the REPL reading from a file.
///
/// This is the equivalent of R's `R_ReplFile()` from main.c.
pub unsafe fn R_ReplFile(_fp: *mut std::ffi::c_void, _rho: SEXP) {
    // in the full implementation, this reads expressions from the file
    // and evaluates them
}

// ---------------------------------------------------------------------------
// Rf_ReplIteration — single REPL iteration (stub)
// ---------------------------------------------------------------------------

/// Perform a single REPL iteration.
///
/// This is the equivalent of R's `Rf_ReplIteration()` from main.c.
pub unsafe fn Rf_ReplIteration(
    _rho: SEXP,
    _savestack: c_int,
    _browselevel: c_int,
    _state: *mut std::ffi::c_void,
) -> c_int {
    // return 0 (continue)
    0
}

// ---------------------------------------------------------------------------
// Rf_ReplConsole — interactive REPL (stub)
// ---------------------------------------------------------------------------

/// Run the interactive REPL.
///
/// This is the equivalent of R's `Rf_ReplConsole()` from main.c.
pub unsafe fn Rf_ReplConsole(_rho: SEXP, _savestack: c_int, _browselevel: c_int) {
    // in the full implementation, this runs the interactive REPL loop
}

// ---------------------------------------------------------------------------
// Rf_mainloop — main R loop (stub)
// ---------------------------------------------------------------------------

/// The main R read-eval-print loop.
///
/// This is the equivalent of R's `Rf_mainloop()` from main.c.
/// Called from main() after Rf_initialize_R().
pub unsafe fn Rf_mainloop() {
    // Headless: no REPL loop. For embedded use, call eval() directly.
}

// ---------------------------------------------------------------------------
// R_Parse1File — parse one expression from file (stub)
// ---------------------------------------------------------------------------

pub unsafe fn R_Parse1File(
    _fp: *mut std::ffi::c_void,
    _prompt: c_int,
    _status: *mut c_int,
) -> SEXP {
    unsafe {
        if !_status.is_null() {
            *_status = PARSE_EOF;
        }
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// setup_Rmainloop — setup before mainloop (stub)
// ---------------------------------------------------------------------------

pub unsafe fn setup_Rmainloop() {
    // Unimplemented
}

// ---------------------------------------------------------------------------
// Top-level handlers (stubs)
// ---------------------------------------------------------------------------

pub unsafe fn Rf_callToplevelHandlers(
    _expr: SEXP,
    _value: SEXP,
    _succeeded: c_int,
    _visible: c_int,
) {
    // Unimplemented
}

pub unsafe fn Rf_addTaskCallback(_fun: SEXP, _data: SEXP) -> c_int {
    0
}

pub unsafe fn Rf_removeTaskCallback(_name: SEXP) -> c_int {
    0
}

// ---------------------------------------------------------------------------
// Memory profiling (stubs)
// ---------------------------------------------------------------------------

pub unsafe fn R_GetMaxVSize() -> u64 {
    u64::MAX
}

pub unsafe fn R_GetMaxNSize() -> u64 {
    u64::MAX
}

pub unsafe fn R_GetVSize() -> u64 {
    crate::sexp::memory::with_arena(|a| a.total_bytes_allocated() as u64)
}

pub unsafe fn R_GetNSize() -> u64 {
    crate::sexp::memory::with_arena(|a| a.node_count() as u64)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::sexp::session::RSession;

    use super::*;

    #[test]
    fn test_repl_stub() {
        unsafe {
            Rf_mainloop();
        }
    }

    #[test]
    fn test_parse_status_constants() {
        assert_eq!(PARSE_OK, 0);
        assert_eq!(PARSE_EOF, 3);
    }

    #[test]
    fn test_r_quiet() {
        let _session = RSession::new();
        unsafe {
            assert_eq!(R_Quiet(), 0);
            R_SetQuiet(1);
            assert_eq!(R_Quiet(), 1);
            R_SetQuiet(0);
        }
    }

    #[test]
    fn test_r_interactive() {
        let _session = RSession::new();
        unsafe {
            assert_eq!(R_Interactive(), 1);
            R_SetInteractive(0);
            assert_eq!(R_Interactive(), 0);
            R_SetInteractive(1);
        }
    }

    #[test]
    fn test_eval_depth() {
        let _session = RSession::new();
        unsafe {
            R_SetEvalDepth(10);
            assert_eq!(R_GetEvalDepth(), 10);
            R_SetEvalDepth(0);
        }
    }

    #[test]
    fn test_session_main_state_is_local_on_same_thread() {
        let mut left = RSession::new();
        let mut right = RSession::new();

        left.with_arena(|_| unsafe {
            R_SetQuiet(1);
            R_SetInteractive(0);
            R_SetEvalDepth(12);
            R_SetPPStackTop(4);
            R_SetCollectWarnings(5);
            R_SetVisible(FALSE);
            assert_eq!(R_Quiet(), 1);
            assert_eq!(R_Interactive(), 0);
            assert_eq!(R_GetEvalDepth(), 12);
            assert_eq!(R_PPStackTop(), 4);
            assert_eq!(R_GetCollectWarnings(), 5);
            assert_eq!(R_GetVisible(), FALSE);
        })
        .unwrap();

        right
            .with_arena(|_| unsafe {
                assert_eq!(R_Quiet(), 0);
                assert_eq!(R_Interactive(), 1);
                assert_eq!(R_GetEvalDepth(), 0);
                assert_eq!(R_PPStackTop(), 0);
                assert_eq!(R_GetCollectWarnings(), 0);
                assert_eq!(R_GetVisible(), TRUE);
            })
            .unwrap();

        left.with_arena(|_| unsafe {
            assert_eq!(R_Quiet(), 1);
            assert_eq!(R_Interactive(), 0);
            assert_eq!(R_GetEvalDepth(), 12);
            assert_eq!(R_PPStackTop(), 4);
            assert_eq!(R_GetCollectWarnings(), 5);
            assert_eq!(R_GetVisible(), FALSE);
        })
        .unwrap();
    }
}
