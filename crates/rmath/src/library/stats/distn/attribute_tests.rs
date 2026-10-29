use super::*;
use crate::sexp::{RSession, Sexp, SexpMut};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
fn reals(session: &RSession, values: &[f64]) -> Sexp<'static> {
    let owner = session.owner_token().unwrap();
    let value = owner
        .node_factory()
        .allocate(|a| {
            a.alloc_vector_sexp(SEXPTYPE::REALSXP, values.len() as i64)
                .map(|s| s.as_raw())
        })
        .unwrap()
        .into_owned()
        .unwrap();
    let mut value = SexpMut::try_from_checked(value).unwrap();
    for (i, &x) in values.iter().enumerate() {
        value.try_set_real_elt(i as i64, x).unwrap();
    }
    value.freeze()
}
fn note(session: &RSession, source: &Sexp<'_>, child: &Sexp<'_>) {
    session.with_active(|| unsafe {
        crate::attrib_core::setAttrib(
            source.as_raw(),
            crate::sexp::symbol::Rf_install(c"note".as_ptr()),
            child.as_raw(),
        )
    });
}
#[test]
fn registered_attribute_graph_is_shallow_ordered_and_survives_source_release() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let source = reals(&session, &[0.0, 1.0]);
    let child = reals(&session, &[19.0, 23.0]);
    note(&session, &source, &child);
    session.with_active(|| unsafe {
        let class = Rf_mkString(c"dpq.contract".as_ptr());
        let _class = protect(class);
        crate::attrib_core::setAttrib(source.as_raw(), crate::attrib_core::R_ClassSymbol(), class);
        SET_S4_OBJECT(source.as_raw());
    });
    let zero = reals(&session, &[0.0]);
    let one = reals(&session, &[1.0]);
    let result = session.with_active(|| {
        let routine = lookup_call("pnorm").unwrap();
        let raw = unsafe {
            routine
                .invoke_call(&[
                    source.as_raw(),
                    zero.as_raw(),
                    one.as_raw(),
                    one.as_raw(),
                    zero.as_raw(),
                ])
                .unwrap()
        };
        owner.sexp(raw).unwrap().into_owned().unwrap()
    });
    let mut a = source.try_attrib().unwrap();
    let mut b = result.try_attrib().unwrap();
    while !a.is_nil() {
        assert_ne!(a.as_raw(), b.as_raw());
        assert_eq!(a.try_tag().unwrap(), b.try_tag().unwrap());
        assert_eq!(a.try_car().unwrap(), b.try_car().unwrap());
        a = a.try_cdr().unwrap();
        b = b.try_cdr().unwrap();
    }
    assert!(b.is_nil());
    session.with_active(|| unsafe {
        assert_eq!(OBJECT(result.as_raw()), 1);
        assert_ne!(crate::mainutils::coerce::IS_S4_OBJECT(result.as_raw()), 0);
    });
    drop((a, b, source, child));
    session.with_active(|| unsafe { crate::sexp::gengc::full_gc() });
    let value = session.with_active(|| unsafe {
        owner
            .sexp(crate::attrib_core::getAttrib(
                result.as_raw(),
                crate::sexp::symbol::Rf_install(c"note".as_ptr()),
            ))
            .unwrap()
            .into_owned()
            .unwrap()
    });
    assert_eq!(value.try_real_elt(1), Ok(23.0));
    session.with_active(|| unsafe {
        copy_math_attributes(owner, result.as_raw(), zero.as_raw());
        assert!(ATTRIB(result.as_raw()) == R_NilValue());
        assert_eq!(OBJECT(result.as_raw()), 0);
        assert_eq!(crate::mainutils::coerce::IS_S4_OBJECT(result.as_raw()), 0);
    });
}
#[test]
fn attribute_snapshot_survives_detachment_during_copy_collection() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let factory = owner.node_factory();
    let source = reals(&session, &[0.0]);
    let target = reals(&session, &[1.0]);
    let child = reals(&session, &[47.0]);
    note(&session, &source, &child);
    let child_node = child.allocation().unwrap().clone();
    let source_node = source.allocation().unwrap().clone();
    let heap = source_node.heap_identity();
    let nil = factory.link(&factory.nil()).unwrap();
    drop(child);
    let called = Rc::new(Cell::new(false));
    let observed = called.clone();
    session.with_active(|| {
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
            assert!(child_node.is_live());
        }))
    });
    session.with_active_in(|owner| unsafe {
        (*owner).memory_state.gc_force_gap = 1;
        (*owner).memory_state.gc_force_wait = 1;
    });
    session
        .with_active(|| unsafe { copy_math_attributes(owner, target.as_raw(), source.as_raw()) });
    assert!(called.get());
    assert!(source.try_attrib().unwrap().is_nil());
    let attributes = target.try_attrib().unwrap();
    assert!(attributes.try_tag_name_eq(b"note").unwrap());
    assert_eq!(attributes.try_car().unwrap().try_real_elt(0), Ok(47.0));
}
#[test]
fn owner_revocation_during_attribute_copy_prevents_publication() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let weak = owner.weak_owner().unwrap();
    let source = reals(&session, &[0.0]);
    let target = reals(&session, &[1.0]);
    let child = reals(&session, &[59.0]);
    note(&session, &source, &child);
    let facade = Rc::new(RefCell::new(None));
    let closer = facade.clone();
    let called = Rc::new(Cell::new(false));
    let observed = called.clone();
    session.with_active(|| {
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            if observed.replace(true) {
                return;
            }
            unsafe {
                (*crate::sexp::owner::OwnerToken::current().unwrap().as_ptr())
                    .memory_state
                    .gc_force_gap = 0;
            }
            closer
                .borrow_mut()
                .as_mut()
                .map(|session: &mut RSession| session.close());
        }))
    });
    session.with_active_in(|owner| unsafe {
        (*owner).memory_state.gc_force_gap = 1;
        (*owner).memory_state.gc_force_wait = 1;
    });
    *facade.borrow_mut() = Some(session);
    let published = Cell::new(false);
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::sexp::owner::with_runtime(&weak, |access| {
            access.with_native(|owner| {
                unsafe { copy_math_attributes(owner, target.as_raw(), source.as_raw()) };
                published.set(true);
                Ok(())
            })
        })
    }));
    assert!(called.get());
    assert!(!published.get());
    match outcome {
        Err(_) => {}
        Ok(value) => assert!(value.is_err()),
    };
    drop(facade.borrow_mut().take());
}

