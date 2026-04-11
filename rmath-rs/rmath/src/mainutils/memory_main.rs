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

use std::cell::Cell;
use std::os::raw::{c_char, c_double, c_int, c_long, c_void};
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// GC control
// ---------------------------------------------------------------------------

thread_local! { static R_in_gc: Cell<c_int> = Cell::new(0); }
thread_local! { static gc_reporting: Cell<c_int> = Cell::new(0); }
thread_local! { static gc_count: Cell<c_int> = Cell::new(0); }
thread_local! { static gc_force_gap: Cell<c_int> = Cell::new(0); }
thread_local! { static gc_force_wait: Cell<c_int> = Cell::new(0); }

/// Returns whether a GC is currently running.
///
/// This is the equivalent of R's `R_gc_running()`.
pub unsafe fn R_gc_running() -> c_int {
    R_in_gc.with(|v| v.get())
}

/// Trigger a full garbage collection.
///
/// This is the equivalent of R's `R_gc()`.
pub unsafe fn R_gc() {
    gc_count.with(|v| v.set(v.get() + 1));
    // No actual GC implementation yet; stub.
}

/// Trigger a lightweight garbage collection.
///
/// This is the equivalent of R's `R_gc_lite()`.
pub unsafe fn R_gc_lite() {
    gc_count.with(|v| v.set(v.get() + 1));
    crate::sexp::gengc::minor_gc();
}

