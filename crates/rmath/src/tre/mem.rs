/*
  tre/mem.rs - TRE memory allocator

  Ported from tre-mem.c and xmalloc.c
*/

use std::alloc::Layout;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

#[cfg(test)]
pub(super) mod fault;

unsafe fn alloc(layout: Layout) -> *mut u8 {
    #[cfg(test)]
    if fault::deny() {
        return ptr::null_mut();
    }
    let pointer = unsafe { std::alloc::alloc(layout) };
    #[cfg(test)]
    fault::allocated(pointer);
    pointer
}

unsafe fn dealloc(pointer: *mut u8, layout: Layout) {
    #[cfg(test)]
    fault::deallocated(pointer);
    unsafe { std::alloc::dealloc(pointer, layout) };
}

pub const TRE_MEM_BLOCK_SIZE: usize = 1024;

// Blocks contain AST nodes, pointers, and numeric matcher records. Every
// suballocation retains this alignment, including after an odd-sized request.
const TRE_ALIGNMENT: usize = 16;

#[repr(C)]
pub struct tre_list {
    pub data: *mut c_void,
    pub next: *mut tre_list,
    pub data_size: usize,
}

pub type tre_list_t = tre_list;

#[repr(C)]
pub struct tre_mem_struct {
    pub blocks: *mut tre_list_t,
    pub current: *mut tre_list_t,
    pub ptr: *mut c_char,
    pub n: usize,
    pub failed: c_int,
}

pub type tre_mem_t = *mut tre_mem_struct;

/// Returns a new memory allocator or NULL if out of memory.
pub unsafe fn tre_mem_new() -> tre_mem_t {
    unsafe {
        let layout = Layout::new::<tre_mem_struct>();
        let allocation = alloc(layout);
        if allocation.is_null() {
            return ptr::null_mut();
        }
        let mem = allocation.cast::<tre_mem_struct>();
        ptr::write_bytes(mem, 0, 1);
        mem
    }
}

/// Frees the memory allocator and all memory allocated with it.
pub unsafe fn tre_mem_destroy(mem: tre_mem_t) {
    unsafe {
        if mem.is_null() {
            return;
        }
        let mut l = (*mem).blocks;
        while !l.is_null() {
            let tmp = (*l).next;
            if !(*l).data.is_null() && (*l).data_size > 0 {
                let layout = Layout::from_size_align_unchecked((*l).data_size, TRE_ALIGNMENT);
                dealloc((*l).data as *mut u8, layout);
            }
            let list_layout = Layout::new::<tre_list_t>();
            dealloc(l as *mut u8, list_layout);
            l = tmp;
        }
        let mem_layout = Layout::new::<tre_mem_struct>();
        dealloc(mem as *mut u8, mem_layout);
    }
}

/// Allocates a block of `size` bytes from `mem`.
pub unsafe fn tre_mem_alloc(mem: tre_mem_t, size: usize) -> *mut c_void {
    unsafe { tre_mem_alloc_impl(mem, false, size) }
}

/// Allocates a zero-initialized block of `size` bytes from `mem`.
pub unsafe fn tre_mem_calloc(mem: tre_mem_t, size: usize) -> *mut c_void {
    unsafe { tre_mem_alloc_impl(mem, true, size) }
}

unsafe fn tre_mem_alloc_impl(mem: tre_mem_t, zero: bool, size: usize) -> *mut c_void {
    unsafe {
        if mem.is_null() {
            return ptr::null_mut();
        }

        if (*mem).failed != 0 {
            return ptr::null_mut();
        }

        let Some(size) = size
            .max(1)
            .checked_add(TRE_ALIGNMENT - 1)
            .map(|size| size & !(TRE_ALIGNMENT - 1))
        else {
            (*mem).failed = 1;
            return ptr::null_mut();
        };
        if (*mem).n < size {
            /* We need more memory than is available in the current block.
            Allocate a new block. */
            let Some(data_layout) = size.checked_mul(8).and_then(|bytes| {
                Layout::from_size_align(bytes.max(TRE_MEM_BLOCK_SIZE), TRE_ALIGNMENT).ok()
            }) else {
                (*mem).failed = 1;
                return ptr::null_mut();
            };
            let block_size = data_layout.size();

            let l: *mut tre_list_t = alloc(Layout::new::<tre_list_t>()) as *mut tre_list_t;
            if l.is_null() {
                (*mem).failed = 1;
                return ptr::null_mut();
            }
            let data = alloc(data_layout);
            if data.is_null() {
                dealloc(l as *mut u8, Layout::new::<tre_list_t>());
                (*mem).failed = 1;
                return ptr::null_mut();
            }
            (*l).data = data as *mut c_void;
            (*l).data_size = block_size;
            (*l).next = ptr::null_mut();
            if !(*mem).current.is_null() {
                (*(*mem).current).next = l;
            }
            if (*mem).blocks.is_null() {
                (*mem).blocks = l;
            }
            (*mem).current = l;
            (*mem).ptr = data as *mut c_char;
            (*mem).n = block_size;
        }

        /* Allocate from current block. */
        let ptr = (*mem).ptr as *mut c_void;
        (*mem).ptr = (*mem).ptr.add(size);
        (*mem).n -= size;

        /* Set to zero if needed. */
        if zero {
            ptr::write_bytes(ptr as *mut u8, 0, size);
        }

        ptr
    }
}

/* Simple xmalloc/xrealloc/xfree replacements using std::alloc.
Since xfree doesn't receive the allocation size, we prepend an
aligned header that stores it.  No libc dependency required. */

