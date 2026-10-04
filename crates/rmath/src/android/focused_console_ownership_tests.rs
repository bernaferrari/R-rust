//! Focused production console ownership paths without full base bootstrap.
//! The existing full-base console cases and GNU oracle fixtures remain separate.

use super::{CoreRSession, RSession, RValue};
use std::{cell::Cell, rc::Rc};

fn session() -> RSession {
    // Ordinary lookup still executes the production registry/cached wrappers.
    // These tests install only their parsed user method and active binding.
    RSession {
        core: CoreRSession::new_for_gc_tests(),
        result_limit: None,
    }
}

#[test]
fn focused_console_closed_error_preserves_live_owner_warning() {
    let mut closed = session();
    closed.close();
    let mut other = session();
    let (warning, _, _) = other
        .core
        .eval_code_with_output_capture("warning('other warning')");
    assert!(warning.is_ok(), "{warning:?}");
    drop(warning);
    other.core.with_active_in(|instance| {
        assert_eq!(unsafe { (*instance).error_state.collect_warnings }, 1);
        let result = closed.eval_script("7L");
        assert!(matches!(result.typed, RValue::Error(_)), "{result:?}");
        assert_eq!(result.stdout, "");
        assert!(result.stderr.contains("session is closed"));
        assert_eq!(unsafe { (*instance).error_state.collect_warnings }, 1);
        assert!(
            unsafe { crate::mainutils::errors::take_warnings_block() }
                .unwrap()
                .contains("other warning")
        );
    });
}

#[test]
fn focused_console_active_binding_stop_is_typed_and_recovers() {
    let mut session = session();
    let (setup, _, _) = session.core.eval_code_with_output_capture(
        "makeActiveBinding('.Last.value', function(value) stop('last value failed'), globalenv())",
    );
    assert!(setup.is_ok(), "{setup:?}");
    drop(setup);
    let pin = session
        .core
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap()
        .pin()
        .unwrap();
    let result = session.eval("7L; 8L");
    assert!(
        matches!(&result.typed, RValue::Error(message) if message.contains("last value failed")),
        "{result:?}"
    );
    assert_eq!(unsafe { (*pin.as_ptr()).error_state.toplevel_expr_no }, 0);
    assert!(!unsafe { (*pin.as_ptr()).output_capture.borrow().is_capturing() });
    let (removed, _, _) = session
        .core
        .eval_code_with_output_capture("rm('.Last.value')");
    assert!(removed.is_ok(), "{removed:?}");
    drop(removed);
    assert_eq!(session.eval("7L").stdout, "[1] 7\n");
}

#[test]
fn focused_console_revoked_print_keeps_prefix_and_original_cleanup() {
    for code in ["x", "x; 7L"] {
        let mut session = session();
        let setup = session.eval(
            "print.zz <- function(x, ...) { cat('partial  '); gc(); invisible(x) }; x <- structure(1L, class='zz')",
        );
        assert!(!matches!(setup.typed, RValue::Error(_)), "{setup:?}");
        let owner = session.core.owner_token().unwrap().weak_owner().unwrap();
        let pin = owner.pin().unwrap();
        let notifications = Rc::new(Cell::new(0_u32));
        let observed = notifications.clone();
        session.core.with_active_in(|instance| {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                observed.set(observed.get() + 1);
                if observed.get() == 1 {
                    unsafe {
                        crate::sexp::instance::revoke_instance_availability(instance);
                    }
                }
            }));
        });
        let result = session.eval(code);
        assert_eq!(notifications.get(), 1, "{code}");
        assert!(matches!(result.typed, RValue::Error(_)), "{result:?}");
        assert_eq!(result.stdout, "partial  ", "{code}");
        assert!(result.output.starts_with("partial  "));
        assert!(owner.pin().is_err());
        assert!(!session.is_active());
        let subsequent = session.eval("cat('must not run')");
        assert!(
            matches!(subsequent.typed, RValue::Error(_)),
            "{subsequent:?}"
        );
        assert_eq!(subsequent.stdout, "");
        assert!(!subsequent.output.contains("must not run"));
        assert_eq!(unsafe { (*pin.as_ptr()).error_state.toplevel_expr_no }, 0);
        assert!(!unsafe { (*pin.as_ptr()).output_capture.borrow().is_capturing() });
    }
}

#[test]
fn focused_console_live_print_panic_preserves_payload_and_cleanup() {
    let mut session = session();
    let setup = session.eval(
        "print.zz <- function(x, ...) { cat('before panic'); gc(); invisible(x) }; x <- structure(1L, class='zz')",
    );
    assert!(!matches!(setup.typed, RValue::Error(_)), "{setup:?}");
    let pin = session
        .core
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap()
        .pin()
        .unwrap();
    let notifications = Rc::new(Cell::new(0_u32));
    let observed = notifications.clone();
    session.core.with_active(|| {
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            observed.set(observed.get() + 1);
            std::panic::panic_any(991_u32);
        }));
    });
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| session.eval("x; 7L")));
    let payload = outcome.expect_err("live-owner Rust panic must preserve its payload");
    assert_eq!(payload.downcast_ref::<u32>(), Some(&991));
    assert_eq!(notifications.get(), 1);
    assert_eq!(unsafe { (*pin.as_ptr()).error_state.toplevel_expr_no }, 0);
    assert!(!unsafe { (*pin.as_ptr()).output_capture.borrow().is_capturing() });
    session
        .core
        .with_active_in(|instance| unsafe { (*instance).gc_state.callbacks.clear() });
    assert_eq!(session.eval("7L").stdout, "[1] 7\n");
}
