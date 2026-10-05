//! Original charToRaw bytes and actual allocation-callback ownership.
use crate::sexp::{
    RSession,
    ffi::SEXPTYPE,
    object::{SessionNodeFactory, Sexp},
};
fn args<'s>(f: &SessionNodeFactory<'s>, value: &Sexp<'s>) -> Sexp<'s> {
    f.pairlist_cell(value, &f.nil(), &f.nil()).unwrap()
}
fn call<'s>(f: &SessionNodeFactory<'s>, arguments: &Sexp<'s>) -> Sexp<'s> {
    f.wrap(unsafe {
        super::do_charToRaw(
            f.nil().as_raw(),
            f.nil().as_raw(),
            arguments.as_raw(),
            f.nil().as_raw(),
        )
    })
    .unwrap()
}
fn raw(value: &Sexp<'_>) -> Vec<u8> {
    assert_eq!(value.typeof_(), SEXPTYPE::RAWSXP);
    (0..value.len())
        .map(|i| value.try_raw_elt(i).unwrap())
        .collect()
}
#[test]
fn owned_char_to_raw_missing_matches_original_bytes() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let input = f.strings(&[""]).unwrap();
        let missing = f
            .wrap(unsafe { crate::sexp::globals::R_NaString() })
            .unwrap();
        unsafe {
            crate::sexp::accessors::SET_STRING_ELT(input.as_raw(), 0, missing.as_raw());
        }
        assert_eq!(raw(&call(&f, &args(&f, &input))), b"NA");
        assert_eq!(input.try_string_value_elt(0).unwrap(), None);
    });
}
#[test]
fn owned_char_to_raw_selected_character_survives_detachment_and_collecting_allocation() {
    use std::{cell::Cell, rc::Rc};
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let pin = owner.pin().unwrap().unwrap();
        let instance = pin.as_ptr();
        let f = owner.node_factory();
        let input = f.strings(&["captured"]).unwrap().into_owned().unwrap();
        let child = input
            .try_string_elt(0)
            .unwrap()
            .allocation()
            .unwrap()
            .clone();
        let arguments = args(&f, &input).into_owned().unwrap();
        let seen = Rc::new(Cell::new(0));
        let callback_seen = seen.clone();
        let callback_args = arguments.clone();
        let callback_input = input.clone();
        let callback_child = child.clone();
        let weak = owner.weak_owner().unwrap();
        unsafe {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if callback_seen.get() != 0 {
                    return;
                }
                callback_seen.set(1);
                (*instance).memory_state.gc_force_gap = 0;
                let f = weak.node_factory().unwrap();
                crate::sexp::accessors::SETCAR(callback_args.as_raw(), f.nil().as_raw());
                crate::sexp::accessors::SET_STRING_ELT(
                    callback_input.as_raw(),
                    0,
                    crate::sexp::globals::R_NaString(),
                );
                crate::sexp::gengc::full_gc();
                assert!(
                    callback_child.is_live(),
                    "the operation must own its detached selected CHARSXP"
                );
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
        }
        let output = call(&f, &arguments);
        assert_eq!(seen.get(), 1, "genuine allocation collection must execute");
        assert_eq!(raw(&output), b"captured");
        assert!(child.is_live());
        drop(output);
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        assert!(
            !child.is_live(),
            "selected source releases after publication"
        );
    });
}
#[test]
fn owned_char_to_raw_keeps_exact_first_character_bytes_and_errors() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        for (text, expected) in [
            ("", b"".as_slice()),
            ("NA", b"NA".as_slice()),
            ("é", [0xc3, 0xa9].as_slice()),
            ("first", b"first".as_slice()),
        ] {
            let input = f.strings(&[text, "ignored"]).unwrap();
            assert_eq!(raw(&call(&f, &args(&f, &input))), expected);
        }
        let character = f
            .allocate(|arena| Some(arena.alloc_charsxp(&[0xff, 0x41])))
            .unwrap();
        let input = f.strings(&[""]).unwrap();
        unsafe {
            crate::sexp::accessors::SET_STRING_ELT(input.as_raw(), 0, character.as_raw());
        }
        assert_eq!(raw(&call(&f, &args(&f, &input))), [0xff, 0x41]);
        for input in [f.nil(), f.domain().logical(true), f.strings(&[]).unwrap()] {
            let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                call(&f, &args(&f, &input))
            }))
            .unwrap_err();
            assert_eq!(
                error
                    .downcast_ref::<crate::sexp::context::RError>()
                    .unwrap()
                    .message,
                "argument must be a character vector of length 1"
            );
        }
    });
}

