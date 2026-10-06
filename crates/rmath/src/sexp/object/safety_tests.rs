use super::{Sexp, SexpError, SexpMut, SexpValue, pairlist::PairlistBuilder};
use crate::sexp::{ffi::SEXPTYPE, memory::RArena, session::RSession};

fn identity_hash(value: &Sexp<'_>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn alloc(session: &RSession, kind: SEXPTYPE, len: i32) -> crate::sexp::ffi::SEXP {
    session.with_active_in(|owner| unsafe {
        crate::sexp::memory::with_arena_in(owner, |arena| arena.alloc_vector(kind, len.into()))
    })
}

fn full_gc(session: &RSession) {
    session.with_active_in(|_| {
        session.owner_token().unwrap().full_gc().unwrap();
    });
}

fn roots(session: &RSession) -> usize {
    session.with_active_in(|owner| unsafe {
        crate::sexp::memory::automatic_roots(&(*owner).heap_identity).len()
    })
}

#[test]
fn automatic_handles_trace_cycles_and_last_drop_releases_them() {
    let session = RSession::new_for_gc_tests();
    let first_ptr = alloc(&session, SEXPTYPE::VECSXP, 1);
    let first = session.sexp(first_ptr).unwrap();
    let second_ptr = alloc(&session, SEXPTYPE::VECSXP, 1);
    let second = session.sexp(second_ptr).unwrap();
    SexpMut::try_from_checked(first.clone())
        .unwrap()
        .try_set_vector_elt(0, second.clone())
        .unwrap();
    SexpMut::try_from_checked(second.clone())
        .unwrap()
        .try_set_vector_elt(0, first.clone())
        .unwrap();
    drop(second);
    full_gc(&session);
    let child = first.vector_elt(0).unwrap();
    assert_eq!(child.vector_elt(0).unwrap(), first);
    drop(child);
    drop(first);
    full_gc(&session);
    assert!(session.sexp(first_ptr).is_none());
    assert!(session.sexp(second_ptr).is_none());
}

#[test]
fn checked_clone_retains_one_root_until_last_drop() {
    let session = RSession::new_for_gc_tests();
    let before = roots(&session);
    let ptr = alloc(&session, SEXPTYPE::INTSXP, 1);
    let value = session.sexp(ptr).unwrap();
    let clone = value.clone();
    assert_eq!(roots(&session), before + 1);
    SexpMut::try_from_checked(value.clone())
        .unwrap()
        .try_set_integer_elt(0, 42)
        .unwrap();
    drop(value);
    for _ in 0..3 {
        full_gc(&session);
        assert_eq!(clone.integer_elt(0), Some(42));
    }
    drop(clone);
    assert_eq!(roots(&session), before);
    full_gc(&session);
    assert!(session.sexp(ptr).is_none());
}

#[test]
fn checked_child_outlives_parent_and_retains_original_owner() {
    let session = RSession::new_for_gc_tests();
    let child = session.sexp(alloc(&session, SEXPTYPE::INTSXP, 1)).unwrap();
    let parent = session.sexp(alloc(&session, SEXPTYPE::VECSXP, 1)).unwrap();
    SexpMut::try_from_checked(child.clone())
        .unwrap()
        .try_set_integer_elt(0, 73)
        .unwrap();
    SexpMut::try_from_checked(parent.clone())
        .unwrap()
        .try_set_vector_elt(0, child)
        .unwrap();
    let child = parent.vector_elt(0).unwrap();
    assert_eq!(child.owner(), parent.owner());
    drop(parent);
    full_gc(&session);
    assert_eq!(child.integer_elt(0), Some(73));
    let other = RSession::new_for_gc_tests();
    drop(child); // releases the original owner's lease even while another is active
    drop(other);
    full_gc(&session);
}