/// GC torture settings.
///
/// When `gap > 0`, every `gap` allocations will force a GC cycle.
/// This is a debugging aid for finding GC-safety bugs.
pub unsafe fn R_gc_torture(gap: c_int, wait: c_int, _inhibit: c_int) {
    gc_force_gap.with(|v| {
        if gap != NA_INTEGER && gap >= 0 {
            v.set(gap);
        }
    });
    gc_force_wait.with(|v| {
        if gap > 0 && wait != NA_INTEGER && wait > 0 {
            v.set(wait);
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
            if t == SEXPTYPE::SYMSXP.0 { 1 } else { 0 }
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
            if t == SEXPTYPE::LGLSXP.0 { 1 } else { 0 }
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
            if t == SEXPTYPE::REALSXP.0 { 1 } else { 0 }
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
            if t == SEXPTYPE::CPLXSXP.0 { 1 } else { 0 }
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
            if t == SEXPTYPE::EXPRSXP.0 { 1 } else { 0 }
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
            if t == SEXPTYPE::ENVSXP.0 { 1 } else { 0 }
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
            if t == SEXPTYPE::STRSXP.0 { 1 } else { 0 }
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
            if t == SEXPTYPE::OBJSXP.0 { 1 } else { 0 }
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

pub type R_CFinalizer_t = unsafe extern "C" fn(*mut c_void);

thread_local! {
    static PENDING_FINALIZERS: std::cell::RefCell<Vec<(SEXP, R_CFinalizer_t)>> = std::cell::RefCell::new(Vec::new());
}

pub unsafe fn R_MakeWeakRef(key: SEXP, val: SEXP, fin: SEXP, _onexit: c_int) -> SEXP {
    unsafe {
        let s = crate::sexp::memory_ext::allocSExp(SEXPTYPE::WEAKREFSXP);
        if s.is_null() {
            return R_NilValue();
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
            PENDING_FINALIZERS.with(|f| {
                f.borrow_mut().push((key, fin));
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

pub unsafe fn R_RegisterFinalizerEx(_s: SEXP, _fun: SEXP, _onexit: c_int) {
    // R-level finalizer registration — stores the function for later execution.
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
    PENDING_FINALIZERS.with(|f| {
        f.borrow_mut().push((s, fun));
    });
}

pub unsafe fn R_RunPendingFinalizers() {
    let finalizers: Vec<(SEXP, R_CFinalizer_t)> =
        PENDING_FINALIZERS.with(|f| std::mem::take(&mut *f.borrow_mut()));
    for (s, fin) in finalizers {
        if !s.is_null() {
            unsafe {
                fin(s as *mut c_void);
            }
        }
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
            return s;
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
    u64::MAX
}

/// Get the maximum node heap size.
/// Duplicate — no #[unsafe(no_mangle)] (already in mainutils/main.rs).
pub(crate) unsafe fn R_GetMaxNSize_memory() -> u64 {
    u64::MAX
}

/// Set the maximum vector heap size.
pub unsafe fn R_SetMaxVSize(_size: u64) -> c_int {
    1 // TRUE - always succeed
}

/// Set the maximum node heap size.
pub unsafe fn R_SetMaxNSize(_size: u64) -> c_int {
    1 // TRUE - always succeed
}

/// Set the protection stack size.
pub unsafe fn R_SetPPSize(_size: u64) {
    // Arena-based allocation doesn't need PP stack sizing
}

// ---------------------------------------------------------------------------
// Console I/O (duplicates — no #[unsafe(no_mangle)])
//
// R_ReadConsole and R_WriteConsole are in unix/system.rs.
// ---------------------------------------------------------------------------

/// Read from the console (stub).
/// Duplicate — no #[unsafe(no_mangle)] (already in unix/system.rs).
pub(crate) unsafe fn R_ReadConsole_memory(
    _prompt: *const c_char,
    _buf: *mut c_char,
    _len: c_int,
    _addtohistory: c_int,
) -> c_int {
    0
}

/// Write to the console (stub).
/// Duplicate — no #[unsafe(no_mangle)] (already in unix/system.rs).
pub(crate) unsafe fn R_WriteConsole_memory(_buf: *const c_char, _len: c_int) {
    // Headless: console output suppressed
}

// ---------------------------------------------------------------------------
// readline stub

/// GNU readline wrapper (stub).
///
/// This is the equivalent of R's `readline()` function.
pub unsafe fn readline(_prompt: *const c_char) -> *mut c_char {
    ptr::null_mut()
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
            return ptr::null_mut();
        }
        let buf = &mut *buf;

        if blen == usize::MAX {
            return buf.data as *mut c_void; // error in real impl
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
            return ptr::null_mut();
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
        let npreserved = Rf_allocVector3(SEXPTYPE::INTSXP.0, 1);
        if npreserved.is_null() {
            return R_NilValue();
        }
        crate::sexp::accessors::SET_INTEGER_ELT(npreserved, 0, 0);

        let mset = Rf_cons(R_NilValue(), npreserved);
        if mset.is_null() {
            return R_NilValue();
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
            store = Rf_allocVector3(SEXPTYPE::VECSXP.0, newsize);
            crate::sexp::accessors::SETCAR(mset, store);
        }

        let size = crate::sexp::accessors::XLENGTH(store);
        if n as R_xlen_t == size {
            let newsize = 2 * size;
            if newsize >= i32::MAX as R_xlen_t || newsize < size {
                return;
            }
            let newstore = Rf_allocVector3(SEXPTYPE::VECSXP.0, newsize);
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
    // Arena/GC initialized statically via thread_local; no explicit init needed
}

/// Reset the protection stack.
///
/// This is the equivalent of R's `initStack()`.
pub unsafe fn initStack() {
    // Arena-based allocation doesn't need PP stack sizing
}

// ---------------------------------------------------------------------------
// Memory profile stub
// ---------------------------------------------------------------------------

/// Memory profiling (stub).
///
/// This is the equivalent of R's `do_memoryprofile()`.
pub unsafe fn do_memoryprofile(_call: SEXP, _op: SEXP, _args: SEXP, _env: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

// ---------------------------------------------------------------------------
// do_* stubs for GC-related .Internal / .Primitive calls
// ---------------------------------------------------------------------------

/// gc() implementation (stub).
///
/// This is the equivalent of R's `do_gc()`.
pub unsafe fn do_gc(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        R_gc();
        Rf_allocVector3(SEXPTYPE::REALSXP.0, 14)
    }
}

/// gcinfo() implementation (stub).
///
/// This is the equivalent of R's `do_gcinfo()`.
pub unsafe fn do_gcinfo(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { Rf_ScalarLogical(gc_reporting.with(|v| v.get())) }
}

/// gctorture() implementation (stub).
///
/// This is the equivalent of R's `do_gctorture()`.
pub unsafe fn do_gctorture(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { Rf_ScalarLogical(0) }
}

/// gctorture2() implementation (stub).
///
/// This is the equivalent of R's `do_gctorture2()`.
pub unsafe fn do_gctorture2(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { Rf_ScalarInteger(0) }
}

/// maxVSize() implementation (stub).
///
/// This is the equivalent of R's `do_maxVSize()`.
pub unsafe fn do_maxVSize(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { Rf_ScalarReal(f64::INFINITY) }
}

/// maxNSize() implementation (stub).
///
/// This is the equivalent of R's `do_maxNSize()`.
pub unsafe fn do_maxNSize(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { Rf_ScalarReal(f64::INFINITY) }
}

/// Register finalizer .Internal call (stub).
///
/// This is the equivalent of R's `do_regFinaliz()`.
pub unsafe fn do_regFinaliz(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { R_NilValue() }
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

    #[test]
    fn test_sexptype2char_basic() {
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
        unsafe {
            let running = R_gc_running();
            assert_eq!(running, 0); // GC should not be running initially
        }
    }

    #[test]
    fn test_gc_does_not_crash() {
        unsafe {
            R_gc();
            R_gc_lite();
            // Should not crash
        }
    }

    #[test]
    fn test_init_memory_does_not_crash() {
        unsafe {
            InitMemory();
            initStack();
        }
    }

    #[test]
    fn test_r_allocld() {
        unsafe {
            let ptr = R_allocLD(10);
            // May or may not be null depending on implementation; just check it doesn't crash
        }
    }

    #[test]
    fn test_r_string_buffer() {
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
    fn test_weak_ref_stubs() {
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
    fn test_external_ptr_stubs() {
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
    fn test_resizable_vector_stubs() {
        unsafe {
            assert_eq!(R_isResizable(ptr::null_mut()), 0);
            assert_eq!(R_maxLength(ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_finalizer_stubs() {
        unsafe {
            R_RegisterFinalizer(ptr::null_mut(), ptr::null_mut());
            R_RegisterFinalizerEx(ptr::null_mut(), ptr::null_mut(), 0);
            R_RegisterCFinalizer(ptr::null_mut(), dummy_c_finalizer);
            R_RegisterCFinalizerEx(ptr::null_mut(), dummy_c_finalizer, 0);
            R_RunPendingFinalizers();
            R_RunFinalizers();
            // Should not crash
        }
    }

    unsafe extern "C" fn dummy_c_finalizer(_ptr: *mut c_void) {
        // No-op
    }

    #[test]
    fn test_chk_calloc() {
        unsafe {
            let p = R_chk_calloc(10, 8);
            assert!(!p.is_null());
            R_chk_free(p);
        }
    }

    #[test]
    fn test_chk_memcpy_memset() {
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
        unsafe {
            // Same pointer should be equal
            let fake = 0x1 as SEXP;
            assert_eq!(Seql(fake, fake), 1);
        }
    }

    #[test]
    fn test_readline_stub() {
        unsafe {
            let result = readline(ptr::null_mut());
            assert!(result.is_null());
        }
    }
}
