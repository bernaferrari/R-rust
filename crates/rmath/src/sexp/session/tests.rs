use super::*;
use crate::sexp::instance::{
    current_instance_ptr, replace_current_instance, with_current_instance,
};
use crate::sexp::protect::{R_PreserveObject, R_ReleaseObject, with_preserved_objects};

#[test]
fn explicit_path_policy_bootstrap_restores_original_dispatch_on_unwind() {
    let prior = RSession::new_for_gc_tests();
    let original = prior.instance_ptr();
    let owner = prior.owner_token().unwrap();
    let result = catch_unwind(AssertUnwindSafe(|| {
        RSession::construct_detached(|| {
            let session = RSession::new_with_path_policy(
                crate::mainutils::paths::RuntimePathPolicy::new(Vec::new(), "/tmp/explicit"),
            );
            assert!(session.library_paths().is_empty());
            assert_eq!(session.temp_dir(), std::path::Path::new("/tmp/explicit"));
            assert!(session.base_env().is_some());
            std::panic::panic_any(0xDADA_3275_u32);
        })
    }));
    let payload = match result {
        Err(payload) => payload,
        Ok(_) => panic!("bootstrap must preserve the injected panic"),
    };
    assert_eq!(payload.downcast_ref::<u32>(), Some(&0xDADA_3275));
    assert_eq!(current_instance_ptr(), Some(original));
    owner.require_active().unwrap();
}

#[test]
fn detached_bootstrap_restores_only_live_prior_owner_on_callback_and_unwind() {
    for destroy_prior in [false, true] {
        for inject_panic in [false, true] {
            let mut prior = Some(RSession::new_for_gc_tests());
            let projection = prior.as_ref().unwrap().instance;
            let allocation = Rc::downgrade(&prior.as_ref().unwrap()._instance_owner);
            let result = catch_unwind(AssertUnwindSafe(|| {
                RSession::construct_detached(|| {
                    assert!(current_instance_ptr().is_none());
                    if destroy_prior {
                        drop(prior.take());
                        assert_eq!(allocation.strong_count(), 1);
                    }
                    let session = RSession::new_for_gc_tests();
                    if inject_panic {
                        panic!("injected detached bootstrap unwind");
                    }
                    session
                })
            }));
            assert_eq!(result.is_err(), inject_panic);
            assert_eq!(
                current_instance_ptr(),
                if destroy_prior {
                    None
                } else {
                    Some(projection)
                }
            );
            assert_eq!(allocation.strong_count(), if destroy_prior { 0 } else { 1 });
            if let Ok(detached) = result {
                drop(detached);
                assert_eq!(
                    current_instance_ptr(),
                    if destroy_prior {
                        None
                    } else {
                        Some(projection)
                    }
                );
            }
            drop(prior);
            assert!(current_instance_ptr().is_none());
        }
    }
}

#[test]
fn managed_runtime_capability_uses_original_allocation_before_bootstrap() {
    let session = RSession::new_without_default_packages();
    let weak = unsafe { (*session.instance).runtime_owner.clone().unwrap() };
    let pin = weak
        .pin()
        .expect("managed owner installed during construction");
    assert_eq!(pin.as_ptr(), session.instance);
    assert!(unsafe { (*pin.as_ptr()).initialized });
    assert_eq!(Rc::strong_count(&session._instance_owner), 2);
    drop(pin);
    assert_eq!(Rc::strong_count(&session._instance_owner), 1);
}

#[test]
fn managed_ambient_operation_pins_callback_dropped_owner_through_unwind() {
    for inject_panic in [false, true] {
        let mut session = Some(RSession::new_for_gc_tests());
        let allocation = Rc::downgrade(&session.as_ref().unwrap()._instance_owner);
        let pointer = session.as_ref().unwrap().instance;
        let availability = unsafe { super::super::instance::instance_liveness(pointer) };
        let result = catch_unwind(AssertUnwindSafe(|| {
            with_current_instance(|active| {
                assert_eq!(active, pointer);
                drop(session.take());
                assert!(!availability.is_live());
                assert!(current_instance_ptr().is_none());
                assert_eq!(allocation.strong_count(), 1);
                // The operation retains the original physical allocation,
                // while revocation prevents starting another operation.
                assert!(with_current_instance(|_| panic!("revoked dispatch")).is_none());
                assert!(unsafe { (*active).runtime_owner.is_some() });
                if inject_panic {
                    panic!("injected owner drop unwind");
                }
            })
            .expect("live ambient owner");
        }));
        assert_eq!(result.is_err(), inject_panic);
        assert_eq!(allocation.strong_count(), 0);
        assert!(current_instance_ptr().is_none());
    }
}

#[test]
fn managed_activation_pins_previous_owner_without_restoring_revoked_state() {
    let current = RSession::new_for_gc_tests();
    let mut previous = Some(RSession::new_for_gc_tests());
    let allocation = Rc::downgrade(&previous.as_ref().unwrap()._instance_owner);
    current.with_active(|| {
        drop(previous.take());
        assert_eq!(allocation.strong_count(), 1);
        assert_eq!(current_instance_ptr(), Some(current.instance));
    });
    assert_eq!(allocation.strong_count(), 0);
    assert!(current_instance_ptr().is_none());
}

