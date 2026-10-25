use super::vector_text_owned::Operation;
use crate::sexp::{
    RSession,
    encoding::{self, TextEncoding},
    ffi::SEXPTYPE,
    object::{SessionNodeFactory, Sexp, SexpMut},
};
fn fixture(factory: &SessionNodeFactory<'_>, operation: Operation) -> Sexp<'static> {
    let (text, tag) = match operation {
        Operation::Count | Operation::Trim => ("e\u{301}雪", TextEncoding::Utf8),
        Operation::Repeat => ("é", TextEncoding::Bytes),
        _ => ("éa", TextEncoding::Utf8),
    };
    let scalar = factory
        .allocate(|a| encoding::allocate_character(a, text.as_bytes(), tag))
        .unwrap();
    let mut x = SexpMut::try_from_checked(factory.strings(&[""]).unwrap()).unwrap();
    x.try_set_string_elt(0, scalar).unwrap();
    let parameter = match operation {
        Operation::Count => factory.strings(&["chars"]).unwrap(),
        Operation::Prefix => factory.strings(&["é"]).unwrap(),
        Operation::Suffix => factory.strings(&["a"]).unwrap(),
        Operation::Trim | Operation::Repeat => {
            let mut v = SexpMut::try_from_checked(
                factory
                    .allocate(|a| Some(a.alloc_vector(SEXPTYPE::REALSXP, 1)))
                    .unwrap(),
            )
            .unwrap();
            v.try_set_real_elt(
                0,
                if matches!(operation, Operation::Trim) {
                    1.0
                } else {
                    2.0
                },
            )
            .unwrap();
            v.freeze()
        }
    };
    let mut args = factory.nil();
    for value in [x.freeze(), parameter].into_iter().rev() {
        args = factory
            .pairlist_cell(&value, &args, &factory.nil())
            .unwrap();
    }
    args.into_owned().unwrap()
}
fn callback(operation: Operation, action: u8) {
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
            let args = fixture(&factory, operation);
            let source = args
                .try_car()
                .unwrap()
                .try_string_elt(0)
                .unwrap()
                .allocation()
                .unwrap()
                .clone();
            let ledger = source.heap_identity().byte_ledger();
            let saved = args.clone();
            let retained = source.clone();
            let witness = ledger.clone();
            let called = Rc::new(Cell::new(false));
            let seen = called.clone();
            let revoker = Rc::downgrade(&facade);
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if (action != 0 && witness.workspace_bytes_for_test() == 0) || seen.replace(true) {
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
                let raw = super::vector_text_entry::evaluate(
                    args.as_raw(),
                    factory.nil().as_raw(),
                    operation,
                );
                factory.wrap(raw).unwrap().into_owned().unwrap()
            }));
            assert!(called.get());
            assert_eq!((*instance).memory_state.in_gc, 0);
            assert_eq!(ledger.workspace_bytes_for_test(), 0);
            match action {
                0 => {
                    let result = result.unwrap();
                    match operation {
                        Operation::Count => assert_eq!(result.try_integer_elt(0).unwrap(), 3),
                        Operation::Trim => assert!(
                            result
                                .try_string_elt(0)
                                .unwrap()
                                .try_char_eq("e\u{301}".as_bytes())
                                .unwrap()
                        ),
                        Operation::Prefix | Operation::Suffix => {
                            assert_eq!(result.try_logical_elt(0).unwrap(), 1)
                        }
                        Operation::Repeat => {
                            let scalar = result.try_string_elt(0).unwrap().into_owned().unwrap();
                            assert!(scalar.try_char_eq("éé".as_bytes()).unwrap());
                            assert!(encoding::flags(&scalar).unwrap().bytes);
                        }
                    }
                    factory.wrap(crate::sexp::globals::R_NilValue()).unwrap();
                    crate::sexp::gengc::full_gc();
                    assert!(!source.is_live());
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
            (*instance).memory_state.gc_force_gap = 0;
        });
    }
}
#[test]
fn owned_vector_text_count_survives_detached_arguments_and_gc() {
    callback(Operation::Count, 0);
}
#[test]
fn owned_vector_text_trim_survives_detached_arguments_and_gc() {
    callback(Operation::Trim, 0);
}
#[test]
fn owned_vector_text_prefix_survives_detached_arguments_and_gc() {
    callback(Operation::Prefix, 0);
}
#[test]
fn owned_vector_text_suffix_survives_detached_arguments_and_gc() {
    callback(Operation::Suffix, 0);
}
#[test]
fn owned_vector_text_repeat_survives_detached_arguments_and_gc() {
    callback(Operation::Repeat, 0);
}
#[test]
fn owned_vector_text_constructor_panic_releases_workspace() {
    callback(Operation::Trim, 1);
}
#[test]
fn owned_vector_text_revocation_rejects_publication() {
    callback(Operation::Repeat, 2);
}
#[test]
fn owned_vector_text_foreign_arguments_reject_before_copy() {
    use crate::sexp::owner::with_runtime;
    let first = RSession::new_for_gc_tests();
    let weak = first.owner_token().unwrap().weak_owner().unwrap();
    let second = RSession::new_for_gc_tests();
    let args = fixture(
        &second.owner_token().unwrap().node_factory(),
        Operation::Prefix,
    );
    first.with_active(|| {
        let owner = first.owner_token().unwrap();
        let na = owner
            .sexp(unsafe { crate::sexp::globals::R_NaString() })
            .unwrap()
            .into_owned()
            .unwrap();
        let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        let result = with_runtime(&weak, |access| {
            super::vector_text_owned::evaluate(access, &args, &na, Operation::Prefix, |_, v, _| {
                Ok(v.clone())
            })
        })
        .unwrap();
        assert_eq!(
            result.unwrap_err(),
            crate::sexp::object::SexpError::UnownedPointer {
                address: args.as_raw().addr()
            }
        );
        owner
            .with_arena(|a| {
                assert_eq!(a.total_bytes_allocated(), before);
                assert_eq!(a.workspace_bytes(), 0);
            })
            .unwrap();
    });
}
