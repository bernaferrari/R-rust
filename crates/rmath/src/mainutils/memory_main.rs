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
use crate::sexp::globals::{R_GlobalEnv, R_NilValue};
use crate::sexp::heap::NodeLink;
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
pub(crate) enum PendingFinalizer {
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
    pub(crate) fn obj(&self) -> SEXP {
        match *self {
            PendingFinalizer::C { obj, .. } | PendingFinalizer::R { obj, .. } => obj,
        }
    }

    pub(crate) fn obj_mut(&mut self) -> &mut SEXP {
        match self {
            PendingFinalizer::C { obj, .. } | PendingFinalizer::R { obj, .. } => obj,
        }
    }

    pub(crate) fn fun_mut(&mut self) -> Option<&mut SEXP> {
        match self {
            PendingFinalizer::C { .. } => None,
            PendingFinalizer::R { fun, .. } => Some(fun),
        }
    }

    pub(crate) fn is_ready(&self) -> bool {
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
    pub in_gc: c_int,
    pub gc_reporting: c_int,
    pub gc_count: c_int,
    pub gc_force_gap: c_int,
    pub gc_force_wait: c_int,
    pub max_v_size: u64,
    pub max_n_size: u64,
    pub running_finalizers: bool,
    pub pending_finalizers: Vec<PendingFinalizer>,
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
            running_finalizers: false,
            pending_finalizers: Vec::new(),
        }
    }
}

fn with_memory_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut MemoryRuntimeState) -> R,
{
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        f(&mut (*inst).memory_state)
    })
}

/// Returns whether a GC is currently running.
///
/// This is the equivalent of R's `R_gc_running()`.
pub fn R_gc_running() -> c_int {
    crate::sexp::instance::with_current_instance(|inst| unsafe { (*inst).memory_state.in_gc })
        .unwrap_or(0)
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
            (*instance).memory_state.gc_count = (*instance).memory_state.gc_count.wrapping_add(1);
            if full {
                let previous = (*instance).memory_state.in_gc;
                (*instance).memory_state.in_gc = 1;
                Some(previous)
            } else {
                None
            }
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
            unsafe { (*self.owner.as_ptr()).memory_state.in_gc = previous };
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

// ---------------------------------------------------------------------------
// Transient allocation — duplicates (no #[unsafe(no_mangle)] to avoid conflicts)
//
// R_alloc, vmaxget, vmaxset are already in sexp/memory_ext.rs.
// ---------------------------------------------------------------------------

/// Allocate transient memory for long double values.
///
/// This is the equivalent of R's `R_allocLD()`.
/// For long double alignment, we overallocate by 1 element and align manually.
pub(crate) fn R_allocLD(nelem: usize) -> *mut c_void {
    unsafe {
        // long double is 16 bytes on x86_64; alignment is 16
        // We call R_alloc (from memory_ext.rs) with extra space and align
        let ld_size = std::mem::size_of::<c_double>(); // approximate
        let ld_align = std::mem::align_of::<u128>().max(16);

        let raw = crate::sexp::memory_ext::R_alloc(ld_size, nelem + 1);
        if raw.is_null() {
            return ptr::null_mut();
        }
        let addr = raw as usize;
        let aligned = (addr + ld_align - 1) & !(ld_align - 1);
        aligned as *mut c_void
    }
}

/// S compatibility: allocate zeroed transient memory.
///
/// This is the equivalent of R's `S_alloc()`.
pub fn S_alloc(nelem: c_long, eltsize: c_int) -> *mut c_char {
    unsafe {
        let size = (nelem as usize) * (eltsize as usize);
        let p = crate::sexp::memory_ext::R_alloc(eltsize as usize, nelem as usize) as *mut c_char;
        if !p.is_null() && size > 0 {
            ptr::write_bytes(p as *mut u8, 0, size);
        }
        p
    }
}

/// S compatibility: reallocate transient memory.
///
/// This is the equivalent of R's `S_realloc()`.
pub unsafe fn S_realloc(
    p: *mut c_char,
    new_len: c_long,
    old_len: c_long,
    size: c_int,
) -> *mut c_char {
    unsafe {
        if new_len <= old_len {
            return p;
        }
        let q = crate::sexp::memory_ext::R_alloc(size as usize, new_len as usize) as *mut c_char;
        if !q.is_null() && !p.is_null() {
            let nold = (old_len as usize) * (size as usize);
            if nold > 0 {
                ptr::copy_nonoverlapping(p, q, nold);
            }
            let nnew = (new_len as usize) * (size as usize);
            if nnew > nold {
                ptr::write_bytes(q.add(nold) as *mut u8, 0, nnew - nold);
            }
        }
        q
    }
}

// ---------------------------------------------------------------------------
// GC-on-failure allocation wrappers
// ---------------------------------------------------------------------------

/// malloc with GC fallback.
///
/// This is the equivalent of R's `R_malloc_gc()`.
pub fn R_malloc_gc(n: usize) -> *mut c_void {
    unsafe {
        let p = crate::mainutils::raw_heap::malloc(n);
        if p.is_null() {
            R_gc();
            return crate::mainutils::raw_heap::malloc(n);
        }
        p
    }
}

/// calloc with GC fallback.
///
/// This is the equivalent of R's `R_calloc_gc()`.
pub fn R_calloc_gc(n: usize, s: usize) -> *mut c_void {
    unsafe {
        let p = crate::mainutils::raw_heap::calloc(n, s);
        if p.is_null() {
            R_gc();
            return crate::mainutils::raw_heap::calloc(n, s);
        }
        p
    }
}

/// realloc with GC fallback.
///
/// This is the equivalent of R's `R_realloc_gc()`.
pub unsafe fn R_realloc_gc(p: *mut c_void, n: usize) -> *mut c_void {
    unsafe {
        let q = crate::mainutils::raw_heap::realloc(p, n);
        if q.is_null() {
            R_gc();
            return crate::mainutils::raw_heap::realloc(p, n);
        }
        q
    }
}

// ---------------------------------------------------------------------------
// Memory checking utilities
// ---------------------------------------------------------------------------

/// Checked calloc that errors on failure.
///
/// This is the equivalent of R's `R_chk_calloc()`.
pub fn R_chk_calloc(nelem: usize, elsize: usize) -> *mut c_void {
    unsafe {
        let p = crate::mainutils::raw_heap::calloc(nelem, elsize);
        if p.is_null() {
            crate::mainutils::errors::Rf_error_cstr(c"memory allocation failed (calloc)");
        }
        p
    }
}

/// Checked realloc that errors on failure.
///
/// This is the equivalent of R's `R_chk_realloc()`.
pub unsafe fn R_chk_realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    unsafe {
        let p = crate::mainutils::raw_heap::realloc(ptr, size);
        if p.is_null() {
            crate::mainutils::errors::Rf_error_cstr(c"memory allocation failed (realloc)");
        }
        p
    }
}

