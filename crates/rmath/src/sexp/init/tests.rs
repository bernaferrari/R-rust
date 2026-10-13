//! init tests — extracted verbatim from init.rs.

use super::super::ffi::SEXPTYPE;
use super::super::globals::{
    R_BaseEnv, R_BaseEnv_in, R_EmptyEnv, R_EmptyEnv_in, R_GlobalEnv, R_GlobalEnv_in,
};

use super::super::symbol::Rf_install;
use super::*;

#[test]
fn test_initialize_sets_environments() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        initialize_r();

        let global = R_GlobalEnv();
        let base = R_BaseEnv();
        let empty = R_EmptyEnv();

        assert!(!global.is_null());
        assert!(!base.is_null());
        assert!(!empty.is_null());

        assert_eq!((*global).sxpinfo.type_of(), SEXPTYPE::ENVSXP);
        assert_eq!((*base).sxpinfo.type_of(), SEXPTYPE::ENVSXP);
        assert_eq!((*empty).sxpinfo.type_of(), SEXPTYPE::ENVSXP);

        assert!(is_initialized());

        shutdown_r();
    }
}

#[test]
fn test_initialize_base_bindings_use_canonical_primitive_identity() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        initialize_r();

        let base = R_BaseEnv();
        let plus = Rf_install(c"+".as_ptr());
        let if_sym = Rf_install(c"if".as_ptr());
        let log = Rf_install(c"log".as_ptr());

        let plus_val = crate::sexp::envir::R_findVarInFrame(base, plus);
        let if_val = crate::sexp::envir::R_findVarInFrame(base, if_sym);
        let log_val = crate::sexp::envir::R_findVarInFrame(base, log);

        assert_eq!(
            crate::eval::primitive::PrimitiveDescriptor::from_raw(plus_val)
                .expect("+ primitive descriptor")
                .name,
            "+"
        );
        assert_eq!(
            crate::eval::primitive::PrimitiveDescriptor::from_raw(if_val)
                .expect("if primitive descriptor")
                .name,
            "if"
        );
        assert!(
            crate::eval::primitive::PrimitiveDescriptor::from_raw(log_val).is_none(),
            "direct log binding is an evaluator helper, not a canonical R primitive"
        );
        assert!(crate::sexp::accessors::PRIMOFFSET(log_val) <= -2);
        assert_eq!(
            crate::eval::primitive::portable_primitive_name(
                crate::sexp::object::Sexp::try_from_raw(log_val).unwrap()
            )
            .as_deref(),
            Some("log")
        );

        shutdown_r();
    }
}

#[test]
fn test_null_coalescing_base_function_is_lazy_and_null_specific() {
    let mut session = crate::sexp::session::RSession::new();

    let (result, _, _) = session.eval_code_with_output_capture("NULL %||% 42L");
    assert_eq!(
        result
            .expect("NULL should select the fallback")
            .integer_elt(0),
        Some(42)
    );

    let (result, _, _) =
        session.eval_code_with_output_capture("1L %||% missing_fallback_must_not_be_forced");
    assert_eq!(
        result
            .expect("a non-NULL left operand must not force the fallback")
            .integer_elt(0),
        Some(1)
    );

    let (result, _, _) = session.eval_code_with_output_capture("FALSE %||% 42L");
    assert_eq!(
        result
            .expect("false is a value, not a null operand")
            .logical_elt(0),
        Some(FALSE)
    );
}

#[test]
fn test_as_is_base_function_preserves_and_prepends_explicit_classes() {
    let mut session = crate::sexp::session::RSession::new();

    let (result, _, _) = session.eval_code_with_output_capture(
        r#"
                x <- structure(1:3, names = c("a", "b", "c"), class = c("foo", "AsIs", "bar"))
                y <- I(x)
                identical(oldClass(y), c("AsIs", "foo", "bar")) &&
                    identical(names(y), names(x)) &&
                    identical(oldClass(I(y)), oldClass(y))
            "#,
    );
    assert_eq!(
        result
            .expect("I() should preserve attributes and prepend one AsIs class")
            .logical_elt(0),
        Some(TRUE)
    );

    let (result, _, _) = session.eval_code_with_output_capture(
        r#"
                y <- I(matrix(1:4, 2L, 2L))
                identical(oldClass(y), "AsIs") && identical(dim(y), c(2L, 2L))
            "#,
    );
    assert_eq!(
        result
            .expect("I() should add AsIs without discarding unrelated attributes")
            .logical_elt(0),
        Some(TRUE)
    );
}

