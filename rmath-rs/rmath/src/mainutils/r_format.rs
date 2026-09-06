//! In-engine typed printf engine.
//!
//! Stage B of the zero-libc-engine program: the engine must not call the
//! the C-variadic libc printf entry point (which wasm32 cannot define). This module is
//! the typed printf engine seeded from the wasm-libc facade
//! (`rmath-rs/wasm-libc/src/facade/printf.rs`) and adapted to Rust-native
//! inputs: format strings are `&str`, arguments are [`CArg`] values, output
//! is a [`String`] (or a bounded NUL-terminated write via [`r_snprintf`]).
//!
//! Supported printf subset (everything the C-ported tree uses):
//!
//! * conversions: `%d %i %u %o %x %X %c %s %p %f %F %e %E %g %G %%`
//! * flags: `-`, `+`, space, `0`, `#`
//! * width and precision (including `*`), length modifiers `hh h l ll z`
//!
//! Byte-compatibility with C `snprintf` output is pinned by the conformance
//! goldens; do not "simplify" formats into Rust `format!` where C and Rust
//! disagree (`%e`, `%g`, `%#m`-style details).

/// One printf variadic argument, pre-boxed by the caller. `Str` holds raw
/// bytes so C-string arguments round-trip byte-exactly.
#[derive(Clone, Copy)]
pub enum CArg<'a> {
    Int(i64),
    UInt(u64),
    Double(f64),
    Str(&'a [u8]),
    Char(u8),
    Ptr(usize),
}

impl From<i32> for CArg<'_> {
    fn from(v: i32) -> Self {
        CArg::Int(v as i64)
    }
}
impl From<u32> for CArg<'_> {
    fn from(v: u32) -> Self {
        CArg::UInt(v as u64)
    }
}
impl From<i64> for CArg<'_> {
    fn from(v: i64) -> Self {
        CArg::Int(v)
    }
}
impl From<u64> for CArg<'_> {
    fn from(v: u64) -> Self {
        CArg::UInt(v)
    }
}
impl From<usize> for CArg<'_> {
    fn from(v: usize) -> Self {
        CArg::UInt(v as u64)
    }
}
impl From<isize> for CArg<'_> {
    fn from(v: isize) -> Self {
        CArg::Int(v as i64)
    }
}
impl From<f64> for CArg<'_> {
    fn from(v: f64) -> Self {
        CArg::Double(v)
    }
}
impl From<f32> for CArg<'_> {
    fn from(v: f32) -> Self {
        CArg::Double(v as f64)
    }
}
impl<'a> From<&'a str> for CArg<'a> {
    fn from(v: &'a str) -> Self {
        CArg::Str(v.as_bytes())
    }
}
impl<'a> From<&'a String> for CArg<'a> {
    fn from(v: &'a String) -> Self {
        CArg::Str(v.as_bytes())
    }
}
impl<'a> From<&'a [u8]> for CArg<'a> {
    fn from(v: &'a [u8]) -> Self {
        CArg::Str(v)
    }
}
impl<'a> From<&'a std::ffi::CStr> for CArg<'a> {
    fn from(v: &'a std::ffi::CStr) -> Self {
        CArg::Str(v.to_bytes())
    }
}
impl From<char> for CArg<'_> {
    fn from(v: char) -> Self {
        CArg::Char(v as u8)
    }
}

#[derive(Clone, Copy, Default)]
struct Flags {
    minus: bool,
    plus: bool,
    space: bool,
    zero: bool,
    alt: bool,
}

struct Emitter<'a> {
    buf: &'a mut Vec<u8>,
    limit: Option<usize>, // buffer capacity incl. NUL slot; None = unlimited
    count: usize,         // untruncated length, like C snprintf's return value
}

impl Emitter<'_> {
    /// Appends to the output when there is room; always grows the
    /// untruncated count so the return value matches C even when truncated.
    fn push(&mut self, bytes: &[u8]) {
        self.count += bytes.len();
        match self.limit {
            None => self.buf.extend_from_slice(bytes),
            Some(0) => {}
            Some(limit) => {
                let room = limit - 1;
                if self.buf.len() < room {
                    let take = (room - self.buf.len()).min(bytes.len());
                    self.buf.extend_from_slice(&bytes[..take]);
                }
            }
        }
    }

    fn len(&self) -> usize {
        self.count
    }
}

