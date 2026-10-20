//! output tests — extracted verbatim from output.rs.

use super::*;
use crate::sexp::session::RSession;

#[test]
fn owned_output_capture_retains_only_its_original_runtime_through_closure() {
    let mut original = Some(RSession::new_for_gc_tests());
    let owner = original
        .as_ref()
        .unwrap()
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap();
    let count = owner.allocation_strong_count();
    let guard = original.as_ref().unwrap().with_active(|| {
        let guard = OutputCaptureGuard::start();
        capture_stdout("original  ");
        guard
    });
    assert_eq!(owner.allocation_strong_count(), count + 1);
    let replacement = RSession::new_for_gc_tests();
    let replacement_guard = replacement.with_active(|| {
        let guard = OutputCaptureGuard::start();
        capture_stdout("replacement\n\n");
        guard
    });
    drop(original.take());
    assert!(!owner.is_live());
    assert_eq!(owner.allocation_strong_count(), 1);
    assert_eq!(guard.finish().stdout, "original  ");
    assert_eq!(owner.allocation_strong_count(), 0);
    assert!(replacement.with_active(is_capturing));
    assert_eq!(replacement_guard.finish().stdout, "replacement\n\n");
}

#[test]
fn owned_output_capture_preserves_custom_method_stream_order_and_error_prefix() {
    let mut session = RSession::new_without_default_packages();
    let (value, _, _) = session.eval_script_with_output_capture(
            "print.zz <- function(x, ...) { cat('first  '); message('second'); cat('third\\n\\n') }; structure(1, class='zz')",
        );
    let value = value.unwrap().into_owned().unwrap();
    let output = session.with_active(|| capture_printed_value(value).unwrap());
    assert_eq!(output.stdout, "first  third\n\n");
    assert_eq!(output.stderr, "second\n");
    assert_eq!(output.interleaved, "first  second\nthird\n\n");

    let (value, _, _) = session.eval_script_with_output_capture(
        "print.zz <- function(x, ...) { cat('before  '); stop('boom') }; structure(1, class='zz')",
    );
    let value = value.unwrap().into_owned().unwrap();
    let failure = session.with_active(|| capture_printed_value(value).unwrap_err());
    assert!(failure.message.contains("boom"));
    assert_eq!(failure.captured.stdout, "before  ");
    assert_eq!(failure.captured.interleaved, "before  ");
    assert!(!session.with_active(is_capturing));
}

#[test]
fn exact_top_level_emission_preserves_custom_print_without_newline() {
    let mut session = RSession::new_without_default_packages();
    let (value, captured, visible) = session.eval_script_with_output_capture(
        "print.zz <- function(x, ...) cat('custom  '); structure(1, class='zz')",
    );
    assert!(visible);
    assert_eq!(captured.stdout, "");
    let value = value.unwrap().into_owned().unwrap();
    let emitted = session.with_active(|| capture_printed_value(value).unwrap().stdout);
    assert_eq!(emitted, "custom  ");
}

#[test]
fn exact_top_level_emission_preserves_custom_print_blank_lines() {
    let mut session = RSession::new_without_default_packages();
    let (value, captured, visible) = session.eval_script_with_output_capture(
        "print.zz <- function(x, ...) cat('custom\\n\\n'); structure(1, class='zz')",
    );
    assert!(visible);
    assert_eq!(captured.stdout, "");
    let value = value.unwrap().into_owned().unwrap();
    let emitted = session.with_active(|| capture_printed_value(value).unwrap().stdout);
    assert_eq!(emitted, "custom\n\n");
}

#[test]
fn test_capture_lifecycle() {
    let _session = RSession::new();
    assert!(!is_capturing());
    start_capture();
    assert!(is_capturing());
    capture_stdout("hello ");
    capture_stdout("world\n");
    capture_stderr("warning!\n");
    let output = stop_capture();
    assert_eq!(output.stdout, "hello world\n");
    assert_eq!(output.stderr, "warning!\n");
    assert!(!is_capturing());
}

#[test]
fn test_capture_empty() {
    let _session = RSession::new();
    start_capture();
    let output = stop_capture();
    assert_eq!(output.stdout, "");
    assert_eq!(output.stderr, "");
}

