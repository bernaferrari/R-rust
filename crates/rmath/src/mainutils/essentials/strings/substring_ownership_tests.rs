use crate::sexp::{
    RSession,
    encoding::{self, TextEncoding},
    ffi::{SEXP, SEXPTYPE},
    object::{SessionNodeFactory, Sexp, SexpMut},
};
fn fixture(factory: &SessionNodeFactory<'_>, replacement: bool) -> Sexp<'static> {
    let source = factory
        .allocate(|arena| {
            encoding::allocate_character(arena, "éaβ".as_bytes(), TextEncoding::Bytes)
        })
        .unwrap();
    let mut x = SexpMut::try_from_checked(factory.strings(&[""]).unwrap()).unwrap();
    x.try_set_string_elt(0, source).unwrap();
    let integer = || {
        let mut v = SexpMut::try_from_checked(
            factory
                .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 1)))
                .unwrap(),
        )
        .unwrap();
        v.try_set_integer_elt(0, 3).unwrap();
        v.freeze()
    };
    let mut values = vec![x.freeze(), integer(), integer()];
    if replacement {
        values.push(factory.strings(&["Z"]).unwrap());
    }
    let mut args = factory.nil();
    for value in values.into_iter().rev() {
        args = factory
            .pairlist_cell(&value, &args, &factory.nil())
            .unwrap();
    }
    args.into_owned().unwrap()
}
fn constructor_callback(replacement: bool, action: u8) {
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
    unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            let args = fixture(&factory, replacement);
            let source = args
                .try_car()
                .unwrap()
                .try_string_elt(0)
                .unwrap()
                .allocation()
                .unwrap()
                .clone();
            let ledger = source.heap_identity().byte_ledger();
            let witness = ledger.clone();
            let saved = args.clone();
            let retained = source.clone();
            let called = Rc::new(Cell::new(false));
            let observed = called.clone();
            let revoker = Rc::downgrade(&facade);
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if witness.workspace_bytes_for_test() == 0 || observed.replace(true) {
                    return;
                }
                crate::sexp::accessors::SETCAR(saved.as_raw(), crate::sexp::globals::R_NilValue());
                crate::sexp::accessors::SETCDR(saved.as_raw(), crate::sexp::globals::R_NilValue());
                crate::sexp::gengc::full_gc();
                assert!(retained.is_live());
                match action {
                    1 => std::panic::panic_any(991_u32),
                    2 => drop(revoker.upgrade().unwrap().borrow_mut().take()),
                    _ => {}
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let result = catch_unwind(AssertUnwindSafe(|| {
                let invoke: unsafe fn(SEXP, SEXP, SEXP, SEXP) -> SEXP = if replacement {
                    super::nchar_substr::do_substrgets
                } else {
                    super::nchar_substr::do_substr
                };
                factory
                    .wrap(invoke(
                        factory.nil().as_raw(),
                        factory.nil().as_raw(),
                        args.as_raw(),
                        factory.nil().as_raw(),
                    ))
                    .unwrap()
                    .into_owned()
                    .unwrap()
            }));
            assert!(called.get());
            assert_eq!((*instance).memory_state.in_gc, 0);
            assert_eq!(ledger.workspace_bytes_for_test(), 0);
            match action {
                0 => {
                    let result = result.unwrap();
                    let expected = if replacement { "éZβ" } else { "a" };
                    assert!(
                        result
                            .try_string_elt(0)
                            .unwrap()
                            .try_char_eq(expected.as_bytes())
                            .unwrap()
                    );
                    crate::sexp::gengc::full_gc();
                    assert!(!source.is_live());
                    assert!(
                        result
                            .try_string_elt(0)
                            .unwrap()
                            .try_char_eq(expected.as_bytes())
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
fn owned_substring_extraction_survives_detach_and_collecting_constructor() {
    constructor_callback(false, 0);
}
#[test]
fn owned_substring_replacement_survives_detach_and_collecting_constructor() {
    constructor_callback(true, 0);
}
#[test]
fn owned_substring_constructor_panic_releases_original_workspace() {
    constructor_callback(true, 1);
}
#[test]
fn owned_substring_revocation_rejects_publication() {
    constructor_callback(true, 2);
}
#[test]
fn owned_substring_foreign_arguments_reject_before_copy_or_allocation() {
    use crate::sexp::owner::with_runtime;
    let first = RSession::new_for_gc_tests();
    let original = first.owner_token().unwrap().weak_owner().unwrap();
    let second = RSession::new_for_gc_tests();
    let foreign = fixture(&second.owner_token().unwrap().node_factory(), false);
    first.with_active(|| {
        let factory = first.owner_token().unwrap().node_factory();
        let na = factory
            .wrap(unsafe { crate::sexp::globals::R_NaString() })
            .unwrap()
            .into_owned()
            .unwrap();
        let before = first
            .owner_token()
            .unwrap()
            .with_arena(|a| a.total_bytes_allocated())
            .unwrap();
        let result = with_runtime(&original, |access| {
            super::substring_owned::evaluate(access, &foreign, &na, false)
        })
        .unwrap();
        assert_eq!(
            result.unwrap_err(),
            crate::sexp::object::SexpError::UnownedPointer {
                address: foreign.as_raw().addr(),
            }
        );
        first
            .owner_token()
            .unwrap()
            .with_arena(|a| {
                assert_eq!(a.total_bytes_allocated(), before);
                assert_eq!(a.workspace_bytes(), 0);
            })
            .unwrap();
    });
}
