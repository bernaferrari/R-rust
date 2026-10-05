//! Pinned GNU multi-element warning and actual call, before STRING_ELT.
use crate::sexp::{
    RSession,
    object::{SessionNodeFactory, Sexp},
};
const MESSAGE: &str =
    "argument should be a character vector of length 1\nall but the first element will be ignored";
const CALL: &str = "charToRaw(c(\"first\", \"ignored\"))";
fn fixture(f: &SessionNodeFactory<'_>) -> (Sexp<'static>, Sexp<'static>) {
    let owner = unsafe { crate::sexp::owner::OwnerToken::current() }.unwrap();
    let c = f
        .wrap(unsafe { crate::sexp::symbol::Rf_install(c"c".as_ptr()) })
        .unwrap();
    let head = f
        .wrap(unsafe { crate::sexp::symbol::Rf_install(c"charToRaw".as_ptr()) })
        .unwrap();
    let tail = f
        .pairlist_cell(&f.strings(&["ignored"]).unwrap(), &f.nil(), &f.nil())
        .unwrap();
    let source = f
        .pairlist_cell(&f.strings(&["first"]).unwrap(), &tail, &f.nil())
        .unwrap();
    let call = crate::sexp::owner::with_runtime(&owner.weak_owner().unwrap(), |access| {
        let allocator = access.allocator(&f.domain()).unwrap();
        let expression = allocator.call(&c, &source).unwrap();
        let arguments = f.pairlist_cell(&expression, &f.nil(), &f.nil()).unwrap();
        allocator.call(&head, &arguments).unwrap()
    })
    .unwrap()
    .into_owned()
    .unwrap();
    let arguments = f
        .pairlist_cell(
            &f.strings(&["first", "ignored"]).unwrap(),
            &f.nil(),
            &f.nil(),
        )
        .unwrap()
        .into_owned()
        .unwrap();
    (call, arguments)
}
fn text(f: &SessionNodeFactory<'_>, value: &Sexp<'_>) -> String {
    f.wrap(unsafe {
        crate::mainutils::deparse::deparse1(
            value.as_raw(),
            false,
            crate::mainutils::deparse::DEFAULTDEPARSE,
        )
    })
    .unwrap()
    .try_string_value_elt(0)
    .unwrap()
    .unwrap()
}
fn native(f: &SessionNodeFactory<'_>, call: &Sexp<'_>, args: &Sexp<'_>) -> Sexp<'static> {
    f.wrap(unsafe {
        super::do_charToRaw(
            call.as_raw(),
            f.nil().as_raw(),
            args.as_raw(),
            f.nil().as_raw(),
        )
    })
    .unwrap()
    .into_owned()
    .unwrap()
}
#[test]
fn owned_char_to_raw_warning_matches_original_message_and_actual_call() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let (call, args) = fixture(&f);
        unsafe {
            crate::mainutils::options::R_SetOptionWarn(0);
        }
        let output = native(&f, &call, &args);
        assert_eq!(
            (0..output.len())
                .map(|i| output.try_raw_elt(i).unwrap())
                .collect::<Vec<_>>(),
            b"first"
        );
        assert_eq!(crate::mainutils::errors::collect_warnings(), 1);
        assert_eq!(
            crate::mainutils::errors::last_collected_warning_message(),
            MESSAGE
        );
        let pin = session
            .owner_token()
            .unwrap()
            .weak_owner()
            .unwrap()
            .pin()
            .unwrap();
        let warnings = unsafe { (*pin.as_ptr()).error_state.warnings.owned() }.unwrap();
        assert_eq!(text(&f, &warnings.try_vector_elt(0).unwrap()), CALL);
        assert!(unsafe { (*pin.as_ptr()).error_state.warning_call.is_null() });
        let call_node = call.allocation().unwrap().clone();
        drop(warnings);
        drop(call);
        unsafe {
            crate::sexp::gengc::full_gc();
        }
        assert!(
            call_node.is_live(),
            "actual deferred warning owns the original call"
        );
        unsafe {
            (*pin.as_ptr()).error_state.warnings = crate::sexp::instance::RuntimeValue::empty();
            crate::mainutils::errors::restore_collect_warnings(0);
            crate::sexp::gengc::full_gc();
        }
        assert!(
            !call_node.is_live(),
            "removing the deferred warning releases syntax"
        );
    });
}
#[test]
fn owned_char_to_raw_warning_warn_two_preserves_call_and_recovery() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let (call, args) = fixture(&f);
        crate::mainutils::errors::take_recorded_error_call();
        unsafe {
            crate::mainutils::options::R_SetOptionWarn(2);
        }
        let error =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| native(&f, &call, &args)))
                .unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<crate::sexp::context::RError>()
                .unwrap()
                .message,
            format!("(converted from warning) {MESSAGE}")
        );
        let (recorded, _, _) = crate::mainutils::errors::take_recorded_error_call().unwrap();
        assert_eq!(text(&f, &recorded), CALL);
        let pin = session
            .owner_token()
            .unwrap()
            .weak_owner()
            .unwrap()
            .pin()
            .unwrap();
        assert!(unsafe { (*pin.as_ptr()).error_state.warning_call.is_null() });
        unsafe {
            crate::mainutils::options::R_SetOptionWarn(0);
        }
        let recovery = f
            .pairlist_cell(&f.strings(&["ok"]).unwrap(), &f.nil(), &f.nil())
            .unwrap();
        assert_eq!(native(&f, &f.nil(), &recovery).len(), 2);
    });
}

