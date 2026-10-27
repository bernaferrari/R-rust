use super::*;
use crate::sexp::{object::SexpMut, session::RSession};

#[test]
fn bytecode_string_hints_share_readonly_cells_within_the_original_owner_only() {
    let left = RSession::new_for_gc_tests();
    let right = RSession::new_for_gc_tests();
    let make = |session: &RSession| {
        session.with_active(|| {
            let owner = session.owner_token().unwrap();
            let character = owner.node_factory().character("private literal").unwrap();
            let (first, second) = owner
                .with_arena(|arena| {
                    (
                        arena.immutable_bytecode_string(character.as_raw()),
                        arena.immutable_bytecode_string(character.as_raw()),
                    )
                })
                .unwrap();
            assert_eq!(first, second);
            unsafe {
                crate::sexp::accessors::SET_NAMED(first, 0);
            }
            assert_eq!(unsafe { crate::sexp::accessors::NAMED(first) }, 3);
            let value = owner.sexp(first).unwrap();
            let mut mutable = SexpMut::try_from_checked(value.clone()).unwrap();
            assert!(
                mutable.try_set_string_elt(0, character).is_err(),
                "code cells must remain readonly"
            );
            value.into_owned().unwrap()
        })
    };
    let first = make(&left);
    let second = make(&right);
    assert_ne!(first.as_raw(), second.as_raw());
}

#[test]
fn bytecode_string_hints_never_root_collected_values_or_reuse_retired_generations() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let old = owner
            .with_arena(|arena| {
                let character = arena.alloc_charsxp(b"collected literal");
                let value = arena.immutable_bytecode_string(character);
                arena.node_token(value).unwrap()
            })
            .unwrap();
        owner.full_gc().unwrap();
        assert!(!old.is_live());
        owner
            .with_arena(|arena| {
                let character = arena.alloc_charsxp(b"collected literal");
                let value = arena.immutable_bytecode_string(character);
                let fresh = arena.node_token(value).unwrap();
                assert!(fresh.is_live());
                assert_ne!(fresh.id(), old.id());
                assert_eq!(arena.immutable_bytecode_string(character), value);
            })
            .unwrap();
    });
}

#[test]
fn bytecode_string_cache_admission_is_optional_and_uses_the_original_session_limit() {
    let mut arena = RArena::with_budget(ArenaBudget::new(8192, 4));
    let character = arena.alloc_charsxp(b"literal");
    assert!(!arena.immutable_bytecode_string(character).is_null());
    assert_eq!(
        arena.workspace_bytes(),
        0,
        "an oversized cache escaped admission"
    );
    assert_eq!(arena.budget().max_bytes, 8192);
}

#[test]
fn bytecode_string_workspace_denial_evicts_only_weak_cache_and_can_readmit() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let character = owner
            .node_factory()
            .character("live cached literal")
            .unwrap();
        let raw = owner
            .with_arena(|arena| arena.immutable_bytecode_string(character.as_raw()))
            .unwrap();
        let live = owner.sexp(raw).unwrap().into_owned().unwrap();
        let (baseline, cache, budget) = owner
            .with_arena(|arena| {
                let cache = arena.workspace_bytes();
                assert_eq!(cache, 327680, "locked NodeLink cache layout changed");
                (arena.total_bytes_allocated(), cache, arena.budget())
            })
            .unwrap();
        let held = owner
            .with_arena(|arena| arena.reserve_workspace(128).unwrap())
            .unwrap();
        owner
            .with_arena(|arena| {
                arena.set_budget(ArenaBudget::new(baseline + cache + 128, budget.max_nodes))
            })
            .unwrap();
        let admitted = owner
            .with_arena(|arena| arena.reserve_workspace(1))
            .unwrap()
            .expect("optional weak cache must yield to live workspace");
        owner
            .with_arena(|arena| {
                assert_eq!(arena.workspace_bytes(), 129);
                assert_eq!(arena.budget().max_bytes, baseline + cache + 128);
            })
            .unwrap();
        owner.full_gc().unwrap();
        assert_eq!(
            unsafe { crate::sexp::accessors::STRING_ELT(live.as_raw(), 0) },
            character.as_raw()
        );
        drop(admitted);
        // A refused cache rebuild must still return a valid fresh literal.
        owner
            .with_arena(|arena| {
                let bytes = arena.total_bytes_allocated() + arena.workspace_bytes();
                arena.set_budget(ArenaBudget::new(bytes + 1024, budget.max_nodes));
                assert!(
                    !arena
                        .immutable_bytecode_string(character.as_raw())
                        .is_null()
                );
                assert_eq!(arena.workspace_bytes(), 128);
                arena.set_budget(budget);
                let value = arena.immutable_bytecode_string(character.as_raw());
                assert!(!value.is_null());
                assert_eq!(arena.immutable_bytecode_string(character.as_raw()), value);
                assert_eq!(arena.workspace_bytes(), cache + 128);
            })
            .unwrap();
        drop(held);
    });
}

#[test]
fn bytecode_string_denial_keeps_other_owner_cache_and_held_admission() {
    let mut left = RArena::new();
    let mut right = RArena::new();
    for arena in [&mut left, &mut right] {
        let character = arena.alloc_charsxp(b"owner-local cache");
        assert!(!arena.immutable_bytecode_string(character).is_null());
    }
    let right_workspace = right.workspace_bytes();
    let held = left.reserve_workspace(128).unwrap();
    left.set_budget(ArenaBudget::new(left.total_bytes_allocated() + 128, 0));
    assert!(
        left.reserve_workspace(1).is_none(),
        "eviction must not release live tickets"
    );
    assert_eq!(left.workspace_bytes(), 128);
    assert_eq!(right.workspace_bytes(), right_workspace);
    drop(held);
    let small = left.reserve_workspace(1).unwrap();
    assert_eq!(left.workspace_bytes(), 1);
    assert_eq!(right.workspace_bytes(), right_workspace);
    drop(small);
}
