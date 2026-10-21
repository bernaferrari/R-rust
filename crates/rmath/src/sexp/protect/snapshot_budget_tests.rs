//! Actual checked/native/update snapshots of each original runtime root channel.
use super::*;
use crate::sexp::{RSession, memory::ArenaBudget};
fn verify(channel: u8, view: u8, deny: bool) {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let instance = owner.as_ptr();
        let value = RootValue::from_owner(instance, crate::sexp::globals::R_NilValue()).unwrap();
        for _ in 0..3 {
            match channel {
                0 => (*instance).legacy_protect.push(value.clone(), "snapshot control"),
                1 => { (*instance).root_table.try_claim(value.clone(), true).unwrap(); },
                _ => { (*instance).preserve_stack.try_push(value.clone()).unwrap(); },
            }
        }
        let before = owner.with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes())).unwrap();
        assert_eq!(before.1, 0);
        if deny { owner.with_arena(|a| a.set_budget(ArenaBudget::new(before.0, 0))).unwrap(); }
        let inspect = || {
            macro_rules! snapshot {
                ($expression:expr) => {{
                    let snapshot = $expression;
                    if !deny {
                        let bytes = std::mem::size_of_val(&snapshot[..]);
                        assert!(bytes > 0);
                        let during = owner.with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes())).unwrap();
                        assert!(during.0 + during.1 >= before.0 + before.1 + bytes,
                            "snapshot must remain funded while live: before={before:?}, during={during:?}, bytes={bytes}");
                    }
                    drop(snapshot);
                }};
            }
            macro_rules! table {
                ($table:expr) => { match view {
                    0 => snapshot!($table.checked_entries_snapshot()),
                    1 => snapshot!($table.entries_snapshot()),
                    _ => snapshot!($table.updates_snapshot()),
                }};
            }
            match channel {
                0 => table!((*instance).legacy_protect),
                1 => table!((*instance).root_table),
                _ => table!((*instance).preserve_stack),
            }
        };
        if deny {
            let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(inspect))
                .expect_err("root snapshot escaped the original zero-headroom limit");
            assert!(error.downcast_ref::<crate::sexp::context::RError>().is_some());
        } else { inspect(); }
        assert_eq!(owner.with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes())).unwrap(), before);
    });
}

#[test]
fn root_snapshot_budget_legacy_checked_lifetime() {
    verify(0, 0, false);
}

#[test]
fn root_snapshot_budget_legacy_checked_denial() {
    verify(0, 0, true);
}

#[test]
fn root_snapshot_budget_legacy_native_lifetime() {
    verify(0, 1, false);
}

#[test]
fn root_snapshot_budget_legacy_native_denial() {
    verify(0, 1, true);
}

#[test]
fn root_snapshot_budget_legacy_updates_lifetime() {
    verify(0, 2, false);
}

#[test]
fn root_snapshot_budget_legacy_updates_denial() {
    verify(0, 2, true);
}

#[test]
fn root_snapshot_budget_stable_checked_lifetime() {
    verify(1, 0, false);
}

#[test]
fn root_snapshot_budget_stable_checked_denial() {
    verify(1, 0, true);
}

#[test]
fn root_snapshot_budget_stable_native_lifetime() {
    verify(1, 1, false);
}

#[test]
fn root_snapshot_budget_stable_native_denial() {
    verify(1, 1, true);
}

#[test]
fn root_snapshot_budget_stable_updates_lifetime() {
    verify(1, 2, false);
}

#[test]
fn root_snapshot_budget_stable_updates_denial() {
    verify(1, 2, true);
}

#[test]
fn root_snapshot_budget_preserved_checked_lifetime() {
    verify(2, 0, false);
}

#[test]
fn root_snapshot_budget_preserved_checked_denial() {
    verify(2, 0, true);
}

#[test]
fn root_snapshot_budget_preserved_native_lifetime() {
    verify(2, 1, false);
}

#[test]
fn root_snapshot_budget_preserved_native_denial() {
    verify(2, 1, true);
}

#[test]
fn root_snapshot_budget_preserved_updates_lifetime() {
    verify(2, 2, false);
}

