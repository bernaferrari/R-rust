//! Private Bessel work array admitted to the original runtime's byte ledger.
#![forbid(unsafe_code)]
use crate::sexp::{
    memory::WorkspaceReservation,
    object::{SexpError, SexpResult},
    owner::RuntimeAccess,
};
pub(crate) struct Workspace {
    // Storage must disappear before its original-owner credit is released.
    values: Vec<f64>,
    _reservation: WorkspaceReservation,
}
fn failure() -> SexpError {
    SexpError::EvaluationFailed {
        message: "Bessel workspace exceeds session allocation limit".into(),
    }
}
impl Workspace {
    pub(crate) fn new(access: &RuntimeAccess, length: usize) -> SexpResult<Self> {
        let bytes = length
            .checked_mul(size_of::<f64>())
            .filter(|&n| n <= isize::MAX as usize)
            .ok_or_else(failure)?;
        let mut reservation = access
            .with_arena(|a| a.reserve_workspace(bytes))?
            .ok_or_else(failure)?;
        let mut values = Vec::new();
        values.try_reserve_exact(length).map_err(|_| failure())?;
        if values.capacity() < length
            || values
                .capacity()
                .checked_mul(size_of::<f64>())
                .and_then(|n| reservation.try_resize(n))
                .is_none()
        {
            drop(values);
            return Err(failure());
        }
        access.require_active()?;
        values.resize(length, 0.0);
        Ok(Self {
            values,
            _reservation: reservation,
        })
    }
    /// Grow without losing earlier elements. Both buffers remain admitted while
    /// they overlap, and newly created elements are initialized to zero.
    pub(crate) fn ensure_len(&mut self, access: &RuntimeAccess, length: usize) -> SexpResult<()> {
        if length > self.values.len() {
            let mut replacement = Self::new(access, length)?;
            replacement.values[..self.values.len()].copy_from_slice(&self.values);
            *self = replacement;
        }
        Ok(())
    }
    pub(crate) fn values(&mut self) -> &mut [f64] {
        &mut self.values
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{RSession, memory::ArenaBudget, owner::with_runtime};
    #[test]
    fn bessel_workspace_admission_denial_overflow_and_recovery() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let weak = owner.weak_owner().unwrap();
        session.with_active(|| {
            with_runtime(&weak, |access| {
                let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
                let before_workspace = owner.with_arena(|a| a.workspace_bytes()).unwrap();
                assert!(Workspace::new(access, usize::MAX).is_err());
                owner
                    .with_arena(|a| {
                        a.set_budget(ArenaBudget::new(before + before_workspace + 7, 0))
                    })
                    .unwrap();
                assert!(Workspace::new(access, 1).is_err());
                assert_eq!(
                    owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
                    before
                );
                owner
                    .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
                    .unwrap();
                let mut values = Workspace::new(access, 3).unwrap();
                assert_eq!(values.values(), [0.0; 3]);
                values.values().copy_from_slice(&[1.0, 2.0, 3.0]);
                let allocated = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
                let charged = owner.with_arena(|a| a.workspace_bytes()).unwrap();
                owner
                    .with_arena(|a| a.set_budget(ArenaBudget::new(allocated + charged + 39, 0)))
                    .unwrap();
                assert!(values.ensure_len(access, 5).is_err());
                assert_eq!(values.values(), [1.0, 2.0, 3.0]);
                assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), charged);
                owner
                    .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
                    .unwrap();
                values.ensure_len(access, 5).unwrap();
                assert_eq!(values.values(), [1.0, 2.0, 3.0, 0.0, 0.0]);
                assert!(
                    owner.with_arena(|a| a.workspace_bytes()).unwrap() >= before_workspace + 24
                );
                drop(values);
                assert_eq!(
                    owner.with_arena(|a| a.workspace_bytes()).unwrap(),
                    before_workspace
                );
                assert_eq!(
                    owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
                    before
                );
            })
            .unwrap()
        });
    }
}