#[test]
fn owned_session_projection_survives_moves_and_reentry() {
    let session = RSession::new_for_gc_tests();
    let projection = session.instance_ptr();
    let observer = unsafe { super::super::instance::instance_liveness(projection) };
    let mut moved = Vec::new();
    moved.push(session);
    moved.reserve(32);
    let session = moved.pop().unwrap();
    assert_eq!(session.instance_ptr(), projection);
    assert_eq!(Rc::strong_count(&session._instance_owner), 1);
    session.with_protected(|| {
        assert_eq!(current_instance_ptr(), Some(projection));
        let value = session.global_env().unwrap();
        assert!(value.is_environment());
        session.gc();
        assert!(value.is_environment());
    });
    drop(session);
    assert!(!observer.is_live());
    assert!(current_instance_ptr().is_none());
}

#[test]
fn owner_close_during_activation_never_restores_closed_state() {
    for inject_panic in [false, true] {
        let left = RSession::new_for_gc_tests();
        let mut right = RSession::new_for_gc_tests();
        let observer = unsafe { super::super::instance::instance_liveness(right.instance_ptr()) };
        let result = catch_unwind(AssertUnwindSafe(|| {
            left.with_protected(|| {
                right.close();
                assert!(!observer.is_live());
                assert_eq!(current_instance_ptr(), Some(left.instance_ptr()));
                if inject_panic {
                    panic!("injected close callback unwind");
                }
            });
        }));
        assert_eq!(result.is_err(), inject_panic);
        assert!(current_instance_ptr().is_none());
        assert!(right.owner_token().is_none());
        assert!(catch_unwind(AssertUnwindSafe(|| right.with_active(|| ()))).is_err());
        assert!(current_instance_ptr().is_none());
        // Detachment is idempotent and cannot disturb another owner.
        left.with_active(|| {
            right.close();
        });
        assert!(current_instance_ptr().is_none());
    }
}

#[test]
fn weak_protection_scope_skips_destroyed_owner_even_on_unwind() {
    for inject_panic in [false, true] {
        let session = RSession::new_for_gc_tests();
        let pointer = session.instance_ptr();
        let observer = unsafe { super::super::instance::instance_liveness(pointer) };
        let mut owner = Some(session);
        let mut fresh = None;
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _scope = ProtectScope::new(pointer);
            unsafe {
                super::super::protect::protect_raw_pointer(R_NilValue());
            }
            // This is a user callback's teardown/reentry pattern: the
            // scope retains a weak witness, never the physical allocation.
            drop(owner.take());
            fresh = Some(RSession::new_for_gc_tests());
            unsafe {
                super::super::protect::protect_raw_pointer(R_NilValue());
            }
            if inject_panic {
                panic!("injected scope teardown unwind");
            }
        }));
        assert_eq!(result.is_err(), inject_panic);
        assert!(!observer.is_live());
        assert_eq!(R_ProtectCount(), 1);
        unsafe {
            super::super::protect::unprotect_count(1);
        }
        drop(fresh);
    }
}

#[test]
fn owned_parsed_script_survives_deferred_and_reentrant_collection() {
    let mut session = RSession::new_for_gc_tests();
    let notifications = Rc::new(std::cell::Cell::new(0));
    let observed = notifications.clone();
    super::super::gengc::register_gc_callback(Box::new(move |_| {
        observed.set(observed.get() + 1);
        // Parse results already own leases before the arena lend finishes.
        super::super::gengc::full_gc();
    }));
    unsafe {
        (*session.instance).gc_state.gc_pending = true;
    }
    let (result, _, _) = session.eval_script_with_output_capture("7L; 11L; 19L");
    let result = result.expect("every parsed statement remains alive");
    assert_eq!(result.integer_elt(0), Some(19));
    assert!(notifications.get() > 0);
    super::super::gengc::full_gc();
    assert_eq!(result.integer_elt(0), Some(19));
}

#[test]
fn owned_parsed_single_expression_survives_result_mapping_collection() {
    let mut session = RSession::new_for_gc_tests();
    session.eval_code_with_output_capture_then("23L", |result, _, _| {
        let result = result.expect("owned parse result");
        super::super::gengc::full_gc();
        assert_eq!(result.integer_elt(0), Some(23));
    });
}

