#![forbid(unsafe_code)]
//! Bounded GNU R number grammar and arithmetic, independent of runtime state.
//!
//! Operation order follows pinned util.c/R_strtod5. This is deliberately not
//! C99 strtod: GNU accepts repeated hexadecimal points and has distinct
//! intermediate-underflow behavior. No full cross-platform precision claim is
//! implied by matching this target's independently captured numerical controls.
use crate::sexp::ffi::{NA_INTEGER, NA_REAL};

#[derive(Clone, Copy, Debug)]
pub(crate) struct ParsedNumber {
    pub value: f64,
    /// Offset into the supplied bytes, including leading C whitespace.
    pub consumed: usize,
    /// GNU numerals="warn.loss" requests one accuracy warning before return.
    pub accuracy_loss: bool,
}
impl ParsedNumber {
    fn rejected(accuracy_loss: bool) -> Self {
        Self {
            value: NA_REAL,
            consumed: 0,
            accuracy_loss,
        }
    }
}
fn whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 11 | 12)
}
fn starts_case_insensitive(input: &[u8], offset: usize, word: &[u8]) -> bool {
    input[offset..]
        .get(..word.len())
        .is_some_and(|bytes| bytes.eq_ignore_ascii_case(word))
}
fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
/// Saturate only bookkeeping beyond finite-input numerical relevance. The
/// original exponent prefix rule bounds even adversarial exponent spellings.
fn exponent(input: &[u8], cursor: &mut usize) -> Option<i64> {
    let negative = input.get(*cursor) == Some(&b'-');
    if matches!(input.get(*cursor), Some(b'-' | b'+')) {
        *cursor += 1;
    }
    let first_digit = *cursor;
    let mut value = 0_i64;
    while let Some(byte @ b'0'..=b'9') = input.get(*cursor) {
        if value < 9999 {
            value = value * 10 + i64::from(*byte - b'0');
        }
        *cursor += 1;
    }
    (*cursor != first_digit).then_some(if negative { -value } else { value })
}
fn loss(mantissa: f64, exact: i32) -> Result<bool, ParsedNumber> {
    if exact != 0 && mantissa > 9_007_199_254_740_991.0 {
        if exact == NA_INTEGER {
            Ok(true)
        } else {
            Err(ParsedNumber::rejected(false))
        }
    } else {
        Ok(false)
    }
}
fn power_factor(mut exponent: u64, base: f64, reciprocal: bool) -> f64 {
    let mut factor = 1.0;
    let mut power = base;
    while exponent != 0 {
        if exponent & 1 != 0 {
            if reciprocal {
                factor /= power;
            } else {
                factor *= power;
            }
        }
        exponent >>= 1;
        power *= power;
    }
    factor
}