#[test]
fn checked_vector_iterator_retains_parent_and_roots_yielded_children() {
    let session = RSession::new_for_gc_tests();
    let child = session.sexp(alloc(&session, SEXPTYPE::INTSXP, 1)).unwrap();
    let parent = session.sexp(alloc(&session, SEXPTYPE::VECSXP, 1)).unwrap();
    SexpMut::try_from_checked(parent.clone())
        .unwrap()
        .try_set_vector_elt(0, child)
        .unwrap();
    let mut iter = parent.iter_vector();
    full_gc(&session);
    let yielded = iter.next().unwrap();
    drop(iter);
    full_gc(&session);
    assert_eq!(yielded.integer_elt(0), Some(0));
}

#[test]
fn checked_graph_writes_reject_foreign_owner() {
    let left = RSession::new_for_gc_tests();
    let right = RSession::new_for_gc_tests();
    let parent = left.sexp(alloc(&left, SEXPTYPE::VECSXP, 1)).unwrap();
    let child = right.sexp(alloc(&right, SEXPTYPE::INTSXP, 1)).unwrap();
    assert!(
        SexpMut::try_from_checked(parent.clone())
            .unwrap()
            .try_set_vector_elt(0, child)
            .is_err()
    );
    assert!(parent.vector_elt(0).unwrap().is_nil());
    let mut arena = RArena::new();
    assert!(arena.cons_sexp(parent, Sexp::nil(), None).is_none());
}

#[test]
fn checked_graph_writes_remember_original_owner_and_retain_unrooted_young_child() {
    for kind in [SEXPTYPE::VECSXP, SEXPTYPE::EXPRSXP, SEXPTYPE::STRSXP] {
        let left = RSession::new_for_gc_tests();
        let parent_ptr = alloc(&left, kind, 1);
        let parent = left.sexp(parent_ptr).unwrap();
        full_gc(&left); // Promote the parent before allocating its child.
        let child_ptr = if kind == SEXPTYPE::STRSXP {
            left.with_active_in(|owner| unsafe {
                crate::sexp::memory::with_arena_in(owner, |arena| arena.alloc_charsxp(b"kept"))
            })
        } else {
            alloc(&left, SEXPTYPE::INTSXP, 1)
        };
        let child = left.sexp(child_ptr).unwrap();
        let right = RSession::new_for_gc_tests(); // A different owner is active.
        let mut mutation = SexpMut::try_from_checked(parent).unwrap();
        if kind == SEXPTYPE::STRSXP {
            mutation.try_set_string_elt(0, child).unwrap();
        } else {
            mutation.try_set_vector_elt(0, child).unwrap();
        }
        drop(mutation); // Both nodes now have no checked leases.
        right.with_active_in(|owner| unsafe {
            assert_eq!((*owner).gc_state.remembered_set.len(), 0);
        });
        left.with_active_in(|owner| unsafe {
            assert!(
                (*owner)
                    .gc_state
                    .remembered_set
                    .iter()
                    .any(|ptr| ptr == parent_ptr)
            );
            left.owner_token().unwrap().minor_gc().unwrap();
            // Check membership before wrapping or reading a possibly swept node.
            assert!((*owner).arena.contains(child_ptr));
            let child = left.sexp(child_ptr).unwrap();
            if kind == SEXPTYPE::STRSXP {
                assert_eq!(child.try_as_string().unwrap(), "kept");
            } else {
                assert_eq!(child.integer_elt(0), Some(0));
            }
        });
    }
}

#[test]
fn checked_graph_write_barrier_failure_leaves_graph_and_membership_unchanged() {
    let session = RSession::new_for_gc_tests();
    let parent = session.sexp(alloc(&session, SEXPTYPE::VECSXP, 1)).unwrap();
    full_gc(&session);
    let child_ptr = alloc(&session, SEXPTYPE::INTSXP, 1);
    let child = session.sexp(child_ptr).unwrap();
    session.with_active_in(|owner| unsafe {
        (*owner)
            .gc_state
            .remembered_set
            .fail_next_reservation_for_test();
    });
    let mut mutation = SexpMut::try_from_checked(parent).unwrap();
    assert!(matches!(
        mutation.try_set_vector_elt(0, child.clone()),
        Err(super::SexpError::AllocationFailed {
            object: "GC write barrier"
        })
    ));
    let parent = mutation.freeze();
    assert!(parent.vector_elt(0).unwrap().is_nil());
    session.with_active_in(|owner| unsafe {
        assert_eq!((*owner).gc_state.remembered_set.len(), 0);
    });
    // Retrying the same valid write succeeds after the injected failure.
    let mut mutation = SexpMut::try_from_checked(parent).unwrap();
    mutation.try_set_vector_elt(0, child).unwrap();
    assert_eq!(
        mutation.freeze().vector_elt(0).unwrap().integer_elt(0),
        Some(0)
    );
}

