#![forbid(unsafe_code)]
//! Root views own original-heap admission through storage and consuming iteration.
use super::root_storage::StorageError;
use crate::sexp::memory::{MemoryLedger, WorkspaceReservation};
use std::{alloc::Layout, fmt, ops::Deref};

pub(crate) struct RootSnapshot<T> {
    entries: Vec<T>,
    // Deallocate actual storage before releasing its original workspace ticket.
    admission: WorkspaceReservation,
}
impl<T> RootSnapshot<T> {
    pub(crate) fn as_slice(&self) -> &[T] {
        self.entries.as_slice()
    }
    /// Caller counts and visits a stable root channel without executing callbacks.
    pub(super) fn try_collect(
        ledger: &MemoryLedger,
        count: usize,
        values: impl IntoIterator<Item = T>,
    ) -> Result<Self, StorageError> {
        let failed = || StorageError::Allocation;
        let bytes = |n| Layout::array::<T>(n).ok().map(|layout| layout.size());
        let mut admission = ledger
            .reserve(bytes(count).ok_or_else(failed)?)
            .ok_or_else(failed)?;
        let mut entries = Vec::new();
        entries.try_reserve_exact(count).map_err(|_| failed())?;
        admission
            .try_resize(bytes(entries.capacity()).ok_or_else(failed)?)
            .ok_or_else(failed)?;
        for value in values {
            // Even a caller count mismatch cannot grow an unfunded buffer.
            if entries.len() == count {
                return Err(failed());
            }
            entries.push(value);
        }
        if entries.len() != count {
            return Err(failed());
        }
        Ok(Self { entries, admission })
    }
}
impl<T> Deref for RootSnapshot<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}
impl<T> AsRef<[T]> for RootSnapshot<T> {
    fn as_ref(&self) -> &[T] {
        &self.entries
    }
}
impl<T: fmt::Debug> fmt::Debug for RootSnapshot<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.entries.fmt(f)
    }
}
impl<T: PartialEq, Other: AsRef<[T]>> PartialEq<Other> for RootSnapshot<T> {
    fn eq(&self, other: &Other) -> bool {
        self.entries.as_slice() == other.as_ref()
    }
}

pub(crate) struct RootSnapshotIter<T> {
    entries: std::vec::IntoIter<T>,
    // Vec::IntoIter retains its allocation, including after reaching exhaustion.
    _admission: WorkspaceReservation,
}
impl<T> IntoIterator for RootSnapshot<T> {
    type Item = T;
    type IntoIter = RootSnapshotIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        RootSnapshotIter {
            entries: self.entries.into_iter(),
            _admission: self.admission,
        }
    }
}
impl<'a, T> IntoIterator for &'a RootSnapshot<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}
impl<T> Iterator for RootSnapshotIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        self.entries.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.entries.size_hint()
    }
}
impl<T> ExactSizeIterator for RootSnapshotIter<T> {}
impl<T> DoubleEndedIterator for RootSnapshotIter<T> {
    fn next_back(&mut self) -> Option<T> {
        self.entries.next_back()
    }
}
