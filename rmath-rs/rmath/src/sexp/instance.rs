#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! R instance isolation — per-instance state for concurrent R sessions.
//!
//! An `RInstance` owns all mutable state that was previously process-wide or
//! thread-local, enabling multiple independent R sessions to run concurrently
//! within the same process and on the same thread (sequentially).
//!
//! # Thread-local dispatch
//!
//! The [`set_current_instance`] / [`clear_current_instance`] functions set a
//! thread-local pointer to the "active" instance. When an instance is active,
//! the global accessor functions (`R_GlobalEnv`, `Rf_protect`, `with_arena`,
//! etc.) dispatch to the instance's fields instead of the process-wide
//! fallbacks. This preserves backward compatibility: code that does not use
//! `RSession` continues to work with the original global state.

use std::alloc::{Layout, dealloc};
use std::cell::RefCell;
use std::collections::HashMap;
use std::os::raw::c_int;
use std::time::Instant;

use super::ffi::{SEXP, SEXPTYPE, SexprecCore};
use super::memory::RArena;

// ---------------------------------------------------------------------------
// RInstance
// ---------------------------------------------------------------------------

pub(crate) struct ErrorState {
    pub warn_length: c_int,
    pub show_error_messages: bool,
    pub show_error_calls: bool,
    pub show_warn_calls: bool,
    pub in_error: c_int,
    pub in_warning: c_int,
    pub in_print_warnings: c_int,
    pub immediate_warning: bool,
    pub no_break_warning: bool,
    pub interrupts_suspended: bool,
    pub interrupts_pending: bool,
    pub collect_warnings: c_int,
    pub nwarnings: c_int,
    pub warnings: SEXP,
    pub handler_stack: SEXP,
    pub restart_stack: SEXP,
    pub expressions: c_int,
    pub expressions_keep: c_int,
}

impl Default for ErrorState {
    fn default() -> Self {
        ErrorState {
            warn_length: 1000,
            show_error_messages: true,
            show_error_calls: false,
            show_warn_calls: false,
            in_error: 0,
            in_warning: 0,
            in_print_warnings: 0,
            immediate_warning: false,
            no_break_warning: false,
            interrupts_suspended: false,
            interrupts_pending: false,
            collect_warnings: 0,
            nwarnings: 50,
            warnings: std::ptr::null_mut(),
            handler_stack: std::ptr::null_mut(),
            restart_stack: std::ptr::null_mut(),
            expressions: 500,
            expressions_keep: 500,
        }
    }
}

pub(crate) struct EvalControlState {
    pub no_echo: c_int,
    pub quiet: c_int,
    pub interactive: c_int,
    pub verbose: c_int,
    pub current_expr: SEXP,
    pub visible: c_int,
    pub eval_depth: c_int,
    pub eval_depth_limit: c_int,
    pub pp_stack_top: c_int,
    pub collect_warnings: c_int,
    pub parse_error_msg: [u8; 256],
    pub limits: crate::eval::eval::EvalLimits,
    pub start_time: Option<Instant>,
    pub bc_stack: crate::eval::bc_stack::R_bcstack_t,
    pub bc_int_active: c_int,
    pub min_jit_score: c_int,
    pub loop_jit_score: c_int,
    pub jit_enabled: c_int,
    pub compile_pkgs: c_int,
    pub disable_bytecode: c_int,
    pub check_constants: c_int,
    pub exec_token: SEXP,
}

impl Default for EvalControlState {
    fn default() -> Self {
        EvalControlState {
            no_echo: 0,
            quiet: 0,
            interactive: 1,
            verbose: 0,
            current_expr: std::ptr::null_mut(),
            visible: 1,
            eval_depth: 0,
            eval_depth_limit: 500,
            pp_stack_top: 0,
            collect_warnings: 0,
            parse_error_msg: [0; 256],
            limits: crate::eval::eval::EvalLimits::default(),
            start_time: None,
            bc_stack: crate::eval::bc_stack::R_bcstack_t::new(256),
            bc_int_active: 0,
            min_jit_score: 50,
            loop_jit_score: 50,
            jit_enabled: 0,
            compile_pkgs: 0,
            disable_bytecode: 0,
            check_constants: 0,
            exec_token: std::ptr::null_mut(),
        }
    }
}