#[test]
fn checked_compact_mutation_materializes_in_original_owner_and_restores_active_session() {
    let left = RSession::new_for_gc_tests();
    let ptr = left.with_active(|| unsafe { crate::sexp::altseq::compact_int_seq(10, 2, 3) });
    let value = left.sexp(ptr).unwrap();
    let right = RSession::new_for_gc_tests();
    let right_owner = right.with_active_in(|owner| unsafe {
        (*owner)
            .arena
            .set_budget(crate::sexp::memory::ArenaBudget::new(1, 0));
        owner
    });
    let mut mutation = SexpMut::try_from_checked(value).unwrap();
    mutation.try_set_integer_elt(1, 99).unwrap();
    assert_eq!(
        crate::sexp::instance::current_instance_ptr(),
        Some(right_owner)
    );
    drop(right); // The original session must own the expanded payload.
    let value = mutation.freeze();
    full_gc(&left);
    assert_eq!(value.iter_integer().collect::<Vec<_>>(), vec![10, 99, 14]);
}

#[test]
fn typed_builders_retain_and_reject_foreign_inputs() {
    let session = RSession::new_for_gc_tests();
    let ptr = alloc(&session, SEXPTYPE::INTSXP, 1);
    let builder = crate::sexp::builder::GenericVector::from_values([session.sexp(ptr).unwrap()]);
    full_gc(&session);
    assert!(session.sexp(ptr).is_some());
    let mut foreign = RArena::new();
    assert!(builder.build_in(&mut foreign).is_none());
    full_gc(&session);
    assert!(session.sexp(ptr).is_none());
}

#[test]
fn incremental_pairlist_retains_head_between_allocations() {
    let session = RSession::new_for_gc_tests();
    let mut builder = PairlistBuilder::new_in(session.owner_token().unwrap());
    for i in 0..8 {
        let value = session.sexp(alloc(&session, SEXPTYPE::INTSXP, 1)).unwrap();
        SexpMut::try_from_checked(value.clone())
            .unwrap()
            .try_set_integer_elt(0, i)
            .unwrap();
        builder.push(value, None).unwrap();
        full_gc(&session);
    }
    let list = builder.finish().unwrap();
    full_gc(&session);
    let values: Vec<_> = super::PairlistIter::new(list)
        .map(|cell| cell.car().unwrap().integer_elt(0).unwrap())
        .collect();
    assert_eq!(values, (0..8).collect::<Vec<_>>());
}

#[test]
fn incremental_pairlist_rejects_foreign_values_before_linking() {
    let left = RSession::new_for_gc_tests();
    let right = RSession::new_for_gc_tests();
    let value = right.sexp(alloc(&right, SEXPTYPE::INTSXP, 1)).unwrap();
    left.with_active_in(|_| {
        let mut builder = PairlistBuilder::new_in(left.owner_token().unwrap());
        assert!(builder.push(value, None).is_err());
        assert!(builder.is_empty());
    });
}

#[test]
fn direct_gc_defers_while_arena_is_lent() {
    let session = RSession::new_for_gc_tests();
    session.with_active_in(|owner| unsafe {
        let ptr = crate::sexp::memory::with_arena_in(owner, |arena| {
            let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
            assert_eq!(crate::sexp::gengc::full_gc_in(owner), (0, 0));
            assert!(arena.contains(ptr));
            ptr
        });
        assert!((*owner).gc_state.gc_pending);
        crate::sexp::gengc::run_pending_gc_if_quiescent_in(owner);
        assert!(!(*owner).arena.contains(ptr));
        assert!(!(*owner).gc_state.gc_pending);
    });
}