#[repr(C, align(16))]
struct AllocationHeader {
    size: usize,
}

/// Size of the hidden header prepended to each xmalloc allocation.
const XALLOC_HEADER: usize = std::mem::size_of::<AllocationHeader>();

fn allocation_layout(size: usize) -> Option<Layout> {
    Layout::from_size_align(XALLOC_HEADER.checked_add(size)?, TRE_ALIGNMENT).ok()
}

pub unsafe fn xmalloc(size: usize) -> *mut c_void {
    unsafe {
        if size == 0 {
            return ptr::null_mut();
        }
        let Some(layout) = allocation_layout(size) else {
            return ptr::null_mut();
        };
        let raw = alloc(layout);
        if raw.is_null() {
            return ptr::null_mut();
        }
        raw.cast::<AllocationHeader>()
            .write(AllocationHeader { size });
        raw.add(XALLOC_HEADER) as *mut c_void
    }
}

pub unsafe fn xcalloc(nmemb: usize, size: usize) -> *mut c_void {
    unsafe {
        if nmemb == 0 || size == 0 {
            return ptr::null_mut();
        }
        let Some(total_data) = nmemb.checked_mul(size) else {
            return ptr::null_mut();
        };
        let p = xmalloc(total_data);
        if !p.is_null() {
            ptr::write_bytes(p as *mut u8, 0, total_data);
        }
        p
    }
}

pub unsafe fn xrealloc(ptr: *mut c_void, new_size: usize) -> *mut c_void {
    unsafe {
        if new_size == 0 {
            xfree(ptr);
            return ptr::null_mut();
        }
        if ptr.is_null() {
            return xmalloc(new_size);
        }
        let raw = (ptr as *mut u8).sub(XALLOC_HEADER);
        let old_size = (*raw.cast::<AllocationHeader>()).size;
        let old_layout = allocation_layout(old_size).expect("original TRE allocation layout");
        let Some(new_layout) = allocation_layout(new_size) else {
            return ptr::null_mut();
        };
        #[cfg(test)]
        if fault::deny() {
            return ptr::null_mut();
        }
        let new_raw = std::alloc::realloc(raw, old_layout, new_layout.size());
        if new_raw.is_null() {
            return ptr::null_mut();
        }
        #[cfg(test)]
        {
            fault::deallocated(raw);
            fault::allocated(new_raw);
        }
        new_raw
            .cast::<AllocationHeader>()
            .write(AllocationHeader { size: new_size });
        new_raw.add(XALLOC_HEADER) as *mut c_void
    }
}

pub unsafe fn xfree(ptr: *mut c_void) {
    unsafe {
        if !ptr.is_null() {
            let raw = (ptr as *mut u8).sub(XALLOC_HEADER);
            let size = (*raw.cast::<AllocationHeader>()).size;
            let layout = allocation_layout(size).expect("original TRE allocation layout");
            dealloc(raw, layout);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_odd_requests_preserve_typed_alignment_across_blocks() {
        unsafe {
            let arena = tre_mem_new();
            assert!(!arena.is_null());
            assert!(!tre_mem_alloc(arena, 0).is_null());
            for value in 0..100_u128 {
                let odd = tre_mem_alloc(arena, 1).cast::<u8>();
                assert!(!odd.is_null());
                odd.write(7);
                let typed = tre_mem_calloc(arena, std::mem::size_of::<u128>()).cast::<u128>();
                assert!(!typed.is_null());
                assert!(typed.is_aligned());
                assert_eq!(typed.read(), 0);
                typed.write(value);
                assert_eq!(typed.read(), value);
                assert_eq!(odd.read(), 7);
            }
            tre_mem_destroy(arena);
        }
    }

    #[test]
    fn aligned_realloc_preserves_values_and_rejects_overflow() {
        unsafe {
            let allocation = xmalloc(std::mem::size_of::<u128>()).cast::<u128>();
            assert!(!allocation.is_null());
            assert!(allocation.is_aligned());
            allocation.write(u128::MAX - 1);
            let grown = xrealloc(allocation.cast(), 2 * std::mem::size_of::<u128>()).cast::<u128>();
            assert!(!grown.is_null());
            assert!(grown.is_aligned());
            assert_eq!(grown.read(), u128::MAX - 1);
            assert!(xrealloc(grown.cast(), usize::MAX).is_null());
            assert_eq!(
                grown.read(),
                u128::MAX - 1,
                "failed resize retains original allocation"
            );
            assert!(xrealloc(grown.cast(), 0).is_null());
            assert!(xmalloc(usize::MAX).is_null());
            assert!(xcalloc(usize::MAX, 2).is_null());
            assert!(xmalloc(0).is_null());
            let zero = xcalloc(2, std::mem::size_of::<u128>()).cast::<u128>();
            assert!(!zero.is_null());
            assert!(zero.is_aligned());
            assert_eq!(zero.read(), 0);
            assert_eq!(zero.add(1).read(), 0);
            xfree(zero.cast());
        }
    }

    #[test]
    fn arena_size_overflow_fails_without_corrupting_prior_storage() {
        unsafe {
            let arena = tre_mem_new();
            assert!(!arena.is_null());
            let value = tre_mem_alloc(arena, std::mem::size_of::<u64>()).cast::<u64>();
            assert!(!value.is_null());
            value.write(42);
            assert!(tre_mem_alloc(arena, usize::MAX).is_null());
            assert!(tre_mem_alloc(arena, 1).is_null());
            assert_eq!(value.read(), 42);
            tre_mem_destroy(arena);
        }
    }
}
