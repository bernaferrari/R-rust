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
            if !(*l).data.is_null() && (*l).data_size > 0 {
                let layout = Layout::from_size_align_unchecked((*l).data_size, 1);
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
        /* Account for worst-case alignment padding in the space check. */
        let align = std::mem::size_of::<usize>();
        if (*mem).n < size + align {
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
                let block_size = if (size + align) * 8 > TRE_MEM_BLOCK_SIZE {
                    (size + align) * 8
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
        }

        /* Make sure the next pointer will be aligned. */
        let ptr_addr = (*mem).ptr as usize;
        let end_addr = ptr_addr + size;
        let alignment = if end_addr % align != 0 {
            align - (end_addr % align)
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

/* Simple xmalloc/xrealloc/xfree replacements using std::alloc.
Since xfree doesn't receive the allocation size, we prepend a
usize header that stores it.  No libc dependency required. */

/// Size of the hidden header prepended to each xmalloc allocation.
const XALLOC_HEADER: usize = std::mem::size_of::<usize>();

pub unsafe fn xmalloc(size: usize) -> *mut c_void {
    unsafe {
        if size == 0 {
            return ptr::null_mut();
        }
        let total = XALLOC_HEADER + size;
        let layout = Layout::from_size_align_unchecked(total, XALLOC_HEADER);
        let raw = alloc(layout);
        if raw.is_null() {
            return ptr::null_mut();
        }
        *(raw as *mut usize) = size;
        raw.add(XALLOC_HEADER) as *mut c_void
    }
}

pub unsafe fn xcalloc(nmemb: usize, size: usize) -> *mut c_void {
    unsafe {
        if nmemb == 0 || size == 0 {
            return ptr::null_mut();
        }
        let total_data = nmemb * size;
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
        let old_size = *(raw as *mut usize);
        let old_total = XALLOC_HEADER + old_size;
        let old_layout = Layout::from_size_align_unchecked(old_total, XALLOC_HEADER);
        let new_total = XALLOC_HEADER + new_size;
        let new_raw = std::alloc::realloc(raw, old_layout, new_total);
        if new_raw.is_null() {
            return ptr::null_mut();
        }
        *(new_raw as *mut usize) = new_size;
        new_raw.add(XALLOC_HEADER) as *mut c_void
    }
}

pub unsafe fn xfree(ptr: *mut c_void) {
    unsafe {
        if !ptr.is_null() {
            let raw = (ptr as *mut u8).sub(XALLOC_HEADER);
            let size = *(raw as *mut usize);
            let total = XALLOC_HEADER + size;
            let layout = Layout::from_size_align_unchecked(total, XALLOC_HEADER);
            dealloc(raw, layout);
        }
    }
}