#[test]
fn owned_snapshot_does_not_alias_checked_mutation() {
    let session = RSession::new_for_gc_tests();
    let ptr = alloc(&session, SEXPTYPE::REALSXP, 2);
    let value = session.sexp(ptr).unwrap();
    let copied = value.clone().to_owned_value().unwrap();
    let mut mutation = SexpMut::try_from_checked(value).unwrap();
    mutation.try_set_real_elt(0, 2.5).unwrap();
    assert_eq!(copied, SexpValue::RealVector(vec![Some(0.0), Some(0.0)]));
    assert_eq!(mutation.freeze().real_elt(0), Some(2.5));
}

#[test]
fn checked_mutation_needs_no_raw_authority_and_preserves_read_aliases() {
    let session = RSession::new_for_gc_tests();
    let value = session.sexp(alloc(&session, SEXPTYPE::REALSXP, 1)).unwrap();
    let alias = value.clone();
    let mut mutation = SexpMut::try_from_checked(value).unwrap();
    mutation.try_set_real_elt(0, 9.5).unwrap();
    assert_eq!(alias.real_elt(0), Some(9.5));
    session.gc();
    assert_eq!(mutation.freeze().real_elt(0), Some(9.5));
}

#[test]
fn checked_mutation_rejects_raw_handles_and_immutable_singletons() {
    let mut arena = RArena::new();
    let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
    let raw = unsafe { Sexp::from_raw(ptr) }.unwrap();
    assert!(SexpMut::try_from_checked(raw).is_err());
    assert!(SexpMut::try_from_checked(Sexp::nil()).is_err());
    let checked = arena.sexp(ptr).unwrap();
    let mut mutable = SexpMut::try_from_checked(checked).unwrap();
    mutable.try_set_integer_elt(0, 5).unwrap();
    assert_eq!(mutable.freeze().integer_elt(0), Some(5));
}

#[cfg(not(miri))]
#[test]
fn checked_results_survive_gc_in_live_base_runtime() {
    let mut session = RSession::new_without_default_packages();
    let owner = session.with_active_in(|owner| owner);
    let (result, _, _) =
        session.eval_code_with_output_capture("local({ x <- list(1:4); gc(); x[[1]] })");
    let result = result.expect("base evaluation through rooted inputs");
    // Simulate C-core collection while the checked result retains its lease.
    // The safe session API correctly forbids borrowing session again here.
    for _ in 0..3 {
        unsafe { crate::sexp::gengc::full_gc_in(owner) };
    }
    assert_eq!(result.iter_integer().collect::<Vec<_>>(), vec![1, 2, 3, 4]);
    let (result, _, _) = session.eval_code_with_output_capture("sum(1:4)");
    assert_eq!(
        result
            .expect("runtime recovers after collection")
            .integer_elt(0),
        Some(10)
    );
}

#[test]
fn owner_capability_rejects_collection_in_another_active_runtime() {
    let left = RSession::new_for_gc_tests();
    let owner = left.owner_token().unwrap();
    let value = owner.sexp(alloc(&left, SEXPTYPE::INTSXP, 2)).unwrap();
    let right = RSession::new_for_gc_tests();
    assert!(matches!(
        owner.full_gc(),
        Err(super::SexpError::OwnerNotActive)
    ));
    assert!(matches!(
        owner.minor_gc(),
        Err(super::SexpError::OwnerNotActive)
    ));
    // Wrapping uses the original owner even when ambient dispatch differs.
    let alias = owner.sexp(value.as_raw()).unwrap();
    assert_eq!(alias.integer_elt(0), Some(0));
    assert!(owner.sexp(alloc(&right, SEXPTYPE::INTSXP, 1)).is_err());
    left.with_active_in(|_| owner.full_gc().unwrap());
    assert_eq!(alias.integer_elt(1), Some(0));
}

