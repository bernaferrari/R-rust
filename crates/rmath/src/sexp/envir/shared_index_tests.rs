//! Canonical frame sharing must reuse proofs without owning either environment.
use super::*;
use crate::sexp::{
    env_hash::{self, BindingLookup},
    object::{SessionNodeFactory, SexpMut},
    session::RSession,
    symbol::Rf_install,
};
use std::ffi::CString;

fn symbol<'s>(factory: &SessionNodeFactory<'s>, name: &str) -> Sexp<'s> {
    let name = CString::new(name).unwrap();
    // The original factory owns the symbol immediately; no payload loan crosses interning.
    factory.wrap(unsafe { Rf_install(name.as_ptr()) }).unwrap()
}

fn integer<'s>(factory: &SessionNodeFactory<'s>, value: i32) -> Sexp<'s> {
    let node = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
        .unwrap();
    let mut node = SexpMut::try_from_checked(node).unwrap();
    node.try_set_integer_elt(0, value).unwrap();
    node.freeze()
}

fn environment<'s>(factory: &SessionNodeFactory<'s>, frame: &Sexp<'_>) -> Sexp<'s> {
    let env = factory
        .allocate(|arena| Some(arena.alloc_node(SEXPTYPE::ENVSXP)))
        .unwrap();
    // Both values retain their original allocations; copied field mutation borrows no payload.
    unsafe { crate::sexp::accessors::SET_FRAME(env.as_raw(), frame.as_raw()) };
    env
}

fn lookup(env: &Sexp<'_>, name: &Sexp<'_>) -> Option<Sexp<'static>> {
    // Owning inputs preserve the exact shared graph through any active-binding callback.
    unsafe { find_var_in_frame_result(env.clone(), name.clone()) }
        .unwrap()
        .map(|v| v.into_owned().unwrap())
}

fn original_base_aliases<'s>(
    session: &'s RSession,
    factory: &SessionNodeFactory<'s>,
) -> (Sexp<'s>, Sexp<'s>) {
    let base = factory
        .wrap(unsafe { crate::sexp::globals::R_BaseEnv() })
        .unwrap();
    let namespace_name = symbol(factory, ".BaseNamespaceEnv");
    // Bootstrap's placeholder creates the shared cell before the namespace
    // acquires that frame. Replacing its value must preserve both heads.
    unsafe { define_var_safe(namespace_name.clone(), base.clone(), base.clone()) };
    let namespace = environment(factory, &base.try_frame().unwrap());
    let global = session.global_env().unwrap();
    unsafe {
        crate::sexp::accessors::SET_ENCLOS(namespace.as_raw(), global.as_raw());
        define_var_safe(namespace_name, namespace.clone(), base.clone());
    }
    assert_eq!(unsafe { R_BaseNamespace() }, namespace.as_raw());
    assert_eq!(base.try_frame().unwrap(), namespace.try_frame().unwrap());
    env_hash::promote_to_hash_table(base.as_raw());
    assert!(!env_hash::env_has_hash_table(namespace.as_raw()));
    (base, namespace)
}

#[test]
fn shared_base_frame_minimal_original_aliases_mirror_mutations_and_locks() {
    let session = RSession::new_for_gc_tests();
    let factory = SessionNodeFactory::new(session.owner_token().unwrap());
    let (base, namespace) = original_base_aliases(&session, &factory);
    let name = symbol(&factory, "original_shared_mutations");
    let value = integer(&factory, 29);
    unsafe { define_var_safe(name.clone(), value.clone(), namespace.clone()) };
    drop(value);
    session.owner_token().unwrap().full_gc().unwrap();
    for env in [&base, &namespace] {
        assert_eq!(lookup(env, &name).unwrap().integer_elt(0), Some(29));
        assert!(matches!(
            env_hash::hash_binding_lookup(env, &name),
            BindingLookup::Cell(_)
        ));
    }
    lock_binding_raw(namespace.as_raw(), name.as_raw());
    assert!(binding_is_locked_raw(base.as_raw(), name.as_raw()));
    let replacement = integer(&factory, 51);
    let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        define_var_safe(name.clone(), replacement.clone(), base.clone());
    }));
    assert!(rejected.is_err());
    assert_eq!(lookup(&namespace, &name).unwrap().integer_elt(0), Some(29));
    unlock_binding_raw(base.as_raw(), name.as_raw());
    assert!(!binding_is_locked_raw(namespace.as_raw(), name.as_raw()));
    unsafe { define_var_safe(name.clone(), replacement.clone(), base.clone()) };
    drop(replacement);
    session.owner_token().unwrap().full_gc().unwrap();
    assert_eq!(lookup(&namespace, &name).unwrap().integer_elt(0), Some(51));
    unsafe { remove_binding_raw(namespace.as_raw(), name.as_raw()) };
    assert_eq!(base.try_frame().unwrap(), namespace.try_frame().unwrap());
    session.owner_token().unwrap().full_gc().unwrap();
    for env in [&base, &namespace] {
        assert!(lookup(env, &name).is_none());
        assert_eq!(
            env_hash::hash_binding_lookup(env, &name),
            BindingLookup::Absent
        );
    }
    assert!(!env_hash::env_has_hash_table(namespace.as_raw()));
}