#[test]
fn root_snapshot_budget_preserved_updates_denial() {
    verify(2, 2, true);
}

#[test]
fn root_snapshot_budget_iterator_keeps_charge_through_exhaustion_and_unwind() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let table = &(*owner.as_ptr()).root_table;
        let value =
            RootValue::from_owner(owner.as_ptr(), crate::sexp::globals::R_NilValue()).unwrap();
        for _ in 0..3 {
            table.try_claim(value.clone(), true).unwrap();
        }
        let ledger = (*owner.as_ptr()).heap_identity.byte_ledger();
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        let snapshot = table.updates_snapshot();
        let funded = ledger.workspace_bytes_for_test();
        assert!(funded >= std::mem::size_of_val(&snapshot[..]));
        let mut iterator = snapshot.into_iter();
        assert_eq!(iterator.len(), 3);
        assert!(iterator.next().is_some());
        assert_eq!(ledger.workspace_bytes_for_test(), funded);
        for _ in iterator.by_ref() {}
        assert_eq!(iterator.len(), 0);
        assert_eq!(
            ledger.workspace_bytes_for_test(),
            funded,
            "exhausted iterator still owns its physical allocation"
        );
        drop(iterator);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _iterator = table.updates_snapshot().into_iter();
            assert_eq!(ledger.workspace_bytes_for_test(), funded);
            panic!("snapshot iterator unwind control");
        }));
        assert!(unwind.is_err());
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
    });
}

#[test]
fn root_snapshot_budget_original_close_and_other_active_interpreter() {
    let original = RSession::new_for_gc_tests();
    let (snapshot, ledger) = original.with_active(|| unsafe {
        let owner = original.owner_token().unwrap();
        let value =
            RootValue::from_owner(owner.as_ptr(), crate::sexp::globals::R_NilValue()).unwrap();
        for _ in 0..3 {
            (*owner.as_ptr())
                .root_table
                .try_claim(value.clone(), true)
                .unwrap();
        }
        (
            (*owner.as_ptr()).root_table.entries_snapshot(),
            (*owner.as_ptr()).heap_identity.byte_ledger(),
        )
    });
    let funded = ledger.workspace_bytes_for_test();
    assert!(funded >= std::mem::size_of_val(&snapshot[..]) && funded > 0);
    let other = RSession::new_for_gc_tests();
    other.with_active(|| {
        let owner = other.owner_token().unwrap();
        let before = owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap();
        drop(original);
        assert_eq!(ledger.allocated_bytes(), 0);
        assert_eq!(ledger.workspace_bytes_for_test(), funded);
        assert_eq!(snapshot.len(), 3);
        drop(snapshot);
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
fn root_snapshot_budget_empty_control_succeeds_without_headroom() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let instance = owner.as_ptr();
        let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(before, 0)))
            .unwrap();
        macro_rules! empty {
            ($table:expr) => {
                assert!($table.checked_entries_snapshot().is_empty());
                assert!($table.entries_snapshot().is_empty());
                assert!($table.updates_snapshot().is_empty());
            };
        }
        empty!((*instance).legacy_protect);
        empty!((*instance).root_table);
        empty!((*instance).preserve_stack);
        assert_eq!(
            owner
                .with_arena(|a| (
                    a.total_bytes_allocated(),
                    a.workspace_bytes(),
                    a.budget().max_bytes
                ))
                .unwrap(),
            (before, 0, before)
        );
    });
}