/// Checked free.
///
/// This is the equivalent of R's `R_chk_free()`.
pub unsafe fn R_chk_free(ptr: *mut c_void) {
    unsafe {
        if !ptr.is_null() {
            crate::mainutils::raw_heap::free(ptr);
        }
    }
}

/// Checked memcpy with size limit.
///
/// This is the equivalent of R's `R_chk_memcpy()`.
pub unsafe fn R_chk_memcpy(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    unsafe {
        if n >= isize::MAX as usize {
            error(&format!("object is too large ({} bytes)", n));
        }
        if n > 0 {
            ptr::copy_nonoverlapping(src as *const u8, dest as *mut u8, n);
        }
        dest
    }
}

/// Checked memset with size limit.
///
/// This is the equivalent of R's `R_chk_memset()`.
pub unsafe fn R_chk_memset(s: *mut c_void, c: c_int, n: usize) -> *mut c_void {
    unsafe {
        if n >= isize::MAX as usize {
            error(&format!("object is too large ({} bytes)", n));
        }
        if n > 0 {
            ptr::write_bytes(s as *mut u8, c as u8, n);
        }
        s
    }
}

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

/// Check if SEXP is NULL (NILSXP). Returns c_int (0/1).
/// Duplicate of sexp::accessors::Rf_isNull — kept for callers in this module.
pub(crate) fn Rf_isNull_memory(s: SEXP) -> c_int {
    c_int::from(s.is_null() || s == R_NilValue())
}

/// Check if SEXP is a symbol.
pub(crate) fn Rf_isSymbol_memory(s: SEXP) -> c_int {
    Rf_isSymbol(s)
}

/// Check if SEXP is a logical vector.
pub(crate) fn Rf_isLogical_memory(s: SEXP) -> c_int {
    Rf_isLogical(s)
}

/// Check if SEXP is a real (numeric) vector.
pub(crate) fn Rf_isReal_memory(s: SEXP) -> c_int {
    Rf_isReal(s)
}

/// Check if SEXP is a complex vector.
pub(crate) fn Rf_isComplex_memory(s: SEXP) -> c_int {
    Rf_isComplex(s)
}

/// Check if SEXP is an expression.
pub(crate) fn Rf_isExpression_memory(s: SEXP) -> c_int {
    c_int::from(TYPEOF(s) == SEXPTYPE::EXPRSXP)
}

/// Check if SEXP is an environment.
pub(crate) fn Rf_isEnvironment_memory(s: SEXP) -> c_int {
    Rf_isEnvironment(s)
}

/// Check if SEXP is a string (character vector).
pub(crate) fn Rf_isString_memory(s: SEXP) -> c_int {
    Rf_isString(s)
}

/// Check if SEXP is an S4 object.
pub(crate) fn Rf_isObject_memory(s: SEXP) -> c_int {
    c_int::from(TYPEOF(s) == SEXPTYPE::OBJSXP)
}

// ---------------------------------------------------------------------------
// R_PreserveObject / R_ReleaseObject (duplicates — no #[unsafe(no_mangle)])
//
// Already in sexp/protect.rs with #[unsafe(no_mangle)].
// ---------------------------------------------------------------------------

pub(crate) fn R_PreserveObject_memory(s: SEXP) {
    crate::sexp::protect::R_PreserveObject(s);
}

pub(crate) fn R_ReleaseObject_memory(s: SEXP) {
    crate::sexp::protect::R_ReleaseObject(s);
}

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

