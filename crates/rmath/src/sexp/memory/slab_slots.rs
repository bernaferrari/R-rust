#![forbid(unsafe_code)]
//! Admitted per-header records own payloads and link reclaimed slots without
//! hash tables, raw free pointers, or allocation during reclamation.
use super::{AllocationCharge, PayloadLease, WorkspaceReservation};
use std::{alloc::Layout, num::NonZeroUsize};

/// Local linear slot offset plus one; never an owning R value or authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SlotOffset(NonZeroUsize);
impl SlotOffset {
    pub(super) fn from_index(index: usize) -> Option<Self> {
        NonZeroUsize::new(index.checked_add(1)?).map(Self)
    }
    pub(super) fn index(self) -> usize {
        self.0.get() - 1
    }
}

pub(super) struct SlotRecord {
    payload: Option<PayloadLease>,
    next_free: Option<SlotOffset>,
}

pub(super) struct SlabSlots {
    slots: Vec<SlotRecord>,
    // The actual data buffer drops before its original-heap storage charge.
    _charge: AllocationCharge,
}
impl SlabSlots {
    pub(super) fn requested_bytes(slots: usize) -> Option<usize> {
        Some(Layout::array::<SlotRecord>(slots).ok()?.size())
    }
    pub(super) fn try_new(slots: usize, mut admission: WorkspaceReservation) -> Option<Self> {
        if admission.bytes() != Self::requested_bytes(slots)? {
            return None;
        }
        let mut data = Vec::new();
        data.try_reserve_exact(slots).ok()?;
        admission.try_resize(Self::requested_bytes(data.capacity())?)?;
        data.resize_with(slots, || SlotRecord {
            payload: None,
            next_free: None,
        });
        let charge = admission.commit_allocation()?;
        Some(Self {
            slots: data,
            _charge: charge,
        })
    }
    pub(super) fn get(&self, slot: &usize) -> Option<&PayloadLease> {
        self.slots.get(*slot)?.payload.as_ref()
    }
    pub(super) fn insert(&mut self, slot: usize, value: PayloadLease) -> Option<PayloadLease> {
        self.slots
            .get_mut(slot)
            .expect("payload index matches admitted header slots")
            .payload
            .replace(value)
    }
    pub(super) fn remove(&mut self, slot: &usize) -> Option<PayloadLease> {
        self.slots.get_mut(*slot)?.payload.take()
    }
    pub(super) fn next_free(&self, slot: usize) -> Option<Option<SlotOffset>> {
        Some(self.slots.get(slot)?.next_free)
    }
    pub(super) fn set_next_free(&mut self, slot: usize, next: Option<SlotOffset>) -> Option<()> {
        self.slots.get_mut(slot)?.next_free = next;
        Some(())
    }
    pub(super) fn iter(&self) -> impl Iterator<Item = (usize, &PayloadLease)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.payload.as_ref().map(|p| (i, p)))
    }
    #[cfg(test)]
    pub(super) fn values(&self) -> impl Iterator<Item = &PayloadLease> {
        self.slots
            .iter()
            .filter_map(|record| record.payload.as_ref())
    }
    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.slots.iter().all(|record| record.payload.is_none())
    }
    #[cfg(test)]
    pub(super) fn allocation_size(&self) -> usize {
        Self::requested_bytes(self.slots.capacity()).expect("actual payload index layout")
    }
}
