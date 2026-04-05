#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/alloca.c
//!
//! Stack allocation stub. In Rust, alloca is not needed because:
//! - Rust has safe stack allocation via regular variables
//! - Vec<u8> can be used for dynamic stack-like allocation
//!
//! This module provides a no-op stub for FFI compatibility only.

use std::os::raw::c_void;
use std::ptr;

/// Allocate `size` bytes of automatically reclaimed memory.
///
/// In the original C implementation, this allocated space off the run-time
/// stack so that it is automatically reclaimed upon procedure exit.
/// In Rust this is a heap allocation stub -- callers should prefer Vec<u8>
/// or stack-allocated arrays instead.
///
/// Returns a pointer to the allocated memory, or null if size is 0.
/// The caller must NOT free this pointer (in the original, it was stack-allocated).
///
/// NOTE: This is a compatibility stub. In practice, Rust code should use
/// `Vec::with_capacity(size)` or stack arrays instead.
pub unsafe fn alloca(size: usize) -> *mut c_void {
    unsafe {
        if size == 0 {
            return ptr::null_mut();
        }

        let layout = std::alloc::Layout::from_size_align(size, 16)
            .unwrap_or_else(|_| std::alloc::Layout::from_size_align(size, 1).unwrap());

        let ptr = std::alloc::alloc(layout);
        if ptr.is_null() {
            return ptr::null_mut();
        }
        ptr as *mut c_void
    }
}
