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
use crate::sexp::ffi::{NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_GlobalEnv, R_NilValue};

unsafe fn error(msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    });
}

// ---------------------------------------------------------------------------
// GC control
// ---------------------------------------------------------------------------

pub type R_CFinalizer_t = unsafe extern "C" fn(*mut c_void);

pub(crate) enum PendingFinalizer {
    C { obj: SEXP, fun: R_CFinalizer_t },
    R { obj: SEXP, fun: SEXP },
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
            max_v_size: u64::MAX,
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
    crate::sexp::instance::with_required_current_instance(|inst| f(&mut inst.memory_state))
}

/// Returns whether a GC is currently running.
///
/// This is the equivalent of R's `R_gc_running()`.
pub unsafe fn R_gc_running() -> c_int {
    crate::sexp::instance::with_current_instance(|inst| inst.memory_state.in_gc).unwrap_or(0)
}

/// Trigger a full garbage collection.
///
/// This is the equivalent of R's `R_gc()`.
pub unsafe fn R_gc() {
    with_memory_state(|state| {
        state.gc_count += 1;
        state.in_gc = 1;
    });
    crate::sexp::gengc::full_gc();
    with_memory_state(|state| state.in_gc = 0);
}

/// Trigger a lightweight garbage collection.
///
/// This is the equivalent of R's `R_gc_lite()`.
pub unsafe fn R_gc_lite() {
    with_memory_state(|state| state.gc_count += 1);
    crate::sexp::gengc::minor_gc();
}

