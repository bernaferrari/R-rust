#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/memory.c
//!
//! This module provides stubs and simplified implementations for R's memory
//! management subsystem, including:
//!
//! - GC control (R_gc, R_gc_lite, R_gc_running)
//! - Transient allocation (R_allocLD, S_alloc, S_realloc)
//! - Weak references and finalizers (R_MakeWeakRef, R_RegisterFinalizer, etc.)
//! - External pointer management (R_MakeExternalPtr, etc.)
//! - Memory checking utilities (R_chk_calloc, R_chk_realloc, etc.)
//! - String buffer management (R_AllocStringBuffer, R_FreeStringBuffer)
//! - Multi-set preservation (R_NewPreciousMSet, R_PreserveInMSet, etc.)
//! - Resizable vector support (R_isResizable, R_allocResizableVector, etc.)
//! - Precious list management (R_PreserveObject, R_ReleaseObject)
//! - Protect stack utilities (R_signal_protect_error, etc.)
//! - Type checking functions (Rf_isNull, Rf_isSymbol, etc.)
//! - sexptype2char conversion
//!
//! NOTE: Functions already defined in sexp/memory_ext.rs, sexp/protect.rs,
//! unix/system.rs, unix/embedded.rs, and mainutils/main.rs are provided
//! as pub(crate) functions WITHOUT #[unsafe(no_mangle)] to avoid duplicate symbol errors.

use std::os::raw::{c_char, c_double, c_int, c_long, c_void};
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{ExtPtrBody, Listsxp, NA_INTEGER, NodeBody, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_GlobalEnv, R_MissingArg, R_NilValue};
use crate::sexp::heap::NodeLink;
use crate::sexp::instance::{
    RInstance, with_finalizer_state_in, with_gc_state_in, with_memory_state_in,
};
use crate::sexp::{Sexp, SexpMut, object::SexpResult, owner::OwnerToken};

#[cfg(test)]
#[path = "memory_finalizer_batch_tests.rs"]
mod finalizer_batch_tests;

fn error(msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    });
}

// ---------------------------------------------------------------------------
// GC control
// ---------------------------------------------------------------------------

pub type R_CFinalizer_t = unsafe extern "C" fn(*mut c_void);

#[derive(Clone)]
enum PendingFinalizer {
    C {
        obj: SEXP,
        fun: R_CFinalizer_t,
        ready: bool,
        onexit: bool,
    },
    R {
        obj: SEXP,
        fun: SEXP,
        ready: bool,
        onexit: bool,
    },
}

impl PendingFinalizer {
    fn obj(&self) -> SEXP {
        match *self {
            PendingFinalizer::C { obj, .. } | PendingFinalizer::R { obj, .. } => obj,
        }
    }

    fn obj_mut(&mut self) -> &mut SEXP {
        match self {
            PendingFinalizer::C { obj, .. } | PendingFinalizer::R { obj, .. } => obj,
        }
    }

    fn fun_mut(&mut self) -> Option<&mut SEXP> {
        match self {
            PendingFinalizer::C { .. } => None,
            PendingFinalizer::R { fun, .. } => Some(fun),
        }
    }

    fn is_ready(&self) -> bool {
        match *self {
            PendingFinalizer::C { ready, .. } | PendingFinalizer::R { ready, .. } => ready,
        }
    }

    fn set_ready(&mut self) {
        match self {
            PendingFinalizer::C { ready, .. } | PendingFinalizer::R { ready, .. } => {
                *ready = true;
            }
        }
    }

    fn is_onexit(&self) -> bool {
        match *self {
            PendingFinalizer::C { onexit, .. } | PendingFinalizer::R { onexit, .. } => onexit,
        }
    }
}

#[derive(Default)]
pub(crate) struct FinalizerState {
    running_finalizers: bool,
    pending_finalizers: Vec<PendingFinalizer>,
}

/// Startup default for the vector-heap limit, mirroring upstream
/// startup.c: on macOS the vector pool is capped at max(physical memory,
/// 16 Gb) to avoid R being killed by memory overcommit; other platforms
/// start unlimited. `R_MAX_VSIZE` (upstream's override) is not consulted.
#[cfg(target_os = "macos")]
fn default_max_v_size() -> u64 {
    // 16 Gb, upstream's `MinMaxVSize` floor.
    const MIN_MAX_V_SIZE: u64 = 17179869184;
    // Miri does not implement `_SC_PHYS_PAGES` on macOS; fall back to the
    // floor so Miri sessions can run on macOS dev machines too.
    if cfg!(miri) {
        return MIN_MAX_V_SIZE;
    }
    let pages = unsafe { libc::sysconf(libc::_SC_PHYS_PAGES) };
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGE_SIZE) };
    if pages <= 0 || page_size <= 0 {
        return MIN_MAX_V_SIZE;
    }
    let sysmem = (pages as u64) * (page_size as u64);
    sysmem.max(MIN_MAX_V_SIZE)
}

/// Startup default for the vector-heap limit (unlimited off macOS).
#[cfg(not(target_os = "macos"))]
fn default_max_v_size() -> u64 {
    u64::MAX
}

pub(crate) struct MemoryRuntimeState {
    in_gc: c_int,
    gc_reporting: c_int,
    gc_count: c_int,
    gc_force_gap: c_int,
    gc_force_wait: c_int,
    max_v_size: u64,
    max_n_size: u64,
}

impl Default for MemoryRuntimeState {
    fn default() -> Self {
        Self {
            in_gc: 0,
            gc_reporting: 0,
            gc_count: 0,
            gc_force_gap: 0,
            gc_force_wait: 0,
            max_v_size: default_max_v_size(),
            max_n_size: u64::MAX,
        }
    }
}

fn with_memory_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut MemoryRuntimeState) -> R,
{
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        with_memory_state_in(inst, f)
    })
}

fn with_finalizer_state<R>(f: impl FnOnce(&mut FinalizerState) -> R) -> R {
    crate::sexp::instance::with_required_current_instance(|owner| unsafe {
        with_finalizer_state_in(owner, f)
    })
}

/// Returns whether a GC is currently running.
///
/// This is the equivalent of R's `R_gc_running()`.
pub fn R_gc_running() -> c_int {
    crate::sexp::instance::with_current_instance(|inst| unsafe { gc_running_in(inst) }).unwrap_or(0)
}

/// # Safety
/// `instance` must point to a live allocation for this read.
pub(crate) unsafe fn gc_running_in(instance: *mut RInstance) -> c_int {
    unsafe { with_memory_state_in(instance, |state| state.in_gc) }
}

/// # Safety
/// `instance` must point to a live allocation for this update.
#[cfg(test)]
pub(crate) unsafe fn set_gc_torture_for_test_in(
    instance: *mut RInstance,
    gap: c_int,
    wait: Option<c_int>,
) {
    unsafe {
        with_memory_state_in(instance, |state| {
            state.gc_force_gap = gap;
            if let Some(wait) = wait {
                state.gc_force_wait = wait;
            }
        });
    }
}

/// Own the exact physical GC state until its collection scope ends.
/// Revocation denies new work while cleanup restores the original prior flag.
pub(crate) struct GcOperationGuard {
    owner: crate::sexp::owner::OwnerPin,
    previous_in_gc: Option<c_int>,
}

impl GcOperationGuard {
    pub(crate) fn enter(
        owner: crate::sexp::owner::OwnerPin,
        full: bool,
    ) -> crate::sexp::object::SexpResult<Self> {
        owner.require_live()?;
        let instance = owner.as_ptr();
        let previous_in_gc = unsafe {
            with_memory_state_in(instance, |state| {
                state.gc_count = state.gc_count.wrapping_add(1);
                full.then(|| std::mem::replace(&mut state.in_gc, 1))
            })
        };
        Ok(Self {
            owner,
            previous_in_gc,
        })
    }
}

impl Drop for GcOperationGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous_in_gc {
            // The pin retains this exact allocation even after callback teardown.
            // No ambient lookup or callback is permitted during state cleanup.
            unsafe { with_memory_state_in(self.owner.as_ptr(), |state| state.in_gc = previous) };
        }
    }
}

fn collect_user_gc(full: bool) -> crate::sexp::object::SexpResult<()> {
    use crate::sexp::{object::SexpError, owner::with_runtime};
    let owner = crate::sexp::instance::with_required_current_instance(|instance| unsafe {
        (*instance).runtime_owner.clone()
    })
    .ok_or(SexpError::RootUnavailable)?;
    with_runtime(&owner, |access| {
        let collection = GcOperationGuard::enter(owner.pin()?, full)?;
        access.with_native(|_| {
            crate::sexp::gengc::collect_with_environment_protects(full);
            Ok(())
        })?;
        // Finalizers observe the prior flag, as they did before this operation.
        drop(collection);
        access.with_native(|_| {
            unsafe { R_RunPendingFinalizers() };
            Ok(())
        })
    })?
}

/// Trigger a full garbage collection, retaining original cleanup authority.
///
/// This is the equivalent of R's `R_gc()`.
pub fn R_gc() {
    if let Err(failure) = collect_user_gc(true) {
        error(&failure.to_string());
    }
}