/// All per-instance state for one independent R session.
///
/// Each `RInstance` has its own arena, environments, and protection stack,
/// completely isolated from other instances and from the process-wide globals.
///
/// # Safety
///
/// `RInstance` is `Send` because all fields are owned and no `Rc`/`Arc` or
/// thread-local state is stored inside. However, it is NOT `Sync` because
/// the arena and protect stack are `!Sync`.
///
/// The raw SEXP pointers inside are valid for as long as the arena is alive.
pub struct RInstance {
    /// Arena allocator for this instance.
    pub arena: RArena,
    /// The global environment for this instance.
    pub global_env: SEXP,
    /// The base environment for this instance.
    pub base_env: SEXP,
    /// The empty environment for this instance.
    pub empty_env: SEXP,
    /// The protection stack for this instance.
    pub protect_stack: Vec<SEXP>,
    /// The permanent preserve stack for this instance.
    pub preserve_stack: Vec<SEXP>,
    /// Per-instance execution context stack.
    #[allow(clippy::vec_box)]
    pub(crate) context_stack: Vec<Box<super::context::RCNTXT>>,
    /// Per-instance in-error flag.
    pub(crate) in_error: bool,
    /// Per-instance generational GC state.
    pub(crate) gc_state: super::gengc::GcState,
    /// Per-instance error, warning, interrupt, and expression-limit state.
    pub(crate) error_state: ErrorState,
    /// Per-instance evaluator and REPL control state.
    pub(crate) eval_state: EvalControlState,
    /// Per-instance symbol table for session-local interning.
    pub(crate) symbols: HashMap<String, SEXP>,
    /// Owned SYMSXP nodes for the per-instance symbol table.
    #[allow(clippy::vec_box)]
    pub(crate) symbol_nodes: Vec<Box<SexprecCore>>,
    /// Per-instance Marsaglia-MultiCarry RNG seed state.
    pub(crate) rng_state: (u32, u32),
    /// Per-instance stdout capture buffer.
    pub(crate) capture_stdout: Option<String>,
    /// Per-instance stderr capture buffer.
    pub(crate) capture_stderr: Option<String>,
    /// Per-instance options storage (mirrors the global OPTIONS_TABLE).
    pub options: HashMap<String, SEXP>,
    /// Whether the instance options have been initialized with defaults.
    pub options_initialized: bool,
    /// Per-instance environment hash side tables.
    pub(crate) env_hash_tables: hashbrown::HashMap<usize, hashbrown::HashMap<usize, SEXP>>,
    /// Per-instance raw cons cells allocated outside the arena.
    pub(crate) raw_cons: Vec<*mut SexprecCore>,
    /// Per-instance transient allocations for R_alloc/vmaxget/vmaxset.
    pub(crate) vmax: Vec<(*mut u8, Layout)>,
}

// SAFETY: RInstance owns all its data. The SEXP pointers point into the
// arena's Box allocations which are stable as long as the arena lives.
// No reference counting or shared mutable state is involved.
unsafe impl Send for RInstance {}

impl RInstance {
    /// Create a new, fully independent R instance.
    ///
    /// This allocates three persistent environment sentinels (empty → base →
    /// global) using leaked `Box`es (same pattern as `init.rs`) and an empty
    /// arena and protect stack.
    pub fn new() -> Self {
        let nil = unsafe { super::globals::R_NilValue() };

        // Create environment chain: empty -> base -> global, using leaked
        // Boxes so they outlive the instance (matching the process-wide pattern).
        let empty_env = Self::make_env(nil, nil, nil);
        let base_env = Self::make_env(nil, empty_env, nil);
        let global_env = Self::make_env(nil, base_env, nil);

        let mut instance = RInstance {
            arena: RArena::new(),
            global_env,
            base_env,
            empty_env,
            protect_stack: Vec::new(),
            preserve_stack: Vec::new(),
            context_stack: Vec::new(),
            in_error: false,
            gc_state: super::gengc::GcState::default(),
            error_state: ErrorState::default(),
            eval_state: EvalControlState::default(),
            symbols: HashMap::new(),
            symbol_nodes: Vec::new(),
            rng_state: (1234, 5678),
            capture_stdout: None,
            capture_stderr: None,
            options: HashMap::new(),
            options_initialized: false,
            env_hash_tables: hashbrown::HashMap::new(),
            raw_cons: Vec::new(),
            vmax: Vec::new(),
        };

        instance.initialize_base_bindings();
        instance
    }

    /// Install core base bindings with this instance active.
    pub fn initialize_base_bindings(&mut self) {
        let previous = unsafe { replace_current_instance(Some(self as *mut RInstance)) };
        unsafe {
            super::init::initialize_base_bindings(self.base_env);
            replace_current_instance(previous);
        }
    }

