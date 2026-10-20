//! Original-session admission for native class-name decoding.
#![forbid(unsafe_code)]
use crate::sexp::{memory::WorkspaceReservation, owner::OwnerToken};

pub(crate) struct AdmittedClassName {
    text: String,
    _reservation: WorkspaceReservation,
}
impl AdmittedClassName {
    pub(crate) fn from_bytes(owner: OwnerToken<'_>, bytes: &[u8]) -> Result<Self, String> {
        let failure = || "class name workspace exceeds session memory limit".to_string();
        // Determine the exact lossy UTF-8 length before copying, including
        // replacement characters. No temporary CString or decoded copy.
        let length = bytes
            .utf8_chunks()
            .try_fold(0usize, |length, chunk| {
                length
                    .checked_add(chunk.valid().len())?
                    .checked_add(if chunk.invalid().is_empty() { 0 } else { 3 })
            })
            .ok_or_else(failure)?;
        let reservation = owner
            .with_arena(|arena| arena.reserve_workspace(length))
            .map_err(|e| e.to_string())?
            .ok_or_else(failure)?;
        let mut text = String::new();
        text.try_reserve_exact(length).map_err(|_| failure())?;
        for chunk in bytes.utf8_chunks() {
            text.push_str(chunk.valid());
            if !chunk.invalid().is_empty() {
                text.push('\u{fffd}');
            }
        }
        owner.require_active().map_err(|e| e.to_string())?;
        Ok(Self {
            text,
            _reservation: reservation,
        })
    }
    pub(crate) fn as_str(&self) -> &str {
        &self.text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{RSession, memory::ArenaBudget};

    #[test]
    fn admitted_class_name_exact_lossy_length_denial_release_and_recovery() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let baseline = owner.with_arena(|a| a.workspace_bytes()).unwrap();
        for bytes in [&b"class"[..], &b"\xffa\xf0\x80b"[..], &b""[..]] {
            let name = AdmittedClassName::from_bytes(owner, bytes).unwrap();
            assert_eq!(name.as_str(), String::from_utf8_lossy(bytes));
            assert_eq!(
                owner.with_arena(|a| a.workspace_bytes()).unwrap(),
                baseline + name.as_str().len()
            );
            drop(name);
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), baseline);
        }
        let budget = owner
            .with_arena(|a| {
                let old = a.budget();
                a.set_budget(ArenaBudget::new(
                    a.total_bytes_allocated() + baseline + 1,
                    0,
                ));
                old
            })
            .unwrap();
        assert_eq!(
            AdmittedClassName::from_bytes(owner, b"class")
                .err()
                .unwrap(),
            "class name workspace exceeds session memory limit"
        );
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), baseline);
        owner.with_arena(|a| a.set_budget(budget)).unwrap();
        assert_eq!(
            AdmittedClassName::from_bytes(owner, b"class")
                .unwrap()
                .as_str(),
            "class"
        );
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), baseline);
    }

    #[test]
    fn admitted_class_name_keeps_original_ledger_without_owning_runtime() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let weak = owner.weak_owner().unwrap();
        let baseline = owner.with_arena(|a| a.workspace_bytes()).unwrap();
        let name = AdmittedClassName::from_bytes(owner, b"original").unwrap();
        assert_eq!(weak.allocation_strong_count(), 1);
        let other = RSession::new_for_gc_tests();
        other.with_active(|| {
            let other_owner = other.owner_token().unwrap();
            let before = other_owner.with_arena(|a| a.workspace_bytes()).unwrap();
            drop(name);
            assert_eq!(
                other_owner.with_arena(|a| a.workspace_bytes()).unwrap(),
                before
            );
        });
        session.with_active(|| {
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), baseline)
        });
        let name =
            session.with_active(|| AdmittedClassName::from_bytes(owner, b"detached").unwrap());
        drop(session);
        assert_eq!(weak.allocation_strong_count(), 0);
        assert_eq!(name.as_str(), "detached");
        drop(name);
    }
}