#[test]
fn shared_base_frame_minimal_original_active_aliases_collect_and_release_handler() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let (base, namespace) = original_base_aliases(&session, &factory);
        let name = symbol(&factory, "original_shared_active");
        let expression = session
            .owner_token()
            .unwrap()
            .with_arena(|arena| {
                crate::eval::parser::parse("function() { gc(); 1L + 63L }", arena, factory.clone())
            })
            .unwrap()
            .unwrap();
        let handler = factory
            .wrap(unsafe { crate::eval::eval::Rf_eval(expression.as_raw(), namespace.as_raw()) })
            .unwrap();
        let handler_node = handler.allocation().unwrap().clone();
        make_active_binding_raw(namespace.as_raw(), name.as_raw(), handler.as_raw());
        drop(expression);
        drop(handler);
        session.owner_token().unwrap().full_gc().unwrap();
        assert!(handler_node.is_live());
        for env in [&base, &namespace] {
            assert!(binding_is_active_raw(env.as_raw(), name.as_raw()));
            assert_eq!(lookup(env, &name).unwrap().integer_elt(0), Some(64));
            assert!(matches!(
                env_hash::hash_binding_lookup(env, &name),
                BindingLookup::Cell(_)
            ));
        }
        let result = lookup(&namespace, &name).unwrap();
        let result_node = result.allocation().unwrap().clone();
        remove_binding_raw(base.as_raw(), name.as_raw());
        for env in [&base, &namespace] {
            assert!(!binding_is_active_raw(env.as_raw(), name.as_raw()));
            assert!(lookup(env, &name).is_none());
        }
        assert_eq!(base.try_frame().unwrap(), namespace.try_frame().unwrap());
        session.owner_token().unwrap().full_gc().unwrap();
        assert!(
            !handler_node.is_live(),
            "removed aliases must release their actual handler graph"
        );
        assert_eq!(result.integer_elt(0), Some(64));
        drop(result);
        session.owner_token().unwrap().full_gc().unwrap();
        assert!(
            !result_node.is_live(),
            "the returned value's sole actual lease must release normally"
        );
        assert!(!env_hash::env_has_hash_table(namespace.as_raw()));
    });
}

#[test]
fn shared_base_frame_bootstrap_reuses_the_original_canonical_index() {
    let session = RSession::new_without_default_packages();
    session.with_active(|| {
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let base = factory
            .wrap(unsafe { crate::sexp::globals::R_BaseEnv() })
            .unwrap();
        let namespace = factory.wrap(unsafe { R_BaseNamespace() }).unwrap();
        let name = symbol(&factory, "objects");
        assert_eq!(base.try_frame().unwrap(), namespace.try_frame().unwrap());
        assert!(matches!(
            env_hash::hash_binding_lookup(&base, &name),
            BindingLookup::Cell(_)
        ));
        assert!(
            matches!(
                env_hash::hash_binding_lookup(&namespace, &name),
                BindingLookup::Cell(_)
            ),
            "the real base namespace must use its shared frame's canonical proof"
        );
        assert!(
            !env_hash::env_has_hash_table(namespace.as_raw()),
            "sharing must not create another cache"
        );
        assert_eq!(lookup(&base, &name), lookup(&namespace, &name));
    });
}