/// Parse one GNU numeric prefix. Embedded NUL ends the bounded token exactly as
/// the native adapter's C string does. A failed token consumes zero bytes.
/// `exact=0` permits loss; `NA_INTEGER` warns; other nonzero values reject it.
/// No warning, allocation of R objects, native FFI or callback occurs here.
pub(crate) fn parse_number(input: &[u8], decimal: u8, allow_na: bool, exact: i32) -> ParsedNumber {
    let input = input.split(|byte| *byte == 0).next().unwrap_or_default();
    let mut cursor = 0;
    while input.get(cursor).is_some_and(|byte| whitespace(*byte)) {
        cursor += 1;
    }
    if allow_na && input[cursor..].starts_with(b"NA") {
        return ParsedNumber {
            value: NA_REAL,
            consumed: cursor + 2,
            accuracy_loss: false,
        };
    }
    let negative = input.get(cursor) == Some(&b'-');
    if matches!(input.get(cursor), Some(b'-' | b'+')) {
        cursor += 1;
    }
    let sign = if negative { -1.0 } else { 1.0 };
    for (word, value) in [
        (b"NaN".as_slice(), f64::NAN),
        (b"infinity".as_slice(), f64::INFINITY),
        (b"Inf".as_slice(), f64::INFINITY),
    ] {
        if starts_case_insensitive(input, cursor, word) {
            return ParsedNumber {
                value: sign * value,
                consumed: cursor + word.len(),
                accuracy_loss: false,
            };
        }
    }
    let mut mantissa = 0.0;
    if input.len() - cursor > 2
        && input.get(cursor) == Some(&b'0')
        && matches!(input.get(cursor + 1), Some(b'x' | b'X'))
    {
        cursor += 2;
        let mut fractional_bits = None::<i64>;
        while let Some(byte) = input.get(cursor) {
            if let Some(digit) = hex_digit(*byte) {
                mantissa = 16.0 * mantissa + f64::from(digit);
                if let Some(bits) = &mut fractional_bits {
                    *bits = bits.saturating_add(4);
                }
            } else if *byte == decimal {
                // This reset, and acceptance without digits, are actual GNU
                // grammar: 0x. -> 0 and 0x1.2.3 -> 18.1875.
                fractional_bits = Some(0);
            } else {
                break;
            }
            cursor += 1;
        }
        let accuracy_loss = match loss(mantissa, exact) {
            Ok(loss) => loss,
            Err(rejected) => return rejected,
        };
        let mut power = 0_i64;
        if matches!(input.get(cursor), Some(b'p' | b'P')) {
            cursor += 1;
            let Some(parsed) = exponent(input, &mut cursor) else {
                return ParsedNumber::rejected(accuracy_loss);
            };
            power = parsed;
        }
        if mantissa != 0.0 {
            if let Some(bits) = fractional_bits.filter(|bits| *bits > 0) {
                if power.saturating_sub(bits) < -122 {
                    mantissa /= power_factor(bits as u64, 2.0, false);
                } else {
                    power -= bits;
                }
            }
            if power < 0 {
                mantissa /= power_factor(power.unsigned_abs(), 2.0, false);
            } else {
                mantissa *= power_factor(power as u64, 2.0, false);
            }
        }
        return ParsedNumber {
            value: sign * mantissa,
            consumed: cursor,
            accuracy_loss,
        };
    }
    let mut digits = 0_i64;
    let mut power = 0_i64;
    while let Some(byte @ b'0'..=b'9') = input.get(cursor) {
        mantissa = 10.0 * mantissa + f64::from(*byte - b'0');
        digits = digits.saturating_add(1);
        cursor += 1;
    }
    if input.get(cursor) == Some(&decimal) {
        cursor += 1;
        while let Some(byte @ b'0'..=b'9') = input.get(cursor) {
            mantissa = 10.0 * mantissa + f64::from(*byte - b'0');
            digits = digits.saturating_add(1);
            power = power.saturating_sub(1);
            cursor += 1;
        }
    }
    if digits == 0 {
        return ParsedNumber::rejected(false);
    }
    let accuracy_loss = match loss(mantissa, exact) {
        Ok(loss) => loss,
        Err(rejected) => return rejected,
    };
    if matches!(input.get(cursor), Some(b'e' | b'E')) {
        cursor += 1;
        let Some(parsed) = exponent(input, &mut cursor) else {
            return ParsedNumber::rejected(accuracy_loss);
        };
        power = power.saturating_add(parsed);
    }
    // Preserve GNU's compensation before subnormal scaling, including its
    // sequence of divisions, rather than replacing it with powi/libm.
    if power.saturating_add(digits) < -300 {
        for _ in 0..digits {
            mantissa /= 10.0;
        }
        power = power.saturating_add(digits);
    }
    if power < -307 {
        mantissa *= power_factor(power.unsigned_abs(), 10.0, true);
    } else if power < 0 {
        mantissa /= power_factor(power.unsigned_abs(), 10.0, false);
    } else if mantissa != 0.0 {
        mantissa *= power_factor(power as u64, 10.0, false);
    }
    ParsedNumber {
        value: sign * mantissa,
        consumed: cursor,
        accuracy_loss,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bytes(hex: &str) -> Vec<u8> {
        hex.as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }
    #[test]
    fn bounded_gnu_parser_matches_independent_double_coercion_table() {
        for row in include_str!("coerce/fixtures/number-coercion.tsv").lines() {
            let columns = row.split('|').collect::<Vec<_>>();
            if columns[1] != "double" {
                continue;
            }
            let input = bytes(columns[0]);
            let parsed = parse_number(&input, b'.', false, 0);
            assert!(parsed.consumed <= input.len());
            let actual = if input[parsed.consumed..]
                .iter()
                .all(|byte| whitespace(*byte))
            {
                parsed.value
            } else {
                NA_REAL
            };
            let expected =
                f64::from_bits(u64::from_le_bytes(bytes(columns[2]).try_into().unwrap()));
            if expected.is_nan() {
                assert!(actual.is_nan(), "{:?}", String::from_utf8_lossy(&input));
                assert_eq!(
                    crate::sexp::ffi::R_IsNA(actual),
                    crate::sexp::ffi::R_IsNA(expected)
                );
            } else {
                assert_eq!(
                    actual.to_bits(),
                    expected.to_bits(),
                    "{:?}",
                    String::from_utf8_lossy(&input)
                );
            }
        }
    }
    #[test]
    fn bounded_gnu_parser_preserves_original_near_two_underflow_and_exact_modes() {
        for (input, bits, consumed) in [
            (b"1.9999999999999998".as_slice(), 2.0_f64.to_bits(), 18),
            (b"1e-308", 0x0007_30d6_7819_e8d1, 6),
            (b"0x1p-1074", 0, 9),
            (b"0x1.2.3", 18.1875_f64.to_bits(), 7),
            (b"0x.", 0, 3),
        ] {
            let result = parse_number(input, b'.', false, 0);
            assert_eq!(result.value.to_bits(), bits);
            assert_eq!(result.consumed, consumed);
        }
        for input in [
            b"9007199254740993".as_slice(),
            b"0x20000000000001",
            b"1.9999999999999998",
        ] {
            let result = parse_number(input, b'.', false, 1);
            assert!(crate::sexp::ffi::R_IsNA(result.value));
            assert_eq!(result.consumed, 0);
            assert!(!result.accuracy_loss);
            let result = parse_number(input, b'.', false, NA_INTEGER);
            assert_eq!(result.consumed, input.len());
            assert!(result.accuracy_loss);
        }
        let malformed = parse_number(b"9007199254740993e+", b'.', false, NA_INTEGER);
        assert_eq!(malformed.consumed, 0);
        assert!(malformed.accuracy_loss);
    }
    #[test]
    fn bounded_gnu_parser_limits_offsets_for_all_byte_prefixes_and_decimal_delimiters() {
        let bytes = (0..=255).collect::<Vec<u8>>();
        for offset in 0..bytes.len() {
            for length in 0..=bytes.len() - offset {
                let input = &bytes[offset..offset + length];
                let result = parse_number(input, b'.', false, 0);
                assert!(result.consumed <= input.len());
            }
        }
        for delimiter in 0..=255 {
            assert!(parse_number(b"  +1,25e2trailer", delimiter, false, 0).consumed <= 16);
        }
        let comma = parse_number(b" \t-1,25e2x", b',', false, 0);
        assert_eq!(comma.value, -125.0);
        assert_eq!(comma.consumed, 9);
        let na = parse_number(b" \tNArest", b'.', true, 0);
        assert!(crate::sexp::ffi::R_IsNA(na.value));
        assert_eq!(na.consumed, 4);
        assert_eq!(parse_number(b"+NA", b'.', true, 0).consumed, 0);
        assert_eq!(parse_number(b"1\0suffix", b'.', false, 0).consumed, 1);
    }
}
