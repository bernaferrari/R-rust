//! Native containers retain admission from their original session.
#![forbid(unsafe_code)]

use crate::sexp::{
    memory::WorkspaceReservation,
    owner::{OwnerToken, StoredOwner},
};

pub(super) fn memory_error() -> String {
    "serialization workspace exceeds session memory limit".into()
}

pub(super) fn reserve(
    owner: &OwnerToken<'_>,
    bytes: usize,
) -> Result<WorkspaceReservation, String> {
    owner
        .with_arena(|a| a.reserve_workspace(bytes))
        .map_err(|e| e.to_string())?
        .ok_or_else(memory_error)
}

pub(super) struct WorkspaceVec<T> {
    values: Vec<T>,
    reservation: WorkspaceReservation,
    owner: StoredOwner<'static>,
}

impl<T> WorkspaceVec<T> {
    pub(super) fn new(owner: &OwnerToken<'_>) -> Result<Self, String> {
        let stored = StoredOwner::from_token(*owner)
            .into_owned()
            .map_err(|e| e.to_string())?;
        Ok(Self::with_reservation(reserve(owner, 0)?, stored))
    }

    pub(super) fn with_reservation(
        reservation: WorkspaceReservation,
        owner: StoredOwner<'static>,
    ) -> Self {
        Self {
            values: Vec::new(),
            reservation,
            owner,
        }
    }

    pub(super) fn reserve_len(&mut self, length: usize) -> Result<(), String> {
        self.owner.require_active().map_err(|e| e.to_string())?;
        if length <= self.values.capacity() {
            return Ok(());
        }
        let old_capacity = self.values.capacity();
        let capacity = length
            .max(8)
            .checked_next_power_of_two()
            .ok_or_else(memory_error)?;
        let width = std::mem::size_of::<T>();
        let old_bytes = old_capacity.checked_mul(width).ok_or_else(memory_error)?;
        let bytes = capacity.checked_mul(width).ok_or_else(memory_error)?;
        let peak = old_bytes.checked_add(bytes).ok_or_else(memory_error)?;
        if self.reservation.try_resize(peak).is_none() {
            // No Vec element/payload or arena loan spans this call. Reclaim
            // optional weak storage in the original owner only; do not run GC.
            self.owner
                .with_arena(|arena| arena.release_bytecode_string_hints())
                .map_err(|e| e.to_string())?;
            self.reservation.try_resize(peak).ok_or_else(memory_error)?;
        }
        let mut replacement = Vec::new();
        if replacement.try_reserve_exact(capacity).is_err() {
            self.reservation
                .try_resize(old_bytes)
                .expect("restore admitted storage");
            return Err(memory_error());
        }
        replacement.append(&mut self.values);
        drop(std::mem::replace(&mut self.values, replacement));
        self.reservation
            .try_resize(bytes)
            .expect("shrink replacement peak");
        Ok(())
    }

    pub(super) fn push(&mut self, value: T) -> Result<(), String> {
        self.reserve_len(self.values.len().checked_add(1).ok_or_else(memory_error)?)?;
        self.values.push(value);
        Ok(())
    }

    pub(super) fn resize(&mut self, length: usize, value: T) -> Result<(), String>
    where
        T: Clone,
    {
        self.reserve_len(length)?;
        self.values.resize(length, value);
        Ok(())
    }
}

impl<T> std::ops::Deref for WorkspaceVec<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.values
    }
}
impl<T> std::ops::DerefMut for WorkspaceVec<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.values
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{RSession, memory::ArenaBudget};

    #[test]
    fn serialization_workspace_replacement_charges_both_live_allocations() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let mut values = WorkspaceVec::<u64>::new(&owner).unwrap();
        for n in 0..8 {
            values.push(n).unwrap();
        }
        let bytes = owner.with_arena(|a| a.workspace_bytes()).unwrap();
        let allocated = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(allocated + 2 * bytes, 0)))
            .unwrap();
        assert!(
            values.push(8).is_err(),
            "final storage fits, but replacement coexists with the old buffer"
        );
        assert_eq!(&*values, &[0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), bytes);
        drop(values);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }
}

#[cfg(test)]
#[path = "workspace_vec_tests.rs"]
mod workspace_vec_tests;