#[cfg(feature = "renderplot-device")]
#[test]
fn owned_renderplot_statements_survive_reentrant_gc_and_restore_backend_after_error() {
    use r_graphics_engine::{DrawTarget, Scene};

    // Both concrete backends outlive the session's temporary raw slots.
    let mut previous = Scene::new(64, 48);
    let mut target = Scene::new(96, 72);
    let previous_backend: *mut dyn DrawTarget = &mut previous;
    let backend: *mut dyn DrawTarget = &mut target;
    let mut session = RSession::new_for_gc_tests();
    let instance = session.instance_ptr();
    let notifications = Rc::new(std::cell::Cell::new(0));
    let observed = notifications.clone();
    super::super::gengc::register_gc_callback(Box::new(move |_| {
        observed.set(observed.get() + 1);
        super::super::gengc::full_gc();
    }));
    // SAFETY: this fixture owns the instance and both backend objects for
    // the entire test. No interpreter field or payload loan is retained.
    unsafe {
        (*instance).current_renderplot_backend = Some(previous_backend);
        (*instance).memory_state.gc_force_gap = 1;
        (*instance).memory_state.gc_force_wait = 1;
    }

    let value = session.eval_script_with_output_capture_then_renderplot(
        "7L + 1L; 11L + 2L; 19L + 3L",
        backend,
        |result, _, _| {
            let value = result.expect("all parsed statements survive allocation GC");
            unsafe {
                assert!(!std::ptr::eq(
                    (*instance).current_renderplot_backend.unwrap(),
                    previous_backend,
                ));
                super::super::gengc::full_gc_in(instance);
            }
            assert_eq!(value.try_integer_elt(0).unwrap(), 22);
            value
        },
    );
    unsafe {
        assert!(std::ptr::eq(
            (*instance).current_renderplot_backend.unwrap(),
            previous_backend,
        ));
        super::super::gengc::full_gc_in(instance);
    }
    assert_eq!(value.try_integer_elt(0).unwrap(), 22);
    assert!(notifications.get() >= 3);
    drop(value);

    let error = session.eval_script_with_output_capture_then_renderplot(
        "31L + 1L; undefined_renderplot_gc_symbol; 37L + 1L",
        backend,
        |result, _, _| result.expect_err("undefined symbol must propagate"),
    );
    assert!(error.message.contains("undefined_renderplot_gc_symbol"));
    unsafe {
        assert!(std::ptr::eq(
            (*instance).current_renderplot_backend.unwrap(),
            previous_backend,
        ));
    }

    session.eval_script_with_output_capture_then_renderplot(
        "41L + 1L; 43L + 1L; 47L + 1L",
        backend,
        |result, _, _| {
            let value = result.expect("renderplot evaluation recovers after error");
            unsafe { super::super::gengc::full_gc_in(instance) };
            assert_eq!(value.try_integer_elt(0).unwrap(), 48);
        },
    );
    unsafe {
        assert!(std::ptr::eq(
            (*instance).current_renderplot_backend.unwrap(),
            previous_backend,
        ));
        (*instance).memory_state.gc_force_gap = 0;
        (*instance).current_renderplot_backend = None;
    }
}

#[test]
fn test_session_creation() {
    let session = RSession::new();
    assert!(session.is_active());
    assert!(session.global_env().is_some());
    assert!(session.base_env().is_some());
    assert!(matches!(
        session
            .global_env()
            .expect("session has global env")
            .owner(),
        crate::sexp::object::SexpOwner::Session(_)
    ));
}

#[test]
fn test_detached_session_constructor_restores_current_instance() {
    let current = RSession::new();
    let current_ptr = current.instance_ptr();

    let detached = RSession::new_detached();
    assert!(detached.is_active());
    assert_eq!(current_instance_ptr(), Some(current_ptr));

    let result = detached.with_output_capture(|| {
        crate::sexp::output::capture_stdout("detached");
    });
    assert_eq!(result.1.stdout, "detached");
    assert_eq!(current_instance_ptr(), Some(current_ptr));
}

#[test]
fn test_session_sexp_rejects_foreign_arena_pointer() {
    let mut left = RSession::new();
    let right = RSession::new();

    let ptr = left
        .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
        .expect("left session should be active");

    let left_value = left.sexp(ptr).expect("left owns pointer");
    assert!(matches!(
        left_value.owner(),
        crate::sexp::object::SexpOwner::Session(_)
    ));
    assert!(right.sexp(ptr).is_none());
}

#[test]
fn test_session_eval_sexp_returns_session_scoped_wrapper() {
    let mut session = RSession::new();
    let expr = session
        .with_arena(|arena| {
            crate::sexp::builder::scalar_integer_in(arena, 7)
                .expect("scalar allocation should succeed")
                .as_raw()
        })
        .expect("session should be active");
    let expr = session.sexp(expr).expect("expr belongs to session");

    let result = session
        .eval_sexp(expr)
        .expect("self-evaluating scalar should evaluate");

    assert_eq!(result.clone().integer_elt(0), Some(7));
    assert!(result.is_owner_scoped());
}

#[test]
fn test_session_eval_with_output_capture_returns_typed_wrapper() {
    let mut session = RSession::new();
    let expr = session
        .with_arena(|arena| {
            crate::sexp::builder::scalar_integer_in(arena, 8)
                .expect("scalar allocation should succeed")
                .as_raw()
        })
        .expect("session should be active");
    let expr = session.sexp(expr).expect("expr belongs to session");

    let (result, output, visible) = session.eval_sexp_with_output_capture(expr);
    let result = result.expect("self-evaluating scalar should evaluate");

    assert_eq!(result.integer_elt(0), Some(8));
    assert!(output.stdout.is_empty());
    assert!(visible);
}

#[test]
fn test_session_eval_code_with_output_capture_keeps_parsing_session_owned() {
    let mut session = RSession::new();

    let (result, output, visible) = session.eval_code_with_output_capture("print(1); 2");
    let result = result.expect("source should parse and evaluate");

    assert_eq!(output.stdout, "[1] 1\n");
    assert_eq!(result.real_elt(0), Some(2.0));
    assert!(visible);
}

