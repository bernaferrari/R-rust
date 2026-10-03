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
use std::rc::Rc;
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

/// The actual owning value of an execution-context field.
///
/// A field owns the original allocation lease. Raw SEXP readers borrow a
/// projection from this value; the collector never reconstructs ownership by
/// scanning or remapping its address. Replacement acquires the new lease first,
/// so validation failure leaves the previous value intact.
#[derive(Default, Debug)]
pub struct ContextValue(Option<super::object::Sexp<'static>>);

impl ContextValue {
    pub fn empty() -> Self {
        Self(None)
    }

    pub fn as_raw(&self) -> SEXP {
        self.0
            .as_ref()
            .map_or(ptr::null_mut(), |value| value.as_raw())
    }

    pub fn is_null(&self) -> bool {
        self.0.is_none()
    }

    pub(crate) fn owned(&self) -> Option<super::object::Sexp<'static>> {
        self.0.clone()
    }

    /// Capture an already-proven raw value at the translated owner boundary.
    /// No interpreter callback executes while constructing the field.
    ///
    /// # Safety
    /// `instance` is the original live writable owner; `value` is either null
    /// or its live initialized projection, with no overlapping payload loans.
    pub(crate) unsafe fn from_raw_in(instance: *mut RInstance, value: SEXP) -> Self {
        if value.is_null() {
            return Self::empty();
        }
        let owner = unsafe { super::owner::OwnerToken::from_raw(instance) };
        let value = owner
            .sexp(value)
            .and_then(|value| value.into_owned())
            .unwrap_or_else(|error| r_error(format!("invalid context value: {error}")));
        Self(Some(value))
    }

    /// # Safety
    /// The owner and projection requirements of `from_raw_in` apply. The
    /// caller excludes overlapping access to this context field.
    pub(crate) unsafe fn replace_from_raw_in(&mut self, instance: *mut RInstance, value: SEXP) {
        let next = unsafe { Self::from_raw_in(instance, value) };
        *self = next;
    }

    /// # Safety
    /// `value` is a live projection belonging to the active owner, and the
    /// caller excludes overlapping access to this field.
    pub(crate) unsafe fn replace_from_raw(&mut self, value: SEXP) {
        instance::with_required_current_instance(|instance| unsafe {
            self.replace_from_raw_in(instance, value);
        });
    }
}

/// R's execution context node.
///
/// This is the Rust equivalent of R's `RCNTXT` struct from Defn.h.
/// It tracks the state needed for error handling, loop control flow,
/// and function call boundaries.
pub struct RCNTXT {
    /// Context type flags (CTXT_TOPLEVEL, CTXT_FUNCTION, etc.)
    pub cstackbase: *mut u8,
    /// Type of context (function, loop, toplevel, etc.)
    pub callflag: c_int,
    /// The call being evaluated (for error reporting)
    pub call: ContextValue,
    /// The closure/environment for function contexts
    pub cloenv: ContextValue,
    pub sysparent: ContextValue,
    pub callfun: ContextValue,
    /// Function to call on exit (on.exit handlers)
    pub cfn: Option<unsafe extern "C" fn(*mut SexprecCore) -> *mut SexprecCore>,
    /// Closure being evaluated
    pub closure: ContextValue,
    /// Promises for arguments
    pub promiseargs: ContextValue,
    /// Old working directory (saved on entry)
    pub savelist: ContextValue,
    /// Handler for conditions
    pub handlerstack: ContextValue,
    /// Restart stack
    pub restartstack: ContextValue,
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
    pub rpvec: ContextValue,
    /// Vector clock
    pub rpvbase: usize,
    /// Return value
    pub returnValue: ContextValue,
    /// Number of protect entries at context entry
    pub protectCount: usize,
    /// on.exit expression list (conexit in R)
    pub conexit: ContextValue,
    pub onexit_active: i32,
    /// cleanup function pointer (cend in R)
    pub cend: Option<unsafe extern "C" fn(*mut std::os::raw::c_void)>,
    /// cleanup function data (cenddata in R)
    pub cenddata: *mut std::os::raw::c_void,
    pub srcref: ContextValue,
}

