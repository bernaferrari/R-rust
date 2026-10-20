//! Owning reference tables with original-session native admission.
#![forbid(unsafe_code)]
use super::super::workspace::{WorkspaceVec, memory_error};
use super::*;
use crate::sexp::{
    Sexp,
    owner::{OwnerToken, StoredOwner},
};

struct WriteReference {
    value: Sexp<'static>,
    previous: usize,
}

pub struct WriteHashTable {
    heads: WorkspaceVec<usize>,
    entries: WorkspaceVec<WriteReference>,
    owner: StoredOwner<'static>,
    pub count: i32,
}

impl WriteHashTable {
    pub(crate) fn for_owner(owner: &OwnerToken<'_>) -> Self {
        Self {
            heads: WorkspaceVec::new(owner).unwrap_or_else(|e| crate::sexp::context::r_error(e)),
            entries: WorkspaceVec::new(owner).unwrap_or_else(|e| crate::sexp::context::r_error(e)),
            owner: StoredOwner::from_token(*owner)
                .into_owned()
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string())),
            count: 0,
        }
    }

    pub fn add(&mut self, object: SEXP) {
        self.try_add(object)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e))
    }

    fn try_add(&mut self, object: SEXP) -> Result<(), String> {
        self.owner.require_active().map_err(|e| e.to_string())?;
        let value = self
            .owner
            .sexp(object)
            .and_then(Sexp::into_owned)
            .map_err(|e| e.to_string())?;
        let count = self.count.checked_add(1).ok_or_else(memory_error)?;
        if self.heads.is_empty() {
            self.heads.resize(HASHSIZE as usize, usize::MAX)?;
        }
        let bucket = (object as usize >> 2) % self.heads.len();
        let index = self.entries.len();
        self.entries.push(WriteReference {
            value,
            previous: self.heads[bucket],
        })?;
        self.heads[bucket] = index;
        self.count = count;
        Ok(())
    }

    pub fn get(&self, object: SEXP) -> i32 {
        self.owner
            .require_active()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        if self.heads.is_empty() {
            return 0;
        }
        let mut index = self.heads[(object as usize >> 2) % self.heads.len()];
        let mut found = 0;
        while let Some(entry) = self.entries.get(index) {
            if entry.value.as_raw() == object {
                found = index as i32 + 1;
            }
            index = entry.previous;
        }
        found
    }
}

pub struct ReadRefTable {
    entries: WorkspaceVec<Sexp<'static>>,
    owner: StoredOwner<'static>,
}

impl ReadRefTable {
    pub(super) fn reserve(
        &self,
        bytes: usize,
    ) -> Result<crate::sexp::memory::WorkspaceReservation, String> {
        self.owner
            .with_arena(|a| a.reserve_workspace(bytes))
            .map_err(|e| e.to_string())?
            .ok_or_else(memory_error)
    }
    pub(crate) fn for_owner(owner: &OwnerToken<'_>) -> Self {
        Self {
            entries: WorkspaceVec::new(owner).unwrap_or_else(|e| crate::sexp::context::r_error(e)),
            owner: StoredOwner::from_token(*owner)
                .into_owned()
                .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string())),
        }
    }

    pub fn add(&mut self, value: SEXP) {
        self.owner
            .require_active()
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        let value = self
            .owner
            .sexp(value)
            .and_then(Sexp::into_owned)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
        self.entries
            .push(value)
            .unwrap_or_else(|e| crate::sexp::context::r_error(e));
    }

    pub fn get(&self, index: i32) -> Result<SEXP, String> {
        self.owner.require_active().map_err(|e| e.to_string())?;
        let index = index
            .checked_sub(1)
            .and_then(|i| usize::try_from(i).ok())
            .ok_or_else(|| "reference index out of range".to_string())?;
        self.entries
            .get(index)
            .map(Sexp::as_raw)
            .ok_or_else(|| "reference index out of range".into())
    }
}
#[cfg(test)]
mod workspace_admission_tests {
    use super::*;
    use crate::sexp::{RSession, memory::ArenaBudget};

