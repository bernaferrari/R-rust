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
use super::object::Sexp;

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

const GC_TRIGGER_THRESHOLD: usize = 10_000;
const GC_BYTE_THRESHOLD: usize = 64 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Arena budget and error types
// ---------------------------------------------------------------------------

/// Errors that can occur during arena allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArenaError {
    /// Allocation failed (e.g., out of memory).
    OutOfMemory,
    /// Request would exceed the arena's byte budget.
    ByteBudgetExceeded { limit: usize, requested: usize },
    /// Request would exceed the arena's node budget.
    NodeBudgetExceeded { limit: usize, requested: usize },
    /// Invalid vector length (negative or overflow).
    InvalidLength,
}

impl std::fmt::Display for ArenaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArenaError::OutOfMemory => write!(f, "arena out of memory"),
            ArenaError::ByteBudgetExceeded { limit, requested } => {
                write!(
                    f,
                    "arena byte budget exceeded: limit={limit}, requested={requested}"
                )
            }
            ArenaError::NodeBudgetExceeded { limit, requested } => {
                write!(
                    f,
                    "arena node budget exceeded: limit={limit}, requested={requested}"
                )
            }
            ArenaError::InvalidLength => write!(f, "invalid vector length"),
        }
    }
}

impl std::error::Error for ArenaError {}

/// Budget for arena allocations to prevent unbounded growth.
///
/// A budget of `0` means unlimited for that dimension.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArenaBudget {
    /// Maximum total bytes allowed in this arena (0 = unlimited).
    pub max_bytes: usize,
    /// Maximum number of active nodes allowed in this arena (0 = unlimited).
    pub max_nodes: usize,
}

impl ArenaBudget {
    /// Create an unlimited budget.
    pub const fn unlimited() -> Self {
        ArenaBudget {
            max_bytes: 0,
            max_nodes: 0,
        }
    }

    /// Create a budget with the given limits.
    pub const fn new(max_bytes: usize, max_nodes: usize) -> Self {
        ArenaBudget {
            max_bytes,
            max_nodes,
        }
    }
}

#[derive(Clone, Copy)]
struct DataBuffer {
    ptr: *mut u8,
    layout: Layout,
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
    data_bufs: Vec<DataBuffer>,
    /// Free list of reclaimed SEXP pointers available for reuse.
    free_list: Vec<SEXP>,
    /// Total bytes allocated for tracking.
    total_bytes_allocated: usize,
    /// Optional budget to limit arena growth.
    budget: ArenaBudget,
}

impl RArena {
    /// Create a new empty arena with an unlimited budget.
    pub fn new() -> Self {
        RArena {
            nodes: Vec::new(),
            data_bufs: Vec::new(),
            free_list: Vec::new(),
            total_bytes_allocated: 0,
            budget: ArenaBudget::unlimited(),
        }
    }

    /// Create a new empty arena with the given budget.
    pub fn with_budget(budget: ArenaBudget) -> Self {
        RArena {
            nodes: Vec::new(),
            data_bufs: Vec::new(),
            free_list: Vec::new(),
            total_bytes_allocated: 0,
            budget,
        }
    }

    /// Return the current arena budget.
    pub fn budget(&self) -> ArenaBudget {
        self.budget
    }

    /// Set a new budget. Does not retroactively reject existing allocations.
    pub fn set_budget(&mut self, budget: ArenaBudget) {
        self.budget = budget;
    }

    fn register_data_buffer(&mut self, ptr: *mut u8, layout: Layout) {
        self.total_bytes_allocated += layout.size();
        self.data_bufs.push(DataBuffer { ptr, layout });
    }

    fn take_data_buffer(&mut self, ptr: *mut u8) -> Option<Layout> {
        let index = self.data_bufs.iter().position(|buf| buf.ptr == ptr)?;
        let buf = self.data_bufs.swap_remove(index);
        self.total_bytes_allocated = self.total_bytes_allocated.saturating_sub(buf.layout.size());
        Some(buf.layout)
    }

    fn can_activate_node(&self) -> bool {
        self.budget.max_nodes == 0 || self.node_count() < self.budget.max_nodes
    }

