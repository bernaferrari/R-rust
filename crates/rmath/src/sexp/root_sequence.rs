#![forbid(unsafe_code)]
//! Canonical root occupants with stable lease identities and native ordering.

use super::root_storage::StorageError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SequenceLease(u64);

struct Entry<T> {
    lease: SequenceLease,
    value: T,
}

/// Ordered native roots retain their push identity even when removal shifts
/// their index. A callback snapshot therefore cannot rewrite a newly pushed
/// root merely because its value or native address matches the old occupant.
pub(super) struct RootSequence<T> {
    entries: Vec<Entry<T>>,
    next_lease: u64,
}

impl<T> Default for RootSequence<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            next_lease: 0,
        }
    }
}

impl<T> RootSequence<T> {
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn reserve(&mut self, additional: usize) -> Result<(), StorageError> {
        self.entries
            .try_reserve(additional)
            .map_err(|_| StorageError::Allocation)
    }

    pub(super) fn try_push(&mut self, value: T) -> Result<SequenceLease, StorageError> {
        let lease = SequenceLease(self.next_lease);
        let next = self
            .next_lease
            .checked_add(1)
            .ok_or(StorageError::GenerationExhausted)?;
        self.reserve(1)?;
        self.entries.push(Entry { lease, value });
        self.next_lease = next;
        Ok(lease)
    }

    pub(super) fn entries(&self) -> impl Iterator<Item = (SequenceLease, &T)> {
        self.entries.iter().map(|entry| (entry.lease, &entry.value))
    }

    pub(super) fn get(&self, lease: SequenceLease) -> Option<&T> {
        self.entries
            .iter()
            .find(|entry| entry.lease == lease)
            .map(|entry| &entry.value)
    }

    pub(super) fn pop_count(&mut self, count: usize) {
        self.entries
            .truncate(self.entries.len().saturating_sub(count));
    }

    pub(super) fn truncate(&mut self, depth: usize) {
        self.entries.truncate(depth);
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
    }

    pub(super) fn remove_lease(&mut self, lease: SequenceLease) -> Option<T> {
        let index = self.entries.iter().position(|entry| entry.lease == lease)?;
        Some(self.entries.remove(index).value)
    }

    #[cfg(test)]
    pub(super) fn set_next_lease_for_test(&mut self, next: u64) {
        self.next_lease = next;
    }
}

impl<T: PartialEq> RootSequence<T> {
    /// GNU R_ReleaseObject removes the first matching preserved root.
    pub(super) fn remove_first(&mut self, value: &T) -> Option<T> {
        let index = self
            .entries
            .iter()
            .position(|entry| &entry.value == value)?;
        Some(self.entries.remove(index).value)
    }

    /// GNU UNPROTECT_PTR removes the topmost matching protection entry.
    pub(super) fn remove_last(&mut self, value: &T) -> Option<T> {
        let index = self
            .entries
            .iter()
            .rposition(|entry| &entry.value == value)?;
        Some(self.entries.remove(index).value)
    }

    pub(super) fn replace_if_current(
        &mut self,
        lease: SequenceLease,
        expected: &T,
        replacement: T,
    ) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.lease == lease) else {
            return false;
        };
        if &entry.value != expected {
            return false;
        }
        entry.value = replacement;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_first_release_and_topmost_unprotect_preserve_order() {
        let mut roots = RootSequence::default();
        let first = roots.try_push(10).unwrap();
        let middle = roots.try_push(20).unwrap();
        let last = roots.try_push(10).unwrap();
        assert_eq!(roots.remove_first(&10), Some(10));
        assert_eq!(roots.get(first), None);
        assert_eq!(roots.get(last), Some(&10));
        let another = roots.try_push(10).unwrap();
        assert_eq!(roots.remove_last(&10), Some(10));
        assert_eq!(roots.get(another), None);
        assert_eq!(roots.get(middle), Some(&20));
        assert_eq!(roots.get(last), Some(&10));
        assert_eq!(
            roots.entries().map(|(_, value)| *value).collect::<Vec<_>>(),
            [20, 10]
        );
    }

    #[test]
    fn native_count_release_and_depth_restore_are_lifo() {
        let mut roots = RootSequence::default();
        let first = roots.try_push(1).unwrap();
        roots.try_push(2).unwrap();
        roots.try_push(3).unwrap();
        roots.pop_count(1);
        assert_eq!(roots.len(), 2);
        roots.truncate(1);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots.get(first), Some(&1));
        roots.pop_count(100);
        assert!(roots.is_empty());
    }

    #[test]
    fn same_value_pop_and_push_cannot_receive_an_old_snapshot_update() {
        let mut roots = RootSequence::default();
        let old = roots.try_push(1).unwrap();
        roots.pop_count(1);
        let new = roots.try_push(1).unwrap();
        assert_ne!(old, new);
        assert!(!roots.replace_if_current(old, &1, 2));
        assert_eq!(roots.get(new), Some(&1));
        assert_eq!(roots.remove_lease(old), None);
        assert_eq!(roots.get(new), Some(&1));
    }

    #[test]
    fn shifted_surviving_lease_receives_its_own_snapshot_update() {
        let mut roots = RootSequence::default();
        let removed = roots.try_push(1).unwrap();
        let survivor = roots.try_push(2).unwrap();
        roots.remove_lease(removed);
        assert!(roots.replace_if_current(survivor, &2, 3));
        assert_eq!(roots.get(survivor), Some(&3));
        assert!(!roots.replace_if_current(survivor, &2, 4));
        assert_eq!(roots.remove_lease(survivor), Some(3));
        assert!(roots.is_empty());
    }

    #[test]
    fn exhaustion_does_not_mutate_and_cleanup_still_succeeds() {
        let mut roots = RootSequence::default();
        let existing = roots.try_push(7).unwrap();
        roots.set_next_lease_for_test(u64::MAX);
        assert_eq!(roots.try_push(8), Err(StorageError::GenerationExhausted));
        assert_eq!(roots.len(), 1);
        assert_eq!(roots.get(existing), Some(&7));
        assert_eq!(roots.next_lease, u64::MAX);
        assert_eq!(roots.remove_lease(existing), Some(7));
        assert!(roots.is_empty());
    }

    #[test]
    fn capacity_failure_and_bulk_clear_preserve_identity_rules() {
        let mut roots = RootSequence::default();
        let old = roots.try_push(7).unwrap();
        let before = roots.next_lease;
        assert_eq!(
            roots.reserve(usize::MAX - roots.len()),
            Err(StorageError::Allocation)
        );
        assert_eq!(roots.get(old), Some(&7));
        assert_eq!(roots.next_lease, before);
        roots.clear();
        let new = roots.try_push(7).unwrap();
        assert_ne!(new, old);
        assert_eq!(roots.remove_lease(old), None);
        assert_eq!(roots.get(new), Some(&7));
    }
}
