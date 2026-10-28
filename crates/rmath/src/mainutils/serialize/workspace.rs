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

pub(super) type WorkspaceVec<T> = ScopedWorkspaceVec<'static, T>;

pub(super) struct ScopedWorkspaceVec<'session, T> {
    values: Vec<T>,
    reservation: WorkspaceReservation,
    owner: StoredOwner<'session>,
}

impl<T> WorkspaceVec<T> {
    pub(super) fn new(owner: &OwnerToken<'_>) -> Result<Self, String> {
        let stored = StoredOwner::from_token(*owner)
            .into_owned()
            .map_err(|e| e.to_string())?;
        Ok(Self::with_reservation(reserve(owner, 0)?, stored))
    }
}

impl<'session, T> ScopedWorkspaceVec<'session, T> {
    pub(super) fn scoped(owner: &OwnerToken<'session>) -> Result<Self, String> {
        Ok(Self::with_reservation(
            reserve(owner, 0)?,
            StoredOwner::from_token(*owner),
        ))
    }

    pub(super) fn with_reservation(
        reservation: WorkspaceReservation,
        owner: StoredOwner<'session>,
    ) -> Self {
        Self {
            values: Vec::new(),
            reservation,
            owner,
        }
    }

    pub(super) fn reserve_len(&mut self, length: usize) -> Result<(), String> {
        self.reserve_len_with_allocator(length, |capacity| {
            let mut values = Vec::new();
            values
                .try_reserve_exact(capacity)
                .map_err(|_| memory_error())?;
            Ok(values)
        })
    }

    fn reserve_len_with_allocator(
        &mut self,
        length: usize,
        allocate: impl FnOnce(usize) -> Result<Vec<T>, String>,
    ) -> Result<(), String> {
        self.owner.require_active().map_err(|e| e.to_string())?;
        if length <= self.values.capacity() {
            return Ok(());
        }
        let old_capacity = self.values.capacity();
        let requested = length
            .max(8)
            .checked_next_power_of_two()
            .ok_or_else(memory_error)?;
        let width = std::mem::size_of::<T>();
        let old_bytes = old_capacity.checked_mul(width).ok_or_else(memory_error)?;
        let bytes = requested.checked_mul(width).ok_or_else(memory_error)?;
        let peak = old_bytes.checked_add(bytes).ok_or_else(memory_error)?;
        if self.reservation.try_resize(peak).is_none() {
            self.owner
                .with_arena(|arena| arena.release_bytecode_string_hints())
                .map_err(|e| e.to_string())?;
            self.reservation.try_resize(peak).ok_or_else(memory_error)?;
        }
        let mut replacement = match allocate(requested) {
            Ok(value) => value,
            Err(error) => {
                self.reservation
                    .try_resize(old_bytes)
                    .expect("restore admitted storage");
                return Err(error);
            }
        };
        let actual = replacement.capacity();
        let actual_bytes = actual.checked_mul(width);
        let admitted = actual_bytes
            .and_then(|bytes| old_bytes.checked_add(bytes))
            .and_then(|peak| self.reservation.try_resize(peak));
        if actual < requested || admitted.is_none() {
            drop(replacement);
            self.reservation
                .try_resize(old_bytes)
                .expect("restore admitted storage");
            return Err(memory_error());
        }
        replacement.clear();
        replacement.append(&mut self.values);
        drop(std::mem::replace(&mut self.values, replacement));
        self.reservation
            .try_resize(actual_bytes.expect("admitted actual capacity"))
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

impl<T> std::ops::Deref for ScopedWorkspaceVec<'_, T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.values
    }
}
impl<T> std::ops::DerefMut for ScopedWorkspaceVec<'_, T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.values
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{RSession, memory::ArenaBudget};

    #[test]
    fn serialization_workspace_actual_capacity_is_charged_and_surplus_denial_recovers() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let mut values = WorkspaceVec::<u64>::new(&owner).unwrap();
        values
            .reserve_len_with_allocator(1, |requested| {
                let mut replacement = Vec::new();
                replacement
                    .try_reserve_exact(requested * 2)
                    .map_err(|_| memory_error())?;
                Ok(replacement)
            })
            .unwrap();
        let old = values.values.capacity();
        let width = std::mem::size_of::<u64>();
        assert!(old >= 16);
        assert_eq!(
            owner.with_arena(|a| a.workspace_bytes()).unwrap(),
            old * width
        );
        for value in 0..old {
            values.push(value as u64).unwrap();
        }
        let baseline = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        let limit = baseline + old * 3 * width;
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(limit, 0)))
            .unwrap();
        assert!(
            values
                .reserve_len_with_allocator(old + 1, |requested| {
                    let mut replacement = Vec::new();
                    replacement
                        .try_reserve_exact(requested * 2)
                        .map_err(|_| memory_error())?;
                    Ok(replacement)
                })
                .is_err()
        );
        assert_eq!(values.len(), old);
        assert!(
            values
                .iter()
                .enumerate()
                .all(|(index, value)| *value == index as u64)
        );
        assert_eq!(
            owner.with_arena(|a| a.workspace_bytes()).unwrap(),
            old * width
        );
        assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), limit);
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
            .unwrap();
        values.push(old as u64).unwrap();
        assert_eq!(
            owner.with_arena(|a| a.workspace_bytes()).unwrap(),
            values.values.capacity() * width
        );
        drop(values);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }

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