/// Trigger a lightweight garbage collection in the exact original runtime.
pub fn R_gc_lite() {
    if let Err(failure) = collect_user_gc(false) {
        error(&failure.to_string());
    }
}
/// GC torture settings.
///
/// When `gap > 0`, every `gap` allocations will force a GC cycle.
/// This is a debugging aid for finding GC-safety bugs.
pub fn R_gc_torture(gap: c_int, wait: c_int, _inhibit: c_int) {
    with_memory_state(|state| {
        if gap != NA_INTEGER && gap >= 0 {
            state.gc_force_gap = gap;
        }
        if gap > 0 && wait != NA_INTEGER && wait > 0 {
            state.gc_force_wait = wait;
        }
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TortureAction {
    Skip,
    Defer,
    Collect,
}

/// Consume a batch of deferred allocation ticks without holding either state
/// borrow across a collection. An in-progress cycle defers without counting.
///
/// # Safety
/// `instance` must point to a live allocation throughout the update.
pub(crate) unsafe fn torture_action_in(instance: *mut RInstance, ticks: u32) -> TortureAction {
    unsafe {
        with_memory_state_in(instance, |state| {
            if state.gc_force_gap <= 0 {
                return TortureAction::Skip;
            }
            // These are separate cells; reading in_progress cannot reenter R.
            if state.in_gc != 0 || with_gc_state_in(instance, |gc| gc.in_progress) {
                return TortureAction::Defer;
            }
            let ticks = c_int::try_from(ticks).unwrap_or(c_int::MAX);
            if state.gc_force_wait > ticks {
                state.gc_force_wait -= ticks;
                TortureAction::Skip
            } else {
                state.gc_force_wait = state.gc_force_gap;
                TortureAction::Collect
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Transient allocation — duplicates (no #[unsafe(no_mangle)] to avoid conflicts)
//
// R_alloc, vmaxget, vmaxset are already in sexp/memory_ext.rs.
// ---------------------------------------------------------------------------




// ---------------------------------------------------------------------------
// GC-on-failure allocation wrappers
// ---------------------------------------------------------------------------




// ---------------------------------------------------------------------------
// Memory checking utilities
// ---------------------------------------------------------------------------






// ---------------------------------------------------------------------------
// sexptype2char
// ---------------------------------------------------------------------------

/// Convert a SEXPTYPE to its string name.
///
/// This is the equivalent of R's `sexptype2char()`.
pub fn sexptype2char(type_: SEXPTYPE) -> *const c_char {
    let val = type_.0;
    match val {
        0 => b"NILSXP\0".as_ptr() as *const c_char,      // NILSXP
        1 => b"SYMSXP\0".as_ptr() as *const c_char,      // SYMSXP
        2 => b"LISTSXP\0".as_ptr() as *const c_char,     // LISTSXP
        3 => b"CLOSXP\0".as_ptr() as *const c_char,      // CLOSXP
        4 => b"ENVSXP\0".as_ptr() as *const c_char,      // ENVSXP
        5 => b"PROMSXP\0".as_ptr() as *const c_char,     // PROMSXP
        6 => b"LANGSXP\0".as_ptr() as *const c_char,     // LANGSXP
        7 => b"SPECIALSXP\0".as_ptr() as *const c_char,  // SPECIALSXP
        8 => b"BUILTINSXP\0".as_ptr() as *const c_char,  // BUILTINSXP
        9 => b"CHARSXP\0".as_ptr() as *const c_char,     // CHARSXP
        10 => b"LGLSXP\0".as_ptr() as *const c_char,     // LGLSXP
        13 => b"INTSXP\0".as_ptr() as *const c_char,     // INTSXP
        14 => b"REALSXP\0".as_ptr() as *const c_char,    // REALSXP
        15 => b"CPLXSXP\0".as_ptr() as *const c_char,    // CPLXSXP
        16 => b"STRSXP\0".as_ptr() as *const c_char,     // STRSXP
        17 => b"DOTSXP\0".as_ptr() as *const c_char,     // DOTSXP
        18 => b"ANYSXP\0".as_ptr() as *const c_char,     // ANYSXP
        19 => b"VECSXP\0".as_ptr() as *const c_char,     // VECSXP
        20 => b"EXPRSXP\0".as_ptr() as *const c_char,    // EXPRSXP
        21 => b"BCODESXP\0".as_ptr() as *const c_char,   // BCODESXP
        22 => b"EXTPTRSXP\0".as_ptr() as *const c_char,  // EXTPTRSXP
        23 => b"WEAKREFSXP\0".as_ptr() as *const c_char, // WEAKREFSXP
        25 => b"OBJSXP\0".as_ptr() as *const c_char,     // OBJSXP
        24 => b"RAWSXP\0".as_ptr() as *const c_char,     // RAWSXP
        _ => b"<unknown>\0".as_ptr() as *const c_char,
    }
}

// ---------------------------------------------------------------------------
// Type checking functions (duplicates — no #[unsafe(no_mangle)])
//
// Rf_isNull is in sexp/accessors.rs and mainutils/print.rs.
// Rf_isSymbol, Rf_isReal, etc. are in sexp/constructors.rs.
// ---------------------------------------------------------------------------










// ---------------------------------------------------------------------------
// R_PreserveObject / R_ReleaseObject (duplicates — no #[unsafe(no_mangle)])
//
// Already in sexp/protect.rs with #[unsafe(no_mangle)].
// ---------------------------------------------------------------------------



// ---------------------------------------------------------------------------
// Weak references and finalizers
// ---------------------------------------------------------------------------

fn make_weak_ref(key: SEXP, val: SEXP, fin: SEXP, register: impl FnOnce()) -> SEXP {
    validate_weak_reference_key(key);
    if !is_valid_r_finalizer(fin) {
        error("finalizer must be a function or NULL");
    }
    checked_memory(OwnerToken::with_current(|owner| {
        let factory = owner.node_factory();
        // Keep the key alive through deferred collection and registration;
        // the published key edge is weak and does not take over this root.
        let inputs = [key, val, fin].map(|raw| {
            (!raw.is_null()).then(|| checked_memory(factory.wrap(raw)))
        });
        let [carval, cdrval, tagval] = inputs.each_ref().map(|value| match value {
            Some(value) => checked_memory(factory.link(value)),
            None => NodeLink::NULL,
        });
        let body = NodeBody::List(Listsxp {
            carval,
            cdrval,
            tagval,
        });
        let result = checked_memory(factory.allocate(|arena| {
            let raw = arena.alloc_node(SEXPTYPE::WEAKREFSXP);
            let node = arena.node_token(raw)?;
            let heap = arena.heap_identity();
            let mut header = heap.node_snapshot(&node)?;
            header.data = body;
            heap.replace_node(&node, header)?;
            Some(raw)
        }));
        register();
        result.as_raw()
    }))
}

/// Create a weak reference with an R finalizer.
///
/// This is the equivalent of R's `R_MakeWeakRef()`.
pub fn R_MakeWeakRef(key: SEXP, val: SEXP, fin: SEXP, onexit: c_int) -> SEXP {
    make_weak_ref(key, val, fin, || {
        if key != R_NilValue() && !fin.is_null() && fin != R_NilValue() {
            register_r_finalizer(key, fin, onexit != 0);
        }
    })
}


pub fn R_WeakRefKey(w: SEXP) -> SEXP {
    {
        if w.is_null() {
            return R_NilValue();
        }
        validate_weak_ref_object(w);
        CAR(w)
    }
}


pub fn R_RegisterFinalizerEx(s: SEXP, fun: SEXP, onexit: c_int) {
    {
        if s.is_null() || fun.is_null() || fun == R_NilValue() {
            return;
        }
        validate_weak_reference_key(s);
        if !is_valid_r_finalizer(fun) {
            error("finalizer must be a function or NULL");
        }
        register_r_finalizer(s, fun, onexit != 0);
    }
}

pub fn R_RegisterFinalizer(s: SEXP, fun: SEXP) {
    R_RegisterFinalizerEx(s, fun, 0);
}


pub fn R_RegisterCFinalizer(s: SEXP, fun: R_CFinalizer_t) {
    R_RegisterCFinalizerEx(s, fun, 0);
}

pub fn R_RegisterCFinalizerEx(s: SEXP, fun: R_CFinalizer_t, _onexit: c_int) {
    {
        if s.is_null() {
            return;
        }
        validate_weak_reference_key(s);
        register_c_finalizer(s, fun, _onexit != 0);
    }
}

/// Reset only the original owner's running flag, even if a finalizer switches
/// the ambient owner, unwinds, or destroys its session. The observer does not
/// keep the instance allocation alive.
struct FinalizerRunGuard {
    owner: *mut crate::sexp::instance::RInstance,
    liveness: crate::sexp::instance::InstanceLiveness,
}

impl Drop for FinalizerRunGuard {
    fn drop(&mut self) {
        if self.liveness.is_live() {
            // SAFETY: the exact captured owner remains alive on this thread;
            // no instance/state borrow is held across a finalizer callback.
            unsafe { with_finalizer_state_in(self.owner, |state| state.running_finalizers = false) };
        }
    }
}

pub unsafe fn R_RunPendingFinalizers() {
    let owner = crate::sexp::instance::with_required_current_instance(|owner| owner);
    // SAFETY: the ambient owner is live on entry. Retain only its weak teardown
    // observer; callbacks may destroy the original session.
    let liveness = unsafe { crate::sexp::instance::instance_liveness(owner) };
    let mut finalizers = with_finalizer_state(|state| {
        if state.running_finalizers {
            return Vec::new();
        }
        // Keep originals GC-visible until every execution root is claimed.
        // Snapshot allocation failure also leaves the queue and flag intact.
        let ready: Vec<_> = state
            .pending_finalizers
            .iter()
            .filter(|finalizer| finalizer.is_ready())
            .cloned()
            .collect();
        if !ready.is_empty() {
            state.running_finalizers = true;
        }
        ready
    });
    if finalizers.is_empty() {
        return;
    }
    let run = FinalizerRunGuard { owner, liveness };
    // SAFETY: root claims cannot invoke R callbacks. The owner remains live
    // throughout this preparation, and all field borrows end before dispatch.
    let _batch_roots = unsafe {
        crate::sexp::session::with_instance_active(owner, || {
            let mut roots = Vec::new();
            for finalizer in &mut finalizers {
                let object = canonical_finalizer_value(owner, finalizer.obj());
                *finalizer.obj_mut() = object;
                roots.push(crate::sexp::protect::protect(object));
                if let Some(function) = finalizer.fun_mut() {
                    *function = canonical_finalizer_value(owner, *function);
                    roots.push(crate::sexp::protect::protect(*function));
                }
            }
            // A failed claim unwinds the partial roots and running guard while
            // retaining the complete original queue for a later retry.
            with_finalizer_state_in(owner, |state| {
                state
                    .pending_finalizers
                    .retain(|finalizer| !finalizer.is_ready());
            });
            roots
        })
    };

    for finalizer in finalizers {
        if !run.liveness.is_live() {
            break;
        }
        // SAFETY: validate teardown before every activation. The activation
        // guard and root guards also check teardown before restoring/releasing.
        unsafe {
            crate::sexp::session::with_instance_active(owner, || {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_pending_finalizer(finalizer);
                }));
            });
        }
    }
}

/// Recover the owning projection instead of dereferencing caller provenance.
/// # Safety
/// `owner` must remain live throughout this callback-free preparation.
unsafe fn canonical_finalizer_value(
    owner: *mut crate::sexp::instance::RInstance,
    value: SEXP,
) -> SEXP {
    if value.is_null() {
        return value;
    }
    if let Some(singleton) = crate::sexp::session::immutable_singleton_projection(value) {
        return singleton;
    }
    unsafe { (*owner).canonical_projection(value) }
        .expect("finalizer requires a live allocation owned by its original session")
}

fn run_pending_finalizer(finalizer: PendingFinalizer) {
    unsafe {
        match finalizer {
            PendingFinalizer::C { obj, fun, .. } => {
                if !obj.is_null() {
                    fun(obj as *mut c_void);
                }
            }
            PendingFinalizer::R { obj, fun, .. } => {
                if !obj.is_null() && !fun.is_null() && fun != R_NilValue() {
                    // Publish the complete call graph and its root before the
                    // allocation lend ends and deferred torture GC can run.
                    // Separate Rf_cons lends would expose an unrooted tail.
                    let (call, _call_root) = crate::sexp::memory::with_arena(|arena| {
                        let tail = arena.cons(obj, R_NilValue(), R_NilValue());
                        if tail.is_null() {
                            error("could not allocate finalizer arguments");
                        }
                        let call = arena.cons(fun, tail, R_NilValue());
                        if call.is_null() {
                            error("could not allocate finalizer call");
                        }
                        crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
                        (call, crate::sexp::protect::protect(call))
                    });
                    let _ = crate::eval::eval::Rf_eval(call, R_GlobalEnv());
                }
            }
        }
    }
}

fn register_c_finalizer(obj: SEXP, fun: R_CFinalizer_t, onexit: bool) {
    with_finalizer_state(|state| {
        state.pending_finalizers.push(PendingFinalizer::C {
            obj,
            fun,
            ready: false,
            onexit,
        });
    });
}

fn register_r_finalizer(obj: SEXP, fun: SEXP, onexit: bool) {
    with_finalizer_state(|state| {
        state.pending_finalizers.push(PendingFinalizer::R {
            obj,
            fun,
            ready: false,
            onexit,
        });
    });
}

fn validate_weak_reference_key(key: SEXP) {
    {
        if key.is_null() {
            return;
        }
        match SEXPTYPE(TYPEOF(key)) {
            SEXPTYPE::NILSXP | SEXPTYPE::ENVSXP | SEXPTYPE::EXTPTRSXP | SEXPTYPE::BCODESXP => {}
            _ => error("can only weakly reference/finalize reference objects"),
        }
    }
}

fn validate_weak_ref_object(w: SEXP) {
    {
        if SEXPTYPE(TYPEOF(w)) != SEXPTYPE::WEAKREFSXP {
            error("not a weak reference");
        }
    }
}

fn is_valid_r_finalizer(fun: SEXP) -> bool {
    {
        if fun.is_null() || fun == R_NilValue() {
            return true;
        }
        matches!(
            SEXPTYPE(TYPEOF(fun)),
            SEXPTYPE::CLOSXP | SEXPTYPE::BUILTINSXP | SEXPTYPE::SPECIALSXP
        )
    }
}


/// Mark ready keys and all R finalizer functions. The trace only visits
/// checked heap metadata and cannot run R code or invoke providers.
///
/// # Safety
/// `owner` is the live instance for the current collection.
pub(crate) unsafe fn mark_finalizer_roots_in(owner: *mut crate::sexp::instance::RInstance) {
    // SAFETY: the collector retains this owner during callback-free tracing.
    unsafe {
        with_finalizer_state_in(owner, |state| {
            for finalizer in &state.pending_finalizers {
                if finalizer.is_ready() {
                    crate::sexp::gengc::mark_reachable(finalizer.obj());
                }
                if let PendingFinalizer::R { fun, .. } = finalizer {
                    crate::sexp::gengc::mark_reachable(*fun);
                }
            }
        });
    }
}

/// Make unreachable keys ready and snapshot all ready, non-null keys to trace.
///
/// # Safety
/// `owner` is the live instance for the current collection.
pub(crate) unsafe fn ready_finalizer_roots_in(
    owner: *mut crate::sexp::instance::RInstance,
    unreachable: &std::collections::HashSet<usize>,
) -> Vec<SEXP> {
    // SAFETY: the collector retains this owner during queue bookkeeping.
    unsafe {
        with_finalizer_state_in(owner, |state| {
            let mut roots = Vec::new();
            for finalizer in &mut state.pending_finalizers {
                let obj = finalizer.obj();
                if obj.is_null() {
                    continue;
                }
                if unreachable.contains(&(obj as usize)) {
                    finalizer.set_ready();
                }
                if finalizer.is_ready() {
                    roots.push(obj);
                }
            }
            roots
        })
    }
}

/// Apply an immutable sweep map without lending the queue to a visitor.
///
/// # Safety
/// `owner` is the live instance for the current collection.
pub(crate) unsafe fn remap_finalizer_roots_in(
    owner: *mut crate::sexp::instance::RInstance,
    old_to_new: &std::collections::HashMap<usize, SEXP>,
) {
    use crate::sexp::gengc::update_field;
    // SAFETY: the collector retains the owner and remap does not run R code.
    unsafe {
        with_finalizer_state_in(owner, |state| {
            for finalizer in &mut state.pending_finalizers {
                update_field(finalizer.obj_mut(), old_to_new);
                if let Some(fun) = finalizer.fun_mut() {
                    update_field(fun, old_to_new);
                }
            }
        });
    }
}

#[cfg(test)]
pub(crate) unsafe fn clear_finalizers_for_test(owner: *mut crate::sexp::instance::RInstance) {
    unsafe { with_finalizer_state_in(owner, |state| state.pending_finalizers.clear()) };
}

#[cfg(test)]
pub(crate) unsafe fn queue_ready_c_finalizer_for_test(
    owner: *mut crate::sexp::instance::RInstance,
    obj: SEXP,
    fun: R_CFinalizer_t,
) {
    unsafe {
        with_finalizer_state_in(owner, |state| {
            state.pending_finalizers.push(PendingFinalizer::C {
                obj,
                fun,
                ready: true,
                onexit: false,
            });
        });
    }
}

pub(crate) unsafe fn R_RunExitFinalizers_memory() {
    unsafe {
        mark_exit_finalizers_ready();
        R_RunPendingFinalizers();
    }
}

fn mark_exit_finalizers_ready() {
    with_finalizer_state(|state| {
        for finalizer in &mut state.pending_finalizers {
            if finalizer.is_onexit() {
                finalizer.set_ready();
            }
        }
    });
}

// ---------------------------------------------------------------------------
// External pointer management
//
// The opaque native address is independent from the protected value and tag.
// Graph children carry exact heap links; clearing the address keeps both links.
// ---------------------------------------------------------------------------

fn external_pointer_child(s: SEXP, field: crate::sexp::ffi::EdgeField) -> SEXP {
    let (_, parent) = crate::sexp::memory::checked_projection(s).expect("checked external pointer");
    let heap = parent.heap_identity();
    let link = heap
        .edge(&parent, field)
        .expect("external pointer graph field");
    heap.projection_of_link(link)
        .expect("live external pointer child")
}

fn set_external_pointer_child(s: SEXP, field: crate::sexp::ffi::EdgeField, child: SEXP) {
    let (_, parent) = crate::sexp::memory::checked_projection(s).expect("checked external pointer");
    let heap = parent.heap_identity();
    let link = heap
        .link_from_projection(child)
        .expect("external pointer child belongs to its heap");
    crate::sexp::gengc::write_barrier(s, child);
    let mut header = heap.node_snapshot(&parent).expect("live external pointer");
    header
        .set_edge(field, link)
        .expect("external pointer graph field");
    heap.replace_node(&parent, header)
        .expect("external pointer child publication");
}

/// Create an external pointer.
///
/// This is the equivalent of R's `R_MakeExternalPtr()`.
/// The address is stored opaquely and never dereferenced.
pub fn R_MakeExternalPtr(p: *mut c_void, tag: SEXP, prot: SEXP) -> SEXP {
    checked_memory(OwnerToken::with_current(|owner| {
        let factory = owner.node_factory();
        let tag = (!tag.is_null()).then(|| checked_memory(factory.wrap(tag)));
        let protected = (!prot.is_null()).then(|| checked_memory(factory.wrap(prot)));
        let link = |value: &Option<Sexp<'_>>| match value {
            Some(value) => checked_memory(factory.link(value)),
            None => NodeLink::NULL,
        };
        let body = ExtPtrBody {
            address: p,
            protected: link(&protected),
            tag: link(&tag),
        };
        // tag/prot are lexical roots across this deferred allocation; the
        // initialized header is published before collection can observe it.
        let result = checked_memory(factory.allocate(|arena| {
            let raw = arena.alloc_node(SEXPTYPE::EXTPTRSXP);
            let node = arena.node_token(raw)?;
            let heap = arena.heap_identity();
            let mut header = heap.node_snapshot(&node)?;
            header.data = NodeBody::ExtPtr(body);
            heap.replace_node(&node, header)?;
            Some(raw)
        }));
        result.as_raw()
    }))
}

/// Get the address stored in an external pointer.
///
/// This is the equivalent of R's `R_ExternalPtrAddr()`.
/// The returned address is opaque; this accessor does not dereference it.
pub fn R_ExternalPtrAddr(s: SEXP) -> *mut c_void {
    if s.is_null() {
        return ptr::null_mut();
    }
    let (_, _, header) =
        crate::sexp::memory::checked_header(s).unwrap_or_else(|| error("unowned external pointer"));
    let crate::sexp::ffi::NodeBody::ExtPtr(body) = header.data else {
        error("expected external pointer");
    };
    body.address
}

/// Get the tag of an external pointer.
///
/// This is the equivalent of R's `R_ExternalPtrTag()`.
pub fn R_ExternalPtrTag(s: SEXP) -> SEXP {
    {
        if s.is_null() {
            return R_NilValue();
        }
        external_pointer_child(s, crate::sexp::ffi::EdgeField::ExternalTag)
    }
}

/// Get the protected value of an external pointer.
///
/// This is the equivalent of R's `R_ExternalPtrProtected()`.
pub fn R_ExternalPtrProtected(s: SEXP) -> SEXP {
    {
        if s.is_null() {
            return R_NilValue();
        }
        external_pointer_child(s, crate::sexp::ffi::EdgeField::ExternalProtected)
    }
}

/// Clear the address stored in an external pointer.
///
/// This is the equivalent of R's `R_ClearExternalPtr()`.
pub fn R_ClearExternalPtr(s: SEXP) {
    R_SetExternalPtrAddr(s, ptr::null_mut());
}

/// Set the address stored in an external pointer.
///
/// This is the equivalent of R's `R_SetExternalPtrAddr()`.
pub fn R_SetExternalPtrAddr(s: SEXP, p: *mut c_void) {
    if s.is_null() {
        return;
    }
    checked_memory(OwnerToken::with_current(|owner| {
        let value = checked_memory(owner.sexp(s));
        if value.typeof_() != SEXPTYPE::EXTPTRSXP {
            error("expected external pointer");
        }
        let node = checked_memory(value.allocation());
        let heap = node.heap_identity();
        let mut header = heap
            .node_snapshot(node)
            .unwrap_or_else(|| error("unowned external pointer"));
        let NodeBody::ExtPtr(ref mut body) = header.data else {
            error("expected external pointer");
        };
        body.address = p;
        heap.replace_node(node, header)
            .unwrap_or_else(|| error("could not set external pointer address"));
        // No callback or arena lend between publication and revocation.
        crate::mainutils::native_symbols::invalidate(node);
    }));
}

/// Set the tag of an external pointer.
///
/// This is the equivalent of R's `R_SetExternalPtrTag()`.
pub fn R_SetExternalPtrTag(s: SEXP, tag: SEXP) {
    {
        if !s.is_null() {
            set_external_pointer_child(s, crate::sexp::ffi::EdgeField::ExternalTag, tag);
        }
    }
}

/// Set the protected value of an external pointer.
///
/// This is the equivalent of R's `R_SetExternalPtrProtected()`.
pub fn R_SetExternalPtrProtected(s: SEXP, p: SEXP) {
    {
        if !s.is_null() {
            set_external_pointer_child(s, crate::sexp::ffi::EdgeField::ExternalProtected, p);
        }
    }
}

// ---------------------------------------------------------------------------
// Heap size limits (duplicates — no #[unsafe(no_mangle)])
//
// R_GetMaxVSize and R_GetMaxNSize are in mainutils/main.rs.
// ---------------------------------------------------------------------------

/// Get the maximum vector heap size.
/// Duplicate — no #[unsafe(no_mangle)] (already in mainutils/main.rs).
pub(crate) fn R_GetMaxVSize_memory() -> u64 {
    crate::sexp::instance::with_current_instance(|inst| unsafe {
        with_memory_state_in(inst, |state| state.max_v_size)
    })
    .unwrap_or(u64::MAX)
}

/// Get the maximum node heap size.
/// Duplicate — no #[unsafe(no_mangle)] (already in mainutils/main.rs).
pub(crate) fn R_GetMaxNSize_memory() -> u64 {
    crate::sexp::instance::with_current_instance(|inst| unsafe {
        with_memory_state_in(inst, |state| state.max_n_size)
    })
    .unwrap_or(u64::MAX)
}




fn current_vector_heap_size() -> u64 {
    unsafe { crate::sexp::memory::with_arena(|arena| arena.total_bytes_allocated() as u64) }
}

fn current_node_heap_size() -> u64 {
    unsafe { crate::sexp::memory::with_arena(|arena| arena.node_count() as u64) }
}

// ---------------------------------------------------------------------------
// Console I/O (duplicates — no #[unsafe(no_mangle)])
//
// R_ReadConsole and R_WriteConsole are in unix/system.rs.
// ---------------------------------------------------------------------------



// ---------------------------------------------------------------------------
// readline wrapper

/// GNU readline-compatible wrapper.
///
/// This is the equivalent of R's `readline()` function.
pub unsafe fn readline(prompt: *const c_char) -> *mut c_char {
    let mut buffer = vec![0u8; 8192];
    // SAFETY: forwarded raw pointers satisfy `R_ReadConsole`'s contract per this function's own `# Safety` requirements.
    let ok = unsafe { crate::unix::system::R_ReadConsole(prompt, buffer.as_mut_ptr(), 8192, 1) };
    if ok == 0 {
        return ptr::null_mut();
    }
    let nul = buffer
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(buffer.len());
    let line = std::ffi::CString::new(&buffer[..nul]).unwrap_or_default();
    // strdup equivalent: hand out the NUL-terminated heap buffer itself
    // (nothing in this port frees the readline result, so ownership just leaks)
    line.into_raw()
}

// ---------------------------------------------------------------------------
// R_allocStringBuffer
// ---------------------------------------------------------------------------

/// Represents an R string buffer.
#[repr(C)]
pub struct R_StringBuffer {
    pub data: *mut c_char,
    pub bufsize: usize,
    pub defaultSize: usize,
}

impl Default for R_StringBuffer {
    fn default() -> Self {
        R_StringBuffer {
            data: ptr::null_mut(),
            bufsize: 0,
            defaultSize: 4096,
        }
    }
}

/// Allocate or grow a string buffer.
///
/// This is the equivalent of R's `R_AllocStringBuffer()`.
pub unsafe fn R_AllocStringBuffer(blen: usize, buf: *mut R_StringBuffer) -> *mut c_void {
    unsafe {
        if buf.is_null() {
            error("R_AllocStringBuffer called with null buffer");
        }
        let buf = &mut *buf;

        if blen == usize::MAX {
            error("R_AllocStringBuffer( (size_t)-1 ) is no longer allowed");
        }

        let needed = (blen + 1) * std::mem::size_of::<c_char>();
        if !buf.data.is_null() && needed <= buf.bufsize {
            return buf.data as *mut c_void;
        }

        let bsize = buf.defaultSize.max(1);
        let newsize = needed
            .div_ceil(bsize)
            .checked_mul(bsize)
            .unwrap_or_else(|| error("string buffer size overflow"));

        // The buffer is a Vec<u8> handed out as a raw pointer; `bufsize`
        // always equals its capacity so it can be reconstructed on the
        // next grow and in R_FreeStringBuffer. Only a fresh block is
        // NUL-terminated here; growth keeps the accumulated contents.
        let mut v: Vec<u8> = if buf.data.is_null() {
            let mut v = Vec::with_capacity(newsize);
            v.push(0);
            v
        } else {
            let mut v = Vec::from_raw_parts(buf.data as *mut u8, 0, buf.bufsize);
            // reserve_exact is relative to length (zero), not capacity.
            // Reserving only the capacity delta could leave the allocation
            // unchanged and allow the caller to write past its end.
            v.reserve_exact(newsize);
            v
        };
        buf.data = v.as_mut_ptr() as *mut c_char;
        buf.bufsize = v.capacity();
        std::mem::forget(v);

        buf.data as *mut c_void
    }
}


/// Free a string buffer only if it is larger than defaultSize.
///
/// This is the equivalent of R's `R_FreeStringBufferL()`.
pub unsafe fn R_FreeStringBufferL(buf: *mut R_StringBuffer) {
    unsafe {
        if buf.is_null() {
            return;
        }
        let buf = &mut *buf;
        if buf.bufsize > buf.defaultSize {
            if !buf.data.is_null() {
                drop(Vec::from_raw_parts(buf.data as *mut u8, 0, buf.bufsize));
                buf.data = ptr::null_mut();
            }
            buf.bufsize = 0;
        }
    }
}

fn checked_memory<T>(result: SexpResult<T>) -> T {
    result.unwrap_or_else(|failure| error(&failure.to_string()))
}

fn mset_check<T>(result: SexpResult<T>) -> T {
    result.unwrap_or_else(|_| error("Invalid mset"))
}

/// Rooted view of the `CONS(store, counter)` precious multiset cells.
/// `TAG() == initialSize` retains GNU's representation.
struct MSet<'s> {
    store: Sexp<'s>,
    count: Sexp<'s>,
    initial: Sexp<'s>,
    len: c_int,
}

impl<'s> MSet<'s> {
    fn read(mset: &Sexp<'s>) -> Self {
        if mset.typeof_() != SEXPTYPE::LISTSXP {
            error("Invalid mset");
        }
        let store = mset_check(mset.try_car());
        let count = mset_check(mset.try_cdr());
        let initial = mset_check(mset.try_tag());
        // Dense storage only: element operations must not invoke
        // ALTREP provider callbacks while the multiset is mutated.
        let scalar = |s: &Sexp<'_>| {
            s.typeof_() == SEXPTYPE::INTSXP && s.len() == 1 && ALTREP(s.as_raw()) == 0
        };
        if (!store.is_nil() && (store.typeof_() != SEXPTYPE::VECSXP || ALTREP(store.as_raw()) != 0))
            || !scalar(&count)
            || !scalar(&initial)
        {
            error("Invalid mset");
        }
        let len = mset_check(count.try_integer_elt(0));
        if len < 0 || i64::from(len) > store.len() {
            error("Invalid mset");
        }
        Self {
            store,
            count,
            initial,
            len,
        }
    }
}




// ---------------------------------------------------------------------------
// Resizable vector support
// ---------------------------------------------------------------------------





// ---------------------------------------------------------------------------
// Protection stack error handlers
// ---------------------------------------------------------------------------



// ---------------------------------------------------------------------------
// InitMemory / initStack stubs
// ---------------------------------------------------------------------------



// ---------------------------------------------------------------------------
// Memory profile
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// do_* stubs for GC-related .Internal / .Primitive calls
// ---------------------------------------------------------------------------


/// gcinfo() implementation (stub).
///
/// This is the equivalent of R's `do_gcinfo()`.
pub fn do_gcinfo(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    if args.is_null() || args == R_NilValue() || CAR(args) == R_MissingArg() {
        crate::mainutils::essentials::base_error("argument \"verbose\" is missing, with no default");
    }
    let old = Rf_ScalarLogical(with_memory_state(|state| state.gc_reporting));
    let i = crate::mainutils::coerce::asLogical(CAR(args));
    if i != crate::sexp::ffi::NA_LOGICAL {
        with_memory_state(|state| state.gc_reporting = i);
    }
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    old
}

pub fn do_gctorture(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let old = Rf_ScalarLogical(if with_memory_state(|state| state.gc_force_gap) > 0 {
        crate::sexp::ffi::TRUE
    } else {
        crate::sexp::ffi::FALSE
    });
    let on = if args.is_null() || args == R_NilValue() || CAR(args) == R_MissingArg() {
        Rf_ScalarLogical(crate::sexp::ffi::TRUE)
    } else {
        CAR(args)
    };
    let gap = crate::mainutils::coerce::asLogical(on);
    R_gc_torture(if gap != 0 { 1 } else { 0 }, 0, 0);
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    old
}

pub fn do_gctorture2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    if args.is_null() || args == R_NilValue() || CAR(args) == R_MissingArg() {
        crate::mainutils::essentials::base_error("argument \"step\" is missing, with no default");
    }
    let old = Rf_ScalarInteger(with_memory_state(|state| state.gc_force_gap));
    let gap = crate::mainutils::coerce::asInteger(CAR(args));
    let wait = if CDR(args).is_null() || CDR(args) == R_NilValue() || CAR(CDR(args)) == R_MissingArg()
    {
        0
    } else {
        crate::mainutils::coerce::asInteger(CAR(CDR(args)))
    };
    R_gc_torture(gap, wait, 0);
    crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
    old
}




// ---------------------------------------------------------------------------
// Seql (string equality, encoding-aware)
// ---------------------------------------------------------------------------

/// String equality test, encoding-aware.
///
/// This is the equivalent of R's `Seql()`.
pub fn Seql(a: SEXP, b: SEXP) -> c_int {
    crate::mainutils::relop::Seql(a, b)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::session::RSession;

    // Finalizers run on the owning session's thread. Parallel tests must not
    // reset or increment another session's assertion counter.
    thread_local! {
        static FINALIZER_RUNS: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };
    }

    #[test]
    fn test_sexptype2char_basic() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(
                std::ffi::CStr::from_ptr(sexptype2char(SEXPTYPE::NILSXP))
                    .to_str()
                    .unwrap_or(""),
                "NILSXP"
            );
            assert_eq!(
                std::ffi::CStr::from_ptr(sexptype2char(SEXPTYPE::REALSXP))
                    .to_str()
                    .unwrap_or(""),
                "REALSXP"
            );
            assert_eq!(
                std::ffi::CStr::from_ptr(sexptype2char(SEXPTYPE::STRSXP))
                    .to_str()
                    .unwrap_or(""),
                "STRSXP"
            );
        }
    }

    #[test]
    fn test_gc_running() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let running = R_gc_running();
            assert_eq!(running, 0); // GC should not be running initially
        }
    }

    #[test]
    fn test_gc_does_not_crash() {
        let _session = crate::sexp::session::RSession::new();
        let session = RSession::new();
        session.with_protected(|| {
            R_gc();
            R_gc_lite();
            // Should not crash
        });
    }




    #[test]
    fn string_buffer_small_growth_preserves_contents_and_capacity() {
        // No interpreter needed: exercise the raw allocation boundary under Miri.
        unsafe {
            let mut buf = R_StringBuffer {
                defaultSize: 16,
                ..Default::default()
            };
            R_AllocStringBuffer(15, &mut buf);
            for i in 0..15 {
                *buf.data.add(i) = b'a' as c_char;
            }
            *buf.data.add(15) = 0;
            R_AllocStringBuffer(20, &mut buf);
            assert!(buf.bufsize >= 21);
            for i in 0..15 {
                assert_eq!(*buf.data.add(i), b'a' as c_char);
            }
            for i in 15..20 {
                *buf.data.add(i) = b'b' as c_char;
            }
            *buf.data.add(20) = 0;
            assert_eq!(std::ffi::CStr::from_ptr(buf.data).to_bytes().len(), 20);
            R_FreeStringBuffer(&mut buf);
        }
    }

    #[test]
    fn test_r_string_buffer_grow() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut buf = R_StringBuffer::default();
            // Initial allocation
            let _ = R_AllocStringBuffer(10, &mut buf);
            // Grow
            let ptr = R_AllocStringBuffer(10000, &mut buf);
            assert!(!ptr.is_null());
            R_FreeStringBufferL(&mut buf);
        }
    }

    #[test]
    fn test_r_string_buffer_default_size() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut buf = R_StringBuffer::default();
            // Small allocation within default size
            let ptr = R_AllocStringBuffer(10, &mut buf);
            assert!(!ptr.is_null());
            // Should be within default size
            assert!(buf.bufsize >= buf.defaultSize);
            R_FreeStringBufferL(&mut buf);
            // Should NOT free since within default size
            // (In the stub it might still be allocated, just check no crash)
        }
    }


    #[test]
    fn test_weak_ref_rejects_non_reference_key_like_gnu_r() {
        let _session = crate::sexp::session::RSession::new();
        let key = crate::sexp::constructors::Rf_ScalarInteger(42);
        let val = crate::sexp::constructors::Rf_ScalarInteger(99);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = R_MakeWeakRef(key, val, R_NilValue(), 0);
        }));
        assert!(result.is_err());
    }


    #[test]
    fn test_external_ptr_null_roundtrip() {
        let _session = crate::sexp::session::RSession::new();
        {
            // Create with null values
            let p = R_MakeExternalPtr(ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
            assert!(!p.is_null()); // Should allocate something

            let addr = R_ExternalPtrAddr(p);
            assert!(addr.is_null());

            let tag = R_ExternalPtrTag(p);
            assert!(tag.is_null()); // null tag when created with null

            let prot = R_ExternalPtrProtected(p);
            assert!(prot.is_null()); // null prot when created with null

            // Should not crash
            R_ClearExternalPtr(p);
            R_SetExternalPtrAddr(p, ptr::null_mut());
            R_SetExternalPtrTag(p, ptr::null_mut());
            R_SetExternalPtrProtected(p, ptr::null_mut());
        }
    }

    #[test]
    fn test_external_ptr_roundtrip() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // Create an external pointer with a real address
            let mut data: i32 = 42;
            let data_ptr: *mut c_void = &mut data as *mut i32 as *mut c_void;

            let p = R_MakeExternalPtr(data_ptr, R_NilValue(), R_NilValue());
            assert!(!p.is_null());

            // Retrieve the address
            let addr = R_ExternalPtrAddr(p);
            assert_eq!(addr, data_ptr);
            // Verify we can read back the data through the pointer
            assert_eq!(*(addr as *mut i32), 42);
        }
    }

    #[test]
    fn test_external_ptr_setters() {
        let _session = crate::sexp::session::RSession::new();
        {
            let p = R_MakeExternalPtr(ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
            assert!(!p.is_null());

            // Set a new address
            let mut val: f64 = 3.14;
            let val_ptr: *mut c_void = &mut val as *mut f64 as *mut c_void;
            R_SetExternalPtrAddr(p, val_ptr);
            assert_eq!(R_ExternalPtrAddr(p), val_ptr);

            // Clear the address
            R_ClearExternalPtr(p);
            assert!(R_ExternalPtrAddr(p).is_null());

            // Set tag and prot (use the node itself as a non-null SEXP)
            let tag_sexp = p; // reuse the extptr node as a tag value
            R_SetExternalPtrTag(p, tag_sexp);
            assert_eq!(R_ExternalPtrTag(p), tag_sexp);

            let prot_sexp = p;
            R_SetExternalPtrProtected(p, prot_sexp);
            assert_eq!(R_ExternalPtrProtected(p), prot_sexp);
        }
    }

    #[test]
    fn test_external_ptr_null_args() {
        let _session = crate::sexp::session::RSession::new();
        {
            // Null input should not crash
            assert!(R_ExternalPtrAddr(ptr::null_mut()).is_null());
            assert_eq!(R_ExternalPtrTag(ptr::null_mut()), R_NilValue());
            assert_eq!(R_ExternalPtrProtected(ptr::null_mut()), R_NilValue());

            // Setters on null should be no-ops
            R_ClearExternalPtr(ptr::null_mut());
            R_SetExternalPtrAddr(ptr::null_mut(), ptr::null_mut());
            R_SetExternalPtrTag(ptr::null_mut(), ptr::null_mut());
            R_SetExternalPtrProtected(ptr::null_mut(), ptr::null_mut());
        }
    }




    #[test]
    fn test_gc_torture_primitives_roundtrip_session_state() {
        let session = RSession::new();
        session.with_protected(|| unsafe {
            with_memory_state(|state| {
                state.gc_force_gap = 0;
                state.gc_force_wait = 0;
            });

            let on_args = Rf_cons(Rf_ScalarLogical(crate::sexp::ffi::TRUE), R_NilValue());
            let old = do_gctorture(ptr::null_mut(), ptr::null_mut(), on_args, ptr::null_mut());
            assert_eq!(TYPEOF(old), SEXPTYPE::LGLSXP);
            assert_eq!(*LOGICAL(old), crate::sexp::ffi::FALSE);
            assert_eq!(with_memory_state(|state| state.gc_force_gap), 1);

            let args = Rf_cons(
                Rf_ScalarInteger(5),
                Rf_cons(Rf_ScalarInteger(7), R_NilValue()),
            );
            let old_gap = do_gctorture2(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert_eq!(TYPEOF(old_gap), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(old_gap), 1);
            assert_eq!(with_memory_state(|state| state.gc_force_gap), 5);
            assert_eq!(with_memory_state(|state| state.gc_force_wait), 7);
        });
    }


    #[test]
    fn test_c_finalizer_runs_with_external_pointer_target() {
        let _session = crate::sexp::session::RSession::new();
        let session = RSession::new();
        session.with_protected(|| unsafe {
            with_finalizer_state(|state| state.pending_finalizers.clear());
            let mut value = 41_i32;
            let extptr = R_MakeExternalPtr(
                &mut value as *mut i32 as *mut c_void,
                R_NilValue(),
                R_NilValue(),
            );

            R_RegisterCFinalizer(extptr, increment_external_i32_finalizer);
            assert_eq!(value, 41);
            R_RunPendingFinalizers();
            assert_eq!(value, 41);
            assert_eq!(with_finalizer_state(|state| state.pending_finalizers.len()), 1);
        });
    }

    #[test]
    fn test_c_finalizer_runs_once_after_target_becomes_unreachable() {
        let _session = crate::sexp::session::RSession::new();
        let session = RSession::new();
        session.with_protected(|| unsafe {
            with_finalizer_state(|state| state.pending_finalizers.clear());
            FINALIZER_RUNS.set(0);

            let extptr = R_MakeExternalPtr(ptr::null_mut(), R_NilValue(), R_NilValue());
            R_RegisterCFinalizer(extptr, count_c_finalizer);

            R_RunPendingFinalizers();
            assert_eq!(FINALIZER_RUNS.get(), 0);

            R_gc();
            assert_eq!(FINALIZER_RUNS.get(), 1);
            assert!(with_finalizer_state(|state| state
                .pending_finalizers
                .is_empty()));

            R_gc();
            assert_eq!(FINALIZER_RUNS.get(), 1);
        });
    }

    #[test]
    fn test_onexit_c_finalizer_runs_at_exit_even_when_target_is_reachable() {
        let _session = crate::sexp::session::RSession::new();
        let session = RSession::new();
        session.with_protected(|| unsafe {
            with_finalizer_state(|state| state.pending_finalizers.clear());
            FINALIZER_RUNS.set(0);

            let extptr = R_MakeExternalPtr(ptr::null_mut(), R_NilValue(), R_NilValue());
            R_RegisterCFinalizerEx(extptr, count_c_finalizer, 1);

            R_RunPendingFinalizers();
            assert_eq!(FINALIZER_RUNS.get(), 0);

            R_RunExitFinalizers_memory();
            assert_eq!(FINALIZER_RUNS.get(), 1);
            assert!(with_finalizer_state(|state| state
                .pending_finalizers
                .is_empty()));
        });
    }

    #[test]
    fn test_r_finalizer_registration_tracks_target_and_function() {
        let _session = crate::sexp::session::RSession::new();
        let session = RSession::new();
        session.with_protected(|| unsafe {
            with_finalizer_state(|state| state.pending_finalizers.clear());
            let obj = R_MakeExternalPtr(ptr::null_mut(), R_NilValue(), R_NilValue());
            let body = Rf_ScalarInteger(123);
            let fun = crate::mainutils::dstruct::mkCLOSXP(R_NilValue(), body, R_GlobalEnv());

            R_RegisterFinalizerEx(obj, fun, 0);
            with_finalizer_state(|state| {
                assert_eq!(state.pending_finalizers.len(), 1);
                match state.pending_finalizers[0] {
                    PendingFinalizer::R {
                        obj: stored_obj,
                        fun: stored_fun,
                        ready,
                        onexit,
                    } => {
                        assert_eq!(stored_obj, obj);
                        assert_eq!(stored_fun, fun);
                        assert!(!ready);
                        assert!(!onexit);
                    }
                    _ => panic!("expected R finalizer"),
                }
            });

            R_RunPendingFinalizers();
            assert_eq!(with_finalizer_state(|state| state.pending_finalizers.len()), 1);
        });
    }

    #[test]
    fn test_make_weak_ref_with_r_finalizer_registers_deferred_finalizer() {
        let _session = crate::sexp::session::RSession::new();
        let session = RSession::new();
        session.with_protected(|| {
            with_finalizer_state(|state| state.pending_finalizers.clear());
            let key = R_MakeExternalPtr(ptr::null_mut(), R_NilValue(), R_NilValue());
            let body = Rf_ScalarInteger(123);
            // SAFETY: nil formals, fresh body, global env — a complete closure.
            let fun = unsafe {
                crate::mainutils::dstruct::mkCLOSXP(R_NilValue(), body, R_GlobalEnv())
            };

            let weak = R_MakeWeakRef(key, R_NilValue(), fun, 1);
            assert_eq!(TYPEOF(weak), SEXPTYPE::WEAKREFSXP);
            with_finalizer_state(|state| {
                assert_eq!(state.pending_finalizers.len(), 1);
                match state.pending_finalizers[0] {
                    PendingFinalizer::R {
                        obj,
                        fun: stored_fun,
                        ready,
                        onexit,
                    } => {
                        assert_eq!(obj, key);
                        assert_eq!(stored_fun, fun);
                        assert!(!ready);
                        assert!(onexit);
                    }
                    _ => panic!("expected R finalizer"),
                }
            });
        });
    }

    extern "C" fn dummy_c_finalizer(_ptr: *mut c_void) {
        // No action needed
    }

    unsafe extern "C" fn increment_external_i32_finalizer(ptr: *mut c_void) {
        unsafe {
            let addr = R_ExternalPtrAddr(ptr as SEXP) as *mut i32;
            if !addr.is_null() {
                *addr += 1;
            }
        }
    }

    extern "C" fn count_c_finalizer(_ptr: *mut c_void) {
        FINALIZER_RUNS.set(FINALIZER_RUNS.get() + 1);
    }




    #[test]
    fn test_seql() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // Same pointer should be equal
            let fake = 0x1 as SEXP;
            assert_eq!(Seql(fake, fake), 1);
        }
    }

}