#[test]
fn test_nested_capture() {
    let _session = RSession::new();
    start_capture();
    capture_stdout("outer ");
    capture_stderr("outer err ");

    start_capture();
    capture_stdout("inner ");
    capture_stderr("inner err ");
    let inner = stop_capture();
    assert_eq!(inner.stdout, "inner ");
    assert_eq!(inner.stderr, "inner err ");

    assert!(is_capturing());
    capture_stdout("resumed");
    capture_stderr("resumed err");

    let outer = stop_capture();
    assert_eq!(outer.stdout, "outer resumed");
    assert_eq!(outer.stderr, "outer err resumed err");
    assert!(!is_capturing());
}

#[test]
fn selective_capture_crosses_layers_and_charges_receiving_budget() {
    let mut state = OutputCaptureState::default();
    state.set_max_bytes(Some(4));
    state.start();
    state.capture_stdout("ab");
    state.start_with_options(false, true, false);
    state.start_with_options(false, true, false);
    state.capture_stdout("cde");
    state.capture_stderr("x");
    let inner = state.stop();
    assert_eq!(inner.stderr, "x");
    assert!(!inner.truncated);
    assert_eq!(state.stop().stderr, "");
    let outer = state.stop();
    assert_eq!(outer.stdout, "abcd");
    assert!(outer.truncated);
}

#[test]
fn nested_split_captures_tee_once_and_keep_separate_budgets() {
    let mut state = OutputCaptureState::default();
    state.set_max_bytes(Some(4));
    state.start();
    state.capture_stdout("ab");
    state.start_with_options(true, false, true);
    state.start_with_options(true, false, true);
    state.capture_stdout("cde");
    for _ in 0..2 {
        let inner = state.stop();
        assert_eq!(inner.stdout, "cde");
        assert!(!inner.truncated);
    }
    let outer = state.stop();
    assert_eq!(outer.stdout, "abcd");
    assert!(outer.truncated);
}

#[test]
fn test_bounded_capture_also_bounds_chronological_output() {
    let mut state = OutputCaptureState::default();
    state.set_max_bytes(Some(5));
    state.start();
    state.capture_stdout("abc");
    state.capture_stderr("déf");
    for _ in 0..100 {
        state.capture_stdout("é");
    }
    let output = state.stop();
    assert_eq!(output.stdout, "abc");
    assert_eq!(output.stderr, "d");
    assert_eq!(output.interleaved, "abcd");
    assert!(output.truncated);
}

#[test]
fn test_bounded_capture_stays_bounded_under_long_mixed_unicode_output() {
    for limit in 0..=32 {
        let mut state = OutputCaptureState::default();
        state.set_max_bytes(Some(limit));
        state.start();
        for index in 0..4096 {
            let message = ["", "a", "é", "🙂", "mixed Ω output"][index % 5];
            if index % 2 == 0 {
                state.capture_stdout(message);
            } else {
                state.capture_stderr(message);
            }
            let frame = &state.current;
            let stream_bytes =
                frame.stdout.as_ref().unwrap().len() + frame.stderr.as_ref().unwrap().len();
            assert!(stream_bytes <= limit);
            assert_eq!(frame.interleaved.as_ref().unwrap().len(), stream_bytes);
        }
        let output = state.stop();
        assert!(output.truncated);
        assert!(output.interleaved.len() <= limit);
    }
}

#[test]
fn test_bounded_capture_respects_utf8_boundaries() {
    let mut state = OutputCaptureState::default();
    state.set_max_bytes(Some(5));
    state.start();
    state.capture_stdout("ééé");
    let output = state.stop();

    assert_eq!(output.stdout, "éé");
    assert_eq!(output.stdout.len(), 4);
    assert!(output.truncated);
}

#[test]
fn test_bounded_capture_exact_limit_and_empty_message_are_not_truncated() {
    let mut state = OutputCaptureState::default();
    state.set_max_bytes(Some(3));
    state.start();
    state.capture_stdout("abc");
    state.capture_stderr("");
    let output = state.stop();

    assert_eq!(output.stdout, "abc");
    assert!(!output.truncated);
}

#[test]
fn test_nested_bounded_capture_restores_outer_state() {
    let mut state = OutputCaptureState::default();
    state.set_max_bytes(Some(4));
    state.start();
    state.capture_stdout("ab");

    state.start();
    state.capture_stdout("12345");
    let inner = state.stop();
    assert_eq!(inner.stdout, "1234");
    assert!(inner.truncated);

    state.capture_stdout("cd");
    let outer = state.stop();
    assert_eq!(outer.stdout, "abcd");
    assert!(!outer.truncated);
}