/// Create a weak reference with a native finalizer.
///
/// This is the equivalent of R's `R_MakeWeakRefC()`.
///
/// # Safety
/// `fin` must remain callable for the lifetime of the registration and be
/// safe to invoke with the key's SEXP cast to `*mut c_void`, without unwinding
/// through the C ABI.
pub unsafe fn R_MakeWeakRefC(key: SEXP, val: SEXP, fin: R_CFinalizer_t, onexit: c_int) -> SEXP {
    make_weak_ref(key, val, R_NilValue(), || {
        register_c_finalizer(key, fin, onexit != 0);
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

pub fn R_WeakRefValue(w: SEXP) -> SEXP {
    {
        if w.is_null() {
            return R_NilValue();
        }
        validate_weak_ref_object(w);
        CDR(w)
    }
}

pub fn R_RegisterFinalizer(s: SEXP, fun: SEXP) {
    R_RegisterFinalizerEx(s, fun, 0);
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
            unsafe { (*self.owner).memory_state.running_finalizers = false };
        }
    }
}

pub unsafe fn R_RunPendingFinalizers() {
    let owner = crate::sexp::instance::with_required_current_instance(|owner| owner);
    // SAFETY: the ambient owner is live on entry. Retain only its weak teardown
    // observer; callbacks may destroy the original session.
    let liveness = unsafe { crate::sexp::instance::instance_liveness(owner) };
    let mut finalizers = with_memory_state(|state| {
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
            (*owner)
                .memory_state
                .pending_finalizers
                .retain(|finalizer| !finalizer.is_ready());
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
    with_memory_state(|state| {
        state.pending_finalizers.push(PendingFinalizer::C {
            obj,
            fun,
            ready: false,
            onexit,
        });
    });
}

fn register_r_finalizer(obj: SEXP, fun: SEXP, onexit: bool) {
    with_memory_state(|state| {
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

pub fn R_RunFinalizers() {
    unsafe {
        R_RunPendingFinalizers();
    }
}

pub(crate) fn mark_finalizers_ready_for_unreachable(
    unreachable: &std::collections::HashSet<usize>,
) -> std::collections::HashSet<usize> {
    with_memory_state(|state| mark_finalizers_ready_for_unreachable_in(state, unreachable))
}

pub(crate) fn mark_finalizers_ready_for_unreachable_in(
    state: &mut MemoryRuntimeState,
    unreachable: &std::collections::HashSet<usize>,
) -> std::collections::HashSet<usize> {
    let mut keep_alive = std::collections::HashSet::new();
    for finalizer in &mut state.pending_finalizers {
        let obj = finalizer.obj();
        if !obj.is_null() && unreachable.contains(&(obj as usize)) {
            finalizer.set_ready();
        }
        if finalizer.is_ready() && !obj.is_null() {
            keep_alive.insert(obj as usize);
        }
    }
    keep_alive
}

pub(crate) unsafe fn R_RunExitFinalizers_memory() {
    unsafe {
        mark_exit_finalizers_ready();
        R_RunPendingFinalizers();
    }
}

fn mark_exit_finalizers_ready() {
    with_memory_state(|state| {
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
    crate::sexp::instance::with_current_instance(|inst| unsafe { (*inst).memory_state.max_v_size })
        .unwrap_or(u64::MAX)
}

/// Get the maximum node heap size.
/// Duplicate — no #[unsafe(no_mangle)] (already in mainutils/main.rs).
pub(crate) fn R_GetMaxNSize_memory() -> u64 {
    crate::sexp::instance::with_current_instance(|inst| unsafe { (*inst).memory_state.max_n_size })
        .unwrap_or(u64::MAX)
}

/// Set the maximum vector heap size.
pub fn R_SetMaxVSize(size: u64) -> c_int {
    let current = current_vector_heap_size();
    with_memory_state(|state| {
        if size == u64::MAX || size >= current {
            state.max_v_size = size;
            crate::sexp::ffi::TRUE
        } else {
            crate::sexp::ffi::FALSE
        }
    })
}

/// Set the maximum node heap size.
pub fn R_SetMaxNSize(size: u64) -> c_int {
    let current = current_node_heap_size();
    with_memory_state(|state| {
        if size == u64::MAX || size >= current {
            state.max_n_size = size;
            crate::sexp::ffi::TRUE
        } else {
            crate::sexp::ffi::FALSE
        }
    })
}

/// Set the protection stack size.
pub fn R_SetPPSize(_size: u64) {
    // Arena-based allocation doesn't need PP stack sizing
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

/// Read from the active console callback.
/// Duplicate — no #[unsafe(no_mangle)] (already in unix/system.rs).
pub(crate) unsafe fn R_ReadConsole_memory(
    prompt: *const c_char,
    buf: *mut c_char,
    len: c_int,
    addtohistory: c_int,
) -> c_int {
    // SAFETY: forwarded raw pointers satisfy `R_ReadConsole`'s contract per this function's own `# Safety` requirements.
    unsafe { crate::unix::system::R_ReadConsole(prompt, buf as *mut u8, len, addtohistory) }
}

/// Write to the active console callback.
/// Duplicate — no #[unsafe(no_mangle)] (already in unix/system.rs).
pub(crate) unsafe fn R_WriteConsole_memory(buf: *const c_char, len: c_int) {
    // SAFETY: forwarded raw pointers satisfy `R_WriteConsole`'s contract per this function's own `# Safety` requirements.
    unsafe { crate::unix::system::R_WriteConsole(buf, len) }
}

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

/// Free a string buffer.
///
/// This is the equivalent of R's `R_FreeStringBuffer()`.
pub unsafe fn R_FreeStringBuffer(buf: *mut R_StringBuffer) {
    unsafe {
        if buf.is_null() {
            return;
        }
        let buf = &mut *buf;
        if !buf.data.is_null() {
            drop(Vec::from_raw_parts(buf.data as *mut u8, 0, buf.bufsize));
            buf.data = ptr::null_mut();
        }
        buf.bufsize = 0;
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

/// Create a new multi-set for protecting objects.
///
/// This is the equivalent of R's `R_NewPreciousMSet()`.
///
/// Multi-set representation: `CONS(store, npreserved)` with `TAG() == initialSize`.
/// `store` is a VECSXP or R_NilValue. `npreserved` is an INTSXP of length 1.
pub fn R_NewPreciousMSet(initialSize: c_int) -> SEXP {
    if initialSize < 0 {
        error("'initialSize' must be non-negative");
    }
    checked_memory(OwnerToken::with_current(|owner| {
        let counter = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
        checked_memory(owner.require_active());
        let counter = checked_memory(owner.sexp(counter));
        let set = Rf_cons(R_NilValue(), counter.as_raw());
        checked_memory(owner.require_active());
        let set = checked_memory(owner.sexp(set));
        let initial = Rf_ScalarInteger(initialSize);
        checked_memory(owner.require_active());
        SETTAG(set.as_raw(), initial);
        set.as_raw()
    }))
}

/// Add an object to a multi-set.
///
/// This is the equivalent of R's `R_PreserveInMSet()`.
///
/// Ported from `r-source/src/main/memory.c:3733`.
pub fn R_PreserveInMSet(x: SEXP, mset: SEXP) {
    if x == R_NilValue() || crate::mainutils::relop::isSymbol(x) != 0 {
        return;
    }
    checked_memory(OwnerToken::with_current(|owner| {
        let object = checked_memory(owner.sexp(x));
        let mset = mset_check(owner.sexp(mset));
        loop {
            let current = MSet::read(&mset);
            let index = i64::from(current.len);
            let next = current
                .len
                .checked_add(1)
                .unwrap_or_else(|| error("Multi-set overflow"));
            let mut count = mset_check(SexpMut::try_from_checked(current.count.clone()));
            if index < current.store.len() {
                let mut store = mset_check(SexpMut::try_from_checked(current.store.clone()));
                checked_memory(store.try_set_vector_elt(index, object));
                checked_memory(count.try_set_integer_elt(0, next));
                return;
            }
            let capacity = if current.store.is_nil() {
                match mset_check(current.initial.try_integer_elt(0)) {
                    0 => 4,
                    n if n > 0 => i64::from(n),
                    _ => error("Invalid mset"),
                }
            } else {
                current
                    .store
                    .len()
                    .checked_mul(2)
                    .filter(|n| *n > index && *n < i64::from(c_int::MAX))
                    .unwrap_or_else(|| error("Multi-set overflow"))
            };
            let raw = Rf_allocVector3(SEXPTYPE::VECSXP, capacity);
            checked_memory(owner.require_active());
            let mut replacement =
                checked_memory(SexpMut::try_from_checked(checked_memory(owner.sexp(raw))));
            // The growth allocation can invoke a callback that changed this
            // same set; only publish when the cells we read are unchanged.
            let latest = MSet::read(&mset);
            if latest.len != current.len
                || latest.store.as_raw() != current.store.as_raw()
                || latest.count.as_raw() != current.count.as_raw()
                || latest.initial.as_raw() != current.initial.as_raw()
            {
                continue;
            }
            for i in 0..index {
                let value = checked_memory(current.store.try_vector_elt(i));
                checked_memory(replacement.try_set_vector_elt(i, value));
            }
            checked_memory(replacement.try_set_vector_elt(index, object));
            SETCAR(mset.as_raw(), replacement.as_raw());
            checked_memory(count.try_set_integer_elt(0, next));
            return;
        }
    }));
}

/// Remove (one instance of) the object from the multi-set.
///
/// This is the equivalent of R's `R_ReleaseFromMSet()`.
///
/// Ported from `r-source/src/main/memory.c:3767`.
pub fn R_ReleaseFromMSet(x: SEXP, mset: SEXP) {
    if x == R_NilValue() || crate::mainutils::relop::isSymbol(x) != 0 {
        return;
    }
    checked_memory(OwnerToken::with_current(|owner| {
        let object = checked_memory(owner.sexp(x));
        let mset = mset_check(owner.sexp(mset));
        let current = MSet::read(&mset);
        let n = i64::from(current.len);
        let Some(i) = (0..n)
            .rev()
            .find(|&i| checked_memory(current.store.try_vector_elt(i)).as_raw() == object.as_raw())
        else {
            return;
        };
        let mut count = mset_check(SexpMut::try_from_checked(current.count.clone()));
        let mut output = mset_check(SexpMut::try_from_checked(current.store.clone()));
        for j in i..n - 1 {
            let value = checked_memory(current.store.try_vector_elt(j + 1));
            checked_memory(output.try_set_vector_elt(j, value));
        }
        let nil = checked_memory(owner.sexp(R_NilValue()));
        checked_memory(output.try_set_vector_elt(n - 1, nil));
        checked_memory(count.try_set_integer_elt(0, current.len - 1));
    }));
}

// ---------------------------------------------------------------------------
// Resizable vector support
// ---------------------------------------------------------------------------

/// Check if a vector is resizable.
///
/// This is the equivalent of R's `R_isResizable()`.
pub fn R_isResizable(x: SEXP) -> c_int {
    c_int::from(crate::sexp::accessors::has_resizable_flag(x))
}

/// Get the maximum length of a resizable vector.
///
/// This is the equivalent of R's `R_maxLength()`.
pub fn R_maxLength(x: SEXP) -> R_xlen_t {
    {
        if x.is_null() {
            return 0;
        }
        LENGTH(x) as R_xlen_t
    }
}

/// Allocate a resizable vector.
///
/// This is the equivalent of R's `R_allocResizableVector()`.
pub fn R_allocResizableVector(type_: SEXPTYPE, maxlen: R_xlen_t) -> SEXP {
    Rf_allocVector3(type_.0, maxlen)
}

/// Duplicate a vector and make it resizable.
///
/// This is the equivalent of R's `R_duplicateAsResizable()`.
pub fn R_duplicateAsResizable(x: SEXP) -> SEXP {
    let dup = crate::mainutils::duplicate::Rf_duplicate(x);
    crate::sexp::accessors::set_resizable_flag(dup);
    dup
}

// ---------------------------------------------------------------------------
// Protection stack error handlers
// ---------------------------------------------------------------------------

/// Signal a protect stack overflow error.
///
/// This is the equivalent of R's `R_signal_protect_error()`.
pub fn R_signal_protect_error() {
    {
        crate::mainutils::errors::errorcall_cstr(R_NilValue(), c"protect stack overflow");
    }
}

pub fn R_signal_unprotect_error() {
    {
        crate::mainutils::errors::errorcall_cstr(R_NilValue(), c"unprotect stack underflow");
    }
}

// ---------------------------------------------------------------------------
// InitMemory / initStack stubs
// ---------------------------------------------------------------------------

/// Initialize R's memory subsystem.
///
/// This is the equivalent of R's `InitMemory()`.
pub fn InitMemory() {
    // Arena and GC state are owned by the active RInstance.
}

/// Reset the protection stack.
///
/// This is the equivalent of R's `initStack()`.
pub fn initStack() {
    // Arena-based allocation doesn't need PP stack sizing
}

// ---------------------------------------------------------------------------
// Memory profile
// ---------------------------------------------------------------------------

/// Return a small session-local memory profile.
///
/// This is the equivalent of R's `do_memoryprofile()`.
pub fn do_memoryprofile(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    checked_memory(OwnerToken::with_current(|owner| {
        let raw = Rf_allocVector3(SEXPTYPE::REALSXP, 4);
        checked_memory(owner.require_active());
        if raw.is_null() {
            return R_NilValue();
        }
        let mut result =
            checked_memory(SexpMut::try_from_checked(checked_memory(owner.sexp(raw))));
        let (nodes, free, bytes) = checked_memory(owner.with_arena(|arena| {
            (
                arena.node_count(),
                arena.free_count(),
                arena.total_bytes_allocated(),
            )
        }));
        let peak = crate::sexp::gengc::get_gc_stats()
            .peak_memory
            .max(bytes);
        for (i, value) in [nodes, free, bytes, peak].into_iter().enumerate() {
            checked_memory(result.try_set_real_elt(i as R_xlen_t, value as f64));
        }
        result.freeze().as_raw()
    }))
}

// ---------------------------------------------------------------------------
// do_* stubs for GC-related .Internal / .Primitive calls
// ---------------------------------------------------------------------------

/// gc() implementation.
///
/// This is the equivalent of R's `do_gc()`.
pub fn do_gc(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    crate::mainutils::essentials::do_gc(_call, _op, _args, _rho)
}

/// gcinfo() implementation (stub).
///
/// This is the equivalent of R's `do_gcinfo()`.
pub fn do_gcinfo(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let old = Rf_ScalarLogical(with_memory_state(|state| state.gc_reporting));
    let i = crate::mainutils::coerce::asLogical(CAR(args));
    if i != crate::sexp::ffi::NA_LOGICAL {
        with_memory_state(|state| state.gc_reporting = i);
    }
    old
}

pub fn do_gctorture(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let old = Rf_ScalarLogical(if with_memory_state(|state| state.gc_force_gap) > 0 {
        crate::sexp::ffi::TRUE
    } else {
        crate::sexp::ffi::FALSE
    });
    let gap = crate::mainutils::coerce::asLogical(CAR(args));
    R_gc_torture(if gap != 0 { 1 } else { 0 }, 0, 0);
    old
}

pub fn do_gctorture2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let old = Rf_ScalarInteger(with_memory_state(|state| state.gc_force_gap));
    let gap = crate::mainutils::coerce::asInteger(CAR(args));
    let wait = crate::mainutils::coerce::asInteger(CADR(args));
    R_gc_torture(gap, wait, 0);
    old
}

/// maxVSize() implementation.
///
/// This is the equivalent of R's `do_maxVSize()`.
pub fn do_maxVSize(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        const MB: f64 = 1_048_576.0;
        let new_limit_mb = crate::mainutils::coerce::asReal(CAR(args));
        if new_limit_mb > 0.0 {
            if new_limit_mb.is_infinite() {
                let _ = R_SetMaxVSize(u64::MAX);
            } else {
                let new_limit = (new_limit_mb * MB).ceil();
                if new_limit >= u64::MAX as f64 {
                    let _ = R_SetMaxVSize(u64::MAX);
                } else {
                    let _ = R_SetMaxVSize(new_limit as u64);
                }
            }
        }

        let limit = R_GetMaxVSize_memory();
        if limit == u64::MAX {
            Rf_ScalarReal(f64::INFINITY)
        } else {
            Rf_ScalarReal(limit as f64 / MB)
        }
    }
}

/// maxNSize() implementation.
///
/// This is the equivalent of R's `do_maxNSize()`.
pub fn do_maxNSize(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        let new_limit = crate::mainutils::coerce::asReal(CAR(args));
        if new_limit > 0.0 {
            if new_limit.is_infinite() || new_limit >= u64::MAX as f64 {
                let _ = R_SetMaxNSize(u64::MAX);
            } else {
                let _ = R_SetMaxNSize(new_limit.ceil() as u64);
            }
        }

        let limit = R_GetMaxNSize_memory();
        if limit == u64::MAX {
            Rf_ScalarReal(f64::INFINITY)
        } else {
            Rf_ScalarReal(limit as f64)
        }
    }
}

/// Register finalizer .Internal call.
///
/// This is the equivalent of R's `do_regFinaliz()`.
pub fn do_regFinaliz(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let obj = CAR(args);
    let fun = CADR(args);
    let onexit = crate::mainutils::coerce::asLogical(CADDR(args));
    let t = TYPEOF(obj);
    if t != SEXPTYPE::ENVSXP && t != SEXPTYPE::EXTPTRSXP {
        error("first argument must be environment or external pointer");
    }
    if TYPEOF(fun) != SEXPTYPE::CLOSXP {
        error("second argument must be a function");
    }
    if onexit == crate::sexp::ffi::NA_LOGICAL {
        error("third argument must be 'TRUE' or 'FALSE'");
    }
    R_RegisterFinalizerEx(obj, fun, onexit);
    R_NilValue()
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
    fn test_init_memory_does_not_crash() {
        let _session = crate::sexp::session::RSession::new();
        {
            InitMemory();
            initStack();
        }
    }

    #[test]
    fn test_r_allocld() {
        let _session = crate::sexp::session::RSession::new();
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let ptr = R_allocLD(10);
            // May or may not be null depending on implementation; just check it doesn't crash
            let _ = ptr;
        });
    }

    #[test]
    fn test_r_string_buffer() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut buf = R_StringBuffer::default();
            let ptr = R_AllocStringBuffer(100, &mut buf);
            assert!(!ptr.is_null());
            R_FreeStringBuffer(&mut buf);
            assert!(buf.data.is_null());
            assert_eq!(buf.bufsize, 0);
        }
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
    fn test_weak_ref_roundtrip() {
        let _session = crate::sexp::session::RSession::new();
        let key = R_MakeExternalPtr(ptr::null_mut(), R_NilValue(), R_NilValue());
        let val = crate::sexp::constructors::Rf_ScalarInteger(99);
        let w = R_MakeWeakRef(key, val, R_NilValue(), 0);
        assert!(!w.is_null());

        let k = R_WeakRefKey(w);
        assert_eq!(k, key);

        let v = R_WeakRefValue(w);
        assert_eq!(v, val);

        // null weak ref returns nil
        assert_eq!(R_WeakRefKey(ptr::null_mut()), R_NilValue());
        assert_eq!(R_WeakRefValue(ptr::null_mut()), R_NilValue());
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
    fn test_weak_ref_accessors_reject_non_weak_ref_like_gnu_r() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let not_weak_ref = crate::sexp::constructors::Rf_ScalarInteger(42);
            let key_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = R_WeakRefKey(not_weak_ref);
            }));
            let value_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = R_WeakRefValue(not_weak_ref);
            }));
            assert!(key_result.is_err());
            assert!(value_result.is_err());
        }
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
    fn test_resizable_vector_nulls() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(R_isResizable(ptr::null_mut()), 0);
            assert_eq!(R_maxLength(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_memory_limits_are_session_local_and_enforced() {
        let left = RSession::new();
        let right = RSession::new();
        let mut left_v_limit = 0u64;
        let mut left_n_limit = 0u64;

        left.with_protected(|| unsafe {
            left_v_limit = current_vector_heap_size().saturating_add(1_048_576);
            left_n_limit = current_node_heap_size().saturating_add(100);
            assert_eq!(R_SetMaxVSize(left_v_limit), crate::sexp::ffi::TRUE);
            assert_eq!(R_SetMaxNSize(left_n_limit), crate::sexp::ffi::TRUE);
            assert_eq!(R_GetMaxVSize_memory(), left_v_limit);
            assert_eq!(R_GetMaxNSize_memory(), left_n_limit);
            assert_eq!(
                R_SetMaxVSize(current_vector_heap_size().saturating_sub(1)),
                crate::sexp::ffi::FALSE
            );
            assert_eq!(
                R_SetMaxNSize(current_node_heap_size().saturating_sub(1)),
                crate::sexp::ffi::FALSE
            );
        });

        right.with_protected(|| unsafe {
            // The macOS startup default caps the vector pool at
            // max(physical memory, 16 Gb); assert session locality (left's
            // tighter limit must not leak) rather than the sentinel value.
            let right_v = R_GetMaxVSize_memory();
            assert_ne!(right_v, left_v_limit, "v limit leaked across sessions");
            assert_ne!(
                R_GetMaxNSize_memory(),
                left_n_limit,
                "n limit leaked across sessions"
            );
        });
    }

    #[test]
    fn test_do_max_size_roundtrips_limits() {
        let _session = crate::sexp::session::RSession::new();
        let session = RSession::new();
        session.with_protected(|| unsafe {
            let target_bytes = current_vector_heap_size().saturating_add(2 * 1_048_576);
            let target_mb = target_bytes as f64 / 1_048_576.0;
            let v_args = Rf_cons(Rf_ScalarReal(target_mb), R_NilValue());
            let v_result = do_maxVSize(ptr::null_mut(), ptr::null_mut(), v_args, ptr::null_mut());
            assert_eq!(TYPEOF(v_result), SEXPTYPE::REALSXP);
            assert!((*REAL(v_result) - target_mb).abs() < 1e-9);

            let target_nodes = current_node_heap_size().saturating_add(250);
            let n_args = Rf_cons(Rf_ScalarReal(target_nodes as f64), R_NilValue());
            let n_result = do_maxNSize(ptr::null_mut(), ptr::null_mut(), n_args, ptr::null_mut());
            assert_eq!(TYPEOF(n_result), SEXPTYPE::REALSXP);
            assert_eq!(*REAL(n_result), target_nodes as f64);

            let inf_args = Rf_cons(Rf_ScalarReal(f64::INFINITY), R_NilValue());
            let inf_result =
                do_maxVSize(ptr::null_mut(), ptr::null_mut(), inf_args, ptr::null_mut());
            assert!((*REAL(inf_result)).is_infinite());
            assert_eq!(R_GetMaxVSize_memory(), u64::MAX);
        });
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
    fn test_finalizers_ignore_null_targets() {
        let _session = crate::sexp::session::RSession::new();
        let session = RSession::new();
        session.with_protected(|| unsafe {
            with_memory_state(|state| state.pending_finalizers.clear());
            R_RegisterFinalizer(ptr::null_mut(), ptr::null_mut());
            R_RegisterFinalizerEx(ptr::null_mut(), ptr::null_mut(), 0);
            R_RegisterCFinalizer(ptr::null_mut(), dummy_c_finalizer);
            R_RegisterCFinalizerEx(ptr::null_mut(), dummy_c_finalizer, 0);
            R_RunPendingFinalizers();
            R_RunFinalizers();
            assert!(with_memory_state(|state| state
                .pending_finalizers
                .is_empty()));
        });
    }

    #[test]
    fn test_c_finalizer_runs_with_external_pointer_target() {
        let _session = crate::sexp::session::RSession::new();
        let session = RSession::new();
        session.with_protected(|| unsafe {
            with_memory_state(|state| state.pending_finalizers.clear());
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
            assert_eq!(with_memory_state(|state| state.pending_finalizers.len()), 1);
        });
    }

    #[test]
    fn test_c_finalizer_runs_once_after_target_becomes_unreachable() {
        let _session = crate::sexp::session::RSession::new();
        let session = RSession::new();
        session.with_protected(|| unsafe {
            with_memory_state(|state| state.pending_finalizers.clear());
            FINALIZER_RUNS.set(0);

            let extptr = R_MakeExternalPtr(ptr::null_mut(), R_NilValue(), R_NilValue());
            R_RegisterCFinalizer(extptr, count_c_finalizer);

            R_RunPendingFinalizers();
            assert_eq!(FINALIZER_RUNS.get(), 0);

            R_gc();
            assert_eq!(FINALIZER_RUNS.get(), 1);
            assert!(with_memory_state(|state| state
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
            with_memory_state(|state| state.pending_finalizers.clear());
            FINALIZER_RUNS.set(0);

            let extptr = R_MakeExternalPtr(ptr::null_mut(), R_NilValue(), R_NilValue());
            R_RegisterCFinalizerEx(extptr, count_c_finalizer, 1);

            R_RunPendingFinalizers();
            assert_eq!(FINALIZER_RUNS.get(), 0);

            R_RunExitFinalizers_memory();
            assert_eq!(FINALIZER_RUNS.get(), 1);
            assert!(with_memory_state(|state| state
                .pending_finalizers
                .is_empty()));
        });
    }

    #[test]
    fn test_r_finalizer_registration_tracks_target_and_function() {
        let _session = crate::sexp::session::RSession::new();
        let session = RSession::new();
        session.with_protected(|| unsafe {
            with_memory_state(|state| state.pending_finalizers.clear());
            let obj = R_MakeExternalPtr(ptr::null_mut(), R_NilValue(), R_NilValue());
            let body = Rf_ScalarInteger(123);
            let fun = crate::mainutils::dstruct::mkCLOSXP(R_NilValue(), body, R_GlobalEnv());

            R_RegisterFinalizerEx(obj, fun, 0);
            with_memory_state(|state| {
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
            assert_eq!(with_memory_state(|state| state.pending_finalizers.len()), 1);
        });
    }

    #[test]
    fn test_make_weak_ref_with_r_finalizer_registers_deferred_finalizer() {
        let _session = crate::sexp::session::RSession::new();
        let session = RSession::new();
        session.with_protected(|| {
            with_memory_state(|state| state.pending_finalizers.clear());
            let key = R_MakeExternalPtr(ptr::null_mut(), R_NilValue(), R_NilValue());
            let body = Rf_ScalarInteger(123);
            // SAFETY: nil formals, fresh body, global env — a complete closure.
            let fun = unsafe {
                crate::mainutils::dstruct::mkCLOSXP(R_NilValue(), body, R_GlobalEnv())
            };

            let weak = R_MakeWeakRef(key, R_NilValue(), fun, 1);
            assert_eq!(TYPEOF(weak), SEXPTYPE::WEAKREFSXP);
            with_memory_state(|state| {
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
    fn test_chk_calloc() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let p = R_chk_calloc(10, 8);
            assert!(!p.is_null());
            R_chk_free(p);
        }
    }

    #[test]
    fn test_chk_memcpy_memset() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let buf = R_chk_calloc(100, 1);
            assert!(!buf.is_null());

            R_chk_memset(buf, 0xAA, 10);
            let bytes = std::slice::from_raw_parts(buf as *const u8, 10);
            for &b in bytes {
                assert_eq!(b, 0xAA);
            }

            let buf2 = R_chk_calloc(10, 1);
            R_chk_memcpy(buf2, buf, 10);
            let bytes2 = std::slice::from_raw_parts(buf2 as *const u8, 10);
            for (i, &b) in bytes2.iter().enumerate() {
                assert_eq!(b, 0xAA);
                let _ = i;
            }

            R_chk_free(buf);
            R_chk_free(buf2);
        }
    }

    #[test]
    fn test_type_checking_functions() {
        let _session = crate::sexp::session::RSession::new();
        // R_NilValue should be detected as NULL
        assert_eq!(Rf_isNull_memory(R_NilValue()), 1);
        assert_eq!(Rf_isNull_memory(ptr::null_mut()), 1);

        // Non-null non-NilValue should not be NULL
        // (can't easily test without a valid SEXP)
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

    #[test]
    fn test_readline_stub() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = readline(ptr::null_mut());
            assert!(result.is_null());
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
    fn owned_gc_operation_live_panic_restores_original_state_and_payload() {
        let session = RSession::new_for_gc_tests();
        let original = pin(&session);
        let instance = original.as_ptr();
        register_gc_callback(Box::new(move |_| {
            assert_eq!(unsafe { (*instance).memory_state.in_gc }, 1);
            std::panic::panic_any(73_u32);
        }));
        let outcome = catch_unwind(AssertUnwindSafe(|| R_gc()));
        assert_eq!(*outcome.unwrap_err().downcast::<u32>().unwrap(), 73);
        assert_eq!(unsafe { (*instance).memory_state.in_gc }, 0);
        assert_eq!(unsafe { (*instance).memory_state.gc_count }, 1);
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
            assert_eq!((*instance).memory_state.in_gc, 1);
            R_gc();
            assert_eq!((*instance).memory_state.in_gc, 1);
        }));
        R_gc();
        assert_eq!(calls.get(), 1);
        assert_eq!(unsafe { (*instance).memory_state.in_gc }, 0);
        assert_eq!(unsafe { (*instance).memory_state.gc_count }, 2);
    }

    fn drops_facade(full: bool) {
        let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
        let original = pin(facade.borrow().as_ref().unwrap());
        let instance = original.as_ptr();
        let other = RSession::new_for_gc_tests();
        let other_pin = pin(&other);
        unsafe { (*other_pin.as_ptr()).memory_state.in_gc = 9 };
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
        assert_eq!(unsafe { (*instance).memory_state.in_gc }, 0);
        assert_eq!(unsafe { (*other_pin.as_ptr()).memory_state.in_gc }, 9);
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
                    assert_eq!((*instance).memory_state.in_gc, 1);
                    assert!(binding_node.is_live());
                    assert_eq!((*instance).legacy_protect.len(), 0);
                    (*instance).memory_state.gc_force_gap = 0;
                    if revoke {
                        drop(callback_facade.upgrade().unwrap().borrow_mut().take());
                    } else {
                        std::panic::panic_any(127_u32);
                    }
                }));
                (*instance).memory_state.gc_force_gap = 1;
                (*instance).memory_state.gc_force_wait = 1;
                factory.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
            })
        }));
        assert_eq!(calls.get(), 1);
        assert_eq!(unsafe { (*instance).memory_state.in_gc }, 0);
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