#[test]
fn shared_base_frame_alias_writes_removals_and_closure_changes_stay_coherent() {
    let session = RSession::new_without_default_packages();
    session.with_active(|| {
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let base = factory
            .wrap(unsafe { crate::sexp::globals::R_BaseEnv() })
            .unwrap();
        let namespace = factory.wrap(unsafe { R_BaseNamespace() }).unwrap();
        let name = symbol(&factory, "shared_index_alias_probe");
        let fun = factory
            .allocate(|arena| Some(arena.alloc_node(SEXPTYPE::CLOSXP)))
            .unwrap();
        let body = integer(&factory, 64);
        unsafe {
            crate::sexp::accessors::SET_FORMALS(fun.as_raw(), factory.nil().as_raw());
            crate::sexp::accessors::SET_BODY(fun.as_raw(), body.as_raw());
            crate::sexp::accessors::SET_CLOENV(fun.as_raw(), namespace.as_raw());
            define_var_safe(name.clone(), fun.clone(), namespace.clone());
        }
        for env in [&base, &namespace] {
            assert!(matches!(
                env_hash::hash_binding_lookup(env, &name),
                BindingLookup::Cell(_)
            ));
            assert_eq!(lookup(env, &name).unwrap(), fun);
        }
        let replacement = integer(&factory, 91);
        unsafe { define_var_safe(name.clone(), replacement.clone(), base.clone()) };
        assert_eq!(lookup(&namespace, &name).unwrap(), replacement);
        unsafe { remove_binding_raw(namespace.as_raw(), name.as_raw()) };
        for env in [&base, &namespace] {
            assert!(lookup(env, &name).is_none());
        }
        assert_eq!(base.try_frame().unwrap(), namespace.try_frame().unwrap());
        session.owner_token().unwrap().full_gc().unwrap();
        for env in [&base, &namespace] {
            assert_eq!(
                env_hash::hash_binding_lookup(env, &name),
                BindingLookup::Absent
            );
        }
        assert!(!env_hash::env_has_hash_table(namespace.as_raw()));
    });
}

#[test]
fn shared_base_frame_current_values_and_absence_use_one_original_index() {
    let session = RSession::new_for_gc_tests();
    let factory = SessionNodeFactory::new(session.owner_token().unwrap());
    let name = symbol(&factory, "shared_value");
    let missing = symbol(&factory, "shared_absent");
    let cell = factory
        .pairlist_cell(&integer(&factory, 17), &factory.nil(), &name)
        .unwrap();
    let indexed = environment(&factory, &cell);
    let alias = environment(&factory, &cell);
    env_hash::promote_to_hash_table(indexed.as_raw());
    assert_eq!(lookup(&indexed, &name).unwrap().integer_elt(0), Some(17));
    assert!(matches!(
        env_hash::hash_binding_lookup(&alias, &name),
        BindingLookup::Cell(_)
    ));
    assert_eq!(
        env_hash::hash_binding_lookup(&alias, &missing),
        BindingLookup::Absent
    );
    let updated = integer(&factory, 42);
    unsafe { crate::sexp::accessors::SETCAR(cell.as_raw(), updated.as_raw()) };
    drop(updated);
    session.owner_token().unwrap().full_gc().unwrap();
    assert_eq!(lookup(&alias, &name).unwrap().integer_elt(0), Some(42));
    assert!(!env_hash::env_has_hash_table(alias.as_raw()));
}

#[test]
fn shared_base_frame_name_and_tail_mutation_invalidate_shared_proofs() {
    let session = RSession::new_for_gc_tests();
    let factory = SessionNodeFactory::new(session.owner_token().unwrap());
    let old = symbol(&factory, "shared_old");
    let new = symbol(&factory, "shared_new");
    let tail_name = symbol(&factory, "shared_tail");
    let tail = factory
        .pairlist_cell(&integer(&factory, 2), &factory.nil(), &tail_name)
        .unwrap();
    let head = factory
        .pairlist_cell(&integer(&factory, 1), &tail, &old)
        .unwrap();
    let indexed = environment(&factory, &head);
    let alias = environment(&factory, &head);
    env_hash::promote_to_hash_table(indexed.as_raw());
    assert_eq!(
        lookup(&indexed, &tail_name).unwrap().integer_elt(0),
        Some(2)
    );
    assert_eq!(
        env_hash::hash_binding_lookup(&alias, &new),
        BindingLookup::Absent
    );
    unsafe { crate::sexp::accessors::SETTAG(head.as_raw(), new.as_raw()) };
    assert_eq!(
        env_hash::hash_binding_lookup(&alias, &new),
        BindingLookup::Unavailable
    );
    assert_eq!(lookup(&alias, &new).unwrap().integer_elt(0), Some(1));
    assert!(lookup(&indexed, &old).is_none()); // Rebuild only the original index.
    assert!(matches!(
        env_hash::hash_binding_lookup(&alias, &new),
        BindingLookup::Cell(_)
    ));
    unsafe { crate::sexp::accessors::SETCDR(head.as_raw(), factory.nil().as_raw()) };
    assert_eq!(
        env_hash::hash_binding_lookup(&alias, &tail_name),
        BindingLookup::Unavailable
    );
    assert!(lookup(&alias, &tail_name).is_none());
    assert!(
        tail.is_live(),
        "detached tail remains independently owned, but isn't a binding"
    );
    assert!(lookup(&indexed, &tail_name).is_none());
    assert_eq!(
        env_hash::hash_binding_lookup(&alias, &tail_name),
        BindingLookup::Absent
    );
}

