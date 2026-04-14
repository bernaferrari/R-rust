#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! R execution context (RCNTXT) and context stack.
//!
//! Ports R's src/main/context.c — the context management system used for
//! error handling, longjmp-based unwinding, and interpreter state tracking.
//!
//! In C, R uses setjmp/longjmp for non-local exits. In Rust, we use
//! `std::panic::catch_unwind` with a custom `RError` panic payload.

use std::cell::RefCell;
use std::os::raw::c_int;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use super::ffi::{SEXP, SexprecCore};

// ---------------------------------------------------------------------------
// Context type constants (from Defn.h CTXT_* defines)
// ---------------------------------------------------------------------------

/// Context types matching R's CTXT_* defines.
pub mod ctxt_flags {
    pub const CTXT_TOPLEVEL: i32 = 0;
    pub const CTXT_FUNCTION: i32 = 1;
    pub const CTXT_CCODE: i32 = 2;
    pub const CTXT_LOOP: i32 = 16;
    pub const CTXT_BUILTIN: i32 = 32;
    pub const CTXT_GENERIC: i32 = 64;
    pub const CTXT_RETURN: i32 = 128;
    pub const CTXT_BROWSER: i32 = 256;
    pub const CTXT_DEBUG: i32 = 512;
}

// ---------------------------------------------------------------------------
// RCNTXT — the execution context structure
// ---------------------------------------------------------------------------

/// R's execution context node.
///
/// This is the Rust equivalent of R's `RCNTXT` struct from Defn.h.
/// It tracks the state needed for error handling, loop control flow,
/// and function call boundaries.
#[repr(C)]
pub struct RCNTXT {
    /// Context type flags (CTXT_TOPLEVEL, CTXT_FUNCTION, etc.)
    pub cstackbase: *mut u8,
    /// Type of context (function, loop, toplevel, etc.)
    pub callflag: c_int,
    /// The call being evaluated (for error reporting)
    pub call: SEXP,
    /// The closure/environment for function contexts
    pub cloenv: SEXP,
    pub sysparent: SEXP,
    pub callfun: SEXP,
    /// Function to call on exit (on.exit handlers)
    pub cfn: Option<unsafe extern "C" fn(*mut SexprecCore) -> *mut SexprecCore>,
    /// Closure being evaluated
    pub closure: SEXP,
    /// Promises for arguments
    pub promiseargs: SEXP,
    /// Old working directory (saved on entry)
    pub savelist: SEXP,
    /// Handler for conditions
    pub handlerstack: SEXP,
    /// Restart stack
    pub restartstack: SEXP,
    /// Flag: are we in the middle of a browser?
    pub browserflag: c_int,
    /// Global evaluation depth limit
    pub evaldepth: c_int,
    /// Pointer to the previous context on the context stack
    pub nextcontext: *mut RCNTXT,
    /// Flag: interrupt check pending
    pub intactive: c_int,
    /// Flag: whether this context has been jumped to
    pub jumped: c_int,
    /// The R vector version counter (for ALTREP)
    pub rpvec: SEXP,
    /// Vector clock
    pub rpvbase: usize,
    /// Return value
    pub returnValue: SEXP,
    /// Number of protect entries at context entry
    pub protectCount: usize,
    /// on.exit expression list (conexit in R)
    pub conexit: SEXP,
    /// cleanup function pointer (cend in R)
    pub cend: Option<unsafe extern "C" fn(*mut std::os::raw::c_void)>,
    /// cleanup function data (cenddata in R)
    pub cenddata: *mut std::os::raw::c_void,
}

impl RCNTXT {
    /// Create a new (zeroed) context.
    pub fn new() -> Self {
        RCNTXT {
            cstackbase: ptr::null_mut(),
            callflag: 0,
            call: ptr::null_mut(),
            cloenv: ptr::null_mut(),
            sysparent: ptr::null_mut(),
            callfun: ptr::null_mut(),
            cfn: None,
            closure: ptr::null_mut(),
            promiseargs: ptr::null_mut(),
            savelist: ptr::null_mut(),
            handlerstack: ptr::null_mut(),
            restartstack: ptr::null_mut(),
            browserflag: 0,
            evaldepth: 0,
            nextcontext: ptr::null_mut(),
            intactive: 0,
            jumped: 0,
            rpvec: ptr::null_mut(),
            rpvbase: 0,
            returnValue: ptr::null_mut(),
            protectCount: 0,
            conexit: ptr::null_mut(),
            cend: None,
            cenddata: ptr::null_mut(),
        }
    }
}