#[test]
fn test_bounded_capture_budget_is_shared_by_stdout_and_stderr() {
    let mut state = OutputCaptureState::default();
    state.set_max_bytes(Some(5));
    state.start();
    state.capture_stdout("abc");
    state.capture_stderr("def");
    let output = state.stop();

    assert_eq!(output.stdout, "abc");
    assert_eq!(output.stderr, "de");
    assert!(output.truncated);
    assert_eq!(output.stdout.len() + output.stderr.len(), 5);
}

#[test]
fn test_capture_can_target_instance_explicitly() {
    // A bare allocator is not an initialized executable runtime. Keep
    // both actual session owners alive while targeting their private state.
    let left_session = RSession::new_for_gc_tests();
    let left = left_session
        .with_active(|| crate::sexp::instance::with_required_current_instance(|instance| instance));
    let right_session = RSession::new_for_gc_tests();
    let right = right_session
        .with_active(|| crate::sexp::instance::with_required_current_instance(|instance| instance));

    start_capture_in(left);
    capture_stdout_in(left, "left");
    capture_stderr_in(left, "left err");
    assert!(is_capturing_in(left));
    assert!(!is_capturing_in(right));

    start_capture_in(right);
    capture_stdout_in(right, "right");
    let right_output = stop_capture_in(right);
    assert_eq!(right_output.stdout, "right");
    assert_eq!(right_output.stderr, "");

    let left_output = stop_capture_in(left);
    assert_eq!(left_output.stdout, "left");
    assert_eq!(left_output.stderr, "left err");
}

#[test]
fn test_print_logical_vector() {
    let mut session = RSession::new();
    session
            .with_arena(|arena| {
                let ptr = arena.alloc_vector(SEXPTYPE::LGLSXP, 3);
                let sexp = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(ptr) }.expect("logical vector allocation failed");
                assert!(unsafe { /* SAFETY: fixture has no outstanding payload borrows. */ sexp.clone().set_logical_elt(0, 0) });
                assert!(unsafe { /* SAFETY: fixture has no outstanding payload borrows. */ sexp.clone().set_logical_elt(1, 1) });
                assert!(
                    unsafe { /* SAFETY: fixture has no outstanding payload borrows. */ sexp.clone()
                        .set_logical_elt(2, crate::sexp::ffi::NA_LOGICAL) }
                );

                start_capture();
                print_value(sexp);
                let output = stop_capture();
                assert_eq!(output.stdout, "[1] FALSE  TRUE    NA\n");
            })
            .unwrap();
}

#[test]
fn test_print_named_list_keeps_final_stock_separator_line() {
    let mut session = RSession::new();
    let (result, output, _) = session.eval_code_with_output_capture("print(list(class = 'ts'))");

    assert!(result.is_ok(), "named list should evaluate: {result:?}");
    assert_eq!(output.stdout, "$class\n[1] \"ts\"\n\n");
}

#[test]
fn test_string_output_uses_safe_charsxp_access_and_preserves_na() {
    let mut session = RSession::new();
    session
            .with_arena(|arena| {
                let ptr = arena.alloc_vector(SEXPTYPE::STRSXP, 2);
                let sexp = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(ptr) }.expect("string vector allocation failed");
                let value = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(arena.alloc_charsxp(b"a")) }.expect("CHARSXP");
                let missing = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(unsafe { crate::sexp::globals::R_NaString() }) }
                    .expect("NA_STRING");
                unsafe { /* SAFETY: fixture has no outstanding payload borrows. */ sexp.clone()
                    .try_set_string_elt(0, value) }
                    .expect("set string");
                unsafe { /* SAFETY: fixture has no outstanding payload borrows. */ sexp.clone()
                    .try_set_string_elt(1, missing) }
                    .expect("set string");

                assert_eq!(format_sexp_direct(sexp.clone()), "[1] \"a\" NA ");

                start_capture();
                print_value(sexp);
                let output = stop_capture();
                // Stock R pads the final NA to the field width even in last
                // position (verified against R: `print(c("a", NA))` emits
                // "[1] \"a\" NA \n" with the trailing space).
                assert_eq!(output.stdout, "[1] \"a\" NA \n");
            })
            .unwrap();
}

