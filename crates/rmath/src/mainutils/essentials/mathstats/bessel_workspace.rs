//! Private Bessel work array admitted to the original runtime's byte ledger.
#![forbid(unsafe_code)]
use crate::sexp::{
    memory::WorkspaceReservation,
    object::{SexpError, SexpResult},
    owner::RuntimeAccess,
};
pub(super) struct Workspace {
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
    pub(super) fn new(access: &RuntimeAccess, length: usize) -> SexpResult<Self> {
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
    pub(super) fn values(&mut self) -> &mut [f64] {
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
