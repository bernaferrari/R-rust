#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Memory allocation for R objects.
//!
//! Uses arena allocation without garbage collection. All R objects are
//! allocated from an arena that is freed as a unit. This matches R's
//! allocation patterns where most objects are short-lived within a
//! single R expression evaluation.

use std::alloc::{alloc, dealloc, Layout};
use std::os::raw::c_void;
use std::ptr::{self, NonNull};

use super::ffi::{R_xlen_t, SexprecCore, SexprecData, Vecsxp, NA_INTEGER, SEXP, SEXPTYPE};

// ---------------------------------------------------------------------------
// Element sizes by SEXPTYPE
// ---------------------------------------------------------------------------

/// Get the element size in bytes for a vector SEXPTYPE.
/// Returns 0 for non-vector types.
pub fn sexp_elem_size(t: SEXPTYPE) -> usize {
    match t.0 {
        10 => 4,                                       // LGLSXP: c_int (4 bytes)
        13 => 4,                                       // INTSXP: c_int (4 bytes)
        14 => 8,                                       // REALSXP: c_double (8 bytes)
        15 => 16,                                      // CPLXSXP: Rcomplex (16 bytes)
        16 => std::mem::size_of::<*mut SexprecCore>(), // STRSXP: SEXP pointers
        19 => std::mem::size_of::<*mut SexprecCore>(), // VECSXP: SEXP pointers
        20 => std::mem::size_of::<*mut SexprecCore>(), // EXPRSXP: SEXP pointers
        24 => 1,                                       // RAWSXP: Rbyte (1 byte)
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
    nodes: Vec<Box<SexprecCore>>,
    /// All allocated data buffers (pointer, layout).
    data_bufs: Vec<(*mut u8, Layout)>,
    /// Free list of reclaimed SEXP pointers available for reuse.
    free_list: Vec<SEXP>,
}

impl RArena {
    /// Create a new empty arena.
    pub fn new() -> Self {
        RArena {
            nodes: Vec::new(),
            data_bufs: Vec::new(),
            free_list: Vec::new(),
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
        self.nodes.push(boxed);
        ptr
    }

    /// Allocate a vector SexprecCore node with associated data buffer.
    ///
    /// For INTSXP with length n: allocates n * 4 bytes.
    /// For REALSXP with length n: allocates n * 8 bytes.
    /// For STRSXP/VECSXP with length n: allocates n * sizeof(SEXP) bytes.
    pub fn alloc_vector(&mut self, sexptype: SEXPTYPE, length: R_xlen_t) -> SEXP {
        let elem_size = sexp_elem_size(sexptype);
        let total_bytes = (length as usize).checked_mul(elem_size).unwrap_or(0);

        let mut boxed = Box::new(SexprecCore::new_vector(sexptype, length));

        // Allocate data buffer
        if total_bytes > 0 {
            let layout = Layout::from_size_align(total_bytes, std::mem::align_of::<u64>())
                .expect("invalid layout");
            let data_ptr = if layout.size() > 0 {
                unsafe { alloc(layout) }
            } else {
                ptr::null_mut()
            };

            if data_ptr.is_null() && layout.size() > 0 {
                // Allocation failed — do NOT push the Box, it will be dropped
                return ptr::null_mut();
            }

            // Zero-initialize the data buffer
            if !data_ptr.is_null() {
                unsafe {
                    std::ptr::write_bytes(data_ptr, 0, total_bytes);
                }
            }

            // Store data pointer in gengc_next_node (same as DATAPTR convention)
            let ptr: SEXP = &mut *boxed as *mut _;
            unsafe {
                (*ptr).gengc_next_node = data_ptr as SEXP;
            }

            self.data_bufs.push((data_ptr, layout));
        }

        let ptr: SEXP = &mut *boxed as *mut _;
        self.nodes.push(boxed);
        ptr
    }

    /// Allocate a CHARSXP with inline string data.
    pub fn alloc_charsxp(&mut self, s: &[u8]) -> SEXP {
        let len = s.len() as R_xlen_t;

        let mut boxed = Box::new(SexprecCore::new(SEXPTYPE::CHARSXP));

        // Store truelength in the vecsxp union field
        boxed.data = SexprecData {
            charsxp_truelen: len,
        };

        // Allocate string data (with null terminator)
        let total_bytes = len as usize + 1;
        let layout = Layout::from_size_align(total_bytes, 1).expect("invalid layout");
        let data_ptr = unsafe { alloc(layout) };

        if data_ptr.is_null() {
            // Do NOT push the Box — it will be dropped, avoiding a leak
            return ptr::null_mut();
        }

        // Copy string data
        unsafe {
            std::ptr::copy_nonoverlapping(s.as_ptr(), data_ptr, s.len());
            *data_ptr.add(s.len()) = 0; // null terminator
        }

        // Store data pointer
        let ptr: SEXP = &mut *boxed as *mut _;
        unsafe {
            (*ptr).gengc_next_node = data_ptr as SEXP;
        }

        self.data_bufs.push((data_ptr, layout));
        self.nodes.push(boxed);
        ptr
    }

    /// Allocate a cons cell (LISTSXP).
    pub fn cons(&mut self, car: SEXP, cdr: SEXP, tag: SEXP) -> SEXP {
        let ptr = self.alloc_node(SEXPTYPE::LISTSXP);
        unsafe {
            (*ptr).data.listsxp.carval = car;
            (*ptr).data.listsxp.cdrval = cdr;
            (*ptr).data.listsxp.tagval = tag;
        }
        ptr
    }

    /// Allocate a nil-terminated pairlist chain of n elements.
    pub fn alloc_list_chain(&mut self, n: i32) -> SEXP {
        let mut result: SEXP = ptr::null_mut();
        for _ in 0..n {
            result = self.cons(ptr::null_mut(), result, ptr::null_mut());
        }
        result
    }

    /// Add an existing Box to the arena, returning a raw pointer.
    /// The arena takes ownership of the Box.
    pub fn add_node(&mut self, mut node: Box<SexprecCore>) -> SEXP {
        let ptr: SEXP = &mut *node as *mut _;
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
        if !ptr.is_null() {
            let addr = ptr as usize;
            self.nodes
                .retain(|b| &**b as *const _ as *const _ as usize != addr);
            self.free_list.push(ptr);
        }
    }
}

impl Default for RArena {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RArena {
    fn drop(&mut self) {
        // Free all data buffers
        for (ptr, layout) in &self.data_bufs {
            if !ptr.is_null() && layout.size() > 0 {
                unsafe {
                    dealloc(*ptr, *layout);
                }
            }
        }
        self.data_bufs.clear();
        // nodes are dropped automatically via Vec<Box<...>>
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
            // Data should be zero-initialized
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
        // Should be a chain of 3 cons cells ending in NULL
        unsafe {
            assert_eq!((*list).sxpinfo.type_of(), SEXPTYPE::LISTSXP);
            assert!((*list).data.listsxp.carval.is_null());
            let cdr1 = (*list).data.listsxp.cdrval;
            assert!(!cdr1.is_null());
            let cdr2 = (*cdr1).data.listsxp.cdrval;
            assert!(!cdr2.is_null());
            assert!((*cdr2).data.listsxp.cdrval.is_null()); // end of chain
        }
    }

    #[test]
    fn test_arena_drop() {
        // Verify that arena cleanup doesn't crash
        let mut arena = RArena::new();
        arena.alloc_vector(SEXPTYPE::REALSXP, 100);
        arena.alloc_charsxp(b"test string");
        arena.alloc_node(SEXPTYPE::INTSXP);
        // Drop happens here - should not panic
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
}
