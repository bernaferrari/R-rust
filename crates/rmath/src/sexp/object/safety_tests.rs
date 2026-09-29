use super::{Sexp, SexpMut, SexpView, pairlist::PairlistBuilder};
use crate::sexp::{ffi::SEXPTYPE, memory::RArena, session::RSession};

fn alloc(session: &RSession, kind: SEXPTYPE, len: i32) -> crate::sexp::ffi::SEXP {
    session.with_active_in(|owner| unsafe {
        crate::sexp::memory::with_arena_in(owner, |arena| arena.alloc_vector(kind, len.into()))
    })
}

fn full_gc(session: &RSession) {
    session.with_active_in(|owner| {
        crate::sexp::gengc::full_gc_in(owner);
    });
}

fn roots(session: &RSession) -> usize {
    session.with_active_in(|owner| unsafe { (*owner).root_table.len() })
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
    // SAFETY: this owner outlives the builder and no cell payload is borrowed.
    let mut builder = unsafe { PairlistBuilder::new() };
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
        let mut builder = unsafe { PairlistBuilder::new() };
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
fn scoped_loan_returns_owned_values_and_ends_before_mutation() {
    let mut session = RSession::new_for_gc_tests();
    let ptr = alloc(&session, SEXPTYPE::REALSXP, 2);
    let copied = unsafe {
        session.with_sexp_view(ptr, |view| match view {
            SexpView::Real(data) => data.to_vec(),
            _ => panic!("real view"),
        })
    }
    .unwrap();
    assert_eq!(copied, vec![0.0, 0.0]);
    let value = session.sexp(ptr).unwrap();
    let mut mutation = unsafe { SexpMut::from_owned(value) };
    mutation.try_set_real_elt(0, 2.5).unwrap();
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
        crate::sexp::gengc::full_gc_in(owner);
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
