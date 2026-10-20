//! Admission follows native regex buffers through result construction and iteration.
#![forbid(unsafe_code)]

use crate::sexp::{
    context::RError,
    instance::InstanceLiveness,
    memory::{MemoryLedger, WorkspaceReservation},
};

#[derive(Clone)]
pub(super) struct RegexWorkspace {
    ledger: MemoryLedger,
    availability: Option<InstanceLiveness>,
}

pub(super) fn memory_error() -> ! {
    std::panic::panic_any(RError {
        message: "regex workspace exceeds session memory limit".into(),
    })
}

impl RegexWorkspace {
    pub(super) fn new(ledger: MemoryLedger, availability: InstanceLiveness) -> Self {
        Self {
            ledger,
            availability: Some(availability),
        }
    }

    #[cfg(test)]
    pub(super) fn standalone() -> Self {
        Self {
            ledger: MemoryLedger::new(),
            availability: None,
        }
    }

    pub(super) fn buffer<T>(&self, capacity: usize) -> WorkspaceBuffer<T> {
        self.require_live();
        let reservation = self.ledger.reserve(0).unwrap_or_else(|| memory_error());
        let mut buffer = WorkspaceBuffer {
            values: Vec::new(),
            reservation,
            availability: self.availability.clone(),
        };
        buffer.reserve_capacity(capacity);
        buffer
    }

    fn require_live(&self) {
        if self
            .availability
            .as_ref()
            .is_some_and(|value| !value.is_live())
        {
            memory_error();
        }
    }
}

pub(crate) struct WorkspaceBuffer<T> {
    // Drop backing elements before releasing their charge.
    values: Vec<T>,
    reservation: WorkspaceReservation,
    availability: Option<InstanceLiveness>,
}

impl<T> WorkspaceBuffer<T> {
    fn reserve_capacity(&mut self, capacity: usize) {
        if capacity <= self.values.capacity() {
            return;
        }
        if self
            .availability
            .as_ref()
            .is_some_and(|value| !value.is_live())
        {
            memory_error();
        }
        let old_bytes = self.reservation.bytes();
        let new_bytes = capacity
            .checked_mul(std::mem::size_of::<T>())
            .unwrap_or_else(|| memory_error());
        // Both allocations coexist while initialized elements move to the new buffer.
        self.reservation
            .try_resize(
                old_bytes
                    .checked_add(new_bytes)
                    .unwrap_or_else(|| memory_error()),
            )
            .unwrap_or_else(|| memory_error());
        let mut replacement = Vec::new();
        if replacement.try_reserve_exact(capacity).is_err() {
            self.reservation
                .try_resize(old_bytes)
                .expect("restore admitted regex buffer");
            memory_error();
        }
        replacement.append(&mut self.values);
        drop(std::mem::replace(&mut self.values, replacement));
        self.reservation
            .try_resize(new_bytes)
            .expect("shrink regex replacement peak");
    }

    pub(super) fn push(&mut self, value: T) {
        if self.values.len() == self.values.capacity() {
            let capacity = self
                .values
                .len()
                .checked_add(1)
                .and_then(|n| n.max(4).checked_next_power_of_two())
                .unwrap_or_else(|| memory_error());
            self.reserve_capacity(capacity);
        }
        self.values.push(value);
    }

    pub(super) fn resize(&mut self, length: usize, value: T)
    where
        T: Clone,
    {
        self.reserve_capacity(length);
        self.values.resize(length, value);
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for WorkspaceBuffer<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.values.fmt(f)
    }
}
impl<T> std::ops::Deref for WorkspaceBuffer<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.values
    }
}
impl<T> std::ops::DerefMut for WorkspaceBuffer<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.values
    }
}

pub(crate) struct WorkspaceIntoIter<T> {
    values: std::vec::IntoIter<T>,
    _reservation: WorkspaceReservation,
}
impl<T> IntoIterator for WorkspaceBuffer<T> {
    type Item = T;
    type IntoIter = WorkspaceIntoIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        WorkspaceIntoIter {
            values: self.values.into_iter(),
            _reservation: self.reservation,
        }
    }
}
impl<T> Iterator for WorkspaceIntoIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        self.values.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.values.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use crate::sexp::{RSession, memory::ArenaBudget};

    #[test]
    fn replacement_peak_denial_preserves_storage_and_charge() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let mut values = super::super::regex_workspace().buffer::<u64>(8);
        for n in 0..8 {
            values.push(n);
        }
        owner
            .with_arena(|arena| {
                let used = arena.total_bytes_allocated();
                arena.set_budget(ArenaBudget::new(used + 128, 0));
            })
            .unwrap();
        // A final 16-element allocation fits; its coexistence with the old
        // 8-element backing does not. Reject before moving any old value.
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                values.push(8);
            }))
            .is_err()
        );
        assert_eq!(&*values, &[0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            64
        );
        drop(values);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
    }

    #[test]
    fn partial_iteration_retains_backing_admission_until_drop() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let mut values = super::super::regex_workspace().buffer::<u64>(2);
        values.push(10);
        values.push(20);
        let mut iter = values.into_iter();
        assert_eq!(iter.next(), Some(10));
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            16
        );
        drop(iter);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
    }

    #[test]
    fn closed_owner_buffers_remain_readable_but_cannot_allocate() {
        let mut session = RSession::new_for_gc_tests();
        let workspace = super::super::regex_workspace();
        let mut values = workspace.buffer::<u64>(1);
        values.push(10);
        session.close();
        assert_eq!(values[0], 10);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                values.push(20);
            }))
            .is_err()
        );
        assert_eq!(&*values, &[10]);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                workspace.buffer::<u64>(1)
            }))
            .is_err()
        );
    }
}