/// `snprintf` over typed arguments into a fixed buffer. Writes at most
/// `buf.len() - 1` bytes plus a NUL terminator (when the buffer is non-empty)
/// and returns the number of characters that *would have* been written (C
/// semantics — a return `> buf.len() - 1` means truncation).
pub fn r_snprintf(buf: &mut [u8], fmt: &[u8], args: &[CArg]) -> usize {
    let mut out: Vec<u8> = Vec::new();
    let limit = buf.len();
    let mut em = Emitter {
        buf: &mut out,
        limit: Some(limit),
        count: 0,
    };
    format_into(&mut em, fmt, args);
    let would_write = em.len();
    if !buf.is_empty() {
        let term = out.len().min(buf.len() - 1);
        buf[..term].copy_from_slice(&out[..term]);
        buf[term] = 0;
    }
    would_write
}

/// `snprintf` over typed arguments into a fixed `c_char` buffer (the
/// C-ported tree's `[c_char; N]` locals). Same contract as [`r_snprintf`]:
/// NUL-terminated, at most `buf.len() - 1` payload bytes, returns the
/// would-write count.
pub fn r_snprintf_c(buf: &mut [core::ffi::c_char], fmt: &[u8], args: &[CArg]) -> usize {
    // SAFETY: c_char is a one-byte integer type on all supported targets;
    // borrowing the same memory as u8 for the duration of the call is sound.
    let bytes = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr().cast::<u8>(), buf.len()) };
    r_snprintf(bytes, fmt, args)
}

