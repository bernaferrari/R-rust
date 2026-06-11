#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Memory allocation for R objects.
//!
//! Uses arena allocation without garbage collection. All R objects are
//! allocated from an arena that is freed as a unit. This matches R's
//! allocation patterns where most objects are short-lived within a
//! single R expression evaluation.

use std::alloc::{Layout, alloc, dealloc};
use std::collections::{HashMap, HashSet};
use std::ptr::{self};

/// Size of each slab page for SexprecCore nodes. Larger pages reduce allocator overhead
/// and improve cache locality vs one Box per node. Chose 4096 as balance ( ~256KB per page
/// assuming ~64B SexprecCore).
const NODE_PAGE_SIZE: usize = 4096;

use super::ffi::{R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE, SexprecCore, SexprecData};
use super::object::Sexp;

// ---------------------------------------------------------------------------
// Element sizes by SEXPTYPE
// ---------------------------------------------------------------------------

/// Get the element size in bytes for a vector SEXPTYPE.
/// Returns 0 for non-vector types.
pub fn sexp_elem_size(t: SEXPTYPE) -> usize {
    match t {
        SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP => std::mem::size_of::<i32>(),
        SEXPTYPE::REALSXP => std::mem::size_of::<f64>(),
        SEXPTYPE::CPLXSXP => std::mem::size_of::<Rcomplex>(),
        SEXPTYPE::STRSXP | SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP | SEXPTYPE::BCODESXP => {
            std::mem::size_of::<SEXP>()
        }
        SEXPTYPE::RAWSXP => std::mem::size_of::<Rbyte>(),
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

// Data buffers now use HashMap for O(1) register/take/remove (was linear scan on Vec).
// This eliminates one source of O(n) in arena (take_data_buffer, and frequent free of vectors).
// Layouts are small, HashMap overhead acceptable vs scan on many vectors.

// ---------------------------------------------------------------------------
// RArena: arena allocator for R objects
// ---------------------------------------------------------------------------

/// An arena allocator for R objects.
///
/// Allocates SexprecCore nodes and their associated vector data.
/// The arena does NOT support individual deallocation -- the entire arena
/// is freed at once when dropped.
pub struct RArena {
    /// Slab pages for SexprecCore nodes. Each page is a Vec reserved to NODE_PAGE_SIZE
    /// (no reallocs within page -> stable pointers). This eliminates per-node Box
    /// overhead (allocator metadata + indirection) vs original Vec<Box<...>>.
    /// Pages never move; new pages appended for growth. Matches spirit of R's NodeClass pages.
    node_pages: Vec<Vec<SexprecCore>>,
    /// Current page index for allocation (last page usually).
    slab_page: usize,
    /// Current offset within the slab_page (0 .. NODE_PAGE_SIZE).
    slab_offset: usize,
    /// All allocated data buffers. HashMap for O(1) lookup/remove (was Vec + linear .position).
    /// Key: data ptr; value: layout for dealloc and accounting.
    data_bufs: HashMap<*mut u8, Layout>,
    /// Free list of reclaimed SEXP pointers available for reuse.
    free_list: Vec<SEXP>,
    /// O(1) membership for active node pointers.
    active_addrs: HashSet<usize>,
    /// O(1) membership for free-list pointers.
    free_addrs: HashSet<usize>,
    /// Total bytes allocated for tracking.
    total_bytes_allocated: usize,
    /// Optional budget to limit arena growth.
    budget: ArenaBudget,
}

impl RArena {
    /// Allocate a new page in the slab. Reserves exactly to avoid realloc (stable ptrs inside).
    fn alloc_new_page(&mut self) {
        let page = Vec::with_capacity(NODE_PAGE_SIZE);
        self.node_pages.push(page);
        self.slab_page = self.node_pages.len() - 1;
        self.slab_offset = 0;
    }

    /// Allocate a core node into the current slab page (creating new page if needed).
    /// Returns the raw SEXP ptr. Updates accounting and active tracking.
    /// Used by scalar/vector/CHARS allocs to avoid duplication (elegance + maintainability).
    #[inline]
    fn allocate_core_in_slab<F>(&mut self, ctor: F) -> SEXP
    where
        F: FnOnce() -> SexprecCore,
    {
        if self.slab_offset >= NODE_PAGE_SIZE {
            self.alloc_new_page();
        }
        let page = &mut self.node_pages[self.slab_page];
        page.push(ctor());
        let ptr: SEXP = &mut page[self.slab_offset] as *mut _;
        self.slab_offset += 1;
        self.total_bytes_allocated += std::mem::size_of::<SexprecCore>();
        self.register_new_node(ptr)
    }

    /// Create a new empty arena with an unlimited budget.
    pub fn new() -> Self {
        let mut a = RArena {
            node_pages: Vec::new(),
            slab_page: 0,
            slab_offset: NODE_PAGE_SIZE,
            data_bufs: HashMap::new(),
            free_list: Vec::new(),
            active_addrs: HashSet::new(),
            free_addrs: HashSet::new(),
            total_bytes_allocated: 0,
            budget: ArenaBudget::unlimited(),
        };
        a.alloc_new_page();
        a
    }

    /// Create a new empty arena with the given budget.
    pub fn with_budget(budget: ArenaBudget) -> Self {
        let mut a = RArena {
            node_pages: Vec::new(),
            slab_page: 0,
            slab_offset: NODE_PAGE_SIZE,
            data_bufs: HashMap::new(),
            free_list: Vec::new(),
            active_addrs: HashSet::new(),
            free_addrs: HashSet::new(),
            total_bytes_allocated: 0,
            budget,
        };
        a.alloc_new_page();
        a
    }

    fn track_node_active(&mut self, ptr: SEXP) {
        if !ptr.is_null() {
            self.active_addrs.insert(ptr as usize);
            self.free_addrs.remove(&(ptr as usize));
        }
    }

    fn track_node_freed(&mut self, ptr: SEXP) {
        if !ptr.is_null() {
            self.active_addrs.remove(&(ptr as usize));
            self.free_addrs.insert(ptr as usize);
        }
    }

    fn reuse_free_node(&mut self, sexptype: SEXPTYPE) -> Option<SEXP> {
        let ptr = self.free_list.pop()?;
        unsafe {
            *ptr = SexprecCore::new(sexptype);
        }
        self.track_node_active(ptr);
        Some(ptr)
    }

    fn register_new_node(&mut self, ptr: SEXP) -> SEXP {
        self.track_node_active(ptr);
        ptr
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
        self.data_bufs.insert(ptr, layout);
    }

    fn take_data_buffer(&mut self, ptr: *mut u8) -> Option<Layout> {
        if let Some(layout) = self.data_bufs.remove(&ptr) {
            self.total_bytes_allocated = self.total_bytes_allocated.saturating_sub(layout.size());
            Some(layout)
        } else {
            None
        }
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

    /// Allocate a scalar SexprecCore node using slab pages.
    ///
    /// Returns a raw SEXP pointer to the allocated node.
    /// The pointer is valid for the lifetime of the arena.
    /// Uses page from slab (Vec with exact reserve) to avoid per-node Box overhead
    /// and improve locality (hard problem from review: one alloc per node was bad).
    pub(crate) fn alloc_node(&mut self, sexptype: SEXPTYPE) -> SEXP {
        if !self.can_activate_node() {
            return ptr::null_mut();
        }

        if let Some(ptr) = self.reuse_free_node(sexptype) {
            return ptr;
        }

        let active_nodes = self.active_addrs.len();
        let should_gc =
            active_nodes > GC_TRIGGER_THRESHOLD || self.total_bytes_allocated > GC_BYTE_THRESHOLD;
        if should_gc {
            crate::sexp::gengc::maybe_collect_during_alloc();
            if let Some(ptr) = self.reuse_free_node(sexptype) {
                return ptr;
            }
        }

        if !self.can_grow_bytes_by(std::mem::size_of::<SexprecCore>()) {
            return ptr::null_mut();
        }

        self.allocate_core_in_slab(|| SexprecCore::new(sexptype))
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
    pub(crate) fn alloc_vector(&mut self, sexptype: SEXPTYPE, length: R_xlen_t) -> SEXP {
        if length < 0 {
            return ptr::null_mut();
        }

        if self.total_bytes_allocated > GC_BYTE_THRESHOLD {
            crate::sexp::gengc::maybe_collect_during_alloc();
        }

        let elem_size = sexp_elem_size(sexptype);
        let total_bytes = match (length as usize).checked_mul(elem_size) {
            Some(n) => n,
            None => return ptr::null_mut(),
        };

        if !self.can_allocate_new_node_with_payload(total_bytes) {
            return ptr::null_mut();
        }

        let node_ptr = self.allocate_core_in_slab(|| SexprecCore::new_vector(sexptype, length));

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

            unsafe {
                (*node_ptr).gengc_next_node = data_ptr as SEXP;
            }

            self.register_data_buffer(data_ptr, layout);
        }

        self.total_bytes_allocated += std::mem::size_of::<SexprecCore>();
        self.register_new_node(node_ptr)
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
    pub(crate) fn alloc_vector_checked(
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
            crate::sexp::gengc::maybe_collect_during_alloc();
        }

        let node_ptr = self.allocate_core_in_slab(|| SexprecCore::new_vector(sexptype, length));

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
            unsafe {
                (*node_ptr).gengc_next_node = data_ptr as SEXP;
            }
            self.register_data_buffer(data_ptr, layout);
        }

        self.total_bytes_allocated += std::mem::size_of::<SexprecCore>();
        Ok(self.register_new_node(node_ptr))
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
    pub(crate) fn alloc_charsxp(&mut self, s: &[u8]) -> SEXP {
        let len = s.len() as R_xlen_t;

        let total_bytes = match (len as usize).checked_add(1) {
            Some(n) => n,
            None => return ptr::null_mut(),
        };
        if !self.can_allocate_new_node_with_payload(total_bytes) {
            return ptr::null_mut();
        }

        let node_ptr = self.allocate_core_in_slab(|| {
            let mut c = SexprecCore::new(SEXPTYPE::CHARSXP);
            c.data = SexprecData { charsxp_truelen: len };
            c
        });

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

        unsafe {
            (*node_ptr).gengc_next_node = data_ptr as SEXP;
        }

        self.register_data_buffer(data_ptr, layout);
        self.total_bytes_allocated += std::mem::size_of::<SexprecCore>();
        self.register_new_node(node_ptr)
    }

    /// Allocate a CHARSXP and return an arena-scoped safe wrapper.
    pub fn alloc_charsxp_sexp(&mut self, s: &[u8]) -> Option<Sexp<'_>> {
        let ptr = self.alloc_charsxp(s);
        self.sexp(ptr)
    }

    /// Allocate a cons cell (LISTSXP).
    pub(crate) fn cons(&mut self, car: SEXP, cdr: SEXP, tag: SEXP) -> SEXP {
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
    pub(crate) fn alloc_list_chain(&mut self, n: i32) -> SEXP {
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

    /// Add an existing node (for legacy compat in some paths). Pushes into current slab page
    /// (assumes caller ensures no overflow; for hard perf problem we prefer alloc_node).
    pub(crate) fn add_node(&mut self, node: Box<SexprecCore>) -> SEXP {
        if !self.can_allocate_new_node_with_payload(0) {
            return ptr::null_mut();
        }

        if self.slab_offset >= NODE_PAGE_SIZE {
            self.alloc_new_page();
        }
        let page = &mut self.node_pages[self.slab_page];
        // Note: this path is rare/legacy; to keep exact, we could transmute but for safety
        // we fall back to old style for add_node by pushing the inner. But since we removed Box storage,
        // extract and push the value (move out of Box).
        let core = *node; // move out
        page.push(core);
        let ptr: SEXP = &mut page[self.slab_offset] as *mut _;
        self.slab_offset += 1;
        self.total_bytes_allocated += std::mem::size_of::<SexprecCore>();
        self.register_new_node(ptr)
    }

    /// Get the number of nodes allocated in this arena.
    pub fn node_count(&self) -> usize {
        self.active_addrs.len()
    }

    /// Return true if this pointer is one of the arena's active nodes.
    pub(crate) fn contains(&self, ptr: SEXP) -> bool {
        if ptr.is_null() {
            return false;
        }
        self.active_addrs.contains(&(ptr as usize))
    }

    /// Wrap an active arena-owned pointer in a safe `Sexp`.
    ///
    /// Unlike raw construction, this checks that the pointer belongs to this
    /// arena and is not currently on the free list, tying the wrapper lifetime
    /// to the arena borrow.
    pub(crate) fn sexp(&self, ptr: SEXP) -> Option<Sexp<'_>> {
        if self.contains(ptr) {
            Sexp::from_arena_raw(ptr, self).ok()
        } else {
            None
        }
    }

    /// Iterate over all arena nodes (across slab pages).
    pub(crate) fn nodes(&self) -> impl Iterator<Item = SEXP> + '_ {
        self.node_pages.iter().flat_map(|page| {
            page.iter().map(|n| n as *const _ as SEXP)
        })
    }

    /// Iterate over nodes that are currently active, excluding free-list slots.
    pub(crate) fn active_nodes(&self) -> impl Iterator<Item = SEXP> + '_ {
        self.nodes()
            .filter(|ptr| self.active_addrs.contains(&(*ptr as usize)))
    }

    /// Free a node by adding it to the free list for reuse.
    pub(crate) fn free_node(&mut self, ptr: SEXP) {
        if ptr.is_null() {
            return;
        }
        if self.free_addrs.contains(&(ptr as usize)) {
            return;
        }
        if !self.active_addrs.contains(&(ptr as usize)) {
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
        self.track_node_freed(ptr);
    }

    /// Get the fragmentation ratio (freed / total capacity).
    pub fn fragmentation_ratio(&self) -> f64 {
        let total: usize = self.node_pages.iter().map(|p| p.capacity()).sum();
        if total == 0 {
            0.0
        } else {
            self.free_list.len() as f64 / total as f64
        }
    }

    /// Get mutable access to the free list for compaction operations.
    pub(crate) fn free_list_mut(&mut self) -> &mut Vec<SEXP> {
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
            for (&ptr, &layout) in &self.data_bufs {
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
        for (&ptr, &layout) in &self.data_bufs {
            if !ptr.is_null() && layout.size() > 0 {
                unsafe {
                    dealloc(ptr, layout);
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
    let Some(instance_ptr) = super::instance::current_instance_ptr() else {
        return super::instance::with_required_current_instance(|inst| with_arena_in(inst, f));
    };
    // Arena borrow tracking (depth) via the shared counter in instance.rs.
    // We bump the depth for visibility/diagnostics around this raw .arena lend.
    let _prev_arena = super::instance::INSTANCE_MUT_BORROW_DEPTH.with(|c| {
        let d = c.get();
        c.set(d + 1);
        d
    });
    let r = unsafe { f(&mut (*instance_ptr).arena) };
    super::instance::INSTANCE_MUT_BORROW_DEPTH.with(|c| c.set(c.get() - 1));
    r
}

pub(crate) fn with_arena_in<F, R>(inst: &mut super::instance::RInstance, f: F) -> R
where
    F: FnOnce(&mut RArena) -> R,
{
    f(&mut inst.arena)
}

/// Reset the active instance evaluation arena, freeing all allocations.
pub fn reset_arena() {
    super::instance::with_required_current_instance(reset_arena_in);
}

pub(crate) fn reset_arena_in(inst: &mut super::instance::RInstance) {
    inst.arena = RArena::new();
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

    #[test]
    fn test_arena_can_target_instance_explicitly() {
        let mut left = super::super::instance::RInstance::new();
        let mut right = super::super::instance::RInstance::new();
        let left_before = with_arena_in(&mut left, |arena| arena.node_count());
        let right_before = with_arena_in(&mut right, |arena| arena.node_count());

        let left_node = with_arena_in(&mut left, |arena| arena.alloc_node(SEXPTYPE::INTSXP));
        assert!(!left_node.is_null());
        assert_eq!(
            with_arena_in(&mut left, |arena| arena.node_count()),
            left_before + 1
        );
        assert_eq!(
            with_arena_in(&mut right, |arena| arena.node_count()),
            right_before
        );

        let right_node = with_arena_in(&mut right, |arena| arena.alloc_node(SEXPTYPE::REALSXP));
        assert!(!right_node.is_null());
        assert_eq!(
            with_arena_in(&mut left, |arena| arena.node_count()),
            left_before + 1
        );
        assert_eq!(
            with_arena_in(&mut right, |arena| arena.node_count()),
            right_before + 1
        );

        reset_arena_in(&mut left);
        assert_eq!(with_arena_in(&mut left, |arena| arena.node_count()), 0);
        assert_eq!(
            with_arena_in(&mut right, |arena| arena.node_count()),
            right_before + 1
        );
    }
}
