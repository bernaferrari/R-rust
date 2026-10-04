//! Original GNU warning conditions identify the precise arithmetic phase.
use super::{call, list, model, real};
use crate::sexp::{
    RSession,
    object::{SessionNodeFactory, Sexp},
};
const DENOMINATOR: &str = "infl$sigma * sqrt(omh)";
const STUDENTIZED: &str = "res/(infl$sigma * sqrt(omh))";
const FIXED_GLM: &str = "res/(sigma(model) * sqrt(omh))";
const PRODUCT: &str = "omh * (((n - p - 1) + e.star^2)/(n - p))^p";
fn text(f: &SessionNodeFactory<'_>, call: &Sexp<'_>) -> String {
    let value = f
        .wrap(unsafe { crate::mainutils::deparse::deparse1line(call.as_raw(), false) })
        .unwrap();
    value.try_string_value_elt(0).unwrap().unwrap()
}
fn collected(f: &SessionNodeFactory<'_>) -> Vec<String> {
    let count = crate::mainutils::errors::collect_warnings();
    let weak = unsafe { crate::sexp::owner::OwnerToken::current() }
        .unwrap()
        .weak_owner()
        .unwrap();
    let pin = weak.pin().unwrap();
    let entries = unsafe { (*pin.as_ptr()).error_state.warnings.owned() }.unwrap();
    let calls: Vec<_> = (0..count)
        .map(|i| {
            entries
                .try_vector_elt(i as i64)
                .unwrap()
                .into_owned()
                .unwrap()
        })
        .collect();
    calls.iter().map(|call| text(f, call)).collect()
}
#[test]
fn covratio_condition_calls_denominator_and_final_match_original_phases() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let previous = unsafe { crate::mainutils::options::R_SetOptionWarn(0) };
        let before = crate::mainutils::errors::collect_warnings();
        let fit = model(&f);
        let infl = list(
            &f,
            &["hat", "sigma"],
            &[real(&f, &[0.2; 7]), real(&f, &[2.; 8])],
        );
        let result = call(
            &f,
            &[
                ("", fit),
                ("infl", infl),
                ("res", real(&f, &[1., 2., 3., 4., 5., 6., 7., 8.])),
            ],
        );
        assert_eq!(result.len(), 8);
        assert_eq!(collected(&f), [DENOMINATOR, PRODUCT]);
        crate::mainutils::errors::restore_collect_warnings(before);
        unsafe {
            crate::mainutils::options::R_SetOptionWarn(previous);
        }
    });
}
#[test]
fn covratio_condition_calls_studentized_and_final_match_original_phases() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let previous = unsafe { crate::mainutils::options::R_SetOptionWarn(0) };
        let before = crate::mainutils::errors::collect_warnings();
        let infl = list(
            &f,
            &["hat", "sigma"],
            &[real(&f, &[0.2; 7]), real(&f, &[2.; 7])],
        );
        call(
            &f,
            &[
                ("", model(&f)),
                ("infl", infl),
                ("res", real(&f, &[1., 2., 3., 4., 5., 6., 7., 8.])),
            ],
        );
        assert_eq!(collected(&f), [STUDENTIZED, PRODUCT]);
        crate::mainutils::errors::restore_collect_warnings(before);
        unsafe {
            crate::mainutils::options::R_SetOptionWarn(previous);
        }
    });
}
#[test]
fn covratio_condition_calls_fixed_glm_uses_original_sigma_expression() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        let original = model(&f);
        let family = list(&f, &["family"], &[f.strings(&["binomial"]).unwrap()]);
        let fit = list(
            &f,
            &["residuals", "hat", "sigma", "rank", "family"],
            &[
                original.try_vector_elt(0).unwrap(),
                original.try_vector_elt(1).unwrap(),
                original.try_vector_elt(2).unwrap(),
                original.try_vector_elt(3).unwrap(),
                family,
            ],
        );
        let class = f.strings(&["glm", "lm"]).unwrap();
        unsafe {
            crate::sexp::attrib_core::setAttrib(
                fit.as_raw(),
                crate::sexp::attrib_core::R_ClassSymbol(),
                class.as_raw(),
            );
        }
        let previous = unsafe { crate::mainutils::options::R_SetOptionWarn(0) };
        let before = crate::mainutils::errors::collect_warnings();
        let infl = list(
            &f,
            &["hat", "sigma"],
            &[real(&f, &[0.2; 8]), real(&f, &[2.; 3])],
        );
        call(
            &f,
            &[
                ("", fit),
                ("infl", infl),
                ("res", real(&f, &[1., 2., 3., 4., 5., 6., 7.])),
            ],
        );
        assert_eq!(collected(&f), [FIXED_GLM]);
        crate::mainutils::errors::restore_collect_warnings(before);
        unsafe {
            crate::mainutils::options::R_SetOptionWarn(previous);
        }
    });
}
#[test]
fn covratio_condition_calls_warn_two_preserves_original_error_call() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let f = session.owner_token().unwrap().node_factory();
        crate::mainutils::errors::take_recorded_error_call();
        let previous = unsafe { crate::mainutils::options::R_SetOptionWarn(2) };
        let infl = list(
            &f,
            &["hat", "sigma"],
            &[real(&f, &[0.2; 7]), real(&f, &[2.; 7])],
        );
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            call(
                &f,
                &[
                    ("", model(&f)),
                    ("infl", infl),
                    ("res", real(&f, &[1., 2., 3., 4., 5., 6., 7., 8.])),
                ],
            )
        }));
        unsafe {
            crate::mainutils::options::R_SetOptionWarn(previous);
        }
        assert!(
            error
                .unwrap_err()
                .downcast_ref::<crate::sexp::context::RError>()
                .unwrap()
                .message
                .contains("longer object length")
        );
        let (call, _, _) = crate::mainutils::errors::take_recorded_error_call().unwrap();
        assert_eq!(text(&f, &call), STUDENTIZED);
        let pin = unsafe { crate::sexp::owner::OwnerToken::current() }
            .unwrap()
            .weak_owner()
            .unwrap()
            .pin()
            .unwrap();
        assert!(unsafe { (*pin.as_ptr()).error_state.warning_call.is_null() });
        assert_eq!(
            super::call(&f, &[("", model(&f))]).len(),
            8,
            "the same original session recovers after warn=2"
        );
    });
}

