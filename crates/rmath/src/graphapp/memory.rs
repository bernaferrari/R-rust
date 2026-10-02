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
    // C long is only 32 bits on Win64; payloads contain 64-bit pointers.
    _alignment: [usize; 0],
}

const HEADER_SIZE: usize = std::mem::size_of::<MemHeader>();

unsafe fn header_for_data(data: *mut u8) -> *mut MemHeader {
    unsafe { data.sub(HEADER_SIZE) as *mut MemHeader }
}

fn layout_for_data_size(size: c_long) -> Option<Layout> {
    let size = usize::try_from(size).ok()?;
    // Preserve GraphApp's extra word of padding, with checked arithmetic.
    let datasize = size.checked_add(4)? & !3;
    Layout::from_size_align(
        HEADER_SIZE.checked_add(datasize)?,
        std::mem::align_of::<MemHeader>(),
    )
    .ok()
}

/// Allocate a platform-sized byte count without truncating it to C long.
pub unsafe fn memalloc_bytes(size: usize) -> *mut u8 {
    let Ok(size) = c_long::try_from(size) else {
        return ptr::null_mut();
    };
    unsafe { memalloc(size) }
}

/// Allocate zeroed memory of the given size.
/// Returns a pointer to the usable memory area (after the header).
pub unsafe fn memalloc(size: c_long) -> *mut u8 {
    unsafe {
        let Some(layout) = layout_for_data_size(size) else {
            return ptr::null_mut();
        };
        let datasize = layout.size() - HEADER_SIZE;

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
        let Some(old_layout) = layout_for_data_size(old_size) else {
            return ptr::null_mut();
        };
        let Some(new_layout) = layout_for_data_size(new_size) else {
            return ptr::null_mut();
        };
        let oldsize = old_layout.size() - HEADER_SIZE;
        let newsize = new_layout.size() - HEADER_SIZE;

        if newsize != oldsize {
            let new_total = new_layout.size();
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

        if extra < 0 {
            return ptr::null_mut();
        }
        let size = memlength(a);
        let Some(new_size) = size.checked_add(extra) else {
            return ptr::null_mut();
        };
        memrealloc(a, new_size)
    }
}

/// Join two blocks: append b to a and return the result.
pub unsafe fn memjoin(a: *mut u8, b: *mut u8) -> *mut u8 {
    unsafe {
        let size = memlength(a);
        let extra = memlength(b);
        let result = memexpand(a, extra);
        if !result.is_null() && !b.is_null() {
            let source = if a == b { result } else { b };
            ptr::copy_nonoverlapping(source, result.add(size as usize), extra as usize);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphapp_memory_aligns_object_payloads_after_allocation_and_growth() {
        use super::super::types::{ObjInfo, drawstruct, imagedata};
        let required_alignment = std::mem::align_of::<ObjInfo>()
            .max(std::mem::align_of::<drawstruct>())
            .max(std::mem::align_of::<imagedata>());
        assert!(std::mem::align_of::<MemHeader>() >= required_alignment);
        assert_eq!(HEADER_SIZE % required_alignment, 0);
        unsafe {
            for size in [0, 1, 8, 128] {
                let data = memalloc(size);
                assert!(!data.is_null());
                assert_eq!(data.addr() % required_alignment, 0);
                let grown = memrealloc(data, 256);
                assert!(!grown.is_null());
                assert_eq!(grown.addr() % required_alignment, 0);
                memfree(grown);
            }
        }
    }

    #[test]
    fn graphapp_memory_rejects_bad_sizes_without_losing_existing_storage() {
        unsafe {
            assert!(memalloc(-1).is_null());
            assert!(memalloc(c_long::MAX).is_null());
            let data = memalloc(8);
            assert!(!data.is_null());
            *data = 42;
            assert!(memrealloc(data, c_long::MAX).is_null());
            assert!(memexpand(data, c_long::MAX).is_null());
            assert!(memexpand(data, -1).is_null());
            assert_eq!(memlength(data), 8);
            assert_eq!(*data, 42);
            memfree(data);
        }
    }

    #[test]
    fn graphapp_memory_growth_preserves_bytes_and_zeroes_new_storage() {
        unsafe {
            let data = memalloc(8);
            assert!(!data.is_null());
            std::ptr::write_bytes(data, 42, 8);
            let data = memexpand(data, 8);
            assert!(!data.is_null());
            assert_eq!(memlength(data), 16);
            assert_eq!(std::slice::from_raw_parts(data, 8), &[42; 8]);
            assert_eq!(std::slice::from_raw_parts(data.add(8), 8), &[0; 8]);
            memfree(data);
        }
    }

    #[test]
    fn graphapp_memory_self_join_survives_reallocation() {
        unsafe {
            let data = memalloc(8);
            assert!(!data.is_null());
            std::ptr::write_bytes(data, 42, 8);
            let joined = memjoin(data, data);
            assert!(!joined.is_null());
            assert_eq!(memlength(joined), 16);
            assert_eq!(std::slice::from_raw_parts(joined, 16), &[42; 16]);
            memfree(joined);
        }
    }
}
