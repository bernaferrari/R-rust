//! Allocation-time notifications can reenter collection before the arena
//! closure has returned its new node to the checked handle layer.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn collect_again_after_each_allocation(session: &RSession) {
    let reentering = Arc::new(AtomicBool::new(false));
    let in_callback = reentering.clone();
    session.with_active(|| {
        super::super::super::gengc::register_gc_callback(Box::new(move |_| {
            if !in_callback.swap(true, Ordering::SeqCst) {
                super::super::super::gengc::full_gc();
                in_callback.store(false, Ordering::SeqCst);
            }
        }));
    });
    // SAFETY: update only the original live owner's torture counters; no
    // arena loan or callback overlaps these strictly local writes.
    unsafe {
        let owner = session.owner_token().unwrap().as_ptr();
        (*owner).memory_state.gc_force_gap = 1;
        (*owner).memory_state.gc_force_wait = 1;
    }
}

#[test]
fn fresh_vector_and_string_are_rooted_before_allocation_gc_callbacks() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    collect_again_after_each_allocation(&session);
    let vector = allocate(owner, SEXPTYPE::REALSXP, 3).unwrap();
    let text = string(owner, "retained during callback collection").unwrap();
    session.gc();
    assert_eq!(vector.len(), 3);
    assert_eq!(vector.try_real_elt(2).unwrap(), 0.0);
    assert_eq!(
        text.try_as_string().unwrap(),
        "retained during callback collection"
    );
}

struct One;
impl AltrepClass for One {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::INTSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(1)
    }
    fn element<'s>(&self, _: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
        Ok(AltrepElement::Integer(37))
    }
}

#[test]
fn pending_metadata_and_materialization_survive_allocation_gc_callbacks() {
    let session = RSession::new_for_gc_tests();
    let class = session
        .register_altrep_class("reentrant.allocation", One)
        .unwrap();
    collect_again_after_each_allocation(&session);
    let object = AltrepBuilder::new(class).build().unwrap();
    assert_eq!(object.try_integer_elt(0).unwrap(), 37);
    force_materialization(&object).unwrap();
    session.gc();
    assert_eq!(object.try_integer_elt(0).unwrap(), 37);
    assert!(Metadata::load(&object).is_some());
}

#[test]
fn compact_factories_root_results_before_allocation_gc_callbacks() {
    let session = RSession::new_for_gc_tests();
    collect_again_after_each_allocation(&session);
    let integer = session.compact_integer_sequence(7, 2, 4).unwrap();
    let real = session.compact_real_sequence(0.25, 0.5, 4).unwrap();
    let empty = session.compact_integer_sequence(0, 1, 0).unwrap();
    session.gc();
    assert_eq!(integer.try_integer_elt(3).unwrap(), 13);
    assert_eq!(real.try_real_elt(3).unwrap(), 1.75);
    assert!(empty.is_empty());
    assert!(integer.compact_seq().is_some());
    assert!(real.compact_seq().is_some());
}
