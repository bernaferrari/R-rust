#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Memory allocation for R objects.
//!
//! # Arena + generational GC
//!
//! Objects are allocated from a slab arena ([`RArena`]) that owns node pages
//! and vector payloads. That arena is **not** a "no-GC forever" pool: unreachable
//! objects are reclaimed by the generational mark-sweep collector in
//! [`super::gengc`]. Allocation may trigger collection when node/byte
//! thresholds trip; survivors promote young→old under write-barrier tracking.
//!
//! # Mandatory rooting
//!
//! Raw `SEXP` values held only in Rust locals are invisible to the collector.
//! Any pointer that must survive an allocating call (or an explicit `gc()`)
//! has to be rooted via [`super::protect`] (legacy PROTECT stack or the Rust
//! root table) or another traced root (environments, the bytecode operand
//! stack, remembered-set edges). Dropping roots because "there is no GC" is
//! use-after-free — do not treat this module as GC-free.

use std::alloc::Layout;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ptr::{self};
use std::rc::Rc;

/// Size of each slab page for SexprecCore nodes. Larger pages reduce allocator overhead
/// and improve cache locality vs one Box per node. Chose 4096 as balance ( ~256KB per page
/// assuming ~64B SexprecCore).
const NODE_PAGE_SIZE: usize = 4096;
const _: () = assert!(NODE_PAGE_SIZE % 64 == 0);

use super::ffi::{R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE, SexprecCore, SexprecData};
use super::heap::{CheckedNode, HeapIdentity, NodeId, NodePage, NodeProjection, PageMetadata};
use super::object::Sexp;
use super::payload::{OwnedPayload, PayloadError};

/// Byte size of one node for checking legacy projection address ranges.
const NODE_BYTES: usize = std::mem::size_of::<SexprecCore>();
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
    /// The type requires a different header or a dedicated constructor.
    InvalidVectorType { sexptype: SEXPTYPE },
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
            ArenaError::InvalidVectorType { sexptype } => {
                write!(f, "invalid vector type: {}", sexptype.as_c_int())
            }
        }
    }
}

impl std::error::Error for ArenaError {}

/// Validate the union header, platform-sized length and full payload layout
/// before either allocator changes accounting or allocates memory.
fn vector_layout(sexptype: SEXPTYPE, length: R_xlen_t) -> Result<Layout, ArenaError> {
    let elem_size = sexp_elem_size(sexptype);
    if elem_size == 0 {
        return Err(ArenaError::InvalidVectorType { sexptype });
    }
    let length = usize::try_from(length).map_err(|_| ArenaError::InvalidLength)?;
    let bytes = length
        .checked_mul(elem_size)
        .ok_or(ArenaError::InvalidLength)?;
    Layout::from_size_align(bytes, std::mem::align_of::<u64>())
        .map_err(|_| ArenaError::InvalidLength)
}

#[cfg(test)]
thread_local! {
    static BUFFER_ALLOCATION_ATTEMPTS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn buffer_allocation_attempts() -> usize {
    BUFFER_ALLOCATION_ATTEMPTS.with(Cell::get)
}

#[cfg(test)]
fn note_buffer_allocation_attempt() {
    BUFFER_ALLOCATION_ATTEMPTS.with(|count| count.set(count.get() + 1));
}

/// Own initialized transient bytes with the requested layout and at least
/// eight-byte alignment. R_alloc is the verified consumer of this facade.
pub(crate) struct OwnedBuffer {
    allocation: OwnedPayload,
    layout: Layout,
}

impl OwnedBuffer {
    pub(crate) fn zeroed(layout: Layout) -> Option<Self> {
        if layout.size() == 0 || layout.align() > 8 {
            return None;
        }
        #[cfg(test)]
        note_buffer_allocation_attempt();
        let allocation = OwnedPayload::zeroed_bytes(layout.size()).ok()?;
        Some(Self { allocation, layout })
    }

    pub(crate) fn as_ptr(&self) -> *mut u8 {
        self.allocation.as_ptr()
    }
    pub(crate) fn layout(&self) -> Layout {
        self.layout
    }
}

fn zeroed_vector_payload(kind: SEXPTYPE, length: R_xlen_t) -> Result<OwnedPayload, ArenaError> {
    #[cfg(test)]
    if length > 0 {
        note_buffer_allocation_attempt();
    }
    OwnedPayload::zeroed_vector(kind, length).map_err(|error| match error {
        PayloadError::Allocation => ArenaError::OutOfMemory,
        PayloadError::InvalidLength => ArenaError::InvalidLength,
        PayloadError::InvalidVectorType => ArenaError::InvalidVectorType { sexptype: kind },
    })
}

/// One native allocation shared by a checked number of live vector headers.
/// The arena owns the allocation; dropping its final header lease drops it.
struct SharedBuffer {
    allocation: OwnedPayload,
    headers: std::num::NonZeroUsize,
}

/// Budget for arena allocations to prevent unbounded growth.
///
/// A budget of `0` means unlimited for that dimension.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArenaBudget {
    /// Maximum accounted bytes allowed, including arena data and
    /// reservations for temporary native workspaces (0 = unlimited).
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
// This eliminates one source of O(n) in arena (buffer release and frequent freeing of vectors).
// Layouts are small, HashMap overhead acceptable vs scan on many vectors.

// ---------------------------------------------------------------------------
// RArena: arena allocator for R objects
// ---------------------------------------------------------------------------

/// Rust owns the node allocation and safe collector metadata. The `base`
/// address is used only by the legacy projection directory; slot pointers
/// always come from their own interior cell, never a sibling's pointer tag.
struct SlabPage {
    storage: NodePage<SexprecCore>,
    meta: Rc<PageMetadata>,
    _registration: NodePageRegistration,
}

#[derive(Clone)]
struct SlabMetadata {
    base: usize,
    meta: Rc<PageMetadata>,
    projection: NodeProjection<SexprecCore>,
}
impl SlabMetadata {
    fn slot(&self, address: usize) -> Option<usize> {
        let offset = address.checked_sub(self.base)?;
        (offset < NODE_BYTES * self.meta.slots() && offset.is_multiple_of(NODE_BYTES))
            .then_some(offset / NODE_BYTES)
    }
}

thread_local! {
    static SLAB_META: RefCell<BTreeMap<usize, SlabMetadata>> = RefCell::new(BTreeMap::new());
    /// Fast owned metadata lease for the most recently touched range.
    static SLAB_CACHE: RefCell<Option<SlabMetadata>> = const { RefCell::new(None) };
    static GC_EPOCH: Cell<u32> = const { Cell::new(0) };
}
/// An owned directory entry for an actual owned header page. Drop removes
/// only this page's identity, so a reused address cannot evict its successor.
pub(crate) struct NodePageRegistration {
    base: usize,
    meta: Rc<PageMetadata>,
}
impl Drop for NodePageRegistration {
    fn drop(&mut self) {
        let _ = SLAB_META.try_with(|map| {
            let mut map = map.borrow_mut();
            if map
                .get(&self.base)
                .is_some_and(|entry| Rc::ptr_eq(&entry.meta, &self.meta))
            {
                map.remove(&self.base);
            }
        });
        let _ = SLAB_CACHE.try_with(|cache| {
            let mut cache = cache.borrow_mut();
            if cache
                .as_ref()
                .is_some_and(|entry| entry.base == self.base && Rc::ptr_eq(&entry.meta, &self.meta))
            {
                *cache = None;
            }
        });
    }
}
pub(crate) fn register_node_page(page: &NodePage<SexprecCore>) -> NodePageRegistration {
    let base = page.raw_slot(0).expect("nonempty node page") as usize;
    let meta = page.metadata();
    let entry = SlabMetadata {
        base,
        meta: meta.clone(),
        projection: page.projection(),
    };
    SLAB_META.with(|map| {
        map.borrow_mut().insert(base, entry.clone());
    });
    SLAB_CACHE.with(|cache| {
        *cache.borrow_mut() = Some(entry);
    });
    NodePageRegistration { base, meta }
}
#[inline(always)]
fn find_slab_entry(pointer: SEXP) -> Option<(SlabMetadata, usize)> {
    if pointer.is_null() {
        return None;
    }
    let address = pointer as usize;
    let cached = SLAB_CACHE
        .try_with(|cache| {
            cache
                .borrow()
                .as_ref()
                .and_then(|entry| entry.slot(address).map(|slot| (entry.clone(), slot)))
        })
        .ok()
        .flatten();
    if cached.is_some() {
        return cached;
    }
    let entry = SLAB_META
        .try_with(|map| {
            map.borrow()
                .range(..=address)
                .next_back()
                .map(|(_, entry)| entry.clone())
        })
        .ok()
        .flatten()?;
    let slot = entry.slot(address)?;
    let _ = SLAB_CACHE.try_with(|cache| {
        *cache.borrow_mut() = Some(entry.clone());
    });
    Some((entry, slot))
}

#[inline(always)]
fn find_slab_slot(pointer: SEXP) -> Option<(Rc<PageMetadata>, usize)> {
    let (entry, slot) = find_slab_entry(pointer)?;
    Some((entry.meta, slot))
}

/// Resolve a live legacy projection to owned, generational metadata. The
/// token neither dereferences its header nor borrows its arena.
pub(crate) fn checked_node(pointer: SEXP) -> Option<CheckedNode> {
    let (meta, slot) = find_slab_slot(pointer)?;
    CheckedNode::new(meta.clone(), meta.current_id(slot)?)
}

/// Snapshot automatic handle roots from the one registered production heap.
/// Both arena and permanent pages use this directory and heap identity. The
/// directory borrow ends before any root is projected or passed to tracing.
pub(crate) fn automatic_roots(identity: &HeapIdentity) -> Vec<(SEXP, CheckedNode)> {
    let pages = SLAB_META
        .try_with(|map| {
            map.borrow()
                .values()
                .filter(|entry| entry.meta.belongs_to(identity))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    pages
        .iter()
        .flat_map(|entry| {
            entry.meta.rooted_ids().filter_map(move |id| {
                let (pointer, allocation) = entry.projection.resolve_slot(id.slot())?;
                (allocation.id() == &id).then_some((pointer, allocation))
            })
        })
        .collect()
}

/// Use the input address only to select an owned slot. The returned pointer
/// is freshly projected from that slot's canonical Cell, never from the
/// caller's potentially provenance-free or invalidated pointer tag.
pub(crate) fn checked_projection(pointer: SEXP) -> Option<(SEXP, CheckedNode)> {
    let (entry, slot) = find_slab_entry(pointer)?;
    entry.projection.resolve_slot(slot)
}

/// Copy the canonical header through its Cell only if the caller's original
/// allocation identity still belongs to this exact directory slot. An
/// address reused by another generation never refreshes the expected ID.
pub(crate) fn checked_snapshot(pointer: SEXP, expected: &CheckedNode) -> Option<SexprecCore> {
    let (entry, slot) = find_slab_entry(pointer)?;
    if expected.id().slot() != slot {
        return None;
    }
    entry.projection.copy_live(expected.id())
}

/// Result of testing one pointer against the current GC epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GcTouch {
    /// Arena node already marked in this collection.
    AlreadyMarked,
    /// Arena node marked by this call.
    NewlyMarked,
    /// Not a currently live node in the registered owned heap directory.
    OutsideArena,
}

/// Start a collection. Arena nodes marked after this compare equal to the new
/// epoch; the previous cycle's marks do not. `gctorture(TRUE)` runs a
/// collection on every allocation, and the young generation is not reclaimed
/// there, so the sweep must not walk those nodes just to clear a mark bit.
pub(crate) fn begin_gc_epoch() {
    let next = GC_EPOCH.get().wrapping_add(1);
    if next == 0 {
        SLAB_META.with(|map| {
            for entry in map.borrow().values() {
                entry.meta.clear_epochs();
            }
        });
        GC_EPOCH.set(1);
    } else {
        GC_EPOCH.set(next);
    }
}

pub(crate) fn current_gc_epoch() -> u32 {
    GC_EPOCH.get()
}

/// Test helper for epoch-marking any registered live owned node, including
/// permanent pages. Production collection uses exact-generation tokens.
#[inline(always)]
pub(crate) fn gc_touch(ptr: SEXP) -> GcTouch {
    let Some((meta, slot)) = find_slab_slot(ptr) else {
        return GcTouch::OutsideArena;
    };
    let epoch = GC_EPOCH.get();
    debug_assert!(
        epoch != 0,
        "gc_touch on an arena node before begin_gc_epoch"
    );
    match meta.mark(slot, epoch) {
        Some(true) => return GcTouch::AlreadyMarked,
        Some(false) => (),
        None => return GcTouch::OutsideArena,
    }
    GcTouch::NewlyMarked
}

/// Whether this arena node was [`gc_touch`]ed since the latest [`begin_gc_epoch`].
#[inline(always)]
pub(crate) fn arena_node_marked(ptr: SEXP) -> bool {
    let epoch = GC_EPOCH.get();
    if epoch == 0 {
        return false;
    }
    let Some((meta, slot)) = find_slab_slot(ptr) else {
        return false;
    };
    meta.epoch(slot) == Some(epoch)
}

/// Keep the old-generation bitmap aligned with `sxpinfo.gcgen`.
///
/// Called from [`SxpInfo::set_gcgen`](super::ffi::SxpInfo::set_gcgen). Pointers
/// outside registered live owned pages (such as header temporaries) are ignored.
pub(crate) fn note_slab_generation(ptr: SEXP, generation: u8) {
    let Some((meta, slot)) = find_slab_slot(ptr) else {
        return;
    };
    meta.set_old(slot, generation == 1);
}

/// Live or old-generation nodes in slab order. Full words collapse to a
/// pointer walk; zero words are skipped. `gctorture(TRUE)` used to hash every
/// slot on every allocation.
pub(crate) struct SlotIter<'a> {
    pages: &'a [SlabPage],
    old_only: bool,
    page: usize,
    slot: usize,
    slot_end: usize,
}

impl Iterator for SlotIter<'_> {
    type Item = SEXP;

    #[inline(always)]
    fn next(&mut self) -> Option<SEXP> {
        if self.slot < self.slot_end {
            let ptr = self.pages[self.page]
                .storage
                .raw_slot(self.slot)
                .expect("live page slot");
            self.slot += 1;
            return Some(ptr);
        }
        self.slow_next()
    }
}

