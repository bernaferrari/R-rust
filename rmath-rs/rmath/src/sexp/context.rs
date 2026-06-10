#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! R execution context (RCNTXT) and context stack.
//!
//! Ports R's src/main/context.c — the context management system used for
//! error handling, longjmp-based unwinding, and interpreter state tracking.
//!
//! In C, R uses setjmp/longjmp for non-local exits. In Rust, we use
//! `std::panic::catch_unwind` with a custom `RError` panic payload.

use std::os::raw::c_int;
use std::ptr;
use std::sync::OnceLock;

use super::ffi::{SEXP, SexprecCore};
use super::instance;
use super::instance::RInstance;

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
    pub srcref: SEXP,
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
            srcref: ptr::null_mut(),
        }
    }
}

impl Default for RCNTXT {
    fn default() -> Self {
        Self::new()
    }
}

fn context_ptr(ctx: &RCNTXT) -> *mut RCNTXT {
    ctx as *const RCNTXT as *mut RCNTXT
}

/// Get a reference to the current (top) context, if any.
pub unsafe fn R_GlobalContext() -> *mut RCNTXT {
    instance::with_current_instance(|instance| unsafe { R_GlobalContext_in(instance) })
        .unwrap_or(ptr::null_mut())
}

/// Get the top context from an explicit runtime instance.
pub unsafe fn R_GlobalContext_in(instance: &mut RInstance) -> *mut RCNTXT {
    instance
        .context_stack
        .last()
        .map(|ctx| context_ptr(ctx))
        .unwrap_or(ptr::null_mut())
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
    instance::with_required_current_instance(|instance| unsafe {
        Rf_begincontext_in(
            instance,
            callflag,
            call,
            cloenv,
            sysparent,
            cfn,
            closure,
            promiseargs,
        )
    })
}

/// Push a context onto an explicit runtime instance.
pub unsafe fn Rf_begincontext_in(
    instance: &mut RInstance,
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
        callfun: closure,
        closure,
        promiseargs,
        ..RCNTXT::new()
    });

    let prev = instance
        .context_stack
        .last()
        .map(|prev_ctx| context_ptr(prev_ctx))
        .unwrap_or(ptr::null_mut());
    ctx.nextcontext = prev;
    ctx.protectCount = instance.protect_stack.borrow().len();

    let ptr: *mut RCNTXT = &mut *ctx;
    instance.context_stack.push(ctx);
    ptr
}

/// Pop the top context from the stack.
///
/// This is the equivalent of R's `endcontext()`.
pub unsafe fn Rf_endcontext(c: *mut RCNTXT) {
    instance::with_required_current_instance(|instance| unsafe {
        Rf_endcontext_in(instance, c);
    });
}

/// Pop the top context from an explicit runtime instance.
pub unsafe fn Rf_endcontext_in(instance: &mut RInstance, c: *mut RCNTXT) {
    if let Some(top) = instance.context_stack.last() {
        let top_ptr = context_ptr(top);
        if top_ptr == c {
            instance.context_stack.pop();
        }
    }
}

/// RAII context guard bound to the instance that created the context.
pub struct ContextGuard {
    instance: *mut RInstance,
    context: *mut RCNTXT,
}

impl ContextGuard {
    pub fn context(&self) -> *mut RCNTXT {
        self.context
    }
}

impl Drop for ContextGuard {
    fn drop(&mut self) {
        unsafe {
            Rf_endcontext_in(&mut *self.instance, self.context);
        }
    }
}

/// Push a context on the active instance and return an owner-bound guard.
pub unsafe fn begin_context_guard(
    callflag: c_int,
    call: SEXP,
    cloenv: SEXP,
    sysparent: SEXP,
    cfn: Option<unsafe extern "C" fn(*mut SexprecCore) -> *mut SexprecCore>,
    closure: SEXP,
    promiseargs: SEXP,
) -> ContextGuard {
    instance::with_required_current_instance(|instance| unsafe {
        let instance_ptr = instance as *mut RInstance;
        let context = Rf_begincontext_in(
            instance,
            callflag,
            call,
            cloenv,
            sysparent,
            cfn,
            closure,
            promiseargs,
        );
        ContextGuard {
            instance: instance_ptr,
            context,
        }
    })
}

/// Find a context of the given type, searching from the top of the stack.
///
/// This is the equivalent of R's `findcontext()`.
pub unsafe fn Rf_findcontext(ctxt_type: c_int, cloenv: SEXP, call: SEXP) -> *mut RCNTXT {
    instance::with_required_current_instance(|instance| unsafe {
        Rf_findcontext_in(instance, ctxt_type, cloenv, call)
    })
}

/// Find a context on an explicit runtime instance.
pub unsafe fn Rf_findcontext_in(
    instance: &mut RInstance,
    ctxt_type: c_int,
    cloenv: SEXP,
    _call: SEXP,
) -> *mut RCNTXT {
    unsafe {
        for ctx in instance.context_stack.iter().rev() {
            let c = context_ptr(ctx);
            if !c.is_null() {
                let ctx_ref = &*c;
                if ctxt_type == 0 || (ctx_ref.callflag & ctxt_type) != 0 {
                    if cloenv.is_null() || ctx_ref.cloenv == cloenv {
                        return c;
                    }
                }
            }
        }
        ptr::null_mut()
    }
}

// ---------------------------------------------------------------------------
// Session state
// ---------------------------------------------------------------------------

