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

use std::alloc::{Layout, alloc, alloc_zeroed, dealloc};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ptr::{self};
use std::rc::Rc;

/// Size of each slab page for SexprecCore nodes. Larger pages reduce allocator overhead
/// and improve cache locality vs one Box per node. Chose 4096 as balance ( ~256KB per page
/// assuming ~64B SexprecCore).
const NODE_PAGE_SIZE: usize = 4096;
/// `u64` words in one page's occupancy bitmap (`NODE_PAGE_SIZE` bits).
const OCCUPANCY_WORDS_PER_PAGE: usize = NODE_PAGE_SIZE / 64;
const _: () = assert!(NODE_PAGE_SIZE % 64 == 0);

use super::ffi::{R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE, SexprecCore, SexprecData};
use super::object::Sexp;

/// Byte size of one node. `pointer.add` uses this stride.
const NODE_BYTES: usize = std::mem::size_of::<SexprecCore>();
/// Bytes occupied by the node array itself, before stride rounding.
const SLAB_BYTES: usize = NODE_PAGE_SIZE * NODE_BYTES;
/// Power-of-two allocation size and alignment. A node address maps to its
/// page by masking, which keeps the per-node GC touch off a hash table when
/// the page is hot.
const SLAB_STRIDE: usize = SLAB_BYTES.next_power_of_two();
const _: () = assert!(SLAB_STRIDE.is_power_of_two());
const _: () = assert!(SLAB_STRIDE >= SLAB_BYTES);

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
// This eliminates one source of O(n) in arena (take_data_buffer, and frequent free of vectors).
// Layouts are small, HashMap overhead acceptable vs scan on many vectors.

// ---------------------------------------------------------------------------
// RArena: arena allocator for R objects
// ---------------------------------------------------------------------------

/// Per-page GC side table. Owned as a raw allocation beside the node slab so
/// an `&RArena` borrow does not cover these bytes (same rule as node slots).
///
/// `occupancy` is the live-node bitmap. `old` is the old-generation subset
/// the torture sweep walks. `epoch[slot] == current` means the node was
/// marked in this collection, so sweeps do not clear a header bit on every
/// young node.
#[repr(C)]
struct PageGcMeta {
    occupancy: [u64; OCCUPANCY_WORDS_PER_PAGE],
    old: [u64; OCCUPANCY_WORDS_PER_PAGE],
    epoch: [u32; NODE_PAGE_SIZE],
}

const OCCUPANCY_OFFSET: usize = std::mem::offset_of!(PageGcMeta, occupancy);
const OLD_OFFSET: usize = std::mem::offset_of!(PageGcMeta, old);
const EPOCH_OFFSET: usize = std::mem::offset_of!(PageGcMeta, epoch);

/// One raw-allocated slab of `NODE_PAGE_SIZE` node slots.
///
/// Pages are allocated and freed through the allocator API and are only ever
/// accessed through raw pointers. The previous `Vec` storage formed Rust
/// references over the page on every allocation (`push`, indexing), and each
/// of those retags invalidated the raw SEXPs of all nodes handed out by
/// earlier allocations — aliasing UB under Stacked Borrows.
struct SlabPage {
    base: *mut SexprecCore,
    meta: *mut PageGcMeta,
}

impl Drop for SlabPage {
    fn drop(&mut self) {
        unregister_slab(self.base, self.meta);
        self.meta = ptr::null_mut();
        if !self.base.is_null() {
            unsafe { dealloc(self.base.cast(), slab_layout()) };
            self.base = ptr::null_mut();
        }
    }
}

fn slab_layout() -> Layout {
    Layout::from_size_align(SLAB_STRIDE, SLAB_STRIDE).expect("slab page layout is valid")
}

thread_local! {
    static SLAB_META: RefCell<HashMap<usize, *mut PageGcMeta>> = RefCell::new(HashMap::new());
    /// Last page touched. Torture marking walks nodes allocated together, so
    /// the next lookup usually hits this instead of the page map.
    static SLAB_CACHE: Cell<(usize, *mut PageGcMeta)> = const { Cell::new((0, ptr::null_mut())) };
    /// 0 means "no collection has started". Node epochs are never 0 after a
    /// visit, and 0 is never a current epoch, so a stale 0 cannot look marked.
    static GC_EPOCH: Cell<u32> = const { Cell::new(0) };
}

