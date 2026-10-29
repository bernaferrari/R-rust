#![allow(unsafe_code)]
use super::*;
use crate::sexp::accessors::{S4_OBJECT_MASK, SET_OBJECT, SET_S4_OBJECT, UNSET_S4_OBJECT};
use std::cell::Cell;

struct CollectingFlags(Rc<Cell<usize>>);
impl AltrepClass for CollectingFlags {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::INTSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(2)
    }
    fn element<'s>(
        &self,
        context: &AltrepContext<'s>,
        index: i64,
    ) -> SexpResult<AltrepElement<'s>> {
        self.0.set(self.0.get() + 1);
        context.gc()?;
        Ok(AltrepElement::Integer(41 + index as i32))
    }
}

#[test]
fn attribute_free_dense_copy_preserves_flags_through_provider_gc_and_isolation() {
    for object_bit in [0, 1] {
        let session = RSession::new_for_gc_tests();
        let callbacks = Rc::new(Cell::new(0));
        let class = session
            .register_altrep_class("public.flags", CollectingFlags(callbacks.clone()))
            .unwrap();
        let source = AltrepBuilder::new(class).build().unwrap();
        unsafe {
            SET_S4_OBJECT(source.as_raw());
            SET_OBJECT(source.as_raw(), object_bit);
        }
        assert!(source.try_attrib().unwrap().is_nil());
        let descriptor = altrep_class(&source).unwrap();
        let result = materialized_copy(&source).unwrap();
        session.gc();
        assert_ne!(result.header().sxpinfo.gp() & S4_OBJECT_MASK, 0);
        assert_eq!(i32::from(result.header().sxpinfo.obj()), object_bit);
        assert!(result.try_attrib().unwrap().is_nil());
        assert!(!is_altrep(&result));
        assert_eq!(callbacks.get(), 2);
        unsafe {
            UNSET_S4_OBJECT(result.as_raw());
            SET_OBJECT(result.as_raw(), 1 - object_bit);
        }
        let mut result = SexpMut::try_from_checked(result).unwrap();
        result.try_set_integer_elt(0, 999).unwrap();
        assert_eq!(source.try_integer_elt(0).unwrap(), 41);
        assert_ne!(source.header().sxpinfo.gp() & S4_OBJECT_MASK, 0);
        assert_eq!(i32::from(source.header().sxpinfo.obj()), object_bit);
        assert_eq!(altrep_class(&source).unwrap(), descriptor);
        assert!(!is_materialized(&source));
    }
}

#[test]
fn plain_dense_copy_does_not_acquire_flags_and_closed_owner_runs_no_callback() {
    let mut session = RSession::new_for_gc_tests();
    let callbacks = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class("public.plain", CollectingFlags(callbacks.clone()))
        .unwrap();
    let source = AltrepBuilder::new(class)
        .build()
        .unwrap()
        .into_owned()
        .unwrap();
    let result = materialized_copy(&source).unwrap();
    assert_eq!(result.header().sxpinfo.gp() & S4_OBJECT_MASK, 0);
    assert_eq!(i32::from(result.header().sxpinfo.obj()), 0);
    drop(result);
    session.close();
    let before = callbacks.get();
    assert!(materialized_copy(&source).is_err());
    assert_eq!(callbacks.get(), before);
}
