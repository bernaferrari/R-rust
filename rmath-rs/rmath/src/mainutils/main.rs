#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/main.c — main REPL and global variable definitions.
//!
//! Provides Rf_mainloop(), R_ReplFile(), Rf_ReplIteration(), and other
//! core REPL functions. Currently stubbed since it depends on the parser,
//! readline, and the full evaluation system.

use std::cell::Cell;
use std::os::raw::c_int;
use std::ptr;

use crate::sexp::ffi::{FALSE, SEXP, TRUE};
use crate::sexp::globals::{R_NilValue, R_Visible, set_R_Visible};
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Global REPL state
// ---------------------------------------------------------------------------

thread_local! { static R_NoEcho_val: Cell<c_int> = Cell::new(0); }
thread_local! { static R_Quiet_val: Cell<c_int> = Cell::new(0); }
thread_local! { static R_Interactive_val: Cell<c_int> = Cell::new(1); }
thread_local! { static R_Verbose_val: Cell<c_int> = Cell::new(0); }

/// Console buffer size.
pub const CONSOLE_BUFFER_SIZE: usize = 1024;

thread_local! { static R_CurrentExpr: Cell<SEXP> = Cell::new(ptr::null_mut()); }

// ---------------------------------------------------------------------------
// Global symbols (initialized lazily)
// ---------------------------------------------------------------------------

/// Get the .Last.value symbol.
pub unsafe fn R_LastvalueSymbol() -> SEXP {
    unsafe {
        Rf_install(
            std::ffi::CString::new(".Last.value")
                .expect("CString::new failed: contains null byte")
                .as_ptr(),
        )
    }
}

/// Get the .Random.seed symbol.
pub unsafe fn R_SeedsSymbol() -> SEXP {
    unsafe {
        Rf_install(
            std::ffi::CString::new(".Random.seed")
                .expect("CString::new failed: contains null byte")
                .as_ptr(),
        )
    }
}

// ---------------------------------------------------------------------------
// R_Visible accessor
// ---------------------------------------------------------------------------

/// Get R_Visible flag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetVisible() -> c_int {
    unsafe { if R_Visible() != 0 { TRUE } else { FALSE } }
}

/// Set R_Visible flag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SetVisible(v: c_int) {
    unsafe {
        set_R_Visible(v);
    }
}

/// Get R_Interactive flag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_Interactive() -> c_int {
    R_Interactive_val.with(|v| v.get())
}

/// Set R_Interactive flag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SetInteractive(v: c_int) {
    R_Interactive_val.with(|c| c.set(v));
}

/// Get R_Quiet flag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_Quiet() -> c_int {
    R_Quiet_val.with(|v| v.get())
}

/// Set R_Quiet flag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SetQuiet(v: c_int) {
    R_Quiet_val.with(|c| c.set(v));
}

/// Get R_NoEcho flag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_NoEcho() -> c_int {
    R_NoEcho_val.with(|v| v.get())
}

/// Get R_Verbose flag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_Verbose() -> c_int {
    R_Verbose_val.with(|v| v.get())
}

// ---------------------------------------------------------------------------
// R_EvalDepth
// ---------------------------------------------------------------------------

thread_local! { static R_EvalDepth_val: Cell<c_int> = Cell::new(0); }

/// Get evaluation depth.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetEvalDepth() -> c_int {
    R_EvalDepth_val.with(|v| v.get())
}

/// Set evaluation depth.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SetEvalDepth(v: c_int) {
    R_EvalDepth_val.with(|c| c.set(v));
}

// ---------------------------------------------------------------------------
// R_PPStackTop
// ---------------------------------------------------------------------------

thread_local! { static R_PPStackTop_val: Cell<c_int> = Cell::new(0); }

/// Get protection stack top.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_PPStackTop() -> c_int {
    R_PPStackTop_val.with(|v| v.get())
}

/// Set protection stack top.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SetPPStackTop(v: c_int) {
    R_PPStackTop_val.with(|c| c.set(v));
}

// ---------------------------------------------------------------------------
// Warnings
// ---------------------------------------------------------------------------

thread_local! { static R_CollectWarnings: Cell<c_int> = Cell::new(0); }

/// Get warnings collection flag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetCollectWarnings() -> c_int {
    R_CollectWarnings.with(|v| v.get())
}

/// Set warnings collection flag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SetCollectWarnings(v: c_int) {
    R_CollectWarnings.with(|c| c.set(v));
}