#[test]
fn test_trailing_empty_actual_uses_formal_default() {
    let mut session = RSession::new();

    let (result, output, visible) = session.eval_code_with_output_capture("identical(0, -0,)");
    let result = result.expect("the empty num.eq actual should select its default");

    assert_eq!(result.logical_elt(0), Some(1));
    assert!(output.stdout.is_empty());
    assert!(visible);
}

#[test]
fn test_closure_trailing_empty_actual_uses_formal_default() {
    let mut session = RSession::new();

    let (result, output, visible) =
        session.eval_code_with_output_capture("(function(x, option = TRUE) option)(0,)");
    let result = result.expect("an empty actual should select the closure formal's default");

    assert_eq!(result.logical_elt(0), Some(1));
    assert!(output.stdout.is_empty());
    assert!(visible);
}

#[test]
fn test_parenthesized_assignment_is_visible() {
    let mut session = RSession::new();

    let (result, output, visible) =
        session.eval_script_with_output_capture("(x <- 0 * (-1)); invisible(NULL)");
    result.expect("parenthesized assignment should evaluate");

    assert_eq!(output.stdout, "[1] 0\n");
    assert!(!visible, "the final invisible(NULL) remains invisible");
}

#[test]
fn test_script_auto_print_keeps_separator_after_attributes_list() {
    let mut session = RSession::new();

    let (result, output, visible) = session.eval_script_with_output_capture(
        "attributes(structure(1:2, tsp = c(1, 2, 1), class = 'ts')); \
             cat('after\\n'); invisible(NULL)",
    );
    result.expect("attributes list and following expression should evaluate");

    assert_eq!(
        output.stdout,
        "$tsp\n[1] 1 2 1\n\n$class\n[1] \"ts\"\n\nafter\n"
    );
    assert!(!visible);
}

#[test]
fn test_session_script_parse_error_maps_to_upstream_message() {
    let mut session = RSession::new();

    // Parse failures must surface as a clean REvalError rendered like
    // Rscript ("Error: unexpected ')' in ..."), never as a panic from
    // the error-conversion path touching unscoped runtime state.
    let (result, output, visible) = session.eval_script_with_output_capture("print(1 + )");
    let err = result.expect_err("syntax error must fail the script");
    assert_eq!(err.message, "unexpected ')' in \"print(1 + )\"");
    assert!(output.stdout.is_empty());
    assert!(!visible);

    // The session stays usable after a parse failure.
    let (result, output, _) = session.eval_script_with_output_capture("print(40 + 2)");
    assert_eq!(
        result.expect("session should recover").real_elt(0),
        Some(42.0)
    );
    assert_eq!(output.stdout, "[1] 42\n");
}

#[test]
fn test_session_script_parse_error_at_end_of_input_has_no_context() {
    let mut session = RSession::new();

    let (result, _, _) = session.eval_script_with_output_capture("1 + +");
    let err = result.expect_err("incomplete input must fail the script");
    assert_eq!(err.message, "unexpected end of input");
}

#[test]
fn test_session_malformed_script_has_no_prefix_side_effects() {
    let mut session = RSession::new();

    for (name, malformed) in [
        ("unterminated_string_side_effect", "\"unterminated"),
        ("unknown_input_side_effect", "\0"),
    ] {
        let script = format!("{name} <- 1; {malformed}");
        let (result, output, visible) = session.eval_script_with_output_capture(&script);
        assert!(result.is_err(), "malformed script unexpectedly succeeded");
        assert!(output.stdout.is_empty());
        assert!(!visible);
        drop(result);

        let probe = format!("exists(\"{name}\")");
        let (result, _, _) = session.eval_script_with_output_capture(&probe);
        assert_eq!(
            result
                .expect("existence probe should evaluate")
                .logical_elt(0),
            Some(crate::sexp::ffi::FALSE),
            "valid prefix mutated the session before the parse failure"
        );
    }
}

#[test]
fn test_session_close() {
    let mut session = RSession::new();
    assert!(session.is_active());
    session.close();
    assert!(!session.is_active());
}

#[test]
fn test_session_close_non_current_keeps_current_instance() {
    let mut older = RSession::new();
    let newer = RSession::new();
    let newer_instance = newer.instance_ptr();

    let previous = unsafe { replace_current_instance(Some(newer.instance_ptr())) };
    older.close();

    assert!(
        with_current_instance(|inst| std::ptr::eq(inst as *const RInstance, newer_instance))
            .unwrap_or(false)
    );
    assert!(newer.is_active());
    unsafe {
        replace_current_instance(previous);
    }
}

#[test]
fn test_session_drop_non_current_keeps_current_instance() {
    let older = RSession::new();
    let newer = RSession::new();
    let newer_instance = newer.instance_ptr();

    let previous = unsafe { replace_current_instance(Some(newer.instance_ptr())) };
    drop(older);

    assert!(
        with_current_instance(|inst| std::ptr::eq(inst as *const RInstance, newer_instance))
            .unwrap_or(false)
    );
    assert!(newer.is_active());
    unsafe {
        replace_current_instance(previous);
    }
}

