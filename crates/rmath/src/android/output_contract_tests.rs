//! Exact console emission with a base-only fixture, independent of package startup.

use super::*;

fn session() -> RSession {
    RSession {
        core: CoreRSession::new_without_default_packages(),
        result_limit: None,
    }
}

#[test]
fn exact_console_capture_preserves_invisible_explicit_bytes() {
    let mut session = session();
    for (code, expected) in [
        ("cat('recovered')", "recovered"),
        ("cat(' a  \\n\\n')", " a  \n\n"),
        ("cat('\\r')", "\r"),
        ("cat(''); invisible(7L)", ""),
    ] {
        let result = session.eval(code);
        assert!(
            !matches!(result.typed, RValue::Error(_)),
            "{code}: {result:?}"
        );
        assert_eq!(result.stdout, expected, "{code}");
        assert_eq!(result.output, expected, "{code}");
    }
}

#[test]
fn exact_console_capture_prints_visible_values_without_inventing_separators() {
    let mut session = session();
    for (code, expected) in [
        ("x <- 41; x + 1", "[1] 42\n"),
        ("cat('prefix'); 7L", "prefix[1] 7\n"),
        (
            "f <- function() { on.exit(cat('exit')); return(7L) }; f()",
            "exit[1] 7\n",
        ),
        ("1L; 2L", "[1] 1\n[1] 2\n"),
    ] {
        let result = session.eval(code);
        assert!(
            !matches!(result.typed, RValue::Error(_)),
            "{code}: {result:?}"
        );
        assert_eq!(result.stdout, expected, "{code}");
        assert_eq!(result.output, expected, "{code}");
    }
}

#[test]
fn exact_console_capture_preserves_pre_error_output_and_recovers() {
    let mut session = session();
    for (code, expected) in [
        ("cat('before  '); stop('boom')", "before  "),
        ("cat('before\\n\\n'); stop('boom')", "before\n\n"),
    ] {
        let result = session.eval(code);
        assert!(matches!(result.typed, RValue::Error(_)));
        assert_eq!(result.stdout, expected, "{code}");
        assert!(result.stderr.contains("boom"));
        assert!(result.output.starts_with(expected));
        assert_eq!(session.eval("cat('recovered')").output, "recovered");
    }
}

#[test]
fn exact_console_capture_preserves_final_and_intermediate_custom_print() {
    let mut session = session();
    for (code, expected) in [
        (
            "print.zz <- function(x, ...) cat('custom  '); structure(1, class='zz')",
            "custom  ",
        ),
        (
            "print.zz <- function(x, ...) cat('custom\\n\\n'); structure(1, class='zz')",
            "custom\n\n",
        ),
        (
            "print.zz <- function(x, ...) cat('custom  '); structure(1, class='zz'); 2L",
            "custom  [1] 2\n",
        ),
    ] {
        let result = session.eval(code);
        assert!(
            !matches!(result.typed, RValue::Error(_)),
            "{code}: {result:?}"
        );
        assert_eq!(result.stdout, expected, "{code}");
        assert_eq!(result.output, expected, "{code}");
    }
}

#[test]
fn exact_console_capture_matches_independent_gnu_oracle() {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../r-embed/tests/fixtures/console-output-contract/expected.json"
    ))
    .unwrap();
    for case in cases.as_array().unwrap() {
        // Each GNU fixture ran in a fresh process; avoid carrying custom print
        // methods or bindings from another case into the Rust session.
        let mut session = session();
        let code = case["code"].as_str().unwrap();
        let result = session.eval(code);
        assert_eq!(
            matches!(result.typed, RValue::Error(_)),
            case["error"].as_bool().unwrap(),
            "{}: {result:?}",
            case["name"]
        );
        assert_eq!(result.stdout, case["stdout"].as_str().unwrap(), "{case}");
        if !case["error"].as_bool().unwrap() {
            assert_eq!(result.output, result.stdout, "{case}");
        }
    }
}

#[test]
fn exact_console_capture_preserves_custom_print_stream_order_and_error_prefix() {
    let mut session = session();
    let result = session.eval(
        "print.zz <- function(x, ...) {cat('first  '); message('second'); cat('third\\n\\n')}; structure(1, class='zz')",
    );
    assert!(!matches!(result.typed, RValue::Error(_)), "{result:?}");
    assert_eq!(result.stdout, "first  third\n\n");
    assert_eq!(result.stderr, "second\n");
    assert_eq!(result.output, "first  second\nthird\n\n");

    let result = session.eval("cat('before  '); message('notice'); cat('after'); stop('boom')");
    assert!(matches!(&result.typed, RValue::Error(message) if message == "boom"));
    assert_eq!(result.stdout, "before  after");
    assert!(result.stderr.starts_with("notice\nError"), "{result:?}");
    assert!(
        result.output.starts_with("before  notice\nafterError"),
        "{result:?}"
    );
    // Call attribution can contain the literal "boom" as well as the error
    // message. Count rendered errors rather than occurrences in that call.
    assert_eq!(
        result
            .stderr
            .lines()
            .filter(|line| line.starts_with("Error"))
            .count(),
        1
    );
    assert_eq!(session.eval("cat('recovered')").stdout, "recovered");
}
