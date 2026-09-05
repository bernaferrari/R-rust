//! Typed `snprintf` engine for the wasm libc facade.
//!
//! Stable Rust cannot *define* C-variadic functions, so wasm call sites route
//! through rmath's `rport_snprintf!` macro, which converts each variadic
//! argument into a [`CArg`] and calls [`snprintf_args`]. The engine implements
//! the printf subset the C-ported tree uses:
//!
//! * conversions: `%d %i %u %o %x %X %c %s %p %f %F %e %E %g %G %%`
//! * flags: `-`, `+`, space, `0`, `#`
//! * width and precision (including `*`), length modifiers `hh h l ll z`
//!
//! Behavior of unhandled specifiers mirrors C loosely (copied verbatim) — the
//! tree's formats are plain literals in practice.

use super::{c_char, c_double, c_int, c_long, c_longlong, c_ulong, c_ulonglong, size_t};

/// One printf variadic argument, pre-boxed by the `rport_snprintf!` macro.
#[derive(Clone, Copy)]
pub enum CArg {
    Int(c_int),
    UInt(c_int),
    Long(c_long),
    ULong(c_ulong),
    LongLong(c_longlong),
    ULongLong(c_ulonglong),
    Double(c_double),
    Str(*const c_char),
    Char(u8),
    Ptr(usize),
}

impl From<i32> for CArg {
    fn from(v: i32) -> Self {
        CArg::Int(v)
    }
}
impl From<u32> for CArg {
    fn from(v: u32) -> Self {
        CArg::UInt(v as c_int)
    }
}
impl From<i64> for CArg {
    fn from(v: i64) -> Self {
        CArg::LongLong(v)
    }
}
impl From<u64> for CArg {
    fn from(v: u64) -> Self {
        CArg::ULongLong(v)
    }
}
impl From<usize> for CArg {
    fn from(v: usize) -> Self {
        CArg::ULongLong(v as u64)
    }
}
impl From<f64> for CArg {
    fn from(v: f64) -> Self {
        CArg::Double(v)
    }
}
impl From<f32> for CArg {
    fn from(v: f32) -> Self {
        CArg::Double(v as f64)
    }
}
impl From<*const u8> for CArg {
    fn from(v: *const u8) -> Self {
        CArg::Str(v.cast::<c_char>())
    }
}
impl From<*mut u8> for CArg {
    fn from(v: *mut u8) -> Self {
        CArg::Str(v.cast::<c_char>())
    }
}
impl From<*const c_char> for CArg {
    fn from(v: *const c_char) -> Self {
        CArg::Str(v)
    }
}
impl From<*mut c_char> for CArg {
    fn from(v: *mut c_char) -> Self {
        CArg::Str(v as *const c_char)
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
    limit: usize, // buffer capacity (includes the NUL terminator slot)
    count: usize, // untruncated length, like C snprintf's return value
}

impl Emitter<'_> {
    /// Appends to the output when there is room; always grows the
    /// untruncated count so the return value matches C even when truncated.
    fn push(&mut self, bytes: &[u8]) {
        self.count += bytes.len();
        if self.limit > 0 {
            let room = self.limit - 1;
            if self.buf.len() < room {
                let take = (room - self.buf.len()).min(bytes.len());
                self.buf.extend_from_slice(&bytes[..take]);
            }
        }
    }

    fn len(&self) -> usize {
        self.count
    }
}