#[test]
fn test_gnu_eval_which_stopifnot_are_base_closures() {
    let mut session = crate::sexp::session::RSession::new();

    let (result, _, _) = session.eval_code_with_output_capture(
            r#"
                exists("eval", baseenv(), inherits = FALSE) &&
                    exists("evalq", baseenv(), inherits = FALSE) &&
                    exists("eval.parent", baseenv(), inherits = FALSE) &&
                    exists("which", baseenv(), inherits = FALSE) &&
                    exists("stopifnot", baseenv(), inherits = FALSE) &&
                    exists("parent.frame", baseenv(), inherits = FALSE) &&
                    identical(typeof(eval), "closure") &&
                    identical(typeof(which), "closure") &&
                    identical(typeof(stopifnot), "closure") &&
                    identical(typeof(parent.frame), "closure") &&
                    identical(eval(1 + 1), 2) &&
                    identical(eval(quote(1 + 1)), 2) &&
                    identical(evalq(a, list(a = 3L)), 3L) &&
                    identical(which(c(TRUE, FALSE, TRUE)), c(1L, 3L)) &&
                    identical(cumsum(c(1+1i, 2+2i)), c(1+1i, 3+3i)) &&
                    is.null(stopifnot(TRUE)) &&
                    inherits(try(stopifnot(FALSE), silent = TRUE), "try-error") &&
                    (function() identical(parent.frame(), .GlobalEnv))() &&
                    identical((function() { x <- 10L; (function() eval.parent(quote(x)))() })(), 10L) &&
                    exists("sys.nframe", baseenv(), inherits = FALSE) &&
                    identical(typeof(sys.nframe), "closure") &&
                    inherits(try(sys.nframe(1), silent = TRUE), "try-error") &&
                    is.integer((function() sys.nframe())())


            "#,
        );
    assert_eq!(
        result
            .expect("GNU eval/which/stopifnot should be base closures")
            .logical_elt(0),
        Some(TRUE)
    );
}

#[test]
fn test_initialize_installs_machine_constants() {
    let mut session = crate::sexp::session::RSession::new();

    let (result, _, _) = session.eval_code_with_output_capture(".Machine$double.eps");
    assert_eq!(
        result
            .expect(".Machine should be installed in the base environment")
            .real_elt(0),
        Some(f64::EPSILON)
    );
}

#[test]
fn test_initialize_installs_live_options_binding() {
    let mut session = crate::sexp::session::RSession::new();

    let (result, _, _) = session.eval_code_with_output_capture(".Options$width");
    assert_eq!(
        result
            .expect(".Options should be installed in the base environment")
            .integer_elt(0),
        Some(80)
    );

    let (result, _, _) =
        session.eval_code_with_output_capture("options(rErr.eps = 1e-30); .Options$rErr.eps");
    assert_eq!(
        result
            .expect("options() should refresh the .Options binding")
            .real_elt(0),
        Some(1e-30)
    );
}

#[test]
fn test_options_binding_refresh_survives_gc_torture() {
    let mut session = crate::sexp::session::RSession::new();

    let (result, _, _) = session.eval_code_with_output_capture(
            "gctorture(TRUE); options(alpha = 20L, beta = 22L); value <- .Options$alpha + .Options$beta; gctorture(FALSE); value",
        );
    assert_eq!(
        result
            .expect("the refreshed .Options pairlist should remain rooted")
            .integer_elt(0),
        Some(42)
    );
}

#[test]
fn test_idempotent() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        initialize_r();
        let g1 = R_GlobalEnv();

        initialize_r();
        let g2 = R_GlobalEnv();

        assert_eq!(g1, g2);

        shutdown_r();
    }
}

#[test]
fn test_shutdown_clears() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        initialize_r();
        assert!(is_initialized());

        shutdown_r();
        assert!(!is_initialized());
        assert!(!R_GlobalEnv().is_null());
    }
}

#[test]
fn test_pre_interned_symbols() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        initialize_r();

        let plus = Rf_install(c"+".as_ptr());
        assert!(!plus.is_null());

        let plus2 = Rf_install(c"+".as_ptr());
        assert_eq!(plus, plus2);

        let if_sym = Rf_install(c"if".as_ptr());
        assert!(!if_sym.is_null());

        shutdown_r();
    }
}