fn register_slab(base: *mut SexprecCore, meta: *mut PageGcMeta) {
    let key = base as usize;
    SLAB_META.with(|map| {
        map.borrow_mut().insert(key, meta);
    });
    SLAB_CACHE.set((key, meta));
}

fn unregister_slab(base: *mut SexprecCore, meta: *mut PageGcMeta) {
    if !base.is_null() {
        let key = base as usize;
        SLAB_META.with(|map| {
            map.borrow_mut().remove(&key);
        });
        let (cached_base, cached_meta) = SLAB_CACHE.get();
        if cached_base == key || cached_meta == meta {
            SLAB_CACHE.set((0, ptr::null_mut()));
        }
    }
    if !meta.is_null() {
        unsafe { dealloc(meta.cast(), Layout::new::<PageGcMeta>()) };
    }
}

#[inline(always)]
fn find_slab_slot(ptr: SEXP) -> Option<(*mut PageGcMeta, usize)> {
    if ptr.is_null() {
        return None;
    }
    let addr = ptr as usize;
    let base = addr & !(SLAB_STRIDE - 1);
    let (cached_base, cached_meta) = SLAB_CACHE.get();
    let meta = if cached_base == base && !cached_meta.is_null() {
        cached_meta
    } else {
        let found = SLAB_META.with(|map| map.borrow().get(&base).copied())?;
        if found.is_null() {
            return None;
        }
        SLAB_CACHE.set((base, found));
        found
    };
    let delta = addr - base;
    if !delta.is_multiple_of(NODE_BYTES) {
        return None;
    }
    let slot = delta / NODE_BYTES;
    if slot >= NODE_PAGE_SIZE {
        return None;
    }
    Some((meta, slot))
}

#[inline(always)]
unsafe fn bitmap_word_ptr(meta: *mut PageGcMeta, old: bool, word: usize) -> *mut u64 {
    let offset = if old { OLD_OFFSET } else { OCCUPANCY_OFFSET };
    unsafe { (meta as *mut u8).add(offset).cast::<u64>().add(word) }
}

#[inline(always)]
unsafe fn epoch_slot_ptr(meta: *mut PageGcMeta, slot: usize) -> *mut u32 {
    unsafe { (meta as *mut u8).add(EPOCH_OFFSET).cast::<u32>().add(slot) }
}

#[inline(always)]
unsafe fn set_bitmap_bit(meta: *mut PageGcMeta, old: bool, slot: usize, on: bool) {
    let word_ptr = unsafe { bitmap_word_ptr(meta, old, slot >> 6) };
    let mask = 1u64 << (slot & 63);
    unsafe {
        let mut word = ptr::read(word_ptr);
        if on {
            word |= mask;
        } else {
            word &= !mask;
        }
        ptr::write(word_ptr, word);
    }
}

#[inline(always)]
unsafe fn bitmap_word(meta: *const PageGcMeta, old: bool, word: usize) -> u64 {
    unsafe { ptr::read(bitmap_word_ptr(meta as *mut PageGcMeta, old, word)) }
}

fn next_set_run(meta: *const PageGcMeta, old_only: bool, mut slot: usize) -> Option<(usize, usize)> {
    if meta.is_null() {
        return None;
    }
    while slot < NODE_PAGE_SIZE {
        let word_index = slot >> 6;
        let bit = slot & 63;
        let word = unsafe { bitmap_word(meta, old_only, word_index) };
        if word == 0 || (word >> bit) == 0 {
            slot = (word_index + 1) << 6;
            continue;
        }
        let start = slot + (word >> bit).trailing_zeros() as usize;
        let end = occupied_run_end(meta, old_only, start);
        if end <= start {
            slot = start + 1;
            continue;
        }
        return Some((start, end));
    }
    None
}

fn occupied_run_end(meta: *const PageGcMeta, old_only: bool, start: usize) -> usize {
    let mut end = start;
    while end < NODE_PAGE_SIZE {
        let word_index = end >> 6;
        let bit = end & 63;
        let word = unsafe { bitmap_word(meta, old_only, word_index) };
        if bit == 0 && word == u64::MAX {
            end += 64;
            continue;
        }
        let shifted = word >> bit;
        let ones = (!shifted).trailing_zeros() as usize;
        if ones == 0 {
            break;
        }
        end += ones;
        if bit + ones < 64 {
            break;
        }
    }
    end
}

