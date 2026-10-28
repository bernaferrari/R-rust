use super::*;
use crate::sexp::session::RSession;
use std::{cell::Cell, rc::Rc};

#[test]
fn coercion_attributes_keep_detached_nested_values_rooted_across_gc() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let source = factory.wrap(unsafe { Rf_ScalarInteger(1) }).unwrap();
    let result = factory.wrap(unsafe { Rf_ScalarReal(1.) }).unwrap();
    let value = factory.wrap(unsafe { Rf_ScalarInteger(81) }).unwrap();
    let value_node = value.allocation().unwrap().clone();
    let nested = factory
        .wrap(unsafe { Rf_allocVector3(SEXPTYPE::VECSXP, 1) })
        .unwrap();
    unsafe { SET_VECTOR_ELT(nested.as_raw(), 0, value.as_raw()) };
    let nested_node = nested.allocation().unwrap().clone();
    let tag = factory
        .wrap(unsafe { Rf_install(c"coercion.custom".as_ptr()) })
        .unwrap();
    let attributes = factory
        .pairlist_cell(&nested, &factory.nil(), &tag)
        .unwrap();
    unsafe { SET_ATTRIB(source.as_raw(), attributes.as_raw()) };
    drop((attributes, nested, value, tag));
    let source_node = source.allocation().unwrap().clone();
    let heap = source_node.heap_identity();
    let nil = factory.link(&factory.nil()).unwrap();
    let notified = Rc::new(Cell::new(false));
    let observed = notified.clone();
    crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
        if observed.replace(true) {
            return;
        }
        unsafe {
            (*crate::sexp::owner::OwnerToken::current().unwrap().as_ptr())
                .memory_state
                .gc_force_gap = 0;
        }
        let mut header = heap.node_snapshot(&source_node).unwrap();
        header.attrib = nil;
        heap.replace_node(&source_node, header).unwrap();
        crate::sexp::gengc::full_gc();
        assert!(nested_node.is_live());
        assert!(value_node.is_live());
    }));
    session.with_active_in(|owner| unsafe {
        (*owner).memory_state.gc_force_gap = 1;
        (*owner).memory_state.gc_force_wait = 1;
    });
    let protection = crate::sexp::protect::R_ProtectCount();
    unsafe { SHALLOW_DUPLICATE_ATTRIB(result.as_raw(), source.as_raw()) };
    assert!(notified.get());
    assert_eq!(crate::sexp::protect::R_ProtectCount(), protection);
    assert!(source.try_attrib().unwrap().is_nil());
    let copied = result.try_attrib().unwrap();
    assert!(copied.try_tag_name_eq(b"coercion.custom").unwrap());
    assert_eq!(
        copied
            .try_car()
            .unwrap()
            .try_vector_elt(0)
            .unwrap()
            .try_integer_elt(0)
            .unwrap(),
        81
    );
}

#[test]
fn coercion_attributes_reject_foreign_source_before_publication() {
    let original = RSession::new_for_gc_tests();
    let source = original.with_active(|| {
        let factory = original.owner_token().unwrap().node_factory();
        let source = factory.wrap(unsafe { Rf_ScalarInteger(1) }).unwrap();
        let value = factory.wrap(unsafe { Rf_ScalarInteger(81) }).unwrap();
        let tag = factory
            .wrap(unsafe { Rf_install(c"coercion.custom".as_ptr()) })
            .unwrap();
        let attributes = factory.pairlist_cell(&value, &factory.nil(), &tag).unwrap();
        unsafe { SET_ATTRIB(source.as_raw(), attributes.as_raw()) };
        source
    });
    let other = RSession::new_for_gc_tests();
    other.with_active(|| {
        let factory = other.owner_token().unwrap().node_factory();
        let result = factory.wrap(unsafe { Rf_ScalarReal(1.) }).unwrap();
        let protection = crate::sexp::protect::R_ProtectCount();
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            SHALLOW_DUPLICATE_ATTRIB(result.as_raw(), source.as_raw())
        }))
        .unwrap_err();
        let message = error
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| error.downcast_ref::<&str>().copied())
            .unwrap();
        assert!(message.contains("different owner"), "{message}");
        assert!(result.try_attrib().unwrap().is_nil());
        assert_eq!(crate::sexp::protect::R_ProtectCount(), protection);
    });
}
