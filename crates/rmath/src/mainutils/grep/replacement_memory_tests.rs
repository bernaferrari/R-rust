//! Original-owner admission for the actual production replacement return value.
use super::perl_replace;
use crate::sexp::{RSession, memory::ArenaBudget};

#[test]
fn replacement_output_retains_original_workspace_until_final_drop() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let text = "a".repeat(1024);
        let _source = owner.node_factory().character(&text).unwrap();
        let before = owner
            .with_arena(|arena| (arena.total_bytes_allocated(), arena.workspace_bytes()))
            .unwrap();
        let output = perl_replace("(a+)", &text, r"\U\1", false, false).unwrap();
        assert_eq!(output.len(), text.len());
        assert!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap() >= output.len(),
            "returned native text capacity must remain funded"
        );
        drop(output);
        assert_eq!(
            owner
                .with_arena(|arena| (arena.total_bytes_allocated(), arena.workspace_bytes()))
                .unwrap(),
            before
        );
    });
}

#[test]
fn replacement_output_growth_denies_before_allocation_and_recovers_at_same_limit() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let text = "a".repeat(1024);
        let _source = owner.node_factory().character(&text).unwrap();
        let before = owner
            .with_arena(|arena| arena.total_bytes_allocated())
            .unwrap();
        owner
            .with_arena(|arena| arena.set_budget(ArenaBudget::new(before + 128, 0)))
            .unwrap();
        let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            perl_replace("(a+)", &text, r"\U\1", false, false)
        }));
        let error =
            denied.expect_err("native replacement output escaped the original128-byte headroom");
        assert!(
            error
                .downcast_ref::<crate::sexp::context::RError>()
                .is_some()
        );
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
        let recovered = perl_replace("(a)", "a", r"\U\1", false, false);
        assert_eq!(recovered.as_deref(), Some("A"));
        drop(recovered);
        assert_eq!(
            owner
                .with_arena(|arena| (
                    arena.total_bytes_allocated(),
                    arena.workspace_bytes(),
                    arena.budget().max_bytes
                ))
                .unwrap(),
            (before, 0, before + 128)
        );
    });
}

#[test]
fn replacement_output_registered_arena_lend_uses_original_ledger() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let text = "a".repeat(1024);
        let _source = owner.node_factory().character(&text).unwrap();
        owner
            .with_arena(|arena| {
                let before = arena.total_bytes_allocated();
                arena.set_budget(ArenaBudget::new(before + 128, 0));
                let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    perl_replace("(a+)", &text, r"\U\1", false, false)
                }));
                assert!(
                    denied
                        .expect_err("registered lend must observe the current original limit")
                        .downcast_ref::<crate::sexp::context::RError>()
                        .is_some()
                );
                assert_eq!(arena.workspace_bytes(), 0);
                let output = perl_replace("(a)", "a", r"\U\1", false, false);
                assert_eq!(output.as_deref(), Some("A"));
                drop(output);
                assert_eq!(
                    (arena.workspace_bytes(), arena.budget().max_bytes),
                    (0, before + 128)
                );
            })
            .unwrap();
    });
}

#[test]
fn replacement_output_original_owner_teardown_keeps_only_native_data_charge() {
    let session = RSession::new_for_gc_tests();
    let (output, ledger) = session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let output = perl_replace("(a+)", &"a".repeat(1024), r"\U\1", false, false).unwrap();
        (output, (*owner.as_ptr()).heap_identity.byte_ledger())
    });
    drop(session);
    assert_eq!(ledger.allocated_bytes(), 0);
    assert!(
        ledger.workspace_bytes_for_test() >= output.len(),
        "native output must retain its original ticket after runtime teardown"
    );
    let other = RSession::new_for_gc_tests();
    other.with_active(|| {
        let owner = other.owner_token().unwrap();
        let before = owner
            .with_arena(|arena| (arena.total_bytes_allocated(), arena.workspace_bytes()))
            .unwrap();
        assert_eq!(output.len(), 1024);
        drop(output);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        assert_eq!(
            owner
                .with_arena(|arena| (arena.total_bytes_allocated(), arena.workspace_bytes()))
                .unwrap(),
            before
        );
    });
}

#[test]
fn replacement_output_short_original_limit_control() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let before = owner
            .with_arena(|arena| arena.total_bytes_allocated())
            .unwrap();
        owner
            .with_arena(|arena| arena.set_budget(ArenaBudget::new(before + 128, 0)))
            .unwrap();
        let output = perl_replace("(a)", "a", r"\U\1", false, false);
        assert_eq!(output.as_deref(), Some("A"));
        drop(output);
        assert_eq!(
            owner
                .with_arena(|arena| (arena.workspace_bytes(), arena.budget().max_bytes))
                .unwrap(),
            (0, before + 128)
        );
    });
}