    fn can_grow_bytes_by(&self, bytes: usize) -> bool {
        self.budget.max_bytes == 0
            || self
                .total_bytes_allocated
                .checked_add(bytes)
                .is_some_and(|total| total <= self.budget.max_bytes)
    }

    fn can_allocate_new_node_with_payload(&self, bytes: usize) -> bool {
        self.can_activate_node()
            && bytes
                .checked_add(std::mem::size_of::<SexprecCore>())
                .is_some_and(|total| self.can_grow_bytes_by(total))
    }

    /// Allocate a scalar SexprecCore node.
    ///
    /// Returns a raw SEXP pointer to the allocated node.
    /// The pointer is valid for the lifetime of the arena.
    pub fn alloc_node(&mut self, sexptype: SEXPTYPE) -> SEXP {
        if !self.can_activate_node() {
            return ptr::null_mut();
        }

        if let Some(ptr) = self.free_list.pop() {
            unsafe {
                *ptr = SexprecCore::new(sexptype);
            }
            return ptr;
        }

        let active_nodes = self.nodes.len() - self.free_list.len();
        let should_gc =
            active_nodes > GC_TRIGGER_THRESHOLD || self.total_bytes_allocated > GC_BYTE_THRESHOLD;
        if should_gc {
            crate::sexp::gengc::minor_gc();
            if let Some(ptr) = self.free_list.pop() {
                unsafe {
                    *ptr = SexprecCore::new(sexptype);
                }
                return ptr;
            }
        }

        if !self.can_grow_bytes_by(std::mem::size_of::<SexprecCore>()) {
            return ptr::null_mut();
        }

        let mut boxed = Box::new(SexprecCore::new(sexptype));
        let ptr: SEXP = &mut *boxed as *mut _;
        self.total_bytes_allocated += std::mem::size_of::<SexprecCore>();
        self.nodes.push(boxed);
        ptr
    }

    /// Allocate a scalar node and return an arena-scoped safe wrapper.
    pub fn alloc_node_sexp(&mut self, sexptype: SEXPTYPE) -> Option<Sexp<'_>> {
        let ptr = self.alloc_node(sexptype);
        self.sexp(ptr)
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

        if self.total_bytes_allocated > GC_BYTE_THRESHOLD {
            crate::sexp::gengc::minor_gc();
        }

        let elem_size = sexp_elem_size(sexptype);
        let total_bytes = match (length as usize).checked_mul(elem_size) {
            Some(n) => n,
            None => return ptr::null_mut(),
        };

        if !self.can_allocate_new_node_with_payload(total_bytes) {
            return ptr::null_mut();
        }

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

