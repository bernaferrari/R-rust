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


/// All descriptor edits here go through the public safe value API, exactly as
/// provider code can do through its owning context. No corrupt header seam.
struct ReplaceDeclaration {
    phase: Rc<std::cell::Cell<u8>>,
}
impl ReplaceDeclaration {
    fn replace(&self, c: &AltrepContext<'_>) -> SexpResult<()> {
        let control = c.data1()?;
        let replacement = control.try_vector_elt(0)?;
        let slots = c.object().try_attrib()?.try_car()?;
        SexpMut::try_from_checked(slots)?.try_set_vector_elt(0, replacement)?;
        SexpMut::try_from_checked(control)?.try_set_vector_elt(1, c.object())?;
        c.gc()
    }
}
impl AltrepClass for ReplaceDeclaration {
    fn vector_type(&self) -> SEXPTYPE { SEXPTYPE::INTSXP }
    fn cache_in_data2(&self) -> bool { true }
    fn length(&self, c: &AltrepContext<'_>) -> SexpResult<i64> {
        if self.phase.get() == 1 { self.replace(c)?; }
        Ok(2)
    }
    fn element<'s>(&self, c: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        if self.phase.get() == 2 { self.replace(c)?; }
        Ok(AltrepElement::Integer(37 + i as i32))
    }
}
struct OtherDeclaration;
impl AltrepClass for OtherDeclaration {
    fn vector_type(&self) -> SEXPTYPE { SEXPTYPE::REALSXP }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> { Ok(2) }
    fn element<'s>(&self, _: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
        Ok(AltrepElement::Real(99.0))
    }
}

#[test]
fn provider_declaration_replacement_rejects_length_and_element_publication() {
    for phase in [1, 2] {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let state = Rc::new(std::cell::Cell::new(phase));
        let original = session.register_altrep_class("declaration.original", ReplaceDeclaration {
            phase: state.clone(),
        }).unwrap();
        let replacement = session.register_altrep_class("declaration.replacement", OtherDeclaration).unwrap();
        let mut control = SexpMut::try_from_checked(allocate(owner, SEXPTYPE::VECSXP, 2).unwrap()).unwrap();
        control.try_set_vector_elt(0, replacement.descriptor()).unwrap();
        let control = control.freeze();
        let mut sentinel = SexpMut::try_from_checked(allocate(owner, SEXPTYPE::INTSXP, 1).unwrap()).unwrap();
        sentinel.try_set_integer_elt(0, 73).unwrap();
        let sentinel = sentinel.freeze();
        let built = AltrepBuilder::new(original.clone()).data1(control.clone()).data2(sentinel.clone()).build();
        let object = if phase == 1 {
            assert!(matches!(built, Err(SexpError::Altrep { reason: "ALTREP declaration changed during callback" })));
            let object = control.try_vector_elt(1).unwrap();
            assert_eq!(object.len(), 0);
            object
        } else {
            let object = built.unwrap();
            // Scalar reads also reject changed declarations before returning
            // values from the former provider.
            assert!(object.try_integer_elt(0).is_err());
            SexpMut::try_from_checked(Metadata::load(&object).unwrap().slots.clone()).unwrap()
                .try_set_vector_elt(0, original.descriptor()).unwrap();
            assert!(force_materialization(&object).is_err());
            assert_eq!(object.len(), 2);
            object
        };
        let metadata = Metadata::load(&object).unwrap();
        assert_eq!(metadata.descriptor().unwrap().as_raw(), replacement.descriptor().as_raw());
        assert!(object.header().payload.is_empty());
        assert!(metadata.get(Slot::DenseCache).unwrap().is_nil());
        assert_eq!(metadata.data2().unwrap().as_raw(), sentinel.as_raw());
        // Restore the declaration explicitly; callback changes themselves
        // remain observable after a rejected operation.
        SexpMut::try_from_checked(metadata.slots.clone()).unwrap()
            .try_set_vector_elt(0, original.descriptor()).unwrap();
        state.set(0);
        if phase == 1 {
            let retry = AltrepBuilder::new(original).data1(control).data2(sentinel).build().unwrap();
            force_materialization(&retry).unwrap();
            assert_eq!(retry.try_integer_elt(1).unwrap(), 38);
        } else {
            force_materialization(&object).unwrap();
            assert_eq!(object.try_integer_elt(1).unwrap(), 38);
            assert_eq!(metadata.data2().unwrap().try_integer_elt(1).unwrap(), 38);
        }
    }
}

#[test]
fn rejected_dense_payload_publication_keeps_provider_cache_cells_unchanged() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let class = session.register_altrep_class("publication.conflict", One).unwrap();
    let sentinel = allocate(owner, SEXPTYPE::VECSXP, 1).unwrap();
    let object = AltrepBuilder::new(class).data2(sentinel.clone()).build().unwrap();
    let storage = InstanceStorage::load(&object).unwrap();
    let source = allocate(owner, SEXPTYPE::INTSXP, 1).unwrap();
    // A real competing canonical publication invalidates the saved lazy
    // declaration. The cache transaction must reject without overwriting
    // provider data or installing its abandoned private output.
    let node = object.allocation().unwrap();
    node.heap_identity().attach_initialized_payload(node, |lease| lease.set_integer_elt(0, 91)).unwrap();
    assert!(storage.publish_dense(source, CachePolicy::Data2).is_err());
    assert!(storage.metadata.get(Slot::DenseCache).unwrap().is_nil());
    assert_eq!(storage.metadata.data2().unwrap().as_raw(), sentinel.as_raw());
    assert_eq!(object.try_integer_elt(0).unwrap(), 91);
}
