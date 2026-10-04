#![allow(unsafe_code)]
use super::*;
use crate::sexp::ffi::{EdgeField, NodeBody, VectorMetadata};
use std::cell::Cell;

fn collect(session: &RSession) {
    session.with_active(crate::sexp::gengc::full_gc);
}

fn sequence(session: &RSession) -> Sexp<'_> {
    new_sequence(
        session.owner_token().unwrap(),
        SEXPTYPE::INTSXP,
        3.0,
        2.0,
        8,
    )
    .unwrap()
}

fn attribute<'s>(session: &'s RSession, name: &std::ffi::CStr, number: i32) -> Sexp<'s> {
    let owner = session.owner_token().unwrap();
    let tag = storage::intern(owner, name).unwrap();
    let mut value =
        SexpMut::try_from_checked(allocate(owner, SEXPTYPE::INTSXP, 1).unwrap()).unwrap();
    value.try_set_integer_elt(0, number).unwrap();
    owner
        .node_factory()
        .pairlist_cell(&value.freeze(), &owner.node_factory().nil(), &tag)
        .unwrap()
}

#[test]
fn private_sequence_public_attribute_is_copied_without_skipping_first_cell() {
    let session = RSession::new_for_gc_tests();
    let value = sequence(&session);
    let descriptor = altrep_class(&value).unwrap();
    let state = data1(&value).unwrap();
    let attrs = attribute(&session, c"custom.sequence.attribute", 71);
    session.with_active(|| unsafe {
        crate::sexp::accessors::SET_ATTRIB(value.as_raw(), attrs.as_raw())
    });
    drop(attrs);
    let copy = session.with_active(|| unsafe {
        session
            .sexp(crate::mainutils::duplicate::Rf_duplicate(value.as_raw()))
            .unwrap()
    });
    collect(&session);
    let copied_attrs = copy.try_attrib().unwrap();
    assert_eq!(
        copied_attrs.try_car().unwrap().try_integer_elt(0).unwrap(),
        71
    );
    assert_eq!(
        copied_attrs.try_tag().unwrap().as_raw(),
        storage::intern(session.owner_token().unwrap(), c"custom.sequence.attribute")
            .unwrap()
            .as_raw()
    );
    assert_eq!(copy.try_integer_elt(7).unwrap(), 17);
    assert_eq!(altrep_class(&value).unwrap().as_raw(), descriptor.as_raw());
    assert_eq!(data1(&value).unwrap().as_raw(), state.as_raw());
    assert!(value.compact_seq().is_some());
}

#[test]
fn private_sequence_metadata_is_sole_root_after_public_attributes_are_removed() {
    let session = RSession::new_for_gc_tests();
    let value = sequence(&session);
    let state = data1(&value).unwrap();
    let state_node = state.allocation().unwrap().clone();
    drop(state);
    session.with_active(|| unsafe {
        crate::sexp::accessors::SET_ATTRIB(value.as_raw(), crate::sexp::globals::R_NilValue())
    });
    collect(&session);
    assert!(state_node.is_live());
    assert!(value.try_attrib().unwrap().is_nil());
    assert_eq!(value.try_integer_elt(7).unwrap(), 17);
    drop(value);
    collect(&session);
    assert!(!state_node.is_live());
}

#[test]
fn private_sequence_lent_payload_writes_override_formula_and_reuse_parent_storage() {
    let session = RSession::new_for_gc_tests();
    let value = sequence(&session);
    let descriptor = altrep_class(&value).unwrap().as_raw();
    let state = data1(&value).unwrap().as_raw();
    session.with_active_in(|owner| unsafe {
        crate::sexp::memory::with_arena_in(owner, |arena| {
            let count = arena.node_count();
            force_materialization(&value).unwrap();
            let pointer = crate::sexp::accessors::INTEGER(value.as_raw());
            *pointer.add(2) = 99;
            assert_eq!(value.try_integer_elt(2).unwrap(), 99);
            assert!(matches!(
                altrep_elt(&value, 2).unwrap(),
                AltrepElement::Integer(99)
            ));
            assert_eq!(arena.node_count(), count);
        });
    });
    let payload = value.header().payload;
    force_materialization(&value).unwrap();
    collect(&session);
    assert_eq!(value.header().payload, payload);
    assert_eq!(value.try_integer_elt(2).unwrap(), 99);
    assert_eq!(altrep_class(&value).unwrap().as_raw(), descriptor);
    assert_eq!(data1(&value).unwrap().as_raw(), state);
    assert!(is_altrep(&value));
}