#[test]
fn integer_copy_checks_extent_and_retains_an_independent_snapshot() {
    let session = RSession::new_for_gc_tests();
    let value = session.sexp(alloc(&session, SEXPTYPE::INTSXP, 2)).unwrap();
    let mut short = [99];
    assert!(matches!(
        value.copy_integer_into(&mut short),
        Err(super::SexpError::LengthMismatch {
            expected: 2,
            actual: 1
        })
    ));
    assert_eq!(short, [99]);
    let mut snapshot = [99; 2];
    value.copy_integer_into(&mut snapshot).unwrap();
    let mut mutation = SexpMut::try_from_checked(value).unwrap();
    mutation.try_set_integer_elt(0, 7).unwrap();
    session.gc();
    assert_eq!(snapshot, [0, 0]);
    assert_eq!(mutation.freeze().integer_elt(0), Some(7));
}

#[test]
fn native_view_keeps_original_generation_without_manufacturing_a_root() {
    let session = RSession::new_for_gc_tests();
    let ptr = alloc(&session, SEXPTYPE::INTSXP, 1);
    // The actual header storage remains in this fixture's live arena. Invalidate
    // it at the allocator seam without reading its reclaimed payload.
    let view = unsafe { Sexp::from_raw_unchecked(ptr) };
    assert!(matches!(
        view.clone().into_owned(),
        Err(super::SexpError::RootUnavailable)
    ));
    assert!(view.is_live());
    session.with_active_in(|owner| unsafe {
        crate::sexp::memory::with_arena_in(owner, |arena| arena.free_node(ptr));
    });
    assert!(!view.is_live());
    let reused = session.with_active_in(|owner| unsafe {
        crate::sexp::memory::with_arena_in(owner, |arena| arena.alloc_node(SEXPTYPE::REALSXP))
    });
    assert_eq!(ptr, reused);
    assert_eq!(
        view.try_integer_elt(0),
        Err(super::SexpError::StaleAllocation)
    );
    let fresh = session.sexp(reused).unwrap();
    assert!(fresh.is_live());
    assert_eq!(fresh.typeof_(), SEXPTYPE::REALSXP);
}

#[test]
fn checked_handle_rejects_reclaimed_and_reused_allocation() {
    let session = RSession::new_for_gc_tests();
    let ptr = alloc(&session, SEXPTYPE::INTSXP, 1);
    let old = session.sexp(ptr).unwrap();
    let clone = old.clone();
    assert!(old.is_live());
    // Exercise invalidation at the unsafe allocator seam without reading the
    // reclaimed payload. Ordinary collection retains checked handle leases.
    session.with_active_in(|owner| unsafe {
        crate::sexp::memory::with_arena_in(owner, |arena| arena.free_node(ptr));
    });
    assert!(!old.is_live());
    assert_eq!(
        old.try_integer_elt(0),
        Err(super::SexpError::StaleAllocation)
    );
    assert!(matches!(
        SexpMut::try_from_checked(clone),
        Err(super::SexpError::StaleAllocation)
    ));
    let reused = session.with_active_in(|owner| unsafe {
        crate::sexp::memory::with_arena_in(owner, |arena| arena.alloc_node(SEXPTYPE::REALSXP))
    });
    assert_eq!(ptr, reused, "exercise actual physical slot reuse");
    let fresh = session.sexp(reused).unwrap();
    assert!(fresh.is_live());
    assert!(!old.is_live());
    assert_ne!(old, fresh, "reusing storage must not reuse value identity");
    let mut identities = std::collections::HashSet::new();
    let old_identity = old.node.as_ref().unwrap().id().link();
    let fresh_identity = fresh.node.as_ref().unwrap().id().link();
    identities.insert(old_identity);
    assert!(!identities.contains(&fresh_identity));
    identities.insert(fresh_identity);
    assert_eq!(identity_hash(&old), identity_hash(&old.clone()));
    assert_eq!(identity_hash(&fresh), identity_hash(&fresh.clone()));
    assert_eq!(identities.len(), 2);
    assert_eq!(
        old.try_integer_elt(0),
        Err(super::SexpError::StaleAllocation)
    );
    assert_eq!(fresh.typeof_(), SEXPTYPE::REALSXP);
    assert_eq!(fresh.len(), 0);
}