// ---------------------------------------------------------------------------
// Time limits (stubs)
// ---------------------------------------------------------------------------

pub unsafe fn resetTimeLimits() {
    // Stub
}

pub unsafe fn checkTimeLimits() {
    // Stub
}

// ---------------------------------------------------------------------------
// SrcRef state (stubs)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_InitSrcRefState(_cntxt: *mut std::ffi::c_void) {
    // Stub
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_FinalizeSrcRefState() {
    // Stub
}

// ---------------------------------------------------------------------------
// Parse status
// ---------------------------------------------------------------------------

pub const PARSE_OK: c_int = 0;
pub const PARSE_INCOMPLETE: c_int = 1;
pub const PARSE_ERROR: c_int = 2;
pub const PARSE_EOF: c_int = 3;
pub const PARSE_NULL: c_int = 4;

thread_local! { static R_ParseErrorMsg: Cell<[u8; 256]> = Cell::new([0; 256]); }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetParseErrorMsg() -> *const std::os::raw::c_char {
    R_ParseErrorMsg.with(|v| std::ptr::addr_of!(*v).cast::<std::os::raw::c_char>())
}

// ---------------------------------------------------------------------------
// R_ReplFile — REPL from file (stub)
// ---------------------------------------------------------------------------

/// Run the REPL reading from a file.
///
/// This is the equivalent of R's `R_ReplFile()` from main.c.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ReplFile(_fp: *mut std::ffi::c_void, _rho: SEXP) {
    // Stub: in the full implementation, this reads expressions from the file
    // and evaluates them
}

// ---------------------------------------------------------------------------
// Rf_ReplIteration — single REPL iteration (stub)
// ---------------------------------------------------------------------------

/// Perform a single REPL iteration.
///
/// This is the equivalent of R's `Rf_ReplIteration()` from main.c.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_ReplIteration(
    _rho: SEXP,
    _savestack: c_int,
    _browselevel: c_int,
    _state: *mut std::ffi::c_void,
) -> c_int {
    // Stub: return 0 (continue)
    0
}

// ---------------------------------------------------------------------------
// Rf_ReplConsole — interactive REPL (stub)
// ---------------------------------------------------------------------------

/// Run the interactive REPL.
///
/// This is the equivalent of R's `Rf_ReplConsole()` from main.c.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_ReplConsole(_rho: SEXP, _savestack: c_int, _browselevel: c_int) {
    // Stub: in the full implementation, this runs the interactive REPL loop
}

// ---------------------------------------------------------------------------
// Rf_mainloop — main R loop (stub)
// ---------------------------------------------------------------------------

/// The main R read-eval-print loop.
///
/// This is the equivalent of R's `Rf_mainloop()` from main.c.
/// Called from main() after Rf_initialize_R().
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_mainloop() {
    // Stub: in the full implementation, this sets up and runs the REPL
    eprintln!("Rf_mainloop() called (stub — no REPL implementation yet)");
}

// ---------------------------------------------------------------------------
// R_Parse1File — parse one expression from file (stub)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_Parse1File(
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
    // Stub
}

// ---------------------------------------------------------------------------
// Top-level handlers (stubs)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_callToplevelHandlers(
    _expr: SEXP,
    _value: SEXP,
    _succeeded: c_int,
    _visible: c_int,
) {
    // Stub
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_addTaskCallback(_fun: SEXP, _data: SEXP) -> c_int {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_removeTaskCallback(_name: SEXP) -> c_int {
    0
}

// ---------------------------------------------------------------------------
// Memory profiling (stubs)
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetMaxVSize() -> u64 {
    u64::MAX
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetMaxNSize() -> u64 {
    u64::MAX
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetVSize() -> u64 {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_GetNSize() -> u64 {
    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
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
        unsafe {
            assert_eq!(R_Quiet(), 0);
            R_SetQuiet(1);
            assert_eq!(R_Quiet(), 1);
            R_SetQuiet(0);
        }
    }

    #[test]
    fn test_r_interactive() {
        unsafe {
            assert_eq!(R_Interactive(), 1);
            R_SetInteractive(0);
            assert_eq!(R_Interactive(), 0);
            R_SetInteractive(1);
        }
    }

    #[test]
    fn test_eval_depth() {
        unsafe {
            R_SetEvalDepth(10);
            assert_eq!(R_GetEvalDepth(), 10);
            R_SetEvalDepth(0);
        }
    }
}