/// Set this session's in-error flag.
pub fn R_SetInError(flag: bool) {
    instance::with_required_current_instance(|instance| {
        instance.in_error = flag;
    });
}

/// Get this session's in-error flag.
pub fn R_GetInError() -> bool {
    instance::with_required_current_instance(|instance| instance.in_error)
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
    Error {
        message: String,
    },
    Break,
    Next,
    Return(SEXP),
    /// Non-local return from `invokeRestart()` to the matching `withRestarts()`.
    Restart(SEXP),
    /// Targeted context jump for exiting handlers (tryCatch/withCallingHandlers).
    /// Carries the target environment to match against context stack entries,
    /// and the result vector containing [cond, call, handler].
    ExitingHandler {
        target_env: SEXP,
        result: SEXP,
    },
}

unsafe impl Send for RSignal {}

static R_PANIC_HOOK: OnceLock<()> = OnceLock::new();

/// Suppress stderr noise for Rust panics used as R control-flow signals.
///
/// Real Rust panics still go through the previously installed hook.
pub fn install_r_panic_hook() {
    R_PANIC_HOOK.get_or_init(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if info.payload().downcast_ref::<RSignal>().is_some()
                || info.payload().downcast_ref::<RError>().is_some()
            {
                return;
            }
            default_hook(info);
        }));
    });
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoopAction {
    Break,
    Continue,
}

/// Try to handle a loop body panic. Returns LoopAction for break/next,
/// re-panics for other signals.
/// Run a loop body with a single `catch_unwind` context, matching upstream R's
/// one `setjmp` per loop rather than one per iteration.
pub unsafe fn run_hoisted_loop<F>(mut driver: F)
where
    F: FnMut() + std::panic::UnwindSafe,
{
    loop {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(&mut driver));
        match result {
            Ok(()) => break,
            Err(payload) => match handle_loop_signal(payload) {
                LoopAction::Break => break,
                LoopAction::Continue => continue,
            },
        }
    }
}

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

/// Check whether any context on the stack has `cloenv == target_env`.
/// Used by ExitingHandler signal handling to determine if the current
/// catch_unwind frame is the intended target.
pub fn context_env_exists(target_env: SEXP) -> bool {
    instance::with_current_instance(|instance| {
        instance
            .context_stack
            .iter()
            .rev()
            .any(|ctx| ctx.cloenv == target_env)
    })
    .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::instance::{RInstance, replace_current_instance};
    use crate::sexp::session::RSession;

    #[test]
    fn test_rcntxt_new() {
        let ctx = RCNTXT::new();
        assert_eq!(ctx.callflag, 0);
        assert!(ctx.call.is_null());
    }

    #[test]
    fn test_context_push_pop() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
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
        });
    }

    #[test]
    fn test_context_nested() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
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
        });
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
        let session = RSession::new();
        session.with_protected(|| unsafe {
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
        });
    }

    #[test]
    fn test_session_context_stacks_are_local_on_same_thread() {
        let mut left = RSession::new();
        let mut right = RSession::new();

        let left_ctx = left
            .with_arena(|_| unsafe {
                let ctx = Rf_begincontext(
                    ctxt_flags::CTXT_FUNCTION,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    None,
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
                assert_eq!(R_GlobalContext(), ctx);
                ctx
            })
            .unwrap();

        right
            .with_arena(|_| unsafe {
                assert!(R_GlobalContext().is_null());
                let right_ctx = Rf_begincontext(
                    ctxt_flags::CTXT_LOOP,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    None,
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
                assert_eq!(R_GlobalContext(), right_ctx);
                let found = Rf_findcontext(ctxt_flags::CTXT_LOOP, ptr::null_mut(), ptr::null_mut());
                assert_eq!(found, right_ctx);
                Rf_endcontext(right_ctx);
                assert!(R_GlobalContext().is_null());
            })
            .unwrap();

        left.with_arena(|_| unsafe {
            assert_eq!(R_GlobalContext(), left_ctx);
            let found = Rf_findcontext(ctxt_flags::CTXT_FUNCTION, ptr::null_mut(), ptr::null_mut());
            assert_eq!(found, left_ctx);
            Rf_endcontext(left_ctx);
            assert!(R_GlobalContext().is_null());
        })
        .unwrap();
    }

    #[test]
    fn test_context_guard_drops_against_original_instance() {
        let mut left = RInstance::new();
        let mut right = RInstance::new();

        unsafe {
            let previous = replace_current_instance(Some(&mut left as *mut RInstance));
            let guard = begin_context_guard(
                ctxt_flags::CTXT_FUNCTION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                None,
                ptr::null_mut(),
                ptr::null_mut(),
            );

            assert_eq!(left.context_stack.len(), 1);
            assert!(right.context_stack.is_empty());

            replace_current_instance(Some(&mut right as *mut RInstance));
            drop(guard);

            assert!(left.context_stack.is_empty());
            assert!(right.context_stack.is_empty());
            replace_current_instance(previous);
        }
    }

    #[test]
    fn test_session_in_error_flags_are_local_on_same_thread() {
        let mut left = RSession::new();
        let mut right = RSession::new();

        left.with_arena(|_| {
            R_SetInError(true);
            assert!(R_GetInError());
        })
        .unwrap();

        right
            .with_arena(|_| {
                assert!(!R_GetInError());
                R_SetInError(false);
                assert!(!R_GetInError());
            })
            .unwrap();

        left.with_arena(|_| {
            assert!(R_GetInError());
            R_SetInError(false);
            assert!(!R_GetInError());
        })
        .unwrap();
    }
}