#[cfg(test)]
mod owned_gc_operation_tests {
    use super::*;
    use crate::sexp::{gengc::register_gc_callback, owner::OwnerPin, session::RSession};
    use std::{
        cell::{Cell, RefCell},
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };

    fn pin(session: &RSession) -> OwnerPin {
        session
            .owner_token()
            .unwrap()
            .weak_owner()
            .unwrap()
            .pin()
            .unwrap()
    }

    #[test]
    fn torture_ticks_stay_session_local_and_defer_without_consuming_wait() {
        let session = RSession::new_for_gc_tests();
        let other = RSession::new_for_gc_tests();
        let original = pin(&session);
        let second = pin(&other);
        let instance = original.as_ptr();
        unsafe {
            set_gc_torture_for_test_in(instance, 3, Some(5));
            assert_eq!(
                torture_action_in(second.as_ptr(), u32::MAX),
                TortureAction::Skip
            );
            assert_eq!(torture_action_in(instance, 2), TortureAction::Skip);
        }
        let guard = GcOperationGuard::enter(pin(&session), true).unwrap();
        assert_eq!(
            unsafe { torture_action_in(instance, u32::MAX) },
            TortureAction::Defer
        );
        drop(guard);
        unsafe {
            with_gc_state_in(instance, |state| state.in_progress = true);
            assert_eq!(torture_action_in(instance, u32::MAX), TortureAction::Defer);
            with_gc_state_in(instance, |state| state.in_progress = false);
            assert_eq!(torture_action_in(instance, 2), TortureAction::Skip);
            assert_eq!(torture_action_in(instance, 1), TortureAction::Collect);
            assert_eq!(torture_action_in(instance, u32::MAX), TortureAction::Collect);
            assert_eq!(torture_action_in(instance, 2), TortureAction::Skip);
            assert_eq!(torture_action_in(instance, 1), TortureAction::Collect);
        }
    }

