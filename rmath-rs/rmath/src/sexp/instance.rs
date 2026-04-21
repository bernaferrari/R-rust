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
use std::ptr;

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
        let nil = ptr::null_mut::<SexprecCore>();

        // Create environment chain: empty -> base -> global, using leaked
        // Boxes so they outlive the instance (matching the process-wide pattern).
        let empty_env = Self::make_env(nil, nil, nil);
        let base_env = Self::make_env(nil, empty_env, nil);
        let global_env = Self::make_env(nil, base_env, nil);

        RInstance {
            arena: RArena::new(),
            global_env,
            base_env,
            empty_env,
            protect_stack: Vec::new(),
            options: HashMap::new(),
            options_initialized: false,
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

/// Clear the current thread-local R instance.
///
/// Should be called when an `RSession` is closed or dropped.
pub fn clear_current_instance() {
    CURRENT_INSTANCE.with(|ci| {
        *ci.borrow_mut() = None;
    });
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
