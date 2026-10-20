#![forbid(unsafe_code)]
//! One prepaid payload-owner slot for each canonical header slot. Publication
//! and replacement never grow a native hash table or move header storage.
use super::{AllocationCharge, PayloadLease, WorkspaceReservation};
use std::alloc::Layout;

pub(super) struct PayloadSlots {
    slots: Vec<Option<PayloadLease>>,
    // The actual data buffer drops before its original-heap storage charge.
    _charge: AllocationCharge,
}
impl PayloadSlots {
    pub(super) fn requested_bytes(slots: usize) -> Option<usize> {
        Some(Layout::array::<Option<PayloadLease>>(slots).ok()?.size())
    }
    pub(super) fn try_new(slots: usize, mut admission: WorkspaceReservation) -> Option<Self> {
        if admission.bytes() != Self::requested_bytes(slots)? {
            return None;
        }
        let mut data = Vec::new();
        data.try_reserve_exact(slots).ok()?;
        admission.try_resize(Self::requested_bytes(data.capacity())?)?;
        data.resize_with(slots, || None);
        let charge = admission.commit_allocation()?;
        Some(Self {
            slots: data,
            _charge: charge,
        })
    }
    pub(super) fn get(&self, slot: &usize) -> Option<&PayloadLease> {
        self.slots.get(*slot)?.as_ref()
    }
    pub(super) fn insert(&mut self, slot: usize, value: PayloadLease) -> Option<PayloadLease> {
        self.slots
            .get_mut(slot)
            .expect("payload index matches admitted header slots")
            .replace(value)
    }
    pub(super) fn remove(&mut self, slot: &usize) -> Option<PayloadLease> {
        self.slots.get_mut(*slot)?.take()
    }
    pub(super) fn iter(&self) -> impl Iterator<Item = (usize, &PayloadLease)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.as_ref().map(|p| (i, p)))
    }
    #[cfg(test)]
    pub(super) fn values(&self) -> impl Iterator<Item = &PayloadLease> {
        self.slots.iter().flatten()
    }
    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.slots.iter().all(Option::is_none)
    }
    #[cfg(test)]
    pub(super) fn allocation_size(&self) -> usize {
        Self::requested_bytes(self.slots.capacity()).expect("actual payload index layout")
    }
}