struct UntrustedSequence(Rc<Cell<usize>>);
impl AltrepClass for UntrustedSequence {
    fn vector_type(&self) -> SEXPTYPE {
        self.0.set(self.0.get() + 1);
        SEXPTYPE::INTSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        self.0.set(self.0.get() + 1);
        Ok(8)
    }
    fn element<'s>(&self, _: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
        self.0.set(self.0.get() + 1);
        Ok(AltrepElement::Integer(900))
    }
}

#[test]
fn private_sequence_reserved_class_names_reject_before_any_provider_callback() {
    let session = RSession::new_for_gc_tests();
    let callbacks = Rc::new(Cell::new(0));
    assert!(
        session
            .register_altrep_class(
                ".builtin.compact_intseq",
                UntrustedSequence(callbacks.clone())
            )
            .is_err()
    );
    assert_eq!(callbacks.get(), 0);
    let value = sequence(&session);
    assert_eq!(value.try_integer_elt(7).unwrap(), 17);
    assert_eq!(callbacks.get(), 0);
    assert!(
        session
            .register_altrep_class(
                ".builtin.compact_intseq",
                UntrustedSequence(callbacks.clone())
            )
            .is_err()
    );
    assert_eq!(callbacks.get(), 0);
}

#[test]
fn private_sequence_foreign_and_retired_metadata_edges_cannot_replace_original() {
    let session = RSession::new_for_gc_tests();
    let value = sequence(&session);
    let node = value.allocation().unwrap();
    let heap = node.heap_identity();
    let original = heap.node_snapshot(node).unwrap();
    let other = RSession::new_for_gc_tests();
    let foreign = sequence(&other);
    let NodeBody::Vector(vector) = foreign.header().body else {
        unreachable!()
    };
    let mut rejected = heap.node_snapshot(node).unwrap();
    rejected
        .set_edge(EdgeField::VectorMetadata, vector.metadata.link())
        .unwrap();
    assert!(heap.replace_node(node, rejected).is_none());
    assert_eq!(heap.node_snapshot(node).unwrap().data, original.data);
    let stale = sequence(&session);
    let NodeBody::Vector(vector) = stale.header().body else {
        unreachable!()
    };
    let stale_link = vector.metadata.link();
    drop(stale);
    collect(&session);
    let mut rejected = heap.node_snapshot(node).unwrap();
    rejected
        .set_edge(EdgeField::VectorMetadata, stale_link)
        .unwrap();
    assert!(heap.replace_node(node, rejected).is_none());
    assert_eq!(heap.node_snapshot(node).unwrap().data, original.data);
    assert!(
        matches!(value.header().body, NodeBody::Vector(vector) if matches!(vector.metadata, VectorMetadata::BuiltinSequence(_)))
    );
    assert_eq!(value.try_integer_elt(7).unwrap(), 17);
}

#[test]
fn private_sequence_malformed_formula_never_uses_unchecked_passive_arithmetic() {
    let session = RSession::new_for_gc_tests();
    let value = sequence(&session);
    let mut state = SexpMut::try_from_checked(data1(&value).unwrap()).unwrap();
    state.try_set_real_elt(1, f64::INFINITY).unwrap();
    assert!(value.compact_seq().is_none());
    assert!(value.try_integer_elt(0).is_err());
    assert!(force_materialization(&value).is_err());
    assert!(!is_materialized(&value));
}