/// Result of testing one pointer against the current GC epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GcTouch {
    /// Arena node already marked in this collection.
    AlreadyMarked,
    /// Arena node marked by this call.
    NewlyMarked,
    /// Not a slab node. Callers keep the header mark bit for persistent objects.
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
            for meta in map.borrow().values().copied() {
                if meta.is_null() {
                    continue;
                }
                unsafe {
                    ptr::write_bytes(epoch_slot_ptr(meta, 0), 0u8, NODE_PAGE_SIZE);
                }
            }
        });
        GC_EPOCH.set(1);
    } else {
        GC_EPOCH.set(next);
    }
}

/// Mark `ptr` if it is an arena node. Persistent nodes return [`GcTouch::OutsideArena`]
/// and keep using the header mark bit.
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
    unsafe {
        let slot_epoch = epoch_slot_ptr(meta, slot);
        if epoch != 0 && ptr::read(slot_epoch) == epoch {
            return GcTouch::AlreadyMarked;
        }
        if epoch != 0 {
            ptr::write(slot_epoch, epoch);
        }
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
    unsafe { ptr::read(epoch_slot_ptr(meta, slot)) == epoch }
}

/// Keep the old-generation bitmap aligned with `sxpinfo.gcgen`.
///
/// Called from [`SxpInfo::set_gcgen`](super::ffi::SxpInfo::set_gcgen). Pointers
/// that are not slab nodes (header temporaries, persistent sentinels) are ignored.
pub(crate) fn note_slab_generation(ptr: SEXP, generation: u8) {
    let Some((meta, slot)) = find_slab_slot(ptr) else {
        return;
    };
    unsafe { set_bitmap_bit(meta, true, slot, generation == 1) };
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
    base: *mut SexprecCore,
}