#[test]
fn test_atomic_element_formatting_reports_access_errors() {
    let mut session = RSession::new();
    session
            .with_arena(|arena| {
                let real = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(arena.alloc_vector(SEXPTYPE::REALSXP, 1)) }
                    .expect("real vector allocation failed");

                assert!(
                    format_integer_element(real.clone(), 0).contains("expected integer vector")
                );
                assert!(format_real_element(real, 2).contains("outside vector length"));
            })
            .unwrap();
}

#[test]
fn test_format_atomic_na_values() {
    assert_eq!(format_integer_value(crate::sexp::ffi::NA_INTEGER), "NA");
    assert_eq!(format_real_value(crate::sexp::ffi::NA_REAL), "NA");
    assert_eq!(format_real_value(f64::NAN), "NaN");
    assert_eq!(format_real_value(f64::INFINITY), "Inf");
    assert_eq!(format_real_value(f64::NEG_INFINITY), "-Inf");
}

#[test]
fn test_format_real_value_prints_signed_zero_as_zero() {
    // GNU EncodeReal0 canonicalizes both zeros. R 4.6.1 `cat(0)` and
    // `cat(-0)` emit `0`. The session is required: the fixed-format path
    // reads options("digits") before it chooses a decimal count.
    let _session = RSession::new();
    assert_eq!(format_real_value(0.0), "0");
    assert_eq!(format_real_value(-0.0), "0");
}

#[test]
fn test_numeric_vector_alignment_matches_r_simple_output() {
    let vals = vec!["2".to_string(), "NA".to_string(), "4".to_string()];
    assert_eq!(format_aligned_values(vals), " 2 NA  4");
}

#[test]
fn test_real_vector_alignment_keeps_decimal_column() {
    let mut session = RSession::new();
    session
            .with_arena(|arena| {
                let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 3);
                let sexp = unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ Sexp::from_raw(ptr) }.expect("real vector allocation failed");
                unsafe { /* SAFETY: fixture has no outstanding payload borrows. */ sexp.clone().try_set_real_elt(0, 200.0) }.expect("set real");
                unsafe { /* SAFETY: fixture has no outstanding payload borrows. */ sexp.clone().try_set_real_elt(1, 80200.0) }.expect("set real");
                unsafe { /* SAFETY: fixture has no outstanding payload borrows. */ sexp.clone().try_set_real_elt(2, 100.5) }.expect("set real");

                assert_eq!(
                    format_sexp_direct(sexp.clone()),
                    "[1]   200.0 80200.0   100.5"
                );

                start_capture();
                print_value(sexp);
                let output = stop_capture();
                assert_eq!(output.stdout, "[1]   200.0 80200.0   100.5\n");
            })
            .unwrap();
}

#[test]
fn printed_utf8_character_matrix_keeps_invalid_field_text_and_original_width() {
    use crate::sexp::{
        encoding::{TextEncoding, allocate_character},
        object::SexpMut,
    };
    let session = crate::sexp::session::RSession::new_for_gc_tests();
    session.with_active(|| {
        let factory = session.owner_token().unwrap().node_factory();
        let invalid = factory
            .allocate(|a| allocate_character(a, &[0xf4, 0x90, 0x80, 0x80], TextEncoding::Utf8))
            .unwrap();
        let mut values = SexpMut::try_from_checked(factory.strings(&["x", ""]).unwrap()).unwrap();
        values.try_set_string_elt(1, invalid).unwrap();
        let values = values.freeze();
        let output = super::format_character_matrix_with(values.clone(), 2, 1, |row, _| {
            super::printed_string_element(values.clone(), row as i64, true)
        });
        assert!(!output.contains("NA"), "{output:?}");
        let (width, body) = if crate::mainutils::platform::locale_is_utf8() {
            (12, "\"\\xf4\\x90\\x80\\x80\"")
        } else {
            (6, "\"\\364\\220\\200\\200\"")
        };
        let lines = output.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 3);
        assert!(
            lines[1].ends_with(&format!("\"x\"{}", " ".repeat(width - 3))),
            "{output:?}"
        );
        assert!(lines[2].ends_with(body), "{output:?}");
    });
}