#[test]
fn private_sequence_forged_public_tags_remain_ordinary_attributes() {
    let session = RSession::new_for_gc_tests();
    for name in [c".InternalAltSeq", c".InternalAltrep"] {
        let value = allocate(session.owner_token().unwrap(), SEXPTYPE::INTSXP, 8).unwrap();
        let attrs = attribute(&session, name, 123);
        session.with_active(|| unsafe {
            crate::sexp::accessors::SET_ATTRIB(value.as_raw(), attrs.as_raw())
        });
        assert!(value.compact_seq().is_none());
        assert!(!is_altrep(&value));
        assert!(altrep_class(&value).is_none());
        assert_eq!(
            value
                .try_attrib()
                .unwrap()
                .try_car()
                .unwrap()
                .try_integer_elt(0)
                .unwrap(),
            123
        );
        let copy = session.with_active(|| unsafe {
            session
                .sexp(crate::mainutils::duplicate::Rf_duplicate(value.as_raw()))
                .unwrap()
        });
        collect(&session);
        assert_eq!(
            copy.try_attrib()
                .unwrap()
                .try_car()
                .unwrap()
                .try_integer_elt(0)
                .unwrap(),
            123
        );
        assert!(!is_altrep(&copy));
    }
}

#[test]
fn private_sequence_serialized_values_and_first_public_attribute_roundtrip() {
    let session = RSession::new_for_gc_tests();
    let value = sequence(&session);
    let attrs = attribute(&session, c"sequence.serialized.attribute", 53);
    session.with_active(|| unsafe {
        crate::sexp::accessors::SET_ATTRIB(value.as_raw(), attrs.as_raw())
    });
    drop(attrs);
    let bytes = session.with_active(|| unsafe {
        let nil = crate::sexp::globals::R_NilValue();
        session
            .sexp(crate::mainutils::serialize::R_serialize(
                value.as_raw(),
                nil,
                nil,
                nil,
                nil,
            ))
            .unwrap()
    });
    collect(&session);
    let copy = session.with_active(|| unsafe {
        session
            .sexp(crate::mainutils::serialize::R_unserialize(
                bytes.as_raw(),
                crate::sexp::globals::R_NilValue(),
            ))
            .unwrap()
    });
    drop(bytes);
    collect(&session);
    assert_eq!(copy.typeof_(), SEXPTYPE::INTSXP);
    assert_eq!(copy.len(), 8);
    for index in 0..8 {
        assert_eq!(copy.try_integer_elt(index).unwrap(), 3 + 2 * index as i32);
    }
    let attrs = copy.try_attrib().unwrap();
    assert_eq!(attrs.try_car().unwrap().try_integer_elt(0).unwrap(), 53);
    assert!(attrs.try_cdr().unwrap().is_nil());
    // SequenceClass has no portable serialized state: the documented fallback
    // exports dense values while retaining every actual public attribute.
    assert!(!is_altrep(&copy));
    assert!(value.compact_seq().is_some());
}

#[test]
fn private_sequence_incompatible_cache_disables_passive_formula_access() {
    let session = RSession::new_for_gc_tests();
    let value = sequence(&session);
    let NodeBody::Vector(vector) = value.header().body else {
        unreachable!()
    };
    let metadata = value.checked_child(vector.metadata.link()).unwrap();
    let wrong_shape = allocate(session.owner_token().unwrap(), SEXPTYPE::REALSXP, 1).unwrap();
    SexpMut::try_from_checked(metadata)
        .unwrap()
        .try_set_vector_elt(3, wrong_shape)
        .unwrap();
    assert!(value.compact_seq().is_none());
    // The checked provider still validates its original formula and ignores
    // a mismatched cache; no passive read treats the malformed cache as data.
    assert_eq!(value.try_integer_elt(7).unwrap(), 17);
    force_materialization(&value).unwrap();
    collect(&session);
    assert_eq!(value.try_integer_elt(7).unwrap(), 17);
}

