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
