#![forbid(unsafe_code)]
//! Owned, non-moving node pages and checked allocation identities.
//!
//! The translated runtime may project a raw pointer from a `Cell`; page
//! ownership, allocation identity and collector metadata stay ordinary Rust.
//! IDs never keep a dead node alive and cannot select a reused or foreign slot.

use std::{
    cell::Cell,
    hash::{Hash, Hasher},
    rc::{Rc, Weak},
};

#[derive(Clone, Debug)]
pub(crate) struct HeapIdentity(Rc<()>);
impl HeapIdentity {
    pub(crate) fn new() -> Self {
        Self(Rc::new(()))
    }
    fn same(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct NodeId {
    heap: HeapIdentity,
    page: usize,
    page_identity: Rc<()>,
    slot: usize,
    generation: u64,
}
impl NodeId {
    pub(crate) fn page(&self) -> usize {
        self.page
    }
    pub(crate) fn slot(&self) -> usize {
        self.slot
    }
}
impl PartialEq for NodeId {
    fn eq(&self, other: &Self) -> bool {
        self.heap.same(&other.heap)
            && self.page == other.page
            && Rc::ptr_eq(&self.page_identity, &other.page_identity)
            && self.slot == other.slot
            && self.generation == other.generation
    }
}
impl Eq for NodeId {}
impl Hash for NodeId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::ptr::hash(Rc::as_ptr(&self.heap.0), state);
        self.page.hash(state);
        std::ptr::hash(Rc::as_ptr(&self.page_identity), state);
        self.slot.hash(state);
        self.generation.hash(state);
    }
}

/// An owned metadata lease validates liveness without borrowing the arena.
/// It retains no header storage; page destruction invalidates all its IDs.
#[derive(Clone)]
pub(crate) struct CheckedNode {
    metadata: Rc<PageMetadata>,
    id: NodeId,
}
impl std::fmt::Debug for CheckedNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CheckedNode")
            .field("id", &self.id)
            .field("live", &self.is_live())
            .finish_non_exhaustive()
    }
}
impl CheckedNode {
    pub(crate) fn new(metadata: Rc<PageMetadata>, id: NodeId) -> Option<Self> {
        metadata.validates(&id).then_some(Self { metadata, id })
    }
    pub(crate) fn is_live(&self) -> bool {
        self.metadata.validates(&self.id)
    }
    pub(crate) fn same_heap(&self, other: &Self) -> bool {
        self.id.heap.same(&other.id.heap)
    }
    pub(crate) fn belongs_to(&self, heap: &HeapIdentity) -> bool {
        self.id.heap.same(heap)
    }
    pub(crate) fn mark(&self, epoch: u32) -> Option<bool> {
        if !self.is_live() {
            return None;
        }
        self.metadata.mark(self.id.slot, epoch)
    }
    pub(crate) fn id(&self) -> &NodeId {
        &self.id
    }
}
impl PartialEq for CheckedNode {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for CheckedNode {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HeapError {
    Allocation,
    InvalidSlot,
    LiveSlot,
    RetiredSlot,
}

fn cells<T>(len: usize, mut initial: impl FnMut() -> T) -> Result<Box<[Cell<T>]>, HeapError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|_| HeapError::Allocation)?;
    values.resize_with(len, || Cell::new(initial()));
    Ok(values.into_boxed_slice())
}