    #[test]
    fn owned_gc_operation_live_panic_restores_original_state_and_payload() {
        let session = RSession::new_for_gc_tests();
        let original = pin(&session);
        let instance = original.as_ptr();
        register_gc_callback(Box::new(move |_| {
            assert_eq!(unsafe { gc_running_in(instance) }, 1);
            std::panic::panic_any(73_u32);
        }));
        let outcome = catch_unwind(AssertUnwindSafe(|| R_gc()));
        assert_eq!(*outcome.unwrap_err().downcast::<u32>().unwrap(), 73);
        assert_eq!(unsafe { gc_running_in(instance) }, 0);
        assert_eq!(unsafe { with_memory_state_in(instance, |state| state.gc_count) }, 1);
    }

    #[test]
    fn owned_gc_operation_nested_collection_restores_outer_flag() {
        let session = RSession::new_for_gc_tests();
        let original = pin(&session);
        let instance = original.as_ptr();
        let calls = Rc::new(Cell::new(0));
        let callback_calls = calls.clone();
        register_gc_callback(Box::new(move |_| unsafe {
            callback_calls.set(callback_calls.get() + 1);
            assert_eq!(gc_running_in(instance), 1);
            R_gc();
            assert_eq!(gc_running_in(instance), 1);
        }));
        R_gc();
        assert_eq!(calls.get(), 1);
        assert_eq!(unsafe { gc_running_in(instance) }, 0);
        assert_eq!(unsafe { with_memory_state_in(instance, |state| state.gc_count) }, 2);
    }

