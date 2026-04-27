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

unsafe fn header_for_data(data: *mut u8) -> *mut MemHeader {
    unsafe { data.sub(HEADER_SIZE) as *mut MemHeader }
}

fn layout_for_data_size(size: c_long) -> Option<Layout> {
    let datasize = align4(size) as usize;
    Layout::from_size_align(HEADER_SIZE + datasize, std::mem::align_of::<MemHeader>()).ok()
}

/// Allocate zeroed memory of the given size.
/// Returns a pointer to the usable memory area (after the header).
pub unsafe fn memalloc(size: c_long) -> *mut u8 {
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
pub unsafe fn memrealloc(a: *mut u8, new_size: c_long) -> *mut u8 {
    unsafe {
        if new_size <= 0 {
            if !a.is_null() {
                memfree(a);
            }
            return ptr::null_mut();
        }

        if a.is_null() {
            return memalloc(new_size);
        }

        let block = header_for_data(a) as *mut u8;
        let old_size = (*(block as *const MemHeader)).size;
        let oldsize = if old_size > 0 {
            align4(old_size) as usize
        } else {
            0
        };
        let newsize = align4(new_size) as usize;

        if newsize != oldsize {
            let old_layout = match layout_for_data_size(old_size) {
                Some(layout) => layout,
                None => return ptr::null_mut(),
            };
            let new_total = HEADER_SIZE + newsize;
            let new_block = realloc(block, old_layout, new_total);
            if new_block.is_null() {
                return ptr::null_mut();
            }

            let data = new_block.add(HEADER_SIZE);
            if newsize > oldsize {
                ptr::write_bytes(data.add(oldsize), 0, newsize - oldsize);
            }

            (*(new_block as *mut MemHeader)).size = new_size;
            return data;
        }

        (*(block as *mut MemHeader)).size = new_size;
        a
    }
}

/// Get the length of an allocated block.
pub unsafe fn memlength(a: *mut u8) -> c_long {
    unsafe {
        if a.is_null() {
            0
        } else {
            (*header_for_data(a)).size
        }
    }
}

/// Free a previously allocated block.
pub unsafe fn memfree(a: *mut u8) {
    unsafe {
        if a.is_null() {
            return;
        }
        let header = header_for_data(a);
        let size = (*header).size;
        if let Some(layout) = layout_for_data_size(size) {
            dealloc(header as *mut u8, layout);
        }
    }
}

/// Expand a block by appending extra bytes at the end.
pub unsafe fn memexpand(a: *mut u8, extra: c_long) -> *mut u8 {
    unsafe {
        if extra == 0 {
            return a;
        }

        if a.is_null() {
            return memalloc(extra);
        }

        let block = header_for_data(a) as *mut u8;
        let size = (*(block as *const MemHeader)).size;
        let oldsize = if size > 0 { align4(size) as usize } else { 0 };
        let newsize = align4(size + extra) as usize;

        if newsize != oldsize {
            let old_layout = match layout_for_data_size(size) {
                Some(layout) => layout,
                None => return ptr::null_mut(),
            };
            let new_total = HEADER_SIZE + newsize;
            let new_block = realloc(block, old_layout, new_total);
            if new_block.is_null() {
                return ptr::null_mut();
            }

            let data = new_block.add(HEADER_SIZE);
            if newsize > oldsize {
                ptr::write_bytes(data.add(oldsize), 0, newsize - oldsize);
            }
            (*(new_block as *mut MemHeader)).size = size + extra;
            return data;
        }

        (*(block as *mut MemHeader)).size = size + extra;
        a
    }
}

/// Join two blocks: append b to a and return the result.
pub unsafe fn memjoin(a: *mut u8, b: *mut u8) -> *mut u8 {
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
