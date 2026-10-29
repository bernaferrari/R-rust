//! Fixed numerical buffers charged to the original session's byte ledger.
#![forbid(unsafe_code)]

use crate::sexp::{memory::WorkspaceReservation, owner::OwnerToken};

fn memory_error() -> String {
    "cov/cor workspace exceeds session allocation limit".into()
}

pub(super) struct Workspace<T> {
    // Fields drop in declaration order: storage disappears before its credit.
    values: Vec<T>,
    _reservation: WorkspaceReservation,
}

impl<T: Copy> Workspace<T> {
    pub(super) fn filled(owner: OwnerToken<'_>, length: usize, value: T) -> Result<Self, String> {
        Self::filled_with(owner, length, value, |length| {
            let mut values = Vec::new();
            values
                .try_reserve_exact(length)
                .map_err(|_| memory_error())?;
            Ok(values)
        })
    }

    fn filled_with(
        owner: OwnerToken<'_>,
        length: usize,
        value: T,
        allocate: impl FnOnce(usize) -> Result<Vec<T>, String>,
    ) -> Result<Self, String> {
        owner.require_active().map_err(|e| e.to_string())?;
        let bytes = length
            .checked_mul(size_of::<T>())
            .ok_or_else(memory_error)?;
        let mut reservation = owner
            .with_arena(|arena| arena.reserve_workspace(bytes))
            .map_err(|e| e.to_string())?
            .ok_or_else(memory_error)?;
        let mut values = allocate(length)?;
        let actual = values.capacity().checked_mul(size_of::<T>());
        if values.capacity() < length
            || actual
                .and_then(|bytes| reservation.try_resize(bytes))
                .is_none()
        {
            drop(values);
            return Err(memory_error());
        }
        owner.require_active().map_err(|e| e.to_string())?;
        values.resize_with(length, || value);
        Ok(Self {
            values,
            _reservation: reservation,
        })
    }
}

impl<T> std::ops::Deref for Workspace<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.values
    }
}
impl<T> std::ops::DerefMut for Workspace<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.values
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{RSession, memory::ArenaBudget};

    #[test]
    fn workspace_accounts_actual_capacity_and_denies_before_allocation() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let values = Workspace::filled_with(owner, 3, 0u64, |n| {
            let mut v = Vec::new();
            v.try_reserve_exact(n * 2).map_err(|_| memory_error())?;
            Ok(v)
        })
        .unwrap();
        let charge = values.values.capacity() * size_of::<u64>();
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), charge);
        let total = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(total + charge, 0)))
            .unwrap();
        let denied =
            Workspace::filled_with(owner, 1, 0u64, |_| panic!("allocation before admission"));
        assert!(denied.is_err());
        drop(values);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        let values = Workspace::filled(owner, 3, 7u64).unwrap();
        assert_eq!(&*values, &[7, 7, 7]);
        drop(values);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }

    #[test]
    fn workspace_surplus_denial_and_allocator_failure_release_credit() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let baseline = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(baseline + 24, 0)))
            .unwrap();
        let denied = Workspace::filled_with(owner, 3, 0u64, |n| {
            let mut v = Vec::new();
            v.try_reserve_exact(n * 2).map_err(|_| memory_error())?;
            Ok(v)
        });
        assert!(denied.is_err());
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        assert!(Workspace::filled_with(owner, 3, 0u64, |_| Err(memory_error())).is_err());
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _values = Workspace::filled(owner, 3, 0u64).unwrap();
            panic!("fixture unwind");
        }));
        assert!(result.is_err());
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }

    #[test]
    fn workspace_releases_original_ledger_after_owner_teardown() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let anchor = owner
            .node_factory()
            .character("anchor")
            .unwrap()
            .into_owned()
            .unwrap();
        let ledger = anchor.allocation().unwrap().heap_identity().byte_ledger();
        let values = Workspace::filled(owner, 3, 9u64).unwrap();
        let charge = values.values.capacity() * size_of::<u64>();
        drop(anchor);
        drop(session);
        let mut other = RSession::new_for_gc_tests();
        let before = other.with_arena(|a| a.total_bytes_allocated()).unwrap();
        assert_eq!(ledger.workspace_bytes_for_test(), charge);
        assert_eq!(&*values, &[9, 9, 9]);
        drop(values);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        assert_eq!(
            other.with_arena(|a| a.total_bytes_allocated()).unwrap(),
            before
        );
    }
    #[test]
    fn workspace_revocation_during_allocation_releases_original_credit() {
        let mut session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let weak = owner.weak_owner().unwrap();
        let anchor = owner
            .node_factory()
            .character("anchor")
            .unwrap()
            .into_owned()
            .unwrap();
        let ledger = anchor.allocation().unwrap().heap_identity().byte_ledger();
        let result = crate::sexp::owner::with_runtime(&weak, |access| {
            access.with_native(|owner| {
                Ok(Workspace::filled_with(owner, 3, 0u64, |n| {
                    session.close();
                    let mut values = Vec::new();
                    values.try_reserve_exact(n).map_err(|_| memory_error())?;
                    Ok(values)
                }))
            })
        });
        assert!(matches!(
            result,
            Err(crate::sexp::SexpError::RootUnavailable)
        ));
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
    }
}