    fn drops_facade(full: bool) {
        let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
        let original = pin(facade.borrow().as_ref().unwrap());
        let instance = original.as_ptr();
        let other = RSession::new_for_gc_tests();
        let other_pin = pin(&other);
        unsafe { with_memory_state_in(other_pin.as_ptr(), |state| state.in_gc = 9) };
        let callback_facade = Rc::downgrade(&facade);
        let calls = Rc::new(Cell::new(0));
        let callback_calls = calls.clone();
        let outcome = catch_unwind(AssertUnwindSafe(|| unsafe {
            crate::sexp::session::with_instance_active(instance, || {
                register_gc_callback(Box::new(move |_| {
                    callback_calls.set(callback_calls.get() + 1);
                    drop(callback_facade.upgrade().unwrap().borrow_mut().take());
                }));
                if full { R_gc() } else { R_gc_lite() }
            });
        }));
        assert_eq!(calls.get(), 1);
        assert!(facade.borrow().is_none());
        assert!(original.require_live().is_err());
        assert_eq!(unsafe { gc_running_in(instance) }, 0);
        assert_eq!(unsafe { gc_running_in(other_pin.as_ptr()) }, 9);
        assert!(outcome.unwrap_err().is::<crate::sexp::context::RError>());
    }

    #[test]
    fn owned_gc_operation_full_revocation_cleans_original_and_denies_continuation() {
        drops_facade(true);
    }