#[test]
fn checked_children_do_not_adopt_reused_slots() {
    let session = RSession::new_for_gc_tests();
    let child_ptr = alloc(&session, SEXPTYPE::INTSXP, 1);
    let child = session.sexp(child_ptr).unwrap();
    let vector = session.sexp(alloc(&session, SEXPTYPE::VECSXP, 1)).unwrap();
    let cell_ptr = session.with_active_in(|owner| unsafe {
        crate::sexp::memory::with_arena_in(owner, |arena| {
            arena.cons(
                child_ptr,
                crate::sexp::globals::R_NilValue(),
                crate::sexp::globals::R_NilValue(),
            )
        })
    });
    let cell = session.sexp(cell_ptr).unwrap();
    SexpMut::try_from_checked(vector.clone())
        .unwrap()
        .try_set_vector_elt(0, child.clone())
        .unwrap();
    let saved = vector.reference_elt(0).unwrap();
    // Force actual slot retirement at the allocator seam. The canonical
    // parent edges retain their old generation even when storage is reused.
    session.with_active_in(|owner| unsafe {
        crate::sexp::memory::with_arena_in(owner, |arena| arena.free_node(child_ptr));
    });
    let replacement_ptr = session.with_active_in(|owner| unsafe {
        crate::sexp::memory::with_arena_in(owner, |arena| arena.alloc_node(SEXPTYPE::REALSXP))
    });
    assert_eq!(child_ptr, replacement_ptr);
    let replacement = session.sexp(replacement_ptr).unwrap();
    assert!(replacement.is_live());
    assert!(matches!(cell.try_car(), Err(SexpError::StaleAllocation)));
    assert!(matches!(
        vector.try_vector_elt(0),
        Err(SexpError::StaleAllocation)
    ));
    assert!(vector.copied_header_link(saved).is_none());
    let raw_read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::sexp::accessors::CAR(cell.as_raw())
    }));
    assert!(
        raw_read.is_err(),
        "raw projections must reject a stale saved edge"
    );
}

