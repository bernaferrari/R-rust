//! Checked Y diagnostics; formatting uses only bounded stack storage.
#![forbid(unsafe_code)]
use crate::sexp::{
    object::{SexpError, SexpResult},
    owner::RuntimeAccess,
};
use core::fmt::{self, Write};
struct Text<const N: usize> {
    bytes: [u8; N],
    len: usize,
}
impl<const N: usize> Text<N> {
    fn new() -> Self {
        Self {
            bytes: [0; N],
            len: 0,
        }
    }
    fn text(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).expect("formatted UTF-8")
    }
}
impl<const N: usize> Write for Text<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.len.checked_add(s.len()).ok_or(fmt::Error)?;
        self.bytes
            .get_mut(self.len..end)
            .ok_or(fmt::Error)?
            .copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}
/// C's default six-significant-digit `%g`, including rounded style selection.
struct General(f64);
impl fmt::Display for General {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.0;
        if value.is_nan() {
            return f.write_str("nan");
        }
        if value.is_sign_negative() {
            f.write_str("-")?;
        }
        if value.is_infinite() {
            return f.write_str("inf");
        }
        if value == 0.0 {
            return f.write_str("0");
        }
        let mut sci = Text::<32>::new();
        write!(&mut sci, "{:.5e}", value.abs())?;
        let bytes = sci.text().as_bytes();
        let split = bytes.iter().position(|&b| b == b'e').ok_or(fmt::Error)?;
        let mut digits = [0; 6];
        let mut len = 0;
        for &b in &bytes[..split] {
            if b != b'.' {
                *digits.get_mut(len).ok_or(fmt::Error)? = b;
                len += 1;
            }
        }
        let mut exponent = 0_i32;
        let mut negative = false;
        for &b in &bytes[split + 1..] {
            match b {
                b'-' => negative = true,
                b'+' => {}
                b'0'..=b'9' => exponent = exponent * 10 + i32::from(b - b'0'),
                _ => return Err(fmt::Error),
            }
        }
        if negative {
            exponent = -exponent;
        }
        while len > 1 && digits[len - 1] == b'0' {
            len -= 1;
        }
        let mut out = Text::<32>::new();
        if !(-4..6).contains(&exponent) {
            out.write_char(char::from(digits[0]))?;
            if len > 1 {
                out.write_char('.')?;
                out.write_str(core::str::from_utf8(&digits[1..len]).map_err(|_| fmt::Error)?)?;
            }
            write!(&mut out, "e{exponent:+03}")?;
        } else if exponent < 0 {
            out.write_str("0.")?;
            for _ in 0..(-exponent - 1) {
                out.write_char('0')?;
            }
            out.write_str(core::str::from_utf8(&digits[..len]).map_err(|_| fmt::Error)?)?;
        } else {
            let point = exponent as usize + 1;
            for i in 0..len.max(point) {
                if i == point {
                    out.write_char('.')?;
                }
                out.write_char(char::from(if i < len { digits[i] } else { b'0' }))?;
            }
        }
        f.write_str(out.text())
    }
}
fn emit(access: &RuntimeAccess, message: &Text<256>) -> SexpResult<()> {
    crate::nmath::error::ml_warn_message(message.text());
    access.require_active()
}
fn format_error() -> SexpError {
    SexpError::EvaluationFailed {
        message: "Bessel diagnostic formatting failed".into(),
    }
}
pub(super) fn positive(
    access: &RuntimeAccess,
    x: f64,
    order: f64,
    work: &mut [f64],
) -> SexpResult<f64> {
    access.require_active()?;
    if x < 0.0 {
        let value = crate::special::bessel_y::bessel_y_ex(x, order, work);
        access.require_active()?;
        return Ok(value);
    }
    if order > crate::special::bessel_y::MAX_ALPHA_Y {
        let mut message = Text::new();
        write!(
            &mut message,
            "besselY(x, nu): nu={} too large for bessel_y() algorithm",
            General(order)
        )
        .map_err(|_| format_error())?;
        emit(access, &message)?;
        return Ok(f64::NAN);
    }
    let result =
        crate::special::bessel_y::bessel_y_positive_status(x, order, work).ok_or_else(|| {
            SexpError::EvaluationFailed {
                message: "invalid checked Bessel Y calculation".into(),
            }
        })?;
    if result.ncalc != result.count && result.ncalc != -1 {
        let mut message = Text::new();
        if result.ncalc < -1 {
            write!(
                &mut message,
                "bessel_y({}): ncalc (={}) != nb (={}); alpha={}. Arg. out of range?",
                General(x),
                result.ncalc,
                result.count,
                General(result.fraction)
            )
            .map_err(|_| format_error())?;
        } else {
            write!(
                &mut message,
                "bessel_y({},nu={}): precision lost in result",
                General(x),
                General(result.fraction + f64::from(result.count) - 1.0)
            )
            .map_err(|_| format_error())?;
        }
        emit(access, &message)?;
    }
    Ok(result.value)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bessely_diagnostic_general_matches_pinned_c_format() {
        for (value, expected) in [
            (0.0, "0"),
            (-0.0, "-0"),
            (1e-300, "1e-300"),
            (1e-8, "1e-08"),
            (0.00123456789, "0.00123457"),
            (12.3456789, "12.3457"),
            (1e7 + 1.0, "1e+07"),
            (1.23456789e8, "1.23457e+08"),
            (f64::INFINITY, "inf"),
            (f64::NEG_INFINITY, "-inf"),
            (f64::NAN, "nan"),
            (0.0001, "0.0001"),
            (0.0000999999, "9.99999e-05"),
            (999999.0, "999999"),
            (1000000.0, "1e+06"),
            (9.999999e-5, "0.0001"),
            (999999.9, "1e+06"),
            (9.999995e-5, "0.0001"),
            (0.001234565, "0.00123456"),
            (0.001234575, "0.00123457"),
            (10000050.0, "1e+07"),
            (10000150.0, "1.00002e+07"),
        ] {
            let mut text = Text::<32>::new();
            write!(&mut text, "{}", General(value)).unwrap();
            assert_eq!(text.text(), expected);
        }
    }
}