/// Shared collector metadata contains no node pointer or node borrow.
pub(crate) struct PageMetadata {
    heap: HeapIdentity,
    page: usize,
    identity: Rc<()>,
    slots: usize,
    retired: Cell<bool>,
    live: Box<[Cell<u64>]>,
    old: Box<[Cell<u64>]>,
    epochs: Box<[Cell<u32>]>,
    generations: Box<[Cell<u64>]>,
}
impl PageMetadata {
    pub(crate) fn try_new(
        heap: HeapIdentity,
        page: usize,
        slots: usize,
    ) -> Result<Self, HeapError> {
        let words = slots.checked_add(63).ok_or(HeapError::Allocation)? / 64;
        Ok(Self {
            heap,
            page,
            identity: Rc::new(()),
            slots,
            retired: Cell::new(false),
            live: cells(words, || 0)?,
            old: cells(words, || 0)?,
            epochs: cells(slots, || 0)?,
            generations: cells(slots, || 0)?,
        })
    }
    pub(crate) fn page(&self) -> usize {
        self.page
    }
    pub(crate) fn slots(&self) -> usize {
        self.slots
    }
    pub(crate) fn invalidate_all(&self) {
        self.retired.set(true);
        for word in &self.live {
            word.set(0);
        }
        for word in &self.old {
            word.set(0);
        }
        self.clear_epochs();
    }
    pub(crate) fn belongs_to(&self, heap: &HeapIdentity) -> bool {
        self.heap.same(heap)
    }
    pub(crate) fn is_live(&self, slot: usize) -> bool {
        !self.retired.get()
            && slot < self.slots
            && self.live[slot >> 6].get() & (1 << (slot & 63)) != 0
    }
    pub(crate) fn reusable(&self, slot: usize) -> bool {
        !self.retired.get()
            && slot < self.slots
            && !self.is_live(slot)
            && self.generations[slot].get() != u64::MAX
    }
    fn bit(&self, old: bool, slot: usize, on: bool) {
        let words = if old { &self.old } else { &self.live };
        let word = &words[slot >> 6];
        let mask = 1_u64 << (slot & 63);
        word.set(if on {
            word.get() | mask
        } else {
            word.get() & !mask
        });
    }
    pub(crate) fn activate(&self, slot: usize, old: bool) -> Result<NodeId, HeapError> {
        if slot >= self.slots {
            return Err(HeapError::InvalidSlot);
        }
        if self.retired.get() {
            return Err(HeapError::RetiredSlot);
        }
        if self.is_live(slot) {
            return Err(HeapError::LiveSlot);
        }
        let generation = self.generations[slot]
            .get()
            .checked_add(1)
            .ok_or(HeapError::RetiredSlot)?;
        self.generations[slot].set(generation);
        self.epochs[slot].set(0);
        self.bit(false, slot, true);
        self.bit(true, slot, old);
        Ok(NodeId {
            heap: self.heap.clone(),
            page: self.page,
            page_identity: self.identity.clone(),
            slot,
            generation,
        })
    }
    pub(crate) fn current_id(&self, slot: usize) -> Option<NodeId> {
        self.is_live(slot).then(|| NodeId {
            heap: self.heap.clone(),
            page: self.page,
            page_identity: self.identity.clone(),
            slot,
            generation: self.generations[slot].get(),
        })
    }
    pub(crate) fn validates(&self, id: &NodeId) -> bool {
        self.heap.same(&id.heap)
            && self.page == id.page
            && Rc::ptr_eq(&self.identity, &id.page_identity)
            && self.is_live(id.slot)
            && self.generations[id.slot].get() == id.generation
    }
    /// Invalidate the current identity before its storage is made reusable.
    /// Generation exhaustion permanently retires the slot instead of wrapping.
    pub(crate) fn release(&self, id: &NodeId) -> bool {
        if !self.validates(id) {
            return false;
        }
        self.bit(false, id.slot, false);
        self.bit(true, id.slot, false);
        self.epochs[id.slot].set(0);
        let generation = &self.generations[id.slot];
        generation.set(generation.get().saturating_add(1));
        true
    }
    pub(crate) fn set_old(&self, slot: usize, old: bool) {
        if self.is_live(slot) {
            self.bit(true, slot, old);
        }
    }
    pub(crate) fn epoch(&self, slot: usize) -> Option<u32> {
        self.is_live(slot).then(|| self.epochs[slot].get())
    }
    pub(crate) fn mark(&self, slot: usize, epoch: u32) -> Option<bool> {
        if epoch == 0 || !self.is_live(slot) {
            return None;
        }
        let previous = self.epochs[slot].replace(epoch);
        Some(previous == epoch)
    }
    pub(crate) fn clear_epochs(&self) {
        for epoch in &self.epochs {
            epoch.set(0);
        }
    }
    fn word(&self, old: bool, word: usize) -> u64 {
        let bits = if old { &self.old } else { &self.live };
        bits[word].get()
    }
    pub(crate) fn next_run(&self, old: bool, mut slot: usize) -> Option<(usize, usize)> {
        while slot < self.slots {
            let word_index = slot >> 6;
            let bit = slot & 63;
            let word = self.word(old, word_index);
            if word >> bit == 0 {
                slot = (word_index + 1) << 6;
                continue;
            }
            let start = slot + (word >> bit).trailing_zeros() as usize;
            let mut end = start;
            while end < self.slots {
                let index = end >> 6;
                let bit = end & 63;
                let word = self.word(old, index);
                if bit == 0 && word == u64::MAX {
                    end += 64;
                    continue;
                }
                let ones = (!(word >> bit)).trailing_zeros() as usize;
                if ones == 0 {
                    break;
                }
                end += ones;
                if bit + ones < 64 {
                    break;
                }
            }
            return Some((start, end.min(self.slots)));
        }
        None
    }
    #[cfg(test)]
    fn force_generation(&self, slot: usize, value: u64) {
        self.generations[slot].set(value);
    }
}