#[test]
fn root_snapshot_budget_scope_gc_reclaims_garbage_under_lowered_limit() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let instance = owner.as_ptr();
        let object = owner
            .with_arena(|arena| {
                let object = arena.alloc_vector(crate::sexp::ffi::SEXPTYPE::INTSXP, 1);
                *crate::sexp::accessors::INTEGER(object) = 99;
                let value = RootValue::from_owner(instance, object).unwrap();
                (*instance).root_table.try_claim(value, true).unwrap();
                for _ in 0..3 {
                    arena.alloc_vector(crate::sexp::ffi::SEXPTYPE::INTSXP, 1);
                }
                arena.set_budget(ArenaBudget::new(1, 0));
                object
            })
            .unwrap();
        let (_, freed) = crate::sexp::gengc::full_gc();
        assert!(
            freed >= 3,
            "collection must perform actual reclamation under the original one-byte limit"
        );
        with_protected_objects(|legacy, roots| {
            assert!(legacy.is_empty());
            assert_eq!(roots, [object]);
            assert_eq!(*crate::sexp::accessors::INTEGER(roots[0]), 99);
            // The diagnostic copies retain no table loan across this callback.
            let value =
                RootValue::from_owner(instance, crate::sexp::globals::R_NilValue()).unwrap();
            let root = (*instance).root_table.try_claim(value, true).unwrap();
            (*instance).root_table.release(root);
        });
        assert_eq!(
            owner
                .with_arena(|arena| (arena.workspace_bytes(), arena.budget().max_bytes))
                .unwrap(),
            (0, 1)
        );
    });
}

#[test]
fn root_snapshot_budget_scope_large_diagnostic_view_requires_admission() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let instance = owner.as_ptr();
        let value = RootValue::from_owner(instance, crate::sexp::globals::R_NilValue()).unwrap();
        for _ in 0..33 {
            (*instance)
                .root_table
                .try_claim(value.clone(), true)
                .unwrap();
        }
        with_protected_objects(|legacy, roots| {
            assert!(legacy.is_empty());
            assert_eq!(roots.len(), 33);
            assert!(
                owner.with_arena(|arena| arena.workspace_bytes()).unwrap()
                    >= 33 * std::mem::size_of::<crate::sexp::ffi::SEXP>()
            );
        });
        owner
            .with_arena(|arena| arena.set_budget(ArenaBudget::new(1, 0)))
            .unwrap();
        let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_protected_objects(|_, _| {
                panic!("unfunded large diagnostic callback must not run")
            });
        }))
        .unwrap_err();
        assert!(
            denied
                .downcast_ref::<crate::sexp::context::RError>()
                .is_some()
        );
        (*instance).root_table.clear();
        with_protected_objects(|legacy, roots| {
            assert!(legacy.is_empty() && roots.is_empty());
        });
        assert_eq!(
            owner
                .with_arena(|arena| (arena.workspace_bytes(), arena.budget().max_bytes))
                .unwrap(),
            (0, 1)
        );
    });
}

#[test]
fn root_owner_directory_registered_lend_recovers_canonical_pointer_and_rejects_foreign_heap() {
    let session = RSession::new_for_gc_tests();
    let foreign = RSession::new_for_gc_tests();
    let foreign_value = foreign.with_active(|| {
        foreign
            .owner_token()
            .unwrap()
            .node_factory()
            .character("foreign")
            .unwrap()
            .into_owned()
            .unwrap()
    });
    let foreign_ledger = foreign.with_active(|| unsafe {
        (*foreign.owner_token().unwrap().as_ptr())
            .heap_identity
            .byte_ledger()
    });
    let foreign_before = foreign_ledger.allocated_bytes();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let instance = owner.as_ptr();
        let (root, object) = owner
            .with_arena(|arena| {
                let object = arena.alloc_vector(crate::sexp::ffi::SEXPTYPE::INTSXP, 1);
                let address_only = std::ptr::without_provenance_mut(object.addr());
                let value = RootValue::from_owner(instance, address_only).unwrap();
                let canonical = value.projection().unwrap();
                assert_eq!(canonical, object);
                *crate::sexp::accessors::INTEGER(canonical) = 42;
                assert!(matches!(
                    RootValue::from_owner(instance, foreign_value.as_raw()),
                    Err(ProtectError::ForeignOwner)
                ));
                let root = (*instance).root_table.try_claim(value, true).unwrap();
                (root, object)
            })
            .unwrap();
        assert_eq!(*crate::sexp::accessors::INTEGER(object), 42);
        assert_eq!(
            (*instance).root_table.generation_at(root),
            Some(root.generation)
        );
        (*instance).root_table.release(root);
        assert_eq!(foreign_ledger.allocated_bytes(), foreign_before);
        assert_eq!(foreign_ledger.workspace_bytes_for_test(), 0);
    });
}
