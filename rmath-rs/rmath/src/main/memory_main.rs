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
//! unix/system.rs, unix/embedded.rs, and main/main.rs are provided
//! as pub(crate) functions WITHOUT #[unsafe(no_mangle)] to avoid duplicate symbol errors.

use std::cell::Cell;
use std::os::raw::{c_char, c_double, c_int, c_long, c_void};
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;

// ---------------------------------------------------------------------------
// GC control
// ---------------------------------------------------------------------------

thread_local! { static R_in_gc: Cell<c_int> = Cell::new(0); }
thread_local! { static gc_reporting: Cell<c_int> = Cell::new(0); }
thread_local! { static gc_count: Cell<c_int> = Cell::new(0); }

/// Returns whether a GC is currently running.
///
/// This is the equivalent of R's `R_gc_running()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_gc_running() -> c_int {
    unsafe { R_in_gc.with(|v| v.get()) }
}

/// Trigger a full garbage collection.
///
/// This is the equivalent of R's `R_gc()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_gc() {
    unsafe {
        gc_count.with(|v| v.set(v.get() + 1));
        // No actual GC implementation yet; stub.
    }
}

/// Trigger a lightweight garbage collection.
///
/// This is the equivalent of R's `R_gc_lite()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_gc_lite() {
    unsafe {
        gc_count.with(|v| v.set(v.get() + 1));
        // No actual GC implementation yet; stub.
    }
}

/// GC torture settings (no-op).
///
/// This is the equivalent of R's `R_gc_torture()`.
pub unsafe fn R_gc_torture(_gap: c_int, _wait: c_int, _inhibit: c_int) {
    // No-op stub.
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
            // In a full implementation this would call error()
            // For now we just return null
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
            // In a full implementation this would call error()
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
// Rf_isNull is in sexp/accessors.rs and main/print.rs.
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

/// Permanently protect an SEXP from garbage collection.
pub(crate) unsafe fn R_PreserveObject_memory(s: SEXP) {
    unsafe {
        crate::sexp::protect::R_PreserveObject(s);
    }
}

/// Release a previously preserved object.
pub(crate) unsafe fn R_ReleaseObject_memory(s: SEXP) {
    unsafe {
        crate::sexp::protect::R_ReleaseObject(s);
    }
}

// ---------------------------------------------------------------------------
// Weak references and finalizers
// ---------------------------------------------------------------------------

/// Type for C finalizers.
pub type R_CFinalizer_t = unsafe extern "C" fn(*mut c_void);

/// Create a weak reference.
///
/// This is the equivalent of R's `R_MakeWeakRef()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_MakeWeakRef(_key: SEXP, _val: SEXP, _fin: SEXP, _onexit: c_int) -> SEXP {
    unsafe {
        // Stub: return R_NilValue since we don't have full WEAKREFSXP support yet.
        R_NilValue()
    }
}

/// Create a weak reference with a C finalizer.
///
/// This is the equivalent of R's `R_MakeWeakRefC()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_MakeWeakRefC(
    _key: SEXP,
    _val: SEXP,
    _fin: R_CFinalizer_t,
    _onexit: c_int,
) -> SEXP {
    unsafe { R_NilValue() }
}

/// Get the key of a weak reference.
///
/// This is the equivalent of R's `R_WeakRefKey()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_WeakRefKey(_w: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

/// Get the value of a weak reference.
///
/// This is the equivalent of R's `R_WeakRefValue()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_WeakRefValue(_w: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

/// Run the finalizer for a weak reference.
///
/// This is the equivalent of R's `R_RunWeakRefFinalizer()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_RunWeakRefFinalizer(_w: SEXP) {
    // No-op stub.
}

/// Register a finalizer for an object.
///
/// This is the equivalent of R's `R_RegisterFinalizer()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_RegisterFinalizer(_s: SEXP, _fun: SEXP) {
    // No-op stub.
}

/// Register a finalizer for an object with exit control.
///
/// This is the equivalent of R's `R_RegisterFinalizerEx()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_RegisterFinalizerEx(_s: SEXP, _fun: SEXP, _onexit: c_int) {
    // No-op stub.
}