impl SlotIter<'_> {
    #[inline(never)]
    fn slow_next(&mut self) -> Option<SEXP> {
        if !self.arm_next_run() {
            return None;
        }
        let ptr = self.pages[self.page]
            .storage
            .raw_slot(self.slot)
            .expect("live page slot");
        self.slot += 1;
        Some(ptr)
    }

    fn arm_next_run(&mut self) -> bool {
        let page_count = self.pages.len();
        let mut page = self.page;
        let mut slot = self.slot;
        while page < page_count {
            if slot >= NODE_PAGE_SIZE {
                page += 1;
                slot = 0;
                continue;
            }
            let meta = &self.pages[page].meta;
            if let Some((start, end)) = meta.next_run(self.old_only, slot) {
                self.page = page;
                self.slot = start;
                self.slot_end = end;
                return true;
            }
            page += 1;
            slot = 0;
        }
        self.page = page;
        self.slot = 0;
        self.slot_end = 0;
        false
    }
}

/// An arena allocator for R objects.
///
/// Allocates SexprecCore nodes and their associated vector data.
/// Collection retires individual allocation identities and reuses their slots.
/// Dropping the arena releases its owned pages and payloads.
pub struct RArena {
    /// Owned pages of interior cells keep legacy header projections stable.
    /// Node identities and collector metadata are independent of raw headers.
    node_pages: Vec<SlabPage>,
    heap_identity: HeapIdentity,
    /// Current page index for allocation (last page usually).
    slab_page: usize,
    /// Current offset within the slab_page (0 .. NODE_PAGE_SIZE).
    slab_offset: usize,
    /// All allocated data buffers. HashMap for O(1) lookup/remove (was Vec + linear .position).
    /// Key: data ptr; value: owned allocation and live header lease count.
    data_bufs: HashMap<*mut u8, SharedBuffer>,
    /// Free list of reclaimed SEXP pointers available for reuse.
    free_list: Vec<SEXP>,
    /// O(1) membership for active node pointers.
    active_addrs: HashSet<usize>,
    /// O(1) membership for free-list pointers.
    free_addrs: HashSet<usize>,
    /// Total bytes allocated for tracking.
    total_bytes_allocated: usize,
    /// Bytes reserved by temporary native workspaces which live outside the
    /// arena (for example, a numerical transform's scratch Vecs).
    transient_bytes: Rc<Cell<usize>>,
    /// Bytes promised to compact-sequence payloads queued during an arena
    /// lend and not yet in `data_bufs`. Shared so that promise can be made
    /// without borrowing the arena a second time.
    pending_data_bytes: Rc<Cell<usize>>,
    /// Deferred alloc-time GC hook state (see `with_arena_in`): arena
    /// methods cannot touch instance state without aliasing the live
    /// borrow, so hooks record their firings here instead.
    alloc_gc_torture_ticks: u32,
    alloc_gc_collect_requested: bool,
    /// Live size after the last collection. Triggers use *growth* since
    /// then so a large live heap does not GC on every `{`.
    nodes_at_last_gc: usize,
    bytes_at_last_gc: usize,
    /// Optional budget to limit arena growth.
    budget: ArenaBudget,
}

impl RArena {
    /// Allocate a new page in the slab. Reserves exactly to avoid realloc (stable ptrs inside).
    #[inline(always)]
    fn alloc_new_page(&mut self) {
        let storage = NodePage::try_new(
            self.heap_identity.clone(),
            self.node_pages.len(),
            NODE_PAGE_SIZE,
            || SexprecCore::new(SEXPTYPE::NILSXP),
        )
        .expect("arena node page allocation failed");
        let meta = storage.metadata();
        let registration = register_node_page(&storage);
        self.node_pages.push(SlabPage {
            storage,
            meta,
            _registration: registration,
        });
        self.slab_page = self.node_pages.len() - 1;
        self.slab_offset = 0;
    }

    /// Allocate a core node into the current slab page (creating new page if needed).
    /// Returns the raw SEXP ptr. Updates accounting and active tracking.
    /// Used by scalar/vector/CHARS allocs to avoid duplication (elegance + maintainability).
    #[inline(always)]
    fn allocate_core_in_slab<F>(&mut self, ctor: F) -> SEXP
    where
        F: FnOnce() -> SexprecCore,
    {
        while let Some(pointer) = self.free_list.pop() {
            let Some((page, slot)) = self.reusable_slot(pointer) else {
                continue;
            };
            let ptr = self.node_pages[page]
                .storage
                .replace_inactive(slot, ctor())
                .expect("reusable arena slot");
            return self.register_new_node(ptr);
        }
        if self.slab_offset >= NODE_PAGE_SIZE {
            self.alloc_new_page();
        }
        let ptr = self.node_pages[self.slab_page]
            .storage
            .replace_inactive(self.slab_offset, ctor())
            .expect("fresh inactive arena slot");
        self.slab_offset += 1;
        self.add_accounted_bytes(std::mem::size_of::<SexprecCore>());
        self.register_new_node(ptr)
    }

    /// Create a new empty arena with an unlimited budget.
    pub fn new() -> Self {
        Self::with_budget(ArenaBudget::unlimited())
    }

    /// Create a new empty arena with the given budget.
    pub fn with_budget(budget: ArenaBudget) -> Self {
        Self::with_budget_and_identity(budget, HeapIdentity::new())
    }

    /// Reset an arena fixture without changing the session's persistent heap
    /// domain. New page identities still invalidate all old arena handles.
    #[cfg(test)]
    pub(crate) fn fresh_with_identity(identity: HeapIdentity) -> Self {
        Self::with_budget_and_identity(ArenaBudget::unlimited(), identity)
    }

    fn with_budget_and_identity(budget: ArenaBudget, heap_identity: HeapIdentity) -> Self {
        let mut a = RArena {
            node_pages: Vec::new(),
            heap_identity,
            slab_page: 0,
            slab_offset: NODE_PAGE_SIZE,
            data_bufs: HashMap::new(),
            free_list: Vec::new(),
            active_addrs: HashSet::new(),
            free_addrs: HashSet::new(),
            total_bytes_allocated: 0,
            transient_bytes: Rc::new(Cell::new(0)),
            pending_data_bytes: Rc::new(Cell::new(0)),
            alloc_gc_torture_ticks: 0,
            alloc_gc_collect_requested: false,
            nodes_at_last_gc: 0,
            bytes_at_last_gc: 0,
            budget,
        };

        a.alloc_new_page();
        a
    }

    fn track_node_active(&mut self, ptr: SEXP) {
        if ptr.is_null() {
            return;
        }
        self.active_addrs.insert(ptr as usize);
        self.free_addrs.remove(&(ptr as usize));
        let Some((meta, slot)) = find_slab_slot(ptr) else {
            debug_assert!(ptr.is_null(), "active node is not in a slab page");
            return;
        };
        let id = meta
            .activate(slot, false)
            .expect("inactive nonretired arena node");
        let header = self.node_pages[meta.page()]
            .storage
            .copy_live(&id)
            .expect("initialized owned arena node");
        meta.set_old(slot, header.sxpinfo.gcgen() == 1);
        let allocation = CheckedNode::new(meta, id).expect("new live arena allocation");
        note_lend_allocation(std::ptr::from_ref(self).addr(), ptr, allocation);
    }

    fn track_node_freed(&mut self, ptr: SEXP) {
        if ptr.is_null() {
            return;
        }
        self.active_addrs.remove(&(ptr as usize));
        self.free_addrs.insert(ptr as usize);
        if let Some((meta, slot)) = find_slab_slot(ptr) {
            if let Some(id) = meta.current_id(slot) {
                meta.release(&id);
            }
        }
    }

    fn reusable_slot(&self, pointer: SEXP) -> Option<(usize, usize)> {
        let (metadata, slot) = find_slab_slot(pointer)?;
        let page = self.node_pages.get(metadata.page())?;
        (Rc::ptr_eq(&page.meta, &metadata) && metadata.reusable(slot))
            .then_some((metadata.page(), slot))
    }

    fn fresh_header_bytes(&self) -> usize {
        if self
            .free_list
            .iter()
            .rev()
            .any(|&pointer| self.reusable_slot(pointer).is_some())
        {
            0
        } else {
            NODE_BYTES
        }
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
        note_lend_budget(std::ptr::from_ref(self) as usize, budget.max_bytes);
    }

    /// Reserve native scratch space against this session's byte budget.
    ///
    /// The returned guard releases the reservation on every exit path,
    /// including an R error or cancellation unwind. The allocation itself is
    /// owned by the caller; this only accounts for its peak workspace.
    pub(crate) fn try_reserve_transient(&mut self, bytes: usize) -> Option<TransientReservation> {
        TransientReservation::new(
            self.budget.max_bytes,
            self.total_bytes_allocated,
            self.pending_data_bytes.get(),
            &self.transient_bytes,
            bytes,
        )
    }

    fn register_data_buffer(&mut self, allocation: OwnedPayload) {
        let ptr = allocation.as_ptr();
        assert!(
            !self.data_bufs.contains_key(&ptr),
            "buffer ownership transferred twice"
        );
        assert!(!ptr.is_null(), "nonempty buffer ownership");
        self.add_accounted_bytes(allocation.layout().size());
        self.data_bufs.insert(
            ptr,
            SharedBuffer {
                allocation,
                headers: std::num::NonZeroUsize::new(1).unwrap(),
            },
        );
    }

    fn add_accounted_bytes(&mut self, bytes: usize) {
        self.total_bytes_allocated = self.total_bytes_allocated.saturating_add(bytes);
        LEND_LEDGER.with(|slot| {
            if let Some(ledger) = slot
                .borrow()
                .iter()
                .rev()
                .find(|ledger| ledger.arena == std::ptr::from_ref(self) as usize)
            {
                ledger.total.set(ledger.total.get().saturating_add(bytes));
            }
        });
    }

    fn sub_accounted_bytes(&mut self, bytes: usize) {
        self.total_bytes_allocated = self.total_bytes_allocated.saturating_sub(bytes);
        LEND_LEDGER.with(|slot| {
            if let Some(ledger) = slot
                .borrow()
                .iter()
                .rev()
                .find(|ledger| ledger.arena == std::ptr::from_ref(self) as usize)
            {
                ledger.total.set(ledger.total.get().saturating_sub(bytes));
            }
        });
    }

    /// Move initialized payload ownership into the arena. A rejected payload
    /// is dropped without publishing its pointer or changing accounting.
    fn adopt_data_buffer(&mut self, allocation: OwnedPayload) -> bool {
        if allocation.as_ptr().is_null() || !self.can_grow_bytes_by(allocation.layout().size()) {
            return false;
        }
        self.register_data_buffer(allocation);
        true
    }