    #[test]
    fn owned_gc_operation_lite_revocation_denies_ambient_finalizer_continuation() {
        drops_facade(false);
    }

    fn allocation_callback_cleanup(revoke: bool) {
        let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
        let original = pin(facade.borrow().as_ref().unwrap());
        let instance = original.as_ptr();
        let original_owner = facade
            .borrow()
            .as_ref()
            .unwrap()
            .owner_token()
            .unwrap()
            .weak_owner()
            .unwrap();
        let factory = original_owner.node_factory().unwrap();
        let binding = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
            .unwrap();
        let binding_node = binding.allocation().unwrap().clone();
        assert!(
            facade
                .borrow()
                .as_ref()
                .unwrap()
                .define_var("owned_torture_binding", binding)
        );
        let calls = Rc::new(Cell::new(0));
        let callback_calls = calls.clone();
        let callback_facade = Rc::downgrade(&facade);
        let outcome = catch_unwind(AssertUnwindSafe(|| unsafe {
            crate::sexp::session::with_instance_active(instance, || {
                register_gc_callback(Box::new(move |_| {
                    callback_calls.set(callback_calls.get() + 1);
                    assert_eq!(gc_running_in(instance), 1);
                    assert!(binding_node.is_live());
                    assert_eq!((*instance).legacy_protect.len(), 0);
                    set_gc_torture_for_test_in(instance, 0, None);
                    if revoke {
                        drop(callback_facade.upgrade().unwrap().borrow_mut().take());
                    } else {
                        std::panic::panic_any(127_u32);
                    }
                }));
                set_gc_torture_for_test_in(instance, 1, Some(1));
                factory.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
            })
        }));
        assert_eq!(calls.get(), 1);
        assert_eq!(unsafe { gc_running_in(instance) }, 0);
        assert_eq!(unsafe { (*instance).legacy_protect.len() }, 0);
        if revoke {
            assert!(facade.borrow().is_none());
            assert!(original.require_live().is_err());
            assert!(matches!(
                outcome.unwrap(),
                Err(crate::sexp::object::SexpError::RootUnavailable)
            ));
        } else {
            assert!(original.require_live().is_ok());
            assert_eq!(*outcome.unwrap_err().downcast::<u32>().unwrap(), 127);
        }
    }

