//! output — extracted verbatim from output.rs.

use super::*;

pub(crate) unsafe fn format_sexp(x: SEXP) -> String {
    if x.is_null() {
        return "NULL".to_string();
    }
    if let Some(sexp) = unsafe { crate::sexp::object::Sexp::from_raw(x) } {
        format_sexp_direct(sexp)
    } else {
        "NULL".to_string()
    }
}

pub(crate) fn format_aligned_values(vals: Vec<String>) -> String {
    let width = vals.iter().map(|v| v.len()).max().unwrap_or(0);
    vals.into_iter()
        .map(|v| format!("{v:>width$}"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn format_named_values(names: &[String], values: &[String]) -> String {
    let widths: Vec<usize> = names
        .iter()
        .zip(values)
        .map(|(name, value)| {
            let width = name.len().max(value.len());
            if name == "<NA>" { width.max(5) } else { width }
        })
        .collect();
    let name_line = names
        .iter()
        .zip(&widths)
        .map(|(name, width)| format!("{name:>width$}"))
        .collect::<Vec<_>>()
        .join(" ");
    let value_line = values
        .iter()
        .zip(&widths)
        .map(|(value, width)| format!("{value:>width$}"))
        .collect::<Vec<_>>()
        .join(" ");
    format!("{name_line}\n{value_line}")
}

pub(crate) fn format_integer_value(v: i32) -> String {
    if v == NA_INTEGER {
        "NA".to_string()
    } else {
        v.to_string()
    }
}

pub(crate) fn format_real_value(v: f64) -> String {
    let v = if v == 0.0 { 0.0 } else { v };
    if R_IsNA(v) {
        "NA".to_string()
    } else if R_IsNaN(v) {
        "NaN".to_string()
    } else if v.is_infinite() {
        if v.is_sign_negative() {
            "-Inf".to_string()
        } else {
            "Inf".to_string()
        }
    } else if v == 0.0 {
        // log10(0) is -inf. The i32 cast saturates and the decimal count
        // overflows before clamp. GNU prints both signed zeros as 0.
        "0".to_string()
    } else if !needs_scientific(v) {
        let (_, rgt, _) = real_format_widths(v);
        let decimals = rgt.max(0) as usize;
        format!("{v:.decimals$}")
    } else {
        format_r_default_real(v)
    }
}

/// `scientific()` from GNU format.c: (neg, kpower, nsig) for a finite
/// non-zero double, with `nsig = min(R_print.digits, #{significant digits})`.
pub(crate) fn scientific_parts(v: f64) -> (i32, i32, i32) {
    let digits = unsafe { crate::mainutils::format::format_get_R_print().digits };
    let neg = if v < 0.0 { 1 } else { 0 };
    // Rust's `{:e}` with no precision is the shortest round-trip form, so its
    // mantissa digit count is the #{significant digits} GNU computes.
    let s = format!("{:e}", v.abs());
    let (mantissa, exponent) = s.split_once('e').unwrap_or((s.as_str(), "0"));
    let kpower: i32 = exponent.parse().unwrap_or(0);
    let nsig_actual = mantissa.chars().filter(|c| c.is_ascii_digit()).count() as i32;
    let nsig = if digits < 1 {
        1
    } else {
        digits.min(nsig_actual).max(1)
    };
    (neg, kpower, nsig)
}

/// The single-element case of GNU formatReal (format.c): fixed width `wF`,
/// fixed decimals `rgt`, and scientific width `wE`, used to pick the format.
/// Returns `(wF, rgt, wE)`.
pub(crate) fn real_format_widths(v: f64) -> (i32, i32, i32) {
    let (neg, kpower, nsig) = scientific_parts(v);
    let left = kpower + 1;
    let sleft = neg + if left <= 0 { 1 } else { left };
    let right = nsig - left;
    let mut rgt = right.max(0);
    let digits = unsafe { crate::mainutils::format::format_get_R_print().digits };
    if digits == 0 {
        rgt = 0;
    }
    let w_f = sleft + rgt + i32::from(rgt != 0);
    // E Format has the form [S]X[.XXX]E+XX[X]
    let e = if left > 100 || left <= -99 { 2 } else { 1 };
    let d = nsig - 1;
    let w_e = neg + i32::from(d > 0) + d + 4 + e;
    (w_f, rgt, w_e)
}

pub(crate) fn needs_scientific(v: f64) -> bool {
    if !v.is_finite() || v == 0.0 {
        return false;
    }
    let scipen = unsafe { crate::mainutils::options::GetOptionScipen() };
    let (w_f, _, w_e) = real_format_widths(v);
    // Fixpoint whenever it needs no more space than E + scipen.
    w_f > w_e + scipen
}

pub(crate) fn is_finite_r_number(v: f64) -> bool {
    !R_IsNA(v) && !R_IsNaN(v) && v.is_finite()
}

pub(crate) fn format_real_value_for_vector(v: f64, force_decimal_for_whole: bool) -> String {
    if force_decimal_for_whole && is_finite_r_number(v) && v.fract() == 0.0 {
        format!("{v:.1}")
    } else {
        format_real_value(v)
    }
}

pub(crate) fn format_real_vector_values(x: Sexp<'_>, limit: R_xlen_t) -> Vec<String> {
    let values: Vec<_> = (0..x.clone().len().min(limit))
        .map(|i| x.clone().try_real_elt(i))
        .collect();
    let force_decimal_for_whole = values
        .iter()
        .filter_map(|value| value.as_ref().ok().copied())
        .any(|value| is_finite_r_number(value) && value.fract() != 0.0);

    values
        .into_iter()
        .map(|value| {
            value
                .map(|value| format_real_value_for_vector(value, force_decimal_for_whole))
                .unwrap_or_else(format_access_error)
        })
        .collect()
}

/// GNU `cat` uses `EncodeRealDrop0`: fixed-point trailing zeros are not kept.
pub(crate) fn format_cat_real(v: f64) -> String {
    trim_float(format_real_value(v))
}

pub(crate) fn trim_float(s: String) -> String {
    let (mut mantissa, exponent) = match s.find(['e', 'E']) {
        Some(idx) => (s[..idx].to_string(), s[idx..].to_string()),
        None => (s, String::new()),
    };
    if mantissa.contains('.') {
        while mantissa.ends_with('0') {
            mantissa.pop();
        }
        if mantissa.ends_with('.') {
            mantissa.pop();
        }
    }
    let exponent = if exponent.len() >= 2 {
        let mark = &exponent[..1];
        let rest = &exponent[1..];
        let (sign, digits) = if let Some(stripped) = rest.strip_prefix('+') {
            ("+", stripped)
        } else if let Some(stripped) = rest.strip_prefix('-') {
            ("-", stripped)
        } else {
            ("+", rest)
        };
        // C's %e keeps at least two exponent digits (e+09, e-05).
        let padded = if digits.len() < 2 {
            format!("0{digits}")
        } else {
            digits.to_string()
        };
        format!("{mark}{sign}{padded}")
    } else {
        exponent
    };
    format!("{mantissa}{exponent}")
}

/// Scientific rendering, GNU `EncodeReal0` with `d = nsig - 1` decimals.
/// Only reached when `needs_scientific` selected the E format.
pub(crate) fn format_r_default_real(v: f64) -> String {
    let (_, _, nsig) = scientific_parts(v);
    let decimals = (nsig - 1).max(0) as usize;
    trim_float(format!("{v:.decimals$e}"))
}

pub(crate) fn format_logical_value(v: i32) -> String {
    match v {
        0 => "FALSE".to_string(),
        1 => "TRUE".to_string(),
        _ => "NA".to_string(),
    }
}

pub(crate) fn format_complex_value(v: super::super::ffi::Rcomplex) -> String {
    if R_IsNA(v.r) || R_IsNA(v.i) {
        return "NA".to_string();
    }
    // EncodeReal0: x == 0.0 becomes +0, so cat/print of -0i is "+0i".
    let imag = if v.i == 0.0 { 0.0 } else { v.i };
    let real = format_cat_real(v.r);
    let imaginary = format_cat_real(imag.abs());
    if imag.is_sign_negative() {
        format!("{real}-{imaginary}i")
    } else {
        format!("{real}+{imaginary}i")
    }
}

pub(crate) fn format_access_error(err: impl std::fmt::Display) -> String {
    format!("<{err}>")
}

pub(crate) fn format_raw_element(x: Sexp<'_>, i: R_xlen_t) -> String {
    x.try_raw_elt(i)
        .map(format_raw_value)
        .unwrap_or_else(format_access_error)
}

pub(crate) fn format_integer_element(x: Sexp<'_>, i: R_xlen_t) -> String {
    x.try_integer_elt(i)
        .map(format_integer_value)
        .unwrap_or_else(format_access_error)
}

pub(crate) fn format_real_element(x: Sexp<'_>, i: R_xlen_t) -> String {
    x.try_real_elt(i)
        .map(format_real_value)
        .unwrap_or_else(format_access_error)
}

pub(crate) fn format_logical_element(x: Sexp<'_>, i: R_xlen_t) -> String {
    x.try_logical_elt(i)
        .map(format_logical_value)
        .unwrap_or_else(format_access_error)
}

pub(crate) fn format_complex_element(x: Sexp<'_>, i: R_xlen_t) -> String {
    x.try_complex_elt(i)
        .map(format_complex_value)
        .unwrap_or_else(format_access_error)
}

pub(crate) fn format_raw_value(v: u8) -> String {
    format!("{v:02x}")
}
