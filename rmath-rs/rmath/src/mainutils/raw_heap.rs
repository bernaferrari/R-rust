//! C-shaped malloc/calloc/realloc/free over the Rust global allocator.
//!
//! A few ported C APIs hand out raw `*mut c_void` blocks whose size the
//! caller never tracks (allocation and free live in different functions),
//! so each block carries a one-word header recording its size; `realloc`
//! and `free` read it back to rebuild the matching `Layout`. This mirrors
//! the allocator policy of the wasm32 `libc` facade (see `wasm-libc`).
//!
//! # Safety contract
//!
//! Pointers passed to `realloc`/`free` MUST originate from `malloc` or
//! `calloc` in this module — never from libc, `Box`, `Vec`, or foreign code.

use std::alloc::{Layout, alloc, dealloc};
use std::ffi::c_void;
use std::ptr;

/// malloc-compatible alignment (`max_align_t` on the supported targets).
const ALIGN: usize = 16;

/// Bytes of size-tracking header preceding each user block.
const HEADER: usize = std::mem::size_of::<usize>();

fn layout_for(total: usize) -> Option<Layout> {
    Layout::from_size_align(total, ALIGN).ok()
}

unsafe fn alloc_block(size: usize) -> *mut c_void {
    unsafe {
        // C's malloc(0) returns a unique freeable pointer; mirror that.
        let size = size.max(1);
        let Some(layout) = layout_for(size + HEADER) else {
            return ptr::null_mut();
        };
        let base = alloc(layout);
        if base.is_null() {
            return ptr::null_mut();
        }
        (base as *mut usize).write(size);
        base.add(HEADER) as *mut c_void
    }
}

unsafe fn block_size(p: *mut c_void) -> usize {
    unsafe { *(p.sub(HEADER) as *const usize) }
}

/// `malloc` equivalent: `size` bytes, null on failure.
pub(crate) unsafe fn malloc(size: usize) -> *mut c_void {
    unsafe { alloc_block(size) }
}

/// `calloc` equivalent: zeroed `nmemb * size` bytes, null on failure or
/// multiplication overflow.
pub(crate) unsafe fn calloc(nmemb: usize, size: usize) -> *mut c_void {
    unsafe {
        let Some(total) = nmemb.checked_mul(size) else {
            return ptr::null_mut();
        };
        let p = alloc_block(total);
        if !p.is_null() {
            ptr::write_bytes(p as *mut u8, 0, total);
        }
        p
    }
}

/// `realloc` equivalent. Shrinking keeps the block (documented choice,
/// identical to the wasm32 facade); a failed grow leaves the original
/// block untouched and returns null.
pub(crate) unsafe fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    unsafe {
        if ptr.is_null() {
            return malloc(size);
        }
        if size == 0 {
            free(ptr);
            return ptr::null_mut();
        }
        let old = block_size(ptr);
        if old >= size {
            return ptr;
        }
        let new = malloc(size);
        if new.is_null() {
            return ptr::null_mut();
        }
        ptr::copy_nonoverlapping(ptr as *const u8, new as *mut u8, old);
        free(ptr);
        new
    }
}

/// `free` equivalent; null-safe.
pub(crate) unsafe fn free(ptr: *mut c_void) {
    unsafe {
        if ptr.is_null() {
            return;
        }
        let size = block_size(ptr);
        let Some(layout) = layout_for(size + HEADER) else {
            return;
        };
        dealloc(ptr.sub(HEADER) as *mut u8, layout);
    }
}