#[test]
fn raw_flags_cannot_mutate_retained_singletons_from_a_closed_bank() {
    use crate::sexp::accessors::*;
    let session = RSession::new_for_gc_tests();
    let logical = unsafe { Sexp::from_static_raw_unchecked(crate::sexp::globals::R_True()) };
    let missing = unsafe { Sexp::from_static_raw_unchecked(crate::sexp::globals::R_NaString()) };
    let parent = session.sexp(alloc(&session, SEXPTYPE::VECSXP, 2)).unwrap();
    let mut mutation = SexpMut::try_from_checked(parent).unwrap();
    mutation.try_set_vector_elt(0, logical.clone()).unwrap();
    mutation.try_set_vector_elt(1, missing.clone()).unwrap();
    let parent = mutation.freeze();
    let before = logical.header();
    let missing_before = missing.header();
    crate::sexp::globals::close_immutable_singletons_for_test();
    let assert_logical_unchanged = || {
        let after = logical.header();
        assert_eq!(after.sxpinfo.type_and_flags, before.sxpinfo.type_and_flags);
        assert_eq!(after.sxpinfo.rcount, before.sxpinfo.rcount);
        assert_eq!(after.body, before.body);
        assert_eq!(after.attrib, before.attrib);
        assert_eq!(after.payload, before.payload);
        assert_eq!(logical.logical_elt(0), Some(1));
    };
    // Void raw setters reject an immutable target by leaving it untouched.
    // A retained lease keeps the original header and payload observable.
    unsafe {
        SET_NAMED(logical.as_raw(), 0);
    }
    assert_logical_unchanged();
    session.with_active(|| unsafe {
        assert_eq!(TYPEOF(logical.as_raw()), SEXPTYPE::LGLSXP.0);
        assert_eq!(NAMED(logical.as_raw()), 2);
        for (write, value) in [
            (SET_OBJECT as unsafe fn(_, _), 1),
            (SET_NAMED, 0),
            (SET_ALTREP, 1),
            (SET_MARK, 1),
            (SETLEVELS, 3),
            (SET_MISSING, 1),
        ] {
            write(logical.as_raw(), value);
            assert_logical_unchanged();
        }
        SET_S4_OBJECT(logical.as_raw());
        assert_logical_unchanged();
        mark_charsxp_encoding(missing.as_raw(), "UTF-8");
        let after = missing.header();
        assert_eq!(
            after.sxpinfo.type_and_flags,
            missing_before.sxpinfo.type_and_flags
        );
        assert_eq!(after.sxpinfo.rcount, missing_before.sxpinfo.rcount);
        assert_eq!(after.body, missing_before.body);
        assert_eq!(after.attrib, missing_before.attrib);
        assert_eq!(after.payload, missing_before.payload);
        assert_eq!(getCharCE(missing.as_raw()), 0);
    });
    assert!(parent.vector_elt(1).unwrap().is_na_string());
    full_gc(&session);
    assert_eq!(parent.vector_elt(0).unwrap().logical_elt(0), Some(1));
}

#[test]
fn checked_child_rejects_foreign_slot_before_header_read() {
    let left = RSession::new_for_gc_tests();
    let right = RSession::new_for_gc_tests();
    let parent = left.sexp(alloc(&left, SEXPTYPE::VECSXP, 1)).unwrap();
    let foreign = right.sexp(alloc(&right, SEXPTYPE::INTSXP, 1)).unwrap();
    // Inject a foreign exact identity through the actual typed cell, bypassing
    // domain validation only. No native payload projection is involved.
    let lease = parent.header().payload_lease().unwrap().clone();
    lease
        .set_reference_elt(0, foreign.node.as_ref().unwrap().link().unwrap())
        .unwrap();
    assert!(parent.vector_elt(0).is_none());
    assert!(
        parent
            .copied_header_link(foreign.node.as_ref().unwrap().link().unwrap())
            .is_none()
    );
}

#[test]
fn checked_factories_recover_owned_provenance_from_address_only_inputs() {
    use crate::sexp::ffi::SexprecCore;
    let session = RSession::new_for_gc_tests();
    let arena_ptr = alloc(&session, SEXPTYPE::INTSXP, 1);
    let address_only = std::ptr::without_provenance_mut::<SexprecCore>(arena_ptr.addr());
    let arena_value = session.sexp(address_only).unwrap();
    assert_eq!(arena_value.integer_elt(0), Some(0));
    let mut mutation = SexpMut::try_from_checked(arena_value).unwrap();
    mutation.try_set_integer_elt(0, 51).unwrap();
    assert_eq!(mutation.integer_elt(0), Some(51));
    let global = session.with_active(|| unsafe { crate::sexp::globals::R_GlobalEnv() });
    let address_only = std::ptr::without_provenance_mut::<SexprecCore>(global.addr());
    assert!(session.sexp(address_only).unwrap().is_environment());
    let nil = Sexp::nil().as_raw();
    let address_only = std::ptr::without_provenance_mut::<SexprecCore>(nil.addr());
    assert_eq!(
        session.sexp(address_only).unwrap().typeof_(),
        SEXPTYPE::NILSXP
    );
    let parent = session.sexp(alloc(&session, SEXPTYPE::VECSXP, 1)).unwrap();
    // The raw entry captures a checked identity once. Child reads then resolve
    // that saved token rather than reclassifying an element address.
    unsafe {
        crate::sexp::accessors::SET_VECTOR_ELT(
            parent.as_raw(),
            0,
            std::ptr::without_provenance_mut::<SexprecCore>(arena_ptr.addr()),
        );
    }
    assert_eq!(parent.vector_elt(0).unwrap().integer_elt(0), Some(51));
    assert!(
        parent
            .copied_header_link(parent.reference_elt(0).unwrap())
            .is_some()
    );
    let mut arena = RArena::new();
    let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
    let address_only = std::ptr::without_provenance_mut::<SexprecCore>(ptr.addr());
    assert_eq!(arena.sexp(address_only).unwrap().integer_elt(0), Some(0));
}

