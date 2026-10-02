#![forbid(unsafe_code)]
//! Stable root leases, independent of native pointer representations.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SlotId {
    pub(super) index: usize,
    pub(super) generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StorageError {
    Allocation,
    GenerationExhausted,
}

struct Entry<T> {
    generation: u64,
    value: Option<T>,
    managed: bool,
}

/// Every occupied slot has one lease generation. Vacancies have no lease;
/// releasing a value therefore needs neither allocation nor a new generation.
pub(super) struct RootStorage<T> {
    entries: Vec<Entry<T>>,
    free: Vec<usize>,
    next_generation: u64,
}

impl<T> Default for RootStorage<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            free: Vec::new(),
            next_generation: 0,
        }
    }
}

impl<T> RootStorage<T> {
    pub(super) fn checkpoint(&self) -> u64 {
        self.next_generation
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    fn reserve_new_slots(&mut self, additional: usize) -> Result<(), StorageError> {
        let needed = self
            .entries
            .len()
            .checked_add(additional)
            .ok_or(StorageError::Allocation)?;
        self.entries
            .try_reserve(additional)
            .map_err(|_| StorageError::Allocation)?;
        // Reserve cleanup capacity before publishing any root. At most one
        // free-list entry exists per physical slot, so release never grows it.
        if self.free.capacity() < needed {
            self.free
                .try_reserve(needed.saturating_sub(self.free.len()))
                .map_err(|_| StorageError::Allocation)?;
        }
        Ok(())
    }

    pub(super) fn try_claim(&mut self, value: T, managed: bool) -> Result<SlotId, StorageError> {
        let generation = self.next_generation;
        let next = generation
            .checked_add(1)
            .ok_or(StorageError::GenerationExhausted)?;
        if self.free.is_empty() {
            self.reserve_new_slots(1)?;
        }
        // All fallible work precedes these changes, including managed status.
        let index = if let Some(index) = self.free.pop() {
            self.entries[index] = Entry {
                generation,
                value: Some(value),
                managed,
            };
            index
        } else {
            let index = self.entries.len();
            self.entries.push(Entry {
                generation,
                value: Some(value),
                managed,
            });
            index
        };
        self.next_generation = next;
        Ok(SlotId { index, generation })
    }

    pub(super) fn get(&self, slot: SlotId) -> Option<&T> {
        let entry = self.entries.get(slot.index)?;
        (entry.generation == slot.generation)
            .then_some(entry.value.as_ref())
            .flatten()
    }

    pub(super) fn at(&self, index: usize) -> Option<(SlotId, &T)> {
        let entry = self.entries.get(index)?;
        Some((
            SlotId {
                index,
                generation: entry.generation,
            },
            entry.value.as_ref()?,
        ))
    }

    pub(super) fn entries(&self) -> impl Iterator<Item = (SlotId, &T)> {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                Some((
                    SlotId {
                        index,
                        generation: entry.generation,
                    },
                    entry.value.as_ref()?,
                ))
            })
    }

    pub(super) fn retain_managed(&mut self, slot: SlotId) -> bool {
        let Some(entry) = self.entries.get_mut(slot.index) else {
            return false;
        };
        if entry.generation != slot.generation || entry.value.is_none() {
            return false;
        }
        entry.managed = true;
        true
    }

    pub(super) fn replace(&mut self, slot: SlotId, value: T) -> bool {
        let Some(entry) = self.entries.get_mut(slot.index) else {
            return false;
        };
        if entry.generation != slot.generation || entry.value.is_none() {
            return false;
        }
        entry.value = Some(value);
        true
    }

    pub(super) fn release(&mut self, slot: SlotId) -> bool {
        let Some(entry) = self.entries.get_mut(slot.index) else {
            return false;
        };
        if entry.generation != slot.generation || entry.value.is_none() {
            return false;
        }
        entry.value = None;
        entry.managed = false;
        // try_claim reserved room for all possible vacancies in advance.
        self.free.push(slot.index);
        while self
            .entries
            .last()
            .is_some_and(|entry| entry.value.is_none())
        {
            let tail = self.entries.len() - 1;
            self.entries.pop();
            if let Some(position) = self.free.iter().position(|&index| index == tail) {
                self.free.swap_remove(position);
            }
        }
        true
    }

    pub(super) fn restore(&mut self, checkpoint: u64) {
        // Reverse order remains valid when releasing a tail collapses it.
        for index in (0..self.entries.len()).rev() {
            let slot = self.entries.get(index).and_then(|entry| {
                (entry.value.is_some() && !entry.managed && entry.generation >= checkpoint)
                    .then_some(SlotId {
                        index,
                        generation: entry.generation,
                    })
            });
            if let Some(slot) = slot {
                self.release(slot);
            }
        }
    }

    pub(super) fn truncate(&mut self, depth: usize) {
        self.entries.truncate(depth);
        self.free.retain(|&index| index < depth);
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.free.clear();
    }

    #[cfg(any(test, kani))]
    pub(super) fn set_next_generation_for_test(&mut self, generation: u64) {
        self.next_generation = generation;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arbitrary_release_and_reuse_keep_other_leases() {
        let mut roots = RootStorage::default();
        let first = roots.try_claim(10, false).unwrap();
        let second = roots.try_claim(20, true).unwrap();
        let third = roots.try_claim(30, false).unwrap();
        let entries_capacity = roots.entries.capacity();
        let free_capacity = roots.free.capacity();
        assert!(roots.release(second));
        let replacement = roots.try_claim(40, true).unwrap();
        assert_eq!(replacement.index, second.index);
        assert_ne!(replacement.generation, second.generation);
        assert!(!roots.release(second));
        assert_eq!(roots.get(replacement), Some(&40));
        assert!(roots.release(first));
        assert_eq!(roots.get(third), Some(&30));
        assert!(roots.release(third));
        assert_eq!(roots.get(replacement), Some(&40));
        assert!(roots.release(replacement));
        assert_eq!(roots.len(), 0);
        assert_eq!(roots.entries.capacity(), entries_capacity);
        assert_eq!(roots.free.capacity(), free_capacity);
    }

    #[test]
    fn exhausted_claim_is_atomic_but_release_still_succeeds() {
        let mut roots = RootStorage::default();
        let existing = roots.try_claim(7, true).unwrap();
        roots.set_next_generation_for_test(u64::MAX);
        assert_eq!(
            roots.try_claim(8, false),
            Err(StorageError::GenerationExhausted)
        );
        assert_eq!(roots.get(existing), Some(&7));
        assert_eq!(roots.len(), 1);
        assert!(roots.free.is_empty());
        assert_eq!(roots.checkpoint(), u64::MAX);
        assert!(roots.release(existing));
        assert_eq!(roots.get(existing), None);
        assert_eq!(roots.len(), 0);
        assert_eq!(roots.checkpoint(), u64::MAX);
    }

    #[test]
    fn failed_capacity_reservation_preserves_root_and_checkpoint() {
        let mut roots = RootStorage::default();
        let existing = roots.try_claim(7, true).unwrap();
        let checkpoint = roots.checkpoint();
        assert_eq!(
            roots.reserve_new_slots(usize::MAX - roots.len()),
            Err(StorageError::Allocation)
        );
        assert_eq!(roots.get(existing), Some(&7));
        assert_eq!(roots.len(), 1);
        assert!(roots.free.is_empty());
        assert_eq!(roots.checkpoint(), checkpoint);
    }

    #[test]
    fn scope_restore_removes_reused_unmanaged_slots_and_preserves_managed() {
        let mut roots = RootStorage::default();
        let first = roots.try_claim(1, true).unwrap();
        let second = roots.try_claim(2, true).unwrap();
        roots.release(first);
        let checkpoint = roots.checkpoint();
        let reused = roots.try_claim(3, false).unwrap();
        let managed = roots.try_claim(4, true).unwrap();
        roots.restore(checkpoint);
        assert_eq!(roots.get(reused), None);
        assert_eq!(roots.get(second), Some(&2));
        assert_eq!(roots.get(managed), Some(&4));
        let replacement = roots.try_claim(5, false).unwrap();
        assert!(!roots.replace(reused, 6));
        assert_eq!(roots.get(replacement), Some(&5));
    }
}