            self.register_data_buffer(data_ptr, layout);
        }

        let ptr: SEXP = &mut *boxed as *mut _;
        self.total_bytes_allocated += std::mem::size_of::<SexprecCore>();
        self.nodes.push(boxed);
        ptr
    }

    /// Allocate a vector node and return an arena-scoped safe wrapper.
    pub fn alloc_vector_sexp(&mut self, sexptype: SEXPTYPE, length: R_xlen_t) -> Option<Sexp<'_>> {
        let ptr = self.alloc_vector(sexptype, length);
        self.sexp(ptr)
    }

    /// Allocate a vector SexprecCore node with associated data buffer,
    /// returning `Result` instead of a raw pointer.
    ///
    /// Checks the arena budget before allocating and returns a descriptive
    /// error if the budget would be exceeded.
    pub fn alloc_vector_checked(
        &mut self,
        sexptype: SEXPTYPE,
        length: R_xlen_t,
    ) -> Result<SEXP, ArenaError> {
        if length < 0 {
            return Err(ArenaError::InvalidLength);
        }

        // Check node budget
        if self.budget.max_nodes > 0 {
            let active = self.node_count();
            if active >= self.budget.max_nodes {
                return Err(ArenaError::NodeBudgetExceeded {
                    limit: self.budget.max_nodes,
                    requested: active + 1,
                });
            }
        }

        let elem_size = sexp_elem_size(sexptype);
        let data_bytes = match (length as usize).checked_mul(elem_size) {
            Some(n) => n,
            None => return Err(ArenaError::InvalidLength),
        };
        let total_increase = data_bytes
            .checked_add(std::mem::size_of::<SexprecCore>())
            .ok_or(ArenaError::InvalidLength)?;

        // Check byte budget
        if self.budget.max_bytes > 0 {
            let new_total = self
                .total_bytes_allocated
                .checked_add(total_increase)
                .ok_or(ArenaError::ByteBudgetExceeded {
                    limit: self.budget.max_bytes,
                    requested: usize::MAX,
                })?;
            if new_total > self.budget.max_bytes {
                return Err(ArenaError::ByteBudgetExceeded {
                    limit: self.budget.max_bytes,
                    requested: new_total,
                });
            }
        }

        // Run GC if approaching thresholds (same as alloc_vector)
        if self.total_bytes_allocated > GC_BYTE_THRESHOLD {
            crate::sexp::gengc::minor_gc();
        }

        let mut boxed = Box::new(SexprecCore::new_vector(sexptype, length));

        if data_bytes > 0 {
            let layout = match Layout::from_size_align(data_bytes, std::mem::align_of::<u64>()) {
                Ok(l) => l,
                Err(_) => return Err(ArenaError::OutOfMemory),
            };
            let data_ptr = unsafe { alloc(layout) };
            if data_ptr.is_null() {
                return Err(ArenaError::OutOfMemory);
            }
            unsafe {
                std::ptr::write_bytes(data_ptr, 0, data_bytes);
            }
            let ptr: SEXP = &mut *boxed as *mut _;
            unsafe {
                (*ptr).gengc_next_node = data_ptr as SEXP;
            }
            self.register_data_buffer(data_ptr, layout);
        }

        let ptr: SEXP = &mut *boxed as *mut _;
        self.total_bytes_allocated += std::mem::size_of::<SexprecCore>();
        self.nodes.push(boxed);
        Ok(ptr)
    }

    /// Allocate a vector node and return an arena-scoped safe wrapper with a
    /// descriptive allocation error.
    pub fn alloc_vector_checked_sexp(
        &mut self,
        sexptype: SEXPTYPE,
        length: R_xlen_t,
    ) -> Result<Sexp<'_>, ArenaError> {
        let ptr = self.alloc_vector_checked(sexptype, length)?;
        self.sexp(ptr).ok_or(ArenaError::OutOfMemory)
    }

    /// Allocate a CHARSXP with inline string data.
    ///
    /// Returns null if allocation fails (OOM safety).
    pub fn alloc_charsxp(&mut self, s: &[u8]) -> SEXP {
        let len = s.len() as R_xlen_t;

        let total_bytes = match (len as usize).checked_add(1) {
            Some(n) => n,
            None => return ptr::null_mut(),
        };
        if !self.can_allocate_new_node_with_payload(total_bytes) {
            return ptr::null_mut();
        }

        let mut boxed = Box::new(SexprecCore::new(SEXPTYPE::CHARSXP));

        boxed.data = SexprecData {
            charsxp_truelen: len,
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

        self.register_data_buffer(data_ptr, layout);
        self.total_bytes_allocated += std::mem::size_of::<SexprecCore>();
        self.nodes.push(boxed);
        ptr
    }

    /// Allocate a CHARSXP and return an arena-scoped safe wrapper.
    pub fn alloc_charsxp_sexp(&mut self, s: &[u8]) -> Option<Sexp<'_>> {
        let ptr = self.alloc_charsxp(s);
        self.sexp(ptr)
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

    /// Allocate a cons cell from safe wrappers and return an arena-scoped
    /// wrapper for the new cell.
    pub fn cons_sexp<'a>(
        &'a mut self,
        car: Sexp<'_>,
        cdr: Sexp<'_>,
        tag: Option<Sexp<'_>>,
    ) -> Option<Sexp<'a>> {
        let ptr = self.cons(
            car.as_raw(),
            cdr.as_raw(),
            tag.map_or(ptr::null_mut(), Sexp::as_raw),
        );
        self.sexp(ptr)
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
        if !self.can_allocate_new_node_with_payload(0) {
            return ptr::null_mut();
        }

        let ptr: SEXP = &mut *node as *mut _;
        self.total_bytes_allocated += std::mem::size_of::<SexprecCore>();
        self.nodes.push(node);
        ptr
    }

    /// Get the number of nodes allocated in this arena.
    pub fn node_count(&self) -> usize {
        self.nodes.len() - self.free_list.len()
    }

    /// Return true if this pointer is one of the arena's active nodes.
    pub fn contains(&self, ptr: SEXP) -> bool {
        if ptr.is_null() {
            return false;
        }
        self.nodes.iter().any(|b| std::ptr::eq(&**b, ptr))
            && !self.free_list.iter().any(|free| std::ptr::eq(*free, ptr))
    }

    /// Wrap an active arena-owned pointer in a safe `Sexp`.
    ///
    /// Unlike raw construction, this checks that the pointer belongs to this
    /// arena and is not currently on the free list, tying the wrapper lifetime
    /// to the arena borrow.
    pub fn sexp(&self, ptr: SEXP) -> Option<Sexp<'_>> {
        if self.contains(ptr) {
            Sexp::from_raw(ptr)
        } else {
            None
        }
    }

    /// Iterate over all arena nodes.
    pub fn nodes(&self) -> impl Iterator<Item = SEXP> + '_ {
        self.nodes.iter().map(|b| &**b as *const _ as SEXP)
    }

    /// Iterate over nodes that are currently active, excluding free-list slots.
    pub fn active_nodes(&self) -> impl Iterator<Item = SEXP> + '_ {
        self.nodes()
            .filter(|ptr| !self.free_list.iter().any(|free| free == ptr))
    }

    /// Free a node by adding it to the free list for reuse.
    pub fn free_node(&mut self, ptr: SEXP) {
        if ptr.is_null() {
            return;
        }
        if self.free_list.contains(&ptr) {
            return;
        }
        let addr = ptr as usize;
        let found = self
            .nodes
            .iter()
            .any(|b| &**b as *const _ as *const _ as usize == addr);
        if !found {
            return;
        }

        unsafe {
            let data_ptr = (*ptr).gengc_next_node as *mut u8;
            if !data_ptr.is_null() {
                if let Some(layout) = self.take_data_buffer(data_ptr) {
                    dealloc(data_ptr, layout);
                }
            }
            (*ptr).gengc_next_node = ptr::null_mut();
            (*ptr).attrib = ptr::null_mut();
            (*ptr).sxpinfo.set_mark(false);
            (*ptr)
                .sxpinfo
                .set_gcgen(crate::sexp::gengc::Generation::Old as u8);
        }

        self.free_list.push(ptr);
    }

    /// Get the fragmentation ratio (freed / total capacity).
    pub fn fragmentation_ratio(&self) -> f64 {
        if self.nodes.is_empty() {
            0.0
        } else {
            self.free_list.len() as f64 / self.nodes.len() as f64
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
            for buf in &self.data_bufs {
                if !buf.ptr.is_null() {
                    debug_assert!(buf.layout.size() > 0);
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
        for buf in &self.data_bufs {
            if !buf.ptr.is_null() && buf.layout.size() > 0 {
                unsafe {
                    dealloc(buf.ptr, buf.layout);
                }
            }
        }
        self.data_bufs.clear();
        self.free_list.clear();
    }
}

// ---------------------------------------------------------------------------
// Instance evaluation arena
// ---------------------------------------------------------------------------

/// Access the active instance evaluation arena.
///
/// Allocation is intentionally scoped to an `RInstance`: unscoped arena
/// fallback would let objects escape the session that owns evaluator state,
/// which breaks Android multi-instance isolation.
pub fn with_arena<F, R>(f: F) -> R
where
    F: FnOnce(&mut RArena) -> R,
{
    super::instance::with_required_current_instance(|inst| f(&mut inst.arena))
}

/// Reset the active instance evaluation arena, freeing all allocations.
pub fn reset_arena() {
    with_arena(|arena| {
        *arena = RArena::new();
    });
}

/// Access the active instance arena for GC operations.
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
    fn test_arena_sexp_wraps_only_owned_active_nodes() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_node(SEXPTYPE::INTSXP);
        assert!(arena.sexp(ptr).is_some());

        arena.free_node(ptr);
        assert!(arena.sexp(ptr).is_none());
    }

    #[test]
    fn test_arena_sexp_rejects_foreign_nodes() {
        let mut owner = RArena::new();
        let foreign = RArena::new();
        let ptr = owner.alloc_node(SEXPTYPE::INTSXP);

        assert!(owner.sexp(ptr).is_some());
        assert!(foreign.sexp(ptr).is_none());
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
            let s = std::ffi::CStr::from_ptr(data as *const libc::c_char);
            assert_eq!(s.to_str().unwrap_or(""), "hello");
        }
    }

    #[test]
    fn test_arena_alloc_charsxp_empty() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_charsxp(b"");
        assert!(!ptr.is_null());
        unsafe {
            let data = (*ptr).gengc_next_node as *const u8;
            let s = std::ffi::CStr::from_ptr(data as *const libc::c_char);
            assert_eq!(s.to_str().unwrap_or(""), "");
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
    fn test_arena_free_node_idempotent() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_node(SEXPTYPE::INTSXP);
        arena.free_node(ptr);
        arena.free_node(ptr);
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
    fn test_arena_active_nodes_excludes_free_list() {
        let mut arena = RArena::new();
        let live = arena.alloc_node(SEXPTYPE::INTSXP);
        let freed = arena.alloc_node(SEXPTYPE::REALSXP);
        arena.free_node(freed);

        let active: Vec<SEXP> = arena.active_nodes().collect();
        assert_eq!(active, vec![live]);
    }

    #[test]
    fn test_arena_free_vector_clears_payload_pointer() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 2);
        assert!(!ptr.is_null());
        unsafe {
            assert!(!(*ptr).gengc_next_node.is_null());
        }
        arena.free_node(ptr);
        unsafe {
            assert!((*ptr).gengc_next_node.is_null());
        }
    }

    #[test]
    fn test_arena_free_expression_vector_uses_tracked_layout() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::EXPRSXP, 2);
        assert!(!ptr.is_null());
        unsafe {
            assert!(!(*ptr).gengc_next_node.is_null());
        }
        arena.free_node(ptr);
        assert_eq!(arena.free_count(), 1);
        unsafe {
            assert!((*ptr).gengc_next_node.is_null());
        }
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
    fn test_arena_node_budget_applies_to_raw_alloc_node() {
        let mut arena = RArena::with_budget(ArenaBudget::new(0, 1));
        let first = arena.alloc_node(SEXPTYPE::INTSXP);
        assert!(!first.is_null());
        let second = arena.alloc_node(SEXPTYPE::REALSXP);
        assert!(second.is_null());

        arena.free_node(first);
        let reused = arena.alloc_node(SEXPTYPE::REALSXP);
        assert_eq!(reused, first);
    }

    #[test]
    fn test_arena_byte_budget_applies_to_raw_allocations() {
        let node_bytes = std::mem::size_of::<SexprecCore>();

        let mut arena = RArena::with_budget(ArenaBudget::new(node_bytes, 0));
        let node = arena.alloc_node(SEXPTYPE::INTSXP);
        assert!(!node.is_null());
        assert!(arena.alloc_charsxp(b"x").is_null());
        arena.free_node(node);
        assert_eq!(arena.alloc_node(SEXPTYPE::REALSXP), node);

        let mut arena = RArena::with_budget(ArenaBudget::new(node_bytes + 8, 0));
        assert!(!arena.alloc_vector(SEXPTYPE::REALSXP, 1).is_null());
        assert!(arena.alloc_vector(SEXPTYPE::REALSXP, 1).is_null());
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

    #[test]
    fn test_arena_add_node_obeys_budget() {
        let node_bytes = std::mem::size_of::<SexprecCore>();
        let mut arena = RArena::with_budget(ArenaBudget::new(node_bytes, 1));
        assert!(
            !arena
                .add_node(Box::new(SexprecCore::new(SEXPTYPE::INTSXP)))
                .is_null()
        );
        assert!(
            arena
                .add_node(Box::new(SexprecCore::new(SEXPTYPE::REALSXP)))
                .is_null()
        );
    }
}