    #[test]
    fn serialization_workspace_decoded_string_retains_original_admission() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let mut reader = BinaryReader::new(b"abc");
        let bytes = reader
            .read_string_bytes(
                3,
                super::super::super::workspace::reserve(&owner, 0).unwrap(),
            )
            .unwrap();
        assert!(
            owner.with_arena(|a| a.workspace_bytes()).unwrap() >= 3,
            "decoded native string must remain charged until the R copy is published"
        );
        drop(bytes);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }

    #[test]
    fn serialization_workspace_write_references_deny_before_native_growth() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let value = owner.node_factory().strings(&["reference-root"]).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(a.total_bytes_allocated() + 1, 0)))
            .unwrap();
        let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut refs = WriteHashTable::new();
            refs.add(value.as_raw());
        }));
        assert!(
            denied.is_err(),
            "write reference storage must obey the original byte cap"
        );
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }

    #[test]
    fn serialization_workspace_read_references_deny_before_native_growth() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let value = owner.node_factory().strings(&["reference-root"]).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(a.total_bytes_allocated() + 1, 0)))
            .unwrap();
        let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut refs = ReadRefTable::new();
            refs.add(value.as_raw());
        }));
        assert!(
            denied.is_err(),
            "read reference roots must obey the original byte cap"
        );
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }

    #[test]
    fn serialization_workspace_pull_denies_before_consuming_source() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(a.total_bytes_allocated() + 1, 0)))
            .unwrap();
        let mut consumed = 0;
        {
            let mut fill = |buf: &mut super::super::super::owned_input::InputBytes, need: usize| {
                while buf.len() < need {
                    buf.push_from(|| {
                        consumed += 1;
                        Some(0)
                    })
                    .unwrap();
                }
                true
            };
            let mut reader = BinaryReader::new_pull(&owner, &mut fill);
            let denied =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| reader.read_i32()));
            assert!(
                denied.is_err() || denied.unwrap().is_err(),
                "pull buffering must admit before reading"
            );
        }
        assert_eq!(consumed, 0, "denied input must remain unread");
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }

    #[test]
    fn serialization_workspace_reference_growth_failure_retains_entries_and_recovers() {
        for write in [false, true] {
            let session = RSession::new_for_gc_tests();
            let owner = session.owner_token().unwrap();
            let value = owner.node_factory().strings(&["retained"]).unwrap();
            let mut reads = ReadRefTable::new();
            let mut writes = WriteHashTable::new();
            if write {
                writes.add(value.as_raw());
            } else {
                reads.add(value.as_raw());
            }
            let charged = owner.with_arena(|a| a.workspace_bytes()).unwrap();
            assert!(charged > 0);
            let limit = owner
                .with_arena(|a| a.total_bytes_allocated() + charged)
                .unwrap();
            owner
                .with_arena(|a| a.set_budget(ArenaBudget::new(limit, 0)))
                .unwrap();
            let mut denied = false;
            for _ in 0..128 {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if write {
                        writes.add(value.as_raw());
                    } else {
                        reads.add(value.as_raw());
                    }
                }));
                if result.is_err() {
                    denied = true;
                    break;
                }
            }
            assert!(denied, "reference roots cannot escape the byte cap");
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), charged);
            assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), limit);
            if write {
                assert_eq!(writes.get(value.as_raw()), 1);
            } else {
                assert_eq!(reads.get(1).unwrap(), value.as_raw());
            }
            drop((reads, writes));
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
            // The released reservation admits a new operation under the same cap.
            if write {
                WriteHashTable::new().add(value.as_raw());
            } else {
                ReadRefTable::new().add(value.as_raw());
            }
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        }
    }

    #[test]
    fn serialization_workspace_owning_tables_retain_gc_roots_until_drop() {
        for write in [false, true] {
            let session = RSession::new_for_gc_tests();
            let owner = session.owner_token().unwrap();
            let value = owner.node_factory().strings(&["gc-root"]).unwrap();
            let allocation = value.allocation().unwrap().clone();
            let pointer = value.as_raw();
            let mut reads = ReadRefTable::new();
            let mut writes = WriteHashTable::new();
            if write {
                writes.add(pointer);
            } else {
                reads.add(pointer);
            }
            drop(value);
            owner.full_gc().unwrap();
            assert!(allocation.is_live());
            if write {
                assert_eq!(writes.get(pointer), 1);
            } else {
                assert_eq!(reads.get(1).unwrap(), pointer);
            }
            drop((reads, writes));
            owner.full_gc().unwrap();
            assert!(!allocation.is_live());
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        }
    }

    #[test]
    fn serialization_workspace_tables_refuse_revoked_original_with_replacement_active() {
        let mut original = RSession::new_for_gc_tests();
        let owner = original.owner_token().unwrap();
        let value = owner
            .node_factory()
            .strings(&["original"])
            .unwrap()
            .into_owned()
            .unwrap();
        let mut reads = ReadRefTable::new();
        let mut writes = WriteHashTable::new();
        reads.add(value.as_raw());
        writes.add(value.as_raw());
        original.close();
        drop(original);
        let replacement = RSession::new_for_gc_tests();
        let owner = replacement.owner_token().unwrap();
        let before = owner.with_arena(|a| a.workspace_bytes()).unwrap();
        assert_eq!(
            reads.get(1).unwrap_err(),
            crate::sexp::object::SexpError::RootUnavailable.to_string()
        );
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| writes.get(value.as_raw())))
                .is_err()
        );
        drop((reads, writes, value));
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), before);
    }
}
