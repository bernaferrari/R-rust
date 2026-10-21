use super::*;
use crate::sexp::{RSession, memory::ArenaBudget, object::SessionNodeFactory, owner::with_runtime};

fn raw(f: &SessionNodeFactory<'_>, bytes: &[u8]) -> Sexp<'static> {
    let value = f
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::RAWSXP, bytes.len() as i64)))
        .unwrap();
    let mut value = SexpMut::try_from_checked(value).unwrap();
    for (i, &byte) in bytes.iter().enumerate() {
        value.try_set_raw_elt(i as i64, byte).unwrap();
    }
    value.freeze().into_owned().unwrap()
}

#[test]
fn raw_to_char_copy_denies_then_recovers_at_the_same_original_limit() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let f = owner.node_factory();
        let large = raw(&f, b"ABCD");
        let small = raw(&f, b"xy");
        let resident = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(resident + 2, 0)))
            .unwrap();
        with_runtime(&owner.weak_owner().unwrap(), |access| {
            assert!(copy(&large, 4, access).is_err());
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
            let bytes = copy(&small, 2, access).unwrap();
            assert_eq!(bytes.data, b"xy");
            assert_eq!(
                owner.with_arena(|a| a.workspace_bytes()).unwrap(),
                bytes.data.capacity()
            );
            drop(bytes);
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
            assert_eq!(
                owner.with_arena(|a| a.budget().max_bytes).unwrap(),
                resident + 2
            );
        })
        .unwrap();
    });
}

#[test]
fn raw_to_char_copy_retains_original_charge_after_teardown_with_other_active() {
    let original = RSession::new_for_gc_tests();
    let (bytes, ledger) = original.with_active(|| {
        let owner = original.owner_token().unwrap();
        let input = raw(&owner.node_factory(), b"ABC");
        let ledger = input.allocation().unwrap().heap_identity().byte_ledger();
        let bytes = with_runtime(&owner.weak_owner().unwrap(), |access| {
            copy(&input, 3, access)
        })
        .unwrap()
        .unwrap();
        (bytes, ledger)
    });
    drop(original);
    let other = RSession::new_for_gc_tests();
    other.with_active(|| {
        let owner = other.owner_token().unwrap();
        let before = owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap();
        assert_eq!(bytes.data, b"ABC");
        assert_eq!(ledger.workspace_bytes_for_test(), bytes.data.capacity());
        drop(bytes);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        assert_eq!(
            owner
                .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
                .unwrap(),
            before
        );
    });
}

#[test]
fn raw_to_char_empty_copy_succeeds_under_already_lowered_one_byte_limit() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let input = raw(&owner.node_factory(), b"");
        assert!(owner.with_arena(|a| a.total_bytes_allocated()).unwrap() > 1);
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(1, 0)))
            .unwrap();
        with_runtime(&owner.weak_owner().unwrap(), |access| {
            let bytes = copy(&input, 0, access).unwrap();
            assert_eq!(bytes.data.capacity(), 0);
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
            assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), 1);
        })
        .unwrap();
    });
}

#[test]
fn raw_to_char_foreign_input_cannot_use_current_allocator_or_coercion() {
    use std::cell::Cell;
    let original = RSession::new_for_gc_tests();
    let (args, address) = original.with_active(|| {
        let f = original.owner_token().unwrap().node_factory();
        let input = raw(&f, b"ABC");
        let address = input.as_raw().addr();
        (
            f.pairlist_cell(&input, &f.nil(), &f.nil())
                .unwrap()
                .into_owned()
                .unwrap(),
            address,
        )
    });
    let other = RSession::new_for_gc_tests();
    other.with_active(|| {
        let owner = other.owner_token().unwrap();
        let before = owner.with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes())).unwrap();
        let called = Cell::new(false);
        let result = with_runtime(&owner.weak_owner().unwrap(), |access| evaluate(access, args, |_| { called.set(true); Ok(0) })).unwrap();
        assert!(matches!(result, Err(EvaluationError::Access(SexpError::UnownedPointer { address: rejected })) if rejected == address));
        assert!(!called.get());
        assert_eq!(owner.with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes())).unwrap(), before);
    });
}

#[test]
fn raw_to_char_returned_error_retains_original_message_admission() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let f = owner.node_factory();
        let input = raw(&f, b"A\0B");
        let arguments = f
            .pairlist_cell(&input, &f.nil(), &f.nil())
            .unwrap()
            .into_owned()
            .unwrap();
        let resident = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        with_runtime(&owner.weak_owner().unwrap(), |access| {
            let error = evaluate(access, arguments, |flag| flag.try_logical_elt(0)).unwrap_err();
            let EvaluationError::Diagnostic(message) = error else {
                panic!("embedded-NUL diagnostic")
            };
            assert_eq!(message.to_string(), "embedded nul in string: 'A\\0B'");
            assert!(
                owner.with_arena(|a| a.total_bytes_allocated()).unwrap()
                    >= resident + message.len(),
                "returned R error still owns its message storage"
            );
        })
        .unwrap();
    });
}
