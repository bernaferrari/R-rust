//! GNU promise wire format: deferred bindings remain deferred and retain their environment.
use super::*;
use crate::sexp::{
    object::{SessionNodeFactory, Sexp, SexpMut},
    session::RSession,
};

const GNU_ENV: &[u8] = include_bytes!("fixtures/gnu-promise-environment-v2.rds");

fn raw<'s>(factory: &SessionNodeFactory<'s>, bytes: &[u8]) -> Sexp<'s> {
    let value = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::RAWSXP, bytes.len() as R_xlen_t)))
        .unwrap();
    let mut value = SexpMut::try_from_checked(value).unwrap();
    for (index, byte) in bytes.iter().copied().enumerate() {
        value.try_set_raw_elt(index as R_xlen_t, byte).unwrap();
    }
    value.freeze()
}

fn binding<'s>(factory: &SessionNodeFactory<'s>, environment: &Sexp<'s>, name: &CStr) -> Sexp<'s> {
    let symbol = factory.wrap(unsafe { Rf_install(name.as_ptr()) }).unwrap();
    unsafe { crate::sexp::envir::find_var_in_frame_result(environment.clone(), symbol) }
        .unwrap()
        .unwrap()
}

#[test]
fn gnu_serialized_promises_preserve_deferred_cached_and_cyclic_environment_fields() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let input = raw(&factory, GNU_ENV);
        let environment = factory
            .wrap(unsafe { R_unserialize(input.as_raw(), R_NilValue()) })
            .unwrap();
        let lazy = binding(&factory, &environment, c"lazy");
        let cached = binding(&factory, &environment, c"cached");
        assert_eq!(lazy.typeof_(), SEXPTYPE::PROMSXP);
        assert_eq!(lazy.try_prvalue().unwrap(), factory.unbound());
        assert_eq!(lazy.try_prenv().unwrap(), environment);
        assert_eq!(binding(&factory, &environment, c"self"), environment);
        assert_eq!(cached.typeof_(), SEXPTYPE::PROMSXP);
        assert_eq!(cached.try_prvalue().unwrap().try_integer_elt(0).unwrap(), 9);
        assert_eq!(cached.try_prenv().unwrap(), factory.nil());
        session.owner_token().unwrap().full_gc().unwrap();
        assert!(lazy.allocation().unwrap().is_live());
        let value = unsafe { crate::sexp::envir::force_promise_result(lazy.clone()) }
            .unwrap()
            .unwrap();
        assert_eq!(value.try_integer_elt(0).unwrap(), 7);
        assert_eq!(lazy.try_prvalue().unwrap(), value);
        assert_eq!(lazy.try_prenv().unwrap(), factory.nil());
    });
}

#[test]
fn serialized_promise_writer_roundtrips_without_forcing_the_original() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let factory = SessionNodeFactory::new(session.owner_token().unwrap());
        let expression = factory.wrap(unsafe { Rf_ScalarInteger(23) }).unwrap();
        let promise = factory.promise(&expression, &factory.nil()).unwrap();
        let mut writer = BinaryWriter::new();
        unsafe { WriteItemInternal(promise.as_raw(), &mut WriteHashTable::new(), &mut writer) };
        assert_eq!(promise.try_prvalue().unwrap(), factory.unbound());
        let bytes = writer.into_vec();
        let mut reader = BinaryReader::new(&bytes);
        let result = unsafe { ReadItemInternal(&mut reader, &mut ReadRefTable::new()) }.unwrap();
        let result = factory.wrap(result).unwrap();
        assert_eq!(result.typeof_(), SEXPTYPE::PROMSXP);
        assert_eq!(result.try_prvalue().unwrap(), factory.unbound());
        assert_eq!(result.try_prcode().unwrap().try_integer_elt(0).unwrap(), 23);
        assert_eq!(reader.pos, bytes.len());
        session.owner_token().unwrap().full_gc().unwrap();
        assert_eq!(result.try_prvalue().unwrap(), factory.unbound());
    });
}