#[test]
fn shared_base_frame_distinct_heads_and_rehash_do_not_share_stale_membership() {
    let session = RSession::new_for_gc_tests();
    let factory = SessionNodeFactory::new(session.owner_token().unwrap());
    let name = symbol(&factory, "shared_shadow");
    let tail = factory
        .pairlist_cell(&integer(&factory, 3), &factory.nil(), &name)
        .unwrap();
    let indexed = environment(&factory, &tail);
    let alias = environment(&factory, &tail);
    env_hash::promote_to_hash_table(indexed.as_raw());
    assert_eq!(lookup(&indexed, &name).unwrap().integer_elt(0), Some(3));
    let head = factory
        .pairlist_cell(&integer(&factory, 9), &tail, &name)
        .unwrap();
    unsafe { crate::sexp::accessors::SET_FRAME(alias.as_raw(), head.as_raw()) };
    assert_eq!(
        env_hash::hash_binding_lookup(&alias, &name),
        BindingLookup::Unavailable
    );
    assert_eq!(lookup(&alias, &name).unwrap().integer_elt(0), Some(9));
    assert_eq!(lookup(&indexed, &name).unwrap().integer_elt(0), Some(3));
    env_hash::mark_hashed(alias.as_raw(), 67);
    assert_eq!(lookup(&alias, &name).unwrap().integer_elt(0), Some(9));
    unsafe { crate::sexp::accessors::SET_FRAME(alias.as_raw(), tail.as_raw()) };
    assert_eq!(lookup(&alias, &name).unwrap().integer_elt(0), Some(3));
    assert_eq!(lookup(&indexed, &name).unwrap().integer_elt(0), Some(3));
    session.owner_token().unwrap().full_gc().unwrap();
    assert_eq!(lookup(&alias, &name).unwrap().integer_elt(0), Some(3));
}

#[test]
fn shared_base_frame_indexes_do_not_root_aliases_or_retired_generations() {
    let session = RSession::new_for_gc_tests();
    let factory = SessionNodeFactory::new(session.owner_token().unwrap());
    let name = symbol(&factory, "shared_retired");
    let cell = factory
        .pairlist_cell(&integer(&factory, 8), &factory.nil(), &name)
        .unwrap();
    let indexed = environment(&factory, &cell);
    let alias = environment(&factory, &cell);
    env_hash::promote_to_hash_table(indexed.as_raw());
    assert_eq!(lookup(&indexed, &name).unwrap().integer_elt(0), Some(8));
    let indexed_node = indexed.allocation().unwrap().clone();
    drop(indexed);
    drop(cell);
    session.owner_token().unwrap().full_gc().unwrap();
    assert!(
        !indexed_node.is_live(),
        "an alias must not retain the indexed environment"
    );
    assert_eq!(lookup(&alias, &name).unwrap().integer_elt(0), Some(8));
    let alias_node = alias.allocation().unwrap().clone();
    let alias_address = alias.as_raw() as usize;
    let old_link = alias_node.link().unwrap();
    drop(alias);
    session.owner_token().unwrap().full_gc().unwrap();
    assert!(!alias_node.is_live());
    let mut replacements = Vec::new();
    for _ in 0..256 {
        let replacement = environment(&factory, &factory.nil());
        assert!(lookup(&replacement, &name).is_none());
        if replacement.as_raw() as usize == alias_address {
            assert_ne!(replacement.allocation().unwrap().link().unwrap(), old_link);
            assert!(alias_node.heap_identity().resolve_link(old_link).is_none());
            return;
        }
        replacements.push(replacement);
    }
    panic!("the real allocator did not exercise the retired environment address");
}