#[test]
fn test_explicit_gc_owner_restores_nested_activation() {
    let left = RSession::new_without_default_packages();
    let right = RSession::new_without_default_packages();
    left.with_active_in(|left_inst| {
        unsafe {
            (*left_inst).gc_state.gc_pending = true;
        }
        right.with_active_in(|right_inst| unsafe {
            assert_eq!(with_current_instance(|inst| inst), Some(right_inst));
            (*right_inst).gc_state.gc_pending = true;
            // An evaluation frame must defer collection for this owner.
            (*right_inst).eval_state.eval_depth = 1;
            super::super::gengc::run_pending_gc_if_quiescent_in(right_inst);
            assert!((*right_inst).gc_state.gc_pending);
            (*right_inst).eval_state.eval_depth = 0;
            super::super::gengc::run_pending_gc_if_quiescent_in(right_inst);
            assert!(!(*right_inst).gc_state.gc_pending);
            assert!((*left_inst).gc_state.gc_pending);
        });
        assert_eq!(with_current_instance(|inst| inst), Some(left_inst));
        unsafe {
            super::super::gengc::run_pending_gc_if_quiescent_in(left_inst);
            assert!(!(*left_inst).gc_state.gc_pending);
        }
        // A callback unwind must also restore the outer owner.
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            right.with_active_in(|_| panic!("nested activation"));
        }));
        assert!(panic.is_err());
        assert_eq!(with_current_instance(|inst| inst), Some(left_inst));
    });
}

#[test]
fn test_session_method_activation_restores_previous_instance() {
    let older = RSession::new();
    let older_instance = older.instance_ptr();
    let newer = RSession::new();
    let newer_instance = newer.instance_ptr();

    let previous = unsafe { replace_current_instance(Some(newer.instance_ptr())) };
    assert!(
        current_instance_ptr()
            .map(|ptr| std::ptr::eq(ptr as *const RInstance, newer_instance))
            .unwrap_or(false)
    );

    let activated_older = older.with_active(|| {
        current_instance_ptr()
            .map(|ptr| std::ptr::eq(ptr as *const RInstance, older_instance))
            .unwrap_or(false)
    });

    assert!(activated_older);
    assert!(
        current_instance_ptr()
            .map(|ptr| std::ptr::eq(ptr as *const RInstance, newer_instance))
            .unwrap_or(false)
    );
    unsafe {
        replace_current_instance(previous);
    }
}

#[test]
fn test_session_isolates_srcref_and_handler_bookkeeping_on_one_thread() {
    let outer = RSession::new();
    let inner = RSession::new();
    outer.with_active(|| unsafe {
        (*outer.instance_ptr()).error_state.current_srcref_location = Some(("outer.R".into(), 7));
        (*outer.instance_ptr())
            .error_state
            .try_catch_handler_classes
            .push(vec!["warning".into()]);
        (*outer.instance_ptr())
            .error_state
            .calling_handlers_signaled = true;
    });
    inner.with_active(|| unsafe {
        assert_eq!(
            (*inner.instance_ptr()).error_state.current_srcref_location,
            None
        );
        assert!(
            (*inner.instance_ptr())
                .error_state
                .try_catch_handler_classes
                .is_empty()
        );
        assert!(
            !(*inner.instance_ptr())
                .error_state
                .calling_handlers_signaled
        );
    });
    outer.with_active(|| unsafe {
        assert_eq!(
            (*outer.instance_ptr()).error_state.current_srcref_location,
            Some(("outer.R".into(), 7))
        );
        assert_eq!(
            (*outer.instance_ptr())
                .error_state
                .try_catch_handler_classes,
            vec![vec![String::from("warning")]]
        );
        assert!(
            (*outer.instance_ptr())
                .error_state
                .calling_handlers_signaled
        );
    });
}

#[test]
fn test_session_output_capture_is_local_on_same_thread() {
    let left = RSession::new();
    let right = RSession::new();

    let (_, left_output) = left.with_output_capture(|| {
        crate::sexp::output::capture_stdout("left out");
        crate::sexp::output::capture_stderr("left err");
    });
    let (_, right_output) = right.with_output_capture(|| {
        crate::sexp::output::capture_stdout("right out");
        crate::sexp::output::capture_stderr("right err");
    });

    assert_eq!(left_output.stdout, "left out");
    assert_eq!(left_output.stderr, "left err");
    assert_eq!(right_output.stdout, "right out");
    assert_eq!(right_output.stderr, "right err");
}

#[test]
fn test_session_eval_limits_are_local_on_same_thread() {
    let mut left = RSession::new();
    let mut right = RSession::new();

    left.set_eval_limits(crate::eval::eval::EvalLimits {
        max_eval_depth: 7,
        max_execution_time_ms: 0,
        max_alloc_bytes: 0,
    });
    right.set_eval_limits(crate::eval::eval::EvalLimits {
        max_eval_depth: 19,
        max_execution_time_ms: 0,
        max_alloc_bytes: 0,
    });

    assert_eq!(left.eval_limits().max_eval_depth, 7);
    assert_eq!(right.eval_limits().max_eval_depth, 19);
    assert_eq!(
        left.with_active(crate::eval::eval::get_eval_limits)
            .max_eval_depth,
        7
    );
    assert_eq!(
        right
            .with_active(crate::eval::eval::get_eval_limits)
            .max_eval_depth,
        19
    );

    left.reset_eval_limits();
    assert_eq!(left.eval_limits(), crate::eval::eval::EvalLimits::default());
    assert_eq!(right.eval_limits().max_eval_depth, 19);
}

