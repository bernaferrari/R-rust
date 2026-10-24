//! Admission follows native regex buffers through result construction and iteration.
#![forbid(unsafe_code)]

use crate::sexp::{
    context::RError,
    instance::InstanceLiveness,
    memory::{MemoryLedger, WorkspaceReservation},
};

#[derive(Clone)]
pub(crate) struct RegexWorkspace {
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

    pub(crate) fn buffer<T>(&self, capacity: usize) -> WorkspaceBuffer<T> {
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

    pub(super) fn text(&self, capacity: usize) -> TextBuilder {
        TextBuilder {
            bytes: self.buffer(capacity),
        }
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
    fn try_reserve_capacity(&mut self, capacity: usize) -> Result<(), ()> {
        if capacity <= self.values.capacity() {
            return Ok(());
        }
        if self
            .availability
            .as_ref()
            .is_some_and(|value| !value.is_live())
        {
            return Err(());
        }
        let old_bytes = self.reservation.bytes();
        let new_bytes = capacity.checked_mul(std::mem::size_of::<T>()).ok_or(())?;
        self.reservation
            .try_resize(old_bytes.checked_add(new_bytes).ok_or(())?)
            .ok_or(())?;
        let mut replacement = Vec::new();
        if replacement.try_reserve_exact(capacity).is_err() {
            self.reservation
                .try_resize(old_bytes)
                .expect("restore admitted regex buffer");
            return Err(());
        }
        // Allocators may report more capacity than requested. Keep the old
        // vector untouched until the complete actual replacement peak is funded.
        let actual = replacement.capacity().checked_mul(std::mem::size_of::<T>());
        let admitted = actual
            .and_then(|bytes| old_bytes.checked_add(bytes))
            .and_then(|peak| self.reservation.try_resize(peak));
        if admitted.is_none() {
            drop(replacement);
            self.reservation
                .try_resize(old_bytes)
                .expect("restore regex replacement admission");
            return Err(());
        }
        replacement.append(&mut self.values);
        drop(std::mem::replace(&mut self.values, replacement));
        self.reservation
            .try_resize(actual.expect("admitted actual capacity"))
            .expect("shrink regex replacement peak");
        Ok(())
    }

    fn reserve_capacity(&mut self, capacity: usize) {
        self.try_reserve_capacity(capacity)
            .unwrap_or_else(|()| memory_error());
    }

    fn grow_for(&mut self, needed: usize) {
        let preferred = needed.max(4).checked_next_power_of_two().unwrap_or(needed);
        if self.try_reserve_capacity(preferred).is_err()
            && (needed == preferred || self.try_reserve_capacity(needed).is_err())
        {
            memory_error();
        }
    }

    pub(crate) fn push(&mut self, value: T) {
        if self.values.len() == self.values.capacity() {
            self.grow_for(
                self.values
                    .len()
                    .checked_add(1)
                    .unwrap_or_else(|| memory_error()),
            );
        }
        self.values.push(value);
    }

    pub(crate) fn clear(&mut self) {
        // Logical removal does not release the retained allocation's ticket.
        self.values.clear();
    }

    pub(super) fn resize(&mut self, length: usize, value: T)
    where
        T: Clone,
    {
        self.reserve_capacity(length);
        self.values.resize(length, value);
    }
}

impl WorkspaceBuffer<u8> {
    pub(super) fn extend_bytes(&mut self, bytes: &[u8]) {
        let needed = self
            .values
            .len()
            .checked_add(bytes.len())
            .unwrap_or_else(|| memory_error());
        if needed > self.values.capacity() {
            self.grow_for(needed);
        }
        self.values.extend_from_slice(bytes);
    }
}

// Readonly UTF8 output owns the actual allocation and its original ticket.
// No into-String escape can release admission while native storage is alive.
pub(crate) struct WorkspaceText {
    value: String,
    _reservation: WorkspaceReservation,
}
impl std::ops::Deref for WorkspaceText {
    type Target = str;
    fn deref(&self) -> &str {
        &self.value
    }
}
impl AsRef<str> for WorkspaceText {
    fn as_ref(&self) -> &str {
        &self.value
    }
}

#[cfg(test)]
impl WorkspaceText {
    pub(super) fn capacity(&self) -> usize {
        self.value.capacity()
    }
}
impl std::fmt::Debug for WorkspaceText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.value.fmt(f)
    }
}
impl<Other: AsRef<str> + ?Sized> PartialEq<Other> for WorkspaceText {
    fn eq(&self, other: &Other) -> bool {
        self.value == other.as_ref()
    }
}

pub(super) struct TextBuilder {
    bytes: WorkspaceBuffer<u8>,
}
impl TextBuilder {
    pub(super) fn push(&mut self, character: char) {
        let mut encoded = [0; 4];
        self.push_str(character.encode_utf8(&mut encoded));
    }
    pub(super) fn push_str(&mut self, text: &str) {
        self.bytes.extend_bytes(text.as_bytes());
    }
    pub(super) fn finish(self) -> Option<WorkspaceText> {
        let WorkspaceBuffer {
            values,
            reservation,
            availability: _,
        } = self.bytes;
        match String::from_utf8(values) {
            Ok(value) => Some(WorkspaceText {
                value,
                _reservation: reservation,
            }),
            Err(_) => None,
        }
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