/// Owned pages expose only interior cells. Forming an arena reference never
/// forms an exclusive reference over previously projected legacy node bytes.
pub(crate) struct NodePage<T> {
    // Rc ownership permits moving the page after legacy pointers have been
    // projected. A movable Box would retag these bytes uniquely on each move.
    values: Rc<[Cell<T>]>,
    metadata: Rc<PageMetadata>,
}

/// A projection directory retains metadata and a weak reference to the one
/// canonical Cell allocation. It cannot prolong header ownership or turn an
/// address-only caller pointer into a pointer with fabricated provenance.
pub(crate) struct NodeProjection<T> {
    values: Weak<[Cell<T>]>,
    metadata: Rc<PageMetadata>,
}
impl<T> Clone for NodeProjection<T> {
    fn clone(&self) -> Self {
        Self {
            values: self.values.clone(),
            metadata: self.metadata.clone(),
        }
    }
}
impl<T> NodeProjection<T> {
    pub(crate) fn resolve_slot(&self, slot: usize) -> Option<(*mut T, CheckedNode)> {
        let token = CheckedNode::new(self.metadata.clone(), self.metadata.current_id(slot)?)?;
        let values = self.values.upgrade()?;
        let pointer = values.get(slot)?.as_ptr();
        Some((pointer, token))
    }
}
impl<T> Drop for NodePage<T> {
    fn drop(&mut self) {
        self.metadata.invalidate_all();
    }
}
impl<T> NodePage<T> {
    pub(crate) fn try_new(
        heap: HeapIdentity,
        page: usize,
        slots: usize,
        initial: impl FnMut() -> T,
    ) -> Result<Self, HeapError> {
        let metadata = Rc::new(PageMetadata::try_new(heap, page, slots)?);
        Ok(Self {
            values: Rc::from(cells(slots, initial)?),
            metadata,
        })
    }
    pub(crate) fn metadata(&self) -> Rc<PageMetadata> {
        self.metadata.clone()
    }
    pub(crate) fn projection(&self) -> NodeProjection<T> {
        NodeProjection {
            values: Rc::downgrade(&self.values),
            metadata: self.metadata.clone(),
        }
    }
    pub(crate) fn token(&self, slot: usize) -> Option<CheckedNode> {
        CheckedNode::new(self.metadata.clone(), self.metadata.current_id(slot)?)
    }
    pub(crate) fn raw_slot(&self, slot: usize) -> Option<*mut T> {
        self.values.get(slot).map(Cell::as_ptr)
    }
    pub(crate) fn replace_inactive(&self, slot: usize, value: T) -> Result<*mut T, HeapError> {
        if !self.metadata.reusable(slot) {
            return Err(HeapError::RetiredSlot);
        }
        let cell = self.values.get(slot).ok_or(HeapError::InvalidSlot)?;
        drop(cell.replace(value));
        Ok(cell.as_ptr())
    }
    pub(crate) fn resolve(&self, id: &NodeId) -> Option<*mut T> {
        self.metadata
            .validates(id)
            .then(|| self.values[id.slot].as_ptr())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_drop_invalidates_surviving_metadata_leases() {
        let page = NodePage::try_new(HeapIdentity::new(), 0, 2, || 0).unwrap();
        let metadata = page.metadata();
        page.replace_inactive(0, 9).unwrap();
        metadata.activate(0, false).unwrap();
        let token = page.token(0).unwrap();
        let projection = page.projection();
        assert!(projection.resolve_slot(0).is_some());
        assert!(token.is_live());
        drop(page);
        assert!(!token.is_live());
        assert!(projection.resolve_slot(0).is_none());
        assert!(!metadata.reusable(0));
        assert!(!metadata.reusable(1));
        assert_eq!(metadata.activate(1, false), Err(HeapError::RetiredSlot));
    }

    #[test]
    fn same_heap_pages_with_equal_numbers_have_distinct_identities() {
        let heap = HeapIdentity::new();
        let first = NodePage::try_new(heap.clone(), 0, 1, || 0).unwrap();
        let second = NodePage::try_new(heap, 0, 1, || 0).unwrap();
        let first_id = first.metadata.activate(0, false).unwrap();
        let second_id = second.metadata.activate(0, false).unwrap();
        assert_ne!(first_id, second_id);
        assert!(first.token(0).unwrap().same_heap(&second.token(0).unwrap()));
        assert_eq!(first.resolve(&second_id), None);
        assert_eq!(second.resolve(&first_id), None);
    }

    #[test]
    fn reused_slots_reject_stale_and_foreign_heap_ids() {
        let identity = HeapIdentity::new();
        let page = NodePage::try_new(identity.clone(), 0, 4, || 0_i32).unwrap();
        let pointer = page.replace_inactive(0, 7).unwrap();
        let old = page.metadata.activate(0, false).unwrap();
        assert_eq!(page.resolve(&old), Some(pointer));
        assert!(page.metadata.release(&old));
        assert_eq!(page.resolve(&old), None);
        assert_eq!(page.replace_inactive(0, 11).unwrap(), pointer);
        let new = page.metadata.activate(0, false).unwrap();
        assert_ne!(old, new);
        assert_eq!(page.resolve(&old), None);
        assert_eq!(page.resolve(&new), Some(pointer));
        assert_eq!(page.token(0).unwrap().mark(17), Some(false));
        assert_eq!(page.token(0).unwrap().mark(17), Some(true));
        let stale = CheckedNode {
            metadata: page.metadata(),
            id: old,
        };
        assert_eq!(stale.mark(19), None);
        assert_eq!(page.metadata.epoch(0), Some(17));
        let foreign = NodePage::try_new(HeapIdentity::new(), 0, 4, || 0).unwrap();
        foreign.replace_inactive(0, 3).unwrap();
        let foreign_id = foreign.metadata.activate(0, false).unwrap();
        assert_eq!(page.resolve(&foreign_id), None);
    }
    #[test]
    fn generation_exhaustion_retires_storage_without_aliasing_old_ids() {
        let page = NodePage::try_new(HeapIdentity::new(), 0, 1, || 0).unwrap();
        page.metadata.force_generation(0, u64::MAX - 1);
        page.replace_inactive(0, 7).unwrap();
        let last = page.metadata.activate(0, false).unwrap();
        assert!(page.metadata.release(&last));
        assert_eq!(page.resolve(&last), None);
        assert_eq!(
            page.metadata.activate(0, false),
            Err(HeapError::RetiredSlot)
        );
        assert_eq!(page.replace_inactive(0, 11), Err(HeapError::RetiredSlot));
    }
    #[test]
    fn collector_runs_follow_live_and_old_slots_and_clear_reused_marks() {
        let meta = PageMetadata::try_new(HeapIdentity::new(), 0, 130).unwrap();
        let mut ids = Vec::new();
        for slot in 0..130 {
            ids.push(meta.activate(slot, slot % 2 == 0).unwrap());
        }
        assert_eq!(meta.next_run(false, 0), Some((0, 130)));
        assert_eq!(meta.next_run(true, 0), Some((0, 1)));
        assert_eq!(meta.next_run(true, 1), Some((2, 3)));
        assert_eq!(meta.mark(64, 17), Some(false));
        assert_eq!(meta.mark(64, 17), Some(true));
        assert!(meta.release(&ids[64]));
        assert_eq!(meta.next_run(false, 0), Some((0, 64)));
        assert_eq!(meta.next_run(false, 64), Some((65, 130)));
        let replacement = meta.activate(64, false).unwrap();
        assert_ne!(replacement, ids[64]);
        assert_eq!(meta.epoch(64), Some(0));
        assert_eq!(meta.mark(64, 17), Some(false));
        meta.clear_epochs();
        assert_eq!(meta.epoch(64), Some(0));
    }
}