    #[cfg(all(test, feature = "altrep"))]
    pub(crate) fn tracks_altrep_test_buffer(&self, ptr: *mut u8) -> bool {
        self.tracks_data_buffer(ptr)
    }

    fn tracks_data_buffer(&self, ptr: *mut u8) -> bool {
        !ptr.is_null() && self.data_bufs.contains_key(&ptr)
    }

    fn release_data_buffer(&mut self, ptr: *mut u8) {
        let Some(buffer) = self.data_bufs.get_mut(&ptr) else {
            return;
        };
        if let Some(remaining) = std::num::NonZeroUsize::new(buffer.headers.get() - 1) {
            buffer.headers = remaining;
            return;
        }
        let buffer = self
            .data_bufs
            .remove(&ptr)
            .expect("registered final buffer owner");
        self.sub_accounted_bytes(buffer.allocation.layout().size());
        // Dropping the typed owner releases its final shared allocation.
        drop(buffer);
    }

    /// Share a payload between checked live headers in this arena. No callback
    /// runs during the lend. Type, shape, ownership and publication are checked
    /// together; neither header can free storage still retained by the other.
    #[cfg(feature = "altrep")]
    pub(crate) fn share_vector_payload(
        &mut self,
        source: &Sexp<'_>,
        target: &Sexp<'_>,
    ) -> super::object::SexpResult<()> {
        use super::object::SexpError;
        let error = || SexpError::Altrep {
            reason: "invalid shared vector payload",
        };
        let source_ptr = source.clone().as_raw();
        let target_ptr = target.clone().as_raw();
        if source_ptr == target_ptr || !self.contains(source_ptr) || !self.contains(target_ptr) {
            return Err(error());
        }
        let src = source.header();
        let dst = target.header();
        if !src.sxpinfo.type_of().is_vector_type()
            || src.sxpinfo.type_of() != dst.sxpinfo.type_of()
            || source.len() != target.len()
            || !dst.payload.is_null()
        {
            return Err(error());
        }
        let bytes = usize::try_from(source.len())
            .ok()
            .and_then(|n| n.checked_mul(sexp_elem_size(source.typeof_())))
            .ok_or_else(error)?;
        if bytes == 0 {
            return Ok(());
        }
        let buffer = self
            .data_bufs
            .get_mut(&(src.payload as *mut u8))
            .ok_or_else(error)?;
        if buffer.allocation.layout().size() < bytes {
            return Err(error());
        }
        buffer.headers = buffer.headers.checked_add(1).ok_or_else(error)?;
        // SAFETY: both rooted headers have matching shape and a tracked buffer
        // with a newly installed ownership lease; no payload loans are present.
        unsafe {
            (*target_ptr).gengc_next_node = src.payload;
        }
        Ok(())
    }

    fn can_activate_node(&self) -> bool {
        self.budget.max_nodes == 0 || self.node_count() < self.budget.max_nodes
    }

    fn can_grow_bytes_by(&self, bytes: usize) -> bool {
        self.budget.max_bytes == 0
            || self
                .total_bytes_allocated
                .checked_add(self.transient_bytes.get())
                .and_then(|total| total.checked_add(self.pending_data_bytes.get()))
                .and_then(|total| total.checked_add(bytes))
                .is_some_and(|total| total <= self.budget.max_bytes)
    }

    fn can_allocate_node_with_payload(&self, bytes: usize) -> bool {
        self.can_activate_node()
            && bytes
                .checked_add(self.fresh_header_bytes())
                .is_some_and(|total| self.can_grow_bytes_by(total))
    }

    /// Allocate a scalar SexprecCore node using slab pages.
    ///
    /// Returns a raw SEXP pointer to the allocated node.
    /// The pointer is valid for the lifetime of the arena.
    /// Uses page from slab (Vec with exact reserve) to avoid per-node Box overhead
    /// and improve locality (hard problem from review: one alloc per node was bad).
    #[inline(always)]
    pub(crate) fn alloc_node(&mut self, sexptype: SEXPTYPE) -> SEXP {
        self.alloc_gc_torture_ticks = self.alloc_gc_torture_ticks.wrapping_add(1);
        if !self.can_activate_node() {
            return ptr::null_mut();
        }

        if self.growth_warrants_gc() {
            self.alloc_gc_collect_requested = true;
        }

        if !self.can_allocate_node_with_payload(0) {
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
    #[inline(always)]
    pub(crate) fn alloc_vector(&mut self, sexptype: SEXPTYPE, length: R_xlen_t) -> SEXP {
        let Ok(layout) = vector_layout(sexptype, length) else {
            return ptr::null_mut();
        };

        self.alloc_gc_torture_ticks = self.alloc_gc_torture_ticks.wrapping_add(1);

        if self.growth_warrants_gc() {
            self.alloc_gc_collect_requested = true;
        }

        let total_bytes = layout.size();

        if !self.can_allocate_node_with_payload(total_bytes) {
            return ptr::null_mut();
        }

        let data = if total_bytes > 0 {
            let Ok(data) = zeroed_vector_payload(sexptype, length) else {
                return ptr::null_mut();
            };
            Some(data)
        } else {
            None
        };

        let node_ptr = self.allocate_core_in_slab(|| SexprecCore::new_vector(sexptype, length));

        if let Some(data) = data {
            let data_ptr = data.as_ptr();
            self.register_data_buffer(data);
            unsafe {
                (*node_ptr).gengc_next_node = data_ptr as SEXP;
            }
        }

        node_ptr
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
        let layout = vector_layout(sexptype, length)?;

        self.alloc_gc_torture_ticks = self.alloc_gc_torture_ticks.wrapping_add(1);

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

        let data_bytes = layout.size();
        let total_increase = data_bytes
            .checked_add(self.fresh_header_bytes())
            .ok_or(ArenaError::InvalidLength)?;

        // Check byte budget
        if self.budget.max_bytes > 0 {
            let new_total = self
                .total_bytes_allocated
                .checked_add(self.transient_bytes.get())
                .and_then(|total| total.checked_add(self.pending_data_bytes.get()))
                .and_then(|total| total.checked_add(total_increase))
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
        if self.growth_warrants_gc() {
            self.alloc_gc_collect_requested = true;
        }

        let data = if data_bytes > 0 {
            Some(zeroed_vector_payload(sexptype, length)?)
        } else {
            None
        };

        let node_ptr = self.allocate_core_in_slab(|| SexprecCore::new_vector(sexptype, length));

        if let Some(data) = data {
            let data_ptr = data.as_ptr();
            self.register_data_buffer(data);
            unsafe {
                (*node_ptr).gengc_next_node = data_ptr as SEXP;
            }
        }

        Ok(node_ptr)
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
        self.alloc_gc_torture_ticks = self.alloc_gc_torture_ticks.wrapping_add(1);
        let len = s.len() as R_xlen_t;
        let total_bytes = match (len as usize).checked_add(1) {
            Some(n) => n,
            None => return ptr::null_mut(),
        };
        if !self.can_allocate_node_with_payload(total_bytes) {
            return ptr::null_mut();
        }

        #[cfg(test)]
        note_buffer_allocation_attempt();
        let Ok(data) = OwnedPayload::characters(s) else {
            return ptr::null_mut();
        };
        let data_ptr = data.as_ptr();

        // CHARSXP shares the vector header prefix. Initialize true length as
        // well: writing only charsxp_truelen leaves that accessor uninitialized.
        let node_ptr = self.allocate_core_in_slab(|| {
            let mut c = SexprecCore::new(SEXPTYPE::CHARSXP);
            c.data = SexprecData {
                vecsxp: super::ffi::Vecsxp {
                    length: len,
                    truelength: 0,
                },
            };
            // GNU CHARSXP gp bits: ASCII (1<<6) and/or UTF8 (1<<3).
            // Serialization writes these via PackFlags(LEVELS(s)).
            let mut gp = 0u16;
            if s.is_ascii() {
                gp |= 1 << 6;
            } else if std::str::from_utf8(s).is_ok() {
                gp |= 1 << 3;
            }
            c.sxpinfo.set_gp(gp);
            c
        });

        self.register_data_buffer(data);
        unsafe {
            (*node_ptr).gengc_next_node = data_ptr as SEXP;
        }

        node_ptr
    }

    /// Allocate a CHARSXP and return an arena-scoped safe wrapper.
    pub fn alloc_charsxp_sexp(&mut self, s: &[u8]) -> Option<Sexp<'_>> {
        let ptr = self.alloc_charsxp(s);
        self.sexp(ptr)
    }

    /// Allocate a cons cell (LISTSXP).
    ///
    /// # Safety
    /// Each non-null child is initialized and remains live in this owner
    /// (or immutable storage) for the lifetime of the resulting graph.
    pub(crate) unsafe fn cons(&mut self, car: SEXP, cdr: SEXP, tag: SEXP) -> SEXP {
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

    /// Check a graph child before storing it in this arena.
    pub(crate) fn accepts_child(&self, value: &Sexp<'_>) -> bool {
        let ptr = value.clone().as_raw();
        self.contains(ptr) || super::session::is_immutable_singleton(ptr)
    }

    /// Allocate a cons cell from safe wrappers and return an arena-scoped
    /// wrapper for the new cell.
    pub fn cons_sexp<'a>(
        &'a mut self,
        car: Sexp<'_>,
        cdr: Sexp<'_>,
        tag: Option<Sexp<'_>>,
    ) -> Option<Sexp<'a>> {
        if !self.accepts_child(&car)
            || !self.accepts_child(&cdr)
            || tag.as_ref().is_some_and(|tag| !self.accepts_child(tag))
        {
            return None;
        }
        // SAFETY: all children belong to this arena or immutable storage.
        // Keep the source handles live until the parent is fully initialized.
        let ptr = unsafe {
            self.cons(
                car.clone().as_raw(),
                cdr.clone().as_raw(),
                tag.as_ref()
                    .map_or(ptr::null_mut(), |tag| tag.clone().as_raw()),
            )
        };
        self.sexp(ptr)
    }

    /// Allocate a nil-terminated pairlist chain of n elements.
    ///
    /// Matches GNU `allocList`: `n <= 0` yields `R_NilValue`, and the final
    /// CDR is `R_NilValue` (not a null pointer). `serialize` already maps a
    /// null CDR to `NILVALUE_SXP`, but `identical()` and most list walks treat
    /// null and Nil as distinct without this terminator.
    pub(crate) fn alloc_list_chain(&mut self, n: i32) -> SEXP {
        if n <= 0 {
            return unsafe { crate::sexp::globals::R_NilValue() };
        }
        let mut result: SEXP = unsafe { crate::sexp::globals::R_NilValue() };
        for _ in 0..n {
            // SAFETY: each predecessor was allocated in this arena; no GC runs during this lend.
            result = unsafe { self.cons(ptr::null_mut(), result, ptr::null_mut()) };
            if result.is_null() {
                return ptr::null_mut();
            }
        }
        result
    }

    /// Add an existing node (for legacy compat in some paths). Pushes into current slab page
    /// (assumes caller ensures no overflow; for hard perf problem we prefer alloc_node).
    /// # Safety
    /// Payload pointers and graph children are valid and owned by this arena
    /// or immutable storage; transferring the core must not duplicate payload ownership.
    pub(crate) unsafe fn add_node(&mut self, node: Box<SexprecCore>) -> SEXP {
        if !self.can_allocate_node_with_payload(0) {
            return ptr::null_mut();
        }

        self.allocate_core_in_slab(|| *node)
    }

    /// Get the number of nodes allocated in this arena.
    pub fn node_count(&self) -> usize {
        self.active_addrs.len()
    }

    /// True when enough *new* nodes/bytes have appeared since the last
    /// collection. A large live set must not re-trigger GC by itself.
    pub(crate) fn growth_warrants_gc(&self) -> bool {
        self.node_count().saturating_sub(self.nodes_at_last_gc) > GC_TRIGGER_THRESHOLD
            || self
                .total_bytes_allocated
                .saturating_sub(self.bytes_at_last_gc)
                > GC_BYTE_THRESHOLD
    }

    pub(crate) fn note_gc_completed(&mut self) {
        self.nodes_at_last_gc = self.node_count();
        self.bytes_at_last_gc = self.total_bytes_allocated;
    }

    /// Return true if this pointer is one of the arena's active nodes.
    pub(crate) fn contains(&self, ptr: SEXP) -> bool {
        if ptr.is_null() {
            return false;
        }
        self.active_addrs.contains(&(ptr as usize))
    }

    pub(crate) fn heap_identity(&self) -> HeapIdentity {
        self.heap_identity.clone()
    }

    pub(crate) fn node_token(&self, pointer: SEXP) -> Option<CheckedNode> {
        if !self.contains(pointer) {
            return None;
        }
        checked_node(pointer)
    }

    pub(crate) fn node_projection(&self, pointer: SEXP) -> Option<(SEXP, CheckedNode)> {
        if !self.contains(pointer) {
            return None;
        }
        checked_projection(pointer)
    }

    /// Identity of the currently live allocation at a legacy projection.
    /// Capturing an address after reuse identifies its new allocation; callers
    /// retain the returned ID to reject stale handles on later access.
    pub(crate) fn node_id(&self, pointer: SEXP) -> Option<NodeId> {
        self.node_token(pointer).map(|token| token.id().clone())
    }

    /// Resolve an unforgeable identity only while its exact allocation lives.
    pub(crate) fn resolve_node(&self, id: &NodeId) -> Option<SEXP> {
        self.node_pages.get(id.page())?.storage.resolve(id)
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
        self.node_pages
            .iter()
            .enumerate()
            .flat_map(move |(page_idx, page)| {
                // Allocation fills pages sequentially: only the current page
                // can be partially occupied.
                let used = if page_idx == self.slab_page {
                    self.slab_offset
                } else {
                    NODE_PAGE_SIZE
                };
                (0..used).map(move |i| page.storage.raw_slot(i).expect("allocated page slot"))
            })
    }

    /// Live nodes in slab order, skipping free-list holes.
    pub(crate) fn active_nodes(&self) -> SlotIter<'_> {
        self.slot_iter(false)
    }

