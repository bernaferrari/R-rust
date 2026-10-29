//! Exact KS matrix buffers charged to their original session ledger.
#![forbid(unsafe_code)]

use crate::sexp::{memory::WorkspaceReservation, owner::OwnerToken};

pub(super) fn memory_error() -> String {
    "exact KS workspace exceeds session allocation limit".into()
}

pub(super) fn dimensions(n: i32, q: f64) -> Result<(usize, usize), String> {
    if n < 1 {
        return Err("exact KS sample size must be positive".into());
    }
    if !q.is_finite() || !(0.0..=1.0).contains(&q) {
        return Err("exact KS quantile must be finite and between 0 and 1".into());
    }
    let k = (f64::from(n) * q) as usize;
    let k = k.checked_add(1).ok_or_else(memory_error)?;
    let m = k
        .checked_mul(2)
        .and_then(|m| m.checked_sub(1))
        .ok_or_else(memory_error)?;
    // Matrix polynomial powers use the signed GNU integer exponent API.
    if m > i32::MAX as usize {
        return Err(memory_error());
    }
    Ok((k, m))
}

pub(super) struct Workspace {
    // Declaration order releases storage before returning its byte credit.
    values: Vec<f64>,
    _reservation: WorkspaceReservation,
}

impl Workspace {
    pub(super) fn matrices(owner: OwnerToken<'_>, m: usize, n: i32) -> Result<Self, String> {
        let cells = m.checked_mul(m).ok_or_else(memory_error)?;
        let length = cells
            .checked_mul(if n > 1 { 3 } else { 2 })
            .ok_or_else(memory_error)?;
        Self::filled_with(owner, length, |length| {
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
        allocate: impl FnOnce(usize) -> Result<Vec<f64>, String>,
    ) -> Result<Self, String> {
        owner.require_active().map_err(|e| e.to_string())?;
        let bytes = length
            .checked_mul(size_of::<f64>())
            .filter(|&n| n <= isize::MAX as usize)
            .ok_or_else(memory_error)?;
        let mut reservation = owner
            .with_arena(|a| a.reserve_workspace(bytes))
            .map_err(|e| e.to_string())?
            .ok_or_else(memory_error)?;
        let mut values = allocate(length)?;
        let actual = values.capacity().checked_mul(size_of::<f64>());
        if values.capacity() < length || actual.and_then(|n| reservation.try_resize(n)).is_none() {
            drop(values);
            return Err(memory_error());
        }
        owner.require_active().map_err(|e| e.to_string())?;
        values.resize(length, 0.0);
        Ok(Self {
            values,
            _reservation: reservation,
        })
    }

    pub(super) fn split(&mut self, m: usize) -> (&mut [f64], &mut [f64], &mut [f64]) {
        let cells = m * m; // Checked by matrices before allocation.
        let (a, rest) = self.values.split_at_mut(cells);
        let (q, b) = rest.split_at_mut(cells);
        (a, q, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{RSession, memory::ArenaBudget};

    #[test]
    fn dimensions_and_bytes_are_checked_before_allocation() {
        for (n, q) in [
            (0, 0.5),
            (-1, 0.5),
            (10, f64::NAN),
            (10, f64::INFINITY),
            (10, -0.1),
            (10, 1.1),
            (i32::MAX, 1.0),
        ] {
            assert!(dimensions(n, q).is_err());
        }
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        assert!(Workspace::matrices(owner, usize::MAX, 2).is_err());
        assert!(
            Workspace::filled_with(owner, usize::MAX, |_| panic!("unchecked size allocated"))
                .is_err()
        );
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }

    #[test]
    fn denial_precedes_allocation_and_capacity_is_charged() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let values = Workspace::filled_with(owner, 3, |n| {
            let mut v = Vec::new();
            v.try_reserve_exact(n * 2).map_err(|_| memory_error())?;
            Ok(v)
        })
        .unwrap();
        let charge = values.values.capacity() * size_of::<f64>();
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), charge);
        let total = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(total + charge, 0)))
            .unwrap();
        assert!(
            Workspace::filled_with(owner, 1, |_| panic!("allocation before admission")).is_err()
        );
        drop(values);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        let recovered = Workspace::matrices(owner, 1, 1).unwrap();
        assert_eq!(recovered.values, [0.0, 0.0]);
        drop(recovered);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }

    #[test]
    fn surplus_allocator_failure_and_unwind_release_credit() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let baseline = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(baseline + 24, 0)))
            .unwrap();
        assert!(
            Workspace::filled_with(owner, 3, |n| {
                let mut v = Vec::new();
                v.try_reserve_exact(n * 2).map_err(|_| memory_error())?;
                Ok(v)
            })
            .is_err()
        );
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        assert!(Workspace::filled_with(owner, 3, |_| Err(memory_error())).is_err());
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _values = Workspace::matrices(owner, 1, 1).unwrap();
                panic!("fixture unwind");
            }))
            .is_err()
        );
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }

    #[test]
    fn teardown_and_replacement_release_the_original_ledger() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let anchor = owner
            .node_factory()
            .character("anchor")
            .unwrap()
            .into_owned()
            .unwrap();
        let ledger = anchor.allocation().unwrap().heap_identity().byte_ledger();
        let values = Workspace::matrices(owner, 3, 400).unwrap();
        let charge = values.values.capacity() * size_of::<f64>();
        drop(anchor);
        drop(session);
        let mut other = RSession::new_for_gc_tests();
        let before = other.with_arena(|a| a.total_bytes_allocated()).unwrap();
        assert_eq!(ledger.workspace_bytes_for_test(), charge);
        drop(values);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        assert_eq!(
            other.with_arena(|a| a.total_bytes_allocated()).unwrap(),
            before
        );
    }

    #[test]
    fn revocation_during_allocation_returns_original_credit() {
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
                Ok(Workspace::filled_with(owner, 3, |n| {
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
