use super::*;
use crate::sexp::ffi::SEXPTYPE;
use crate::sexp::instance::{RInstance, current_instance_ptr, replace_current_instance};
use crate::sexp::session::RSession;
use std::ptr;
use std::ptr::addr_of_mut;
// -- Legacy stack -------------------------------------------------------
#[test]
fn test_protect_unprotect() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| unsafe {
        let fake = token(0x1);
        let result = protect_raw_pointer(fake);
        assert_eq!(result, fake);
        assert_eq!(R_ProtectCount(), 1);
        unprotect_count(1);
        assert_eq!(R_ProtectCount(), 0);
    });
}
#[test]
fn test_protect_null() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| unsafe {
        protect_raw_pointer(ptr::null_mut());
        assert_eq!(R_ProtectCount(), 0);
    });
}
#[test]
fn test_protect_multiple() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| unsafe {
        let a = token(0x1);
        let b = token(0x2);
        let c = token(0x3);
        protect_raw_pointer(a);
        protect_raw_pointer(b);
        protect_raw_pointer(c);
        assert_eq!(R_ProtectCount(), 3);
        unprotect_count(2);
        assert_eq!(R_ProtectCount(), 1);
        unprotect_count(1);
        assert_eq!(R_ProtectCount(), 0);
    });
}
#[test]
fn test_unprotect_ptr() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| unsafe {
        let a = token(0x1);
        let b = token(0x2);
        protect_raw_pointer(a);
        protect_raw_pointer(b);
        assert_eq!(R_ProtectCount(), 2);
        unprotect_ptr(a);
        assert_eq!(R_ProtectCount(), 1);
        unprotect_ptr(b);
        assert_eq!(R_ProtectCount(), 0);
    });
}
#[test]
fn test_unprotect_ptr_null() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| unsafe {
        unprotect_ptr(ptr::null_mut());
        assert_eq!(R_ProtectCount(), 0);
    });
}
#[test]
fn test_unprotect_zero() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| {
        unprotect_count(0);
        assert_eq!(R_ProtectCount(), 0);
    });
}
#[test]
fn test_unprotect_exceeds_stack() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| unsafe {
        protect_raw_pointer(token(0x1));
        unprotect_count(5);
        assert_eq!(R_ProtectCount(), 0);
    });
}
#[test]
fn test_protect_n_guard() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| {
        let depth_before = R_ProtectCount();
        unsafe {
            protect_raw_pointer(token(0x1));
            protect_raw_pointer(token(0x2));
            protect_raw_pointer(token(0x3));
        }
        let _guard = protect_n(3);
        assert_eq!(R_ProtectCount(), depth_before + 3);
        drop(_guard);
        assert_eq!(R_ProtectCount(), depth_before);
    });
}
#[test]
fn test_protect_n_guard_drops_against_original_instance() {
    let mut left = RInstance::new_for_gc_tests();
    let mut right = RInstance::new_for_gc_tests();
    let previous = unsafe { replace_current_instance(Some(&mut left)) };
    unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        protect_raw_pointer(token(0x1))
    };
    unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        protect_raw_pointer(token(0x2))
    };
    let guard = protect_n(2);
    assert_eq!(R_ProtectCount_in(addr_of_mut!(left)), 2);
    unsafe {
        replace_current_instance(Some(&mut right));
    }
    drop(guard);
    assert_eq!(R_ProtectCount_in(addr_of_mut!(left)), 0);
    assert_eq!(R_ProtectCount_in(addr_of_mut!(right)), 0);
    unsafe {
        replace_current_instance(previous);
    }
}
#[test]
fn test_with_protected_objects() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| unsafe {
        protect_raw_pointer(token(0x1));
        protect_raw_pointer(token(0x2));
        with_protected_objects(|legacy, roots| {
            assert_eq!(legacy.len(), 2);
            assert!(roots.is_empty());
        });
        unprotect_count(2);
    });
}
#[test]
fn test_update_protect_stack_refs() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| unsafe {
        protect_raw_pointer(token(0x1));
        protect_raw_pointer(token(0x2));
        update_protect_stack_refs(|ptr| if ptr == token(0x1) { token(0x100) } else { ptr });
        with_protected_objects(|legacy, _| {
            assert_eq!(legacy[0], token(0x100));
            assert_eq!(legacy[1], token(0x2));
        });
        unprotect_count(2);
    });
}
// -- Root-table guards ---------------------------------------------------
#[test]
fn test_protect_sexp_guard() {
    let mut session = RSession::new_for_gc_tests();
    let value = session
        .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
        .expect("session should be active");
    let value = session.sexp(value).expect("value belongs to session");
    session.with_protected(|| {
        let (roots_before, legacy_before) =
            with_protected_objects(|legacy, roots| (roots.len(), legacy.len()));
        let guard = protect_sexp(value.clone());
        // Root-table guards are invisible to the count-based legacy API.
        assert_eq!(R_ProtectCount(), legacy_before);
        with_protected_objects(|legacy, roots| {
            assert_eq!(legacy.len(), legacy_before);
            assert_eq!(roots, &[value.clone().as_raw()]);
        });
        assert_eq!(roots_before, 0);
        drop(guard);
        with_protected_objects(|_, roots| assert_eq!(roots.len(), roots_before));
        assert_eq!(R_ProtectCount(), legacy_before);
    });
}
#[test]
fn test_rooted_sexp_roots_and_unroots() {
    let mut session = RSession::new_for_gc_tests();
    let value = session
        .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
        .expect("session should be active");
    let value = session.sexp(value).expect("value belongs to session");
    session.with_protected(|| {
        let root = RootedSexp::root(value.clone());
        with_protected_objects(|_, roots| assert_eq!(roots, &[value.clone().as_raw()]));
        let readback = root.get().expect("fresh root must resolve").clone();
        assert_eq!(readback, value);
        let sexp = root.unroot();
        assert_eq!(sexp.as_raw(), value.clone().as_raw());
        with_protected_objects(|_, roots| assert_eq!(roots.len(), 0));
    });
}
#[test]
fn test_rooted_sexp_reprotect_and_nested_lifo_drop() {
    let mut session = RSession::new_for_gc_tests();
    let (raw_first, raw_second) = session
        .with_arena(|arena| {
            (
                arena.alloc_node(SEXPTYPE::INTSXP),
                arena.alloc_node(SEXPTYPE::REALSXP),
            )
        })
        .expect("session should be active");
    let first = session.sexp(raw_first).expect("value belongs to session");
    let second = session.sexp(raw_second).expect("value belongs to session");
    session.with_protected(|| {
        let mut outer = RootedSexp::root(first.clone());
        {
            let inner = RootedSexp::root(first.clone());
            assert!(inner.slot().is_active());
        }
        // The tail drop collapses the inner slot off the table, so the
        // outer root is the only entry left.
        with_protected_objects(|_, roots| assert_eq!(roots.len(), 1));
        outer.reprotect(second.clone());
        with_protected_objects(|_, roots| assert_eq!(roots, &[raw_second]));
        assert_eq!(
            outer.get().expect("outer root must resolve").clone(),
            second
        );
        drop(outer);
        with_protected_objects(|_, roots| assert_eq!(roots.len(), 0));
    });
}
#[test]
fn test_raw_protect_guard_null_legacy() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| {
        let depth_before = R_ProtectCount();
        let guard = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect(ptr::null_mut()) };
        assert_eq!(R_ProtectCount(), depth_before);
        drop(guard);
        assert_eq!(R_ProtectCount(), depth_before);
        with_protected_objects(|_, roots| assert!(roots.is_empty()));
    });
}
#[test]
fn test_safe_protect_rejects_unknown_owner() {
    let mut session = RSession::new_for_gc_tests();
    let raw = session
        .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
        .expect("session should be active");
    let value = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(raw) }.expect("raw value should wrap as legacy boundary");
    session.with_protected(|| {
        assert!(matches!(
            try_protect_sexp(value.clone()),
            Err(ProtectError::UnownedHandle {
                api: "protect_sexp",
                owner: SexpOwner::Unknown,
            })
        ));
        assert!(matches!(
            try_preserve_sexp(value.clone()),
            Err(ProtectError::UnownedHandle {
                api: "preserve_sexp",
                owner: SexpOwner::Unknown,
            })
        ));
        assert!(matches!(
            try_protect_sexp_with_index(value),
            Err(ProtectError::UnownedHandle {
                api: "protect_sexp_with_index",
                owner: SexpOwner::Unknown,
            })
        ));
    });
}
#[test]
fn test_preserve_sexp_guard() {
    let mut session = RSession::new_for_gc_tests();
    let value = session
        .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
        .expect("session should be active");
    let value = session.sexp(value).expect("value belongs to session");
    session.with_protected(|| {
        let guard = preserve_sexp(value.clone());
        with_preserved_objects(|objects| assert_eq!(objects, &[value.as_raw()]));
        drop(guard);
        with_preserved_objects(|objects| assert!(objects.is_empty()));
    });
}
#[test]
fn test_indexed_protect_guard_reprotects_and_unwinds() {
    let mut session = RSession::new_for_gc_tests();
    let first = session
        .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
        .expect("session should be active");
    let second = session
        .with_arena(|arena| arena.alloc_node(SEXPTYPE::REALSXP))
        .expect("session should be active");
    let first = session.sexp(first).expect("first value belongs to session");
    let second = session
        .sexp(second)
        .expect("second value belongs to session");
    session.with_protected(|| {
        let mut guard = protect_sexp_with_index(first.clone());
        assert!(guard.slot().is_active());
        with_protected_objects(|_, roots| assert_eq!(roots.len(), 1));
        guard.reprotect_sexp(second.clone());
        with_protected_objects(|_, roots| assert_eq!(roots, &[second.clone().as_raw()]));
        drop(guard);
        with_protected_objects(|_, roots| assert_eq!(roots.len(), 0));
    });
}
#[test]
fn test_indexed_raw_guard_null_is_inactive() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| {
        let guard = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect_with_index_raw(ptr::null_mut(), "test") };
        assert!(!guard.slot().is_active());
        drop(guard);
        with_protected_objects(|_, roots| assert!(roots.is_empty()));
    });
}
#[test]
fn test_protect_guard_drops_against_original_instance() {
    let mut left = RInstance::new_for_gc_tests();
    let mut right = RInstance::new_for_gc_tests();
    let previous = unsafe { replace_current_instance(Some(&mut left)) };
    let left_ptr = current_instance_ptr().expect("left should be installed");
    let guard = unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        protect(token(0x1))
    };
    // As above: inspect through the installed pointer, never a fresh
    // borrow of the local, while the guard is live.
    with_protected_objects_in(left_ptr, |legacy, roots| {
        assert!(legacy.is_empty());
        assert_eq!(roots, &[token(0x1)]);
    });
    with_protected_objects_in(addr_of_mut!(right), |legacy, roots| {
        assert!(legacy.is_empty());
        assert!(roots.is_empty());
    });
    unsafe {
        replace_current_instance(Some(&mut right));
    }
    drop(guard);
    with_protected_objects_in(left_ptr, |_, roots| {
        // The root either collapsed (tail release) or tombstoned.
        assert!(roots.iter().all(|&p| p.is_null()));
    });
    with_protected_objects_in(addr_of_mut!(right), |_, roots| assert!(roots.is_empty()));
    unsafe {
        replace_current_instance(previous);
    }
}
#[test]
fn test_indexed_guard_drops_against_original_instance() {
    let mut left = RInstance::new_for_gc_tests();
    let mut right = RInstance::new_for_gc_tests();
    let previous = unsafe { replace_current_instance(Some(&mut left)) };
    // Mid-guard inspection goes through the pointer recorded at install
    // time: a fresh `&mut left`/`&left` (or reference derivation from a
    // fresh raw pointer) would retag the allocation and pop the guard
    // owner's exposed tag out from under the wildcard release path
    // (aliasing UB under Stacked Borrows).
    let left_ptr = current_instance_ptr().expect("left should be installed");
    let mut guard = unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        protect_with_index_raw(token(0x1), "test")
    };
    with_protected_objects_in(left_ptr, |legacy, roots| {
        assert!(legacy.is_empty());
        assert_eq!(roots, &[token(0x1)]);
    });
    unsafe {
        replace_current_instance(Some(&mut right));
    }
    (unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        guard.reprotect_raw(token(0x2))
    });
    with_protected_objects_in(left_ptr, |legacy, roots| {
        assert!(legacy.is_empty());
        assert_eq!(roots, &[token(0x2)])
    });
    with_protected_objects_in(addr_of_mut!(right), |legacy, roots| {
        assert!(legacy.is_empty());
        assert!(roots.is_empty());
    });
    drop(guard);
    with_protected_objects_in(left_ptr, |_, roots| {
        assert!(roots.iter().all(|&p| p.is_null()))
    });
    with_protected_objects_in(addr_of_mut!(right), |_, roots| assert!(roots.is_empty()));
    unsafe {
        replace_current_instance(previous);
    }
}
#[test]
fn test_protect_with_index() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| unsafe {
        let fake = token(0x1);
        let idx = R_ProtectWithIndex(fake);
        assert!(!idx.is_null());
        // The shim claims a generational root-table slot: the legacy
        // count-based view does not see it.
        assert_eq!(R_ProtectCount(), 0);
        with_protected_objects(|legacy, roots| {
            assert!(legacy.is_empty());
            assert_eq!(roots, &[fake]);
        });
    });
}
#[test]
fn test_protect_with_index_null() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| unsafe {
        let idx = R_ProtectWithIndex(ptr::null_mut());
        assert!((idx as usize) == 0);
        assert_eq!(R_ProtectCount(), 0);
        with_protected_objects(|_, roots| assert!(roots.is_empty()));
    });
}
#[test]
fn test_reprotect() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| unsafe {
        let a = token(0x1);
        let b = token(0x2);
        let idx = R_ProtectWithIndex(a);
        R_Reprotect(b, idx);
        with_protected_objects(|_, roots| assert_eq!(roots[0], b));
    });
}
#[test]
fn test_reprotect_null_index() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| unsafe {
        R_Reprotect(token(0x1), ptr::null_mut());
    });
}
#[test]
fn test_free_protect_index() {
    unsafe {
        R_FreeProtectIndex(ptr::null_mut());
        R_FreeProtectIndex(0x1 as *mut ProtectIndex);
    }
}
// -- Preserve / release --------------------------------------------------
#[test]
fn test_preserve_release() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| unsafe {
        let fake = token(0x1);
        R_PreserveObject(fake);
        with_preserved_objects(|objects| assert_eq!(objects.len(), 1));
        R_ReleaseObject(fake);
        with_preserved_objects(|objects| assert_eq!(objects.len(), 0));
    });
}
#[test]
fn test_preserve_null() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| unsafe {
        R_PreserveObject(ptr::null_mut());
        with_preserved_objects(|objects| assert_eq!(objects.len(), 0));
    });
}
#[test]
fn test_release_null() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| unsafe {
        R_ReleaseObject(ptr::null_mut());
    });
}
#[test]
fn test_preserve_guard_drops_against_original_instance() {
    let mut left = RInstance::new_for_gc_tests();
    let mut right = RInstance::new_for_gc_tests();
    let previous = unsafe { replace_current_instance(Some(&mut left)) };
    // Direct access to the installed instance goes through the pointer
    // recorded at install time: a fresh `&mut left` would retag the
    // allocation and pop the installed borrow tag out from under the
    // ambient re-acquisition `preserve_sexp` performs (Stacked Borrows).
    let left_ptr = current_instance_ptr().expect("left should be installed");
    let raw = unsafe { (*left_ptr).arena.alloc_node(SEXPTYPE::INTSXP) };
    let value = unsafe { crate::sexp::owner::OwnerToken::from_raw(left_ptr) }
        .sexp(raw)
        .expect("left object should wrap");
    let guard = preserve_sexp(value);
    with_preserved_objects_in(unsafe { &mut *left_ptr }, |objects| {
        assert_eq!(objects, &[raw])
    });
    with_preserved_objects_in(addr_of_mut!(right), |objects| assert!(objects.is_empty()));
    unsafe {
        replace_current_instance(Some(&mut right));
    }
    drop(guard);
    with_preserved_objects_in(unsafe { &mut *left_ptr }, |objects| {
        assert!(objects.is_empty())
    });
    with_preserved_objects_in(addr_of_mut!(right), |objects| assert!(objects.is_empty()));
    unsafe {
        replace_current_instance(previous);
    }
}
// -- Slot generations -----------------------------------------------------
#[test]
fn test_slot_generations_differ_across_release_and_reuse() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| {
        let first = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect_with_index_raw(token(0x1), "test") };
        let first_slot = first.slot();
        assert!(first_slot.is_active());
        drop(first);
        // The same index is handed out again with a fresh generation.
        let second = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect_with_index_raw(token(0x2), "test") };
        let second_slot = second.slot();
        assert_ne!(first_slot.generation(), second_slot.generation());
        // The live handle still matches its own generation; the released
        // handle resolves to a different generation at its index.
        assert!(!second_slot.is_stale());
        assert!(first_slot.is_stale());
        drop(second);
    });
}
#[test]
fn test_rooted_sexp_generation_survives_full_gc() {
    let mut session = RSession::new_for_gc_tests();
    let value = session
        .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
        .expect("session should be active");
    let value = session.sexp(value).expect("value belongs to session");
    session.with_protected(|| {
        let root = RootedSexp::root(value.clone());
        let generation = root.slot().generation();
        assert!(!root.is_stale());
        // The root pins the value in the root table, so collection
        // must leave the value and the slot's generation intact.
        crate::sexp::gengc::full_gc();
        assert!(!root.is_stale());
        assert_eq!(root.slot().generation(), generation);
        let readback = root.get().expect("rooted value must resolve after gc");
        assert_eq!(readback.clone().as_raw(), value.clone().as_raw());
        with_protected_objects(|_, roots| assert_eq!(roots, &[value.clone().as_raw()]));
    });
}
#[test]
fn test_released_slot_reuse_reports_stale_generation() {
    let mut session = RSession::new_for_gc_tests();
    let raw = session
        .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
        .expect("session should be active");
    let value = session.sexp(raw).expect("value belongs to session");
    session.with_protected(|| {
        let root = RootedSexp::root(value.clone());
        let slot = root.slot();
        assert!(slot.is_active());
        assert!(!slot.is_stale());
        let generation = slot.generation();
        // Release the slot, then allocate and churn roots so its index
        // is handed out again.
        let sexp = root.unroot();
        assert_eq!(sexp.as_raw(), value.clone().as_raw());
        with_protected_objects(|_, roots| assert_eq!(roots.len(), 0));
        crate::sexp::instance::with_required_current_instance(|inst| unsafe {
            for _ in 0..1000 {
                (*inst).arena.alloc_node(SEXPTYPE::INTSXP);
            }
        });
        for _ in 0..1000 {
            let churn = RootedSexp::root(value.clone());
            assert_ne!(churn.slot().generation(), generation);
            drop(churn);
        }
        // The old handle's slot was released and its index handed out
        // again: the entry living there (if any) carries a different
        // generation, so the handle reports stale.
        assert!(slot.is_stale());
    });
}
#[test]
fn test_arbitrary_drop_order_keeps_surviving_roots_live() {
    let mut session = RSession::new_for_gc_tests();
    let value = session
        .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
        .expect("session should be active");
    let value = session.sexp(value).expect("value belongs to session");
    session.with_protected(|| {
        let first = RootedSexp::root(value.clone());
        let second = RootedSexp::root(value.clone());
        let third = RootedSexp::root(value.clone());
        with_protected_objects(|_, roots| assert_eq!(roots.len(), 3));
        assert!(!first.is_stale());
        assert!(!second.is_stale());
        assert!(!third.is_stale());
        // Drop out of order: the interior tombstone leaves the survivors'
        // indices untouched, so they keep resolving to their own entries.
        drop(first);
        assert!(second.get().is_some());
        assert!(third.get().is_some());
        assert!(!second.is_stale());
        assert!(!third.is_stale());
        with_protected_objects(|_, roots| assert_eq!(roots.len(), 3));
        // The freed index is reusable: a fresh root lands exactly there
        // with a newer generation while the survivors stay healthy.
        let fresh = RootedSexp::root(value.clone());
        assert!(!fresh.is_stale());
        assert!(fresh.get().is_some());
        assert!(!second.is_stale());
        assert!(!third.is_stale());
        with_protected_objects(|_, roots| assert_eq!(roots.len(), 3));
        drop(fresh);
        drop(third);
        drop(second);
        // All slots released: the tail collapse pops every entry, so the
        // live-entry depth is restored exactly.
        with_protected_objects(|_, roots| assert_eq!(roots.len(), 0));
    });
}
#[test]
fn test_shuffled_roots_survive_vec_drop_order() {
    let mut session = RSession::new_for_gc_tests();
    let value = session
        .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
        .expect("session should be active");
    let value = session.sexp(value).expect("value belongs to session");
    session.with_protected(|| {
        let mut roots: Vec<RootedSexp<'_>> =
            (0..16).map(|_| RootedSexp::root(value.clone())).collect();
        with_protected_objects(|_, table| assert_eq!(table.len(), 16));
        // Deterministic bit-reversal permutation: exercises a genuinely
        // shuffled drop order without pulling in an RNG dependency.
        let mut permuted: Vec<RootedSexp<'_>> = Vec::with_capacity(roots.len());
        while !roots.is_empty() {
            let mid = roots.len() / 2;
            permuted.push(roots.remove(mid));
        }
        let mut roots = permuted;
        // Drop half in shuffled order: every survivor still reads.
        for _ in 0..8 {
            roots.pop();
        }
        // Drain the rest in the shuffled order too: every release either
        // reuses, tombstones, or collapses, so the depth is restored.
        while roots.pop().is_some() {}
        with_protected_objects(|_, table| assert_eq!(table.len(), 0));
    });
}
#[test]
fn test_slot_reuse_rejects_old_token() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| {
        let first = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect_with_index_raw(token(0x1), "test") };
        let stale_token = first.slot();
        assert!(stale_token.is_active());
        assert!(!stale_token.is_stale());
        drop(first);
        assert!(stale_token.is_stale());
        // The freed index is handed out again with a fresh generation.
        let second = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect_with_index_raw(token(0x2), "test") };
        let live_token = second.slot();
        assert!(live_token.is_active());
        assert_ne!(stale_token.generation(), live_token.generation());
        assert!(!live_token.is_stale());
        // A double release against the stale token cannot evict the live
        // owner: generations disagree, so the tombstone is untouched.
        release_protect_slot(stale_token);
        assert!(!live_token.is_stale());
        assert!(!second.slot().is_stale());
        assert_eq!(second.slot().generation(), live_token.generation());
        drop(second);
        with_protected_objects(|_, roots| assert!(roots.is_empty()));
    });
}
// -- Split-storage invariants (the two disciplines never alias) ----------
#[test]
fn test_legacy_unprotect_never_truncates_root_slots() {
    let mut session = RSession::new_for_gc_tests();
    let value = session
        .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
        .expect("session should be active");
    let value = session.sexp(value).expect("value belongs to session");
    session.with_protected(|| {
        let first = RootedSexp::root(value.clone());
        let second = RootedSexp::root(value.clone());
        // Legacy pushes and count-based pops interleave with live roots.
        unsafe {
            protect_raw_pointer(token(0xA));
            protect_raw_pointer(token(0xB));
        }
        assert_eq!(R_ProtectCount(), 2);
        unprotect_count(2);
        assert_eq!(R_ProtectCount(), 0);
        // The roots survived the count-based truncation untouched.
        assert!(!first.is_stale());
        assert!(!second.is_stale());
        assert!(first.get().is_some());
        assert!(second.get().is_some());
        with_protected_objects(|legacy, roots| {
            assert!(legacy.is_empty());
            assert_eq!(roots.len(), 2);
        });
        drop(second);
        assert!(!first.is_stale());
        drop(first);
        with_protected_objects(|_, roots| assert_eq!(roots.len(), 0));
    });
}
#[test]
fn test_root_release_never_shifts_legacy_entries() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| {
        unsafe {
            protect_raw_pointer(token(0x1));
            protect_raw_pointer(token(0x2));
            protect_raw_pointer(token(0x3));
        }
        assert_eq!(R_ProtectCount(), 3);
        // Claim and release roots (in any order) around the live legacy
        // entries: the legacy stack must not shift.
        let a = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect(token(0xA1)) };
        let b = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect_with_index_raw(token(0xA2), "test") };
        drop(a);
        with_protected_objects(|legacy, _| {
            assert_eq!(legacy, &[token(0x1), token(0x2), token(0x3)]);
        });
        drop(b);
        assert_eq!(R_ProtectCount(), 3);
        with_protected_objects(|legacy, roots| {
            assert_eq!(legacy, &[token(0x1), token(0x2), token(0x3)]);
            assert!(roots.iter().all(|&p| p.is_null()) || roots.is_empty());
        });
        // Legacy LIFO unwinding still works after the root churn.
        unprotect_count(3);
        assert_eq!(R_ProtectCount(), 0);
    });
}
#[test]
fn test_nested_sessions_with_interleaved_roots() {
    // Two live sessions on one thread, each with roots claimed while it
    // was ambient. Re-activation goes through each session's STABLE
    // instance pointer (never a fresh `&mut`), so guard owner
    // exposures stay valid while the ambient instance switches — the
    // documented owner discipline (see `with_guard_owner`).
    let left = RSession::new_for_gc_tests();
    let right = RSession::new_for_gc_tests();
    let left_root = left.with_active(|| RootedSexp::root(left.global_env().unwrap()));
    let left_guard = left.with_active(|| unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect(token(0x10)) });
    let right_guard = right.with_active(|| unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect(token(0x20)) });
    left.with_active(|| {
        with_protected_objects(|legacy, roots| {
            assert!(legacy.is_empty());
            assert_eq!(roots.len(), 2); // explicit root + left_guard
        })
    });
    right.with_active(|| {
        with_protected_objects(|legacy, roots| {
            assert!(legacy.is_empty());
            assert_eq!(roots, &[token(0x20)]); // right_guard only
        })
    });
    // Interleaved, out-of-order drops while the OTHER session is
    // ambient: each guard releases against its own instance and slot.
    drop(left_guard);
    left.with_active(|| assert!(!left_root.is_stale()));
    drop(right_guard);
    left.with_active(|| assert!(!left_root.is_stale()));
    drop(left_root);
    left.with_active(|| with_protected_objects(|_, roots| assert!(roots.is_empty())));
    right.with_active(|| with_protected_objects(|_, roots| assert!(roots.is_empty())));
}
#[test]
fn test_update_protect_stack_refs_covers_both_storages() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| unsafe {
        protect_raw_pointer(token(0x1));
        let _root = protect_with_index_raw(token(0x2), "test");
        update_protect_stack_refs(|ptr| {
            if ptr == token(0x1) || ptr == token(0x2) {
                token(0x100)
            } else {
                ptr
            }
        });
        with_protected_objects(|legacy, roots| {
            assert_eq!(legacy[0], token(0x100));
            assert_eq!(roots[0], token(0x100));
        });
        unprotect_count(1);
    });
}
#[test]
fn test_update_preserve_stack_refs() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| unsafe {
        R_PreserveObject(token(0x1));
        update_preserve_stack_refs(|ptr| token(0x200));
        with_preserved_objects(|objects| {
            assert_eq!(objects[0], token(0x200));
        });
        R_ReleaseObject(token(0x200));
    });
}
#[test]
fn test_with_preserved_objects() {
    let session = RSession::new_for_gc_tests();
    session.with_protected(|| unsafe {
        R_PreserveObject(token(0x1));
        R_PreserveObject(token(0x2));
        with_preserved_objects(|objects| {
            assert_eq!(objects.len(), 2);
        });
        R_ReleaseObject(token(0x1));
        R_ReleaseObject(token(0x2));
    });
}
#[test]
fn safe_roots_use_value_owner_not_ambient_session() {
    let left = RSession::new_for_gc_tests();
    let right = RSession::new_for_gc_tests();
    let value = left.global_env().unwrap();
    let mut root = protect_sexp_with_index(value.clone());
    left.with_active(|| {
        with_protected_objects(|_, roots| assert_eq!(roots, &[value.clone().as_raw()]))
    });
    right.with_active(|| with_protected_objects(|_, roots| assert!(roots.is_empty())));
    assert_eq!(
        root.try_reprotect_sexp(right.global_env().unwrap()),
        Err(ProtectError::ForeignOwner)
    );
}
#[test]
fn stale_reprotect_cannot_replace_reused_slot() {
    let session = RSession::new_for_gc_tests();
    let mut old = protect_sexp_with_index(session.global_env().unwrap());
    release_protect_slot(old.slot());
    let live = protect_sexp_with_index(session.base_env().unwrap());
    assert_eq!(
        old.try_reprotect_sexp(session.global_env().unwrap()),
        Err(ProtectError::StaleSlot)
    );
    reprotect_slot(old.slot(), session.global_env().unwrap().as_raw());
    let base = session.base_env().unwrap().as_raw();
    with_protected_objects(|_, roots| {
        assert_eq!(
            roots
                .iter()
                .copied()
                .filter(|p| !p.is_null())
                .collect::<Vec<_>>(),
            vec![base]
        );
    });
    drop(old);
    assert!(!live.slot().is_stale());
}
#[test]
fn public_scope_does_not_revoke_returned_root() {
    let session = RSession::new_for_gc_tests();
    let root = session.with_protected(|| RootedSexp::root(session.global_env().unwrap()));
    session.gc();
    assert!(!root.is_stale());
    assert!(root.get().unwrap().is_environment());
}
// These are actual process-owned native objects, not fabricated addresses.
fn token(n: usize) -> SEXP {
    unsafe {
        match n % 7 {
            0 => super::super::globals::R_NilValue(),
            1 => super::super::globals::R_UnboundValue(),
            2 => super::super::globals::R_MissingArg(),
            3 => super::super::globals::R_RestartToken(),
            4 => super::super::globals::R_NaString(),
            5 => super::super::globals::R_True(),
            _ => super::super::globals::R_False(),
        }
    }
}
#[test]
fn stale_allocation_root_cannot_pin_reused_header_address() {
    use super::super::heap::{HeapIdentity, NodePage};
    let page = NodePage::try_new(HeapIdentity::new(), 0, 1, || {
        super::super::ffi::SexprecCore::new(SEXPTYPE::INTSXP)
    })
    .unwrap();
    let metadata = page.metadata();
    metadata.activate(0, false).unwrap();
    let _registration = super::super::memory::register_node_page(&page);
    let projection = page.raw_slot(0).unwrap();
    let allocation = page.token(0).unwrap();
    let old_value = RootValue::Checked {
        projection,
        allocation: allocation.clone(),
    };
    let table = RootTable::new(crate::sexp::memory::MemoryLedger::new());
    let old_slot = table.try_claim(old_value.clone(), true).unwrap();
    assert!(metadata.release(allocation.id()));
    page.replace_inactive(0, super::super::ffi::SexprecCore::new(SEXPTYPE::REALSXP))
        .unwrap();
    metadata.activate(0, false).unwrap();
    let replacement = page.token(0).unwrap();
    assert_ne!(replacement, allocation);
    assert_eq!(page.raw_slot(0), Some(projection));
    assert!(table.checked_entries_snapshot().is_empty());
    assert_eq!(table.entries_snapshot(), vec![ptr::null_mut()]);
    assert_eq!(table.generation_at(old_slot), None);
    assert_eq!(
        table.try_claim(old_value, true),
        Err(ProtectError::StaleAllocation)
    );
    table.release(old_slot);
    assert_eq!(table.len(), 0);
}
#[test]
fn stale_native_roots_cannot_pin_a_reused_header_address() {
    use super::super::heap::{HeapIdentity, NodePage};
    let page = NodePage::try_new(HeapIdentity::new(), 0, 1, || {
        super::super::ffi::SexprecCore::new(SEXPTYPE::INTSXP)
    })
    .unwrap();
    let metadata = page.metadata();
    metadata.activate(0, false).unwrap();
    let _registration = super::super::memory::register_node_page(&page);
    let projection = page.raw_slot(0).unwrap();
    let allocation = page.token(0).unwrap();
    let value = RootValue::Checked {
        projection,
        allocation: allocation.clone(),
    };
    let legacy = LegacyProtectionStack::new(crate::sexp::memory::MemoryLedger::new());
    let preserved = PreservedRoots::new(crate::sexp::memory::MemoryLedger::new());
    legacy.push(value.clone(), "fixture");
    let lease = preserved.try_push(value.clone()).unwrap();
    assert!(metadata.release(allocation.id()));
    page.replace_inactive(0, super::super::ffi::SexprecCore::new(SEXPTYPE::REALSXP))
        .unwrap();
    metadata.activate(0, false).unwrap();
    assert_eq!(page.raw_slot(0), Some(projection));
    assert_ne!(page.token(0).unwrap(), allocation);
    assert!(legacy.checked_entries_snapshot().is_empty());
    assert!(preserved.checked_entries_snapshot().is_empty());
    assert_eq!(legacy.entries_snapshot(), [ptr::null_mut()]);
    assert_eq!(preserved.entries_snapshot(), [ptr::null_mut()]);
    assert_eq!(
        preserved.try_push(value),
        Err(ProtectError::StaleAllocation)
    );
    legacy.pop_count(1);
    preserved.remove_lease(lease);
    assert_eq!(legacy.len(), 0);
    assert!(preserved.is_empty());
}
#[test]
fn native_root_updates_do_not_overwrite_same_value_reentry() {
    let session = RSession::new_for_gc_tests();
    unsafe {
        protect_raw_pointer(token(1));
    }
    update_protect_stack_refs(|_| {
        unsafe {
            unprotect_count(1);
            protect_raw_pointer(token(1));
        }
        token(2)
    });
    with_protected_objects(|legacy, roots| {
        assert_eq!(legacy, &[token(1)]);
        assert!(roots.is_empty());
    });
    unsafe {
        unprotect_count(1);
        R_PreserveObject(token(1));
    }
    update_preserve_stack_refs(|_| {
        unsafe {
            R_ReleaseObject(token(1));
            R_PreserveObject(token(1));
        }
        token(2)
    });
    with_preserved_objects(|roots| assert_eq!(roots, &[token(1)]));
    unsafe {
        R_ReleaseObject(token(1));
    }
    drop(session);
}
#[test]
fn preserved_guards_release_their_own_duplicate_lease() {
    let session = RSession::new_for_gc_tests();
    let value = session.global_env().unwrap();
    let first = preserve_sexp(value.clone());
    let second = preserve_sexp(value.clone());
    unsafe {
        R_ReleaseObject(value.clone().as_raw());
    }
    drop(first);
    with_preserved_objects(|roots| assert_eq!(roots, &[value.clone().as_raw()]));
    drop(second);
    with_preserved_objects(|roots| assert!(roots.is_empty()));
}
#[test]
fn exhausted_safe_preserve_leaves_roots_and_cleanup_intact() {
    let session = RSession::new_for_gc_tests();
    let value = session.global_env().unwrap();
    let guard = preserve_sexp(value.clone());
    let pointer = session.owner_token().unwrap().as_ptr();
    let before = unsafe { (*pointer).preserve_stack.entries_snapshot() };
    unsafe {
        (*pointer)
            .preserve_stack
            .roots
            .entries
            .borrow_mut()
            .set_next_lease_for_test(u64::MAX);
    }
    assert!(matches!(
        try_preserve_sexp(value),
        Err(ProtectError::GenerationExhausted)
    ));
    assert_eq!(
        unsafe { (*pointer).preserve_stack.entries_snapshot() },
        before
    );
    drop(guard);
    with_preserved_objects(|roots| assert!(roots.is_empty()));
}
#[test]
fn root_claim_recovers_owned_provenance_from_address_only_input() {
    use super::super::heap::{HeapIdentity, NodePage};
    let page = NodePage::try_new(HeapIdentity::new(), 0, 1, || {
        super::super::ffi::SexprecCore::new(SEXPTYPE::INTSXP)
    })
    .unwrap();
    page.metadata().activate(0, false).unwrap();
    let _registration = super::super::memory::register_node_page(&page);
    let pointer = page.raw_slot(0).unwrap();
    let table = RootTable::new(crate::sexp::memory::MemoryLedger::new());
    let slot = table
        .try_claim(
            RootValue::Checked {
                projection: ptr::without_provenance_mut(pointer.addr()),
                allocation: page.token(0).unwrap(),
            },
            true,
        )
        .unwrap();
    let recovered = table.entries_snapshot()[0];
    // Miri verifies that the stored projection came from the actual cell,
    // not from the matching integer address supplied at the native seam.
    assert_eq!(
        unsafe { super::super::accessors::TYPEOF(recovered) },
        SEXPTYPE::INTSXP.as_c_int()
    );
    table.release(slot);
}
#[test]
fn protection_updates_allow_reentry_without_overwriting_recycled_root() {
    let session = RSession::new_for_gc_tests();
    let mut old = Some(unsafe { protect_with_index_raw(token(1), "fixture") });
    let old_slot = old.as_ref().unwrap().slot();
    let mut new = None;
    update_protect_stack_refs(|_| {
        drop(old.take());
        new = Some(unsafe { protect_with_index_raw(token(2), "fixture") });
        token(3)
    });
    let new = new.unwrap();
    assert_ne!(new.slot().generation(), old_slot.generation());
    with_protected_objects(|_, roots| assert_eq!(roots, &[token(2)]));
    drop(new);
    drop(session);
}
#[test]
fn safe_gc_owner_teardown_skips_native_guard_cleanup_even_on_unwind() {
    thread_local! {
        static OWNER: RefCell<Option<RSession>> = const { RefCell::new(None) };
    }
    for inject_panic in [false, true] {
        let session = RSession::new_for_gc_tests();
        let observer = unsafe {
            super::super::instance::instance_liveness(session.owner_token().unwrap().as_ptr())
        };
        let simple = unsafe { protect(token(1)) };
        let indexed = unsafe { protect_with_index_raw(token(2), "fixture") };
        unsafe {
            protect_raw_pointer(token(3));
        }
        let counted = protect_n(1);
        let preserve_lease =
            try_push_preserve_in(session.owner_token().unwrap().as_ptr(), token(4)).unwrap();
        let preserved = PreserveGuard {
            owner: Some(unsafe { GuardOwner::new(session.owner_token().unwrap().as_ptr()) }),
            lease: preserve_lease,
            _confined: PhantomData,
        };
        super::super::gengc::register_gc_callback(Box::new(move |_| {
            drop(OWNER.with(|owner| owner.borrow_mut().take()));
            if inject_panic {
                panic!("injected owner teardown panic");
            }
        }));
        OWNER.with(|owner| assert!(owner.borrow_mut().replace(session).is_none()));
        let result = std::panic::catch_unwind(super::super::gengc::full_gc);
        assert_eq!(result.is_err(), inject_panic);
        assert!(!observer.is_live());
        assert!(!indexed.slot_generation_is(indexed.slot().generation()));
        // Every guard must skip the destroyed allocation, including unwind
        // cleanup; a fresh owner cannot inherit or satisfy its weak token.
        let fresh = RSession::new_for_gc_tests();
        drop((simple, indexed, counted, preserved));
        with_protected_objects(|legacy, roots| assert!(legacy.is_empty() && roots.is_empty()));
        with_preserved_objects(|roots| assert!(roots.is_empty()));
        drop(fresh);
    }
}
#[test]
fn projection_update_callback_may_destroy_owner() {
    let session = RSession::new_for_gc_tests();
    let pointer = session.owner_token().unwrap().as_ptr();
    let guard = unsafe { protect(token(1)) };
    let mut owner = Some(session);
    update_protect_stack_refs_in(pointer, |_| {
        drop(owner.take());
        token(2)
    });
    drop(guard);
    assert!(super::super::instance::current_instance_ptr().is_none());
}
#[test]
fn exhausted_safe_claim_returns_error_without_mutating_existing_roots() {
    let session = RSession::new_for_gc_tests();
    let value = session.global_env().unwrap();
    let pin = value.pin_runtime().unwrap().unwrap();
    let owner = pin.as_ptr();
    let before = unsafe { (*owner).root_table.entries_snapshot() };
    unsafe {
        (*owner).root_table.set_next_generation_for_test(u64::MAX);
    }
    assert!(matches!(
        try_protect_sexp(value.clone()),
        Err(ProtectError::GenerationExhausted)
    ));
    assert!(matches!(
        try_protect_sexp_with_index(value.clone()),
        Err(ProtectError::GenerationExhausted)
    ));
    assert!(matches!(
        RootedSexp::try_root(value.clone()),
        Err(ProtectError::GenerationExhausted)
    ));
    assert_eq!(unsafe { (*owner).root_table.entries_snapshot() }, before);
    drop(value); // Even at generation exhaustion, root cleanup cannot panic.
    assert_eq!(unsafe { (*owner).root_table.len() }, 0);
}

