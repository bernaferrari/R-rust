//! Independent original naresid.exclude replacement-warning calls.
use super::{call, list, model, real};
use crate::sexp::{
    RSession,
    object::{SessionNodeFactory, Sexp},
};
const ROW_ASSIGNMENT: &str = "keep[-omit] <- 1L:n";
fn malformed_model<'s>(f: &SessionNodeFactory<'s>) -> Sexp<'s> {
    let original = model(f);
    let action = real(f, &[10.]);
    let names = f.strings(&["missing"]).unwrap();
    let class = f.strings(&["exclude"]).unwrap();
    unsafe {
        crate::sexp::attrib_core::setAttrib(
            action.as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
            names.as_raw(),
        );
        crate::sexp::attrib_core::setAttrib(
            action.as_raw(),
            crate::sexp::attrib_core::R_ClassSymbol(),
            class.as_raw(),
        );
    }
    list(
        f,
        &[
            "residuals",
            "hat",
            "sigma",
            "rank",
            "df.residual",
            "na.action",
        ],
        &[
            original.try_vector_elt(0).unwrap(),
            original.try_vector_elt(1).unwrap(),
            original.try_vector_elt(2).unwrap(),
            original.try_vector_elt(3).unwrap(),
            real(f, &[6.]),
            action,
        ],
    )
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
#[test]
fn covratio_row_condition_calls_match_original_replacement_before_names_error() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let previous = unsafe { crate::mainutils::options::R_SetOptionWarn(0) };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            call(&f, &[("", malformed_model(&f))])
        }));
        unsafe {
            crate::mainutils::options::R_SetOptionWarn(previous);
        }
        assert_eq!(
            result
                .unwrap_err()
                .downcast_ref::<crate::sexp::context::RError>()
                .unwrap()
                .message,
            "'names' attribute [10] must be the same length as the vector [9]"
        );
        assert_eq!(crate::mainutils::errors::collect_warnings(), 1);
        let pin = session
            .owner_token()
            .unwrap()
            .weak_owner()
            .unwrap()
            .pin()
            .unwrap();
        let warnings = unsafe { (*pin.as_ptr()).error_state.warnings.owned() }.unwrap();
        let call = warnings.try_vector_elt(0).unwrap();
        assert_eq!(text(&f, &call), ROW_ASSIGNMENT);
        assert_eq!(
            crate::mainutils::errors::last_collected_warning_message(),
            "number of items to replace is not a multiple of replacement length"
        );
    });
}
#[test]
fn covratio_row_condition_calls_warn_two_preserves_original_assignment_call() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        crate::mainutils::errors::take_recorded_error_call();
        let previous = unsafe { crate::mainutils::options::R_SetOptionWarn(2) };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            call(&f, &[("", malformed_model(&f))])
        }));
        unsafe {
            crate::mainutils::options::R_SetOptionWarn(previous);
        }
        assert!(
            result
                .unwrap_err()
                .downcast_ref::<crate::sexp::context::RError>()
                .unwrap()
                .message
                .contains("(converted from warning) number of items to replace")
        );
        let (call, _, _) = crate::mainutils::errors::take_recorded_error_call().unwrap();
        assert_eq!(text(&f, &call), ROW_ASSIGNMENT);
        let pin = session
            .owner_token()
            .unwrap()
            .weak_owner()
            .unwrap()
            .pin()
            .unwrap();
        assert!(unsafe { (*pin.as_ptr()).error_state.warning_call.is_null() });
        assert_eq!(
            super::call(&f, &[("", model(&f))]).len(),
            8,
            "the original session recovers after warn=2"
        );
    });
}