/// `sprintf` over typed arguments: returns the full formatted string.
pub fn r_sprintf(fmt: &str, args: &[CArg]) -> String {
    let mut out: Vec<u8> = Vec::new();
    let mut em = Emitter {
        buf: &mut out,
        limit: None,
        count: 0,
    };
    format_into(&mut em, fmt.as_bytes(), args);
    // printf output over the tree's UTF-8/Rust-native inputs is UTF-8; fall
    // back to a lossy decode only for exotic `%c` byte pushes.
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

fn format_into(em: &mut Emitter, fmt: &[u8], args: &[CArg]) {
    let mut next_arg = 0usize;
    let mut i = 0usize;

    while i < fmt.len() {
        if fmt[i] != b'%' {
            em.push(&[fmt[i]]);
            i += 1;
            continue;
        }
        i += 1;
        if i >= fmt.len() {
            break;
        }
        if fmt[i] == b'%' {
            em.push(b"%");
            i += 1;
            continue;
        }

        // flags
        let mut flags = Flags::default();
        loop {
            match fmt.get(i) {
                Some(b'-') => flags.minus = true,
                Some(b'+') => flags.plus = true,
                Some(b' ') => flags.space = true,
                Some(b'0') => flags.zero = true,
                Some(b'#') => flags.alt = true,
                _ => break,
            }
            i += 1;
        }

        // width
        let mut width: Option<usize> = None;
        if fmt.get(i) == Some(&b'*') {
            i += 1;
            let w = match args.get(next_arg) {
                Some(CArg::Int(v)) => *v as i32,
                Some(CArg::UInt(v)) => *v as i32,
                _ => 0,
            };
            next_arg += 1;
            if w < 0 {
                flags.minus = true;
                width = Some((-w) as usize);
            } else {
                width = Some(w as usize);
            }
        } else {
            let start = i;
            while matches!(fmt.get(i), Some(b'0'..=b'9')) {
                i += 1;
            }
            if i > start {
                width = Some(
                    std::str::from_utf8(&fmt[start..i])
                        .unwrap_or("0")
                        .parse()
                        .unwrap_or(0),
                );
            }
        }

        // precision
        let mut precision: Option<usize> = None;
        if fmt.get(i) == Some(&b'.') {
            i += 1;
            if fmt.get(i) == Some(&b'*') {
                i += 1;
                let p = match args.get(next_arg) {
                    Some(CArg::Int(v)) => *v as i32,
                    _ => 0,
                };
                next_arg += 1;
                precision = Some(p.max(0) as usize);
            } else {
                let start = i;
                while matches!(fmt.get(i), Some(b'0'..=b'9')) {
                    i += 1;
                }
                precision = Some(
                    std::str::from_utf8(&fmt[start..i])
                        .unwrap_or("0")
                        .parse()
                        .unwrap_or(0),
                );
            }
        }

        // length modifier (accepted for C compatibility; the typed CArg
        // already carries the full integer width)
        while matches!(
            fmt.get(i),
            Some(b'h') | Some(b'l') | Some(b'z') | Some(b'j') | Some(b't') | Some(b'L')
        ) {
            i += 1;
        }

        let Some(conv) = fmt.get(i).copied() else {
            break;
        };
        i += 1;

        let arg = args.get(next_arg).copied();
        let as_signed = |a: Option<CArg>| -> i64 {
            match a {
                Some(CArg::Int(v)) => v,
                Some(CArg::UInt(v)) => v as i64,
                _ => 0,
            }
        };
        let as_unsigned = |a: Option<CArg>| -> u64 {
            match a {
                Some(CArg::Int(v)) => v as u64,
                Some(CArg::UInt(v)) => v,
                _ => 0,
            }
        };
        let as_double = |a: Option<CArg>| -> f64 {
            match a {
                Some(CArg::Double(v)) => v,
                Some(CArg::Int(v)) => v as f64,
                Some(CArg::UInt(v)) => v as f64,
                _ => 0.0,
            }
        };

        match conv {
            b'd' | b'i' => {
                next_arg += 1;
                let v = as_signed(arg);
                let body = v.to_string();
                let body = if v < 0 {
                    body
                } else if flags.plus {
                    format!("+{body}")
                } else if flags.space {
                    format!(" {body}")
                } else {
                    body
                };
                emit_padded(em, body.as_bytes(), width, flags, precision, b'0', false);
            }
            b'u' => {
                next_arg += 1;
                let body = as_unsigned(arg).to_string();
                emit_padded(em, body.as_bytes(), width, flags, precision, b'0', false);
            }
            b'o' => {
                next_arg += 1;
                let v = as_unsigned(arg);
                let mut body = format!("{v:o}");
                if flags.alt && !body.starts_with('0') {
                    body = format!("0{body}");
                }
                emit_padded(em, body.as_bytes(), width, flags, precision, b'0', false);
            }
            b'x' => {
                next_arg += 1;
                let v = as_unsigned(arg);
                let mut body = format!("{v:x}");
                if flags.alt && v != 0 {
                    body = format!("0x{body}");
                }
                emit_padded(em, body.as_bytes(), width, flags, precision, b'0', false);
            }
            b'X' => {
                next_arg += 1;
                let v = as_unsigned(arg);
                let mut body = format!("{v:X}");
                if flags.alt && v != 0 {
                    body = format!("0X{body}");
                }
                emit_padded(em, body.as_bytes(), width, flags, precision, b'0', false);
            }
            b'c' => {
                next_arg += 1;
                let ch = match arg {
                    Some(CArg::Char(c)) => c,
                    Some(CArg::Int(v)) => v as u8,
                    Some(CArg::UInt(v)) => v as u8,
                    _ => 0,
                };
                emit_padded(
                    em,
                    std::slice::from_ref(&ch),
                    width,
                    flags,
                    None,
                    b' ',
                    false,
                );
            }
            b's' => {
                next_arg += 1;
                let mut text: &[u8] = match arg {
                    Some(CArg::Str(s)) => s,
                    _ => b"(null)",
                };
                if let Some(p) = precision {
                    text = &text[..text.len().min(p)];
                }
                emit_padded(em, text, width, flags, None, b' ', false);
            }
            b'p' => {
                next_arg += 1;
                let v = match arg {
                    Some(CArg::Ptr(v)) => v,
                    Some(CArg::UInt(v)) => v as usize,
                    Some(CArg::Int(v)) => v as usize,
                    _ => 0,
                };
                let body = if v == 0 {
                    "(nil)".to_string()
                } else {
                    format!("0x{v:x}")
                };
                emit_padded(em, body.as_bytes(), width, flags, None, b' ', false);
            }
            b'f' | b'F' => {
                next_arg += 1;
                let v = as_double(arg);
                let p = precision.unwrap_or(6);
                let body = format_fixed(v, p);
                let body = apply_sign(body, v, flags);
                emit_padded(em, body.as_bytes(), width, flags, None, b'0', false);
            }
            b'e' | b'E' => {
                next_arg += 1;
                let v = as_double(arg);
                let p = precision.unwrap_or(6);
                let upper = conv == b'E';
                let body = format_exponent(v, p, upper);
                let body = apply_sign(body, v, flags);
                emit_padded(em, body.as_bytes(), width, flags, None, b'0', false);
            }
            b'g' | b'G' => {
                next_arg += 1;
                let v = as_double(arg);
                let p = precision.unwrap_or(6);
                let upper = conv == b'G';
                let body = format_general(v, p, upper, flags.alt);
                let body = apply_sign(body, v, flags);
                emit_padded(em, body.as_bytes(), width, flags, None, b'0', false);
            }
            _ => {
                // Unknown conversion: emit it verbatim (flags dropped).
                em.push(&[b'%', conv]);
            }
        }
    }
}

fn apply_sign(body: String, v: f64, flags: Flags) -> String {
    if v.is_sign_negative() {
        body // '-' already rendered by Rust
    } else if flags.plus {
        format!("+{body}")
    } else if flags.space {
        format!(" {body}")
    } else {
        body
    }
}

fn emit_padded(
    em: &mut Emitter,
    body: &[u8],
    width: Option<usize>,
    flags: Flags,
    precision: Option<usize>,
    default_pad: u8,
    numeric: bool,
) {
    let mut body = body.to_vec();
    if numeric {
        if let Some(p) = precision {
            // %0*d-style zero padding from precision applies to the digits
            // only; these call sites use plain decimal tags.
            if body.len() < p && !body.starts_with(b"-") {
                let mut v = vec![b'0'; p - body.len()];
                v.extend_from_slice(&body);
                body = v;
            }
        }
    }
    let width = width.unwrap_or(0);
    if body.len() >= width {
        em.push(&body);
        return;
    }
    let fill = width - body.len();
    if flags.minus {
        em.push(&body);
        em.push(&vec![b' '; fill]);
    } else {
        let pad = if flags.zero && default_pad == b'0' {
            b'0'
        } else {
            b' '
        };
        if pad == b'0' {
            // zero padding goes after any sign
            let (sign, rest) = split_sign(&body);
            em.push(sign);
            em.push(&vec![b'0'; fill]);
            em.push(rest);
        } else {
            em.push(&vec![b' '; fill]);
            em.push(&body);
        }
    }
}

fn split_sign(body: &[u8]) -> (&[u8], &[u8]) {
    match body.first() {
        Some(b'-') | Some(b'+') | Some(b' ') => body.split_at(1),
        _ => (&[], body),
    }
}

/// %f with precision (handles inf/nan).
fn format_fixed(v: f64, precision: usize) -> String {
    if v.is_nan() {
        return "nan".to_string();
    }
    if v.is_infinite() {
        return if v > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    format!("{v:.precision$}")
}

/// %e with precision: mantissa in [1,10), exponent always with sign and >=2 digits.
fn format_exponent(v: f64, precision: usize, upper: bool) -> String {
    let e_char = if upper { 'E' } else { 'e' };
    if v.is_nan() {
        return if upper { "NAN" } else { "nan" }.to_string();
    }
    if v.is_infinite() {
        let s = if v > 0.0 { "INF" } else { "-INF" }.to_string();
        return if upper {
            s.to_uppercase()
        } else {
            s.to_lowercase()
        };
    }
    if v == 0.0 {
        let s = format!("0.{:0<width$}", "", width = precision);
        return format!("{s}{e_char}+00");
    }
    // Rust's {:e} gives "1.5e2"; expand to C form "1.500000e+02".
    let rust = format!("{v:.precision$e}");
    let (mant, exp) = rust.split_once('e').unwrap_or((rust.as_str(), "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    format!(
        "{mant}{e_char}{}{:02}",
        if exp < 0 { '-' } else { '+' },
        exp.abs()
    )
}

/// %g: %e or %f by exponent, trailing zeros stripped unless `alt`.
fn format_general(v: f64, precision: usize, upper: bool, alt: bool) -> String {
    if v.is_nan() || v.is_infinite() {
        return format_exponent(v, precision, upper);
    }
    let p = precision.max(1);
    if v == 0.0 {
        return if alt {
            format!("{:.*}", p - 1, v)
        } else {
            "0".to_string()
        };
    }
    // Decimal exponent from Rust's shortest e-notation of the rounded value.
    let rounded = format!("{v:.p$e}");
    let (_, exp_str) = rounded.split_once('e').unwrap_or(("", "0"));
    let x: i32 = exp_str.parse().unwrap_or(0);
    let mut body = if x >= -4 && (x as i64) < p as i64 {
        let fp = (p as i64 - 1 - x as i64).max(0) as usize;
        format!("{v:.fp$}")
    } else {
        format_exponent(v, p - 1, upper)
    };
    if !alt {
        if body.contains('.') {
            while body.ends_with('0') {
                body.pop();
            }
            if body.ends_with('.') {
                body.pop();
            }
        }
    }
    if upper {
        body = body.to_uppercase();
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(fmt: &str, args: &[CArg<'_>]) -> String {
        r_sprintf(fmt, args)
    }

    #[test]
    fn strings_and_ints() {
        assert_eq!(f("[[%%d]] %d", &[CArg::Int(3)]), "[[%d]] 3");
        assert_eq!(f("%s=%05d", &[CArg::Str(b"n"), CArg::Int(42)]), "n=00042");
        assert_eq!(
            f(
                "%x %X %o %u",
                &[
                    CArg::UInt(255),
                    CArg::UInt(255),
                    CArg::UInt(8),
                    CArg::UInt(7)
                ]
            ),
            "ff FF 10 7"
        );
        assert_eq!(f("%c%c", &[CArg::Char(b'a'), CArg::Char(b'b')]), "ab");
    }

    #[test]
    fn floats() {
        assert_eq!(f("%f", &[CArg::Double(1.5)]), "1.500000");
        assert_eq!(f("%.2f", &[CArg::Double(3.14159)]), "3.14");
        assert_eq!(f("%e", &[CArg::Double(12345.0)]), "1.234500e+04");
        assert_eq!(f("%g", &[CArg::Double(0.0001)]), "0.0001");
        assert_eq!(f("%g", &[CArg::Double(1234567.0)]), "1.23457e+06");
        assert_eq!(f("%.3g", &[CArg::Double(0.5)]), "0.5");
        assert_eq!(f("%.17g", &[CArg::Double(0.1)]), "0.10000000000000001");
    }

    #[test]
    fn longs_and_widths() {
        assert_eq!(f("%ld", &[CArg::Int(-5)]), "-5");
        assert_eq!(f("%lu", &[CArg::UInt(5_000_000_000)]), "5000000000");
        assert_eq!(f("%8.3s|", &[CArg::Str(b"abcdef")]), "     abc|");
        assert_eq!(f("%-8d|", &[CArg::Int(3)]), "3       |");
        assert_eq!(f("%+d", &[CArg::Int(3)]), "+3");
        assert_eq!(f("%*d|", &[CArg::Int(5), CArg::Int(42)]), "   42|");
        assert_eq!(f("%.*f", &[CArg::Int(2), CArg::Double(3.14159)]), "3.14");
        assert_eq!(
            f(
                "%*.*f",
                &[CArg::Int(8), CArg::Int(3), CArg::Double(3.14159)]
            ),
            "   3.142"
        );
    }

    #[test]
    fn truncation_and_ret() {
        let mut buf = [0u8; 5];
        let n = r_snprintf(&mut buf, b"abcdefgh", &[]);
        assert_eq!(n, 8); // return is the untruncated length, like C
        let end = buf.iter().position(|&c| c == 0).unwrap();
        assert_eq!(&buf[..end], b"abcd");
    }

    #[test]
    fn star_width_negative_is_left_align() {
        assert_eq!(f("%*d|", &[CArg::Int(-5), CArg::Int(42)]), "42   |");
    }
}
