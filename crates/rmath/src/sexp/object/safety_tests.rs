use super::{Sexp, SexpMut, SexpValue, pairlist::PairlistBuilder};
use crate::sexp::{ffi::SEXPTYPE, memory::RArena, session::RSession};

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
    // SAFETY: no borrowed payload views exist; checked handles retain the node.
    unsafe { value.clone().try_set_integer_elt(0, 42) }.unwrap();
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
    unsafe {
        child.clone().try_set_integer_elt(0, 73).unwrap();
        parent.clone().try_set_vector_elt(0, child).unwrap();
    }
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
    unsafe {
        parent.clone().try_set_vector_elt(0, child).unwrap();
    }
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
    unsafe {
        assert!(parent.clone().try_set_vector_elt(0, child).is_err());
    }
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
        unsafe {
            value.clone().try_set_integer_elt(0, i).unwrap();
        }
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
    identities.insert(old.clone());
    assert!(!identities.contains(&fresh));
    identities.insert(fresh.clone());
    assert_eq!(identities.len(), 2);
    assert_eq!(
        old.try_integer_elt(0),
        Err(super::SexpError::StaleAllocation)
    );
    assert_eq!(fresh.typeof_(), SEXPTYPE::REALSXP);
    assert_eq!(fresh.len(), 0);
}

#[test]
fn checked_child_rejects_foreign_slot_before_header_read() {
    let left = RSession::new_for_gc_tests();
    let right = RSession::new_for_gc_tests();
    let parent = left.sexp(alloc(&left, SEXPTYPE::VECSXP, 1)).unwrap();
    let foreign = right.sexp(alloc(&right, SEXPTYPE::INTSXP, 1)).unwrap();
    // A translated-code write bypasses the checked write barrier. The Rust
    // projection must still reject this graph edge before reading its header.
    unsafe {
        let parent_ptr = parent.clone().as_raw();
        let data = (*parent_ptr)
            .gengc_next_node
            .cast::<crate::sexp::ffi::SEXP>();
        data.write(foreign.clone().as_raw());
    }
    assert!(parent.vector_elt(0).is_none());
    assert!(parent.copied_header(foreign.as_raw()).is_none());
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
    // Legacy graph writes may carry an address without provenance. Checked
    // child reads rederive a projection from the owned allocation.
    unsafe {
        let ptr = parent.clone().as_raw();
        (*ptr)
            .gengc_next_node
            .cast::<crate::sexp::ffi::SEXP>()
            .write(std::ptr::without_provenance_mut::<SexprecCore>(
                arena_ptr.addr(),
            ));
    }
    assert_eq!(parent.vector_elt(0).unwrap().integer_elt(0), Some(51));
    assert!(
        parent
            .copied_header(std::ptr::without_provenance_mut(arena_ptr.addr()))
            .is_some()
    );
    let mut arena = RArena::new();
    let ptr = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
    let address_only = std::ptr::without_provenance_mut::<SexprecCore>(ptr.addr());
    assert_eq!(arena.sexp(address_only).unwrap().integer_elt(0), Some(0));
}
