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

/// Context-pointer derivation policy (the single rule for `*mut RCNTXT`).
///
/// Contexts live in instance-owned `Box<UnsafeCell<RCNTXT>>` allocations.
/// All pointers come from `UnsafeCell::get`, including the nextcontext chain.
/// Re-entering the evaluator never derives a new unique reference to a live
/// context. Popping its owning box invalidates its pointers.
fn stack_top_mut(instance: *mut RInstance) -> Option<*mut RCNTXT> {
    // SAFETY: `instance` is a live instance pointer from the caller; the
    // Vec access is strictly local and the derived pointer is handed to the
    // caller per the module policy.
    unsafe { (*instance).context_stack.last_mut().map(|ctx| ctx.get()) }
}

/// Get a reference to the current (top) context, if any.
pub unsafe fn R_GlobalContext() -> *mut RCNTXT {
    instance::with_current_instance(|instance| unsafe { R_GlobalContext_in(instance) })
        .unwrap_or(ptr::null_mut())
}

pub unsafe fn R_GlobalContext_in(instance: *mut RInstance) -> *mut RCNTXT {
    // Writable by policy: callers write through this pointer (sysparent
    // fixups in nextmethod.rs, conexit updates in R_run_onexits_*), so it
    // must be derived from the owning stack, never cast from `&RCNTXT`.
    // P2: read-only derivation from the Vec; no ambient write intervenes.
    stack_top_mut(instance).unwrap_or(ptr::null_mut())
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
    instance: *mut RInstance,
    callflag: c_int,
    call: SEXP,
    cloenv: SEXP,
    sysparent: SEXP,
    cfn: Option<unsafe extern "C" fn(*mut SexprecCore) -> *mut SexprecCore>,
    closure: SEXP,
    promiseargs: SEXP,
) -> *mut RCNTXT {
    let ctx = Box::new(std::cell::UnsafeCell::new(RCNTXT {
        callflag,
        call,
        cloenv,
        sysparent,
        cfn,
        callfun: closure,
        closure,
        promiseargs,
        ..RCNTXT::new()
    }));

    // P2: the short-lived reads/writes below are strictly local — pushing
    // a Box onto the Vec allocates but never reenters the interpreter, and
    // no ambient write occurs between them.
    let prev = unsafe {
        (*instance)
            .context_stack
            .last_mut()
            .map(|prev_ctx| prev_ctx.get())
            .unwrap_or(ptr::null_mut())
    };
    unsafe {
        (*ctx.get()).nextcontext = prev;
    }
    // `protectCount` snapshots the LEGACY protection stack depth at context
    // entry (the count-based discipline the translated endcontext/unwind
    // code pairs with); root-table slots are invisible to it by design.
    unsafe {
        (*ctx.get()).protectCount = (*instance).legacy_protect.len();
    }

    // Publish ownership before deriving the pointer. UnsafeCell permits
    // subsequent shared stack access without revoking interior mutation.
    unsafe {
        (*instance).context_stack.push(ctx);
        let top = (*instance).context_stack.last_mut().expect("just pushed");
        let ptr: *mut RCNTXT = top.get();
        ptr
    }
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
pub unsafe fn Rf_endcontext_in(instance: *mut RInstance, c: *mut RCNTXT) {
    // P2: strictly-local Vec access; no ambient write intervenes.
    unsafe {
        if let Some(top) = (*instance).context_stack.last() {
            // Address-only comparison: the pop below tears the Box down, so
            // no writable derivation is needed (or allowed) here.
            if top.get() == c {
                (*instance).context_stack.pop();
            }
        }
    }
}

/// RAII context guard bound to the instance that created the context.
pub struct ContextGuard {
    instance: *mut RInstance,
    context: *mut RCNTXT,
}

impl ContextGuard {
    /// The instance this guard is bound to.
    ///
    /// Exposed so tests (and future callers) can inspect the bound instance
    /// through the guard's OWN pointer: a fresh borrow of the underlying
    /// local would retag the allocation and pop the guard's borrow tag out
    /// from under its drop path (aliasing UB under Stacked Borrows).
    pub(crate) fn instance_ptr(&self) -> *mut RInstance {
        self.instance
    }

    pub fn context(&self) -> *mut RCNTXT {
        self.context
    }
}

impl Drop for ContextGuard {
    fn drop(&mut self) {
        unsafe {
            Rf_endcontext_in(self.instance, self.context);
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
        let instance_ptr = instance;
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
    instance: *mut RInstance,
    ctxt_type: c_int,
    cloenv: SEXP,
    _call: SEXP,
) -> *mut RCNTXT {
    unsafe {
        // Writable by policy: the returned context is a mutation target for
        // callers (jumped flags, returnValue), so derive from the owning
        // stack via &mut — never cast from the shared iteration view.
        for ctx in (*instance).context_stack.iter_mut().rev() {
            let c: *mut RCNTXT = ctx.get();
            let ctx_ref = &*c;
            if ctxt_type == 0 || (ctx_ref.callflag & ctxt_type) != 0 {
                if cloenv.is_null() || ctx_ref.cloenv == cloenv {
                    return c;
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
    instance::with_required_current_instance(|instance| unsafe {
        // P2: single-field write; no other raw path touches the instance
        // inside this closure.
        (*instance).in_error = flag;
    });
}

/// Get this session's in-error flag.
pub fn R_GetInError() -> bool {
    instance::with_required_current_instance(|instance| unsafe { (*instance).in_error })
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

/// Raise a canonical R error: panic with the [`RError`] payload.
///
/// This is the crate-wide entry point for raising R errors from Rust code.
/// Handlers catch the panic via `catch_unwind` and convert it into an
/// R-level error condition. Local `error()` helpers in other modules mirror
/// this exact behavior; prefer calling `r_error` (or migrating local helpers
/// to delegate to it) so there is one canonical raiser.
pub fn r_error(msg: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: msg.into(),
    });
}

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
    /// Warning condition unwinding into an enclosing tryCatch(...)
    /// exiting handler (port equivalent of upstream's vwarningcall
    /// unwinding through R_HandlerStack).  Only the message text crosses
    /// the unwind boundary; the catcher rebuilds the condition object.
    Warning {
        message: String,
    },
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

/// Run a loop body with a single `catch_unwind` context, matching upstream R's
/// one `setjmp` per loop rather than one per iteration.
pub unsafe fn run_hoisted_loop<F>(driver: F)
where
    F: FnMut(),
{
    unsafe {
        run_hoisted_loop_with_continue(driver, || ());
    }
}

/// Like [`run_hoisted_loop`], but runs `on_continue` before re-entering the
/// driver after a `next` signal. `for` loops use this to advance the index
/// before resuming, matching C R's jump to the increment clause.
pub unsafe fn run_hoisted_loop_with_continue<F, C>(mut driver: F, mut on_continue: C)
where
    F: FnMut(),
    C: FnMut(),
{
    loop {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(&mut driver));
        match result {
            Ok(()) => break,
            Err(payload) => match handle_loop_signal(payload) {
                LoopAction::Break => break,
                LoopAction::Continue => {
                    on_continue();
                    continue;
                }
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
    instance::with_current_instance(|instance| unsafe {
        // P2: read-only scan of one field; no ambient write intervenes.
        (*instance)
            .context_stack
            .iter()
            .rev()
            .any(|ctx| (*ctx.get()).cloenv == target_env)
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
        let left = RSession::new();
        let right = RSession::new();

        let left_ctx = left.with_active(|| unsafe {
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
        });

        right.with_active(|| unsafe {
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
        });

        left.with_active(|| unsafe {
            assert_eq!(R_GlobalContext(), left_ctx);
            let found = Rf_findcontext(ctxt_flags::CTXT_FUNCTION, ptr::null_mut(), ptr::null_mut());
            assert_eq!(found, left_ctx);
            Rf_endcontext(left_ctx);
            assert!(R_GlobalContext().is_null());
        });
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

            // Mid-guard read goes through the guard's OWN instance pointer:
            // a fresh `&mut left`/`&left` borrow here would retag the
            // allocation and pop the guard's borrow tag out from under its
            // drop path (aliasing UB under Stacked Borrows).
            assert_eq!(unsafe { (*guard.instance_ptr()).context_stack.len() }, 1);
            assert!(right.context_stack.is_empty());

            replace_current_instance(Some(&mut right as *mut RInstance));
            drop(guard);

            assert!(left.context_stack.is_empty());
            assert!(right.context_stack.is_empty());
            replace_current_instance(previous);
        }
    }

    #[test]
    fn test_context_mutation_goes_through_stack_derived_pointers() {
        // The mutation policy in action: a caller writes a context field
        // (sysparent, as nextmethod.rs does) through the pointer handed out
        // by R_GlobalContext_in — which the module derives from the owning
        // Vec<Box<UnsafeCell<RCNTXT>>>, through UnsafeCell::get. The write must be
        // observable through the stack and survive later derivations.
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let c = Rf_begincontext(
                ctxt_flags::CTXT_FUNCTION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                None,
                ptr::null_mut(),
                ptr::null_mut(),
            );
            let global = R_GlobalContext();
            assert_eq!(global, c);
            // Audited write site pattern: (*ctx).sysparent = value.
            (*global).sysparent = 0x42 as SEXP;
            assert_eq!((*global).sysparent, 0x42 as SEXP);
            // A re-derived pointer observes the same mutation (single
            // storage; no `&`-cast copy).
            assert_eq!((*R_GlobalContext()).sysparent, 0x42 as SEXP);
            let found = Rf_findcontext(ctxt_flags::CTXT_FUNCTION, ptr::null_mut(), ptr::null_mut());
            assert_eq!((*found).sysparent, 0x42 as SEXP);

            Rf_endcontext(c);
        });
    }

    #[test]
    fn test_context_teardown_invalidates_derived_pointers() {
        // Policy: Rf_endcontext pops the owning Box, so every pointer
        // derived from it is dead. The module never hands that context out
        // again: the next R_GlobalContext/findcontext derivation returns a
        // different (or null) pointer, and endcontext of a stale pointer is
        // a no-op (top-of-stack comparison fails).
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
                ctxt_flags::CTXT_LOOP,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                None,
                ptr::null_mut(),
                ptr::null_mut(),
            );
            Rf_endcontext(c2);
            // The popped context is gone from every derivation path.
            assert_eq!(R_GlobalContext(), c1);
            let found = Rf_findcontext(ctxt_flags::CTXT_LOOP, ptr::null_mut(), ptr::null_mut());
            assert!(found.is_null());
            // Ending a stale (already-popped) pointer must not pop c1: the
            // top-of-stack comparison is address-only and fails.
            Rf_endcontext(c2);
            assert_eq!(R_GlobalContext(), c1);
            Rf_endcontext(c1);
            assert!(R_GlobalContext().is_null());
        });
    }

    #[test]
    fn test_session_in_error_flags_are_local_on_same_thread() {
        let left = RSession::new();
        let right = RSession::new();

        left.with_active(|| {
            R_SetInError(true);
            assert!(R_GetInError());
        });

        right.with_active(|| {
            assert!(!R_GetInError());
            R_SetInError(false);
            assert!(!R_GetInError());
        });

        left.with_active(|| {
            assert!(R_GetInError());
            R_SetInError(false);
            assert!(!R_GetInError());
        });
    }
}