    #[test]
    fn owned_gc_operation_actual_allocation_callback_panic_restores_original() {
        allocation_callback_cleanup(false);
    }

    #[test]
    fn owned_gc_operation_actual_allocation_callback_revocation_restores_original() {
        allocation_callback_cleanup(true);
    }

    #[test]
    fn precious_mset_growth_rechecks_state_after_gc_callback() {
        use crate::sexp::gengc::register_gc_callback;
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let owner = session.owner_token().unwrap();
            let set = owner.sexp(R_NewPreciousMSet(1)).unwrap();
            let first = owner.sexp(Rf_ScalarInteger(1001)).unwrap();
            let nested = owner.sexp(Rf_ScalarInteger(1002)).unwrap();
            R_PreserveInMSet(first.as_raw(), set.as_raw());

            let set_raw = set.as_raw();
            let first_raw = first.as_raw();
            let nested_raw = nested.as_raw();
            let invoked = std::rc::Rc::new(std::cell::Cell::new(false));
            {
                let invoked = invoked.clone();
                register_gc_callback(Box::new(move |_| {
                    if invoked.replace(true) {
                        return;
                    }
                    R_gc_torture(0, 0, 0);
                    R_ReleaseFromMSet(first_raw, set_raw);
                    R_PreserveInMSet(nested_raw, set_raw);
                    R_PreserveInMSet(first_raw, set_raw);
                }));
            }
            // Its only GC root during the growth collection is the one
            // established inside R_PreserveInMSet.
            let newcomer_raw = Rf_ScalarInteger(1003);
            R_gc_torture(1, 1, 0);
            R_PreserveInMSet(newcomer_raw, set_raw);
            R_gc_torture(0, 0, 0);

            let store = owner.sexp(CAR(set_raw)).unwrap();
            let count = owner.sexp(CDR(set_raw)).unwrap();
            assert!(invoked.get());
            assert_eq!(count.try_integer_elt(0).unwrap(), 3);
            assert_eq!(store.try_vector_elt(0).unwrap().as_raw(), nested_raw);
            assert_eq!(store.try_vector_elt(1).unwrap().as_raw(), first_raw);
            assert_eq!(store.try_vector_elt(2).unwrap().as_raw(), newcomer_raw);

            // Releasing a duplicated entry removes the rightmost instance
            // while keeping the remaining order stable.
            R_PreserveInMSet(first_raw, set_raw);
            R_gc_torture(0, 0, 0);
            R_ReleaseFromMSet(first_raw, set_raw);
            assert_eq!(count.try_integer_elt(0).unwrap(), 3);
            assert_eq!(store.try_vector_elt(0).unwrap().as_raw(), nested_raw);
            assert_eq!(store.try_vector_elt(1).unwrap().as_raw(), first_raw);
            assert_eq!(store.try_vector_elt(2).unwrap().as_raw(), newcomer_raw);

            R_ReleaseFromMSet(nested_raw, set_raw);
            R_ReleaseFromMSet(newcomer_raw, set_raw);
            R_ReleaseFromMSet(first_raw, set_raw);
            R_ReleaseFromMSet(first_raw, set_raw);
            assert_eq!(count.try_integer_elt(0).unwrap(), 0);
            assert_eq!(store.try_vector_elt(0).unwrap().as_raw(), R_NilValue());
        });
    }

    #[test]
    fn resizable_duplicate_marks_only_the_copy() {
        let session = RSession::new();
        session.with_active(|| {
            let owner = session.owner_token().unwrap();
            let source = owner.sexp(Rf_allocVector3(SEXPTYPE::INTSXP, 3)).unwrap();
            let dup = R_duplicateAsResizable(source.as_raw());
            assert_eq!(R_isResizable(source.as_raw()), 0);
            assert_eq!(R_isResizable(dup), 1);
            assert_eq!(XLENGTH(dup), 3);
        });
    }

    #[test]
    fn external_pointer_checked_publication_survives_gc_and_rejects_foreign_inputs() {
        let session = RSession::new_for_gc_tests();
        let other = RSession::new_for_gc_tests();
        session.with_active(|| {
            let owner = session.owner_token().unwrap();
            // tag/prot stay raw with no checked roots; only the lexical root
            // inside R_MakeExternalPtr covers them across the collection.
            let tag = Rf_ScalarInteger(406);
            let prot = {
                let tag_guard = crate::sexp::protect::protect(tag);
                let prot = Rf_ScalarInteger(407);
                drop(tag_guard);
                prot
            };
            let fired = Rc::new(Cell::new(false));
            {
                let seen = fired.clone();
                register_gc_callback(Box::new(move |_| {
                    if !seen.replace(true) {
                        R_gc_torture(0, 0, 0);
                    }
                }));
            }
            R_gc_torture(1, 1, 0);
            // The address is opaque to the runtime and never dereferenced.
            let address = std::ptr::without_provenance_mut::<c_void>(7);
            let ptr = owner.sexp(R_MakeExternalPtr(address, tag, prot)).unwrap();
            R_gc_torture(0, 0, 0);
            assert!(fired.get());

            crate::sexp::gengc::full_gc();
            assert_eq!(R_ExternalPtrAddr(ptr.as_raw()), address);
            assert_eq!(R_ExternalPtrTag(ptr.as_raw()), tag);
            assert_eq!(R_ExternalPtrProtected(ptr.as_raw()), prot);
            assert_eq!(INTEGER_ELT(R_ExternalPtrTag(ptr.as_raw()), 0), 406);
            assert_eq!(INTEGER_ELT(R_ExternalPtrProtected(ptr.as_raw()), 0), 407);

            let nine = std::ptr::without_provenance_mut::<c_void>(9);
            R_SetExternalPtrAddr(ptr.as_raw(), nine);
            assert_eq!(R_ExternalPtrTag(ptr.as_raw()), tag);
            assert_eq!(R_ExternalPtrProtected(ptr.as_raw()), prot);
            assert_eq!(R_ExternalPtrAddr(ptr.as_raw()), nine);
            R_ClearExternalPtr(ptr.as_raw());
            assert!(R_ExternalPtrAddr(ptr.as_raw()).is_null());
            assert_eq!(R_ExternalPtrTag(ptr.as_raw()), tag);
            assert_eq!(R_ExternalPtrProtected(ptr.as_raw()), prot);

            let raw = ptr.as_raw();
            other.with_active(|| {
                let foreign = std::ptr::without_provenance_mut::<c_void>(11);
                let set = catch_unwind(AssertUnwindSafe(|| {
                    R_SetExternalPtrAddr(raw, foreign);
                }));
                assert!(set.is_err());
                let made = catch_unwind(AssertUnwindSafe(|| {
                    R_MakeExternalPtr(foreign, tag, R_NilValue());
                }));
                assert!(made.is_err());
            });
            assert!(R_ExternalPtrAddr(raw).is_null());
            assert_eq!(R_ExternalPtrTag(raw), tag);

            // A collected node is rejected by the checked path rather than
            // mutated through a stale raw pointer. No allocation between the
            // collection and the setter so the slot cannot be reused.
            let stale = R_MakeExternalPtr(address, R_NilValue(), R_NilValue());
            let stale_node = crate::sexp::memory::checked_node(stale).unwrap();
            crate::sexp::gengc::full_gc();
            assert!(!stale_node.is_live());
            let cleared = catch_unwind(AssertUnwindSafe(|| R_ClearExternalPtr(stale)));
            assert!(cleared.is_err());
        });
    }

    #[test]
    fn weak_ref_construction_keeps_key_during_gc_but_not_afterward() {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let owner = session.owner_token().unwrap();
            let key = R_MakeExternalPtr(ptr::null_mut(), R_NilValue(), R_NilValue());
            // Rooted only until the value allocation; the weak edge published
            // by construction must not take over this root.
            let key_root = owner.sexp(key).unwrap();
            let value = Rf_ScalarInteger(1307);
            let key_node = key_root.allocation().unwrap().clone();
            let value_node = crate::sexp::memory::checked_node(value).unwrap();
            let collections = Rc::new(Cell::new(0usize));
            {
                let collections = collections.clone();
                let key_node = key_node.clone();
                let value_node = value_node.clone();
                register_gc_callback(Box::new(move |_| {
                    collections.set(collections.get() + 1);
                    if collections.get() == 1 {
                        assert!(key_node.is_live());
                        assert!(value_node.is_live());
                        R_gc_torture(0, 0, 0);
                    }
                }));
            }
            drop(key_root);
            R_gc_torture(1, 1, 0);
            let weak = owner
                .sexp(R_MakeWeakRef(key, value, R_NilValue(), 0))
                .unwrap();
            R_gc_torture(0, 0, 0);
            assert!(collections.get() >= 1);
            assert_eq!(R_WeakRefKey(weak.as_raw()), key);
            assert_eq!(R_WeakRefValue(weak.as_raw()), value);

            crate::sexp::gengc::full_gc();
            assert!(!key_node.is_live());
            assert_eq!(R_WeakRefKey(weak.as_raw()), R_NilValue());
            assert!(value_node.is_live());
            assert_eq!(INTEGER_ELT(R_WeakRefValue(weak.as_raw()), 0), 1307);
        });
    }

    #[test]
    fn weak_ref_c_finalizer_is_deferred_and_runs_once() {
        thread_local! {
            static RUNS: Cell<usize> = const { Cell::new(0) };
        }
        extern "C" fn count(_: *mut c_void) {
            RUNS.with(|runs| runs.set(runs.get() + 1));
        }
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            RUNS.with(|runs| runs.set(0));
            let owner = session.owner_token().unwrap();
            let key = R_MakeExternalPtr(ptr::null_mut(), R_NilValue(), R_NilValue());
            let key_root = owner.sexp(key).unwrap();
            // SAFETY: `count` has no preconditions and never unwinds.
            let weak = unsafe { R_MakeWeakRefC(key, R_NilValue(), count, 0) };
            let _weak = owner.sexp(weak).unwrap();
            R_gc();
            assert_eq!(RUNS.with(|runs| runs.get()), 0);
            drop(key_root);
            R_gc();
            assert_eq!(RUNS.with(|runs| runs.get()), 1);
            R_gc();
            assert_eq!(RUNS.with(|runs| runs.get()), 1);
        });
    }
}