struct CollectingString(std::rc::Rc<std::cell::Cell<usize>>);
impl crate::sexp::altrep::AltrepClass for CollectingString {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::STRSXP
    }
    fn length(
        &self,
        _: &crate::sexp::altrep::AltrepContext<'_>,
    ) -> crate::sexp::object::SexpResult<i64> {
        Ok(1)
    }
    fn element<'s>(
        &self,
        context: &crate::sexp::altrep::AltrepContext<'s>,
        _: i64,
    ) -> crate::sexp::object::SexpResult<crate::sexp::altrep::AltrepElement<'s>> {
        self.0.set(self.0.get() + 1);
        let data = context.data1()?;
        let character = data.try_string_elt(0)?;
        let object = context.object();
        crate::sexp::altrep::set_data1(&object, object.node_factory()?.nil())?;
        drop(data);
        context.gc()?;
        Ok(crate::sexp::altrep::AltrepElement::String(character))
    }
}
#[test]
fn owned_char_to_raw_provider_collects_with_actual_detached_child() {
    use std::{cell::Cell, rc::Rc};
    let session = RSession::new_for_gc_tests();
    let called = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class("character.bytes.collect", CollectingString(called.clone()))
        .unwrap();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let source = f.strings(&["provider"]).unwrap();
        let child = source
            .try_string_elt(0)
            .unwrap()
            .allocation()
            .unwrap()
            .clone();
        let input = crate::sexp::altrep::AltrepBuilder::new(class)
            .data1(source)
            .build()
            .unwrap();
        let output = call(&f, &args(&f, &input));
        assert_eq!(called.get(), 1);
        assert_eq!(raw(&output), b"provider");
        assert!(child.is_live());
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        assert!(
            !child.is_live(),
            "copied output has no unnecessary source root"
        );
        assert_eq!(raw(&output), b"provider");
    });
}
fn allocation_unwind(revoke: bool) {
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
    let f = weak.node_factory().unwrap();
    let (arguments, input, child) = unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            let input = f.strings(&["unwind"]).unwrap().into_owned().unwrap();
            let child = input
                .try_string_elt(0)
                .unwrap()
                .allocation()
                .unwrap()
                .clone();
            (args(&f, &input).into_owned().unwrap(), input, child)
        })
    };
    let calls = Rc::new(Cell::new(0));
    let callback_calls = calls.clone();
    let callback_facade = Rc::downgrade(&facade);
    let callback_args = arguments.clone();
    let callback_input = input.clone();
    let callback_child = child.clone();
    let callback_owner = weak.clone();
    let outcome = catch_unwind(AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if callback_calls.get() != 0 {
                    return;
                }
                callback_calls.set(1);
                (*instance).memory_state.gc_force_gap = 0;
                let f = callback_owner.node_factory().unwrap();
                crate::sexp::accessors::SETCAR(callback_args.as_raw(), f.nil().as_raw());
                crate::sexp::accessors::SET_STRING_ELT(
                    callback_input.as_raw(),
                    0,
                    crate::sexp::globals::R_NaString(),
                );
                crate::sexp::gengc::full_gc();
                assert!(callback_child.is_live());
                if revoke {
                    drop(callback_facade.upgrade().unwrap().borrow_mut().take());
                } else {
                    std::panic::panic_any(952_u32);
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            call(&f, &arguments)
        })
    }));
    assert_eq!(calls.get(), 1);
    let error = outcome.unwrap_err();
    if revoke {
        assert!(!weak.is_live());
        assert_eq!(
            error
                .downcast_ref::<crate::sexp::context::RError>()
                .unwrap()
                .message,
            crate::sexp::object::SexpError::RootUnavailable.to_string()
        );
    } else {
        assert_eq!(error.downcast_ref::<u32>(), Some(&952));
        unsafe {
            crate::sexp::session::with_instance_active(instance, || {
                let good = f.strings(&["recovered"]).unwrap();
                assert_eq!(raw(&call(&f, &args(&f, &good))), b"recovered");
                crate::sexp::gengc::full_gc();
                assert!(!child.is_live());
            });
        }
    }
    assert_eq!(unsafe { (*instance).memory_state.in_gc }, 0);
}
#[test]
fn owned_char_to_raw_live_collecting_callback_panic_is_preserved() {
    allocation_unwind(false);
}
#[test]
fn owned_char_to_raw_revoked_original_cannot_publish() {
    allocation_unwind(true);
}
#[test]
fn owned_char_to_raw_copy_workspace_and_output_have_separate_budget_admission() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let f = owner.node_factory();
        let input = f.strings(&[&"x".repeat(128)]).unwrap();
        let arguments = args(&f, &input);
        let (old, used) = owner
            .with_arena(|arena| (arena.budget(), arena.total_bytes_allocated()))
            .unwrap();
        for extra in [127, 128] {
            owner
                .with_arena(|arena| {
                    arena.set_budget(crate::sexp::memory::ArenaBudget::new(used + extra, 0))
                })
                .unwrap();
            let error =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| call(&f, &arguments)))
                    .unwrap_err();
            let message = &error
                .downcast_ref::<crate::sexp::context::RError>()
                .unwrap()
                .message;
            if extra == 127 {
                assert!(message.contains("workspace budget"));
            } else {
                assert!(
                    !message.contains("workspace budget"),
                    "workspace fits but the separate RAW allocation must refuse"
                );
                assert!(
                    owner
                        .with_arena(|arena| arena.try_reserve_transient(128).is_some())
                        .unwrap(),
                    "failed output releases its complete copy reservation"
                );
            }
        }
        owner.with_arena(|arena| arena.set_budget(old)).unwrap();
        assert_eq!(raw(&call(&f, &arguments)), vec![b'x'; 128]);
    });
}