#[test]
fn test_environment_chain() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        use super::super::globals::R_NilValue;
        initialize_r();

        let global = R_GlobalEnv();
        let base = R_BaseEnv();
        let empty = R_EmptyEnv();

        // GNU wires the "Autoloads" environment (and, for full
        // sessions, attached package environments) between .GlobalEnv
        // and base, so ENCLOS(.GlobalEnv) is not base itself; the
        // search path must terminate at base without cycling.
        let mut env = crate::sexp::accessors::ENCLOS(global);
        let mut hops = 0;
        while env != base {
            assert!(!env.is_null(), "search path must reach base");
            env = crate::sexp::accessors::ENCLOS(env);
            hops += 1;
            assert!(hops < 64, "search path must not cycle");
        }
        assert_eq!(crate::sexp::accessors::ENCLOS(base), empty);
        assert_eq!(crate::sexp::accessors::ENCLOS(empty), R_NilValue());

        shutdown_r();
    }
}

#[test]
fn managed_initialization_does_not_restore_previous_owner_dropped_by_callback() {
    let previous = std::rc::Rc::new(std::cell::RefCell::new(Some(
        crate::sexp::session::RSession::new_for_gc_tests(),
    )));
    let target = crate::sexp::session::RSession::new_for_gc_tests();
    let fired = std::rc::Rc::new(std::cell::Cell::new(false));
    let target_pointer = target.with_active_in(|instance| unsafe {
        let previous = previous.clone();
        let fired = fired.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            if fired.replace(true) {
                return;
            }
            // This regression targets the first collecting callback;
            // subsequent bootstrap allocations need ordinary GC policy.
            (*instance).memory_state.gc_force_gap = 0;
            (*instance).memory_state.gc_force_wait = 0;
            let old = previous.borrow_mut().take();
            drop(old);
            crate::sexp::gengc::full_gc();
        }));
        (*instance).memory_state.gc_force_gap = 1;
        (*instance).memory_state.gc_force_wait = 1;
        instance
    });
    // Make the previous owner ambient without retaining an activation
    // guard outside initializer scope; the initializer must own its pin.
    let previous_pointer = previous
        .borrow()
        .as_ref()
        .unwrap()
        .with_active_in(|instance| instance);
    unsafe {
        super::super::instance::replace_current_instance(Some(previous_pointer));
    }
    unsafe {
        initialize_r_in(target_pointer);
    }
    assert!(fired.get(), "bootstrap must exercise a collecting callback");
    assert!(previous.borrow().is_none());
    assert!(
        current_instance_ptr().is_none(),
        "revoked previous owner must not be restored"
    );
    assert!(is_initialized_in(target_pointer));
}

#[test]
fn managed_initialization_rejects_callback_revocation_before_publication() {
    let target = crate::sexp::session::RSession::new_for_gc_tests();
    let fired = std::rc::Rc::new(std::cell::Cell::new(false));
    let target_pointer = target.with_active_in(|instance| unsafe {
        let fired = fired.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            if fired.replace(true) {
                return;
            }
            super::super::instance::revoke_instance_availability(instance);
        }));
        (*instance).memory_state.gc_force_gap = 1;
        (*instance).memory_state.gc_force_wait = 1;
        instance
    });
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        initialize_r_in(target_pointer);
    }));
    assert!(failure.is_err());
    assert!(fired.get(), "bootstrap must exercise owner revocation");
    assert!(!is_initialized_in(target_pointer));
    assert!(current_instance_ptr().is_none());
}

#[test]
fn test_initialization_can_target_instance_explicitly() {
    let left = crate::sexp::session::RSession::new_for_gc_tests();
    let right = crate::sexp::session::RSession::new_for_gc_tests();
    left.with_active_in(|left| {
        right.with_active_in(|right| unsafe {
            shutdown_r_in(left);
            shutdown_r_in(right);
            assert!(!is_initialized_in(left));
            assert!(!is_initialized_in(right));

            // Target the original managed left allocation while right is
            // active. Bootstrap must restore the independent active owner.
            initialize_r_in(left);
            assert_eq!(current_instance_ptr(), Some(right));
            assert!(is_initialized_in(left));
            assert!(!is_initialized_in(right));
            assert!(!R_GlobalEnv_in(left).is_null());
            assert!(!R_BaseEnv_in(left).is_null());
            assert!(!R_EmptyEnv_in(left).is_null());
            assert!(!R_GlobalEnv_in(right).is_null());

            let plus = Rf_install_in(left, c"+".as_ptr());
            let left_plus = super::super::session::with_instance_active(left, || {
                crate::sexp::envir::R_findVarInFrame((*left).base_env, plus)
            });
            assert!(
                crate::eval::primitive::PrimitiveDescriptor::from_raw(left_plus)
                    .is_some_and(|descriptor| descriptor.name == "+")
            );
            assert!(!is_initialized_in(right));
            assert_eq!(current_instance_ptr(), Some(right));
        });
    });
}
