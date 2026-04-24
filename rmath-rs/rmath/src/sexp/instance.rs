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

use std::cell::RefCell;
use std::collections::HashMap;

use super::ffi::{SEXP, SEXPTYPE, SexprecCore};
use super::memory::RArena;

// ---------------------------------------------------------------------------
// RInstance
// ---------------------------------------------------------------------------

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
            symbols: HashMap::new(),
            symbol_nodes: Vec::new(),
            rng_state: (1234, 5678),
            capture_stdout: None,
            capture_stderr: None,
            options: HashMap::new(),
            options_initialized: false,
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

/// Returns `true` if a current instance is active on this thread.
#[inline]
pub fn has_current_instance() -> bool {
    CURRENT_INSTANCE.with(|ci| ci.borrow().is_some())
}