/// A genuine ordinary R calling handler evaluates gc(). Its actual collector
/// notification is the host callback seam; no native handler class is forged.
struct WarningTrace {
    args: Sexp<'static>,
    previous_call: Sexp<'static>,
    owner: crate::sexp::owner::WeakOwner,
    facade: std::rc::Weak<std::cell::RefCell<Option<RSession>>>,
    calls: std::cell::RefCell<Vec<String>>,
    action: u8,
    response: crate::sexp::heap::CheckedNode,
}
fn warning_handler_stack(f: &SessionNodeFactory<'_>) -> Sexp<'static> {
    use crate::sexp::{ffi::SEXPTYPE, object::SexpMut};
    // All initialized inputs own their exact original nodes across construction.
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
    let class = f.character("warning").unwrap();
    let entry = f
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, 5)))
        .unwrap();
    let mut entry = SexpMut::try_from_checked(entry).unwrap();
    for (i, value) in [class, env, handler, f.nil(), f.nil()]
        .into_iter()
        .enumerate()
    {
        entry.try_set_vector_elt(i as i64, value).unwrap();
    }
    let entry = entry.freeze();
    // GNU's calling-entry bit and five-slot layout, as used by withCallingHandlers.
    unsafe {
        crate::sexp::accessors::SETLEVELS(entry.as_raw(), 1);
    }
    f.pairlist_cell(&entry, &f.nil(), &f.nil())
        .unwrap()
        .into_owned()
        .unwrap()
}
fn warning_collection(trace: &WarningTrace, instance: *mut crate::sexp::instance::RInstance) {
    let f = trace.owner.node_factory().unwrap();
    let call = unsafe { (*instance).error_state.warning_call.owned() }.unwrap();
    let expected = if trace.calls.borrow().is_empty() {
        STUDENTIZED
    } else {
        PRODUCT
    };
    assert_ne!(call.as_raw(), trace.previous_call.as_raw());
    assert_eq!(
        text(&f, &call),
        expected,
        "gc() runs inside the genuine warning handler"
    );
    trace.calls.borrow_mut().push(expected.into());
    // Clear caller edges only after the operation has captured actual inputs.
    let mut cell = trace.args.clone();
    while !cell.is_nil() {
        unsafe {
            crate::sexp::accessors::SETCAR(cell.as_raw(), f.nil().as_raw());
        }
        cell = cell.try_cdr().unwrap().into_owned().unwrap();
    }
    unsafe {
        crate::sexp::gengc::full_gc();
    }
    assert!(
        trace.response.is_live(),
        "the operation owns the detached residual input"
    );
    assert_eq!(
        text(&f, &call),
        expected,
        "the active warning field retains its syntax through collection"
    );
    match trace.action {
        0 => {}
        1 => std::panic::panic_any(863_u32),
        _ => drop(trace.facade.upgrade().unwrap().borrow_mut().take()),
    }
}
fn real_handler_case(action: u8) {
    use std::{
        cell::RefCell,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let (args, previous_call, owner, response) = {
        let facade = facade.borrow();
        let session = facade.as_ref().unwrap();
        session.with_active(|| {
            let owner = session.owner_token().unwrap();
            let f = owner.node_factory();
            unsafe {
                crate::mainutils::options::R_SetOptionWarn(0);
            }
            let response = real(&f, &[1., 2., 3., 4., 5., 6., 7., 8.]);
            let response_node = response.allocation().unwrap().clone();
            let influence = list(
                &f,
                &["hat", "sigma"],
                &[real(&f, &[0.2; 7]), real(&f, &[2.; 7])],
            );
            let mut args = f.nil();
            for (tag, value) in [("res", response), ("infl", influence), ("", model(&f))] {
                let tag = if tag.is_empty() {
                    f.nil()
                } else {
                    f.wrap(unsafe {
                        crate::sexp::symbol::Rf_install(
                            std::ffi::CString::new(tag).unwrap().as_ptr(),
                        )
                    })
                    .unwrap()
                };
                args = f.pairlist_cell(&value, &args, &tag).unwrap();
            }
            let owner = owner.weak_owner().unwrap();
            let head = f
                .wrap(unsafe {
                    crate::sexp::symbol::Rf_install(c"previous_warning_scope".as_ptr())
                })
                .unwrap();
            let previous = crate::sexp::owner::with_runtime(&owner, |access| {
                access
                    .allocator(&f.domain())
                    .unwrap()
                    .call(&head, &f.nil())
                    .unwrap()
            })
            .unwrap();
            (
                args.into_owned().unwrap(),
                previous.into_owned().unwrap(),
                owner,
                response_node,
            )
        })
    };
    let pin = owner.pin().unwrap();
    let instance = pin.as_ptr();
    let trace = Rc::new(WarningTrace {
        args,
        previous_call,
        owner: owner.clone(),
        facade: Rc::downgrade(&facade),
        calls: RefCell::new(Vec::new()),
        action,
        response,
    });
    unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            let f = owner.node_factory().unwrap();
            let _original =
                crate::mainutils::errors::warning_call_guard(trace.previous_call.as_raw());
            let stack = warning_handler_stack(&f);
            let previous_handlers = (*instance).error_state.handler_stack.clone();
            (*instance).error_state.handler_stack =
                crate::sexp::instance::RuntimeValue::from_owned(stack);
            let callback_trace = trace.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if callback_trace.calls.borrow().len() < 2 {
                    warning_collection(&callback_trace, instance);
                }
            }));
            let result = catch_unwind(AssertUnwindSafe(|| {
                let raw = super::super::mathstats::do_covratio(
                    f.nil().as_raw(),
                    f.nil().as_raw(),
                    trace.args.as_raw(),
                    f.nil().as_raw(),
                );
                f.wrap(raw).unwrap().into_owned().unwrap()
            }));
            // Restore this fixture's exact physical stack even after closure.
            (*instance).error_state.handler_stack = previous_handlers;
            // Physical cleanup remains valid even after revocation and unwind.
            assert_eq!(
                (*instance).error_state.warning_call.as_raw(),
                trace.previous_call.as_raw()
            );
            assert_eq!((*instance).memory_state.in_gc, 0);
            match action {
                0 => {
                    assert_eq!(&*trace.calls.borrow(), &[STUDENTIZED, PRODUCT]);
                    let result = result.unwrap();
                    super::assert_values(
                        &result,
                        &[
                            1.5944636678200694,
                            1.1519999999999997,
                            0.73728000000000016,
                            0.4499999999999999,
                            0.27412254610350989,
                            0.17041420118343195,
                            0.10906508875739646,
                            0.071999999999999981,
                        ],
                    );
                    let warnings = (*instance).error_state.warnings.owned().unwrap();
                    let nodes: Vec<_> = (0..2)
                        .map(|i| {
                            warnings
                                .try_vector_elt(i)
                                .unwrap()
                                .allocation()
                                .unwrap()
                                .clone()
                        })
                        .collect();
                    drop(warnings);
                    crate::sexp::gengc::full_gc();
                    assert!(
                        !trace.response.is_live(),
                        "the completed operation releases its detached input"
                    );
                    assert!(
                        nodes.iter().all(|node| node.is_live()),
                        "the actual deferred warning field owns its calls"
                    );
                    assert_eq!(collected(&f), [STUDENTIZED, PRODUCT]);
                    crate::mainutils::errors::restore_collect_warnings(0);
                    (*instance).error_state.warnings = crate::sexp::instance::RuntimeValue::empty();
                    crate::sexp::gengc::full_gc();
                    assert!(
                        nodes.iter().all(|node| !node.is_live()),
                        "clearing the actual warning field releases the syntax graph"
                    );
                }
                1 => assert_eq!(*result.unwrap_err().downcast::<u32>().unwrap(), 863),
                _ => assert_eq!(
                    result
                        .unwrap_err()
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
fn covratio_condition_calls_own_operands_and_deferred_calls_through_real_handler_collection() {
    real_handler_case(0);
}
#[test]
fn covratio_condition_calls_restore_override_after_live_handler_panic() {
    real_handler_case(1);
}
#[test]
fn covratio_condition_calls_restore_original_override_after_handler_revocation() {
    real_handler_case(2);
}