/// Register a C finalizer for an object.
///
/// This is the equivalent of R's `R_RegisterCFinalizer()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_RegisterCFinalizer(_s: SEXP, _fun: R_CFinalizer_t) {
    // No-op stub.
}

/// Register a C finalizer for an object with exit control.
///
/// This is the equivalent of R's `R_RegisterCFinalizerEx()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_RegisterCFinalizerEx(_s: SEXP, _fun: R_CFinalizer_t, _onexit: c_int) {
    // No-op stub.
}

/// Run any pending finalizers.
///
/// This is the equivalent of R's `R_RunPendingFinalizers()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_RunPendingFinalizers() {
    // No-op stub.
}

/// Run all finalizers (called during exit).
///
/// This is the equivalent of R's `R_RunFinalizers()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_RunFinalizers() {
    // No-op stub.
}

/// Run exit finalizers (called during R cleanup).
/// Duplicate — no #[unsafe(no_mangle)] (already in unix/embedded.rs as unsafe fn).
pub(crate) unsafe fn R_RunExitFinalizers_memory() {
    // No-op stub.
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_MakeExternalPtr(p: *mut c_void, tag: SEXP, prot: SEXP) -> SEXP {
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ExternalPtrAddr(s: SEXP) -> *mut c_void {
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ExternalPtrTag(s: SEXP) -> SEXP {
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ExternalPtrProtected(s: SEXP) -> SEXP {
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ClearExternalPtr(s: SEXP) {
    unsafe {
        if !s.is_null() {
            (*s).data.extptr[0] = ptr::null_mut();
        }
    }
}

/// Set the address stored in an external pointer.
///
/// This is the equivalent of R's `R_SetExternalPtrAddr()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SetExternalPtrAddr(s: SEXP, p: *mut c_void) {
    unsafe {
        if !s.is_null() {
            (*s).data.extptr[0] = p;
        }
    }
}

/// Set the tag of an external pointer.
///
/// This is the equivalent of R's `R_SetExternalPtrTag()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SetExternalPtrTag(s: SEXP, tag: SEXP) {
    unsafe {
        if !s.is_null() {
            (*s).data.extptr[2] = tag as *mut c_void;
        }
    }
}

/// Set the protected value of an external pointer.
///
/// This is the equivalent of R's `R_SetExternalPtrProtected()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SetExternalPtrProtected(s: SEXP, p: SEXP) {
    unsafe {
        if !s.is_null() {
            (*s).data.extptr[1] = p as *mut c_void;
        }
    }
}

// ---------------------------------------------------------------------------
// Heap size limits (duplicates — no #[unsafe(no_mangle)])
//
// R_GetMaxVSize and R_GetMaxNSize are in main/main.rs.
// ---------------------------------------------------------------------------

/// Get the maximum vector heap size.
/// Duplicate — no #[unsafe(no_mangle)] (already in main/main.rs).
pub(crate) unsafe fn R_GetMaxVSize_memory() -> u64 {
    u64::MAX
}

/// Get the maximum node heap size.
/// Duplicate — no #[unsafe(no_mangle)] (already in main/main.rs).
pub(crate) unsafe fn R_GetMaxNSize_memory() -> u64 {
    u64::MAX
}

/// Set the maximum vector heap size.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_SetMaxVSize(_size: u64) -> c_int {
    1 // TRUE - always succeed
}

/// Set the maximum node heap size.
// no_mangle removed (duplicate)
pub unsafe extern "C" fn R_SetMaxNSize(_size: u64) -> c_int {
    1 // TRUE - always succeed
}

/// Set the protection stack size.
// no_mangle removed (duplicate)
pub unsafe extern "C" fn R_SetPPSize(_size: u64) {
    // No-op stub.
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
    // No-op stub.
}

// ---------------------------------------------------------------------------
// readline stub
// ---------------------------------------------------------------------------

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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_AllocStringBuffer(blen: usize, buf: *mut R_StringBuffer) -> *mut c_void {
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_FreeStringBuffer(buf: *mut R_StringBuffer) {
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_FreeStringBufferL(buf: *mut R_StringBuffer) {
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_NewPreciousMSet(initialSize: c_int) -> SEXP {
    unsafe {
        let npreserved = Rf_allocVector3(SEXPTYPE::INTSXP.0, 1);
        if npreserved.is_null() {
            return R_NilValue();
        }
        let mset = Rf_cons(R_NilValue(), npreserved);
        if mset.is_null() {
            return R_NilValue();
        }
        let isize = Rf_ScalarInteger(if initialSize < 0 { 0 } else { initialSize });
        // SET_TAG(mset, isize)
        mset
    }
}

/// Add an object to a multi-set.
///
/// This is the equivalent of R's `R_PreserveInMSet()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_PreserveInMSet(_x: SEXP, _mset: SEXP) {
    // No-op stub.
}

/// Remove an object from a multi-set.
///
/// This is the equivalent of R's `R_ReleaseFromMSet()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ReleaseFromMSet(_x: SEXP, _mset: SEXP) {
    // No-op stub.
}

// ---------------------------------------------------------------------------
// Resizable vector support
// ---------------------------------------------------------------------------

/// Check if a vector is resizable.
///
/// This is the equivalent of R's `R_isResizable()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_isResizable(_x: SEXP) -> c_int {
    0 // FALSE - stub
}

/// Get the maximum length of a resizable vector.
///
/// This is the equivalent of R's `R_maxLength()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_maxLength(x: SEXP) -> R_xlen_t {
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_allocResizableVector(type_: SEXPTYPE, maxlen: R_xlen_t) -> SEXP {
    unsafe { Rf_allocVector3(type_.0, maxlen) }
}

/// Duplicate a vector and make it resizable.
///
/// This is the equivalent of R's `R_duplicateAsResizable()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_duplicateAsResizable(_x: SEXP) -> SEXP {
    unsafe {
        R_NilValue() // stub
    }
}

