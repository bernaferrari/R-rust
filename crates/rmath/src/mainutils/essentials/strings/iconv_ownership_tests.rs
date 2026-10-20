use crate::sexp::{
    RSession,
    object::{SessionNodeFactory, Sexp},
};
fn fixture(factory: &SessionNodeFactory<'_>) -> Sexp<'static> {
    let values = [
        factory.strings(&["é"]).unwrap(),
        factory.strings(&["UTF-8"]).unwrap(),
        factory.strings(&["UTF-16LE"]).unwrap(),
        factory.strings(&["byte"]).unwrap(),
        factory.domain().logical(true),
        factory.domain().logical(true),
    ];
    let mut args = factory.nil();
    for value in values.into_iter().rev() {
        args = factory
            .pairlist_cell(&value, &args, &factory.nil())
            .unwrap();
    }
    args.into_owned().unwrap()
}
fn iconv_output_allocation_callback(action: u8) {
    use std::{
        cell::{Cell, RefCell},
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let weak = facade
        .borrow()
        .as_ref()
        .unwrap()
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap();
    let pin = weak.pin().unwrap();
    let instance = pin.as_ptr();
    let factory = weak.node_factory().unwrap();
    // SAFETY: pin retains the exact original instance; all arena lends finish
    // before callbacks and collection, and the callback uses checked roots.
    unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            let args = fixture(&factory);
            let input = args.try_car().unwrap().allocation().unwrap().clone();
            let saved = args.clone();
            let source = input.clone();
            let called = Rc::new(Cell::new(0));
            let count = called.clone();
            let revoker = Rc::downgrade(&facade);
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if count.replace(1) != 0 {
                    return;
                }
                (*instance).memory_state.gc_force_gap = 0;
                crate::sexp::accessors::SETCAR(saved.as_raw(), crate::sexp::globals::R_NilValue());
                crate::sexp::accessors::SETCDR(saved.as_raw(), crate::sexp::globals::R_NilValue());
                crate::sexp::gengc::full_gc();
                assert!(
                    source.is_live(),
                    "selected original component list survives argument detachment"
                );
                match action {
                    1 => std::panic::panic_any(991_u32),
                    2 => drop(revoker.upgrade().unwrap().borrow_mut().take()),
                    _ => {}
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let result = catch_unwind(AssertUnwindSafe(|| {
                factory
                    .wrap(super::encodings::do_iconv(
                        factory.nil().as_raw(),
                        factory.nil().as_raw(),
                        args.as_raw(),
                        factory.nil().as_raw(),
                    ))
                    .unwrap()
                    .into_owned()
                    .unwrap()
            }));
            assert_eq!(called.get(), 1);
            assert_eq!((*instance).memory_state.in_gc, 0);
            match action {
                0 => {
                    let result = result.unwrap();
                    let bytes = result.try_vector_elt(0).unwrap();
                    assert_eq!(bytes.try_raw_elt(0).unwrap(), 0xe9);
                    assert_eq!(bytes.try_raw_elt(1).unwrap(), 0);
                    crate::sexp::gengc::full_gc();
                    assert!(!input.is_live());
                    assert_eq!(
                        result.try_vector_elt(0).unwrap().try_raw_elt(0).unwrap(),
                        0xe9
                    );
                }
                1 => assert_eq!(*result.unwrap_err().downcast::<u32>().unwrap(), 991),
                2 => {
                    assert!(!weak.is_live());
                    assert!(
                        result
                            .unwrap_err()
                            .downcast_ref::<crate::sexp::context::RError>()
                            .is_some()
                    );
                }
                _ => unreachable!(),
            }
        });
    }
}
#[test]
fn iconv_owned_input_survives_detach_gc_then_releases() {
    iconv_output_allocation_callback(0);
}
#[test]
fn iconv_owned_live_callback_panic_preserves_original_payload() {
    iconv_output_allocation_callback(1);
}
#[test]
fn iconv_owned_revoked_original_cannot_publish() {
    iconv_output_allocation_callback(2);
}
