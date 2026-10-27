use super::*;
use crate::sexp::{memory::ArenaBudget, object::Sexp, session::RSession};

#[test]
fn serialization_workspace_resize_reclaims_hints_without_releasing_live_entries_or_tickets() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let character = owner.node_factory().character("resize literal").unwrap();
        let raw = owner
            .with_arena(|a| a.immutable_bytecode_string(character.as_raw()))
            .unwrap();
        let value = owner.sexp(raw).unwrap().into_owned().unwrap();
        let mut values = WorkspaceVec::<Sexp<'static>>::new(&owner).unwrap();
        for _ in 0..8 {
            values.push(value.clone()).unwrap();
        }
        let held = reserve(&owner, 128).unwrap();
        let charged = owner
            .with_arena(|a| {
                let charged = a.workspace_bytes();
                a.set_budget(ArenaBudget::new(a.total_bytes_allocated() + charged, 0));
                charged
            })
            .unwrap();
        // A nested arena loan cannot be used as an eviction hook.
        assert!(
            owner
                .with_arena(|_| values.reserve_len(9))
                .unwrap()
                .is_err()
        );
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), charged);
        assert_eq!(values.len(), 8);
        values.push(value.clone()).unwrap();
        assert_eq!(values.len(), 9);
        let width = std::mem::size_of::<Sexp<'static>>();
        assert_eq!(
            owner.with_arena(|a| a.workspace_bytes()).unwrap(),
            128 + 16 * width
        );
        owner.full_gc().unwrap();
        for entry in values.iter() {
            assert_eq!(entry.as_raw(), value.as_raw());
            assert_eq!(
                entry.try_string_elt(0).unwrap().as_raw(),
                character.as_raw()
            );
        }
        // Genuine denial retains both old entries and their original charge.
        owner
            .with_arena(|a| {
                a.set_budget(ArenaBudget::new(
                    a.total_bytes_allocated() + a.workspace_bytes(),
                    0,
                ))
            })
            .unwrap();
        assert!(values.reserve_len(17).is_err());
        assert_eq!(values.len(), 9);
        assert_eq!(
            owner.with_arena(|a| a.workspace_bytes()).unwrap(),
            128 + 16 * width
        );
        drop(values);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 128);
        drop(held);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    });
}

#[test]
fn serialization_workspace_resize_rejects_revoked_original_and_preserves_replacement() {
    let original = RSession::new_for_gc_tests();
    let mut values = original.with_active(|| {
        let owner = original.owner_token().unwrap();
        let mut values = WorkspaceVec::<u64>::new(&owner).unwrap();
        values.push(71).unwrap();
        values
    });
    drop(original);
    let replacement = RSession::new_for_gc_tests();
    replacement.with_active(|| {
        let owner = replacement.owner_token().unwrap();
        let character = owner.node_factory().character("replacement cache").unwrap();
        owner
            .with_arena(|a| a.immutable_bytecode_string(character.as_raw()))
            .unwrap();
        let before = owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap();
        assert!(values.reserve_len(9).is_err());
        assert!(
            values.push(72).is_err(),
            "even unused capacity requires original authority"
        );
        assert_eq!(&*values, &[71]);
        drop(values);
        assert_eq!(
            owner
                .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
                .unwrap(),
            before
        );
    });
}