#[test]
fn test_eval_depth_guard_drops_against_original_session() {
    let left = RSession::new();
    let right = RSession::new();

    left.with_active(|| {
        let guard = crate::eval::eval::check_eval_depth().expect("left depth should increment");
        assert_eq!(unsafe { (*left.instance).eval_state.eval_depth }, 1);
        assert_eq!(unsafe { (*right.instance).eval_state.eval_depth }, 0);

        let previous = unsafe { replace_current_instance(Some(right.instance_ptr())) };
        drop(guard);
        assert_eq!(unsafe { (*left.instance).eval_state.eval_depth }, 0);
        assert_eq!(unsafe { (*right.instance).eval_state.eval_depth }, 0);
        unsafe {
            replace_current_instance(previous);
        }
    });
}

#[test]
fn test_eval_timer_guard_drops_against_original_session() {
    let left = RSession::new();
    let right = RSession::new();

    left.with_active(|| {
        let guard = crate::eval::eval::EvalTimerGuard::start_if_needed();
        assert!(unsafe { (*left.instance).eval_state.start_time.is_some() });
        assert!(unsafe { (*right.instance).eval_state.start_time.is_none() });

        let previous = unsafe { replace_current_instance(Some(right.instance_ptr())) };
        drop(guard);
        assert!(unsafe { (*left.instance).eval_state.start_time.is_none() });
        assert!(unsafe { (*right.instance).eval_state.start_time.is_none() });
        unsafe {
            replace_current_instance(previous);
        }
    });
}

#[test]
fn test_eval_with_limits_restores_session_state() {
    let mut session = RSession::new();
    let original_limits = crate::eval::eval::EvalLimits {
        max_eval_depth: 11,
        max_execution_time_ms: 0,
        max_alloc_bytes: 0,
    };
    session.set_eval_limits(original_limits);
    unsafe { (*session.instance).eval_state.start_time = Some(std::time::Instant::now()) };

    let expr = session
        .with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
        .expect("session should be active");
    let expr = session.sexp(expr).expect("expr belongs to session");
    assert!(unsafe {
        /* SAFETY: fresh fixture with no borrowed payload views. */
        expr.clone().set_integer_elt(0, 7)
    });
    let env = session.global_env().expect("session has global env");

    let result = session.with_active(|| unsafe {
        crate::eval::eval::eval_with_limits(
            expr,
            env,
            crate::eval::eval::EvalLimits {
                max_eval_depth: 3,
                max_execution_time_ms: 1_000,
                max_alloc_bytes: 0,
            },
        )
    });
    let result = result.expect("self-evaluating vector should evaluate");
    assert_eq!(result.integer_elt(0), Some(7));
    assert_eq!(
        unsafe { (*session.instance).eval_state.limits },
        original_limits
    );
    assert!(unsafe { (*session.instance).eval_state.start_time.is_some() });
}

#[test]
fn test_session_cancellation_token_is_session_owned() {
    let mut cancelled = RSession::new();
    let mut active = RSession::new();
    let token = CancellationToken::cancelled();

    cancelled.set_cancellation_token(Some(token));
    let (result, _, _) = cancelled.eval_code_with_output_capture("1 + 1");
    let err = result.expect_err("cancelled session should reject eval");
    assert_eq!(err.message, "operation cancelled");

    let (result, _, _) = active.eval_code_with_output_capture("1 + 1");
    let result = result.expect("second session should not inherit cancellation");
    assert_eq!(result.real_elt(0), Some(2.0));
}

#[test]
fn test_session_eval_closed() {
    let mut session = RSession::new();
    session.close();
    let result = session.eval(ptr::null_mut());
    assert!(result.is_err());
}

#[test]
fn test_session_define_and_find_var_with_rust_str() {
    let mut session = RSession::new();
    let value = session
        .with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
        .expect("session should be active");
    let sexp = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(value) }.expect("integer vector allocation failed");
    assert!(unsafe {
        /* SAFETY: fresh fixture with no borrowed payload views. */
        sexp.set_integer_elt(0, 42)
    });

    let value = session.sexp(value).expect("value belongs to session");
    assert!(session.define_var("session_defined_value", value));

    let found = session
        .find_var("session_defined_value")
        .expect("defined value should be found");
    assert_eq!(found.integer_elt(0), Some(42));
}

#[test]
fn test_session_define_var_interns_symbol_in_target_session() {
    let mut older = RSession::new();
    let newer = RSession::new();

    let value = older
        .with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
        .expect("older session should be active");
    let sexp = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(value) }.expect("integer vector allocation failed");
    assert!(unsafe {
        /* SAFETY: fresh fixture with no borrowed payload views. */
        sexp.set_integer_elt(0, 123)
    });

    let value = older.sexp(value).expect("value belongs to older session");
    assert!(older.define_var("session_local_symbol", value));

    let found = older
        .find_var("session_local_symbol")
        .expect("older session should own symbol binding");
    assert_eq!(found.integer_elt(0), Some(123));
    assert!(newer.find_var("session_local_symbol").is_none());
}

