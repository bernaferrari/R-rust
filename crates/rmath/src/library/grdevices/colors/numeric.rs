#![forbid(unsafe_code)]
//! Numeric color specifications use C `strtod` grammar, not R's deliberately
//! different `R_strtod` grammar. Keep this bounded conversion identical on native
//! platforms and Wasm, including rounding before truncating a palette index.

/// Parse a complete, nonnegative numeric color token beginning with a digit.
/// GNU's conversion outside the C `int` range is undefined; reject it before
/// conversion rather than depending on a native or Wasm saturating cast.
pub(super) fn palette_index(input: &[u8]) -> Option<usize> {
    if !input.first()?.is_ascii_digit() {
        return None;
    }
    let value = if input.starts_with(b"0x") || input.starts_with(b"0X") {
        hexadecimal(&input[2..])?
    } else {
        std::str::from_utf8(input).ok()?.parse::<f64>().ok()?
    };
    if !value.is_finite() || !(0.0..2_147_483_648.0).contains(&value) {
        return None;
    }
    Some(value as usize)
}

fn digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Accumulate only the leading 54 bits and a sticky bit. These are sufficient
/// for one ties-to-even rounding to any normal or subnormal f64, even when a
/// long significand and exponent cancel. No intermediate float can overflow.
fn hexadecimal(input: &[u8]) -> Option<f64> {
    let mut cursor = 0;
    let mut point = false;
    let mut digits = false;
    let mut fractional_bits = 0_i64;
    let mut significant_bits = 0_i64;
    let mut leading = 0_u64;
    let mut stored_bits = 0_u32;
    let mut sticky = false;
    while let Some(&byte) = input.get(cursor) {
        if let Some(nibble) = digit(byte) {
            digits = true;
            if point {
                fractional_bits = fractional_bits.saturating_add(4);
            }
            for shift in (0..4).rev() {
                let bit = (nibble >> shift) & 1;
                if significant_bits == 0 && bit == 0 {
                    continue;
                }
                significant_bits = significant_bits.saturating_add(1);
                if stored_bits < 54 {
                    leading = (leading << 1) | u64::from(bit);
                    stored_bits += 1;
                } else {
                    sticky |= bit != 0;
                }
            }
        } else if byte == b'.' && !point {
            point = true;
        } else {
            break;
        }
        cursor += 1;
    }
    if !digits {
        return None;
    }
    let mut power = 0_i64;
    if matches!(input.get(cursor), Some(b'p' | b'P')) {
        cursor += 1;
        let negative = input.get(cursor) == Some(&b'-');
        if matches!(input.get(cursor), Some(b'-' | b'+')) {
            cursor += 1;
        }
        let start = cursor;
        while let Some(byte @ b'0'..=b'9') = input.get(cursor) {
            power = power
                .saturating_mul(10)
                .saturating_add(i64::from(*byte - b'0'));
            cursor += 1;
        }
        if start == cursor {
            return None;
        }
        if negative {
            power = -power;
        }
    }
    if cursor != input.len() {
        return None;
    }
    if significant_bits == 0 {
        return Some(0.0);
    }
    let mut exponent = power
        .saturating_sub(fractional_bits)
        .saturating_add(significant_bits - 1);
    if exponent > 1023 {
        return Some(f64::INFINITY);
    }
    if exponent < -1075 {
        return Some(0.0);
    }
    let precision = if exponent >= -1022 {
        53
    } else {
        (exponent + 1075) as u32
    };
    let mut rounded = if stored_bits > precision {
        leading >> (stored_bits - precision)
    } else {
        leading << (precision - stored_bits)
    };
    if stored_bits > precision {
        let discarded = stored_bits - precision;
        let guard = (leading >> (discarded - 1)) & 1 != 0;
        let trailing = leading & ((1_u64 << (discarded - 1)) - 1) != 0;
        if guard && (trailing || sticky || rounded & 1 != 0) {
            rounded += 1;
        }
    }
    if exponent < -1022 {
        return Some(f64::from_bits(rounded));
    }
    if rounded == 1_u64 << 53 {
        rounded >>= 1;
        exponent += 1;
    }
    if exponent > 1023 {
        return Some(f64::INFINITY);
    }
    Some(f64::from_bits(
        ((exponent + 1023) as u64) << 52 | (rounded - (1_u64 << 52)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_color_rounds_before_truncating_without_r_number_parser_drift() {
        for (token, index) in [
            ("1.9999999999999998", 1),
            ("1.9999999999999999", 2),
            ("0.99999999999999999", 1),
            ("0x1.fffffffffffff7", 1),
            ("0x1.fffffffffffff8", 2),
            ("0x1.fffffffffffff80000000000000001", 2),
            ("0x1.00000000000008000000000000001", 1),
            ("2147483647.9999997", 2_147_483_647),
            ("0x7fffffff.fffffc", 2_147_483_647),
            ("0x1p-1074", 0),
            ("0x0p9999999999999999999999999", 0),
        ] {
            assert_eq!(palette_index(token.as_bytes()), Some(index), "{token}");
        }
    }

    #[test]
    fn numeric_color_rejects_incomplete_tokens_and_undefined_integer_conversions() {
        for token in [
            "",
            "0x",
            "0x.",
            "0x1.2.3",
            "0x1p",
            "0x1p+",
            "0x1p-",
            "1e",
            "1e+",
            "1 ",
            "1\0tail",
            "1_0",
            "1,0",
            "+1",
            "-1",
            " 1",
            "NaN",
            "Inf",
            "1e309",
            "0x1p1024",
            "2147483648",
            "2147483647.9999999",
            "0x80000000",
            "0x7fffffff.fffffe",
        ] {
            assert_eq!(palette_index(token.as_bytes()), None, "{token:?}");
        }
    }

    #[test]
    fn numeric_color_hexadecimal_rounds_once_at_ieee_normal_and_subnormal_boundaries() {
        for (token, bits) in [
            ("1.00000000000008", 1.0_f64.to_bits()),
            ("1.00000000000008000000000000001", 1.0_f64.to_bits() + 1),
            ("1.00000000000018", 1.0_f64.to_bits() + 2),
            ("1p-1074", 1),
            ("1p-1075", 0),
            ("1.0000000000000001p-1075", 1),
            ("1.8p-1074", 2),
            ("0.fffffffffffff8p-1022", 1_u64 << 52),
            ("1.fffffffffffff8p1023", f64::INFINITY.to_bits()),
        ] {
            assert_eq!(
                hexadecimal(token.as_bytes()).unwrap().to_bits(),
                bits,
                "{token}"
            );
        }
        let token = format!("1{}p-{}", "0".repeat(2000), 2000 * 4);
        assert_eq!(hexadecimal(token.as_bytes()), Some(1.0));
    }
}