fn collecting_handler(f: &SessionNodeFactory<'_>) -> Sexp<'static> {
    use crate::sexp::{ffi::SEXPTYPE, object::SexpMut};
    let gc_symbol = f
        .wrap(unsafe { crate::sexp::symbol::Rf_install(c"gc".as_ptr()) })
        .unwrap();
    let gc = f
        .wrap(unsafe { crate::eval::primitive::make_primitive_binding("gc", SEXPTYPE::BUILTINSXP) })
        .unwrap();
    let env = f
        .wrap(unsafe { crate::sexp::globals::R_GlobalEnv() })
        .unwrap();
    unsafe {
        assert!(crate::sexp::envir::define_var_safe(
            gc_symbol.clone(),
            gc,
            env.clone()
        ));
    }
    let owner = unsafe { crate::sexp::owner::OwnerToken::current() }
        .unwrap()
        .weak_owner()
        .unwrap();
    let body = crate::sexp::owner::with_runtime(&owner, |access| {
        access
            .allocator(&f.domain())
            .unwrap()
            .call(&gc_symbol, &f.nil())
            .unwrap()
    })
    .unwrap();
    let condition = f
        .wrap(unsafe { crate::sexp::symbol::Rf_install(c"condition".as_ptr()) })
        .unwrap();
    let formals = f.pairlist_cell(&f.missing(), &f.nil(), &condition).unwrap();
    let closure = f
        .wrap(unsafe {
            crate::mainutils::dstruct::mkCLOSXP(formals.as_raw(), body.as_raw(), env.as_raw())
        })
        .unwrap();
    let entry = f
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, 5)))
        .unwrap();
    let mut entry = SexpMut::try_from_checked(entry).unwrap();
    for (i, value) in [
        f.character("warning").unwrap(),
        env,
        closure,
        f.nil(),
        f.nil(),
    ]
    .into_iter()
    .enumerate()
    {
        entry.try_set_vector_elt(i as i64, value).unwrap();
    }
    let entry = entry.freeze();
    unsafe {
        crate::sexp::accessors::SETLEVELS(entry.as_raw(), 1);
    }
    f.pairlist_cell(&entry, &f.nil(), &f.nil())
        .unwrap()
        .into_owned()
        .unwrap()
}
fn handler_case(action: u8) {
    use std::{
        cell::{Cell, RefCell},
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let (owner, call, args, input, child, previous) = {
        let facade = facade.borrow();
        let session = facade.as_ref().unwrap();
        session.with_active(|| {
            let owner = session.owner_token().unwrap();
            let f = owner.node_factory();
            let (call, args) = fixture(&f);
            let input = args.try_car().unwrap();
            let input_node = input.allocation().unwrap().clone();
            let child = input
                .try_string_elt(0)
                .unwrap()
                .allocation()
                .unwrap()
                .clone();
            let head = f
                .wrap(unsafe {
                    crate::sexp::symbol::Rf_install(c"previous_character_warning".as_ptr())
                })
                .unwrap();
            let weak = owner.weak_owner().unwrap();
            let previous = crate::sexp::owner::with_runtime(&weak, |access| {
                access
                    .allocator(&f.domain())
                    .unwrap()
                    .call(&head, &f.nil())
                    .unwrap()
            })
            .unwrap()
            .into_owned()
            .unwrap();
            (weak, call, args, input_node, child, previous)
        })
    };
    let pin = owner.pin().unwrap();
    let instance = pin.as_ptr();
    let called = Rc::new(Cell::new(0));
    unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            let f = owner.node_factory().unwrap();
            crate::mainutils::options::R_SetOptionWarn(0);
            let _previous = crate::mainutils::errors::warning_call_guard(previous.as_raw());
            let handlers = collecting_handler(&f);
            let old_handlers = (*instance).error_state.handler_stack.clone();
            (*instance).error_state.handler_stack =
                crate::sexp::instance::RuntimeValue::from_owned(handlers);
            let callback_called = called.clone();
            let callback_args = args.clone();
            let callback_input = input.clone();
            let callback_child = child.clone();
            let callback_owner = owner.clone();
            let callback_facade = Rc::downgrade(&facade);
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if callback_called.get() != 0 {
                    return;
                }
                callback_called.set(1);
                let f = callback_owner.node_factory().unwrap();
                let actual = (*instance).error_state.warning_call.owned().unwrap();
                assert_eq!(
                    text(&f, &actual),
                    CALL,
                    "collection must be inside the real warning closure"
                );
                crate::sexp::accessors::SETCAR(callback_args.as_raw(), f.nil().as_raw());
                crate::sexp::gengc::full_gc();
                assert!(
                    callback_input.is_live(),
                    "actual input owns its selected graph before STRING_ELT"
                );
                assert!(callback_child.is_live());
                match action {
                    1 => std::panic::panic_any(953_u32),
                    2 => drop(callback_facade.upgrade().unwrap().borrow_mut().take()),
                    _ => {}
                }
            }));
            let result = catch_unwind(AssertUnwindSafe(|| native(&f, &call, &args)));
            (*instance).error_state.handler_stack = old_handlers;
            assert_eq!(called.get(), 1, "actual warning closure evaluated gc()");
            assert_eq!(
                (*instance).error_state.warning_call.as_raw(),
                previous.as_raw()
            );
            assert_eq!((*instance).memory_state.in_gc, 0);
            match action {
                0 => {
                    let value = result.unwrap();
                    assert_eq!(
                        (0..value.len())
                            .map(|i| value.try_raw_elt(i).unwrap())
                            .collect::<Vec<_>>(),
                        b"first"
                    );
                    crate::sexp::gengc::full_gc();
                    assert!(!input.is_live());
                    assert!(!child.is_live());
                }
                1 => assert_eq!(*result.unwrap_err().downcast::<u32>().unwrap(), 953),
                2 => {
                    assert!(!owner.is_live());
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
fn owned_char_to_raw_warning_real_handler_collects_detached_input() {
    handler_case(0);
}
#[test]
fn owned_char_to_raw_warning_live_handler_panic_restores_call() {
    handler_case(1);
}
#[test]
fn owned_char_to_raw_warning_revoked_handler_cannot_publish() {
    handler_case(2);
}
struct CountElement(std::rc::Rc<std::cell::Cell<usize>>);
impl crate::sexp::altrep::AltrepClass for CountElement {
    fn vector_type(&self) -> crate::sexp::ffi::SEXPTYPE {
        crate::sexp::ffi::SEXPTYPE::STRSXP
    }
    fn length(
        &self,
        _: &crate::sexp::altrep::AltrepContext<'_>,
    ) -> crate::sexp::object::SexpResult<i64> {
        Ok(2)
    }
    fn element<'s>(
        &self,
        context: &crate::sexp::altrep::AltrepContext<'s>,
        _: i64,
    ) -> crate::sexp::object::SexpResult<crate::sexp::altrep::AltrepElement<'s>> {
        self.0.set(self.0.get() + 1);
        Ok(crate::sexp::altrep::AltrepElement::String(
            context.data1()?.try_string_elt(0)?,
        ))
    }
}
#[test]
fn owned_char_to_raw_warning_precedes_arbitrary_element_provider() {
    use std::{cell::Cell, rc::Rc};
    let session = RSession::new_for_gc_tests();
    let count = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class("character.warning.order", CountElement(count.clone()))
        .unwrap();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let (call, _) = fixture(&f);
        let input = crate::sexp::altrep::AltrepBuilder::new(class)
            .data1(f.strings(&["first"]).unwrap())
            .build()
            .unwrap();
        let arguments = f.pairlist_cell(&input, &f.nil(), &f.nil()).unwrap();
        unsafe {
            crate::mainutils::options::R_SetOptionWarn(2);
        }
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| native(
                &f, &call, &arguments
            )))
            .is_err()
        );
        assert_eq!(count.get(), 0, "warn=2 fails before provider STRING_ELT");
    });
}

#[test]
fn owned_char_to_raw_warning_actual_call_overrides_unrelated_mathlib_scope() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let f = owner.node_factory();
        let (call, args) = fixture(&f);
        let head = f
            .wrap(unsafe { crate::sexp::symbol::Rf_install(c"outer_math_warning".as_ptr()) })
            .unwrap();
        let previous = crate::sexp::owner::with_runtime(&owner.weak_owner().unwrap(), |access| {
            access
                .allocator(&f.domain())
                .unwrap()
                .call(&head, &f.nil())
                .unwrap()
        })
        .unwrap();
        let _outer = crate::mainutils::errors::mathlib_warning_call_guard(previous.as_raw());
        let pin = owner.pin().unwrap().unwrap();
        unsafe {
            crate::mainutils::options::R_SetOptionWarn(0);
        }
        native(&f, &call, &args);
        let warnings = unsafe { (*pin.as_ptr()).error_state.warnings.owned() }.unwrap();
        assert_eq!(text(&f, &warnings.try_vector_elt(0).unwrap()), CALL);
        assert_eq!(
            unsafe { (*pin.as_ptr()).error_state.mathlib_warning_call.as_raw() },
            previous.as_raw()
        );
    });
}
