//! GC epoch/touch/generation bookkeeping and slot iteration.

use super::slab::{GC_EPOCH, SLAB_META};
use super::*;

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
/// Called from [`SxpInfo::set_gcgen`](super::super::ffi::SxpInfo::set_gcgen). Pointers
/// outside registered live owned pages (such as header temporaries) are ignored.
pub(crate) fn note_slab_generation(ptr: SEXP, generation: u8) {
    let Some((meta, slot)) = find_slab_slot(ptr) else {
        return;
    };
    meta.set_old(slot, generation == 1);
}

/// A page transaction keeps dense original generations and eligibility bits.
/// It owns projections and the physical backing, without a RefCell loan.
pub(super) struct PageSnapshot {
    pub(super) projection: NodeProjection<SexprecCore>,
    pub(super) eligible: Box<[u64]>,
    pub(super) generations: Box<[u64]>,
}

/// Live nodes as they existed when iteration began. Collection enumeration
/// runs without semantic callbacks, but this iterator also rejects any slot
/// freed or reused before it is consumed. Snapshot metadata uses eight bytes
/// per slot instead of a full allocation token per live node.
pub(crate) struct SlotIter {
    pub(super) pages: Vec<PageSnapshot>,
    pub(super) _backing_owners: Rc<HeapBackingOwners>,
    pub(super) page: usize,
    pub(super) word: usize,
    pub(super) bits: u64,
}

impl Iterator for SlotIter {
    type Item = SEXP;

    fn next(&mut self) -> Option<SEXP> {
        loop {
            let page = self.pages.get(self.page)?;
            if self.bits != 0 {
                let bit = self.bits.trailing_zeros() as usize;
                self.bits &= self.bits - 1;
                let slot = (self.word - 1) * 64 + bit;
                if let Some((pointer, _allocation)) = page
                    .projection
                    .resolve_generation(slot, page.generations[slot])
                {
                    return Some(pointer);
                }
                continue;
            }
            if let Some(bits) = page.eligible.get(self.word) {
                self.bits = *bits;
                self.word += 1;
            } else {
                self.page += 1;
                self.word = 0;
            }
        }
    }
}
