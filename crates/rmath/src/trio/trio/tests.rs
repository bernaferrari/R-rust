use super::*;
/// Helper: format using trio and return the result as a Rust `String`.
fn trio_fmt(format: &str, args: &mut FormatArgs) -> String {
    let format_bytes = format.as_bytes();
    let mut buf: Vec<u8> = Vec::new();
    let rc = trio_format(&mut buf, format_bytes, args);
    assert!(rc >= 0, "trio_format returned error {rc}");
    String::from_utf8(buf).expect("trio produced non-UTF8 output")
}
// ---- %d integer formatting ----
// BUG: trio_write_number writes the NUL terminator from its internal
// buffer to the output stream (`out.write(&buffer[number_start..])`)
// because `buffer` is a fixed-size array where position 0 is set to 0
// and the number digits are placed before that sentinel. The slice
// includes the trailing NUL byte, corrupting output. This also causes
// width/padding tests to produce wrong lengths.
//
// BUG: trio_write_number panics with "subtract with overflow" when
// formatting zero or negative numbers because line 665 computes
// `number_len = MAX_CHARS_IN_UINTMAX - number_start - 1` which
// underflows when the buffer layout differs from expectations.
//
// BUG: The `%%` literal percent parser triggers TRIO_EDBLREF (-4)
// because trio_parse sees the two `%` chars as a double-referenced
// positional parameter instead of a literal percent escape.
#[test]
fn fmt_int_positive() {
    let mut args = FormatArgs::new();
    args.push_int(42);
    assert_eq!(trio_fmt("%d\0", &mut args), "42");
}
#[test]
fn fmt_int_negative() {
    let mut args = FormatArgs::new();
    args.push_int(-7);
    assert_eq!(trio_fmt("%d\0", &mut args), "-7");
}
#[test]
fn fmt_int_zero() {
    let mut args = FormatArgs::new();
    args.push_int(0);
    assert_eq!(trio_fmt("%d\0", &mut args), "0");
}
// ---- %f float formatting ----
#[test]
fn fmt_float_default_precision() {
    let mut args = FormatArgs::new();
    args.push_double(3.14);
    let result = trio_fmt("%f\0", &mut args);
    assert!(
        result.starts_with("3.14"),
        "expected '3.14...' got '{result}'"
    );
}
#[test]
fn fmt_float_explicit_precision() {
    let mut args = FormatArgs::new();
    args.push_double(3.14159);
    let result = trio_fmt("%.2f\0", &mut args);
    assert!(
        result.starts_with("3.14"),
        "expected '3.14' prefix, got '{result}'"
    );
}
#[test]
fn fmt_float_zero() {
    let mut args = FormatArgs::new();
    args.push_double(0.0);
    let result = trio_fmt("%f\0", &mut args);
    assert!(
        result.starts_with("0."),
        "expected '0.' prefix, got '{result}'"
    );
}
// ---- %e scientific notation ----
#[test]
fn fmt_scientific_notation() {
    let mut args = FormatArgs::new();
    args.push_double(1234.5);
    let result = trio_fmt("%e\0", &mut args);
    // Should contain 'e' exponent marker
    assert!(
        result.contains('e') || result.contains('E'),
        "expected scientific notation, got '{result}'"
    );
}
// ---- %s string formatting ----
#[test]
fn fmt_string() {
    let mut args = FormatArgs::new();
    let s = std::ffi::CString::new("hello").unwrap();
    args.push_string(s.as_ptr() as *mut c_char);
    let result = trio_fmt("%s\0", &mut args);
    assert_eq!(result, "hello");
}
#[test]
fn fmt_string_with_precision() {
    let mut args = FormatArgs::new();
    let s = std::ffi::CString::new("hello world").unwrap();
    args.push_string(s.as_ptr() as *mut c_char);
    let result = trio_fmt("%.5s\0", &mut args);
    assert_eq!(result, "hello");
}
// ---- %g general float ----
#[test]
fn fmt_general_float_small() {
    let mut args = FormatArgs::new();
    args.push_double(0.0001);
    let result = trio_fmt("%g\0", &mut args);
    // %g should pick scientific for very small numbers
    assert!(
        result.contains('e') || result.contains("0.0001"),
        "expected compact representation, got '{result}'"
    );
}
#[test]
fn fmt_general_float_normal() {
    let mut args = FormatArgs::new();
    args.push_double(123.456);
    let result = trio_fmt("%g\0", &mut args);
    assert!(
        result.starts_with("123."),
        "expected '123.xxx' got '{result}'"
    );
}
// ---- Width and precision specifiers ----
#[test]
fn fmt_width_right_aligned() {
    let mut args = FormatArgs::new();
    args.push_int(42);
    let result = trio_fmt("%10d\0", &mut args);
    assert_eq!(result.len(), 10, "expected width 10, got '{result}'");
    assert!(
        result.ends_with("42"),
        "expected trailing '42', got '{result}'"
    );
    assert!(
        result.starts_with(' '),
        "expected leading spaces, got '{result}'"
    );
}
// ---- Left-justify ----
#[test]
fn fmt_left_justify() {
    let mut args = FormatArgs::new();
    args.push_int(42);
    let result = trio_fmt("%-10d\0", &mut args);
    assert_eq!(result.len(), 10, "expected width 10, got '{result}'");
    assert!(
        result.starts_with("42"),
        "expected leading '42', got '{result}'"
    );
    assert!(
        result.ends_with(' '),
        "expected trailing spaces, got '{result}'"
    );
}
// ---- Zero-padding ----
#[test]
fn fmt_zero_padding() {
    let mut args = FormatArgs::new();
    args.push_int(42);
    let result = trio_fmt("%05d\0", &mut args);
    assert_eq!(result, "00042");
}
// ---- %% literal percent ----
#[test]
fn fmt_literal_percent() {
    let mut args = FormatArgs::new();
    let result = trio_fmt("100%%\0", &mut args);
    assert_eq!(result, "100%");
}
// ---- NaN, Inf, -Inf ----
#[test]
fn fmt_nan() {
    let mut args = FormatArgs::new();
    args.push_double(f64::NAN);
    let result = trio_fmt("%f\0", &mut args);
    assert!(
        result.to_lowercase().contains("nan"),
        "expected NaN representation, got '{result}'"
    );
}
#[test]
fn fmt_positive_infinity() {
    let mut args = FormatArgs::new();
    args.push_double(f64::INFINITY);
    let result = trio_fmt("%f\0", &mut args);
    assert!(
        result.to_lowercase().contains("inf"),
        "expected Inf representation, got '{result}'"
    );
}
#[test]
fn fmt_negative_infinity() {
    let mut args = FormatArgs::new();
    args.push_double(f64::NEG_INFINITY);
    let result = trio_fmt("%f\0", &mut args);
    let lower = result.to_lowercase();
    assert!(
        lower.contains("-inf"),
        "expected -Inf representation, got '{result}'"
    );
}
// ---- Very large / very small numbers ----
#[test]
fn fmt_very_large_number() {
    let mut args = FormatArgs::new();
    args.push_double(1e308);
    let result = trio_fmt("%e\0", &mut args);
    assert!(
        result.contains('e') || result.contains('E'),
        "expected scientific, got '{result}'"
    );
}
#[test]
fn fmt_very_small_number() {
    let mut args = FormatArgs::new();
    args.push_double(1e-300);
    let result = trio_fmt("%e\0", &mut args);
    assert!(
        result.contains('e') || result.contains('E'),
        "expected scientific, got '{result}'"
    );
}
// ---- Multiple arguments ----
#[test]
fn fmt_multiple_args() {
    let mut args = FormatArgs::new();
    args.push_int(10);
    args.push_int(20);
    let result = trio_fmt("%d+%d\0", &mut args);
    assert_eq!(result, "10+20");
}
// ---- Unsigned hex / octal ----
#[test]
fn fmt_hex_lower() {
    let mut args = FormatArgs::new();
    args.push_uint(255);
    let result = trio_fmt("%x\0", &mut args);
    assert_eq!(result, "ff");
}
#[test]
fn fmt_hex_upper() {
    let mut args = FormatArgs::new();
    args.push_uint(255);
    let result = trio_fmt("%X\0", &mut args);
    assert_eq!(result, "FF");
}
#[test]
fn fmt_octal() {
    let mut args = FormatArgs::new();
    args.push_uint(8);
    let result = trio_fmt("%o\0", &mut args);
    assert_eq!(result, "10");
}
// ---- Char ----
#[test]
fn fmt_char() {
    let mut args = FormatArgs::new();
    args.push_uint(b'A' as u64);
    let result = trio_fmt("%c\0", &mut args);
    assert_eq!(result, "A");
}
// ---- trionan helpers ----
#[test]
fn trionan_classify_normal() {
    let mut neg: c_int = 0;
    let class = crate::trio::trionan::trio_fpclassify_and_signbit(1.0, &mut neg);
    assert_eq!(class, crate::trio::trionan::TRIO_FP_NORMAL);
    assert_eq!(neg, 0);
}
#[test]
fn trionan_classify_negative() {
    let mut neg: c_int = 0;
    let class = crate::trio::trionan::trio_fpclassify_and_signbit(-2.5, &mut neg);
    assert_eq!(class, crate::trio::trionan::TRIO_FP_NORMAL);
    assert_eq!(neg, 1);
}
#[test]
fn trionan_classify_nan() {
    let mut neg: c_int = 0;
    let class = crate::trio::trionan::trio_fpclassify_and_signbit(f64::NAN, &mut neg);
    assert_eq!(class, crate::trio::trionan::TRIO_FP_NAN);
}
#[test]
fn trionan_classify_inf() {
    let mut neg: c_int = 0;
    let class = crate::trio::trionan::trio_fpclassify_and_signbit(f64::INFINITY, &mut neg);
    assert_eq!(class, crate::trio::trionan::TRIO_FP_INFINITE);
    assert_eq!(neg, 0);
}
#[test]
fn trionan_classify_neg_inf() {
    let mut neg: c_int = 0;
    let class = crate::trio::trionan::trio_fpclassify_and_signbit(f64::NEG_INFINITY, &mut neg);
    assert_eq!(class, crate::trio::trionan::TRIO_FP_INFINITE);
    assert_eq!(neg, 1);
}
#[test]
fn trionan_classify_zero() {
    let mut neg: c_int = 0;
    let class = crate::trio::trionan::trio_fpclassify_and_signbit(0.0, &mut neg);
    assert_eq!(class, crate::trio::trionan::TRIO_FP_ZERO);
    assert_eq!(neg, 0);
}
#[test]
fn trionan_classify_neg_zero() {
    let mut neg: c_int = 0;
    let class = crate::trio::trionan::trio_fpclassify_and_signbit(-0.0, &mut neg);
    assert_eq!(class, crate::trio::trionan::TRIO_FP_ZERO);
    assert_eq!(neg, 1);
}
#[test]
fn trionan_isinf() {
    assert_eq!(crate::trio::trionan::trio_isinf(f64::INFINITY), 1);
    assert_eq!(crate::trio::trionan::trio_isinf(f64::NEG_INFINITY), -1);
    assert_eq!(crate::trio::trionan::trio_isinf(1.0), 0);
}
#[test]
fn trionan_isnan() {
    assert_eq!(crate::trio::trionan::trio_isnan(f64::NAN), 1);
    assert_eq!(crate::trio::trionan::trio_isnan(1.0), 0);
}
#[test]
fn trionan_isfinite() {
    assert_ne!(crate::trio::trionan::trio_isfinite(1.0), 0);
    assert_eq!(crate::trio::trionan::trio_isfinite(f64::INFINITY), 0);
    assert_eq!(crate::trio::trionan::trio_isfinite(f64::NAN), 0);
}
#[test]
fn trionan_generators() {
    assert!(crate::trio::trionan::trio_nan().is_nan());
    assert_eq!(crate::trio::trionan::trio_pinf(), f64::INFINITY);
    assert_eq!(crate::trio::trionan::trio_ninf(), f64::NEG_INFINITY);
    assert!(crate::trio::trionan::trio_nzero().is_sign_negative());
    assert_eq!(crate::trio::trionan::trio_nzero(), 0.0);
}
// ---- Wildcard matching ----
#[test]
fn wildcard_match_star() {
    assert!(trio_match_impl("hello world", "hello*"));
    assert!(trio_match_impl("hello world", "*world"));
    assert!(trio_match_impl("hello world", "*lo*wo*"));
}
#[test]
fn wildcard_match_question() {
    assert!(trio_match_impl("hello", "hell?"));
    assert!(!trio_match_impl("hello", "hel?"));
}
#[test]
fn wildcard_match_exact() {
    assert!(trio_match_impl("hello", "hello"));
    assert!(!trio_match_impl("hello", "world"));
}
#[test]
fn wildcard_match_case_insensitive() {
    assert!(trio_match_impl("Hello", "hello"));
    assert!(trio_match_impl("HELLO", "h?llo"));
}
// ==================================================================
// Extended coverage: edge cases and regression tests
// ==================================================================
#[test]
fn fmt_three_int_args() {
    let mut args = FormatArgs::new();
    args.push_int(1);
    args.push_int(2);
    args.push_int(3);
    assert_eq!(trio_fmt("%d+%d=%d\0", &mut args), "1+2=3");
}
#[test]
fn fmt_mixed_string_int_double() {
    let mut args = FormatArgs::new();
    let s = std::ffi::CString::new("hello").unwrap();
    args.push_string(s.as_ptr() as *mut c_char);
    args.push_int(42);
    args.push_double(3.14);
    assert_eq!(trio_fmt("%s %d %.2f\0", &mut args), "hello 42 3.14");
}
#[test]
fn fmt_percent_literal_regression() {
    // Regression: %% was broken because FORMAT_SENTINEL broke early
    let mut args = FormatArgs::new();
    assert_eq!(trio_fmt("100%%\0", &mut args), "100%");
}
#[test]
fn fmt_percent_with_int_arg() {
    let mut args = FormatArgs::new();
    args.push_int(50);
    assert_eq!(trio_fmt("%d%%\0", &mut args), "50%");
}
#[test]
fn fmt_percent_between_args() {
    let mut args = FormatArgs::new();
    args.push_int(1);
    args.push_int(2);
    assert_eq!(trio_fmt("%d %% %d\0", &mut args), "1 % 2");
}
#[test]
fn fmt_g_small_value_regression() {
    // Regression: %g digit extraction was wrong without start_power division
    let mut args = FormatArgs::new();
    args.push_double(0.001);
    let result = trio_fmt("%g\0", &mut args);
    assert_eq!(result, "0.001");
}
#[test]
fn fmt_g_medium_value() {
    let mut args = FormatArgs::new();
    args.push_double(123456.0);
    let result = trio_fmt("%g\0", &mut args);
    assert_eq!(result, "123456");
}
#[test]
fn fmt_g_scientific_threshold() {
    // %g uses scientific notation for very large values
    let mut args = FormatArgs::new();
    args.push_double(1e10);
    let result = trio_fmt("%g\0", &mut args);
    assert!(
        result.contains('e') || result.contains('E'),
        "expected scientific notation for 1e10, got '{}'",
        result
    );
}
#[test]
fn fmt_hex_upper_255() {
    let mut args = FormatArgs::new();
    args.push_int(255);
    assert_eq!(trio_fmt("%X\0", &mut args), "FF");
}
#[test]
fn fmt_hex_lower_255() {
    let mut args = FormatArgs::new();
    args.push_int(255);
    assert_eq!(trio_fmt("%x\0", &mut args), "ff");
}
#[test]
fn fmt_octal_eight() {
    let mut args = FormatArgs::new();
    args.push_int(8);
    assert_eq!(trio_fmt("%o\0", &mut args), "10");
}
#[test]
fn fmt_unsigned_int() {
    let mut args = FormatArgs::new();
    args.push_int(42);
    assert_eq!(trio_fmt("%u\0", &mut args), "42");
}
#[test]
fn fmt_string_zero_precision() {
    // %.0s should output nothing
    let mut args = FormatArgs::new();
    let s = std::ffi::CString::new("hello").unwrap();
    args.push_string(s.as_ptr() as *mut c_char);
    assert_eq!(trio_fmt("%.0s\0", &mut args), "");
}
#[test]
fn fmt_zero_padded_width() {
    let mut args = FormatArgs::new();
    args.push_int(42);
    assert_eq!(trio_fmt("%05d\0", &mut args), "00042");
}
#[test]
fn fmt_zero_padded_negative() {
    let mut args = FormatArgs::new();
    args.push_int(-42);
    assert_eq!(trio_fmt("%06d\0", &mut args), "-00042");
}
#[test]
fn fmt_plus_sign_flag() {
    let mut args = FormatArgs::new();
    args.push_int(42);
    assert_eq!(trio_fmt("%+d\0", &mut args), "+42");
}
#[test]
fn fmt_space_sign_flag() {
    let mut args = FormatArgs::new();
    args.push_int(42);
    assert_eq!(trio_fmt("% d\0", &mut args), " 42");
}
#[test]
fn fmt_hash_hex_prefix() {
    let mut args = FormatArgs::new();
    args.push_int(255);
    assert_eq!(trio_fmt("%#x\0", &mut args), "0xff");
}
#[test]
fn fmt_repeated_calls_no_corruption() {
    // Regression: run many format calls to detect memory corruption
    for i in 0i64..20 {
        let mut args = FormatArgs::new();
        args.push_int(i);
        let result = trio_fmt("%d\0", &mut args);
        assert_eq!(result, i.to_string());
    }
}
