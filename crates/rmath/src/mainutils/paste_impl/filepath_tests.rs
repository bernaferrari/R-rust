use crate::sexp::{
    RSession,
    encoding::{TextEncoding, allocate_character},
    ffi::SEXPTYPE,
    object::{SessionNodeFactory, Sexp, SexpMut},
};

fn fixture(factory: &SessionNodeFactory<'_>, second: TextEncoding) -> Sexp<'static> {
    let mut inputs = SexpMut::try_from_checked(
        factory
            .allocate(|a| Some(a.alloc_vector(SEXPTYPE::VECSXP, 2)))
            .unwrap(),
    )
    .unwrap();
    for (i, bytes, encoding) in [
        (0, "é".as_bytes(), TextEncoding::Utf8),
        (1, &[0xe9][..], second),
    ] {
        let character = factory
            .allocate(|a| allocate_character(a, bytes, encoding))
            .unwrap();
        let mut vector = SexpMut::try_from_checked(
            factory
                .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 1)))
                .unwrap(),
        )
        .unwrap();
        vector.try_set_string_elt(0, character).unwrap();
        inputs.try_set_vector_elt(i, vector.freeze()).unwrap();
    }
    let separator = factory.strings(&["/"]).unwrap();
    let tail = factory
        .pairlist_cell(&separator, &factory.nil(), &factory.nil())
        .unwrap();
    factory
        .pairlist_cell(&inputs.freeze(), &tail, &factory.nil())
        .unwrap()
        .into_owned()
        .unwrap()
}
#[test]
fn owned_filepath_native_bridge_converts_latin1_before_publishing_utf8() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let factory = session.owner_token().unwrap().node_factory();
        let args = fixture(&factory, TextEncoding::Latin1);
        // SAFETY: the factory and owning argument graph retain every input
        // for the actual native entry; the returned node is wrapped immediately.
        let output = factory
            .wrap(unsafe {
                super::do_filepath(
                    factory.nil().as_raw(),
                    factory.nil().as_raw(),
                    args.as_raw(),
                    factory.nil().as_raw(),
                )
            })
            .unwrap();
        let character = output.try_string_elt(0).unwrap();
        assert!(character.try_char_eq("é/é".as_bytes()).unwrap());
        assert!(crate::sexp::encoding::flags(&character).unwrap().utf8);
    });
}
#[test]
fn owned_filepath_native_bridge_rejects_byte_components_with_gnu_error() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let factory = session.owner_token().unwrap().node_factory();
        let args = fixture(&factory, TextEncoding::Bytes);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::do_filepath(
                factory.nil().as_raw(),
                factory.nil().as_raw(),
                args.as_raw(),
                factory.nil().as_raw(),
            )
        }));
        let error = outcome.expect_err("GNU rejects byte-tagged path components");
        let error = error
            .downcast_ref::<crate::sexp::context::RError>()
            .unwrap();
        assert_eq!(
            error.message,
            "strings with \"bytes\" encoding are not allowed"
        );
    });
}

fn output_allocation_callback(action: u8) {
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
            let args = fixture(&factory, TextEncoding::Latin1);
            let input = args.try_car().unwrap().allocation().unwrap().clone();
            let separator = args
                .try_cdr()
                .unwrap()
                .try_car()
                .unwrap()
                .try_string_elt(0)
                .unwrap()
                .allocation()
                .unwrap()
                .clone();
            let saved = args.clone();
            let selected = separator.clone();
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
                let tail = saved.try_cdr().unwrap();
                crate::sexp::accessors::SETCAR(tail.as_raw(), crate::sexp::globals::R_NilValue());
                crate::sexp::gengc::full_gc();
                assert!(
                    source.is_live(),
                    "selected original component list survives argument detachment"
                );
                assert!(
                    selected.is_live(),
                    "selected original separator survives argument detachment"
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
                    .wrap(super::do_filepath(
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
                    assert!(
                        result
                            .try_string_elt(0)
                            .unwrap()
                            .try_char_eq("é/é".as_bytes())
                            .unwrap()
                    );
                    crate::sexp::gengc::full_gc();
                    assert!(!input.is_live());
                    assert!(!separator.is_live());
                    assert!(
                        result
                            .try_string_elt(0)
                            .unwrap()
                            .try_char_eq("é/é".as_bytes())
                            .unwrap()
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
fn owned_filepath_selected_components_and_separator_survive_detach_gc_then_release() {
    output_allocation_callback(0);
}
#[test]
fn owned_filepath_live_callback_panic_preserves_original_payload() {
    output_allocation_callback(1);
}
#[test]
fn owned_filepath_revoked_original_cannot_publish() {
    output_allocation_callback(2);
}
