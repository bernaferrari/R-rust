use super::*;
use crate::sexp::{context::RError, memory::ArenaBudget, session::RSession};
use std::panic::{AssertUnwindSafe, catch_unwind};

const PATTERN: &str = r"(*UCP)(?x:^(?<name>\w+)[^K\w]$)";

#[test]
fn owned_perl_dialect_buffer_retains_exact_original_charge_through_consumer() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let workspace = super::super::regex_workspace();
    let text = prepare(PATTERN, &workspace, false).unwrap();
    assert_eq!(
        owner.with_arena(|a| a.workspace_bytes()).unwrap(),
        text.capacity()
    );
    assert!(text.contains("name"));
    assert!(text.contains(r"\x{88f}"));
    drop(text);
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
}

#[test]
fn owned_perl_dialect_second_pass_stack_cannot_bypass_original_peak_limit() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let workspace = super::super::regex_workspace();
    let text = prepare(PATTERN, &workspace, false).unwrap();
    let bytes = text.capacity();
    drop(text);
    let used = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(used + bytes, 0)))
        .unwrap();
    let result = catch_unwind(AssertUnwindSafe(|| prepare(PATTERN, &workspace, false)));
    let error = result.unwrap_err().downcast::<RError>().unwrap();
    assert_eq!(
        error.message,
        "regex workspace exceeds session memory limit"
    );
    owner
        .with_arena(|a| {
            assert_eq!(a.budget().max_bytes, used + bytes);
            assert_eq!(a.workspace_bytes(), 0);
        })
        .unwrap();
}

#[test]
fn owned_perl_dialect_cap_one_and_syntax_failure_release_every_temporary() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let workspace = super::super::regex_workspace();
    assert!(prepare(r"é(?x:(?<name>\Q[\E))[", &workspace, false).is_err());
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(1, 0)))
        .unwrap();
    assert!(catch_unwind(AssertUnwindSafe(|| prepare(PATTERN, &workspace, false))).is_err());
    owner
        .with_arena(|a| {
            assert_eq!(a.budget().max_bytes, 1);
            assert_eq!(a.workspace_bytes(), 0);
        })
        .unwrap();
}

#[test]
fn owned_perl_dialect_uses_original_ledger_with_foreign_ambient_and_rejects_revocation() {
    let first = RSession::new_for_gc_tests();
    let witness = first
        .owner_token()
        .unwrap()
        .node_factory()
        .strings(&["ledger"])
        .unwrap();
    let ledger = witness.allocation().unwrap().heap_identity().byte_ledger();
    let workspace = super::super::regex_workspace();
    let second = RSession::new_for_gc_tests();
    let before = second
        .owner_token()
        .unwrap()
        .with_arena(|a| a.total_bytes_allocated() + a.workspace_bytes())
        .unwrap();
    let text = prepare(PATTERN, &workspace, false).unwrap();
    assert_eq!(ledger.workspace_bytes_for_test(), text.capacity());
    assert_eq!(
        second
            .owner_token()
            .unwrap()
            .with_arena(|a| a.total_bytes_allocated() + a.workspace_bytes())
            .unwrap(),
        before
    );
    drop(witness);
    drop(first);
    assert!(text.contains("name"));
    assert!(catch_unwind(AssertUnwindSafe(|| prepare(PATTERN, &workspace, false))).is_err());
    drop(text);
    assert_eq!(ledger.workspace_bytes_for_test(), 0);
    assert_eq!(
        second
            .owner_token()
            .unwrap()
            .with_arena(|a| a.total_bytes_allocated() + a.workspace_bytes())
            .unwrap(),
        before
    );
}

#[test]
fn owned_perl_dialect_small_malformed_unicode_patterns_never_panic_or_retain_charge() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let workspace = super::super::regex_workspace();
    let alphabet = [
        '[', ']', '(', ')', '\\', 'Q', 'E', 'w', 'p', '{', '}', '$', '#', 'x', ' ', '\t', '^', '-',
        'é', 'Ω',
    ];
    for source in [r"\p", r"\P", r"[\p", r"[\P", r"\p{", r"(?xx)["] {
        let result = catch_unwind(AssertUnwindSafe(|| prepare(source, &workspace, false)));
        assert!(result.is_ok(), "preparation panic for {source:?}");
        drop(result);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }
    let mut unique = std::collections::HashSet::new();
    for seed in 0..512u64 {
        let mut state = seed ^ 0x9e3779b97f4a7c15;
        let mut source = String::new();
        for _ in 0..12 {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            let random = state.wrapping_mul(2685821657736338717);
            source.push(alphabet[((random >> 32) as usize) % alphabet.len()]);
        }
        unique.insert(source.clone());
        let result = catch_unwind(AssertUnwindSafe(|| prepare(&source, &workspace, false)));
        assert!(result.is_ok(), "preparation panic for {source:?}");
        drop(result);
        assert_eq!(
            owner.with_arena(|a| a.workspace_bytes()).unwrap(),
            0,
            "{source:?}"
        );
    }
    assert!(
        unique.len() > 500,
        "malformed corpus must exercise distinct inputs"
    );
}
