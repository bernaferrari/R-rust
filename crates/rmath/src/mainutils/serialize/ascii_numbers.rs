//! Bounded scalar stream tokens, with exact IEEE rounding and no C parser.
#![forbid(unsafe_code)]
use num::{BigUint, One, ToPrimitive, Zero};

pub(super) fn integer(value: i32) -> String {
    if value == crate::sexp::ffi::NA_INTEGER {
        "NA".into()
    } else {
        value.to_string()
    }
}

pub(super) fn real(value: f64, hexadecimal: bool) -> String {
    if crate::sexp::ffi::R_IsNA(value) {
        return "NA".into();
    }
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Inf"
        } else {
            "Inf"
        }
        .into();
    }
    if hexadecimal {
        return hexadecimal_real(value);
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.into();
    }
    decimal_real(value)
}

fn decimal_real(value: f64) -> String {
    // Round once to 16 significant decimal digits, then select %g notation
    // using the rounded exponent. Avoid host log10/powi approximations near
    // notation boundaries and subnormal values.
    let rounded = format!("{value:.15e}");
    let (mantissa, exponent) = rounded.split_once('e').expect("finite scientific output");
    let exponent: i32 = exponent.parse().expect("finite scientific exponent");
    let sign = if value.is_sign_negative() { "-" } else { "" };
    let digits = mantissa.trim_start_matches('-').replace('.', "");
    let digits = digits.trim_end_matches('0');
    if !(-4..16).contains(&exponent) {
        let fraction = &digits[1..];
        let point = if fraction.is_empty() { "" } else { "." };
        return format!("{sign}{}{point}{fraction}e{exponent:+03}", &digits[..1]);
    }
    let point = exponent + 1;
    if point <= 0 {
        format!("{sign}0.{}{digits}", "0".repeat((-point) as usize))
    } else if point as usize >= digits.len() {
        format!(
            "{sign}{digits}{}",
            "0".repeat(point as usize - digits.len())
        )
    } else {
        let (integer, fraction) = digits.split_at(point as usize);
        format!("{sign}{integer}.{fraction}")
    }
}

fn hexadecimal_real(value: f64) -> String {
    let sign = if value.is_sign_negative() { "-" } else { "" };
    if value == 0.0 {
        return format!("{sign}0x0p+0");
    }
    let bits = value.to_bits();
    let fraction = bits & ((1_u64 << 52) - 1);
    let exponent = (bits >> 52) & 0x7ff;
    let (fraction, exponent) = if exponent == 0 {
        let highest = fraction.ilog2();
        (
            (fraction << (52 - highest)) & ((1_u64 << 52) - 1),
            i64::from(highest) - 1074,
        )
    } else {
        (fraction, exponent as i64 - 1023)
    };
    let digits = format!("{fraction:013x}");
    let digits = digits.trim_end_matches('0');
    let point = if digits.is_empty() { "" } else { "." };
    format!("{sign}0x1{point}{digits}p{exponent:+}")
}

pub(super) fn parse_real(token: &str) -> Result<f64, ()> {
    // GNU's scalar InWord buffer admits at most 127 bytes. Bound the integer
    // mantissa and shifts independently of attacker-controlled exponent text.
    if token.len() > 127 || !token.is_ascii() {
        return Err(());
    }
    if token == "NA" {
        return Ok(crate::sexp::ffi::NA_REAL);
    }
    let unsigned = token.strip_prefix(['+', '-']).unwrap_or(token);
    if unsigned.starts_with("0x") || unsigned.starts_with("0X") {
        parse_hexadecimal(token)
    } else {
        token.parse().map_err(|_| ())
    }
}

fn parse_hexadecimal(token: &str) -> Result<f64, ()> {
    let negative = token.starts_with('-');
    let token = token.strip_prefix(['+', '-']).unwrap_or(token);
    let token = token.get(2..).ok_or(())?;
    let (mantissa, exponent) = match token.find(['p', 'P']) {
        Some(position) => {
            let exponent = &token[position + 1..];
            // Clamp a syntactically valid enormous exponent before computing
            // any shift. IEEE overflow/underflow then has a defined result.
            let unsigned = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
            if unsigned.is_empty() || !unsigned.bytes().all(|b| b.is_ascii_digit()) {
                return Err(());
            }
            let value = unsigned.bytes().fold(0_i64, |value, digit| {
                value
                    .saturating_mul(10)
                    .saturating_add(i64::from(digit - b'0'))
            });
            let value = if exponent.starts_with('-') {
                -value
            } else {
                value
            };
            (&token[..position], value)
        }
        None => (token, 0),
    };
    let mut digits = [0_u8; 127];
    let mut count = 0;
    let mut point = false;
    let mut fractional_digits = 0_i64;
    for byte in mantissa.bytes() {
        if byte == b'.' && !point {
            point = true;
        } else if byte.is_ascii_hexdigit() {
            digits[count] = byte;
            count += 1;
            fractional_digits += i64::from(point);
        } else {
            return Err(());
        }
    }
    if count == 0 {
        return Err(());
    }
    let mantissa = BigUint::parse_bytes(&digits[..count], 16).ok_or(())?;
    let sign = u64::from(negative) << 63;
    if mantissa.is_zero() {
        return Ok(f64::from_bits(sign));
    }
    let exponent = exponent.saturating_sub(4 * fractional_digits);
    let highest = (mantissa.bits() as i64 - 1).saturating_add(exponent);
    if highest > 1023 {
        return Ok(f64::from_bits(sign | f64::INFINITY.to_bits()));
    }
    if highest < -1075 {
        return Ok(f64::from_bits(sign));
    }
    let quantum = (highest - 52).max(-1074);
    let shift = exponent - quantum;
    let significand = if shift >= 0 {
        (&mantissa << shift as usize).to_u64().ok_or(())?
    } else {
        let shift = (-shift) as usize;
        let truncated = &mantissa >> shift;
        let remainder = &mantissa - (&truncated << shift);
        let halfway = BigUint::one() << (shift - 1);
        let truncated = truncated.to_u64().ok_or(())?;
        truncated + u64::from(remainder > halfway || remainder == halfway && truncated & 1 != 0)
    };
    if significand == 0 {
        return Ok(f64::from_bits(sign));
    }
    let highest = significand.ilog2();
    let exponent = i64::from(highest) + quantum;
    if exponent > 1023 {
        return Ok(f64::from_bits(sign | f64::INFINITY.to_bits()));
    }
    if exponent < -1022 {
        return Ok(f64::from_bits(sign | significand));
    }
    let significand = if highest > 52 {
        significand >> (highest - 52)
    } else {
        significand << (52 - highest)
    };
    let fraction = significand & ((1_u64 << 52) - 1);
    Ok(f64::from_bits(
        sign | ((exponent + 1023) as u64) << 52 | fraction,
    ))
}