    /// Old-generation nodes in slab order.
    ///
    /// Torture collections reclaim only this set. Young nodes stay live until a
    /// safe-point collection, so walking them on every `gctorture(TRUE)`
    /// allocation dominated long runs.
    pub(crate) fn old_nodes(&self) -> SlotIter<'_> {
        self.slot_iter(true)
    }

    fn slot_iter(&self, old_only: bool) -> SlotIter<'_> {
        SlotIter {
            pages: &self.node_pages,
            old_only,
            page: 0,
            slot: 0,
            slot_end: 0,
        }
    }

    /// Free a node by adding it to the free list for reuse.
    #[inline(always)]
    /// # Safety
    /// No reachable graph edge or Rust payload borrow may refer to this node.
    /// Unrooted checked metadata handles become stale and reject later access.
    pub(crate) unsafe fn free_node(&mut self, ptr: SEXP) {
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
                self.release_data_buffer(data_ptr);
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

    /// Get the fragmentation ratio among allocated node slots.
    pub fn fragmentation_ratio(&self) -> f64 {
        let total = self.active_addrs.len() + self.free_list.len();
        if total == 0 {
            0.0
        } else {
            self.free_list.len() as f64 / total as f64
        }
    }

    /// Normalize the reusable-node list and rebuild its membership index.
    pub(crate) fn normalize_free_list(&mut self) {
        self.free_list.sort_by_key(|&p| p as usize);
        self.free_list.dedup();
        self.free_addrs.clear();
        for &ptr in &self.free_list {
            if !ptr.is_null() {
                self.free_addrs.insert(ptr as usize);
            }
        }
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
            for (&ptr, buffer) in &self.data_bufs {
                if !ptr.is_null() {
                    debug_assert!(buffer.allocation.layout().size() > 0);
                }
            }
            for &free_ptr in &self.free_list {
                debug_assert!(!free_ptr.is_null());
            }
            true
        });
    }
}

/// RAII release token for a native workspace reservation.
pub(crate) struct TransientReservation {
    counter: Rc<Cell<usize>>,
    bytes: usize,
}

impl TransientReservation {
    fn new(
        max_bytes: usize,
        accounted: usize,
        pending: usize,
        counter: &Rc<Cell<usize>>,
        bytes: usize,
    ) -> Option<Self> {
        let reserved = counter.get().checked_add(bytes)?;
        let total = accounted.checked_add(pending)?.checked_add(reserved)?;
        if max_bytes != 0 && total > max_bytes {
            return None;
        }
        counter.set(reserved);
        Some(Self {
            counter: Rc::clone(counter),
            bytes,
        })
    }
}

impl Drop for TransientReservation {
    fn drop(&mut self) {
        // The shared counter can outlive or move independently of the arena.
        // Rc also confines this token to its creating thread.
        self.counter.set(self.counter.get() - self.bytes);
    }
}

impl Default for RArena {
    fn default() -> Self {
        Self::new()
    }
}

// Registered buffers and slab pages own their allocations through RAII.
// RArena needs no custom deallocator: field destruction releases each once.

// ---------------------------------------------------------------------------
// Instance evaluation arena
// ---------------------------------------------------------------------------

/// Access the active instance evaluation arena.
///
/// Allocation is intentionally scoped to an `RInstance`: unscoped arena
/// fallback would let objects escape the session that owns evaluator state,
/// which breaks Android multi-instance isolation.
///
/// The arena view is derived through `with_current_instance`, which guards
/// ambient mutable instance access with the borrow-depth monitor instead of
/// fabricating an independent `&mut RArena` from the raw current-instance
/// pointer (which could alias a live outer `&mut RInstance`).
///
/// # Safety
/// The active owner is live and exclusively available for the arena lend.
/// The callback must not reenter R or mutate/read the arena through an alias.
pub unsafe fn with_arena<F, R>(f: F) -> R
where
    F: FnOnce(&mut RArena) -> R,
{
    super::instance::with_required_current_instance(|inst| unsafe { with_arena_in(inst, f) })
}