#[test]
fn root_backing_retained_capacity_is_charged_to_original_interpreter() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let table = &(*owner.as_ptr()).root_table;
        let (_, old_bytes) = table.physical_storage_for_test();
        let before = (*owner.as_ptr())
            .heap_identity
            .byte_ledger()
            .allocated_bytes();
        let value =
            RootValue::from_owner(owner.as_ptr(), crate::sexp::globals::R_NilValue()).unwrap();
        let root = table.try_claim(value, true).unwrap();
        let (_, bytes) = table.physical_storage_for_test();
        let after = (*owner.as_ptr())
            .heap_identity
            .byte_ledger()
            .allocated_bytes();
        assert!(bytes > old_bytes);
        assert_eq!(
            after - before,
            bytes - old_bytes,
            "physical root entries and cleanup capacity must remain charged"
        );
        table.release(root);
        assert_eq!(table.physical_storage_for_test().1, bytes);
        assert_eq!(
            (*owner.as_ptr())
                .heap_identity
                .byte_ledger()
                .allocated_bytes(),
            after
        );
    });
}

fn root_backing_denied_growth(delta_only: bool) {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let value =
            RootValue::from_owner(owner.as_ptr(), crate::sexp::globals::R_NilValue()).unwrap();
        let table = &(*owner.as_ptr()).root_table;
        let first = table.try_claim(value.clone(), true).unwrap();
        let capacity = table.physical_storage_for_test().0;
        for _ in table.len()..capacity {
            table.try_claim(value.clone(), true).unwrap();
        }
        let checkpoint = table.checkpoint();
        let len = table.len();
        let bytes = table.physical_storage_for_test().1;
        let before = (*owner.as_ptr())
            .heap_identity
            .byte_ledger()
            .allocated_bytes();
        let headroom = if delta_only { bytes / capacity } else { 0 };
        owner
            .with_arena(|a| {
                a.set_budget(crate::sexp::memory::ArenaBudget::new(before + headroom, 0))
            })
            .unwrap();
        assert_eq!(
            table.try_claim(value.clone(), true),
            Err(ProtectError::Allocation)
        );
        assert_eq!((table.len(), table.checkpoint()), (len, checkpoint));
        assert_eq!(table.physical_storage_for_test().1, bytes);
        assert_eq!(table.generation_at(first), Some(first.generation));
        table.release(first);
        let replacement = table.try_claim(value, true).unwrap();
        assert_eq!(replacement.index, first.index);
        assert_ne!(replacement.generation, first.generation);
        table.release(first);
        assert_eq!(
            table.generation_at(replacement),
            Some(replacement.generation)
        );
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
        table.clear();
    });
}

#[test]
fn root_backing_growth_denial_is_atomic_and_cleanup_needs_no_headroom() {
    root_backing_denied_growth(false);
}

#[test]
fn root_backing_growth_requires_full_replacement_not_only_capacity_delta() {
    root_backing_denied_growth(true);
}

#[test]
fn root_backing_charge_releases_with_the_final_interpreter() {
    let session = RSession::new_for_gc_tests();
    let ledger = session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let value =
            RootValue::from_owner(owner.as_ptr(), crate::sexp::globals::R_NilValue()).unwrap();
        (*owner.as_ptr()).root_table.try_claim(value, true).unwrap();
        let ledger = (*owner.as_ptr()).heap_identity.byte_ledger();
        assert!(
            ledger.allocated_bytes() >= (*owner.as_ptr()).root_table.physical_storage_for_test().1
        );
        ledger
    });
    drop(session);
    assert_eq!(ledger.allocated_bytes(), 0);
}
