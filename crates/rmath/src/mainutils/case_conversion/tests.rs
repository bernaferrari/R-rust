use super::*;
use crate::sexp::{RSession, memory::ArenaBudget};

#[test]
fn output_denies_then_recovers_at_the_same_exact_original_limit() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let input = factory.strings(&["éa", "ß"]).unwrap().into_owned().unwrap();
        let large = Source::read(&input.try_string_elt(0).unwrap()).unwrap();
        let small = Source::read(&input.try_string_elt(1).unwrap()).unwrap();
        let resident = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(resident + 2, 0)))
            .unwrap();
        assert!(convert(&large, &input, true, true, true, 0).is_err());
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        let output = convert(&small, &input, true, true, true, 1).unwrap();
        assert_eq!(output.bytes, "ß".as_bytes());
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 2);
        drop(output);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        assert_eq!(
            owner.with_arena(|a| a.budget().max_bytes).unwrap(),
            resident + 2
        );
    });
}

#[test]
fn output_storage_keeps_original_admission_after_teardown_with_other_runtime_active() {
    let original = RSession::new_for_gc_tests();
    let (output, ledger) = original.with_active(|| {
        let owner = original.owner_token().unwrap();
        let input = owner
            .node_factory()
            .strings(&["éa"])
            .unwrap()
            .into_owned()
            .unwrap();
        let source = Source::read(&input.try_string_elt(0).unwrap()).unwrap();
        let ledger = input.allocation().unwrap().heap_identity().byte_ledger();
        (
            convert(&source, &input, true, true, true, 0).unwrap(),
            ledger,
        )
    });
    drop(original);
    let other = RSession::new_for_gc_tests();
    other.with_active(|| {
        let owner = other.owner_token().unwrap();
        let before = owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap();
        assert_eq!(output.bytes, "ÉA".as_bytes());
        assert_eq!(ledger.workspace_bytes_for_test(), output.bytes.capacity());
        drop(output);
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
fn empty_output_needs_no_admission_under_an_already_lowered_limit() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let input = owner
            .node_factory()
            .strings(&[""])
            .unwrap()
            .into_owned()
            .unwrap();
        let source = Source::read(&input.try_string_elt(0).unwrap()).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(1, 0)))
            .unwrap();
        let output = convert(&source, &input, true, true, true, 0).unwrap();
        assert_eq!(output.bytes.capacity(), 0);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), 1);
    });
}

#[test]
fn foreign_input_cannot_use_the_current_runtime_allocator() {
    use crate::sexp::owner::with_runtime;
    let original = RSession::new_for_gc_tests();
    let input = original.with_active(|| {
        original
            .owner_token()
            .unwrap()
            .node_factory()
            .strings(&["abc"])
            .unwrap()
            .into_owned()
            .unwrap()
    });
    let other = RSession::new_for_gc_tests();
    other.with_active(|| {
        let owner = other.owner_token().unwrap();
        let before = owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap();
        let weak = owner.weak_owner().unwrap();
        let address = input.as_raw().addr();
        let result = with_runtime(&weak, |access| {
            transform(access, input, access.domain().nil(), true)
        })
        .unwrap();
        assert!(matches!(result, Err(SexpError::UnownedPointer { address: rejected }) if rejected == address));
        assert_eq!(
            owner
                .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
                .unwrap(),
            before
        );
    });
}