thread_local! {
    static LENT_ARENAS: std::cell::RefCell<std::collections::HashSet<usize>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
    /// Buffers attached to a vector while its arena was already lent.
    /// `with_arena_in` registers them after the lend ends and before deferred GC.
    static PENDING_DATA_BUFFERS: std::cell::RefCell<Vec<PendingDataBuffer>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

struct PendingDataBuffer {
    instance: usize,
    node: SEXP,
    token: CheckedNode,
    allocation: OwnedPayload,
    /// Owned accounting lease on the arena's pending byte counter.
    reservation: TransientReservation,
}

struct LendLedger {
    instance: usize,
    arena: usize,
    max_bytes: usize,
    total: Cell<usize>,
    transient: Rc<Cell<usize>>,
    pending: Rc<Cell<usize>>,
    fresh: Vec<(SEXP, CheckedNode)>,
}

thread_local! {
    /// One entry per `with_arena_in`, including a nested call that is about
    /// to be rejected. Popping restores the outer lend's snapshot.
    static LEND_LEDGER: RefCell<Vec<LendLedger>> = const { RefCell::new(Vec::new()) };
}

fn install_lend_ledger(inst: *mut super::instance::RInstance) {
    let ledger = unsafe {
        let arena = &(*inst).arena;
        LendLedger {
            instance: inst as usize,
            arena: std::ptr::from_ref(arena) as usize,
            max_bytes: arena.budget.max_bytes,
            total: Cell::new(arena.total_bytes_allocated),
            transient: Rc::clone(&arena.transient_bytes),
            pending: Rc::clone(&arena.pending_data_bytes),
            fresh: Vec::new(),
        }
    };
    LEND_LEDGER.with(|slot| slot.borrow_mut().push(ledger));
}

/// Keep the exact allocation alive until its lend finishes dispatching
/// deferred collection and callbacks. The arena supplies its own identity;
/// no instance field is accessed while the mutable arena borrow is live.
fn note_lend_allocation(arena: usize, projection: SEXP, allocation: CheckedNode) {
    LEND_LEDGER.with(|slot| {
        if let Some(ledger) = slot
            .borrow_mut()
            .iter_mut()
            .rev()
            .find(|ledger| ledger.arena == arena)
        {
            ledger.fresh.push((projection, allocation));
        }
    });
}

/// Snapshot temporary roots for the collector's heap domain. The original
/// tokens reject freed/reused slots; lookup never refreshes them by address.
/// The owned snapshot releases its TLS borrow before tracing or callbacks.
pub(crate) fn fresh_allocation_roots(identity: &HeapIdentity) -> Vec<(SEXP, CheckedNode)> {
    LEND_LEDGER
        .try_with(|slot| {
            slot.borrow()
                .iter()
                .flat_map(|ledger| &ledger.fresh)
                .filter(|(_, allocation)| allocation.belongs_to(identity) && allocation.is_live())
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Reserve transient bytes without borrowing an already-lent arena again.
///
/// # Safety
/// `inst` is a live owner. Any outstanding arena borrow is registered by
/// `with_arena_in`; no other instance field borrow overlaps this operation.
pub(crate) unsafe fn reserve_transient_in(
    inst: *mut super::instance::RInstance,
    bytes: usize,
) -> Option<TransientReservation> {
    if is_arena_lent(inst) {
        LEND_LEDGER.with(|slot| {
            let ledgers = slot.borrow();
            let ledger = ledgers
                .iter()
                .rev()
                .find(|ledger| ledger.instance == inst as usize)?;
            TransientReservation::new(
                ledger.max_bytes,
                ledger.total.get(),
                ledger.pending.get(),
                &ledger.transient,
                bytes,
            )
        })
    } else {
        // Strictly local accounting: no evaluation, allocation hooks or GC.
        unsafe { (*inst).arena.try_reserve_transient(bytes) }
    }
}

fn note_lend_budget(arena: usize, max_bytes: usize) {
    LEND_LEDGER.with(|slot| {
        if let Some(ledger) = slot
            .borrow_mut()
            .iter_mut()
            .rev()
            .find(|ledger| ledger.arena == arena)
        {
            ledger.max_bytes = max_bytes;
        }
    });
}

/// Promise `bytes` against the active lend's budget.
///
/// Returns none when the promise does not fit. The caller must not publish
/// a pointer in that case: the lend's flush would otherwise free a buffer
/// the caller is still holding.
fn reserve_lend_bytes(
    inst: *mut super::instance::RInstance,
    bytes: usize,
) -> Option<TransientReservation> {
    LEND_LEDGER.with(|slot| {
        let ledger_ref = slot.borrow();
        let ledger = ledger_ref
            .iter()
            .rev()
            .find(|ledger| ledger.instance == inst.addr())?;
        if ledger.max_bytes != 0 {
            let total = ledger
                .total
                .get()
                .checked_add(ledger.transient.get())
                .and_then(|total| total.checked_add(ledger.pending.get()))
                .and_then(|total| total.checked_add(bytes))?;
            if total > ledger.max_bytes {
                return None;
            }
        }
        ledger.pending.set(ledger.pending.get().checked_add(bytes)?);
        Some(TransientReservation {
            counter: ledger.pending.clone(),
            bytes,
        })
    })
}

struct ClearLendLedger;
impl Drop for ClearLendLedger {
    fn drop(&mut self) {
        let _ = LEND_LEDGER.try_with(|slot| {
            slot.borrow_mut().pop();
        });
    }
}

/// Zero-fill `bytes` and attach the pointer as `node`'s vector payload.
///
/// When the arena is already lent, the pointer is queued instead of
/// re-entering [`with_arena`]. The bytes are reserved first. If the budget
/// cannot hold them, nothing is published and this returns null. The lend's
/// `with_arena_in` registers a published buffer before deferred collection
/// and does not free it.
///
/// # Safety
/// `node` is a live vector header owned by the active instance, with no prior
/// payload. `bytes > 0` is its complete payload size; the caller roots the node.
pub(crate) unsafe fn attach_zeroed_data_buffer(node: SEXP, bytes: usize) -> *mut u8 {
    unsafe { attach_initialized_data_buffer(node, bytes, |_| {}) }
}

/// Construct and initialize a typed payload before publishing it to any
/// header, pending queue or callback. Initialization failure drops the sole
/// owner and restores its byte reservation, leaving the lazy node unchanged.
///
/// # Safety
/// `node` is a rooted live vector in the active owner, without an existing
/// payload or overlapping payload borrow. `initialize` writes only valid
/// elements of this vector's complete `bytes` region; it does not retain the
/// pointer or reenter R while an arena lend is active.
pub(crate) unsafe fn attach_initialized_data_buffer(
    node: SEXP,
    bytes: usize,
    initialize: impl FnOnce(*mut u8),
) -> *mut u8 {
    unsafe {
        let Some((node, token)) = checked_projection(node) else {
            return ptr::null_mut();
        };
        if bytes == 0 || !(*node).gengc_next_node.is_null() {
            return ptr::null_mut();
        }
        let kind = (*node).sxpinfo.type_of();
        let length = (*node).vecsxp_length();
        let Ok(layout) = vector_layout(kind, length) else {
            return ptr::null_mut();
        };
        if layout.size() != bytes {
            return ptr::null_mut();
        }
        let Some(inst) = super::instance::current_instance_ptr() else {
            return ptr::null_mut();
        };
        // Admit the whole payload before allocation or zero-fill. The guard
        // also restores accounting if allocation or bookkeeping fails.
        let Some(reservation) = reserve_transient_in(inst, layout.size()) else {
            return ptr::null_mut();
        };
        let lent = is_arena_lent(inst);
        if lent && !PENDING_DATA_BUFFERS.with(|queue| queue.borrow_mut().try_reserve(1).is_ok()) {
            return ptr::null_mut();
        }
        let Ok(buffer) = zeroed_vector_payload(kind, length) else {
            return ptr::null_mut();
        };
        let data_ptr = buffer.as_ptr();
        initialize(data_ptr);
        // Transfer the reservation to the lend's pending ledger, or to the
        // arena's registered bytes. No R reentry occurs during this transfer.
        drop(reservation);
        if lent {
            let Some(reservation) = reserve_lend_bytes(inst, layout.size()) else {
                return ptr::null_mut();
            };
            PENDING_DATA_BUFFERS.with(|queue| {
                queue.borrow_mut().push(PendingDataBuffer {
                    instance: inst.addr(),
                    node,
                    token,
                    allocation: buffer,
                    reservation,
                });
            });
            (*node).gengc_next_node = data_ptr as SEXP;
            return data_ptr;
        }
        let adopted = with_arena(|arena| {
            if !arena.adopt_data_buffer(buffer) {
                return false;
            }
            // Publish before any deferred callback can observe or unwind
            // ownership transferred to the arena.
            (*node).gengc_next_node = data_ptr as SEXP;
            true
        });
        if !adopted {
            return ptr::null_mut();
        }
        data_ptr
    }
}

/// True when `node`'s payload was attached during a lend and not yet accepted.
pub(crate) unsafe fn vector_payload_is_pending(node: SEXP) -> bool {
    unsafe {
        if node.is_null() {
            return false;
        }
        let ptr = (*node).gengc_next_node as *mut u8;
        if ptr.is_null() {
            return false;
        }
        PENDING_DATA_BUFFERS.with(|queue| {
            queue.borrow().iter().any(|item| {
                item.node == node && item.token.is_live() && item.allocation.as_ptr() == ptr
            })
        })
    }
}

/// True when `node`'s payload is in the arena map or waiting for the lend to end.
pub(crate) unsafe fn vector_payload_is_tracked(node: SEXP) -> bool {
    unsafe {
        if node.is_null() {
            return false;
        }
        let ptr = (*node).gengc_next_node as *mut u8;
        if ptr.is_null() {
            return false;
        }
        if let Some(inst) = super::instance::current_instance_ptr() {
            let queued = PENDING_DATA_BUFFERS.with(|queue| {
                queue.borrow().iter().any(|item| {
                    item.instance == inst as usize
                        && item.token.is_live()
                        && item.allocation.as_ptr() == ptr
                })
            });
            if queued {
                return true;
            }
            if is_arena_lent(inst) {
                return false;
            }
        }
        with_arena(|arena| arena.tracks_data_buffer(ptr))
    }
}

fn flush_pending_data_buffers(inst: *mut super::instance::RInstance) {
    let key = inst as usize;
    let mine = PENDING_DATA_BUFFERS.with(|queue| {
        let mut queue = queue.borrow_mut();
        let mut kept = Vec::new();
        let mut mine = Vec::new();
        for item in queue.drain(..) {
            if item.instance == key {
                mine.push(item);
            } else {
                kept.push(item);
            }
        }
        *queue = kept;
        mine
    });
    // A nested `with_arena` is rejected before its lend starts, so this flush
    // can run while the outer lend is still active. Those buffers belong to
    // the outer lend; committing them here would free or register them early.
    if is_arena_lent(inst) {
        PENDING_DATA_BUFFERS.with(|queue| {
            queue.borrow_mut().extend(mine);
        });
        return;
    }
    unsafe {
        for item in mine {
            let Some((node, token)) = checked_projection(item.node) else {
                continue;
            };
            if token != item.token || (*node).gengc_next_node as *mut u8 != item.allocation.as_ptr()
            {
                continue;
            }
            // Transfer the accounting lease before registering the moved
            // typed owner. Stale/replaced headers simply drop both leases.
            drop(item.reservation);
            (*inst).arena.register_data_buffer(item.allocation);
            super::altseq::commit_expanded_buffer(node);
        }
    }
}

struct FlushPending(*mut super::instance::RInstance);
impl Drop for FlushPending {
    fn drop(&mut self) {
        flush_pending_data_buffers(self.0);
    }
}
struct ArenaLend(usize);
impl ArenaLend {
    fn new(instance: *mut super::instance::RInstance) -> Self {
        let key = instance.addr();
        assert!(
            LENT_ARENAS.with(|lent| lent.borrow_mut().insert(key)),
            "reentrant mutable arena access; release the arena before calling the interpreter"
        );
        Self(key)
    }
}
pub(crate) fn is_arena_lent(instance: *mut super::instance::RInstance) -> bool {
    LENT_ARENAS.with(|lent| lent.borrow().contains(&instance.addr()))
}

impl Drop for ArenaLend {
    fn drop(&mut self) {
        LENT_ARENAS.with(|lent| {
            lent.borrow_mut().remove(&self.0);
        });
    }
}

/// # Safety
/// `inst` is a live writable owner pointer; no overlapping arena/instance
/// borrow exists. The callback must not reenter R through an alias.
pub(crate) unsafe fn with_arena_in<F, R>(inst: *mut super::instance::RInstance, f: F) -> R
where
    F: FnOnce(&mut RArena) -> R,
{
    // P1: the `&mut RArena` lend below is arena-local by construction —
    // arena methods defer their GC firings (alloc_gc_torture_ticks /
    // alloc_gc_collect_requested) instead of touching instance state, so
    // nothing reenters the interpreter while the lend is live. The
    // deferred firings are processed only after it is released.
    unsafe {
        // Snapshot the budget before the `&mut RArena` lend. Compact-sequence
        // expansion during the callback reserves through this ledger instead
        // of borrowing the arena again.
        let liveness = super::instance::instance_liveness(inst);
        install_lend_ledger(inst);
        let _clear_ledger = ClearLendLedger;
        let result = {
            // Drop flushes after the lend ends, including when `f` unwinds,
            // and before deferred collection below. The ledger outlives the
            // flush so reserved bytes stay in step with the arena.
            let _flush = FlushPending(inst);
            let result = {
                let _lend = ArenaLend::new(inst);
                f(&mut (*inst).arena)
            };
            result
        };
        let torture_ticks = std::mem::take(&mut (*inst).arena.alloc_gc_torture_ticks);
        let collect_requested = std::mem::take(&mut (*inst).arena.alloc_gc_collect_requested);
        crate::eval::parser::flush_literal_warnings();
        if liveness.is_live() {
            crate::sexp::gengc::process_deferred_alloc_gc_in(
                inst,
                torture_ticks,
                collect_requested,
            );
        }
        result
    }
}

/// This is identical to with_arena but named for clarity in GC context.
pub unsafe fn with_arena_for_gc<F, R>(f: F) -> R
where
    F: FnOnce(&mut RArena) -> R,
{
    unsafe { with_arena(f) }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #[test]
    fn automatic_roots_span_registered_arena_and_permanent_pages_in_one_heap() {
        let mut arena = super::RArena::new();
        let identity = arena.heap_identity();
        let mut permanent =
            super::super::instance::persistent::PersistentHeap::new(identity.clone());
        let arena_node = arena.alloc_node(super::SEXPTYPE::LISTSXP);
        let arena_token = arena.node_token(arena_node).unwrap();
        let arena_lease = arena_token.root_lease().unwrap();
        let permanent_node = permanent
            .allocate_header(super::SexprecCore::new(super::SEXPTYPE::ENVSXP))
            .unwrap();
        let permanent_token = permanent.token(permanent_node).unwrap();
        let permanent_lease = permanent_token.root_lease().unwrap();
        let mut foreign = super::RArena::new();
        let foreign_node = foreign.alloc_node(super::SEXPTYPE::INTSXP);
        let foreign_token = foreign.node_token(foreign_node).unwrap();
        let foreign_lease = foreign_token.root_lease().unwrap();
        let roots = super::automatic_roots(&identity);
        assert_eq!(roots.len(), 2);
        assert!(roots.contains(&(arena_node, arena_token.clone())));
        assert!(roots.contains(&(permanent_node, permanent_token.clone())));
        assert!(!roots.contains(&(foreign_node, foreign_token.clone())));
        drop(permanent_lease);
        assert_eq!(
            super::automatic_roots(&identity),
            [(arena_node, arena_token.clone())]
        );
        super::SLAB_META.with(|pages| {
            let _directory_borrow = pages.borrow();
            drop(arena_lease);
        });
        assert!(super::automatic_roots(&identity).is_empty());
        let retained_metadata = arena_token.root_lease().unwrap();
        drop(arena);
        assert!(!arena_token.is_live());
        assert!(super::automatic_roots(&identity).is_empty());
        drop(retained_metadata);
        drop(foreign_lease);
    }

    #[test]
    fn fresh_lend_allocations_survive_nested_collection_notifications() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let session = super::super::session::RSession::new_for_gc_tests();
        let notifications = Arc::new(AtomicUsize::new(0));
        let calls = notifications.clone();
        session.with_active(|| unsafe {
            super::super::instance::with_required_current_instance(|owner| {
                (*owner).memory_state.gc_force_gap = 1;
                (*owner).memory_state.gc_force_wait = 1;
            });
            super::super::gengc::register_gc_callback(Box::new(move |_| {
                if calls.fetch_add(1, Ordering::SeqCst) != 0 {
                    return;
                }
                let identity = super::super::instance::with_required_current_instance(|owner| {
                    (*owner).arena.heap_identity()
                });
                let original = super::fresh_allocation_roots(&identity);
                assert_eq!(original.len(), 4);
                super::super::gengc::full_gc();
                assert!(original.iter().all(|(_, token)| token.is_live()));
                let nested = super::with_arena(|arena| {
                    let node = arena.alloc_vector(super::SEXPTYPE::REALSXP, 3);
                    arena.node_token(node).unwrap()
                });
                assert!(nested.is_live());
                // The nested lend has returned, so its scope is gone while
                // the outer allocation-return scope remains rooted.
                super::super::gengc::full_gc();
                assert!(!nested.is_live());
                assert!(original.iter().all(|(_, token)| token.is_live()));
            }));
            let original = super::with_arena(|arena| {
                let nodes = [
                    arena.alloc_node(super::SEXPTYPE::INTSXP),
                    arena.alloc_vector(super::SEXPTYPE::REALSXP, 3),
                    arena.alloc_charsxp(b"fresh allocation"),
                    arena.alloc_node(super::SEXPTYPE::LISTSXP),
                ];
                nodes.map(|node| arena.node_token(node).unwrap())
            });
            assert_eq!(notifications.load(Ordering::SeqCst), 1);
            assert!(original.iter().all(super::CheckedNode::is_live));
            super::super::gengc::full_gc();
            assert!(original.iter().all(|token| !token.is_live()));
        });
    }

    #[test]
    fn fresh_lend_scope_is_removed_after_notification_panic() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let session = super::super::session::RSession::new_for_gc_tests();
        let panic_once = Arc::new(AtomicBool::new(true));
        session.with_active(|| unsafe {
            let identity = super::super::instance::with_required_current_instance(|owner| {
                (*owner).memory_state.gc_force_gap = 1;
                (*owner).memory_state.gc_force_wait = 1;
                (*owner).arena.heap_identity()
            });
            super::super::gengc::register_gc_callback(Box::new(move |_| {
                if panic_once.swap(false, Ordering::SeqCst) {
                    panic!("injected fresh allocation notification panic");
                }
            }));
            let mut abandoned = None;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                super::with_arena(|arena| {
                    let node = arena.alloc_vector(super::SEXPTYPE::INTSXP, 3);
                    abandoned = arena.node_token(node);
                    node
                })
            }));
            assert!(result.is_err());
            assert!(super::fresh_allocation_roots(&identity).is_empty());
            let abandoned = abandoned.unwrap();
            assert!(abandoned.is_live());
            super::super::gengc::full_gc();
            assert!(!abandoned.is_live());
        });
    }

    #[test]
    fn fresh_lend_roots_preserve_exact_generations_and_heap_domains() {
        let mut left = super::super::instance::RInstance::new_for_gc_tests();
        let mut right = super::super::instance::RInstance::new_for_gc_tests();
        let left_identity = left.arena.heap_identity();
        let right_identity = right.arena.heap_identity();
        unsafe {
            super::with_arena_in(&mut left, |arena| {
                let first = arena.alloc_node(super::SEXPTYPE::INTSXP);
                let old = arena.node_token(first).unwrap();
                arena.free_node(first);
                let replacement = arena.alloc_node(super::SEXPTYPE::REALSXP);
                assert_eq!(replacement, first);
                let current = arena.node_token(replacement).unwrap();
                let roots = super::fresh_allocation_roots(&left_identity);
                assert_eq!(roots.len(), 1);
                assert_eq!(roots[0].1, current);
                assert!(!old.is_live());
                assert!(super::fresh_allocation_roots(&right_identity).is_empty());
                super::with_arena_in(&mut right, |other| {
                    let foreign = other.alloc_node(super::SEXPTYPE::SYMSXP);
                    let foreign_token = other.node_token(foreign).unwrap();
                    assert_eq!(
                        super::fresh_allocation_roots(&right_identity)[0].1,
                        foreign_token
                    );
                    assert_eq!(super::fresh_allocation_roots(&left_identity)[0].1, current);
                });
                assert!(super::fresh_allocation_roots(&right_identity).is_empty());
                assert_eq!(super::fresh_allocation_roots(&left_identity)[0].1, current);
            });
        }
        assert!(super::fresh_allocation_roots(&left_identity).is_empty());
        assert!(super::fresh_allocation_roots(&right_identity).is_empty());
    }

    #[test]
    fn fresh_lend_scope_is_removed_after_constructor_unwind() {
        let mut owner = super::super::instance::RInstance::new_for_gc_tests();
        let identity = owner.arena.heap_identity();
        let mut abandoned = None;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::with_arena_in(&mut owner, |arena| {
                let node = arena.alloc_charsxp(b"abandoned constructor");
                abandoned = arena.node_token(node);
                assert_eq!(super::fresh_allocation_roots(&identity).len(), 1);
                panic!("injected constructor unwind");
            });
        }));
        assert!(result.is_err());
        assert!(super::fresh_allocation_roots(&identity).is_empty());
        unsafe {
            super::super::gengc::full_gc_in(&mut owner);
        }
        assert!(!abandoned.unwrap().is_live());
    }

    #[test]
    fn standalone_arena_allocations_do_not_accumulate_temporary_roots() {
        let mut arena = super::RArena::new();
        let identity = arena.heap_identity();
        for _ in 0..3 {
            arena.alloc_node(super::SEXPTYPE::INTSXP);
        }
        assert!(super::fresh_allocation_roots(&identity).is_empty());
    }

    #[test]
    fn typed_scratch_storage_preserves_requested_layout_and_views_after_owner_moves() {
        for alignment in [1, 2, 4, 8] {
            let layout = std::alloc::Layout::from_size_align(17 * 8, alignment).unwrap();
            let buffer = super::OwnedBuffer::zeroed(layout).unwrap();
            let pointer = buffer.as_ptr();
            assert_eq!(pointer as usize % 8, 0);
            assert_eq!(buffer.layout(), layout);
            let mut owners = vec![buffer];
            owners.reserve(64);
            // SAFETY: scratch storage is initialized, at least eight-byte
            // aligned and owns the complete seventeen-u64 region. No byte or
            // typed view overlaps this temporary mutable view.
            unsafe {
                let values = std::slice::from_raw_parts_mut(pointer.cast::<u64>(), 17);
                assert!(values.iter().all(|value| *value == 0));
                for (index, value) in values.iter_mut().enumerate() {
                    *value = index as u64 + 1;
                }
                assert_eq!(values[16], 17);
            }
            assert_eq!(owners[0].layout().size(), 136);
        }
        let before = super::buffer_allocation_attempts();
        assert!(
            super::OwnedBuffer::zeroed(std::alloc::Layout::from_size_align(0, 8).unwrap())
                .is_none()
        );
        assert!(
            super::OwnedBuffer::zeroed(std::alloc::Layout::from_size_align(16, 16).unwrap())
                .is_none()
        );
        assert_eq!(super::buffer_allocation_attempts(), before);
    }

    #[test]
    fn user_tls_owned_arena_survives_directory_shutdown_order() {
        struct LateArena {
            arena: super::RArena,
            pointer: super::SEXP,
            token: super::CheckedNode,
        }
        impl Drop for LateArena {
            fn drop(&mut self) {
                assert!(self.token.is_live());
                assert_eq!(self.arena.node_count(), 1);
                // The owning page is still live, but the already-destroyed
                // directory must return no projection instead of panicking.
                assert!(super::checked_projection(self.pointer).is_none());
                assert!(super::checked_snapshot(self.pointer, &self.token).is_none());
            }
        }
        thread_local! {
            static USER_ARENA: std::cell::RefCell<Option<LateArena>> = const { std::cell::RefCell::new(None) };
        }
        std::thread::spawn(|| {
            // Initialize the user's TLS destructor before the runtime's
            // directory TLS. The directory then shuts down first.
            USER_ARENA.with(|slot| {
                let mut arena = super::RArena::new();
                let pointer = arena.alloc_node(super::SEXPTYPE::INTSXP);
                assert!(super::checked_node(pointer).is_some());
                let token = arena.node_token(pointer).unwrap();
                *slot.borrow_mut() = Some(LateArena {
                    arena,
                    pointer,
                    token,
                });
            });
        })
        .join()
        .expect("owned arena cleanup must tolerate destroyed directory TLS");
    }

    #[test]
    fn owned_header_snapshots_reject_stale_foreign_and_wrong_slot_tokens() {
        let mut arena = super::RArena::new();
        let first = arena.alloc_node(super::SEXPTYPE::INTSXP);
        let second = arena.alloc_node(super::SEXPTYPE::REALSXP);
        let first_id = arena.node_token(first).unwrap();
        let second_id = arena.node_token(second).unwrap();
        let input = std::ptr::without_provenance_mut::<super::SexprecCore>(first.addr());
        assert_eq!(
            super::checked_snapshot(input, &first_id)
                .unwrap()
                .sxpinfo
                .type_of(),
            super::SEXPTYPE::INTSXP
        );
        assert!(super::checked_snapshot(input, &second_id).is_none());
        let mut foreign = super::RArena::new();
        let other = foreign.alloc_node(super::SEXPTYPE::INTSXP);
        assert!(super::checked_snapshot(input, &foreign.node_token(other).unwrap()).is_none());
        // SAFETY: first has no reachable edge or payload loan. The snapshot
        // and expected metadata lease do not borrow its header storage.
        unsafe {
            arena.free_node(first);
        }
        let replacement = arena.alloc_node(super::SEXPTYPE::SYMSXP);
        assert_eq!(replacement, first);
        assert!(super::checked_snapshot(input, &first_id).is_none());
        let current = arena.node_token(replacement).unwrap();
        assert_eq!(
            super::checked_snapshot(input, &current)
                .unwrap()
                .sxpinfo
                .type_of(),
            super::SEXPTYPE::SYMSXP
        );
        drop(arena);
        assert!(super::checked_snapshot(input, &current).is_none());
    }

    #[test]
    fn prepared_payload_unwind_restores_ownership_and_budget_before_publication() {
        let session = super::super::session::RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let node = super::with_arena(|arena| arena.alloc_vector(super::SEXPTYPE::INTSXP, 0));
            let _root = super::super::protect::protect(node);
            (*node).data = super::SexprecData {
                vecsxp: super::super::ffi::Vecsxp {
                    length: 3,
                    truelength: 3,
                },
            };
            for lent in [false, true] {
                let initialize = || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        super::attach_initialized_data_buffer(node, 12, |_| {
                            panic!("injected initialization unwind")
                        });
                    }));
                    assert!(result.is_err());
                    assert!((*node).gengc_next_node.is_null());
                };
                if lent {
                    super::with_arena(|arena| {
                        let before = arena.total_bytes_allocated;
                        initialize();
                        assert_eq!(arena.total_bytes_allocated, before);
                        assert_eq!(arena.transient_bytes.get(), 0);
                        assert_eq!(arena.pending_data_bytes.get(), 0);
                        assert!(arena.data_bufs.is_empty());
                    });
                } else {
                    initialize();
                }
                assert!(super::PENDING_DATA_BUFFERS.with(|queue| queue.borrow().is_empty()));
            }
            let data = super::attach_initialized_data_buffer(node, 12, |data| {
                let values = std::slice::from_raw_parts_mut(data.cast::<i32>(), 3);
                values.copy_from_slice(&[4, 5, 6]);
            });
            assert_eq!(
                std::slice::from_raw_parts(data.cast::<i32>(), 3),
                &[4, 5, 6]
            );
            assert!(super::vector_payload_is_tracked(node));
        });
    }

    #[test]
    fn queued_payload_cannot_attach_to_a_reused_header_generation() {
        let session = super::super::session::RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let replacement = super::with_arena(|arena| {
                let node = arena.alloc_vector(super::SEXPTYPE::INTSXP, 0);
                (*node).data = super::SexprecData {
                    vecsxp: super::super::ffi::Vecsxp {
                        length: 3,
                        truelength: 3,
                    },
                };
                let old = arena.node_token(node).unwrap();
                assert!(!super::attach_zeroed_data_buffer(node, 12).is_null());
                assert_eq!(arena.pending_data_bytes.get(), 12);
                // The fixture has no root/graph edge or payload reference;
                // the queue retains metadata and payload ownership, not a
                // live header borrow. Explicit reclamation expires its ID.
                arena.free_node(node);
                let replacement = arena.alloc_vector(super::SEXPTYPE::REALSXP, 0);
                assert_eq!(replacement, node);
                assert!(!old.is_live());
                replacement
            });
            assert!((*replacement).gengc_next_node.is_null());
            assert_eq!((*replacement).sxpinfo.type_of(), super::SEXPTYPE::REALSXP);
            super::with_arena(|arena| {
                assert_eq!(arena.pending_data_bytes.get(), 0);
                assert!(arena.data_bufs.is_empty());
                assert_eq!(arena.total_bytes_allocated, super::NODE_BYTES);
            });
        });
    }

    #[test]
    fn typed_payload_projections_cover_chunk_boundaries_and_owner_moves() {
        for kind in [
            super::SEXPTYPE::RAWSXP,
            super::SEXPTYPE::INTSXP,
            super::SEXPTYPE::REALSXP,
            super::SEXPTYPE::CPLXSXP,
            super::SEXPTYPE::VECSXP,
        ] {
            let mut arena = super::RArena::new();
            let node = arena.alloc_vector(kind, 17);
            let data = unsafe { (*node).gengc_next_node as *mut u8 };
            assert_eq!(data as usize % 8, 0);
            let mut owners = vec![arena];
            owners.reserve(64);
            // SAFETY: moved Rc owners retain the entire initialized typed
            // allocation. Every access is within the seventeen logical
            // elements, and no competing payload borrow is held.
            unsafe {
                match kind {
                    super::SEXPTYPE::RAWSXP => {
                        let values = std::slice::from_raw_parts_mut(data, 17);
                        for (index, value) in values.iter_mut().enumerate() {
                            *value = index as u8;
                        }
                        assert_eq!(values[16], 16);
                    }
                    super::SEXPTYPE::INTSXP => {
                        let values = std::slice::from_raw_parts_mut(data.cast::<i32>(), 17);
                        for (index, value) in values.iter_mut().enumerate() {
                            *value = index as i32;
                        }
                        assert_eq!(values[16], 16);
                    }
                    super::SEXPTYPE::REALSXP => {
                        let values = std::slice::from_raw_parts_mut(data.cast::<f64>(), 17);
                        for (index, value) in values.iter_mut().enumerate() {
                            *value = index as f64;
                        }
                        assert_eq!(values[16], 16.0);
                    }
                    super::SEXPTYPE::CPLXSXP => {
                        let values =
                            std::slice::from_raw_parts_mut(data.cast::<super::Rcomplex>(), 17);
                        for (index, value) in values.iter_mut().enumerate() {
                            value.r = index as f64;
                            value.i = -(index as f64);
                        }
                        assert_eq!(values[16], super::Rcomplex { r: 16.0, i: -16.0 });
                    }
                    super::SEXPTYPE::VECSXP => {
                        let values = std::slice::from_raw_parts_mut(data.cast::<super::SEXP>(), 17);
                        assert!(values.iter().all(|value| value.is_null()));
                        values[16] = node;
                        assert_eq!(values[16], node);
                    }
                    _ => unreachable!(),
                }
            }
            assert!(owners[0].tracks_data_buffer(data));
        }
    }

    #[test]
    fn all_header_allocations_reuse_slots_and_charge_only_payload_growth() {
        let mut arena =
            super::RArena::with_budget(super::ArenaBudget::new(super::NODE_BYTES + 16, 1));
        let first = arena.alloc_node(super::SEXPTYPE::LISTSXP);
        let old = arena.node_token(first).unwrap();
        // SAFETY: these unrooted fixture allocations have no external graph
        // edge or payload borrow; checked metadata leases can become stale.
        unsafe {
            arena.free_node(first);
        }
        for checked in [false, true] {
            let vector = if checked {
                arena
                    .alloc_vector_checked(super::SEXPTYPE::INTSXP, 3)
                    .unwrap()
            } else {
                arena.alloc_vector(super::SEXPTYPE::INTSXP, 3)
            };
            assert_eq!(vector, first);
            assert_eq!(arena.total_bytes_allocated, super::NODE_BYTES + 12);
            assert!(!old.is_live());
            unsafe {
                arena.free_node(vector);
            }
            assert_eq!(arena.total_bytes_allocated, super::NODE_BYTES);
        }
        let chars = arena.alloc_charsxp(b"abcdefghijklmno");
        assert_eq!(chars, first);
        assert_eq!(arena.total_bytes_allocated, super::NODE_BYTES + 16);
        unsafe {
            arena.free_node(chars);
        }
        let zero = arena.alloc_vector(super::SEXPTYPE::REALSXP, 0);
        assert_eq!(zero, first);
        assert_eq!(arena.total_bytes_allocated, super::NODE_BYTES);
        unsafe {
            arena.free_node(zero);
        }
        let boxed = Box::new(super::SexprecCore::new(super::SEXPTYPE::SYMSXP));
        assert_eq!(unsafe { arena.add_node(boxed) }, first);
        assert_eq!(arena.node_count(), 1);
        assert_eq!(arena.node_pages.len(), 1);
        assert_eq!(arena.slab_offset, 1);
    }

    #[test]
    fn checked_projection_recovers_canonical_provenance_from_address_only_inputs() {
        let mut arena = super::RArena::new();
        let first = arena.alloc_node(super::SEXPTYPE::INTSXP);
        let second = arena.alloc_node(super::SEXPTYPE::REALSXP);
        for expected in [first, second] {
            let input = std::ptr::without_provenance_mut::<super::SexprecCore>(expected.addr());
            let (canonical, token) = super::checked_projection(input).unwrap();
            assert_eq!(canonical, expected);
            assert!(token.is_live());
            assert_eq!(arena.node_projection(input).unwrap().0, expected);
            // SAFETY: checked_projection re-derives the original live Cell
            // pointer; the address-only input itself is never dereferenced.
            unsafe {
                assert!(!(*canonical).sxpinfo.mark());
                (*canonical).sxpinfo.set_mark(true);
                assert!((*canonical).sxpinfo.mark());
                // A genuine previously projected alias remains valid when
                // the directory derives another pointer to the same Cell.
                assert!((*expected).sxpinfo.mark());
            }
        }
        let input = std::ptr::without_provenance_mut::<super::SexprecCore>(first.addr());
        // SAFETY: the fixture owns first, and no header/payload borrow remains.
        unsafe {
            arena.free_node(first);
        }
        assert!(super::checked_projection(input).is_none());
        drop(arena);
        assert!(super::checked_projection(input).is_none());
    }

    #[test]
    fn legacy_node_projection_survives_owned_page_moves() {
        let page = super::NodePage::try_new(super::HeapIdentity::new(), 0, 2, || {
            super::SexprecCore::new(super::SEXPTYPE::NILSXP)
        })
        .unwrap();
        let pointer = page
            .replace_inactive(1, super::SexprecCore::new(super::SEXPTYPE::INTSXP))
            .unwrap();
        page.metadata().activate(1, false).unwrap();
        let token = page.token(1).unwrap();
        let mut pages = Vec::new();
        pages.push(page);
        // SAFETY: the live Cell header belongs to pages; moving its owned
        // Rc handle must preserve the original projection's access rights.
        unsafe {
            assert_eq!((*pointer).sxpinfo.type_of(), super::SEXPTYPE::INTSXP);
        }
        pages.reserve(64);
        assert!(token.is_live());
        // SAFETY: reserve moved the page handles, and no header borrow is held.
        unsafe {
            (*pointer).sxpinfo.set_mark(true);
            assert!((*pointer).sxpinfo.mark());
        }
    }

    #[test]
    fn node_directory_accepts_only_exact_slots_and_retires_page_tokens() {
        let page = super::NodePage::try_new(super::HeapIdentity::new(), 0, 3, || {
            super::SexprecCore::new(super::SEXPTYPE::NILSXP)
        })
        .unwrap();
        let registration = super::register_node_page(&page);
        let metadata = page.metadata();
        for slot in 0..3 {
            metadata.activate(slot, false).unwrap();
        }
        let base = page.raw_slot(0).unwrap();
        let last = page.raw_slot(2).unwrap();
        assert!(super::checked_node(base).is_some());
        assert!(super::checked_node(last).is_some());
        let interior = (base as usize + 1) as super::SEXP;
        let past_end = (base as usize + 3 * super::NODE_BYTES) as super::SEXP;
        assert!(super::checked_node(interior).is_none());
        assert!(super::checked_node(past_end).is_none());
        let token = super::checked_node(last).unwrap();
        drop(page);
        assert!(!token.is_live());
        assert!(super::checked_node(last).is_none());
        drop(registration);
        assert!(super::find_slab_slot(base).is_none());
    }

    #[test]
    fn production_arena_identities_reject_reuse_foreign_arenas_and_drop() {
        let mut arena = super::RArena::new();
        let pointer = arena.alloc_node(super::SEXPTYPE::INTSXP);
        let old = arena.node_token(pointer).unwrap();
        let old_id = old.id().clone();
        assert_eq!(arena.resolve_node(&old_id), Some(pointer));
        let foreign = super::RArena::new();
        assert_eq!(foreign.resolve_node(&old_id), None);
        assert!(foreign.node_token(pointer).is_none());
        // SAFETY: this fixture owns the live node and holds no header/payload
        // reference. Checked metadata leases do not borrow its storage.
        unsafe {
            arena.free_node(pointer);
        }
        assert!(!old.is_live());
        assert_eq!(arena.resolve_node(&old_id), None);
        let replacement = arena.alloc_node(super::SEXPTYPE::REALSXP);
        assert_eq!(replacement, pointer);
        let new = arena.node_token(replacement).unwrap();
        assert_ne!(old, new);
        assert!(new.is_live());
        assert_eq!(arena.resolve_node(&old_id), None);
        drop(arena);
        assert!(!new.is_live());
        assert!(super::checked_node(pointer).is_none());
    }

    #[test]
    fn vector_allocation_validates_platform_length_and_layout_without_consuming_budget() {
        let mut arena = super::RArena::new();
        let before = (arena.node_count(), arena.total_bytes_allocated);
        for kind in [super::SEXPTYPE::RAWSXP, super::SEXPTYPE::REALSXP] {
            for length in [-1, i64::MAX] {
                assert!(arena.alloc_vector_sexp(kind, length).is_none());
                assert!(matches!(
                    arena.alloc_vector_checked_sexp(kind, length),
                    Err(super::ArenaError::InvalidLength)
                ));
                assert_eq!((arena.node_count(), arena.total_bytes_allocated), before);
            }
        }
        #[cfg(target_pointer_width = "32")]
        for length in [1_i64 << 32, (1_i64 << 32) + 1] {
            assert!(
                arena
                    .alloc_vector_sexp(super::SEXPTYPE::RAWSXP, length)
                    .is_none()
            );
            assert!(matches!(
                arena.alloc_vector_checked_sexp(super::SEXPTYPE::RAWSXP, length),
                Err(super::ArenaError::InvalidLength)
            ));
            assert_eq!((arena.node_count(), arena.total_bytes_allocated), before);
        }
    }

    #[test]
    fn vector_allocation_preserves_all_supported_types_and_zero_lengths() {
        let mut arena = super::RArena::new();
        for tag in 0..=31 {
            let kind = super::SEXPTYPE::from(tag);
            if super::sexp_elem_size(kind) == 0 {
                continue;
            }
            for length in [0, 1, 8] {
                for checked in [false, true] {
                    let value = if checked {
                        arena.alloc_vector_checked_sexp(kind, length).unwrap()
                    } else {
                        arena.alloc_vector_sexp(kind, length).unwrap()
                    };
                    assert_eq!(value.typeof_(), kind);
                    assert_eq!(value.len(), length);
                }
            }
        }
    }

    #[test]
    fn vector_allocation_rejects_incompatible_headers_without_consuming_budget() {
        let mut arena = super::RArena::new();
        let before = (arena.node_count(), arena.total_bytes_allocated);
        for tag in -1..=100 {
            let kind = super::SEXPTYPE::from(tag);
            if super::sexp_elem_size(kind) != 0 {
                continue;
            }
            for length in [0, 1, 8] {
                assert!(arena.alloc_vector_sexp(kind, length).is_none(), "tag {tag}");
                assert!(
                    arena.alloc_vector_checked_sexp(kind, length).is_err(),
                    "tag {tag}"
                );
                assert_eq!((arena.node_count(), arena.total_bytes_allocated), before);
            }
        }
    }

    #[test]
    fn transient_reservations_share_limits_and_release_without_arena_borrows() {
        let mut arena = super::RArena::with_budget(super::ArenaBudget::new(1024, 0));
        let reservation = arena.try_reserve_transient(1024).unwrap();
        assert!(arena.try_reserve_transient(1).is_none());
        assert!(!arena.can_grow_bytes_by(1));
        assert!(
            arena
                .alloc_vector_checked(super::SEXPTYPE::REALSXP, 1)
                .is_err()
        );
        drop(reservation);
        assert!(arena.can_grow_bytes_by(1024));
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _reservation = arena.try_reserve_transient(1024).unwrap();
            panic!("workspace failure");
        }));
        assert!(unwound.is_err());
        assert!(arena.try_reserve_transient(1024).is_some());
        // Safe guards must remain safe if their arena moves or is destroyed.
        let reservation = arena.try_reserve_transient(128).unwrap();
        let moved = Box::new(arena);
        drop(moved);
        drop(reservation);
    }
    use crate::sexp::ffi::*;

    use super::*;

    #[test]
    fn nested_arena_lend_is_rejected_and_outer_borrow_survives() {
        let _session = crate::sexp::session::RSession::new_for_gc_tests();
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|outer| {
                let before = outer.node_count();
                let failure = std::panic::catch_unwind(|| with_arena(|_| ()));
                assert!(failure.is_err());
                outer.alloc_node(SEXPTYPE::LISTSXP);
                assert_eq!(outer.node_count(), before + 1);
            })
        });
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena(|arena| {
                arena.alloc_node(SEXPTYPE::LISTSXP);
            })
        });
    }

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
    fn growth_warrants_gc_uses_delta_since_last_collection() {
        let mut arena = RArena::new();
        for _ in 0..=GC_TRIGGER_THRESHOLD {
            assert!(!arena.alloc_node(SEXPTYPE::INTSXP).is_null());
        }
        assert!(arena.growth_warrants_gc());
        arena.note_gc_completed();
        assert!(!arena.growth_warrants_gc());
        for _ in 0..=GC_TRIGGER_THRESHOLD {
            assert!(!arena.alloc_node(SEXPTYPE::INTSXP).is_null());
        }
        assert!(arena.growth_warrants_gc());
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
    fn charsxp_header_initializes_length_and_truelength() {
        let mut arena = RArena::new();
        let value = arena.alloc_charsxp(b"header");
        unsafe {
            assert_eq!((*value).vecsxp_length(), 6);
            assert_eq!((*value).vecsxp_truelength(), 0);
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

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            arena.free_node(ptr)
        });
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
            let s = std::ffi::CStr::from_ptr(data as *const core::ffi::c_char);
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
            let s = std::ffi::CStr::from_ptr(data as *const core::ffi::c_char);
            assert_eq!(s.to_str().unwrap_or(""), "");
        }
    }

    #[test]
    fn test_arena_cons() {
        let mut arena = RArena::new();
        let car = arena.alloc_node(SEXPTYPE::INTSXP);
        let cdr = arena.alloc_node(SEXPTYPE::REALSXP);
        let cell = unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            arena.cons(car, cdr, ptr::null_mut())
        };
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
            assert_eq!(
                (*cdr2).data.listsxp.cdrval,
                crate::sexp::globals::R_NilValue()
            );
        }
    }

    #[test]
    fn test_arena_alloc_list_chain_zero() {
        let mut arena = RArena::new();
        let list = arena.alloc_list_chain(0);
        assert_eq!(list, unsafe { crate::sexp::globals::R_NilValue() });
    }

    #[test]
    fn test_arena_alloc_list_chain_negative() {
        let mut arena = RArena::new();
        let list = arena.alloc_list_chain(-1);
        assert_eq!(list, unsafe { crate::sexp::globals::R_NilValue() });
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
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            arena.free_node(ptr)
        });
        assert_eq!(arena.node_count(), 0);
        assert_eq!(arena.free_count(), 1);
    }

    #[test]
    fn test_arena_free_node_idempotent() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_node(SEXPTYPE::INTSXP);
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            arena.free_node(ptr)
        });
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            arena.free_node(ptr)
        });
        assert_eq!(arena.free_count(), 1);
    }

    #[test]
    fn test_arena_normalize_free_list_rebuilds_membership_index() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_node(SEXPTYPE::INTSXP);
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            arena.free_node(ptr)
        });
        arena.free_list.push(ptr);
        arena.free_addrs.clear();

        arena.normalize_free_list();

        assert_eq!(arena.free_count(), 1);
        assert!(arena.free_addrs.contains(&(ptr as usize)));
        assert!(!arena.active_addrs.contains(&(ptr as usize)));
    }

    #[test]
    fn test_arena_free_node_null() {
        let mut arena = RArena::new();
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            arena.free_node(ptr::null_mut())
        });
        assert_eq!(arena.free_count(), 0);
    }

    #[test]
    fn test_arena_free_node_reuse() {
        let mut arena = RArena::new();
        let ptr1 = arena.alloc_node(SEXPTYPE::INTSXP);
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            arena.free_node(ptr1)
        });
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
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            arena.free_node(freed)
        });

        let active: Vec<SEXP> = arena.active_nodes().collect();
        assert_eq!(active, vec![live]);
    }

    fn membership_scan(arena: &super::RArena) -> Vec<SEXP> {
        arena
            .nodes()
            .filter(|ptr| arena.active_addrs.contains(&(*ptr as usize)))
            .collect()
    }

    #[test]
    fn active_nodes_bitmap_matches_membership_and_old_sweep_skips_young() {
        let mut arena = super::RArena::new();
        let a = arena.alloc_node(super::SEXPTYPE::INTSXP);
        let b = arena.alloc_node(super::SEXPTYPE::REALSXP);
        let c = arena.alloc_node(super::SEXPTYPE::LGLSXP);
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            arena.free_node(b)
        });
        let reused = arena.alloc_node(super::SEXPTYPE::REALSXP);
        assert_eq!(reused, b);
        assert_eq!(arena.active_nodes().collect::<Vec<_>>(), vec![a, b, c]);
        assert_eq!(
            arena.active_nodes().collect::<Vec<_>>(),
            membership_scan(&arena)
        );

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            arena.free_node(a)
        });
        assert_eq!(arena.active_nodes().collect::<Vec<_>>(), vec![b, c]);
        assert!(arena.old_nodes().next().is_none());

        unsafe {
            (*c).sxpinfo.set_gcgen(1);
        }
        assert_eq!(arena.old_nodes().collect::<Vec<_>>(), vec![c]);
        unsafe {
            (*b).sxpinfo.set_gcgen(1);
            (*c).sxpinfo.set_gcgen(0);
        }
        // `b` was allocated before `c`, so the old set stays in slab order.
        assert_eq!(arena.old_nodes().collect::<Vec<_>>(), vec![b]);

        let mut arena = super::RArena::new();
        let span = if cfg!(miri) {
            8
        } else {
            super::NODE_PAGE_SIZE + 3
        };
        let mut ptrs = Vec::with_capacity(span);
        for _ in 0..span {
            ptrs.push(arena.alloc_node(super::SEXPTYPE::INTSXP));
        }
        let mut freed = vec![0usize, 1];
        if span > super::NODE_PAGE_SIZE {
            freed.push(super::NODE_PAGE_SIZE);
        }
        freed.push(span - 1);
        for index in &freed {
            (unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                arena.free_node(ptrs[*index])
            });
        }
        let expect: Vec<SEXP> = ptrs
            .iter()
            .enumerate()
            .filter(|(index, _)| !freed.contains(&index))
            .map(|(_, ptr)| *ptr)
            .collect();
        let active = arena.active_nodes().collect::<Vec<_>>();
        assert_eq!(active, expect);
        assert_eq!(active, membership_scan(&arena));
        assert_eq!(active.len(), arena.node_count());

        unsafe {
            (*ptrs[2]).sxpinfo.set_gcgen(1);
            (*ptrs[3]).sxpinfo.set_gcgen(1);
        }
        super::begin_gc_epoch();
        assert_eq!(
            super::gc_touch(std::ptr::null_mut()),
            super::GcTouch::OutsideArena
        );
        let stray = super::SexprecCore::new(super::SEXPTYPE::INTSXP);
        assert_eq!(
            super::gc_touch(std::ptr::from_ref(&stray).cast_mut()),
            super::GcTouch::OutsideArena
        );
        assert_eq!(super::gc_touch(ptrs[2]), super::GcTouch::NewlyMarked);
        assert_eq!(super::gc_touch(ptrs[2]), super::GcTouch::AlreadyMarked);
        assert!(super::arena_node_marked(ptrs[2]));
        assert!(!super::arena_node_marked(ptrs[3]));
        let dead_old: Vec<SEXP> = arena
            .old_nodes()
            .filter(|ptr| !super::arena_node_marked(*ptr))
            .collect();
        assert_eq!(dead_old, vec![ptrs[3]]);
        assert!(arena.old_nodes().count() < arena.active_nodes().count());

        if cfg!(miri) {
            return;
        }

        let mut arena = super::RArena::new();
        let total = super::NODE_PAGE_SIZE * 4;
        let mut live = Vec::with_capacity(total);
        for index in 0..total {
            let ptr = arena.alloc_node(super::SEXPTYPE::INTSXP);
            if index % 64 == 0 {
                unsafe { (*ptr).sxpinfo.set_gcgen(1) };
            }
            live.push(ptr);
        }
        // Stand-in for the reachable graph: much smaller than the slab once
        // young garbage has piled up, which is the long torture run.
        let reachable: Vec<SEXP> = live.iter().copied().step_by(8).collect();
        let iters = 12u32;
        let mut hash_ns = 0u128;
        let mut bitmap_ns = 0u128;
        let mut old_ns = 0u128;
        for _ in 0..iters {
            let started = std::time::Instant::now();
            let mut seen = 0usize;
            for ptr in arena.nodes() {
                if arena.active_addrs.contains(&(ptr as usize)) {
                    seen += 1;
                    std::hint::black_box(ptr);
                }
            }
            hash_ns += started.elapsed().as_nanos();
            std::hint::black_box(seen);

            let started = std::time::Instant::now();
            let mut seen = 0usize;
            for ptr in arena.active_nodes() {
                seen += 1;
                std::hint::black_box(ptr);
            }
            bitmap_ns += started.elapsed().as_nanos();
            std::hint::black_box(seen);

            let started = std::time::Instant::now();
            super::begin_gc_epoch();
            for ptr in &reachable {
                std::hint::black_box(super::gc_touch(*ptr));
            }
            let mut dead_old = 0usize;
            for ptr in arena.old_nodes() {
                if !super::arena_node_marked(ptr) {
                    dead_old += 1;
                }
                std::hint::black_box(ptr);
            }
            old_ns += started.elapsed().as_nanos();
            std::hint::black_box(dead_old);
        }
        let hash_per = hash_ns / u128::from(iters);
        let bitmap_per = bitmap_ns / u128::from(iters);
        let old_per = old_ns / u128::from(iters);
        eprintln!(
            "active_nodes scan: membership {hash_per}ns, bitmap {bitmap_per}ns ({:.1}x), old-generation sweep {old_per}ns ({:.1}x vs membership)",
            hash_per as f64 / bitmap_per.max(1) as f64,
            hash_per as f64 / old_per.max(1) as f64
        );
    }

    #[test]
    fn test_arena_free_vector_clears_payload_pointer() {
        let mut arena = RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 2);
        assert!(!ptr.is_null());
        unsafe {
            assert!(!(*ptr).gengc_next_node.is_null());
        }
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            arena.free_node(ptr)
        });
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
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            arena.free_node(ptr)
        });
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
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            arena.free_node(ptr)
        });
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
    fn test_vector_and_charsxp_account_node_once() {
        let node_bytes = std::mem::size_of::<SexprecCore>();

        let mut arena = RArena::new();
        assert!(!arena.alloc_vector(SEXPTYPE::REALSXP, 1).is_null());
        assert_eq!(
            arena.total_bytes_allocated(),
            node_bytes + std::mem::size_of::<f64>()
        );

        let mut arena = RArena::new();
        assert!(!arena.alloc_charsxp(b"abc").is_null());
        assert_eq!(arena.total_bytes_allocated(), node_bytes + 4);
    }

    #[test]
    fn test_arena_node_budget_applies_to_raw_alloc_node() {
        let mut arena = RArena::with_budget(ArenaBudget::new(0, 1));
        let first = arena.alloc_node(SEXPTYPE::INTSXP);
        assert!(!first.is_null());
        let second = arena.alloc_node(SEXPTYPE::REALSXP);
        assert!(second.is_null());

        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            arena.free_node(first)
        });
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
        (unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            arena.free_node(node)
        });
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
        let ptr = unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            arena.add_node(boxed)
        };
        assert!(!ptr.is_null());
        assert_eq!(arena.node_count(), 1);
    }

    #[test]
    fn test_arena_add_node_obeys_budget() {
        let node_bytes = std::mem::size_of::<SexprecCore>();
        let mut arena = RArena::with_budget(ArenaBudget::new(node_bytes, 1));
        assert!(
            !(unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ arena
                .add_node(Box::new(SexprecCore::new(SEXPTYPE::INTSXP))) })
                .is_null()
        );
        assert!(
            (unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ arena
                .add_node(Box::new(SexprecCore::new(SEXPTYPE::REALSXP))) })
                .is_null()
        );
    }

    #[test]
    fn test_arena_can_target_instance_explicitly() {
        let mut left = super::super::instance::RInstance::new();
        let mut right = super::super::instance::RInstance::new();
        let left_before = unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena_in(&mut left, |arena| arena.node_count())
        };
        let right_before = unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena_in(&mut right, |arena| arena.node_count())
        };

        let left_node = unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena_in(&mut left, |arena| arena.alloc_node(SEXPTYPE::INTSXP))
        };
        assert!(!left_node.is_null());
        assert_eq!(
            (unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                with_arena_in(&mut left, |arena| arena.node_count())
            }),
            left_before + 1
        );
        assert_eq!(
            (unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                with_arena_in(&mut right, |arena| arena.node_count())
            }),
            right_before
        );

        let right_node = unsafe {
            /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
            with_arena_in(&mut right, |arena| arena.alloc_node(SEXPTYPE::REALSXP))
        };
        assert!(!right_node.is_null());
        assert_eq!(
            (unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                with_arena_in(&mut left, |arena| arena.node_count())
            }),
            left_before + 1
        );
        assert_eq!(
            (unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                with_arena_in(&mut right, |arena| arena.node_count())
            }),
            right_before + 1
        );

        // No checked handles or payload borrows remain in this raw fixture.
        left.arena = RArena::new();
        assert_eq!(
            (unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                with_arena_in(&mut left, |arena| arena.node_count())
            }),
            0
        );
        assert_eq!(
            (unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                with_arena_in(&mut right, |arena| arena.node_count())
            }),
            right_before + 1
        );
    }

    #[test]
    fn test_ambient_arena_borrow_depth_resets_after_panic() {
        let _session = crate::sexp::session::RSession::new_for_gc_tests();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (unsafe {
                /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
                with_arena(|_| panic!("intentional arena borrow panic"))
            });
        }));

        assert!(result.is_err());
        assert_eq!(super::super::instance::instance_borrow_depth(), 0);
    }
}
