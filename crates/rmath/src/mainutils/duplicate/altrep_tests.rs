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
fn class_duplicates_copy_public_attributes_deep_or_shallow_across_gc() {
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
        let descriptor = altrep::altrep_class(&source).unwrap();
        assert!(!altrep::is_materialized(&source));
        let result = session
            .sexp(unsafe { duplicate1(source.as_raw(), deep) })
            .unwrap();
        session.gc();
        assert_eq!(result.integer_elt(0), Some(73));
        assert!(!altrep::is_altrep(&result));
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
        // Ordinary atomic duplication requests DATAPTR to copy the values.
        // Expansion retains the source's class and public attribute chain.
        assert!(altrep::is_materialized(&source));
        assert!(altrep::is_altrep(&source));
        assert_eq!(altrep::altrep_class(&source).unwrap(), descriptor);
        assert_eq!(source.attrib().unwrap().cdr().unwrap(), attributes);
        assert_eq!(source.try_integer_elt(0).unwrap(), 73);
    }
}