impl Default for RCNTXT {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Thread-local context stack
// ---------------------------------------------------------------------------

thread_local! {
    /// The thread-local context stack.
    #[allow(clippy::vec_box)]
    static CONTEXT_STACK: RefCell<Vec<Box<RCNTXT>>> = RefCell::new(Vec::new());
}

/// Get a reference to the current (top) context, if any.
pub unsafe fn R_GlobalContext() -> *mut RCNTXT {
    CONTEXT_STACK.with(|stack| {
        let stack = stack.borrow();
        if let Some(ctx) = stack.last() {
            &**ctx as *const RCNTXT as *mut RCNTXT
        } else {
            ptr::null_mut()
        }
    })
}

/// Push a new context onto the stack and return a mutable pointer to it.
///
/// This is the equivalent of R's `begincontext()`.
pub unsafe fn Rf_begincontext(
    callflag: c_int,
    call: SEXP,
    cloenv: SEXP,
    sysparent: SEXP,
    cfn: Option<unsafe extern "C" fn(*mut SexprecCore) -> *mut SexprecCore>,
    closure: SEXP,
    promiseargs: SEXP,
) -> *mut RCNTXT {
    let mut ctx = Box::new(RCNTXT {
        callflag,
        call,
        cloenv,
        sysparent,
        cfn,
        closure,
        promiseargs,
        ..RCNTXT::new()
    });

    // Link to previous context
    CONTEXT_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        let prev = if let Some(prev_ctx) = stack.last() {
            &**prev_ctx as *const RCNTXT as *mut RCNTXT
        } else {
            ptr::null_mut()
        };
        ctx.nextcontext = prev;

        // Record protect count at entry
        ctx.protectCount = super::protect::R_ProtectCount();

        let ptr: *mut RCNTXT = &mut *ctx;
        stack.push(ctx);
        ptr
    })
}

/// Pop the top context from the stack.
///
/// This is the equivalent of R's `endcontext()`.
pub unsafe fn Rf_endcontext(c: *mut RCNTXT) {
    CONTEXT_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        if let Some(top) = stack.last() {
            let top_ptr: *mut RCNTXT = &**top as *const RCNTXT as *mut RCNTXT;
            if top_ptr == c {
                stack.pop();
            }
        }
    });
}

/// Find a context of the given type, searching from the top of the stack.
///
/// This is the equivalent of R's `findcontext()`.
pub unsafe fn Rf_findcontext(ctxt_type: c_int, cloenv: SEXP, call: SEXP) -> *mut RCNTXT {
    unsafe {
        CONTEXT_STACK.with(|stack| {
            let stack = stack.borrow();
            for ctx in stack.iter().rev() {
                let c: *mut RCNTXT = &**ctx as *const RCNTXT as *mut RCNTXT;
                if !c.is_null() {
                    let ctx_ref = &*c;
                    // Match by type
                    if ctxt_type == 0 || (ctx_ref.callflag & ctxt_type) != 0 {
                        // If cloenv specified, match environment
                        if cloenv.is_null() || ctx_ref.cloenv == cloenv {
                            return c;
                        }
                    }
                }
            }
            ptr::null_mut()
        })
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Whether an error is currently being handled.
static IN_ERROR: AtomicBool = AtomicBool::new(false);

/// Set the global in-error flag.
pub fn R_SetInError(flag: bool) {
    IN_ERROR.store(flag, Ordering::Relaxed);
}

/// Get the global in-error flag.
pub fn R_GetInError() -> bool {
    IN_ERROR.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// RError — custom panic payload for R error handling
// ---------------------------------------------------------------------------

/// Custom error type used as a panic payload for R error handling.
///
/// This replaces C's longjmp mechanism. When R code calls `error()`,
/// we panic with this payload. Callers use `catch_unwind` to catch it.
#[derive(Debug, Clone)]
pub struct RError {
    /// The error message.
    pub message: String,
}

impl std::fmt::Display for RError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "R error: {}", self.message)
    }
}

impl std::error::Error for RError {}