// A genuine R calling handler evaluates gc(); the collector notification is
// the approved host callback, rather than an unsupported native warning entry.
struct Trace {
    args: Sexp<'static>,
    model: Sexp<'static>,
    previous_call: Sexp<'static>,
    owner: crate::sexp::owner::WeakOwner,
    facade: std::rc::Weak<std::cell::RefCell<Option<RSession>>>,
    notifications: std::cell::Cell<usize>,
    action: u8,
    residual: crate::sexp::heap::CheckedNode,
    omitted: crate::sexp::heap::CheckedNode,
    influence: Option<Sexp<'static>>,
    influence_children: Vec<crate::sexp::heap::CheckedNode>,
}
fn warning_handler_stack(f: &SessionNodeFactory<'_>) -> Sexp<'static> {
    use crate::sexp::{ffi::SEXPTYPE, object::SexpMut};
    let symbol = f
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
            symbol.clone(),
            gc,
            env.clone()
        ));
    }
    let body = crate::sexp::owner::with_runtime(
        &unsafe { crate::sexp::owner::OwnerToken::current() }
            .unwrap()
            .weak_owner()
            .unwrap(),
        |access| {
            access
                .allocator(&f.domain())
                .unwrap()
                .call(&symbol, &f.nil())
                .unwrap()
        },
    )
    .unwrap();
    let argument = f
        .wrap(unsafe { crate::sexp::symbol::Rf_install(c"condition".as_ptr()) })
        .unwrap();
    let formals = f.pairlist_cell(&f.missing(), &f.nil(), &argument).unwrap();
    let handler = f
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
        handler,
        f.nil(),
        f.nil(),
    ]
    .into_iter()
    .enumerate()
    {
        entry.try_set_vector_elt(i as i64, value).unwrap();
    }
    let entry = entry.freeze();
    // Actual GNU calling-handler bit and five-slot entry layout.
    unsafe {
        crate::sexp::accessors::SETLEVELS(entry.as_raw(), 1);
    }
    f.pairlist_cell(&entry, &f.nil(), &f.nil())
        .unwrap()
        .into_owned()
        .unwrap()
}
fn collection(trace: &Trace, instance: *mut crate::sexp::instance::RInstance) {
    trace.notifications.set(trace.notifications.get() + 1);
    let f = trace.owner.node_factory().unwrap();
    let call = unsafe { (*instance).error_state.warning_call.owned() }.unwrap();
    assert_eq!(text(&f, &call), ROW_ASSIGNMENT);
    assert_ne!(call.as_raw(), trace.previous_call.as_raw());
    // The checked original objects are fixture-owned, and the operation has
    // already captured their exact children. Remove every incidental edge.
    unsafe {
        let mut cell = trace.args.clone();
        while !cell.is_nil() {
            crate::sexp::accessors::SETCAR(cell.as_raw(), f.nil().as_raw());
            cell = cell.try_cdr().unwrap().into_owned().unwrap();
        }
        if let Some(influence) = &trace.influence {
            crate::sexp::accessors::SET_VECTOR_ELT(influence.as_raw(), 0, f.nil().as_raw());
            crate::sexp::accessors::SET_VECTOR_ELT(influence.as_raw(), 1, f.nil().as_raw());
        }
        crate::sexp::accessors::SET_VECTOR_ELT(trace.model.as_raw(), 0, f.nil().as_raw());
        crate::sexp::accessors::SET_VECTOR_ELT(trace.model.as_raw(), 5, f.nil().as_raw());
        crate::sexp::gengc::full_gc();
    }
    assert!(
        trace.residual.is_live(),
        "the operation owns its detached residuals"
    );
    assert!(
        trace.omitted.is_live(),
        "Rows owns its detached original action"
    );
    assert!(
        trace.influence_children.iter().all(|node| node.is_live()),
        "selected supplied influence children survive the row warning callback"
    );
    assert_eq!(text(&f, &call), ROW_ASSIGNMENT);
    match trace.action {
        1 => std::panic::panic_any(941_u32),
        2 => drop(trace.facade.upgrade().unwrap().borrow_mut().take()),
        _ => {}
    }
}
fn real_handler_case(action: u8) {
    use std::{
        cell::RefCell,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let (args, model, previous_call, owner, residual, omitted, influence, influence_children) = {
        let facade = facade.borrow();
        let session = facade.as_ref().unwrap();
        session.with_active(|| {
            let owner = session.owner_token().unwrap();
            let f = owner.node_factory();
            unsafe {
                crate::mainutils::options::R_SetOptionWarn(0);
            }
            let model = malformed_model(&f);
            let residual = model
                .try_vector_elt(0)
                .unwrap()
                .allocation()
                .unwrap()
                .clone();
            let omitted = model
                .try_vector_elt(5)
                .unwrap()
                .allocation()
                .unwrap()
                .clone();
            let influence = if action == 3 {
                // This original-GNU neighbor has no residual names, so row
                // restoration proceeds past the independently tested names error.
                unsafe {
                    crate::sexp::accessors::SET_ATTRIB(
                        model.try_vector_elt(0).unwrap().as_raw(),
                        f.nil().as_raw(),
                    );
                }
                Some(list(
                    &f,
                    &["hat", "sigma"],
                    &[real(&f, &[0.2; 8]), real(&f, &[2.; 8])],
                ))
            } else {
                None
            };
            let mut influence_children = Vec::new();
            let tail = if let Some(influence) = &influence {
                for i in 0..2 {
                    influence_children.push(
                        influence
                            .try_vector_elt(i)
                            .unwrap()
                            .allocation()
                            .unwrap()
                            .clone(),
                    );
                }
                let tag = f
                    .wrap(unsafe { crate::sexp::symbol::Rf_install(c"infl".as_ptr()) })
                    .unwrap();
                f.pairlist_cell(influence, &f.nil(), &tag).unwrap()
            } else {
                f.nil()
            };
            let args = f.pairlist_cell(&model, &tail, &f.nil()).unwrap();
            let owner = owner.weak_owner().unwrap();
            let head = f
                .wrap(unsafe { crate::sexp::symbol::Rf_install(c"previous_row_warning".as_ptr()) })
                .unwrap();
            let previous_call = crate::sexp::owner::with_runtime(&owner, |access| {
                access
                    .allocator(&f.domain())
                    .unwrap()
                    .call(&head, &f.nil())
                    .unwrap()
            })
            .unwrap();
            (
                args.into_owned().unwrap(),
                model.into_owned().unwrap(),
                previous_call.into_owned().unwrap(),
                owner,
                residual,
                omitted,
                influence.map(|value| value.into_owned().unwrap()),
                influence_children,
            )
        })
    };
    let pin = owner.pin().unwrap();
    let instance = pin.as_ptr();
    let trace = Rc::new(Trace {
        args,
        model,
        previous_call,
        owner: owner.clone(),
        facade: Rc::downgrade(&facade),
        notifications: std::cell::Cell::new(0),
        action,
        residual,
        omitted,
        influence,
        influence_children,
    });
    unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            let f = owner.node_factory().unwrap();
            let _original =
                crate::mainutils::errors::warning_call_guard(trace.previous_call.as_raw());
            let handlers = warning_handler_stack(&f);
            let old_handlers = (*instance).error_state.handler_stack.clone();
            (*instance).error_state.handler_stack =
                crate::sexp::instance::RuntimeValue::from_owned(handlers);
            let callback = trace.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if callback.notifications.get() == 0 {
                    collection(&callback, instance);
                }
            }));
            let result = catch_unwind(AssertUnwindSafe(|| {
                super::super::mathstats::do_covratio(
                    f.nil().as_raw(),
                    f.nil().as_raw(),
                    trace.args.as_raw(),
                    f.nil().as_raw(),
                )
            }));
            (*instance).error_state.handler_stack = old_handlers;
            assert_eq!(
                trace.notifications.get(),
                1,
                "actual R warning handler reached gc()"
            );
            assert_eq!(
                (*instance).error_state.warning_call.as_raw(),
                trace.previous_call.as_raw()
            );
            assert_eq!((*instance).memory_state.in_gc, 0);
            if action == 3 {
                let values = f.wrap(result.unwrap()).unwrap();
                super::assert_values(
                    &values,
                    &[
                        1.7984385166642349,
                        1.6290450378826291,
                        1.5529426541460845,
                        1.6373127499260918,
                        1.5433789848080433,
                        1.6454205829863175,
                        1.5337027303755011,
                        1.6533641933438721,
                        1.7984385166642349,
                    ],
                );
                assert_eq!(crate::mainutils::errors::collect_warnings(), 3);
                crate::sexp::gengc::full_gc();
                assert!(
                    trace.influence_children.iter().all(|node| !node.is_live()),
                    "completed operation releases the selected influence snapshots"
                );
                return;
            }
            let failure = result.unwrap_err();
            match action {
                0 => {
                    assert_eq!(
                        failure
                            .downcast_ref::<crate::sexp::context::RError>()
                            .unwrap()
                            .message,
                        "'names' attribute [10] must be the same length as the vector [9]"
                    );
                    let warnings = (*instance).error_state.warnings.owned().unwrap();
                    let call = warnings.try_vector_elt(0).unwrap();
                    let node = call.allocation().unwrap().clone();
                    assert_eq!(text(&f, &call), ROW_ASSIGNMENT);
                    drop(call);
                    drop(warnings);
                    crate::sexp::gengc::full_gc();
                    assert!(!trace.residual.is_live());
                    assert!(!trace.omitted.is_live());
                    assert!(node.is_live(), "actual deferred warning owns the syntax");
                    (*instance).error_state.warnings = crate::sexp::instance::RuntimeValue::empty();
                    crate::mainutils::errors::restore_collect_warnings(0);
                    crate::sexp::gengc::full_gc();
                    assert!(
                        !node.is_live(),
                        "clearing actual deferred warnings releases syntax"
                    );
                }
                1 => assert_eq!(*failure.downcast::<u32>().unwrap(), 941),
                _ => assert_eq!(
                    failure
                        .downcast_ref::<crate::sexp::context::RError>()
                        .unwrap()
                        .message,
                    crate::sexp::object::SexpError::RootUnavailable.to_string()
                ),
            }
        });
    }
}
#[test]
fn covratio_row_condition_calls_own_action_and_syntax_through_real_handler_collection() {
    real_handler_case(0);
}
#[test]
fn covratio_row_condition_calls_restore_original_call_after_live_handler_panic() {
    real_handler_case(1);
}
#[test]
fn covratio_row_condition_calls_restore_original_call_after_handler_revocation() {
    real_handler_case(2);
}

#[test]
fn covratio_row_condition_calls_capture_supplied_children_before_default_row_warning() {
    real_handler_case(3);
}