#[test]
fn shared_base_frame_canonical_proof_survives_original_environment_release() {
    let session = RSession::new_for_gc_tests();
    let factory = SessionNodeFactory::new(session.owner_token().unwrap());
    let name = symbol(&factory, "canonical_frame_after_source_release");
    let cell = factory
        .pairlist_cell(&integer(&factory, 19), &factory.nil(), &name)
        .unwrap();
    let source = environment(&factory, &cell);
    let alias = environment(&factory, &cell);
    env_hash::promote_to_hash_table(source.as_raw());
    assert_eq!(lookup(&source, &name).unwrap().integer_elt(0), Some(19));
    let source_node = source.allocation().unwrap().clone();
    let cell_link = cell.allocation().unwrap().link().unwrap();
    drop(source);
    drop(cell);
    session.owner_token().unwrap().full_gc().unwrap();
    assert!(!source_node.is_live());
    assert_eq!(
        env_hash::hash_binding_lookup(&alias, &name),
        BindingLookup::Cell(cell_link),
        "the actual frame proof belongs to its surviving frame, not a retired environment"
    );
    assert!(!env_hash::env_has_hash_table(alias.as_raw()));
    assert_eq!(lookup(&alias, &name).unwrap().integer_elt(0), Some(19));
}

#[test]
fn shared_base_frame_canonical_rekey_preserves_each_current_alias_head() {
    let session = RSession::new_for_gc_tests();
    let factory = SessionNodeFactory::new(session.owner_token().unwrap());
    let name = symbol(&factory, "canonical_rekey_shadow");
    let later = symbol(&factory, "canonical_rekey_later");
    let old = factory
        .pairlist_cell(&integer(&factory, 11), &factory.nil(), &name)
        .unwrap();
    let source = environment(&factory, &old);
    let promoted_alias = environment(&factory, &old);
    let passive_alias = environment(&factory, &old);
    for env in [&source, &promoted_alias] {
        env_hash::promote_to_hash_table(env.as_raw());
        assert_eq!(lookup(env, &name).unwrap().integer_elt(0), Some(11));
    }
    let head = factory
        .pairlist_cell(&integer(&factory, 33), &old, &name)
        .unwrap();
    unsafe { crate::sexp::accessors::SET_FRAME(source.as_raw(), head.as_raw()) };
    assert_eq!(lookup(&source, &name).unwrap().integer_elt(0), Some(33));
    // Rekeying moved one proof. The surviving old head still has its own
    // canonical semantics, before and after another marked alias rebuilds it.
    assert_eq!(
        lookup(&passive_alias, &name).unwrap().integer_elt(0),
        Some(11)
    );
    assert_eq!(
        lookup(&promoted_alias, &name).unwrap().integer_elt(0),
        Some(11)
    );
    assert_eq!(
        env_hash::hash_binding_lookup(&passive_alias, &name),
        BindingLookup::Cell(old.allocation().unwrap().link().unwrap())
    );
    let tail = factory
        .pairlist_cell(&integer(&factory, 55), &factory.nil(), &later)
        .unwrap();
    unsafe { crate::sexp::accessors::SETCDR(old.as_raw(), tail.as_raw()) };
    for env in [&source, &promoted_alias, &passive_alias] {
        assert_eq!(lookup(env, &later).unwrap().integer_elt(0), Some(55));
    }
    unsafe { crate::sexp::accessors::SET_FRAME(source.as_raw(), tail.as_raw()) };
    assert!(lookup(&source, &name).is_none());
    for env in [&promoted_alias, &passive_alias] {
        assert_eq!(lookup(env, &name).unwrap().integer_elt(0), Some(11));
    }
    session.owner_token().unwrap().full_gc().unwrap();
    for env in [&source, &promoted_alias, &passive_alias] {
        assert_eq!(lookup(env, &later).unwrap().integer_elt(0), Some(55));
    }
}

