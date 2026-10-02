use super::*;
use crate::sexp::{
    altrep::{self, AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    object::{Sexp, SexpResult},
    session::RSession,
};

struct CollectingInteger;
impl AltrepClass for CollectingInteger {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::INTSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(1)
    }
    fn element<'s>(&self, context: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
        context.gc()?;
        Ok(AltrepElement::Integer(73))
    }
}

fn lazy<'s>(session: &'s RSession, name: &str) -> Sexp<'s> {
    let class = session
        .register_altrep_class(name, CollectingInteger)
        .unwrap();
    AltrepBuilder::new(class).build().unwrap()
}

fn scalar(session: &RSession, value: i32) -> Sexp<'_> {
    session
        .sexp(unsafe { crate::sexp::constructors::Rf_ScalarInteger(value) })
        .unwrap()
}

fn public_attribute<'s>(session: &'s RSession, value: &Sexp<'s>) -> Sexp<'s> {
    let attributes = session
        .sexp(unsafe { Rf_cons(value.clone().as_raw(), R_NilValue()) })
        .unwrap();
    unsafe {
        SETTAG(
            attributes.clone().as_raw(),
            crate::sexp::symbol::Rf_install(c"custom".as_ptr()),
        );
    }
    attributes
}

unsafe fn attach(object: &Sexp<'_>, attributes: &Sexp<'_>) {
    let raw = object.clone().as_raw();
    if unsafe { altrep::has_extension_raw(raw) } {
        unsafe { SETCDR(ATTRIB(raw), attributes.clone().as_raw()) };
    } else {
        unsafe { SET_ATTRIB(raw, attributes.clone().as_raw()) };
    }
}

#[test]
fn legacy_duplicate_attributes_copy_deep_or_shallow_and_root_callback_results() {
    for deep in [0, 1] {
        let session = RSession::new_for_gc_tests();
        let source = lazy(&session, "attribute-source");
        let attribute_child = lazy(&session, "collecting-attribute-child");
        let container = session
            .sexp(unsafe { Rf_allocVector3(SEXPTYPE::VECSXP, 1) })
            .unwrap();
        unsafe {
            SET_VECTOR_ELT(
                container.clone().as_raw(),
                0,
                attribute_child.clone().as_raw(),
            )
        };
        let attributes = public_attribute(&session, &container);
        unsafe {
            attach(&source, &attributes);
            SET_OBJECT(source.clone().as_raw(), 1);
            SET_S4_OBJECT(source.clone().as_raw());
        }
        // Deliberately return an unrooted freshly allocated callback result.
        // Deep copying the nested lazy attribute collects while copying it.
        let result = unsafe { crate::sexp::constructors::Rf_ScalarInteger(18) };
        unsafe { altrep_duplicate_attributes(result, source.clone().as_raw(), deep) };
        let result = session.sexp(result).unwrap();
        session.gc();
        assert_eq!(result.integer_elt(0), Some(18));
        let copied_attributes = result.attrib().unwrap();
        assert_ne!(copied_attributes, attributes);
        let copied_container = copied_attributes.car().unwrap();
        if deep == 0 {
            assert_eq!(copied_container, container);
        } else {
            assert_ne!(copied_container, container);
            assert_eq!(
                copied_container.vector_elt(0).unwrap().integer_elt(0),
                Some(73)
            );
        }
        unsafe {
            assert_eq!(OBJECT(result.clone().as_raw()), 1);
            assert_ne!(IS_S4_OBJECT(result.clone().as_raw()), 0);
        }
        assert!(!altrep::is_materialized(&source));
    }
}

#[test]
fn legacy_duplicate_attributes_preserve_target_lazy_metadata() {
    let session = RSession::new_for_gc_tests();
    let source = lazy(&session, "source");
    let target = lazy(&session, "target");
    let source_data = scalar(&session, 91);
    let target_data = scalar(&session, 27);
    altrep::set_data1(&source, source_data).unwrap();
    altrep::set_data1(&target, target_data.clone()).unwrap();
    let attributes = public_attribute(&session, &scalar(&session, 7));
    unsafe {
        attach(&source, &attributes);
        altrep_duplicate_attributes(target.clone().as_raw(), source.clone().as_raw(), 1);
    }
    session.gc();
    assert_eq!(altrep::data1(&target).unwrap(), target_data);
    assert!(!altrep::is_materialized(&target));
    assert_eq!(
        target
            .attrib()
            .unwrap()
            .cdr()
            .unwrap()
            .car()
            .unwrap()
            .integer_elt(0),
        Some(7)
    );
    assert_eq!(target.integer_elt(0), Some(73));
}

#[test]
fn legacy_duplicate_attributes_clear_public_state_and_leave_same_object_unchanged() {
    let session = RSession::new_for_gc_tests();
    let source = lazy(&session, "source");
    let target = lazy(&session, "target");
    let attributes = public_attribute(&session, &scalar(&session, 9));
    unsafe {
        attach(&target, &attributes);
        SET_OBJECT(target.clone().as_raw(), 1);
        SET_S4_OBJECT(target.clone().as_raw());
        altrep_duplicate_attributes(target.clone().as_raw(), target.clone().as_raw(), 1);
    }
    assert_eq!(target.attrib().unwrap().cdr().unwrap(), attributes);
    unsafe {
        altrep_duplicate_attributes(target.clone().as_raw(), source.clone().as_raw(), 1);
        assert_eq!(OBJECT(target.clone().as_raw()), 0);
        assert_eq!(IS_S4_OBJECT(target.clone().as_raw()), 0);
    }
    assert!(target.attrib().unwrap().cdr().unwrap().is_nil());
    assert!(!altrep::is_materialized(&target));
    assert_eq!(target.integer_elt(0), Some(73));
}