impl RCNTXT {
    /// Create a new (zeroed) context.
    pub fn new() -> Self {
        RCNTXT {
            cstackbase: ptr::null_mut(),
            callflag: 0,
            call: ContextValue::empty(),
            cloenv: ContextValue::empty(),
            sysparent: ContextValue::empty(),
            callfun: ContextValue::empty(),
            cfn: None,
            closure: ContextValue::empty(),
            promiseargs: ContextValue::empty(),
            savelist: ContextValue::empty(),
            handlerstack: ContextValue::empty(),
            restartstack: ContextValue::empty(),
            browserflag: 0,
            evaldepth: 0,
            nextcontext: ptr::null_mut(),
            intactive: 0,
            jumped: 0,
            rpvec: ContextValue::empty(),
            rpvbase: 0,
            returnValue: ContextValue::empty(),
            protectCount: 0,
            conexit: ContextValue::empty(),
            onexit_active: 0,
            cend: None,
            cenddata: ptr::null_mut(),
            srcref: ContextValue::empty(),
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
/// Contexts live in original `Rc<UnsafeCell<RCNTXT>>` allocations.
/// All pointers come from `UnsafeCell::get`, including the nextcontext chain.
/// Re-entering the evaluator never derives a new unique reference to a live
/// context. Removing stack ownership invalidates unleased pointers. Guards and callback
/// operations retain the same original cell until their cleanup completes.
fn stack_top_mut(instance: *mut RInstance) -> Option<*mut RCNTXT> {
    // SAFETY: `instance` is a live instance pointer from the caller; the
    // Vec access is strictly local and the derived pointer is handed to the
    // caller per the module policy.
    unsafe { (*instance).context_stack.last_mut().map(|ctx| ctx.get()) }
}

/// Retain the original context allocation before executing a callback.
/// This is the same cell owned by the stack, not a context snapshot.
///
/// # Safety
/// `instance` is a live original owner with no overlapping context-stack loan.
pub(crate) unsafe fn retain_context_in(
    instance: *mut RInstance,
    context: *mut RCNTXT,
) -> Option<Rc<std::cell::UnsafeCell<RCNTXT>>> {
    unsafe {
        (*instance)
            .context_stack
            .iter()
            .find(|cell| cell.get() == context)
            .cloned()
    }
}

/// # Safety
/// `instance` is a live original owner. This short snapshot ends before any
/// callback executes. Standalone translated fixtures retain it by borrowing.
pub(crate) unsafe fn pin_context_owner_in(
    instance: *mut RInstance,
) -> Option<super::owner::OwnerPin> {
    let owner = unsafe { (*instance).runtime_owner.clone() };
    owner.map(|owner| {
        owner
            .pin()
            .unwrap_or_else(|error| r_error(format!("unavailable context owner: {error}")))
    })
}

pub(crate) fn require_context_owner_live(pin: &Option<super::owner::OwnerPin>) {
    if let Some(pin) = pin {
        pin.require_live()
            .unwrap_or_else(|error| r_error(format!("unavailable context owner: {error}")));
    }
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
    let owner_pin = unsafe { pin_context_owner_in(instance) };
    // Capture every input before installing the srcref symbol: symbol creation
    // can allocate and collect, including through a reentrant GC callback.
    let call = unsafe { ContextValue::from_raw_in(instance, call) };
    let cloenv = unsafe { ContextValue::from_raw_in(instance, cloenv) };
    let sysparent = unsafe { ContextValue::from_raw_in(instance, sysparent) };
    let closure = unsafe { ContextValue::from_raw_in(instance, closure) };
    let promiseargs = unsafe { ContextValue::from_raw_in(instance, promiseargs) };
    let callfun = ContextValue(closure.owned());
    let srcref = unsafe {
        crate::sexp::attrib_core::getAttrib(
            call.as_raw(),
            crate::sexp::symbol::Rf_install(c"srcref".as_ptr()),
        )
    };
    require_context_owner_live(&owner_pin);
    let srcref = unsafe { ContextValue::from_raw_in(instance, srcref) };
    let ctx = Rc::new(std::cell::UnsafeCell::new(RCNTXT {
        callflag,
        call,
        cloenv,
        sysparent,
        cfn,
        callfun,
        closure,
        promiseargs,
        srcref,
        ..RCNTXT::new()
    }));

    // P2: the short-lived reads/writes below are strictly local — pushing
    // an Rc onto the Vec allocates but never reenters the interpreter, and
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

    // Publish original allocation ownership before deriving the pointer. UnsafeCell permits
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
            // Address-only comparison: the pop below releases stack ownership, so
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
    // These are the original context cell and interpreter allocation, retained
    // through callbacks and owner-bound cleanup. Context values drop first.
    _context_owner: Rc<std::cell::UnsafeCell<RCNTXT>>,
    _owner_pin: Option<super::owner::OwnerPin>,
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
        let owner_pin = unsafe { pin_context_owner_in(instance) };
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
            _context_owner: unsafe { retain_context_in(instance, context) }
                .expect("just pushed context"),
            _owner_pin: owner_pin,
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
                if cloenv.is_null() || ctx_ref.cloenv.as_raw() == cloenv {
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

pub use super::transfer::TransferTicket;

/// Nonlocal evaluation signals carry Send-only tickets. Their actual values
/// and original context allocations stay in owning, thread-confined storage.
#[derive(Debug)]
pub enum RSignal {
    Error { message: String },
    Break,
    Next,
    Return(TransferTicket),
    Warning { message: String },
    Message { message: String },
    Restart(TransferTicket),
    Abort,
    ExitingHandler(TransferTicket),
    Jump(TransferTicket),
}

/// Capture a native projection in the original active transfer scope.
/// # Safety
/// The projection is initialized and no payload loan overlaps this operation.
pub(crate) unsafe fn own_control_value(value: SEXP) -> super::object::Sexp<'static> {
    let owner =
        unsafe { super::owner::OwnerToken::current() }.unwrap_or_else(|e| r_error(e.to_string()));
    let pin = owner
        .pin()
        .unwrap_or_else(|e| r_error(e.to_string()))
        .unwrap_or_else(|| r_error("owning control values require a managed runtime"));
    let result = owner
        .sexp(value)
        .and_then(|v| v.into_owned())
        .unwrap_or_else(|e| r_error(e.to_string()));
    pin.require_live()
        .unwrap_or_else(|e| r_error(e.to_string()));
    result
}

/// # Safety
/// A nonnull target identifies an original live context in the active owner.
unsafe fn transfer_target(target: *mut RCNTXT) -> Option<Rc<std::cell::UnsafeCell<RCNTXT>>> {
    if target.is_null() {
        return None;
    }
    let pin = super::transfer::active_owner_pin().unwrap_or_else(|e| r_error(e.to_string()));
    Some(
        unsafe { retain_context_in(pin.as_ptr(), target) }
            .unwrap_or_else(|| r_error("nonlocal transfer target is no longer owned")),
    )
}

/// # Safety
/// Target and value meet the checked native projection contracts above.
pub(crate) unsafe fn return_transfer(target: *mut RCNTXT, value: SEXP) -> TransferTicket {
    let data = super::transfer::OwnedTransfer::Return {
        target: unsafe { transfer_target(target) },
        value: unsafe { own_control_value(value) },
    };
    super::transfer::publish(data).unwrap_or_else(|e| r_error(e.to_string()))
}

/// # Safety
/// Target and value meet the checked native projection contracts above.
pub(crate) unsafe fn jump_transfer(target: *mut RCNTXT, mask: i32, value: SEXP) -> TransferTicket {
    let data = super::transfer::OwnedTransfer::Jump {
        target: unsafe { transfer_target(target) },
        mask,
        value: unsafe { own_control_value(value) },
    };
    super::transfer::publish(data).unwrap_or_else(|e| r_error(e.to_string()))
}

/// # Safety
/// Both values are initialized projections of the active original owner.
pub(crate) unsafe fn exiting_handler_transfer(target_env: SEXP, result: SEXP) -> TransferTicket {
    let data = super::transfer::OwnedTransfer::ExitingHandler {
        target_env: unsafe { own_control_value(target_env) },
        result: unsafe { own_control_value(result) },
    };
    super::transfer::publish(data).unwrap_or_else(|e| r_error(e.to_string()))
}

/// # Safety
/// Both values are initialized projections of the active original owner.
pub(crate) unsafe fn restart_transfer(target: SEXP, args: SEXP) -> TransferTicket {
    let data = super::transfer::OwnedTransfer::Restart {
        target: unsafe { own_control_value(target) },
        args: unsafe { own_control_value(args) },
    };
    super::transfer::publish(data).unwrap_or_else(|e| r_error(e.to_string()))
}

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
            RSignal::Return(ticket) => {
                let lease = ticket.take().unwrap_or_else(|e| r_error(e.to_string()));
                lease
                    .require_live()
                    .unwrap_or_else(|e| r_error(e.to_string()));
                let super::transfer::OwnedTransfer::Return { value, .. } = lease.data() else {
                    r_error("incorrect return transfer kind")
                };
                value.as_raw()
            }
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
            .any(|ctx| (*ctx.get()).cloenv.as_raw() == target_env)
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
        let ctx = unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            RCNTXT::new()
        };
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
        // Vec<Rc<UnsafeCell<RCNTXT>>>, through UnsafeCell::get. The write must be
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
            // Replacement captures the original owning allocation before publication.
            let value = crate::sexp::constructors::Rf_ScalarInteger(42);
            (*global).sysparent.replace_from_raw(value);
            assert_eq!((*global).sysparent.as_raw(), value);
            // A re-derived pointer observes the same mutation (single
            // storage; no `&`-cast copy).
            assert_eq!((*R_GlobalContext()).sysparent.as_raw(), value);
            let found = Rf_findcontext(ctxt_flags::CTXT_FUNCTION, ptr::null_mut(), ptr::null_mut());
            assert_eq!((*found).sysparent.as_raw(), value);

            Rf_endcontext(c);
        });
    }

    #[test]
    fn test_context_teardown_invalidates_derived_pointers() {
        // Policy: Rf_endcontext releases the original owning cell, so every unleased pointer
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
    fn owned_context_fields_retain_original_allocations_through_gc_and_release_on_pop() {
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
            let factory = owner.node_factory();
            let values: Vec<_> = (0..13)
                .map(|_| {
                    factory
                        .allocate(|arena| {
                            Some(arena.alloc_vector(crate::sexp::ffi::SEXPTYPE::INTSXP, 1))
                        })
                        .unwrap()
                })
                .collect();
            let pointers: Vec<_> = values.iter().map(|value| value.as_raw()).collect();
            let nodes: Vec<_> = pointers
                .iter()
                .map(|&pointer| crate::sexp::memory::checked_projection(pointer).unwrap().1)
                .collect();
            let context = Rf_begincontext_in(
                instance,
                ctxt_flags::CTXT_FUNCTION,
                pointers[0],
                pointers[1],
                pointers[2],
                None,
                pointers[4],
                pointers[5],
            );
            // Every stored value is the original allocation. There is no raw
            // context scanner or secondary protection ledger to rescue them.
            (*context)
                .callfun
                .replace_from_raw_in(instance, pointers[3]);
            (*context)
                .savelist
                .replace_from_raw_in(instance, pointers[6]);
            (*context)
                .handlerstack
                .replace_from_raw_in(instance, pointers[7]);
            (*context)
                .restartstack
                .replace_from_raw_in(instance, pointers[8]);
            (*context).rpvec.replace_from_raw_in(instance, pointers[9]);
            (*context)
                .returnValue
                .replace_from_raw_in(instance, pointers[10]);
            (*context)
                .conexit
                .replace_from_raw_in(instance, pointers[11]);
            (*context)
                .srcref
                .replace_from_raw_in(instance, pointers[12]);
            drop(values);
            owner.full_gc().unwrap();
            assert!(nodes.iter().all(|node| node.is_live()));
            let projections = [
                (*context).call.as_raw(),
                (*context).cloenv.as_raw(),
                (*context).sysparent.as_raw(),
                (*context).callfun.as_raw(),
                (*context).closure.as_raw(),
                (*context).promiseargs.as_raw(),
                (*context).savelist.as_raw(),
                (*context).handlerstack.as_raw(),
                (*context).restartstack.as_raw(),
                (*context).rpvec.as_raw(),
                (*context).returnValue.as_raw(),
                (*context).conexit.as_raw(),
                (*context).srcref.as_raw(),
            ];
            assert_eq!(projections.as_slice(), pointers.as_slice());
            Rf_endcontext_in(instance, context);
            owner.full_gc().unwrap();
            assert!(nodes.iter().all(|node| !node.is_live()));
        });
    }

    #[test]
    fn owned_context_guard_keeps_original_cell_after_stack_teardown() {
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
            let value = owner
                .node_factory()
                .allocate(|arena| Some(arena.alloc_vector(crate::sexp::ffi::SEXPTYPE::INTSXP, 1)))
                .unwrap();
            let pointer = value.as_raw();
            let node = crate::sexp::memory::checked_projection(pointer).unwrap().1;
            let guard = begin_context_guard(
                ctxt_flags::CTXT_FUNCTION,
                pointer,
                ptr::null_mut(),
                ptr::null_mut(),
                None,
                ptr::null_mut(),
                ptr::null_mut(),
            );
            drop(value);
            (*instance).context_stack.clear();
            owner.full_gc().unwrap();
            assert!(node.is_live());
            assert_eq!((*guard.context()).call.as_raw(), pointer);
            drop(guard);
            owner.full_gc().unwrap();
            assert!(!node.is_live());
        });
    }

    #[test]
    fn owned_context_cleanup_callback_can_teardown_stack_and_collect() {
        struct CallbackData {
            instance: *mut RInstance,
            node: crate::sexp::heap::CheckedNode,
            invoked: bool,
        }
        unsafe extern "C" fn cleanup(data: *mut std::os::raw::c_void) {
            let data = unsafe { &mut *data.cast::<CallbackData>() };
            unsafe {
                (*data.instance).context_stack.clear();
                crate::sexp::gengc::full_gc_in(data.instance);
            }
            data.invoked = data.node.is_live();
        }
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
            let value = owner
                .node_factory()
                .allocate(|arena| Some(arena.alloc_vector(crate::sexp::ffi::SEXPTYPE::INTSXP, 1)))
                .unwrap();
            let pointer = value.as_raw();
            let node = crate::sexp::memory::checked_projection(pointer).unwrap().1;
            let context = Rf_begincontext_in(
                instance,
                ctxt_flags::CTXT_FUNCTION,
                pointer,
                ptr::null_mut(),
                ptr::null_mut(),
                None,
                ptr::null_mut(),
                ptr::null_mut(),
            );
            let mut data = CallbackData {
                instance,
                node: node.clone(),
                invoked: false,
            };
            (*context).cend = Some(cleanup);
            (*context).cenddata = (&mut data as *mut CallbackData).cast();
            drop(value);
            crate::eval::context::R_run_onexits_for_context(context);
            assert!(
                data.invoked,
                "callback must observe the original retained context value"
            );
            assert!(R_GlobalContext_in(instance).is_null());
            owner.full_gc().unwrap();
            assert!(!node.is_live());
        });
    }

    #[test]
    fn owned_context_rejects_foreign_replacement_without_losing_current_value() {
        let session = RSession::new_for_gc_tests();
        let other = RSession::new_for_gc_tests();
        let foreign = other.with_active(|| {
            other
                .owner_token()
                .unwrap()
                .node_factory()
                .allocate(|arena| Some(arena.alloc_vector(crate::sexp::ffi::SEXPTYPE::INTSXP, 1)))
                .unwrap()
        });
        session.with_active_in(|instance| unsafe {
            let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
            let local = owner
                .node_factory()
                .allocate(|arena| Some(arena.alloc_vector(crate::sexp::ffi::SEXPTYPE::INTSXP, 1)))
                .unwrap();
            let pointer = local.as_raw();
            let guard = begin_context_guard(
                ctxt_flags::CTXT_FUNCTION,
                pointer,
                ptr::null_mut(),
                ptr::null_mut(),
                None,
                ptr::null_mut(),
                ptr::null_mut(),
            );
            drop(local);
            let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (*guard.context())
                    .call
                    .replace_from_raw_in(instance, foreign.as_raw());
            }));
            assert!(error.is_err());
            owner.full_gc().unwrap();
            assert_eq!((*guard.context()).call.as_raw(), pointer);
            drop(guard);
            assert!(R_GlobalContext_in(instance).is_null());
        });
    }

    #[test]
    fn owned_context_guard_releases_values_after_collecting_unwind() {
        let session = RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
            let value = owner
                .node_factory()
                .allocate(|arena| Some(arena.alloc_vector(crate::sexp::ffi::SEXPTYPE::INTSXP, 1)))
                .unwrap();
            let pointer = value.as_raw();
            let node = crate::sexp::memory::checked_projection(pointer).unwrap().1;
            let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _guard = begin_context_guard(
                    ctxt_flags::CTXT_FUNCTION,
                    pointer,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    None,
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
                drop(value);
                owner.full_gc().unwrap();
                assert!(node.is_live());
                r_error("context unwind fixture");
            }));
            assert!(unwind.is_err());
            assert!(R_GlobalContext_in(instance).is_null());
            owner.full_gc().unwrap();
            assert!(!node.is_live());
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
