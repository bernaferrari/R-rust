//! GNU print formatting — the exact text shape `print`/`cat` produce.
//!
//! The conformance corpus is print-driven: cases prove semantics through
//! captured output, so parity is parity of this formatting. The rules
//! implemented here mirror R's `format()` machinery for the migrated types:
//! seven significant digits for reals, scientific notation outside the
//! `%g` exponent window, integral reals without a decimal point, element
//! separators and the `[1]` positional prefix.

use super::heap::Heap;
use super::id::ValueId;
use super::intern::SymbolInterner;
use super::object::{RObject, NA_INTEGER, NA_REAL_BITS};

/// Format a real scalar exactly as R's 7-significant-digit default.
/// NA is the dedicated bit pattern; NaN (a different payload) prints as
/// NaN exactly as GNU distinguishes them.
pub fn format_real(v: f64) -> String {
    if v.is_nan() {
        return if v.to_bits() == NA_REAL_BITS {
            "NA".to_string()
        } else {
            "NaN".to_string()
        };
    }
    if v.is_infinite() {
        return if v > 0.0 { "Inf".to_string() } else { "-Inf".to_string() };
    }
    if v == 0.0 {
        return "0".to_string();
    }
    let abs = v.abs();
    let exponent = abs.log10().floor() as i32;
    // The %g window: fixed notation inside [-4, precision), scientific
    // outside — R's digits=7 default exactly.
    if exponent >= 7 || exponent < -4 {
        let mantissa = v / 10f64.powi(exponent);
        let mantissa_text = trim_fixed(mantissa, 6);
        let sign = if exponent < 0 { "-" } else { "+" };
        format!("{}e{}{:02}", mantissa_text, sign, exponent.abs())
    } else {
        let decimals = (6 - exponent).max(0) as usize;
        trim_fixed(v, decimals)
    }
}

/// Fixed-notation formatting with trailing zeros and a trailing decimal
/// point trimmed — GNU drops both: 2.0 prints as "2", 0.10 as "0.1".
fn trim_fixed(v: f64, decimals: usize) -> String {
    let text = format!("{:.*}", decimals, v);
    if text.contains('.') {
        let trimmed = text.trim_end_matches('0').trim_end_matches('.');
        trimmed.to_string()
    } else {
        text
    }
}

/// One element's display text — the per-type shapes GNU's print uses.
fn format_element(heap: &Heap, interner: &SymbolInterner, value: &RObject, index: usize) -> String {
    match value {
        RObject::Real(values) => format_real(values[index]),
        RObject::Integer(values) | RObject::Logical(values) => {
            let v = values[index];
            if v == NA_INTEGER {
                "NA".to_string()
            } else if matches!(value, RObject::Logical(_)) {
                if v != 0 {
                    "TRUE".to_string()
                } else {
                    "FALSE".to_string()
                }
            } else {
                v.to_string()
            }
        }
        RObject::String(values) => {
            let text = interner.string_text(values[index]);
            if text == "\0NA" {
                "NA".to_string()
            } else {
                format!("\"{}\"", text)
            }
        }
        _ => "?".to_string(),
    }
}

/// Format a vector as print's output text — `[1]` prefix, space-separated
/// elements, one line. Multi-line width wrapping is outside the slice;
/// conformance vectors are short enough that the single-line shape covers
/// them.
pub fn format_value(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> String {
    let object = match heap.object(value) {
        Some(o) => o,
        None => return "NULL".to_string(),
    };
    let length = match object {
        RObject::Real(v) => v.len(),
        RObject::Integer(v) => v.len(),
        RObject::Logical(v) => v.len(),
        RObject::String(v) => v.len(),
        RObject::Nil => return "NULL".to_string(),
        RObject::List(items) => {
            // List printing in GNU is a recursive [[i]] layout — the slice
            // prints the flat element count shape for scalarness checks.
            let mut out = String::new();
            for (i, item) in items.iter().enumerate() {
                out.push_str(&format!("[[{}]]\n", i + 1));
                out.push_str(&format_value(heap, interner, *item));
                out.push('\n');
            }
            return out.trim_end().to_string();
        }
        RObject::Closure { .. } => return "<function>".to_string(),
        RObject::Environment { .. } => return "<environment>".to_string(),
        _ => return "<object>".to_string(),
    };
    if length == 0 {
        return match object {
            RObject::Real(_) => "numeric(0)".to_string(),
            RObject::Integer(_) => "integer(0)".to_string(),
            RObject::Logical(_) => "logical(0)".to_string(),
            RObject::String(_) => "character(0)".to_string(),
            _ => "NULL".to_string(),
        };
    }
    let mut out = String::from("[1]");
    for i in 0..length {
        out.push(' ');
        out.push_str(&format_element(heap, interner, object, i));
    }
    out
}

/// The cat-coercion of a scalar: unquoted, NA drops exactly as GNU (NA
/// contributes nothing to output).
pub fn format_cat_element(heap: &Heap, interner: &SymbolInterner, value: &RObject, index: usize) -> String {
    match value {
        RObject::Real(values) => format_real(values[index]),
        RObject::Integer(values) | RObject::Logical(values) => {
            let v = values[index];
            if v == NA_INTEGER {
                "NA".to_string()
            } else if matches!(value, RObject::Logical(_)) {
                if v != 0 {
                    "TRUE".to_string()
                } else {
                    "FALSE".to_string()
                }
            } else {
                v.to_string()
            }
        }
        RObject::String(values) => {
            let text = interner.string_text(values[index]);
            if text == "\0NA" {
                "NA".to_string()
            } else {
                text.to_string()
            }
        }
        _ => "?".to_string(),
    }
}