#[test]
fn test_session_define_var_rejects_foreign_value() {
    let mut owner = RSession::new();
    let other = RSession::new();

    let value = owner
        .with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
        .expect("owner session should be active");
    let value = owner.sexp(value).expect("value belongs to owner");

    assert!(!other.define_var("foreign_value", value));
    assert!(other.find_var("foreign_value").is_none());
}

#[test]
fn test_session_rejects_interior_nul_symbol_names() {
    let mut session = RSession::new();
    let value = session
        .with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
        .expect("session should be active");
    let sexp = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(value) }.expect("integer vector allocation failed");
    assert!(unsafe {
        /* SAFETY: fresh fixture with no borrowed payload views. */
        sexp.set_integer_elt(0, 99)
    });

    let value = session.sexp(value).expect("value belongs to session");
    assert!(!session.define_var("session_bad\0name", value));

    assert!(session.find_var("session_bad\0name").is_none());
    assert!(session.find_var("session_bad").is_none());
}

#[test]
fn test_session_protected_scope() {
    let mut session = RSession::new();
    let first = session
        .with_arena(|arena| arena.alloc_node(crate::sexp::ffi::SEXPTYPE::INTSXP))
        .unwrap();
    let second = session
        .with_arena(|arena| arena.alloc_node(crate::sexp::ffi::SEXPTYPE::REALSXP))
        .unwrap();
    let depth_before = session.with_active(R_ProtectCount);
    session.with_protected(|| {
            std::mem::forget(unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect(first) });
            std::mem::forget(unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect(second) });
            // Leaked guards land in the root table; the count-based legacy
            // view does not see them.
            crate::sexp::protect::with_protected_objects(|legacy, roots| {
                assert!(legacy.is_empty());
                assert_eq!(roots.len(), 2);
            });
        });
    let depth_after = session.with_active(R_ProtectCount);
    assert_eq!(depth_before, depth_after);
    // The scope unwind truncated the leaked roots away.
    crate::sexp::protect::with_protected_objects(|_, roots| {
        assert!(roots.iter().all(|&p| p.is_null()))
    });
}

#[test]
fn test_session_protected_scope_unwinds_on_panic() {
    let mut session = RSession::new();
    let value = session
        .with_arena(|arena| arena.alloc_node(crate::sexp::ffi::SEXPTYPE::INTSXP))
        .unwrap();
    let depth_before = session.with_active(R_ProtectCount);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        session.with_protected(|| {
                std::mem::forget(unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect(value) });
                panic!("forced protected-scope unwind");
            });
    }));

    assert!(result.is_err());
    let depth_after = session.with_active(R_ProtectCount);
    assert_eq!(depth_before, depth_after);
    crate::sexp::protect::with_protected_objects(|_, roots| {
        assert!(roots.iter().all(|&p| p.is_null()))
    });
}

#[test]
fn test_session_protect_and_preserve_are_local_on_same_thread() {
    let mut left = RSession::new();
    let right = RSession::new();
    let protected = left
        .with_arena(|arena| arena.alloc_node(crate::sexp::ffi::SEXPTYPE::INTSXP))
        .unwrap();
    let preserved = left
        .with_arena(|arena| arena.alloc_node(crate::sexp::ffi::SEXPTYPE::REALSXP))
        .unwrap();

    left.with_active(|| {
            std::mem::forget(unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect(protected) });
            unsafe {
                R_PreserveObject(preserved);
            }
            crate::sexp::protect::with_protected_objects(|legacy, roots| {
                assert!(legacy.is_empty());
                assert_eq!(roots, &[protected]);
            });
            with_preserved_objects(|objects| assert_eq!(objects, &[preserved]));
        });

    right.with_active(|| {
        crate::sexp::protect::with_protected_objects(|legacy, roots| {
            assert!(legacy.is_empty());
            assert!(roots.is_empty());
        });
        with_preserved_objects(|objects| assert!(objects.is_empty()));
    });

    let nested = left
        .with_arena(|arena| arena.alloc_node(crate::sexp::ffi::SEXPTYPE::INTSXP))
        .unwrap();
    left.with_active(|| {
            // A nested scope reclaims only ITS OWN leaks; the root leaked
            // outside any scope survives until session teardown.
            left.with_protected(|| {
                std::mem::forget(unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect(nested) });
            });
            crate::sexp::protect::with_protected_objects(|_, roots| {
                assert_eq!(roots, &[protected]);
            });
            unsafe {
                R_ReleaseObject(preserved);
            }
            with_preserved_objects(|objects| assert!(objects.is_empty()));
        });
}

#[test]
fn test_session_gc() {
    let session = RSession::new();
    // Should not panic
    session.gc();
}

