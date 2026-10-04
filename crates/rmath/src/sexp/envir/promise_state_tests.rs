//! Public GNU promise-state contracts with a deliberately bounded evaluator.
#![forbid(unsafe_code)]

use crate::sexp::RSession;

const RECURSIVE: &str =
    "promise already under evaluation: recursive default argument reference or earlier problems?";

fn session() -> RSession {
    let mut session = RSession::new_without_default_packages();
    session.set_eval_limits(crate::eval::eval::EvalLimits {
        max_eval_depth: 64,
        ..crate::eval::eval::EvalLimits::default()
    });
    session
}

#[test]
fn promise_state_full_independent_gnu_contract() {
    let mut session = session();
    let (result, _, _) = session.eval_script_with_output_capture(include_str!(
        "../../../../r-embed/tests/fixtures/gnu-promise-evaluation-state/contract.R"
    ));
    result.expect("the complete independently executed GNU contract must succeed");
}

#[test]
fn promise_recursive_default_reports_specific_gnu_error_before_depth_limit() {
    let mut session = session();
    let (result, _, _) = session.eval_script_with_output_capture(
        "f <- function(x=x) x; tryCatch(f(), error=function(e) conditionMessage(e))",
    );
    assert_eq!(
        result.unwrap().try_string_value_elt(0).unwrap().as_deref(),
        Some(RECURSIVE)
    );
    let (result, _, _) = session.eval_script_with_output_capture("37L");
    assert_eq!(result.unwrap().try_integer_elt(0).unwrap(), 37);
}

#[test]
fn promise_recursive_delayed_binding_reports_specific_gnu_error() {
    let mut session = session();
    let (result, _, _) = session.eval_script_with_output_capture(
        "delayedAssign('.self', .self); tryCatch(.self, error=function(e) conditionMessage(e))",
    );
    assert_eq!(
        result.unwrap().try_string_value_elt(0).unwrap().as_deref(),
        Some(RECURSIVE)
    );
}

#[test]
fn promise_interrupted_retry_warns_once_then_caches_success() {
    let mut session = session();
    let (result, _, _) = session.eval_script_with_output_capture(
        r"
        count <- 0L
        delayedAssign('.retry', {
            count <<- count + 1L
            if (count == 1L) stop('first')
            37L
        })
        first <- tryCatch(.retry, error=function(e) conditionMessage(e))
        warnings <- character()
        second <- withCallingHandlers(.retry, warning=function(w) {
            warnings <<- c(warnings, conditionMessage(w))
            invokeRestart('muffleWarning')
        })
        third <- withCallingHandlers(.retry, warning=function(w) {
            warnings <<- c(warnings, conditionMessage(w))
            invokeRestart('muffleWarning')
        })
        identical(first, 'first') && identical(second, 37L) &&
            identical(third, 37L) && identical(count, 2L) &&
            identical(warnings, 'restarting interrupted promise evaluation')
        ",
    );
    assert_eq!(result.unwrap().try_logical_elt(0).unwrap(), 1);
}

#[test]
fn promise_restart_warning_handler_reentry_sees_evaluating_state() {
    let mut session = session();
    let (result, _, _) = session.eval_script_with_output_capture(
        r"
        count <- 0L
        delayedAssign('.retry', {
            count <<- count + 1L
            if (count == 1L) stop('first')
            37L
        })
        first <- tryCatch(.retry, error=function(e) conditionMessage(e))
        reentry <- NULL
        messages <- character()
        second <- withCallingHandlers(.retry, warning=function(w) {
            messages <<- c(messages, conditionMessage(w))
            reentry <<- tryCatch(.retry, error=function(e) conditionMessage(e))
            invokeRestart('muffleWarning')
        })
        identical(first, 'first') && identical(second, 37L) &&
            identical(count, 2L) &&
            identical(messages, 'restarting interrupted promise evaluation') &&
            identical(reentry,
                'promise already under evaluation: recursive default argument reference or earlier problems?')
        ",
    );
    assert_eq!(result.unwrap().try_logical_elt(0).unwrap(), 1);
}

#[test]
fn promise_restart_warning_converted_to_error_preserves_gnu_blocked_state() {
    let mut session = session();
    let (result, _, _) = session.eval_script_with_output_capture(
        r"
        count <- 0L
        delayedAssign('.retry', {
            count <<- count + 1L
            if (count == 1L) stop('first')
            37L
        })
        first <- tryCatch(.retry, error=function(e) conditionMessage(e))
        options(warn=2L)
        second <- tryCatch(.retry, error=function(e) conditionMessage(e))
        options(warn=0L)
        third <- tryCatch(.retry, error=function(e) conditionMessage(e))
        identical(first, 'first') && identical(count, 1L) &&
            identical(second,
                '(converted from warning) restarting interrupted promise evaluation') &&
            identical(third,
                'promise already under evaluation: recursive default argument reference or earlier problems?')
        ",
    );
    assert_eq!(result.unwrap().try_logical_elt(0).unwrap(), 1);
}
