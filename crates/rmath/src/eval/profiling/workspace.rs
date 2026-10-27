//! Fixed-capacity profiler buffers retain admission from their original ledger.
#![forbid(unsafe_code)]
use crate::sexp::{memory::WorkspaceReservation, object::SexpResult, owner::OwnerToken};

pub(super) struct ProfileBuffer<T> {
    // Backing storage drops before its reservation.
    values: Vec<T>,
    reservation: WorkspaceReservation,
}
impl<T> ProfileBuffer<T> {
    pub(super) fn with_capacity(owner: OwnerToken<'_>, count: usize) -> SexpResult<Option<Self>> {
        owner.require_active()?;
        let Some(bytes) = count.checked_mul(std::mem::size_of::<T>()) else {
            return Ok(None);
        };
        let Some(mut reservation) = owner.with_arena(|arena| arena.reserve_workspace(bytes))?
        else {
            return Ok(None);
        };
        let mut values = Vec::new();
        if values.try_reserve_exact(count).is_err() {
            return Ok(None);
        }
        let Some(actual) = values.capacity().checked_mul(std::mem::size_of::<T>()) else {
            return Ok(None);
        };
        if reservation.try_resize(actual).is_none() {
            return Ok(None);
        }
        Ok(Some(Self {
            values,
            reservation,
        }))
    }
    pub(super) fn push(&mut self, value: T) -> bool {
        if self.values.len() == self.values.capacity() {
            return false;
        }
        self.values.push(value);
        true
    }
    pub(super) fn copy_from_slice(&mut self, source: &[T]) -> bool
    where
        T: Copy,
    {
        if source.len() > self.values.capacity() - self.values.len() {
            return false;
        }
        self.values.extend_from_slice(source);
        true
    }
    pub(super) fn into_parts(self) -> (Vec<T>, WorkspaceReservation) {
        (self.values, self.reservation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{memory::ArenaBudget, session::RSession};
    #[test]
    fn profiler_buffer_denial_precedes_allocation_and_preserves_held_tickets() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let held = owner
            .with_arena(|arena| arena.reserve_workspace(128).unwrap())
            .unwrap();
        let before = owner
            .with_arena(|arena| {
                let before = (arena.total_bytes_allocated(), arena.workspace_bytes());
                arena.set_budget(ArenaBudget::new(before.0 + before.1 + 63, 0));
                before
            })
            .unwrap();
        assert!(
            ProfileBuffer::<u64>::with_capacity(owner, 8)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            owner
                .with_arena(|arena| (arena.total_bytes_allocated(), arena.workspace_bytes()))
                .unwrap(),
            before
        );
        drop(held);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
    }
    #[test]
    fn profiler_buffer_capacity_never_grows_without_new_admission() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let mut buffer = ProfileBuffer::<u64>::with_capacity(owner, 2)
            .unwrap()
            .unwrap();
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            16
        );
        assert!(buffer.push(1));
        assert!(buffer.push(2));
        assert!(!buffer.push(3));
        let (values, reservation) = buffer.into_parts();
        assert_eq!(values, [1, 2]);
        drop(values);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            16
        );
        drop(reservation);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
    }
}

#[cfg(test)]
mod owner_tests {
    use super::*;
    use crate::sexp::session::RSession;
    #[test]
    fn profiler_buffer_retired_owner_release_does_not_touch_replacement() {
        let original = RSession::new_for_gc_tests();
        let (buffer, ledger, weak) = original.with_active(|| {
            let owner = original.owner_token().unwrap();
            let buffer = ProfileBuffer::<u64>::with_capacity(owner, 8)
                .unwrap()
                .unwrap();
            let ledger = owner
                .with_arena(|arena| arena.heap_identity().byte_ledger())
                .unwrap();
            (buffer, ledger, owner.weak_owner().unwrap())
        });
        drop(original);
        assert_eq!(weak.allocation_strong_count(), 0);
        assert_eq!(ledger.workspace_bytes_for_test(), 64);
        let replacement = RSession::new_for_gc_tests();
        replacement.with_active(|| {
            let owner = replacement.owner_token().unwrap();
            let held = owner
                .with_arena(|arena| arena.reserve_workspace(128).unwrap())
                .unwrap();
            drop(buffer);
            assert_eq!(ledger.workspace_bytes_for_test(), 0);
            assert_eq!(
                owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
                128
            );
            drop(held);
        });
    }
}
