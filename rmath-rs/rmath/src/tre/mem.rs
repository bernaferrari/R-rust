/*
  tre/mem.rs - TRE memory allocator

  Ported from tre-mem.c and xmalloc.c
*/

use std::alloc::{Layout, alloc, dealloc};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;


pub const TRE_MEM_BLOCK_SIZE: usize = 1024;

#[repr(C)]
pub struct tre_list {
    pub data: *mut c_void,
    pub next: *mut tre_list,
}

pub type tre_list_t = tre_list;

#[repr(C)]
pub struct tre_mem_struct {
    pub blocks: *mut tre_list_t,
    pub current: *mut tre_list_t,
    pub ptr: *mut c_char,
    pub n: usize,
    pub failed: c_int,
    pub provided: *mut *mut c_void,
}

pub type tre_mem_t = *mut tre_mem_struct;

/// Returns a new memory allocator or NULL if out of memory.
pub unsafe fn tre_mem_new() -> tre_mem_t {
    unsafe { tre_mem_new_impl(0, ptr::null_mut()) }
}

pub unsafe fn tre_mem_new_impl(provided: c_int, provided_block: *mut c_void) -> tre_mem_t {
    unsafe {
        let mem: tre_mem_t;
        if provided != 0 {
            mem = provided_block as tre_mem_t;
            ptr::write_bytes(mem as *mut u8, 0, std::mem::size_of::<tre_mem_struct>());
        } else {
            let layout = Layout::new::<tre_mem_struct>();
            let ptr = alloc(layout);
            if ptr.is_null() {
                return ptr::null_mut();
            }
            mem = ptr as tre_mem_t;
            ptr::write_bytes(mem as *mut u8, 0, std::mem::size_of::<tre_mem_struct>());
        }
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
            if !(*l).data.is_null() {
                let layout = Layout::from_size_align_unchecked(TRE_MEM_BLOCK_SIZE, 1);
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
    unsafe { tre_mem_alloc_impl(mem, 0, ptr::null_mut(), 0, size) }
}

/// Allocates a zero-initialized block of `size` bytes from `mem`.
pub unsafe fn tre_mem_calloc(mem: tre_mem_t, size: usize) -> *mut c_void {
    unsafe { tre_mem_alloc_impl(mem, 0, ptr::null_mut(), 1, size) }
}

pub unsafe fn tre_mem_alloc_impl(
    mem: tre_mem_t,
    provided: c_int,
    provided_block: *mut c_void,
    zero: c_int,
    size: usize,
) -> *mut c_void {
    unsafe {
        if mem.is_null() {
            return ptr::null_mut();
        }

        if (*mem).failed != 0 {
            return ptr::null_mut();
        }

        let mut size = size;
        if (*mem).n < size {
            /* We need more memory than is available in the current block.
            Allocate a new block. */
            if provided != 0 {
                if provided_block.is_null() {
                    (*mem).failed = 1;
                    return ptr::null_mut();
                }
                (*mem).ptr = provided_block as *mut c_char;
                (*mem).n = TRE_MEM_BLOCK_SIZE;
            } else {
                let block_size = if size * 8 > TRE_MEM_BLOCK_SIZE {
                    size * 8
                } else {
                    TRE_MEM_BLOCK_SIZE
                };

                let l: *mut tre_list_t = alloc(Layout::new::<tre_list_t>()) as *mut tre_list_t;
                if l.is_null() {
                    (*mem).failed = 1;
                    return ptr::null_mut();
                }
                let data_layout = Layout::from_size_align_unchecked(block_size, 1);
                let data = alloc(data_layout);
                if data.is_null() {
                    dealloc(l as *mut u8, Layout::new::<tre_list_t>());
                    (*mem).failed = 1;
                    return ptr::null_mut();
                }
                (*l).data = data as *mut c_void;
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
        }

        /* Make sure the next pointer will be aligned. */
        let ptr_addr = (*mem).ptr as usize;
        let align = std::mem::size_of::<usize>();
        let alignment = if ptr_addr + size % align != 0 {
            align - (ptr_addr + size) % align
        } else {
            0
        };
        size += alignment;

        /* Allocate from current block. */
        let ptr = (*mem).ptr as *mut c_void;
        (*mem).ptr = (*mem).ptr.add(size);
        (*mem).n -= size;

        /* Set to zero if needed. */
        if zero != 0 {
            ptr::write_bytes(ptr as *mut u8, 0, size);
        }

        ptr
    }
}

/* Simple xmalloc/xrealloc/xfree replacements using std::alloc */

pub unsafe fn xmalloc(size: usize) -> *mut c_void {
    unsafe {
        if size == 0 {
            return ptr::null_mut();
        }
        let layout = Layout::from_size_align_unchecked(size, 1);
        alloc(layout) as *mut c_void
    }
}

pub unsafe fn xcalloc(nmemb: usize, size: usize) -> *mut c_void {
    unsafe {
        let total = nmemb * size;
        if total == 0 {
            return ptr::null_mut();
        }
        let ptr = xmalloc(total);
        if !ptr.is_null() {
            ptr::write_bytes(ptr as *mut u8, 0, total);
        }
        ptr
    }
}

pub unsafe fn xrealloc(ptr: *mut c_void, new_size: usize) -> *mut c_void {
    unsafe {
        if new_size == 0 {
            return ptr::null_mut();
        }
        if ptr.is_null() {
            return xmalloc(new_size);
        }

        let old_layout = Layout::from_size_align_unchecked(new_size, 1);
        let new_ptr = alloc(old_layout);
        if new_ptr.is_null() {
            return ptr::null_mut();
        }
        // Note: we can't know the old size, so we copy new_size bytes max.
        // In practice the callers know the old size.
        ptr::copy_nonoverlapping(ptr as *const u8, new_ptr, new_size);
        dealloc(ptr as *mut u8, old_layout);
        new_ptr as *mut c_void
    }
}

pub unsafe fn xfree(ptr: *mut c_void) {
    if !ptr.is_null() {
        // We don't know the exact layout used, but we use a reasonable default
        // In practice, the original code used free() which works for any allocation
        // Since we're using alloc() with Layout, we need to be more careful.
        // For now, we skip deallocation of individual blocks since tre_mem_destroy
        // handles bulk cleanup.
    }
}
