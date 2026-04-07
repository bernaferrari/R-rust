#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Memory allocation for R objects.
//!
//! Uses arena allocation without garbage collection. All R objects are
//! allocated from an arena that is freed as a unit. This matches R's
//! allocation patterns where most objects are short-lived within a
//! single R expression evaluation.

use std::alloc::{Layout, alloc, dealloc};
use std::ptr::{self};

use super::ffi::{R_xlen_t, SEXP, SEXPTYPE, SexprecCore, SexprecData};

// ---------------------------------------------------------------------------
// Element sizes by SEXPTYPE
// ---------------------------------------------------------------------------

/// Get the element size in bytes for a vector SEXPTYPE.
/// Returns 0 for non-vector types.
pub fn sexp_elem_size(t: SEXPTYPE) -> usize {
    match t.0 {
        10 => 4,
        13 => 4,
        14 => 8,
        15 => 16,
        16 => std::mem::size_of::<*mut SexprecCore>(),
        19 => std::mem::size_of::<*mut SexprecCore>(),
        20 => std::mem::size_of::<*mut SexprecCore>(),
        24 => 1,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// RArena: arena allocator for R objects
// ---------------------------------------------------------------------------

/// An arena allocator for R objects.
///
/// Allocates SexprecCore nodes and their associated vector data.
/// The arena does NOT support individual deallocation -- the entire arena
/// is freed at once when dropped.
pub struct RArena {
    /// All allocated node pointers (Box<SexprecCore> for stable addresses).
    #[allow(clippy::vec_box)]
    nodes: Vec<Box<SexprecCore>>,
    /// All allocated data buffers (pointer, layout).
    data_bufs: Vec<(*mut u8, Layout)>,
    /// Free list of reclaimed SEXP pointers available for reuse.
    free_list: Vec<SEXP>,
    /// Total bytes allocated for tracking.
    total_bytes_allocated: usize,
}

impl RArena {
    /// Create a new empty arena.
    pub fn new() -> Self {
        RArena {
            nodes: Vec::new(),
            data_bufs: Vec::new(),
            free_list: Vec::new(),
            total_bytes_allocated: 0,
        }
    }

    /// Allocate a scalar SexprecCore node.
    ///
    /// Returns a raw SEXP pointer to the allocated node.
    /// The pointer is valid for the lifetime of the arena.
    pub fn alloc_node(&mut self, sexptype: SEXPTYPE) -> SEXP {
        if let Some(ptr) = self.free_list.pop() {
            unsafe {
                *ptr = SexprecCore::new(sexptype);
            }
            return ptr;
        }

        let mut boxed = Box::new(SexprecCore::new(sexptype));
        let ptr: SEXP = &mut *boxed as *mut _;
        self.total_bytes_allocated += std::mem::size_of::<SexprecCore>();
        self.nodes.push(boxed);
        ptr
    }

    /// Allocate a vector SexprecCore node with associated data buffer.
    ///
    /// For INTSXP with length n: allocates n * 4 bytes.
    /// For REALSXP with length n: allocates n * 8 bytes.
    /// For STRSXP/VECSXP with length n: allocates n * sizeof(SEXP) bytes.
    ///
    /// Returns null if allocation fails (OOM safety).
    pub fn alloc_vector(&mut self, sexptype: SEXPTYPE, length: R_xlen_t) -> SEXP {
        if length < 0 {
            return ptr::null_mut();
        }

        let elem_size = sexp_elem_size(sexptype);
        let total_bytes = match (length as usize).checked_mul(elem_size) {
            Some(n) => n,
            None => return ptr::null_mut(),
        };

        let mut boxed = Box::new(SexprecCore::new_vector(sexptype, length));

        if total_bytes > 0 {
            let layout = match Layout::from_size_align(total_bytes, std::mem::align_of::<u64>()) {
                Ok(l) => l,
                Err(_) => return ptr::null_mut(),
            };
            let data_ptr = if layout.size() > 0 {
                unsafe { alloc(layout) }
            } else {
                ptr::null_mut()
            };

            if data_ptr.is_null() && layout.size() > 0 {
                return ptr::null_mut();
            }

            if !data_ptr.is_null() {
                unsafe {
                    std::ptr::write_bytes(data_ptr, 0, total_bytes);
                }
            }

            let ptr: SEXP = &mut *boxed as *mut _;
            unsafe {
                (*ptr).gengc_next_node = data_ptr as SEXP;
            }

            self.total_bytes_allocated += total_bytes;
            self.data_bufs.push((data_ptr, layout));
        }

        let ptr: SEXP = &mut *boxed as *mut _;
        self.total_bytes_allocated += std::mem::size_of::<SexprecCore>();
        self.nodes.push(boxed);
        ptr
    }

    /// Allocate a CHARSXP with inline string data.
    ///
    /// Returns null if allocation fails (OOM safety).
    pub fn alloc_charsxp(&mut self, s: &[u8]) -> SEXP {
        let len = s.len() as R_xlen_t;

        let mut boxed = Box::new(SexprecCore::new(SEXPTYPE::CHARSXP));

        boxed.data = SexprecData {
            charsxp_truelen: len,
        };

        let total_bytes = match (len as usize).checked_add(1) {
            Some(n) => n,
            None => return ptr::null_mut(),
        };
        let layout = match Layout::from_size_align(total_bytes, 1) {
            Ok(l) => l,
            Err(_) => return ptr::null_mut(),
        };
        let data_ptr = unsafe { alloc(layout) };

        if data_ptr.is_null() {
            return ptr::null_mut();
        }

        unsafe {
            std::ptr::copy_nonoverlapping(s.as_ptr(), data_ptr, s.len());
            *data_ptr.add(s.len()) = 0;
        }

        let ptr: SEXP = &mut *boxed as *mut _;
        unsafe {
            (*ptr).gengc_next_node = data_ptr as SEXP;
        }

        self.total_bytes_allocated += total_bytes + std::mem::size_of::<SexprecCore>();
        self.data_bufs.push((data_ptr, layout));
        self.nodes.push(boxed);
        ptr
    }

    /// Allocate a cons cell (LISTSXP).
    pub fn cons(&mut self, car: SEXP, cdr: SEXP, tag: SEXP) -> SEXP {
        let ptr = self.alloc_node(SEXPTYPE::LISTSXP);
        if ptr.is_null() {
            return ptr::null_mut();
        }
        unsafe {
            (*ptr).data.listsxp.carval = car;
            (*ptr).data.listsxp.cdrval = cdr;
            (*ptr).data.listsxp.tagval = tag;
        }
        ptr
    }

    /// Allocate a nil-terminated pairlist chain of n elements.
    pub fn alloc_list_chain(&mut self, n: i32) -> SEXP {
        if n <= 0 {
            return ptr::null_mut();
        }
        let mut result: SEXP = ptr::null_mut();
        for _ in 0..n {
            result = self.cons(ptr::null_mut(), result, ptr::null_mut());
            if result.is_null() {
                return ptr::null_mut();
            }
        }
        result
    }

    /// Add an existing Box to the arena, returning a raw pointer.
    /// The arena takes ownership of the Box.
    pub fn add_node(&mut self, mut node: Box<SexprecCore>) -> SEXP {
        let ptr: SEXP = &mut *node as *mut _;
        self.total_bytes_allocated += std::mem::size_of::<SexprecCore>();
        self.nodes.push(node);
        ptr
    }

    /// Get the number of nodes allocated in this arena.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Iterate over all arena nodes.
    pub fn nodes(&self) -> impl Iterator<Item = SEXP> + '_ {
        self.nodes.iter().map(|b| &**b as *const _ as SEXP)
    }

    /// Free a node by adding it to the free list for reuse.
    pub fn free_node(&mut self, ptr: SEXP) {
        if ptr.is_null() {
            return;
        }
        let addr = ptr as usize;
        let before = self.nodes.len();
        self.nodes
            .retain(|b| &**b as *const _ as *const _ as usize != addr);
        if self.nodes.len() < before {
            self.free_list.push(ptr);
        }
    }

    /// Get the fragmentation ratio (freed / total capacity).
    pub fn fragmentation_ratio(&self) -> f64 {
        let total = self.nodes.len() + self.free_list.len();
        if total == 0 {
            0.0
        } else {
            self.free_list.len() as f64 / total as f64
        }
    }

    /// Get mutable access to the free list for compaction operations.
    pub fn free_list_mut(&mut self) -> &mut Vec<SEXP> {
        &mut self.free_list
    }

    /// Get the number of free slots available for reuse.
    pub fn free_count(&self) -> usize {
        self.free_list.len()
    }

    /// Get total bytes allocated by this arena.
    pub fn total_bytes_allocated(&self) -> usize {
        self.total_bytes_allocated
    }

    /// Verify arena invariants (debug only).
    fn verify_invariants(&self) {
        debug_assert!({
            for &buf in &self.data_bufs {
                let (ptr, layout) = buf;
                if !ptr.is_null() {
                    debug_assert!(layout.size() > 0);
                }
            }
            for &free_ptr in &self.free_list {
                debug_assert!(!free_ptr.is_null());
            }
            true
        });
    }
}

impl Default for RArena {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RArena {
    fn drop(&mut self) {
        for (ptr, layout) in &self.data_bufs {
            if !ptr.is_null() && layout.size() > 0 {
                unsafe {
                    dealloc(*ptr, *layout);
                }
            }
        }
        self.data_bufs.clear();
        self.free_list.clear();
    }
}

// ---------------------------------------------------------------------------
// Thread-local evaluation arena
// ---------------------------------------------------------------------------

/// Get or create the thread-local evaluation arena.
/// This is the default arena used by FFI constructor functions.
pub fn with_arena<F, R>(f: F) -> R
where
    F: FnOnce(&mut RArena) -> R,
{
    thread_local! {
        static EVAL_ARENA: std::cell::RefCell<RArena> = std::cell::RefCell::new(RArena::new());
    }
    EVAL_ARENA.with(|arena| {
        let mut arena = arena.borrow_mut();
        f(&mut arena)
    })
}

/// Reset the thread-local evaluation arena, freeing all allocations.
pub fn reset_arena() {
    with_arena(|arena| {
        *arena = RArena::new();
    });
}

/// Access the thread-local arena for GC operations.
/// This is identical to with_arena but named for clarity in GC context.
pub fn with_arena_for_gc<F, R>(f: F) -> R
where
    F: FnOnce(&mut RArena) -> R,
{
    with_arena(f)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::sexp::ffi::*;

    use super::*;

    #[test]
    fn test_arena_alloc_node() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_node(SEXPTYPE::INTSXP);
        assert!(!ptr.is_null());
        unsafe {
            assert_eq!((*ptr).sxpinfo.type_of(), SEXPTYPE::INTSXP);
        }
        assert_eq!(arena.node_count(), 1);
    }

    #[test]
    fn test_arena_alloc_vector_real() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 5);
        assert!(!ptr.is_null());
        unsafe {
            assert_eq!((*ptr).sxpinfo.type_of(), SEXPTYPE::REALSXP);
            assert_eq!((*ptr).vecsxp_length(), 5);
        }
    }

    #[test]
    fn test_arena_alloc_vector_int() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
        assert!(!ptr.is_null());
        unsafe {
            assert_eq!((*ptr).vecsxp_length(), 3);
            let data = (*ptr).gengc_next_node as *mut i32;
            assert_eq!(*data.add(0), 0);
            assert_eq!(*data.add(1), 0);
            assert_eq!(*data.add(2), 0);
        }
    }

    #[test]
    fn test_arena_alloc_vector_empty() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 0);
        assert!(!ptr.is_null());
        unsafe {
            assert_eq!((*ptr).vecsxp_length(), 0);
        }
    }

    #[test]
    fn test_arena_alloc_vector_negative_length() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, -1);
        assert!(ptr.is_null());
    }

    #[test]
    fn test_arena_alloc_charsxp() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_charsxp(b"hello");
        assert!(!ptr.is_null());
        unsafe {
            assert_eq!((*ptr).sxpinfo.type_of(), SEXPTYPE::CHARSXP);
            let data = (*ptr).gengc_next_node as *const u8;
            let s = std::ffi::CStr::from_ptr(data as *const i8);
            assert_eq!(s.to_str().unwrap(), "hello");
        }
    }

    #[test]
    fn test_arena_alloc_charsxp_empty() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_charsxp(b"");
        assert!(!ptr.is_null());
        unsafe {
            let data = (*ptr).gengc_next_node as *const u8;
            let s = std::ffi::CStr::from_ptr(data as *const i8);
            assert_eq!(s.to_str().unwrap(), "");
        }
    }

    #[test]
    fn test_arena_cons() {
        let mut arena = RArena::new();
        let car = arena.alloc_node(SEXPTYPE::INTSXP);
        let cdr = arena.alloc_node(SEXPTYPE::REALSXP);
        let cell = arena.cons(car, cdr, ptr::null_mut());
        assert!(!cell.is_null());
        unsafe {
            assert_eq!((*cell).sxpinfo.type_of(), SEXPTYPE::LISTSXP);
            assert_eq!((*cell).data.listsxp.carval, car);
            assert_eq!((*cell).data.listsxp.cdrval, cdr);
            assert!((*cell).data.listsxp.tagval.is_null());
        }
    }

    #[test]
    fn test_arena_alloc_list_chain() {
        let mut arena = RArena::new();
        let list = arena.alloc_list_chain(3);
        assert!(!list.is_null());
        unsafe {
            assert_eq!((*list).sxpinfo.type_of(), SEXPTYPE::LISTSXP);
            assert!((*list).data.listsxp.carval.is_null());
            let cdr1 = (*list).data.listsxp.cdrval;
            assert!(!cdr1.is_null());
            let cdr2 = (*cdr1).data.listsxp.cdrval;
            assert!(!cdr2.is_null());
            assert!((*cdr2).data.listsxp.cdrval.is_null());
        }
    }

    #[test]
    fn test_arena_alloc_list_chain_zero() {
        let mut arena = RArena::new();
        let list = arena.alloc_list_chain(0);
        assert!(list.is_null());
    }

    #[test]
    fn test_arena_alloc_list_chain_negative() {
        let mut arena = RArena::new();
        let list = arena.alloc_list_chain(-1);
        assert!(list.is_null());
    }

    #[test]
    fn test_arena_drop() {
        let mut arena = RArena::new();
        arena.alloc_vector(SEXPTYPE::REALSXP, 100);
        arena.alloc_charsxp(b"test string");
        arena.alloc_node(SEXPTYPE::INTSXP);
        drop(arena);
    }

    #[test]
    fn test_sexp_elem_size() {
        assert_eq!(sexp_elem_size(SEXPTYPE::LGLSXP), 4);
        assert_eq!(sexp_elem_size(SEXPTYPE::INTSXP), 4);
        assert_eq!(sexp_elem_size(SEXPTYPE::REALSXP), 8);
        assert_eq!(sexp_elem_size(SEXPTYPE::CPLXSXP), 16);
        assert_eq!(sexp_elem_size(SEXPTYPE::RAWSXP), 1);
        assert_eq!(
            sexp_elem_size(SEXPTYPE::STRSXP),
            std::mem::size_of::<*mut SexprecCore>()
        );
        assert_eq!(
            sexp_elem_size(SEXPTYPE::VECSXP),
            std::mem::size_of::<*mut SexprecCore>()
        );
        assert_eq!(sexp_elem_size(SEXPTYPE::NILSXP), 0);
        assert_eq!(sexp_elem_size(SEXPTYPE::SYMSXP), 0);
    }

    #[test]
    fn test_arena_write_read_real() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 3);
        unsafe {
            let data = (*ptr).gengc_next_node as *mut f64;
            *data.add(0) = 1.5;
            *data.add(1) = 2.5;
            *data.add(2) = 3.5;
            assert_eq!(*data.add(0), 1.5);
            assert_eq!(*data.add(1), 2.5);
            assert_eq!(*data.add(2), 3.5);
        }
    }

    #[test]
    fn test_arena_write_read_int() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
        unsafe {
            let data = (*ptr).gengc_next_node as *mut i32;
            *data.add(0) = 42;
            *data.add(1) = -1;
            *data.add(2) = NA_INTEGER;
            assert_eq!(*data.add(0), 42);
            assert_eq!(*data.add(1), -1);
            assert_eq!(*data.add(2), NA_INTEGER);
        }
    }

    #[test]
    fn test_arena_free_node() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_node(SEXPTYPE::INTSXP);
        assert_eq!(arena.node_count(), 1);
        assert_eq!(arena.free_count(), 0);
        arena.free_node(ptr);
        assert_eq!(arena.node_count(), 0);
        assert_eq!(arena.free_count(), 1);
    }

    #[test]
    fn test_arena_free_node_null() {
        let mut arena = RArena::new();
        arena.free_node(ptr::null_mut());
        assert_eq!(arena.free_count(), 0);
    }

    #[test]
    fn test_arena_free_node_reuse() {
        let mut arena = RArena::new();
        let ptr1 = arena.alloc_node(SEXPTYPE::INTSXP);
        arena.free_node(ptr1);
        let ptr2 = arena.alloc_node(SEXPTYPE::REALSXP);
        assert_eq!(ptr1, ptr2);
        assert_eq!(arena.node_count(), 1);
        assert_eq!(arena.free_count(), 0);
    }

    #[test]
    fn test_arena_fragmentation_ratio() {
        let mut arena = RArena::new();
        assert_eq!(arena.fragmentation_ratio(), 0.0);
        let ptr = arena.alloc_node(SEXPTYPE::INTSXP);
        assert_eq!(arena.fragmentation_ratio(), 0.0);
        arena.free_node(ptr);
        assert_eq!(arena.fragmentation_ratio(), 1.0);
    }

    #[test]
    fn test_arena_total_bytes() {
        let mut arena = RArena::new();
        assert_eq!(arena.total_bytes_allocated(), 0);
        arena.alloc_node(SEXPTYPE::INTSXP);
        assert!(arena.total_bytes_allocated() > 0);
    }

    #[test]
    fn test_arena_default() {
        let arena = RArena::default();
        assert_eq!(arena.node_count(), 0);
        assert_eq!(arena.free_count(), 0);
    }

    #[test]
    fn test_arena_add_node() {
        let mut arena = RArena::new();
        let boxed = Box::new(SexprecCore::new(SEXPTYPE::INTSXP));
        let ptr = arena.add_node(boxed);
        assert!(!ptr.is_null());
        assert_eq!(arena.node_count(), 1);
    }
}