#[test]
fn attribute_copy_budget_denial_leaves_target_unpublished_and_retry_recovers() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let source = reals(&session, &[0.0]);
    let target = reals(&session, &[1.0]);
    let child = reals(&session, &[61.0]);
    note(&session, &source, &child);
    session
        .with_active(|| unsafe { copy_math_attributes(owner, target.as_raw(), source.as_raw()) });
    let retained = target.try_attrib().unwrap().into_owned().unwrap();
    session.with_active(|| unsafe {
        SET_ATTRIB(target.as_raw(), R_NilValue());
        crate::sexp::gengc::full_gc();
    });
    let (bytes, nodes) = owner
        .with_arena(|a| (a.total_bytes_allocated(), a.node_count()))
        .unwrap();
    owner
        .with_arena(|a| a.set_budget(crate::sexp::memory::ArenaBudget::new(bytes, nodes)))
        .unwrap();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        session.with_active(|| unsafe {
            copy_math_attributes(owner, target.as_raw(), source.as_raw())
        })
    }));
    assert!(
        outcome.is_err(),
        "a fresh attribute cell requires node admission"
    );
    assert!(target.try_attrib().unwrap().is_nil());
    owner
        .with_arena(|a| a.set_budget(crate::sexp::memory::ArenaBudget::unlimited()))
        .unwrap();
    session
        .with_active(|| unsafe { copy_math_attributes(owner, target.as_raw(), source.as_raw()) });
    assert_eq!(
        target
            .try_attrib()
            .unwrap()
            .try_car()
            .unwrap()
            .try_real_elt(0),
        Ok(61.0)
    );
    drop(retained);
}