    /// Allocate a leaked environment node (outside the arena).
    fn make_env(frame: SEXP, enclos: SEXP, hashtab: SEXP) -> SEXP {
        let mut boxed = Box::new(SexprecCore::new(SEXPTYPE::ENVSXP));
        let env: SEXP = &mut *boxed as *mut _;
        unsafe {
            (*env).data.envsxp.frame = frame;
            (*env).data.envsxp.enclos = enclos;
            (*env).data.envsxp.hashtab = hashtab;
        }
        Box::leak(boxed)
    }
}

impl Default for RInstance {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RInstance {
    fn drop(&mut self) {
        for ptr in self.raw_cons.drain(..) {
            if !ptr.is_null() {
                unsafe {
                    let _ = Box::from_raw(ptr);
                }
            }
        }
        for (ptr, layout) in self.vmax.drain(..) {
            if !ptr.is_null() && layout.size() > 0 {
                unsafe {
                    dealloc(ptr, layout);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Thread-local current instance
// ---------------------------------------------------------------------------

thread_local! {
    /// Pointer to the currently active `RInstance`, if any.
    ///
    /// Stored as a raw pointer to avoid requiring `Sync` on `RInstance`.
    /// The instance itself is owned by an `RSession` (via `Box<RInstance>`),
    /// so the pointer is valid for the lifetime of that session.
    static CURRENT_INSTANCE: RefCell<Option<*mut RInstance>> = const { RefCell::new(None) };
}

/// Set the current thread-local R instance.
///
/// # Safety
///
/// The caller must ensure that `instance` points to a valid, live `RInstance`
/// and that no other instance is currently active on this thread.
pub unsafe fn set_current_instance(instance: *mut RInstance) {
    CURRENT_INSTANCE.with(|ci| {
        *ci.borrow_mut() = Some(instance);
    });
}

/// Replace the current thread-local R instance and return the previous value.
///
/// This is the primitive used by scoped session activation. It only stores raw
/// pointers; callers remain responsible for ensuring any non-null pointer stays
/// valid while installed.
pub unsafe fn replace_current_instance(instance: Option<*mut RInstance>) -> Option<*mut RInstance> {
    CURRENT_INSTANCE.with(|ci| {
        let mut current = ci.borrow_mut();
        let previous = *current;
        *current = instance;
        previous
    })
}

/// Clear the current thread-local R instance.
///
/// Should be called when an `RSession` is closed or dropped.
pub fn clear_current_instance() {
    CURRENT_INSTANCE.with(|ci| {
        *ci.borrow_mut() = None;
    });
}

/// Clear the current thread-local R instance only if it matches `instance`.
///
/// Returns `true` when the pointer matched and the thread-local slot was
/// cleared. This prevents an older session from detaching a newer active
/// session that became current on the same thread.
pub fn clear_current_instance_if(instance: *const RInstance) -> bool {
    CURRENT_INSTANCE.with(|ci| {
        let mut current = ci.borrow_mut();
        if current
            .map(|ptr| std::ptr::eq(ptr as *const RInstance, instance))
            .unwrap_or(false)
        {
            *current = None;
            true
        } else {
            false
        }
    })
}

/// Return the current raw instance pointer, if one is active.
#[inline]
pub fn current_instance_ptr() -> Option<*mut RInstance> {
    CURRENT_INSTANCE.with(|ci| *ci.borrow())
}

/// Execute a closure with a reference to the current instance, if active.
///
/// Returns `None` (and does not call `f`) if no instance is currently active.
#[inline]
pub fn with_current_instance<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut RInstance) -> R,
{
    CURRENT_INSTANCE.with(|ci| {
        let borrow = ci.borrow();
        match *borrow {
            Some(ptr) => {
                // SAFETY: The pointer was set by `set_current_instance` and is
                // valid as long as the owning RSession is alive.
                unsafe { Some(f(&mut *ptr)) }
            }
            None => None,
        }
    })
}

/// Execute a closure with the current instance.
///
/// Mutable interpreter state must be accessed through an active `RInstance`.
/// A missing instance indicates an unscoped runtime entrypoint that should be
/// routed through `RSession` before it reaches interpreter internals.
#[inline]
pub fn with_required_current_instance<F, R>(f: F) -> R
where
    F: FnOnce(&mut RInstance) -> R,
{
    with_current_instance(f).expect("mutable R runtime state requires an active RInstance")
}

/// Returns `true` if a current instance is active on this thread.
#[inline]
pub fn has_current_instance() -> bool {
    CURRENT_INSTANCE.with(|ci| ci.borrow().is_some())
}