impl Iterator for SlotIter<'_> {
    type Item = SEXP;

    #[inline(always)]
    fn next(&mut self) -> Option<SEXP> {
        if self.slot < self.slot_end {
            // SAFETY: `slot` is inside a run armed from a set bitmap bit, and
            // bits are set only for slots written by the allocator. `base` is
            // that page's slab allocation.
            let ptr = unsafe { self.base.add(self.slot) };
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
        let ptr = unsafe { self.base.add(self.slot) };
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
            let meta = self.pages[page].meta;
            if let Some((start, end)) = next_set_run(meta, self.old_only, slot) {
                self.page = page;
                self.slot = start;
                self.slot_end = end;
                self.base = self.pages[page].base;
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
/// The arena does NOT support individual deallocation -- the entire arena
/// is freed at once when dropped.
pub struct RArena {
    /// Slab pages of raw-allocated node slots. Pages never move once
    /// allocated and are only touched through raw pointers (see [`SlabPage`]):
    /// node SEXPs handed out to the rest of the interpreter must keep a
    /// valid borrow-stack tag for the arena's lifetime. Matches spirit of
    /// R's NodeClass pages.
    node_pages: Vec<SlabPage>,
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
        let layout = slab_layout();
        let base = unsafe { alloc(layout) } as *mut SexprecCore;
        assert!(
            !base.is_null(),
            "arena slab page allocation failed (out of memory)"
        );
        debug_assert_eq!(
            base as usize & (SLAB_STRIDE - 1),
            0,
            "slab page is stride-aligned"
        );
        let meta = unsafe { alloc_zeroed(Layout::new::<PageGcMeta>()) } as *mut PageGcMeta;
        assert!(
            !meta.is_null(),
            "arena page metadata allocation failed (out of memory)"
        );
        // Push before registering so a panic in the page map still frees both
        // allocations through `SlabPage::drop`.
        self.node_pages.push(SlabPage { base, meta });
        register_slab(base, meta);
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
        if self.slab_offset >= NODE_PAGE_SIZE {
            self.alloc_new_page();
        }
        // SAFETY: slab_offset < NODE_PAGE_SIZE and the page holds exactly
        // NODE_PAGE_SIZE slots. All page access stays at the raw-pointer
        // level: forming Rust references over the page would retag the
        // allocation and invalidate the SEXPs of earlier nodes.
        let ptr: SEXP = unsafe { self.node_pages[self.slab_page].base.add(self.slab_offset) };
        unsafe { std::ptr::write(ptr, ctor()) };
        self.slab_offset += 1;
        self.add_accounted_bytes(std::mem::size_of::<SexprecCore>());
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
            transient_bytes: Rc::new(Cell::new(0)),
            pending_data_bytes: Rc::new(Cell::new(0)),
            alloc_gc_torture_ticks: 0,
            alloc_gc_collect_requested: false,
            nodes_at_last_gc: 0,
            bytes_at_last_gc: 0,
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
        unsafe {
            set_bitmap_bit(meta, false, slot, true);
            // The header is the source of truth (fresh nodes are young; a
            // copied node may already be old). Drop a stale epoch so the slot
            // cannot look marked in the collection that is already running.
            set_bitmap_bit(meta, true, slot, (*ptr).sxpinfo.gcgen() == 1);
            ptr::write(epoch_slot_ptr(meta, slot), 0);
        }
    }

    fn track_node_freed(&mut self, ptr: SEXP) {
        if ptr.is_null() {
            return;
        }
        self.active_addrs.remove(&(ptr as usize));
        self.free_addrs.insert(ptr as usize);
        if let Some((meta, slot)) = find_slab_slot(ptr) {
            unsafe {
                set_bitmap_bit(meta, false, slot, false);
                set_bitmap_bit(meta, true, slot, false);
                ptr::write(epoch_slot_ptr(meta, slot), 0);
            }
        }
    }

    #[inline(always)]
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
        note_lend_budget(budget.max_bytes);
    }

    /// Reserve native scratch space against this session's byte budget.
    ///
    /// The returned guard releases the reservation on every exit path,
    /// including an R error or cancellation unwind. The allocation itself is
    /// owned by the caller; this only accounts for its peak workspace.
    pub(crate) fn try_reserve_transient(&mut self, bytes: usize) -> Option<TransientReservation> {
        let total = self
            .total_bytes_allocated
            .checked_add(self.transient_bytes.get())?
            .checked_add(self.pending_data_bytes.get())?
            .checked_add(bytes)?;
        if self.budget.max_bytes != 0 && total > self.budget.max_bytes {
            return None;
        }
        self.transient_bytes
            .set(self.transient_bytes.get().checked_add(bytes)?);
        Some(TransientReservation {
            counter: Rc::clone(&self.transient_bytes),
            bytes,
        })
    }

    fn register_data_buffer(&mut self, ptr: *mut u8, layout: Layout) {
        self.add_accounted_bytes(layout.size());
        self.data_bufs.insert(ptr, layout);
    }

    /// Move a lend-time reservation into the registered buffer map.
    fn commit_reserved_data_buffer(&mut self, ptr: *mut u8, layout: Layout) {
        let size = layout.size();
        let pending = self.pending_data_bytes.get();
        self.pending_data_bytes.set(pending.saturating_sub(size));
        self.register_data_buffer(ptr, layout);
    }

    fn add_accounted_bytes(&mut self, bytes: usize) {
        self.total_bytes_allocated = self.total_bytes_allocated.saturating_add(bytes);
        LEND_LEDGER.with(|slot| {
            if let Some(ledger) = slot.borrow().last() {
                ledger
                    .total
                    .set(ledger.total.get().saturating_add(bytes));
            }
        });
    }

    fn sub_accounted_bytes(&mut self, bytes: usize) {
        self.total_bytes_allocated = self.total_bytes_allocated.saturating_sub(bytes);
        LEND_LEDGER.with(|slot| {
            if let Some(ledger) = slot.borrow().last() {
                ledger
                    .total
                    .set(ledger.total.get().saturating_sub(bytes));
            }
        });
    }

    /// Account for a buffer allocated outside this arena.
    ///
    /// Returns false when the byte budget cannot hold `layout`, leaving the
    /// map unchanged. The caller still owns `ptr` in that case.
    fn adopt_data_buffer(&mut self, ptr: *mut u8, layout: Layout) -> bool {
        if ptr.is_null() || layout.size() == 0 || !self.can_grow_bytes_by(layout.size()) {
            return false;
        }
        self.register_data_buffer(ptr, layout);
        true
    }

    fn tracks_data_buffer(&self, ptr: *mut u8) -> bool {
        !ptr.is_null() && self.data_bufs.contains_key(&ptr)
    }

    fn take_data_buffer(&mut self, ptr: *mut u8) -> Option<Layout> {
        if let Some(layout) = self.data_bufs.remove(&ptr) {
            self.sub_accounted_bytes(layout.size());
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
                .checked_add(self.transient_bytes.get())
                .and_then(|total| total.checked_add(self.pending_data_bytes.get()))
                .and_then(|total| total.checked_add(bytes))
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
    #[inline(always)]
    pub(crate) fn alloc_node(&mut self, sexptype: SEXPTYPE) -> SEXP {
        self.alloc_gc_torture_ticks = self.alloc_gc_torture_ticks.wrapping_add(1);
        if !self.can_activate_node() {
            return ptr::null_mut();
        }

        if let Some(ptr) = self.reuse_free_node(sexptype) {
            return ptr;
        }

        if self.growth_warrants_gc() {
            self.alloc_gc_collect_requested = true;
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
    #[inline(always)]
    pub(crate) fn alloc_vector(&mut self, sexptype: SEXPTYPE, length: R_xlen_t) -> SEXP {
        if length < 0 {
            return ptr::null_mut();
        }

        self.alloc_gc_torture_ticks = self.alloc_gc_torture_ticks.wrapping_add(1);

        if self.growth_warrants_gc() {
            self.alloc_gc_collect_requested = true;
        }

        let elem_size = sexp_elem_size(sexptype);
        let total_bytes = match (length as usize).checked_mul(elem_size) {
            Some(n) => n,
            None => return ptr::null_mut(),
        };

        if !self.can_allocate_new_node_with_payload(total_bytes) {
            return ptr::null_mut();
        }

        let data = if total_bytes > 0 {
            let layout = match Layout::from_size_align(total_bytes, std::mem::align_of::<u64>()) {
                Ok(l) => l,
                Err(_) => return ptr::null_mut(),
            };
            let data_ptr = unsafe { alloc(layout) };
            if data_ptr.is_null() {
                return ptr::null_mut();
            }
            unsafe {
                std::ptr::write_bytes(data_ptr, 0, total_bytes);
            }
            Some((data_ptr, layout))
        } else {
            None
        };

        let node_ptr = self.allocate_core_in_slab(|| SexprecCore::new_vector(sexptype, length));

        if let Some((data_ptr, layout)) = data {
            unsafe {
                (*node_ptr).gengc_next_node = data_ptr as SEXP;
            }
            self.register_data_buffer(data_ptr, layout);
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
        if length < 0 {
            return Err(ArenaError::InvalidLength);
        }

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
            Some((data_ptr, layout))
        } else {
            None
        };

        let node_ptr = self.allocate_core_in_slab(|| SexprecCore::new_vector(sexptype, length));

        if let Some((data_ptr, layout)) = data {
            unsafe {
                (*node_ptr).gengc_next_node = data_ptr as SEXP;
            }
            self.register_data_buffer(data_ptr, layout);
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
        if !self.can_allocate_new_node_with_payload(total_bytes) {
            return ptr::null_mut();
        }

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

        unsafe {
            (*node_ptr).gengc_next_node = data_ptr as SEXP;
        }

        self.register_data_buffer(data_ptr, layout);
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
        if !self.can_allocate_new_node_with_payload(0) {
            return ptr::null_mut();
        }

        if self.slab_offset >= NODE_PAGE_SIZE {
            self.alloc_new_page();
        }
        // Write through the raw slot pointer; see `allocate_core_in_slab`
        // for why the page must never be touched through Rust references.
        let ptr: SEXP = unsafe { self.node_pages[self.slab_page].base.add(self.slab_offset) };
        unsafe { std::ptr::write(ptr, *node) };
        self.slab_offset += 1;
        self.add_accounted_bytes(std::mem::size_of::<SexprecCore>());
        self.register_new_node(ptr)
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
                let base = page.base;
                // Allocation fills pages sequentially: only the current page
                // can be partially occupied.
                let used = if page_idx == self.slab_page {
                    self.slab_offset
                } else {
                    NODE_PAGE_SIZE
                };
                (0..used).map(move |i| unsafe { base.add(i) } as SEXP)
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
            base: ptr::null_mut(),
        }
    }

    /// Free a node by adding it to the free list for reuse.
    #[inline(always)]
    /// # Safety
    /// No live handle, graph edge or Rust payload borrow may refer to this node.
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

/// RAII release token for a native workspace reservation.
pub(crate) struct TransientReservation {
    counter: Rc<Cell<usize>>,
    bytes: usize,
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
    ptr: *mut u8,
    layout: Layout,
    /// `layout.size()` is already in the arena's `pending_data_bytes`.
    reserved: bool,
}

struct LendLedger {
    max_bytes: usize,
    total: Cell<usize>,
    transient: Rc<Cell<usize>>,
    pending: Rc<Cell<usize>>,
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
            max_bytes: arena.budget.max_bytes,
            total: Cell::new(arena.total_bytes_allocated),
            transient: Rc::clone(&arena.transient_bytes),
            pending: Rc::clone(&arena.pending_data_bytes),
        }
    };
    LEND_LEDGER.with(|slot| slot.borrow_mut().push(ledger));
}

fn note_lend_budget(max_bytes: usize) {
    LEND_LEDGER.with(|slot| {
        if let Some(ledger) = slot.borrow_mut().last_mut() {
            ledger.max_bytes = max_bytes;
        }
    });
}

/// Promise `bytes` against the active lend's budget.
///
/// Returns false when the promise does not fit. The caller must not publish
/// a pointer in that case: the lend's flush would otherwise free a buffer
/// the caller is still holding.
fn reserve_lend_bytes(bytes: usize) -> bool {
    LEND_LEDGER.with(|slot| {
        let ledger_ref = slot.borrow();
        let Some(ledger) = ledger_ref.last() else {
            return false;
        };
        if ledger.max_bytes != 0 {
            let Some(total) = ledger
                .total
                .get()
                .checked_add(ledger.transient.get())
                .and_then(|total| total.checked_add(ledger.pending.get()))
                .and_then(|total| total.checked_add(bytes))
            else {
                return false;
            };
            if total > ledger.max_bytes {
                return false;
            }
        }
        ledger
            .pending
            .set(ledger.pending.get().saturating_add(bytes));
        true
    })
}

struct ClearLendLedger;
impl Drop for ClearLendLedger {
    fn drop(&mut self) {
        LEND_LEDGER.with(|slot| {
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
/// `node` is a live vector header. `bytes > 0`.
pub(crate) unsafe fn attach_zeroed_data_buffer(node: SEXP, bytes: usize) -> *mut u8 {
    unsafe {
        if node.is_null() || bytes == 0 {
            return ptr::null_mut();
        }
        let Ok(layout) = Layout::from_size_align(bytes, std::mem::align_of::<u64>()) else {
            return ptr::null_mut();
        };
        let data_ptr = alloc(layout);
        if data_ptr.is_null() {
            return ptr::null_mut();
        }
        ptr::write_bytes(data_ptr, 0, bytes);

        let Some(inst) = super::instance::current_instance_ptr() else {
            dealloc(data_ptr, layout);
            return ptr::null_mut();
        };
        if is_arena_lent(inst) {
            // Refuse before the pointer is stored. Flush keeps every queued
            // pointer, so publishing one the budget cannot own would either
            // exceed the budget or free memory the caller still holds.
            if !reserve_lend_bytes(layout.size()) {
                dealloc(data_ptr, layout);
                return ptr::null_mut();
            }
            (*node).gengc_next_node = data_ptr as SEXP;
            PENDING_DATA_BUFFERS.with(|queue| {
                queue.borrow_mut().push(PendingDataBuffer {
                    instance: inst as usize,
                    node,
                    ptr: data_ptr,
                    layout,
                    reserved: true,
                });
            });
            return data_ptr;
        }
        (*node).gengc_next_node = data_ptr as SEXP;
        let adopted = with_arena(|arena| arena.adopt_data_buffer(data_ptr, layout));
        if !adopted {
            (*node).gengc_next_node = ptr::null_mut();
            dealloc(data_ptr, layout);
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
            queue
                .borrow()
                .iter()
                .any(|item| item.node == node && item.ptr == ptr)
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
                queue
                    .borrow()
                    .iter()
                    .any(|item| item.instance == inst as usize && item.ptr == ptr)
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
            // A queued pointer was returned to the caller. Keep it alive.
            if item.reserved {
                (*inst)
                    .arena
                    .commit_reserved_data_buffer(item.ptr, item.layout);
            } else {
                (*inst).arena.register_data_buffer(item.ptr, item.layout);
            }
            super::altseq::commit_expanded_buffer(item.node);
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
        crate::sexp::gengc::process_deferred_alloc_gc_in(inst, torture_ticks, collect_requested);
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
        let _session = crate::sexp::session::RSession::new();
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
        assert_eq!(arena.active_nodes().collect::<Vec<_>>(), membership_scan(&arena));

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
        let _session = crate::sexp::session::RSession::new();

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
