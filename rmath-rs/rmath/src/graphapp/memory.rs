#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Memory management functions for GraphApp.
//!
//! Ported from array.c - provides custom memory allocation with
//! length tracking, similar to a managed memory pool.

use std::alloc::{Layout, alloc, dealloc, realloc};
use std::os::raw::c_long;
use std::ptr;

/// Header stored before each allocated block to track its size.
#[repr(C)]
#[derive(Clone, Copy)]
struct MemHeader {
    size: c_long,
}

const HEADER_SIZE: usize = std::mem::size_of::<MemHeader>();

/// Align size to 4-byte boundary.
fn align4(size: c_long) -> c_long {
    ((size + 4) >> 2) << 2
}

/// Allocate zeroed memory of the given size.
/// Returns a pointer to the usable memory area (after the header).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memalloc(size: c_long) -> *mut u8 {
    unsafe {
        let datasize = align4(size) as usize;
        let total = HEADER_SIZE + datasize;

        let layout = match Layout::from_size_align(total, std::mem::align_of::<MemHeader>()) {
            Ok(l) => l,
            Err(_) => return ptr::null_mut(),
        };

        let block = alloc(layout);
        if block.is_null() {
            return ptr::null_mut();
        }

        // Store size in header
        let header = block as *mut MemHeader;
        (*header).size = size;

        // Zero-fill the data area
        let data = block.add(HEADER_SIZE);
        ptr::write_bytes(data, 0, datasize);

        data
    }
}

/// Reallocate memory to a new size.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memrealloc(a: *mut u8, new_size: c_long) -> *mut u8 {
    unsafe {
        if new_size <= 0 {
            if !a.is_null() {
                memfree(a);
            }
            return ptr::null_mut();
        }

        let (block, old_size) = if a.is_null() {
            (ptr::null_mut(), 0)
        } else {
            (
                a.sub(HEADER_SIZE),
                (*(*(a.sub(HEADER_SIZE)) as *const MemHeader)).size,
            )
        };

        let oldsize = if old_size > 0 {
            align4(old_size) as usize
        } else {
            0
        };
        let newsize = align4(new_size) as usize;

        if newsize != oldsize {
            let total = HEADER_SIZE + newsize;
            let layout = match Layout::from_size_align(total, std::mem::align_of::<MemHeader>()) {
                Ok(l) => l,
                Err(_) => return ptr::null_mut(),
            };

            let new_block = realloc(block, layout, newsize);
            if new_block.is_null() {
                return ptr::null_mut();
            }

            let data = new_block.add(HEADER_SIZE);
            // Zero-fill new area
            if newsize > oldsize {
                ptr::write_bytes(data.add(oldsize), 0, newsize - oldsize);
            }

            // Update header
            (*(*new_block as *mut MemHeader)).size = new_size;
            return data;
        }

        // Update header even if size didn't change
        if !block.is_null() {
            (*(*block as *mut MemHeader)).size = new_size;
        }
        a
    }
}

/// Get the length of an allocated block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memlength(a: *mut u8) -> c_long {
    unsafe {
        if a.is_null() {
            0
        } else {
            (*(*a.sub(HEADER_SIZE) as *const MemHeader)).size
        }
    }
}

/// Free a previously allocated block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memfree(a: *mut u8) {
    unsafe {
        if a.is_null() {
            return;
        }
        let block = a.sub(HEADER_SIZE);
        let size = (*(*block as *const MemHeader)).size;
        let datasize = align4(size) as usize;
        let total = HEADER_SIZE + datasize;

        if let Ok(layout) = Layout::from_size_align(total, std::mem::align_of::<MemHeader>()) {
            dealloc(block, layout);
        }
    }
}

/// Expand a block by appending extra bytes at the end.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memexpand(a: *mut u8, extra: c_long) -> *mut u8 {
    unsafe {
        if extra == 0 {
            return a;
        }

        let (block, size) = if a.is_null() {
            (ptr::null_mut(), 0)
        } else {
            (
                a.sub(HEADER_SIZE),
                (*(*a.sub(HEADER_SIZE) as *const MemHeader)).size,
            )
        };

        let oldsize = if size > 0 { align4(size) as usize } else { 0 };
        let newsize = align4(size + extra) as usize;

        if newsize != oldsize {
            let total = HEADER_SIZE + newsize;
            let layout = match Layout::from_size_align(total, std::mem::align_of::<MemHeader>()) {
                Ok(l) => l,
                Err(_) => return ptr::null_mut(),
            };

            let new_block = realloc(block, layout, newsize);
            if new_block.is_null() {
                return ptr::null_mut();
            }

            let data = new_block.add(HEADER_SIZE);
            if newsize > oldsize {
                ptr::write_bytes(data.add(oldsize), 0, newsize - oldsize);
            }

            (*(*new_block as *mut MemHeader)).size = size + extra;
            return data;
        }

        // Update header
        if !block.is_null() {
            (*(*block as *mut MemHeader)).size = size + extra;
        }
        a
    }
}

/// Join two blocks: append b to a and return the result.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memjoin(a: *mut u8, b: *mut u8) -> *mut u8 {
    unsafe {
        let size = memlength(a);
        let extra = memlength(b);
        let result = memexpand(a, extra);
        if !result.is_null() {
            if !b.is_null() {
                ptr::copy_nonoverlapping(b, result.add(size as usize), extra as usize);
            }
        }
        result
    }
}