// ---------------------------------------------------------------------------
// RSignal — discriminated control flow signals
// ---------------------------------------------------------------------------

/// Discriminated R evaluation signal.
///
/// Replaces the undifferentiated `RError` panic payload for control flow.
/// Each variant represents a distinct R control flow mechanism.
#[derive(Debug)]
pub enum RSignal {
    Error { message: String },
    Break,
    Next,
    Return(SEXP),
}

unsafe impl Send for RSignal {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoopAction {
    Break,
    Continue,
}

/// Try to handle a loop body panic. Returns LoopAction for break/next,
/// re-panics for other signals.
pub fn handle_loop_signal(payload: Box<dyn std::any::Any + Send>) -> LoopAction {
    match payload.downcast::<RSignal>() {
        Ok(signal) => match *signal {
            RSignal::Break => LoopAction::Break,
            RSignal::Next => LoopAction::Continue,
            other => std::panic::panic_any(other),
        },
        Err(payload) => match payload.downcast::<RError>() {
            Ok(err) => std::panic::panic_any(RSignal::Error {
                message: err.message.clone(),
            }),
            Err(payload) => std::panic::resume_unwind(payload),
        },
    }
}

pub fn handle_closure_signal(payload: Box<dyn std::any::Any + Send>) -> SEXP {
    match payload.downcast::<RSignal>() {
        Ok(signal) => match *signal {
            RSignal::Return(val) => val,
            other => std::panic::panic_any(other),
        },
        Err(payload) => match payload.downcast::<RError>() {
            Ok(err) => std::panic::panic_any(RSignal::Error {
                message: err.message.clone(),
            }),
            Err(payload) => std::panic::resume_unwind(payload),
        },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rcntxt_new() {
        let ctx = RCNTXT::new();
        assert_eq!(ctx.callflag, 0);
        assert!(ctx.call.is_null());
    }

    #[test]
    fn test_context_push_pop() {
        unsafe {
            let c = Rf_begincontext(
                ctxt_flags::CTXT_TOPLEVEL,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                None,
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert!(!c.is_null());

            let top = R_GlobalContext();
            assert_eq!(top, c);

            Rf_endcontext(c);
        }
    }

    #[test]
    fn test_context_nested() {
        unsafe {
            let c1 = Rf_begincontext(
                ctxt_flags::CTXT_TOPLEVEL,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                None,
                ptr::null_mut(),
                ptr::null_mut(),
            );
            let c2 = Rf_begincontext(
                ctxt_flags::CTXT_FUNCTION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                None,
                ptr::null_mut(),
                ptr::null_mut(),
            );

            let top = R_GlobalContext();
            assert_eq!(top, c2);

            Rf_endcontext(c2);
            let top = R_GlobalContext();
            assert_eq!(top, c1);

            Rf_endcontext(c1);
        }
    }

    #[test]
    fn test_r_error_display() {
        let err = RError {
            message: "test error".to_string(),
        };
        assert_eq!(format!("{}", err), "R error: test error");
    }

    #[test]
    fn test_findcontext() {
        unsafe {
            // Clear any leftover contexts from previous tests
            CONTEXT_STACK.with(|stack| stack.borrow_mut().clear());

            let c1 = Rf_begincontext(
                ctxt_flags::CTXT_TOPLEVEL,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                None,
                ptr::null_mut(),
                ptr::null_mut(),
            );
            let c2 = Rf_begincontext(
                ctxt_flags::CTXT_FUNCTION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                None,
                ptr::null_mut(),
                ptr::null_mut(),
            );

            // Find the function context by flag
            let found = Rf_findcontext(ctxt_flags::CTXT_FUNCTION, ptr::null_mut(), ptr::null_mut());
            assert_eq!(found, c2);

            // Find with type 0 matches ANY context (returns topmost)
            let found = Rf_findcontext(0, ptr::null_mut(), ptr::null_mut());
            assert_eq!(found, c2);

            Rf_endcontext(c2);

            // Now only c1 remains
            let found = Rf_findcontext(ctxt_flags::CTXT_TOPLEVEL, ptr::null_mut(), ptr::null_mut());
            // CTXT_TOPLEVEL is 0, matches any, so finds c1
            assert_eq!(found, c1);

            Rf_endcontext(c1);

            CONTEXT_STACK.with(|stack| stack.borrow_mut().clear());
        }
    }
}