#[test]
fn private_sequence_same_heap_generic_metadata_uses_actual_provider() {
    let session = RSession::new_for_gc_tests();
    let value = sequence(&session);
    let callbacks = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "generic.sequence.lookalike",
            UntrustedSequence(callbacks.clone()),
        )
        .unwrap();
    let state = data1(&value).unwrap();
    let generic = AltrepBuilder::new(class).data1(state).build().unwrap();
    let NodeBody::Vector(vector) = generic.header().body else {
        unreachable!()
    };
    let parent = value.allocation().unwrap();
    let heap = parent.heap_identity();
    let mut header = heap.node_snapshot(parent).unwrap();
    header
        .set_edge(EdgeField::VectorMetadata, vector.metadata.link())
        .unwrap();
    heap.replace_node(parent, header).unwrap();
    assert!(
        value.compact_seq().is_none(),
        "a private edge marker cannot authenticate a substituted generic class"
    );
    session.with_active_in(|owner| unsafe {
        crate::sexp::memory::with_arena_in(owner, |_arena| {
            let before = callbacks.get();
            assert!(value.compact_seq().is_none());
            assert_eq!(
                callbacks.get(),
                before,
                "passive admission invokes no provider"
            );
        });
    });
    assert_eq!(value.try_integer_elt(7).unwrap(), 900);
    collect(&session);
    assert_eq!(value.try_integer_elt(7).unwrap(), 900);
}

#[test]
fn private_sequence_class_permit_rejects_foreign_stale_wrong_type_and_kind() {
    let session = RSession::new_for_gc_tests();
    let value = sequence(&session);
    let descriptor = altrep_class(&value).unwrap();
    let class = descriptor.allocation().unwrap();
    let heap = class.heap_identity();
    assert!(heap.has_builtin_sequence_permit(class, SEXPTYPE::INTSXP));
    assert!(!heap.has_builtin_sequence_permit(class, SEXPTYPE::REALSXP));
    assert!(!heap.has_builtin_sequence_permit(class, SEXPTYPE::LGLSXP));
    assert!(
        heap.attach_builtin_sequence_permit(class, SEXPTYPE::REALSXP)
            .is_none()
    );
    assert!(
        heap.resource_erased(class).is_none(),
        "class permits do not widen the external-pointer API"
    );
    let other = RSession::new_for_gc_tests();
    assert!(
        !other
            .owner_token()
            .unwrap()
            .node_factory()
            .domain()
            .belongs_to(&heap)
    );
    let foreign = sequence(&other);
    let foreign_descriptor = altrep_class(&foreign).unwrap();
    let foreign_class = foreign_descriptor.allocation().unwrap();
    assert!(!heap.has_builtin_sequence_permit(foreign_class, SEXPTYPE::INTSXP));
    assert!(
        heap.attach_builtin_sequence_permit(foreign_class, SEXPTYPE::INTSXP)
            .is_none()
    );
    let dense = allocate(session.owner_token().unwrap(), SEXPTYPE::INTSXP, 8).unwrap();
    assert!(
        heap.attach_builtin_sequence_permit(dense.allocation().unwrap(), SEXPTYPE::INTSXP)
            .is_none()
    );
    let temporary_class = session.with_active(|| {
        session
            .owner_token()
            .unwrap()
            .node_factory()
            .allocate(|arena| Some(arena.alloc_node(SEXPTYPE::SYMSXP)))
            .unwrap()
    });
    let old = temporary_class.allocation().unwrap().clone();
    let old_address = temporary_class.as_raw().addr();
    heap.attach_builtin_sequence_permit(&old, SEXPTYPE::INTSXP)
        .unwrap();
    assert!(heap.has_builtin_sequence_permit(&old, SEXPTYPE::INTSXP));
    drop(temporary_class);
    collect(&session);
    assert!(!old.is_live());
    assert!(!heap.has_builtin_sequence_permit(&old, SEXPTYPE::INTSXP));
    assert!(
        heap.attach_builtin_sequence_permit(&old, SEXPTYPE::INTSXP)
            .is_none()
    );
    let reused = session.with_active(|| {
        session
            .owner_token()
            .unwrap()
            .node_factory()
            .allocate(|arena| Some(arena.alloc_node(SEXPTYPE::SYMSXP)))
            .unwrap()
    });
    assert_eq!(
        reused.as_raw().addr(),
        old_address,
        "fixture exercises actual slot reuse"
    );
    assert!(!heap.has_builtin_sequence_permit(reused.allocation().unwrap(), SEXPTYPE::INTSXP));
    assert!(!heap.has_builtin_sequence_permit(&old, SEXPTYPE::INTSXP));
    assert_eq!(value.try_integer_elt(7).unwrap(), 17);
}