/// GC torture settings.
///
/// When `gap > 0`, every `gap` allocations will force a GC cycle.
/// This is a debugging aid for finding GC-safety bugs.
pub unsafe fn R_gc_torture(gap: c_int, wait: c_int, _inhibit: c_int) {
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
pub(crate) unsafe fn R_allocLD(nelem: usize) -> *mut c_void {
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
pub unsafe fn S_alloc(nelem: c_long, eltsize: c_int) -> *mut c_char {
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
pub unsafe fn R_malloc_gc(n: usize) -> *mut c_void {
    unsafe {
        let p = libc::malloc(n);
        if p.is_null() {
            R_gc();
            return libc::malloc(n);
        }
        p
    }
}

/// calloc with GC fallback.
///
/// This is the equivalent of R's `R_calloc_gc()`.
pub unsafe fn R_calloc_gc(n: usize, s: usize) -> *mut c_void {
    unsafe {
        let p = libc::calloc(n, s);
        if p.is_null() {
            R_gc();
            return libc::calloc(n, s);
        }
        p
    }
}

/// realloc with GC fallback.
///
/// This is the equivalent of R's `R_realloc_gc()`.
pub unsafe fn R_realloc_gc(p: *mut c_void, n: usize) -> *mut c_void {
    unsafe {
        let q = libc::realloc(p, n);
        if q.is_null() {
            R_gc();
            return libc::realloc(p, n);
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
pub unsafe fn R_chk_calloc(nelem: usize, elsize: usize) -> *mut c_void {
    unsafe {
        let p = libc::calloc(nelem, elsize);
        if p.is_null() {
            static MSG: &[u8] = b"memory allocation failed (calloc)\0";
            crate::mainutils::errors::Rf_error(MSG.as_ptr() as *const c_char);
        }
        p
    }
}

/// Checked realloc that errors on failure.
///
/// This is the equivalent of R's `R_chk_realloc()`.
pub unsafe fn R_chk_realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    unsafe {
        let p = if !ptr.is_null() {
            libc::realloc(ptr, size)
        } else {
            libc::malloc(size)
        };
        if p.is_null() {
            static MSG: &[u8] = b"memory allocation failed (realloc)\0";
            crate::mainutils::errors::Rf_error(MSG.as_ptr() as *const c_char);
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
            libc::free(ptr);
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
pub unsafe fn sexptype2char(type_: SEXPTYPE) -> *const c_char {
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
pub(crate) unsafe fn Rf_isNull_memory(s: SEXP) -> c_int {
    unsafe {
        if s.is_null() || s == R_NilValue() {
            1
        } else {
            0
        }
    }
}

/// Check if SEXP is a symbol.
pub(crate) unsafe fn Rf_isSymbol_memory(s: SEXP) -> c_int {
    unsafe {
        if s.is_null() {
            0
        } else {
            let t = TYPEOF(s);
            if t == SEXPTYPE::SYMSXP { 1 } else { 0 }
        }
    }
}

/// Check if SEXP is a logical vector.
pub(crate) unsafe fn Rf_isLogical_memory(s: SEXP) -> c_int {
    unsafe {
        if s.is_null() {
            0
        } else {
            let t = TYPEOF(s);
            if t == SEXPTYPE::LGLSXP { 1 } else { 0 }
        }
    }
}

/// Check if SEXP is a real (numeric) vector.
pub(crate) unsafe fn Rf_isReal_memory(s: SEXP) -> c_int {
    unsafe {
        if s.is_null() {
            0
        } else {
            let t = TYPEOF(s);
            if t == SEXPTYPE::REALSXP { 1 } else { 0 }
        }
    }
}

/// Check if SEXP is a complex vector.
pub(crate) unsafe fn Rf_isComplex_memory(s: SEXP) -> c_int {
    unsafe {
        if s.is_null() {
            0
        } else {
            let t = TYPEOF(s);
            if t == SEXPTYPE::CPLXSXP { 1 } else { 0 }
        }
    }
}

/// Check if SEXP is an expression.
pub(crate) unsafe fn Rf_isExpression_memory(s: SEXP) -> c_int {
    unsafe {
        if s.is_null() {
            0
        } else {
            let t = TYPEOF(s);
            if t == SEXPTYPE::EXPRSXP { 1 } else { 0 }
        }
    }
}

/// Check if SEXP is an environment.
pub(crate) unsafe fn Rf_isEnvironment_memory(s: SEXP) -> c_int {
    unsafe {
        if s.is_null() {
            0
        } else {
            let t = TYPEOF(s);
            if t == SEXPTYPE::ENVSXP { 1 } else { 0 }
        }
    }
}

/// Check if SEXP is a string (character vector).
pub(crate) unsafe fn Rf_isString_memory(s: SEXP) -> c_int {
    unsafe {
        if s.is_null() {
            0
        } else {
            let t = TYPEOF(s);
            if t == SEXPTYPE::STRSXP { 1 } else { 0 }
        }
    }
}

/// Check if SEXP is an S4 object.
pub(crate) unsafe fn Rf_isObject_memory(s: SEXP) -> c_int {
    unsafe {
        if s.is_null() {
            0
        } else {
            let t = TYPEOF(s);
            if t == SEXPTYPE::OBJSXP { 1 } else { 0 }
        }
    }
}

// ---------------------------------------------------------------------------
// R_PreserveObject / R_ReleaseObject (duplicates — no #[unsafe(no_mangle)])
//
// Already in sexp/protect.rs with #[unsafe(no_mangle)].
// ---------------------------------------------------------------------------

pub(crate) unsafe fn R_PreserveObject_memory(s: SEXP) {
    unsafe {
        crate::sexp::protect::R_PreserveObject(s);
    }
}

pub(crate) unsafe fn R_ReleaseObject_memory(s: SEXP) {
    unsafe {
        crate::sexp::protect::R_ReleaseObject(s);
    }
}

// ---------------------------------------------------------------------------
// Weak references and finalizers
// ---------------------------------------------------------------------------

pub unsafe fn R_MakeWeakRef(key: SEXP, val: SEXP, fin: SEXP, _onexit: c_int) -> SEXP {
    unsafe {
        if !is_valid_r_finalizer(fin) {
            error("finalizer must be a function or NULL");
        }
        let s = crate::sexp::memory_ext::allocSExp(SEXPTYPE::WEAKREFSXP);
        if s.is_null() {
            error("could not allocate weak reference");
        }
        (*s).data.listsxp.carval = key;
        (*s).data.listsxp.cdrval = val;
        (*s).data.listsxp.tagval = fin;
        s
    }
}

pub unsafe fn R_MakeWeakRefC(key: SEXP, val: SEXP, fin: R_CFinalizer_t, _onexit: c_int) -> SEXP {
    unsafe {
        let s = R_MakeWeakRef(key, val, R_NilValue(), 0);
        if !s.is_null() {
            with_memory_state(|state| {
                state
                    .pending_finalizers
                    .push(PendingFinalizer::C { obj: key, fun: fin });
            });
        }
        s
    }
}

pub unsafe fn R_WeakRefKey(w: SEXP) -> SEXP {
    unsafe {
        if w.is_null() {
            return R_NilValue();
        }
        (*w).data.listsxp.carval
    }
}

pub unsafe fn R_WeakRefValue(w: SEXP) -> SEXP {
    unsafe {
        if w.is_null() {
            return R_NilValue();
        }
        (*w).data.listsxp.cdrval
    }
}

pub unsafe fn R_RegisterFinalizer(s: SEXP, fun: SEXP) {
    unsafe {
        R_RegisterFinalizerEx(s, fun, 0);
    }
}

pub unsafe fn R_RegisterFinalizerEx(s: SEXP, fun: SEXP, _onexit: c_int) {
    unsafe {
        if s.is_null() || fun.is_null() || fun == R_NilValue() {
            return;
        }
        if !is_valid_r_finalizer(fun) {
            error("finalizer must be a function or NULL");
        }
        with_memory_state(|state| {
            state
                .pending_finalizers
                .push(PendingFinalizer::R { obj: s, fun });
        });
    }
}

pub unsafe fn R_RegisterCFinalizer(s: SEXP, fun: R_CFinalizer_t) {
    unsafe {
        R_RegisterCFinalizerEx(s, fun, 0);
    }
}

pub unsafe fn R_RegisterCFinalizerEx(s: SEXP, fun: R_CFinalizer_t, _onexit: c_int) {
    if s.is_null() {
        return;
    }
    with_memory_state(|state| {
        state
            .pending_finalizers
            .push(PendingFinalizer::C { obj: s, fun });
    });
}

pub unsafe fn R_RunPendingFinalizers() {
    let finalizers = with_memory_state(|state| {
        if state.running_finalizers || state.pending_finalizers.is_empty() {
            Vec::new()
        } else {
            state.running_finalizers = true;
            std::mem::take(&mut state.pending_finalizers)
        }
    });
    if finalizers.is_empty() {
        return;
    }

    for finalizer in finalizers {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            run_pending_finalizer(finalizer);
        }));
    }

    with_memory_state(|state| {
        state.running_finalizers = false;
    });
}

unsafe fn run_pending_finalizer(finalizer: PendingFinalizer) {
    unsafe {
        match finalizer {
            PendingFinalizer::C { obj, fun } => {
                if !obj.is_null() {
                    fun(obj as *mut c_void);
                }
            }
            PendingFinalizer::R { obj, fun } => {
                if !obj.is_null() && !fun.is_null() && fun != R_NilValue() {
                    let call = Rf_lang2(fun, obj);
                    let _ = crate::eval::eval::Rf_eval(call, R_GlobalEnv());
                }
            }
        }
    }
}

unsafe fn is_valid_r_finalizer(fun: SEXP) -> bool {
    unsafe {
        if fun.is_null() || fun == R_NilValue() {
            return true;
        }
        matches!(
            SEXPTYPE(TYPEOF(fun)),
            SEXPTYPE::CLOSXP | SEXPTYPE::BUILTINSXP | SEXPTYPE::SPECIALSXP
        )
    }
}

pub unsafe fn R_RunFinalizers() {
    unsafe {
        R_RunPendingFinalizers();
    }
}

pub(crate) unsafe fn R_RunExitFinalizers_memory() {
    unsafe {
        R_RunFinalizers();
    }
}

// ---------------------------------------------------------------------------
// External pointer management
//
// In R's C code, EXTPTRSXP uses the listsxp union layout:
//   EXTPTR_PTR(s)  = s->u.listsxp.carval  = extptr[0]  (void*)
//   EXTPTR_PROT(s) = CDR(s)               = extptr[1]  (SEXP)
//   EXTPTR_TAG(s)  = TAG(s)               = extptr[2]  (SEXP)
// ---------------------------------------------------------------------------

/// Create an external pointer.
///
/// This is the equivalent of R's `R_MakeExternalPtr()`.
pub unsafe fn R_MakeExternalPtr(p: *mut c_void, tag: SEXP, prot: SEXP) -> SEXP {
    unsafe {
        let s = crate::sexp::memory_ext::allocSExp(SEXPTYPE::EXTPTRSXP);
        if s.is_null() {
            error("could not allocate external pointer");
        }
        (*s).data.extptr[0] = p;
        (*s).data.extptr[1] = prot as *mut c_void;
        (*s).data.extptr[2] = tag as *mut c_void;
        s
    }
}

/// Get the address stored in an external pointer.
///
/// This is the equivalent of R's `R_ExternalPtrAddr()`.
pub unsafe fn R_ExternalPtrAddr(s: SEXP) -> *mut c_void {
    unsafe {
        if s.is_null() {
            return ptr::null_mut();
        }
        (*s).data.extptr[0]
    }
}

/// Get the tag of an external pointer.
///
/// This is the equivalent of R's `R_ExternalPtrTag()`.
pub unsafe fn R_ExternalPtrTag(s: SEXP) -> SEXP {
    unsafe {
        if s.is_null() {
            return R_NilValue();
        }
        (*s).data.extptr[2] as SEXP
    }
}

/// Get the protected value of an external pointer.
///
/// This is the equivalent of R's `R_ExternalPtrProtected()`.
pub unsafe fn R_ExternalPtrProtected(s: SEXP) -> SEXP {
    unsafe {
        if s.is_null() {
            return R_NilValue();
        }
        (*s).data.extptr[1] as SEXP
    }
}

/// Clear the address stored in an external pointer.
///
/// This is the equivalent of R's `R_ClearExternalPtr()`.
pub unsafe fn R_ClearExternalPtr(s: SEXP) {
    unsafe {
        if !s.is_null() {
            (*s).data.extptr[0] = ptr::null_mut();
        }
    }
}

/// Set the address stored in an external pointer.
///
/// This is the equivalent of R's `R_SetExternalPtrAddr()`.
pub unsafe fn R_SetExternalPtrAddr(s: SEXP, p: *mut c_void) {
    unsafe {
        if !s.is_null() {
            (*s).data.extptr[0] = p;
        }
    }
}

/// Set the tag of an external pointer.
///
/// This is the equivalent of R's `R_SetExternalPtrTag()`.
pub unsafe fn R_SetExternalPtrTag(s: SEXP, tag: SEXP) {
    unsafe {
        if !s.is_null() {
            (*s).data.extptr[2] = tag as *mut c_void;
        }
    }
}

/// Set the protected value of an external pointer.
///
/// This is the equivalent of R's `R_SetExternalPtrProtected()`.
pub unsafe fn R_SetExternalPtrProtected(s: SEXP, p: SEXP) {
    unsafe {
        if !s.is_null() {
            (*s).data.extptr[1] = p as *mut c_void;
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
pub(crate) unsafe fn R_GetMaxVSize_memory() -> u64 {
    crate::sexp::instance::with_current_instance(|inst| inst.memory_state.max_v_size)
        .unwrap_or(u64::MAX)
}

/// Get the maximum node heap size.
/// Duplicate — no #[unsafe(no_mangle)] (already in mainutils/main.rs).
pub(crate) unsafe fn R_GetMaxNSize_memory() -> u64 {
    crate::sexp::instance::with_current_instance(|inst| inst.memory_state.max_n_size)
        .unwrap_or(u64::MAX)
}

/// Set the maximum vector heap size.
pub unsafe fn R_SetMaxVSize(size: u64) -> c_int {
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
pub unsafe fn R_SetMaxNSize(size: u64) -> c_int {
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
pub unsafe fn R_SetPPSize(_size: u64) {
    // Arena-based allocation doesn't need PP stack sizing
}

fn current_vector_heap_size() -> u64 {
    crate::sexp::memory::with_arena(|arena| arena.total_bytes_allocated() as u64)
}

fn current_node_heap_size() -> u64 {
    crate::sexp::memory::with_arena(|arena| arena.node_count() as u64)
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
    unsafe { crate::unix::system::R_ReadConsole(prompt, buf as *mut u8, len, addtohistory) }
}

/// Write to the active console callback.
/// Duplicate — no #[unsafe(no_mangle)] (already in unix/system.rs).
pub(crate) unsafe fn R_WriteConsole_memory(buf: *const c_char, len: c_int) {
    unsafe { crate::unix::system::R_WriteConsole(buf, len) }
}

// ---------------------------------------------------------------------------
// readline wrapper

/// GNU readline-compatible wrapper.
///
/// This is the equivalent of R's `readline()` function.
pub unsafe fn readline(prompt: *const c_char) -> *mut c_char {
    unsafe {
        let mut buffer = vec![0u8; 8192];
        let ok = crate::unix::system::R_ReadConsole(prompt, buffer.as_mut_ptr(), 8192, 1);
        if ok == 0 {
            return ptr::null_mut();
        }
        let nul = buffer
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(buffer.len());
        let line = std::ffi::CString::new(&buffer[..nul]).unwrap_or_default();
        libc::strdup(line.as_ptr())
    }
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
        if needed < buf.bufsize {
            return buf.data as *mut c_void;
        }

        let mut newsize = needed;
        let bsize = buf.defaultSize;
        newsize = (newsize / bsize) * bsize;
        if newsize < needed {
            newsize += bsize;
        }

        if buf.data.is_null() {
            buf.data = libc::malloc(newsize) as *mut c_char;
            if !buf.data.is_null() {
                *buf.data = 0;
            }
        } else {
            buf.data = libc::realloc(buf.data as *mut c_void, newsize) as *mut c_char;
        }

        if buf.data.is_null() {
            buf.bufsize = 0;
            error("could not allocate memory in R_AllocStringBuffer");
        }

        buf.bufsize = newsize;
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
            libc::free(buf.data as *mut c_void);
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
                libc::free(buf.data as *mut c_void);
                buf.data = ptr::null_mut();
            }
            buf.bufsize = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// Multi-set preservation (for bison-generated parsers)
// ---------------------------------------------------------------------------

/// Create a new multi-set for protecting objects.
///
/// This is the equivalent of R's `R_NewPreciousMSet()`.
///
/// Multi-set representation: `CONS(store, npreserved)` with `TAG() == initialSize`.
/// `store` is a VECSXP or R_NilValue. `npreserved` is an INTSXP of length 1.
pub unsafe fn R_NewPreciousMSet(initialSize: c_int) -> SEXP {
    unsafe {
        let npreserved = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
        if npreserved.is_null() {
            error("could not allocate precious mset");
        }
        crate::sexp::accessors::SET_INTEGER_ELT(npreserved, 0, 0);

        let mset = Rf_cons(R_NilValue(), npreserved);
        if mset.is_null() {
            error("could not allocate precious mset");
        }

        let size = if initialSize < 0 { 0 } else { initialSize };
        let isize = Rf_ScalarInteger(size);
        crate::sexp::accessors::SETTAG(mset, isize);

        mset
    }
}

/// Add an object to a multi-set.
///
/// This is the equivalent of R's `R_PreserveInMSet()`.
///
/// Ported from `r-source/src/main/memory.c:3733`.
pub unsafe fn R_PreserveInMSet(x: SEXP, mset: SEXP) {
    unsafe {
        if x == R_NilValue() || crate::mainutils::relop::isSymbol(x) != 0 {
            return;
        }

        let store = crate::sexp::accessors::CAR(mset);
        let n_ptr = crate::sexp::accessors::INTEGER(crate::sexp::accessors::CDR(mset));
        if n_ptr.is_null() {
            return;
        }
        let n = *n_ptr;

        let mut store = store;
        if store == R_NilValue() {
            let mut newsize =
                crate::sexp::accessors::INTEGER_ELT(crate::sexp::accessors::TAG(mset), 0)
                    as R_xlen_t;
            if newsize == 0 {
                newsize = 4;
            }
            store = Rf_allocVector3(SEXPTYPE::VECSXP, newsize);
            crate::sexp::accessors::SETCAR(mset, store);
        }

        let size = crate::sexp::accessors::XLENGTH(store);
        if n as R_xlen_t == size {
            let newsize = 2 * size;
            if newsize >= i32::MAX as R_xlen_t || newsize < size {
                return;
            }
            let newstore = Rf_allocVector3(SEXPTYPE::VECSXP, newsize);
            for i in 0..size {
                crate::sexp::accessors::SET_VECTOR_ELT(
                    newstore,
                    i,
                    crate::sexp::accessors::VECTOR_ELT(store, i),
                );
            }
            crate::sexp::accessors::SETCAR(mset, newstore);
            store = newstore;
        }

        crate::sexp::accessors::SET_VECTOR_ELT(store, n as R_xlen_t, x);
        *n_ptr = n + 1;
    }
}

/// Remove (one instance of) the object from the multi-set.
///
/// This is the equivalent of R's `R_ReleaseFromMSet()`.
///
/// Ported from `r-source/src/main/memory.c:3767`.
pub unsafe fn R_ReleaseFromMSet(x: SEXP, mset: SEXP) {
    unsafe {
        if x == R_NilValue() || crate::mainutils::relop::isSymbol(x) != 0 {
            return;
        }

        let store = crate::sexp::accessors::CAR(mset);
        if store == R_NilValue() {
            return;
        }

        let n_ptr = crate::sexp::accessors::INTEGER(crate::sexp::accessors::CDR(mset));
        if n_ptr.is_null() {
            return;
        }
        let n = *n_ptr;

        let mut i = (n as R_xlen_t) - 1;
        loop {
            if crate::sexp::accessors::VECTOR_ELT(store, i) == x {
                while i < (n as R_xlen_t) - 1 {
                    crate::sexp::accessors::SET_VECTOR_ELT(
                        store,
                        i,
                        crate::sexp::accessors::VECTOR_ELT(store, i + 1),
                    );
                    i += 1;
                }
                crate::sexp::accessors::SET_VECTOR_ELT(store, i, R_NilValue());
                *n_ptr = n - 1;
                return;
            }
            if i == 0 {
                break;
            }
            i -= 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Resizable vector support
// ---------------------------------------------------------------------------

/// Check if a vector is resizable.
///
/// This is the equivalent of R's `R_isResizable()`.
pub unsafe fn R_isResizable(x: SEXP) -> c_int {
    if x.is_null() {
        return 0;
    }
    unsafe { (((*x).sxpinfo.gp() & (1 << 5)) != 0) as c_int }
}

/// Get the maximum length of a resizable vector.
///
/// This is the equivalent of R's `R_maxLength()`.
pub unsafe fn R_maxLength(x: SEXP) -> R_xlen_t {
    unsafe {
        if x.is_null() {
            return 0;
        }
        LENGTH(x) as R_xlen_t
    }
}

/// Allocate a resizable vector.
///
/// This is the equivalent of R's `R_allocResizableVector()`.
pub unsafe fn R_allocResizableVector(type_: SEXPTYPE, maxlen: R_xlen_t) -> SEXP {
    unsafe { Rf_allocVector3(type_.0, maxlen) }
}

/// Duplicate a vector and make it resizable.
///
/// This is the equivalent of R's `R_duplicateAsResizable()`.
pub unsafe fn R_duplicateAsResizable(x: SEXP) -> SEXP {
    unsafe {
        let dup = crate::mainutils::duplicate::Rf_duplicate(x);
        if !dup.is_null() {
            let gp = (*dup).sxpinfo.gp() | (1 << 5);
            (*dup).sxpinfo.set_gp(gp);
        }
        dup
    }
}

/// Resize a vector.
///
/// This is the equivalent of R's `R_resizeVector()`.
pub unsafe fn R_resizeVector(x: SEXP, newlen: R_xlen_t) {
    unsafe {
        if x.is_null() || newlen == 0 {
            return;
        }
        let t = TYPEOF(x);
        let oldlen = XLENGTH(x);
        if newlen <= oldlen {
            return;
        }
        let new_vec = crate::sexp::constructors::Rf_allocVector(t, newlen as c_int);
        if new_vec.is_null() {
            return;
        }
        let elem_size = match t {
            10..=14 | 16 | 24 => 4,
            15 => 8,
            _ => return,
        };
        let src = crate::sexp::accessors::DATAPTR(x) as *const u8;
        let dst = crate::sexp::accessors::DATAPTR(new_vec) as *mut u8;
        let copy_bytes = (oldlen as usize) * (elem_size as usize);
        if !src.is_null() && !dst.is_null() {
            std::ptr::copy_nonoverlapping(src, dst, copy_bytes);
        }
    }
}

// ---------------------------------------------------------------------------
// Protection stack error handlers
// ---------------------------------------------------------------------------

/// Signal a protect stack overflow error.
///
/// This is the equivalent of R's `R_signal_protect_error()`.
pub unsafe fn R_signal_protect_error() {
    unsafe {
        crate::mainutils::errors::errorcall(
            R_NilValue(),
            b"protect stack overflow\0".as_ptr() as *const c_char,
        );
    }
}

pub unsafe fn R_signal_unprotect_error() {
    unsafe {
        crate::mainutils::errors::errorcall(
            R_NilValue(),
            b"unprotect stack underflow\0".as_ptr() as *const c_char,
        );
    }
}

// ---------------------------------------------------------------------------
// InitMemory / initStack stubs
// ---------------------------------------------------------------------------

/// Initialize R's memory subsystem.
///
/// This is the equivalent of R's `InitMemory()`.
pub unsafe fn InitMemory() {
    // Arena and GC state are owned by the active RInstance.
}

/// Reset the protection stack.
///
/// This is the equivalent of R's `initStack()`.
pub unsafe fn initStack() {
    // Arena-based allocation doesn't need PP stack sizing
}

// ---------------------------------------------------------------------------
// Memory profile
// ---------------------------------------------------------------------------

/// Return a small session-local memory profile.
///
/// This is the equivalent of R's `do_memoryprofile()`.
pub unsafe fn do_memoryprofile(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, 4);
        if result.is_null() {
            return R_NilValue();
        }
        let data = REAL(result);
        crate::sexp::instance::with_required_current_instance(|instance| {
            *data.add(0) = instance.arena.node_count() as f64;
            *data.add(1) = instance.arena.free_count() as f64;
            *data.add(2) = instance.arena.total_bytes_allocated() as f64;
            *data.add(3) = instance
                .gc_state
                .stats
                .peak_memory
                .max(instance.arena.total_bytes_allocated()) as f64;
        });
        result
    }
}

// ---------------------------------------------------------------------------
// do_* stubs for GC-related .Internal / .Primitive calls
// ---------------------------------------------------------------------------

/// gc() implementation.
///
/// This is the equivalent of R's `do_gc()`.
pub unsafe fn do_gc(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::essentials::do_gc(_call, _op, _args, _rho) }
}

/// gcinfo() implementation (stub).
///
/// This is the equivalent of R's `do_gcinfo()`.
pub unsafe fn do_gcinfo(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let old = Rf_ScalarLogical(with_memory_state(|state| state.gc_reporting));
        let i = crate::mainutils::coerce::asLogical(CAR(args));
        if i != crate::sexp::ffi::NA_LOGICAL {
            with_memory_state(|state| state.gc_reporting = i);
        }
        old
    }
}

pub unsafe fn do_gctorture(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let old = Rf_ScalarLogical(if with_memory_state(|state| state.gc_force_gap) > 0 {
            crate::sexp::ffi::TRUE
        } else {
            crate::sexp::ffi::FALSE
        });
        let gap = crate::mainutils::coerce::asLogical(CAR(args));
        R_gc_torture(if gap != 0 { 1 } else { 0 }, 0, 0);
        old
    }
}

pub unsafe fn do_gctorture2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let old = Rf_ScalarInteger(with_memory_state(|state| state.gc_force_gap));
        let gap = crate::mainutils::coerce::asInteger(CAR(args));
        let wait = crate::mainutils::coerce::asInteger(CADR(args));
        R_gc_torture(gap, wait, 0);
        old
    }
}

/// maxVSize() implementation.
///
/// This is the equivalent of R's `do_maxVSize()`.
pub unsafe fn do_maxVSize(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
pub unsafe fn do_maxNSize(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
pub unsafe fn do_regFinaliz(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
}

// ---------------------------------------------------------------------------
// Seql (string equality, encoding-aware)
// ---------------------------------------------------------------------------

/// String equality test, encoding-aware.
///
/// This is the equivalent of R's `Seql()`.
pub unsafe fn Seql(a: SEXP, b: SEXP) -> c_int {
    unsafe { crate::mainutils::relop::Seql(a, b) }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::session::RSession;

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
        session.with_protected(|| unsafe {
            R_gc();
            R_gc_lite();
            // Should not crash
        });
    }

    #[test]
    fn test_init_memory_does_not_crash() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
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
        unsafe {
            let key = crate::sexp::constructors::Rf_ScalarInteger(42);
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
    }

    #[test]
    fn test_external_ptr_null_roundtrip() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
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
        unsafe {
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
        unsafe {
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

        left.with_protected(|| unsafe {
            let left_v_limit = current_vector_heap_size().saturating_add(1_048_576);
            let left_n_limit = current_node_heap_size().saturating_add(100);
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
            assert_eq!(R_GetMaxVSize_memory(), u64::MAX);
            assert_eq!(R_GetMaxNSize_memory(), u64::MAX);
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
            assert_eq!(value, 42);
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
                    } => {
                        assert_eq!(stored_obj, obj);
                        assert_eq!(stored_fun, fun);
                    }
                    _ => panic!("expected R finalizer"),
                }
            });

            R_RunPendingFinalizers();
            assert!(with_memory_state(|state| state
                .pending_finalizers
                .is_empty()));
        });
    }

    unsafe extern "C" fn dummy_c_finalizer(_ptr: *mut c_void) {
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
        unsafe {
            // R_NilValue should be detected as NULL
            assert_eq!(Rf_isNull_memory(R_NilValue()), 1);
            assert_eq!(Rf_isNull_memory(ptr::null_mut()), 1);

            // Non-null non-NilValue should not be NULL
            // (can't easily test without a valid SEXP)
        }
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