#[test]
fn shared_base_frame_canonical_proof_rejects_recycled_frame_generation() {
    let session = RSession::new_for_gc_tests();
    let factory = SessionNodeFactory::new(session.owner_token().unwrap());
    let name = symbol(&factory, "canonical_retired_frame");
    let cell = factory
        .pairlist_cell(&integer(&factory, 7), &factory.nil(), &name)
        .unwrap();
    let source = environment(&factory, &cell);
    env_hash::promote_to_hash_table(source.as_raw());
    assert_eq!(lookup(&source, &name).unwrap().integer_elt(0), Some(7));
    let old_node = cell.allocation().unwrap().clone();
    let old_link = old_node.link().unwrap();
    let old_address = cell.as_raw() as usize;
    let new_value = integer(&factory, 82);
    drop(cell);
    drop(source);
    session.owner_token().unwrap().full_gc().unwrap();
    assert!(
        !old_node.is_live(),
        "a canonical proof cannot root its frame"
    );
    let mut replacements = Vec::new();
    for _ in 0..256 {
        let replacement = factory
            .pairlist_cell(&new_value, &factory.nil(), &name)
            .unwrap();
        if replacement.as_raw() as usize == old_address {
            assert_ne!(replacement.allocation().unwrap().link().unwrap(), old_link);
            assert!(old_node.heap_identity().resolve_link(old_link).is_none());
            let fresh_env = environment(&factory, &replacement);
            assert_eq!(
                env_hash::hash_binding_lookup(&fresh_env, &name),
                BindingLookup::Unavailable
            );
            assert_eq!(lookup(&fresh_env, &name).unwrap().integer_elt(0), Some(82));
            return;
        }
        replacements.push(replacement);
    }
    panic!("the real allocator did not exercise the retired frame address");
}

#[test]
fn shared_base_frame_foreign_heaps_cannot_supply_alias_proofs_or_names() {
    let left = RSession::new_for_gc_tests();
    let (env, name) = left.with_active(|| {
        let factory = SessionNodeFactory::new(left.owner_token().unwrap());
        let name = symbol(&factory, "shared_domain");
        let cell = factory
            .pairlist_cell(&integer(&factory, 1), &factory.nil(), &name)
            .unwrap();
        let env = environment(&factory, &cell);
        env_hash::promote_to_hash_table(env.as_raw());
        assert!(lookup(&env, &name).is_some());
        (env.into_owned().unwrap(), name.into_owned().unwrap())
    });
    let right = RSession::new_for_gc_tests();
    let factory = SessionNodeFactory::new(right.owner_token().unwrap());
    let foreign_name = symbol(&factory, "shared_domain");
    let foreign_env = environment(&factory, &factory.nil());
    assert_eq!(
        env_hash::hash_binding_lookup(&env, &foreign_name),
        BindingLookup::Unavailable
    );
    assert_eq!(
        env_hash::hash_binding_lookup(&foreign_env, &name),
        BindingLookup::Unavailable
    );
    left.with_active(|| assert_eq!(lookup(&env, &name).unwrap().integer_elt(0), Some(1)));
}

#[test]
fn shared_base_frame_active_binding_executes_in_the_original_target_environment() {
    let session = RSession::new_for_gc_tests();
    let factory = SessionNodeFactory::new(session.owner_token().unwrap());
    let name = symbol(&factory, "shared_active");
    let cell = factory
        .pairlist_cell(&factory.unbound(), &factory.nil(), &name)
        .unwrap();
    let indexed = environment(&factory, &cell);
    let alias = environment(&factory, &cell);
    env_hash::promote_to_hash_table(indexed.as_raw());
    assert!(matches!(
        env_hash::hash_binding_lookup(&indexed, &name),
        BindingLookup::Cell(_)
    ));
    let body = integer(&factory, 73);
    let fun = factory
        .allocate(|arena| Some(arena.alloc_node(SEXPTYPE::CLOSXP)))
        .unwrap();
    let global = session.global_env().unwrap();
    unsafe {
        crate::sexp::accessors::SET_FORMALS(fun.as_raw(), factory.nil().as_raw());
        crate::sexp::accessors::SET_BODY(fun.as_raw(), body.as_raw());
        crate::sexp::accessors::SET_CLOENV(fun.as_raw(), global.as_raw());
        make_active_binding_raw(alias.as_raw(), name.as_raw(), fun.as_raw());
    }
    assert_eq!(
        lookup(&indexed, &name).unwrap(),
        fun,
        "sharing the proof must not run another environment's handler"
    );
    drop(fun);
    drop(body);
    session.owner_token().unwrap().full_gc().unwrap();
    assert!(matches!(
        env_hash::hash_binding_lookup(&alias, &name),
        BindingLookup::Cell(_)
    ));
    let result = lookup(&alias, &name).unwrap();
    assert_eq!(result.integer_elt(0), Some(73));
    unsafe {
        remove_binding_raw(alias.as_raw(), name.as_raw());
        remove_binding_raw(indexed.as_raw(), name.as_raw());
    }
    drop(cell);
    session.owner_token().unwrap().full_gc().unwrap();
    assert_eq!(
        result.integer_elt(0),
        Some(73),
        "the actual result owns its graph after handler removal"
    );
    assert!(!env_hash::env_has_hash_table(alias.as_raw()));
}