#[test]
fn test_session_arena_budget_controls_future_allocations() {
    let mut session = RSession::new();
    let node_bytes = std::mem::size_of::<crate::sexp::ffi::SexprecCore>();
    let current_bytes = session
        .with_arena(|arena| arena.total_bytes_allocated())
        .unwrap();
    let current_nodes = session.with_arena(|arena| arena.node_count()).unwrap();
    let budget = ArenaBudget::new(current_bytes + node_bytes, current_nodes + 1);
    session.set_arena_budget(budget);
    assert_eq!(session.arena_budget(), budget);

    session
        .with_arena(|arena| {
            assert!(!arena.alloc_node(SEXPTYPE::INTSXP).is_null());
            assert!(arena.alloc_node(SEXPTYPE::REALSXP).is_null());
        })
        .unwrap();
}

#[test]
fn test_session_default() {
    let session = RSession::default();
    assert!(session.is_active());
}

// -----------------------------------------------------------------------
// Concurrent session stress tests
// -----------------------------------------------------------------------

#[test]
fn concurrent_sessions_are_independent() {
    // Spawn 4 threads, each creating their own RSession, defining a
    // unique variable, evaluating code, and verifying isolation.
    let handles: Vec<_> = (0..4)
            .map(|i| {
                std::thread::spawn(move || {
                    let mut session = RSession::new();
                    assert!(session.is_active());
                    assert!(session.global_env().is_some());

                    // Define a variable unique to this thread/session
                    let var_name = format!("thread_var_{i}");
                    let value = session
                        .with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
                        .expect("session should be active");
                    let sexp = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(value) }.expect("allocation failed");
                    assert!(unsafe { /* SAFETY: fresh fixture with no borrowed payload views. */ sexp.set_integer_elt(0, (i + 100) as i32) });

                    let value = session.sexp(value).expect("value belongs to session");
                    assert!(session.define_var(&var_name, value));

                    // Verify the variable
                    let found = session
                        .find_var(&var_name)
                        .expect("should find the variable we just defined");
                    assert_eq!(
                        found.integer_elt(0),
                        Some((i + 100) as i32),
                        "thread {i} got wrong value"
                    );

                    // Verify other threads' variables are NOT visible
                    for j in 0..4 {
                        if j != i {
                            let other_name = format!("thread_var_{j}");
                            assert!(
                                session.find_var(&other_name).is_none(),
                                "thread {i} should NOT see thread {j}'s variable"
                            );
                        }
                    }

                    drop(found);
                    session.close();
                    assert!(!session.is_active());
                    (i, true)
                })
            })
            .collect();

    for h in handles {
        let (thread_id, ok) = h.join().expect("thread should not panic");
        assert!(ok, "thread {thread_id} failed");
    }
}

#[test]
fn concurrent_sessions_eval_code_independently() {
    // Each thread evaluates R code and checks isolation of captured output.
    let handles: Vec<_> = (0..4)
        .map(|i| {
            std::thread::spawn(move || {
                let mut session = RSession::new();

                let code = format!("{i} + 100");
                let (result, _output, _visible) = session.eval_code_with_output_capture(&code);
                let result = result.expect("eval should succeed");
                assert_eq!(
                    result.real_elt(0),
                    Some((i + 100) as f64),
                    "thread {i} computed wrong result"
                );
                drop(result);
                session.close();
            })
        })
        .collect();

    for h in handles {
        h.join().expect("thread should not panic");
    }
}

#[test]
fn concurrent_sessions_output_capture_isolated() {
    // Verify that output capture in one session doesn't leak to another
    // session on a different thread.
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let barrier_clone = barrier.clone();

    let h1 = std::thread::spawn(move || {
        let session = RSession::new();
        barrier_clone.wait(); // synchronize start
        let (_, output) = session.with_output_capture(|| {
            crate::sexp::output::capture_stdout("thread_one_output");
        });
        assert_eq!(output.stdout, "thread_one_output");
        // Must NOT contain thread_two's output
        assert!(
            !output.stdout.contains("thread_two"),
            "output leaked between threads"
        );
    });

    let h2 = std::thread::spawn(move || {
        let session = RSession::new();
        barrier.wait(); // synchronize start
        let (_, output) = session.with_output_capture(|| {
            crate::sexp::output::capture_stdout("thread_two_output");
        });
        assert_eq!(output.stdout, "thread_two_output");
        assert!(
            !output.stdout.contains("thread_one"),
            "output leaked between threads"
        );
    });

    h1.join().expect("thread 1 panicked");
    h2.join().expect("thread 2 panicked");
}

#[test]
fn concurrent_sessions_rng_independent() {
    // RNG state should be per-session. Two threads with the same seed
    // should get the same sequence, and different threads should not
    // interfere with each other.
    let handles: Vec<_> = (0..2)
        .map(|i| {
            std::thread::spawn(move || {
                let session = RSession::new();
                session.set_seed(42, 42);
                let r1 = session.unif_rand();
                let r2 = session.unif_rand();
                // Same seed should give same first value across sessions
                (i, r1, r2)
            })
        })
        .collect();

    let results: Vec<_> = handles
        .into_iter()
        .map(|h| h.join().expect("thread panicked"))
        .collect();

    // Both threads used seed (42,42), so they should get the same sequence
    assert_eq!(
        results[0].1, results[1].1,
        "same seed should produce same first random number"
    );
    assert_eq!(
        results[0].2, results[1].2,
        "same seed should produce same second random number"
    );
}
