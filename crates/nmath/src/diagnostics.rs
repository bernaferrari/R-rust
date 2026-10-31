//! Bounded formatting for math diagnostics.
#![forbid(unsafe_code)]
use core::fmt::{self, Write};

pub struct Text<const N: usize> {
    bytes: [u8; N],
    len: usize,
}
impl<const N: usize> Text<N> {
    pub fn new() -> Self {
        Self {
            bytes: [0; N],
            len: 0,
        }
    }
    pub fn text(&self) -> &str {
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
pub struct General(pub f64);
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