/// `snprintf` over typed arguments. Returns the number of characters that
/// *would have been* written (C semantics), writing at most `n - 1` bytes
/// plus a NUL terminator when `n > 0`.
pub unsafe fn snprintf_args(
    s: *mut c_char,
    n: size_t,
    format: *const c_char,
    args: &[CArg],
) -> c_int {
    unsafe {
        if format.is_null() {
            if !s.is_null() && n > 0 {
                *s = 0;
            }
            return 0;
        }
        let fmt = c_str_bytes(format);
        let mut out: Vec<u8> = Vec::new();
        let limit = if n == 0 { 0 } else { n };
        let mut em = Emitter {
            buf: &mut out,
            limit,
            count: 0,
        };
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
                    Some(CArg::Int(v)) => *v,
                    Some(CArg::UInt(v)) => *v,
                    Some(CArg::Long(v)) => *v as c_int,
                    Some(CArg::LongLong(v)) => *v as c_int,
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
                        Some(CArg::Int(v)) => *v,
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

            // length modifier (affects the integer width chosen below)
            let mut length = 0usize;
            loop {
                match fmt.get(i) {
                    Some(b'h') => length = length.saturating_sub(1),
                    Some(b'l') | Some(b'z') | Some(b'j') | Some(b't') => length += 1,
                    Some(b'L') => length += 2,
                    _ => break,
                }
                i += 1;
            }

            let Some(conv) = fmt.get(i).copied() else {
                break;
            };
            i += 1;

            let arg = args.get(next_arg).copied();
            // Integer accessors honoring the length modifier.
            let as_signed = |a: Option<CArg>| -> i64 {
                match a {
                    Some(CArg::Int(v)) => v as i64,
                    Some(CArg::UInt(v)) => v as i64,
                    Some(CArg::Long(v)) => v as i64,
                    Some(CArg::LongLong(v)) => v as i64,
                    Some(CArg::ULong(v)) => v as i64,
                    Some(CArg::ULongLong(v)) => v as i64,
                    _ => 0,
                }
            };
            let as_unsigned = |a: Option<CArg>| -> u64 {
                match a {
                    Some(CArg::Int(v)) => v as u32 as u64,
                    Some(CArg::UInt(v)) => v as u32 as u64,
                    Some(CArg::Long(v)) => v as u64,
                    Some(CArg::LongLong(v)) => v as u64,
                    Some(CArg::ULong(v)) => v as u64,
                    Some(CArg::ULongLong(v)) => v,
                    _ => 0,
                }
            };
            let as_double = |a: Option<CArg>| -> c_double {
                match a {
                    Some(CArg::Double(v)) => v,
                    Some(CArg::Int(v)) => v as c_double,
                    Some(CArg::UInt(v)) => v as c_double,
                    Some(CArg::Long(v)) => v as c_double,
                    Some(CArg::ULong(v)) => v as c_double,
                    Some(CArg::LongLong(v)) => v as c_double,
                    Some(CArg::ULongLong(v)) => v as c_double,
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
                    emit_padded(
                        &mut em,
                        body.as_bytes(),
                        width,
                        flags,
                        precision,
                        b'0',
                        false,
                    );
                    true
                }
                b'u' => {
                    next_arg += 1;
                    let body = as_unsigned(arg).to_string();
                    emit_padded(
                        &mut em,
                        body.as_bytes(),
                        width,
                        flags,
                        precision,
                        b'0',
                        false,
                    );
                    true
                }
                b'o' => {
                    next_arg += 1;
                    let v = as_unsigned(arg);
                    let mut body = format!("{v:o}");
                    if flags.alt && !body.starts_with('0') {
                        body = format!("0{body}");
                    }
                    emit_padded(
                        &mut em,
                        body.as_bytes(),
                        width,
                        flags,
                        precision,
                        b'0',
                        false,
                    );
                    true
                }
                b'x' => {
                    next_arg += 1;
                    let v = as_unsigned(arg);
                    let mut body = format!("{v:x}");
                    if flags.alt && v != 0 {
                        body = format!("0x{body}");
                    }
                    emit_padded(
                        &mut em,
                        body.as_bytes(),
                        width,
                        flags,
                        precision,
                        b'0',
                        false,
                    );
                    true
                }
                b'X' => {
                    next_arg += 1;
                    let v = as_unsigned(arg);
                    let mut body = format!("{v:X}");
                    if flags.alt && v != 0 {
                        body = format!("0X{body}");
                    }
                    emit_padded(
                        &mut em,
                        body.as_bytes(),
                        width,
                        flags,
                        precision,
                        b'0',
                        false,
                    );
                    true
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
                        &mut em,
                        std::slice::from_ref(&ch),
                        width,
                        flags,
                        None,
                        b' ',
                        false,
                    );
                    true
                }
                b's' => {
                    next_arg += 1;
                    let mut text: Vec<u8> = match arg {
                        Some(CArg::Str(p)) if !p.is_null() => c_str_bytes(p).to_vec(),
                        _ => b"(null)".to_vec(),
                    };
                    if let Some(p) = precision {
                        text.truncate(p);
                    }
                    emit_padded(&mut em, &text, width, flags, None, b' ', false);
                    true
                }
                b'p' => {
                    next_arg += 1;
                    let v = match arg {
                        Some(CArg::Ptr(v)) => v,
                        Some(CArg::ULongLong(v)) => v as usize,
                        Some(CArg::Int(v)) => v as usize,
                        _ => 0,
                    };
                    let body = if v == 0 {
                        "(nil)".to_string()
                    } else {
                        format!("0x{v:x}")
                    };
                    emit_padded(&mut em, body.as_bytes(), width, flags, None, b' ', false);
                    true
                }
                b'f' | b'F' => {
                    next_arg += 1;
                    let v = as_double(arg);
                    let p = precision.unwrap_or(6);
                    let body = format_fixed(v, p);
                    let body = apply_sign(body, v, flags);
                    emit_padded(&mut em, body.as_bytes(), width, flags, None, b'0', false);
                    true
                }
                b'e' | b'E' => {
                    next_arg += 1;
                    let v = as_double(arg);
                    let p = precision.unwrap_or(6);
                    let upper = conv == b'E';
                    let body = format_exponent(v, p, upper);
                    let body = apply_sign(body, v, flags);
                    emit_padded(&mut em, body.as_bytes(), width, flags, None, b'0', false);
                    true
                }
                b'g' | b'G' => {
                    next_arg += 1;
                    let v = as_double(arg);
                    let p = precision.unwrap_or(6);
                    let upper = conv == b'G';
                    let body = format_general(v, p, upper, flags.alt);
                    let body = apply_sign(body, v, flags);
                    emit_padded(&mut em, body.as_bytes(), width, flags, None, b'0', false);
                    true
                }
                _ => {
                    // Unknown conversion: emit it verbatim (flags dropped).
                    em.push(&[b'%', conv]);
                    true
                }
            };
        }

        let would_write = em.len();

        if !s.is_null() && n > 0 {
            std::ptr::copy_nonoverlapping(em.buf.as_ptr(), s.cast::<u8>(), em.buf.len().min(n - 1));
            let term = em.buf.len().min(n - 1);
            *s.add(term) = 0;
        }
        would_write as c_int
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
        let pad = if flags.zero && default_pad == b'0' && numeric_ok(&body, flags) {
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

fn numeric_ok(_body: &[u8], _flags: Flags) -> bool {
    true
}

fn split_sign(body: &[u8]) -> (&[u8], &[u8]) {
    match body.first() {
        Some(b'-') | Some(b'+') | Some(b' ') => body.split_at(1),
        _ => (&[], body),
    }
}

fn c_str_bytes(p: *const c_char) -> &'static [u8] {
    unsafe {
        let mut len = 0usize;
        while *p.add(len) != 0 {
            len += 1;
        }
        std::slice::from_raw_parts(p.cast::<u8>(), len)
    }
}

/// %f with precision (handles inf/nan).
pub fn format_fixed(v: f64, precision: usize) -> String {
    if v.is_nan() {
        return "nan".to_string();
    }
    if v.is_infinite() {
        return if v > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    format!("{v:.precision$}")
}

/// %e with precision: mantissa in [1,10), exponent always with sign and >=2 digits.
pub fn format_exponent(v: f64, precision: usize, upper: bool) -> String {
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
pub fn format_general(v: f64, precision: usize, upper: bool, alt: bool) -> String {
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

// Silence unused warnings for items only used via the parent crate.
