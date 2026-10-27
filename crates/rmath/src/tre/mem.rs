/*
  tre/mem.rs - TRE memory allocator

  Ported from tre-mem.c and xmalloc.c
*/

use std::alloc::Layout;
use std::os::raw::{c_int, c_void};
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

pub struct tre_mem_struct {
    storage: super::owned_arena::OwnedArena,
    pub failed: c_int,
}

pub type tre_mem_t = *mut tre_mem_struct;

impl tre_mem_struct {
    pub(super) fn new(workspace: crate::mainutils::grep::RegexWorkspace) -> Result<Self, ()> {
        Ok(Self {
            storage: super::owned_arena::OwnedArena::new(workspace)?,
            failed: 0,
        })
    }
    pub(super) fn clear(&mut self) {
        self.storage.clear();
    }
}

/// Legacy standalone ownership; runtime compilation keeps this value on stack.
pub unsafe fn tre_mem_new() -> tre_mem_t {
    unsafe {
        let allocation = alloc(Layout::new::<tre_mem_struct>()).cast::<tre_mem_struct>();
        if allocation.is_null() {
            return ptr::null_mut();
        }
        let value = tre_mem_struct::new(crate::mainutils::grep::RegexWorkspace::unbounded())
            .expect("unbounded empty TRE arena");
        allocation.write(value);
        allocation
    }
}

pub unsafe fn tre_mem_destroy(mem: tre_mem_t) {
    unsafe {
        if !mem.is_null() {
            ptr::drop_in_place(mem);
            dealloc(mem.cast(), Layout::new::<tre_mem_struct>());
        }
    }
}

pub unsafe fn tre_mem_alloc(mem: tre_mem_t, size: usize) -> *mut c_void {
    unsafe {
        if mem.is_null() || (*mem).failed != 0 {
            return ptr::null_mut();
        }
        match (*mem).storage.allocate(size) {
            Ok(pointer) => pointer,
            Err(()) => {
                (*mem).failed = 1;
                ptr::null_mut()
            }
        }
    }
}

/// Fresh safe-owned cells are initialized to zero before any pointer publication.
pub unsafe fn tre_mem_calloc(mem: tre_mem_t, size: usize) -> *mut c_void {
    unsafe { tre_mem_alloc(mem, size) }
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
    #[test]
    fn owned_compile_arena_keeps_initialized_aligned_handles_across_directory_growth() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let baseline = owner.with_arena(|arena| arena.workspace_bytes()).unwrap();
        let mut arena = tre_mem_struct::new(crate::mainutils::grep::regex_workspace()).unwrap();
        unsafe {
            let first = tre_mem_calloc(&mut arena, 48).cast::<u128>();
            assert!(!first.is_null());
            assert!(first.is_aligned());
            for index in 0..3 {
                assert_eq!(first.add(index).read(), 0);
            }
            first.write(917);
            first.add(2).write(12345);
            let adjacent = tre_mem_alloc(&mut arena, 16).cast::<u128>();
            adjacent.write(82);
            for index in 1..40 {
                let later = tre_mem_calloc(&mut arena, index * 1024).cast::<u128>();
                assert!(!later.is_null());
                assert!(later.is_aligned());
                assert_eq!(later.read(), 0);
                later.write(index as u128);
                if index == 3 {
                    owner.full_gc().unwrap();
                }
                assert_eq!(first.read(), 917);
                assert_eq!(first.add(2).read(), 12345);
                assert_eq!(adjacent.read(), 82);
            }
            assert!(tre_mem_alloc(&mut arena, usize::MAX).is_null());
            assert_eq!(first.read(), 917);
            assert!(tre_mem_alloc(&mut arena, 1).is_null());
        }
        assert!(owner.with_arena(|arena| arena.workspace_bytes()).unwrap() > baseline);
        drop(arena);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            baseline
        );
    }

    #[test]
    fn owned_compile_arena_denial_retains_live_cells_and_unrelated_tickets() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let ledger = owner
            .with_arena(|arena| arena.heap_identity().byte_ledger())
            .unwrap();
        let held = ledger.reserve(128).unwrap();
        let mut arena = tre_mem_struct::new(crate::mainutils::grep::regex_workspace()).unwrap();
        unsafe {
            let live = tre_mem_alloc(&mut arena, 16).cast::<u128>();
            live.write(42);
            let before = owner
                .with_arena(|arena| {
                    let total = arena.total_bytes_allocated();
                    arena.set_budget(crate::sexp::memory::ArenaBudget::new(total, 0));
                    arena.workspace_bytes()
                })
                .unwrap();
            assert!(tre_mem_alloc(&mut arena, 4096).is_null());
            assert_eq!(live.read(), 42);
            assert_eq!(
                owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
                before
            );
        }
        drop(arena);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            128
        );
        drop(held);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
    }

    #[test]
    fn owned_compile_arena_revocation_cannot_charge_replacement_owner() {
        let mut left = crate::sexp::session::RSession::new_for_gc_tests();
        let left_owner = left.owner_token().unwrap();
        let ledger = left_owner
            .with_arena(|arena| arena.heap_identity().byte_ledger())
            .unwrap();
        let mut arena = tre_mem_struct::new(crate::mainutils::grep::regex_workspace()).unwrap();
        unsafe {
            assert!(!tre_mem_alloc(&mut arena, 16).is_null());
        }
        left.close();
        let right = crate::sexp::session::RSession::new_for_gc_tests();
        let right_owner = right.owner_token().unwrap();
        let right_ledger = right_owner
            .with_arena(|arena| arena.heap_identity().byte_ledger())
            .unwrap();
        let held = right_ledger.reserve(256).unwrap();
        unsafe {
            assert!(tre_mem_alloc(&mut arena, 16).is_null());
        }
        drop(arena);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        assert_eq!(right_ledger.workspace_bytes_for_test(), 256);
        drop(held);
    }
}