#[test]
fn node_factory_roots_during_arena_lend_without_borrowing_its_owner() {
    let session = RSession::new_for_gc_tests();
    let factory = super::SessionNodeFactory::new(session.owner_token().unwrap());
    let value = session.with_active_in(|owner| unsafe {
        // The factory was captured before this exclusive field lend. Its
        // wrapping path must use metadata alone, including under Miri.
        crate::sexp::memory::with_arena_in(owner, |arena| {
            factory
                .wrap(arena.alloc_vector(SEXPTYPE::INTSXP, 2))
                .unwrap()
        })
    });
    full_gc(&session);
    assert_eq!(value.integer_elt(1), Some(0));
    let original = value.node.clone().unwrap();
    drop(value);
    full_gc(&session);
    assert!(!original.is_live());
}

#[test]
fn node_factory_rejects_foreign_and_unregistered_addresses() {
    let left = RSession::new_for_gc_tests();
    let right = RSession::new_for_gc_tests();
    let factory = super::SessionNodeFactory::new(left.owner_token().unwrap());
    let foreign = alloc(&right, SEXPTYPE::INTSXP, 1);
    assert!(matches!(
        factory.wrap(foreign),
        Err(SexpError::UnownedPointer { .. })
    ));
    assert!(matches!(
        factory.wrap(std::ptr::dangling_mut()),
        Err(SexpError::UnownedPointer { .. })
    ));
    assert!(factory.wrap(Sexp::nil().as_raw()).unwrap().is_nil());
}

#[test]
fn owned_payload_snapshot_survives_parent_retirement_and_slot_reuse() {
    let mut arena = RArena::new();
    let pointer = arena.alloc_vector(SEXPTYPE::INTSXP, 2);
    let original = arena.node_token(pointer).unwrap();
    let snapshot = {
        let value = arena.sexp(pointer).unwrap();
        let mut mutation = SexpMut::try_from_checked(value).unwrap();
        mutation.try_set_integer_elt(0, 41).unwrap();
        mutation.try_set_integer_elt(1, 43).unwrap();
        mutation.freeze().header()
    };
    let lease = snapshot.payload_lease().unwrap();
    let old_bytes = arena.total_bytes_allocated();
    // The fixture has no node handles, payload references, or graph edges.
    unsafe {
        arena.free_node(pointer);
    }
    assert!(!original.is_live());
    assert_eq!(arena.total_bytes_allocated(), old_bytes);
    let replacement = arena.alloc_vector(SEXPTYPE::INTSXP, 2);
    assert_eq!(replacement.addr(), pointer.addr());
    let current = arena.sexp(replacement).unwrap();
    let mut current = SexpMut::try_from_checked(current).unwrap();
    current.try_set_integer_elt(0, 99).unwrap();
    assert_eq!(lease.integer_elt(0), Some(41));
    assert_eq!(lease.integer_elt(1), Some(43));
    assert!(!lease.same_allocation(current.freeze().header().payload_lease().unwrap()));
    assert!(original.heap_identity().payload_lease(&original).is_none());
}

#[test]
fn raw_factory_rejects_unowned_headers_before_any_safe_reader() {
    let mut foreign = crate::sexp::ffi::SexprecCore::new(SEXPTYPE::NILSXP);
    let pointer = &mut foreign as crate::sexp::ffi::SEXP;
    assert!(unsafe { Sexp::from_raw(pointer) }.is_none());
}