fn decode_callback_case(action: u8) {
    use std::{
        cell::{Cell, RefCell},
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let (input, weak) = {
        let facade = facade.borrow();
        let session = facade.as_ref().unwrap();
        session.with_active(|| {
            let owner = session.owner_token().unwrap();
            (
                raw(&owner.node_factory(), GNU_ENV).into_owned().unwrap(),
                owner.weak_owner().unwrap(),
            )
        })
    };
    let pin = weak.pin().unwrap();
    let instance = pin.as_ptr();
    let observed = Rc::new(Cell::new(0));
    let observed_callback = observed.clone();
    let callback_facade = Rc::downgrade(&facade);
    let callback_input = input.clone();
    let outcome = catch_unwind(AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if observed_callback.get() != 0 {
                    return;
                }
                observed_callback.set(1);
                (*instance).memory_state.gc_force_gap = 0;
                match action {
                    0 => {
                        let mut input = SexpMut::try_from_checked(callback_input.clone()).unwrap();
                        input.try_set_raw_elt(0, b'Q').unwrap();
                        crate::sexp::gengc::full_gc();
                    }
                    1 => std::panic::panic_any(193_u64),
                    _ => drop(callback_facade.upgrade().unwrap().borrow_mut().take()),
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let result = R_unserialize(input.as_raw(), R_NilValue());
            weak.node_factory()
                .unwrap()
                .wrap(result)
                .unwrap()
                .into_owned()
                .unwrap()
        })
    }));
    assert_eq!(
        observed.get(),
        1,
        "actual deserialization allocation must run callback"
    );
    assert_eq!(unsafe { (*instance).memory_state.in_gc }, 0);
    match action {
        0 => {
            let result = outcome.unwrap();
            assert_eq!(input.try_raw_elt(0).unwrap(), b'Q');
            unsafe {
                crate::sexp::session::with_instance_active(instance, || {
                    let factory = weak.node_factory().unwrap();
                    let lazy = binding(&factory, &result, c"lazy");
                    assert_eq!(lazy.try_prvalue().unwrap(), factory.unbound());
                    assert_eq!(lazy.try_prenv().unwrap(), result);
                    session_gc(&weak);
                    assert_eq!(lazy.try_prcode().unwrap().typeof_(), SEXPTYPE::SYMSXP);
                });
            }
        }
        1 => {
            assert_eq!(*outcome.unwrap_err().downcast::<u64>().unwrap(), 193);
        }
        _ => {
            let error = outcome.unwrap_err();
            assert_eq!(
                error
                    .downcast_ref::<crate::sexp::context::RError>()
                    .unwrap()
                    .message,
                crate::sexp::object::SexpError::RootUnavailable.to_string()
            );
            assert!(!weak.is_live());
            assert!(input.allocation().unwrap().is_live());
        }
    }
    // The callback uses a weak facade; the physical operation pin remains
    // authoritative through cleanup, and no facade/value cycle is retained.
    drop(facade.borrow_mut().take());
}
fn session_gc(owner: &crate::sexp::owner::WeakOwner) {
    crate::sexp::owner::with_runtime(owner, |access| access.with_native(|owner| owner.full_gc()))
        .unwrap()
        .unwrap();
}
#[test]
fn promise_decode_owns_bytes_and_fields_through_collecting_mutating_callback() {
    decode_callback_case(0);
}
#[test]
fn promise_decode_preserves_original_live_callback_panic() {
    decode_callback_case(1);
}
#[test]
fn promise_decode_refuses_publication_after_original_facade_revocation() {
    decode_callback_case(2);
}

#[test]
fn promise_roundtrip_retains_full_gp_and_sole_attribute_edge_through_gc() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let factory = session.owner_token().unwrap().node_factory();
        let expression = factory.wrap(unsafe { Rf_ScalarInteger(29) }).unwrap();
        let promise = factory.promise(&expression, &factory.nil()).unwrap();
        let tag = factory
            .wrap(unsafe { Rf_install(c"promise.marker".as_ptr()) })
            .unwrap();
        let marker = factory.wrap(unsafe { Rf_ScalarInteger(54) }).unwrap();
        let attributes = factory
            .pairlist_cell(&marker, &factory.nil(), &tag)
            .unwrap();
        let node = promise.allocation().unwrap();
        let heap = node.heap_identity();
        let mut header = heap.node_snapshot(node).unwrap();
        header.attrib = factory.link(&attributes).unwrap();
        header.sxpinfo.set_gp(2);
        heap.replace_node(node, header).unwrap();
        drop(attributes);
        drop(marker);
        drop(tag);
        session.owner_token().unwrap().full_gc().unwrap();
        let mut writer = BinaryWriter::new();
        unsafe { WriteItemInternal(promise.as_raw(), &mut WriteHashTable::new(), &mut writer) };
        let bytes = writer.into_vec();
        let mut reader = BinaryReader::new(&bytes);
        let decoded = factory
            .wrap(unsafe { ReadItemInternal(&mut reader, &mut ReadRefTable::new()) }.unwrap())
            .unwrap();
        assert_eq!(decoded.header().sxpinfo.gp(), 2);
        assert_eq!(decoded.try_prvalue().unwrap(), factory.unbound());
        drop(promise);
        drop(expression);
        session.owner_token().unwrap().full_gc().unwrap();
        assert_eq!(
            decoded
                .try_attrib()
                .unwrap()
                .try_car()
                .unwrap()
                .try_integer_elt(0)
                .unwrap(),
            54
        );
        assert_eq!(
            decoded.try_prcode().unwrap().try_integer_elt(0).unwrap(),
            29
        );
        assert_eq!(reader.pos, bytes.len());
    });
}
