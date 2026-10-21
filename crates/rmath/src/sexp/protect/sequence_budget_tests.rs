//! Physical memory controls for the two genuine ordered native root channels.
use super::*;
use crate::sexp::{RSession, memory::ArenaBudget};

fn channel(instance: &crate::sexp::instance::RInstance, preserved: bool) -> &NativeRoots {
    if preserved {
        &instance.preserve_stack.roots
    } else {
        instance.legacy_protect.roots_for_test()
    }
}
fn retained(preserved: bool) {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let roots = channel(&*owner.as_ptr(), preserved);
        let old_bytes = roots.entries.borrow().physical_storage_for_test().1;
        let ledger = (*owner.as_ptr()).heap_identity.byte_ledger();
        let before = ledger.allocated_bytes();
        let value =
            RootValue::from_owner(owner.as_ptr(), crate::sexp::globals::R_NilValue()).unwrap();
        roots.try_push(value).unwrap();
        let bytes = roots.entries.borrow().physical_storage_for_test().1;
        assert!(bytes > old_bytes);
        assert_eq!(
            ledger.allocated_bytes() - before,
            bytes - old_bytes,
            "ordered roots must retain their actual capacity charge"
        );
        roots.entries.borrow_mut().clear();
        assert_eq!(ledger.allocated_bytes(), before + bytes - old_bytes);
    });
}
fn denied(preserved: bool, delta_only: bool) {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let roots = channel(&*owner.as_ptr(), preserved);
        let value =
            RootValue::from_owner(owner.as_ptr(), crate::sexp::globals::R_NilValue()).unwrap();
        let first = roots.try_push(value.clone()).unwrap();
        let capacity = roots.entries.borrow().physical_storage_for_test().0;
        for _ in 1..capacity {
            roots.try_push(value.clone()).unwrap();
        }
        let state = roots.entries.borrow().physical_storage_for_test();
        let before = (*owner.as_ptr())
            .heap_identity
            .byte_ledger()
            .allocated_bytes();
        let headroom = if delta_only { state.1 / capacity } else { 0 };
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(before + headroom, 0)))
            .unwrap();
        assert_eq!(roots.try_push(value.clone()), Err(ProtectError::Allocation));
        assert_eq!(roots.entries.borrow().physical_storage_for_test(), state);
        assert_eq!(roots.entries.borrow().len(), capacity);
        assert!(roots.entries.borrow().get(first).is_some());
        roots.entries.borrow_mut().pop_count(1);
        roots.try_push(value).unwrap();
        assert!(roots.entries.borrow().get(first).is_some());
        assert_eq!(
            owner
                .with_arena(|a| (
                    a.total_bytes_allocated(),
                    a.workspace_bytes(),
                    a.budget().max_bytes
                ))
                .unwrap(),
            (before, 0, before + headroom)
        );
    });
}
#[test]
fn ordered_root_budget_legacy_retained_capacity() {
    retained(false);
}
#[test]
fn ordered_root_budget_preserved_retained_capacity() {
    retained(true);
}
#[test]
fn ordered_root_budget_legacy_growth_denial_and_reuse() {
    denied(false, false);
}
#[test]
fn ordered_root_budget_preserved_growth_denial_and_reuse() {
    denied(true, false);
}

#[test]
fn ordered_root_budget_legacy_requires_full_replacement_peak() {
    denied(false, true);
}
#[test]
fn ordered_root_budget_preserved_requires_full_replacement_peak() {
    denied(true, true);
}
#[test]
fn ordered_root_budget_charges_release_with_final_interpreter() {
    for preserved in [false, true] {
        let session = RSession::new_for_gc_tests();
        let ledger = session.with_active(|| unsafe {
            let owner = session.owner_token().unwrap();
            let roots = channel(&*owner.as_ptr(), preserved);
            let value =
                RootValue::from_owner(owner.as_ptr(), crate::sexp::globals::R_NilValue()).unwrap();
            roots.try_push(value).unwrap();
            let bytes = roots.entries.borrow().physical_storage_for_test().1;
            let ledger = (*owner.as_ptr()).heap_identity.byte_ledger();
            assert!(bytes != 0 && ledger.allocated_bytes() >= bytes);
            ledger
        });
        drop(session);
        assert_eq!(ledger.allocated_bytes(), 0);
    }
}
#[test]
fn ordered_root_budget_legacy_infallible_entry_raises_recoverable_r_error() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let legacy = &(*owner.as_ptr()).legacy_protect;
        let value =
            RootValue::from_owner(owner.as_ptr(), crate::sexp::globals::R_NilValue()).unwrap();
        legacy.push(value.clone(), "quota control");
        let cap = legacy
            .roots_for_test()
            .entries
            .borrow()
            .physical_storage_for_test()
            .0;
        for _ in 1..cap {
            legacy.push(value.clone(), "quota control");
        }
        let before = (*owner.as_ptr())
            .heap_identity
            .byte_ledger()
            .allocated_bytes();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(before, 0)))
            .unwrap();
        let denied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            push_protect_in(owner.as_ptr(), crate::sexp::globals::R_NilValue())
        }));
        let error =
            denied.expect_err("legacy infallible contract must reject growth before publishing");
        assert_eq!(
            &error
                .downcast_ref::<crate::sexp::context::RError>()
                .expect("native root budget denial must use an R condition, not an internal panic")
                .message,
            "protect: protection storage allocation failed"
        );
        assert_eq!(legacy.len(), cap);
        legacy.pop_count(cap);
        assert_eq!(legacy.len(), 0);
    });
}