/// Resize a vector.
///
/// This is the equivalent of R's `R_resizeVector()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_resizeVector(_x: SEXP, _newlen: R_xlen_t) {
    // No-op stub.
}

// ---------------------------------------------------------------------------
// Protection stack error handlers
// ---------------------------------------------------------------------------

/// Signal a protect stack overflow error.
///
/// This is the equivalent of R's `R_signal_protect_error()`.
pub unsafe fn R_signal_protect_error() {
    // No-op stub; in a full implementation this would call error().
}

/// Signal an unprotect error.
///
/// This is the equivalent of R's `R_signal_unprotect_error()`.
pub unsafe fn R_signal_unprotect_error() {
    // No-op stub.
}

// ---------------------------------------------------------------------------
// InitMemory / initStack stubs
// ---------------------------------------------------------------------------

/// Initialize R's memory subsystem.
///
/// This is the equivalent of R's `InitMemory()`.
pub unsafe fn InitMemory() {
    // No-op stub. The real implementation sets up GC heaps, R_NilValue, etc.
}

/// Reset the protection stack.
///
/// This is the equivalent of R's `initStack()`.
pub unsafe fn initStack() {
    // No-op stub.
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
    if a == b {
        return 1;
    }
    0 // Stub: in the full implementation this would check encoding-aware equality.
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
                    .unwrap(),
                "NILSXP"
            );
            assert_eq!(
                std::ffi::CStr::from_ptr(sexptype2char(SEXPTYPE::REALSXP))
                    .to_str()
                    .unwrap(),
                "REALSXP"
            );
            assert_eq!(
                std::ffi::CStr::from_ptr(sexptype2char(SEXPTYPE::STRSXP))
                    .to_str()
                    .unwrap(),
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
            let w = R_MakeWeakRef(ptr::null_mut(), ptr::null_mut(), ptr::null_mut(), 0);
            assert_eq!(w, R_NilValue());

            let key = R_WeakRefKey(w);
            assert_eq!(key, R_NilValue());

            let val = R_WeakRefValue(w);
            assert_eq!(val, R_NilValue());
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
