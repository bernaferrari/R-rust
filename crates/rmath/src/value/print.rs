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

/// The decimals a fixed-notation element actually uses after trailing-zero
/// trimming — the input to GNU's uniform-decimal computation.
/// One complex element's display text — real part, explicit imaginary
/// sign, magnitude, trailing i exactly as GNU's complex printing; an NA
/// in either part renders the whole element NA.
pub fn format_complex(z: crate::value::object::RComplex) -> String {
    if z.r.to_bits() == crate::value::object::NA_REAL_BITS
        || z.i.to_bits() == crate::value::object::NA_REAL_BITS
    {
        return "NA".to_string();
    }
    let sign = if z.i.is_sign_negative() && !z.i.is_nan() { "-" } else { "+" };
    format!("{}{}{}{}", format_real(z.r), sign, format_real(z.i.abs()), "i")
}

fn decimals_used(v: f64) -> Option<usize> {
    if v.is_nan() || v.is_infinite() || v == 0.0 {
        return Some(0);
    }
    let abs = v.abs();
    let exponent = abs.log10().floor() as i32;
    if exponent >= 7 || exponent < -4 {
        return None;
    }
    let decimals = (6 - exponent).max(0) as usize;
    let text = format!("{:.*}", decimals, v);
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    Some(trimmed.split('.').nth(1).map(|s| s.len()).unwrap_or(0))
}

/// Fixed-notation formatting at an exact decimal count — no trimming,
/// since uniformity is the point.
fn fixed_exact(v: f64, decimals: usize) -> String {
    format!("{:.*}", decimals, v)
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
        RObject::Complex(values) => format_complex(values[index]),
        RObject::Raw(values) => format!("{:02x}", values[index]),
        RObject::String(values) => {
            let text = interner.string_text(values[index]);
            if text == "\0NA" {
                "NA".to_string()
            } else {
                // GNU escapes embedded quotes and backslashes inside the
                /// surrounding quotes exactly as print.c does.
                let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
                format!("\"{}\"", escaped)
            }
        }
        _ => "?".to_string(),
    }
}

/// Format a vector as print's output text — `[1]` prefix, space-separated
/// elements, one line. Multi-line width wrapping is outside the slice;
/// conformance vectors are short enough that the single-line shape covers
/// them.
/// table class membership exactly as the factor helper.
fn is_table(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> bool {
    match super::primitives::get_attrib(heap, interner, value, "class") {
        Some(class_id) => super::primitives::string_vec_pub(heap, interner, class_id)
            .iter()
            .any(|c| c == "table"),
        None => false,
    }
}

pub fn format_value(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> String {
    let object = match heap.object(value) {
        Some(o) => o,
        None => return "NULL".to_string(),
    };
    // Language objects and bare symbols print deparsed — the source
    /// reconstruction exactly as GNU's print.language, no [1] prefix.
    if matches!(object, RObject::Language { .. } | RObject::Symbol { .. }) {
        return deparse(heap, interner, value);
    }
    // Expression vectors print inside the expression() wrapper with each
    /// element deparsed comma-separated exactly as GNU's print.expression.
    if let RObject::Expression(items) = object {
        if items.is_empty() {
            return "expression()".to_string();
        }
        let parts: Vec<String> = items
            .iter()
            .map(|item| deparse(heap, interner, *item))
            .collect();
        return format!("expression({})", parts.join(", "));
    }
    // factor printing: classed integer codes display as their level labels
    /// unquoted, with the Levels line trailing exactly as print.factor.
    if matches!(object, RObject::Integer(_)) && is_factor(heap, interner, value) {
        return format_factor(heap, interner, value);
    }
    // table printing: the category header row above the count row, columns
    /// right-aligned to the wider of the two exactly as print.table.
    if matches!(object, RObject::Integer(_)) && is_table(heap, interner, value) {
        let counts: Vec<i32> = match object {
            RObject::Integer(vals) => vals.clone(),
            _ => Vec::new(),
        };
        let categories: Vec<String> = match super::primitives::get_attrib(heap, interner, value, "dimnames")
            .and_then(|dn| match heap.object(dn) {
                Some(RObject::List(items)) => items.first().copied(),
                _ => None,
            })
            .map(|cats| super::primitives::string_vec_pub(heap, interner, cats))
        {
            Some(c) if !c.is_empty() => c,
            _ => match super::primitives::get_attrib(heap, interner, value, "names") {
                Some(n) => super::primitives::string_vec_pub(heap, interner, n),
                None => Vec::new(),
            },
        };
        let mut header: Vec<String> = Vec::new();
        let mut body: Vec<String> = Vec::new();
        for (index, count) in counts.iter().enumerate() {
            let cat = categories.get(index).cloned().unwrap_or_default();
            let num = count.to_string();
            let width = cat.len().max(num.len());
            header.push(format!("{:>width$}", cat, width = width));
            body.push(format!("{:>width$}", num, width = width));
        }
        // GNU's print.table leads with a blank line exactly as the golden
        // pins — the margin formatting emits it before the header row.
        return format!("\n{}\n{}", header.join(" "), body.join(" "));
    }
    // POSIXct printing: classed epoch seconds display as quoted datetimes
    /// carrying the tzone suffix exactly as print.POSIXct.
    if matches!(object, RObject::Real(_) | RObject::Integer(_))
        && super::primitives::is_posixct_classed(heap, interner, value)
    {
        let secs: Vec<f64> = match object {
            RObject::Real(vals) => vals.clone(),
            RObject::Integer(vals) => vals
                .iter()
                .map(|&v| {
                    if v == crate::value::object::NA_INTEGER {
                        f64::from_bits(crate::value::object::NA_REAL_BITS)
                    } else {
                        v as f64
                    }
                })
                .collect(),
            _ => Vec::new(),
        };
        if secs.is_empty() {
            return "POSIXct of length 0".to_string();
        }
        let tz = super::primitives::get_attrib(heap, interner, value, "tzone")
            .and_then(|id| match heap.object(id) {
                Some(RObject::String(vals)) if !vals.is_empty() => {
                    Some(interner.string_text(vals[0]).to_string())
                }
                _ => None,
            })
            .unwrap_or_else(|| "UTC".to_string());
        // GNU drops the time component only when every finite element is
        /// exactly midnight — a single non-midnight renders all times
        /// exactly as format.POSIXct's collective omission rule.
        let all_midnight = secs
            .iter()
            .filter(|s| s.is_finite())
            .all(|&s| (s as i64).rem_euclid(86400) == 0);
        let elements: Vec<String> = secs
            .iter()
            .map(|&s| {
                if s.is_nan() {
                    "NA".to_string()
                } else {
                    let full = super::civil::format_iso_datetime(s as i64);
                    let rendered = if all_midnight && full.ends_with(" 00:00:00") {
                        full[..10].to_string()
                    } else {
                        full
                    };
                    format!("\"{} {}\"", rendered, tz)
                }
            })
            .collect();
        let width = elements.iter().map(|e| e.len()).max().unwrap_or(0);
        let mut out = String::from("[1]");
        for element in &elements {
            out.push(' ');
            out.push_str(&" ".repeat(width.saturating_sub(element.len())));
            out.push_str(element);
        }
        return out;
    }
    // Date printing: classed day counts display as quoted ISO strings
    /// exactly as print.Date — a character-vector rendering of the civil
    /// dates.
    if matches!(object, RObject::Real(_) | RObject::Integer(_))
        && super::primitives::is_date_classed(heap, interner, value)
    {
        let days: Vec<f64> = match object {
            RObject::Real(vals) => vals.clone(),
            RObject::Integer(vals) => vals
                .iter()
                .map(|&v| {
                    if v == crate::value::object::NA_INTEGER {
                        f64::from_bits(crate::value::object::NA_REAL_BITS)
                    } else {
                        v as f64
                    }
                })
                .collect(),
            _ => Vec::new(),
        };
        let elements: Vec<String> = days
            .iter()
            .map(|&d| {
                if d.is_nan() {
                    "NA".to_string()
                } else {
                    format!("\"{}\"", super::civil::format_iso(d as i64))
                }
            })
            .collect();
        if elements.is_empty() {
            return "Date of length 0".to_string();
        }
        let width = elements.iter().map(|e| e.len()).max().unwrap_or(0);
        let mut out = String::from("[1]");
        for element in &elements {
            out.push(' ');
            out.push_str(&" ".repeat(width.saturating_sub(element.len())));
            out.push_str(element);
        }
        return out;
    }
    // difftime printing: the unclassed numeric prints inside the "Time
    /// difference of N units" phrasing exactly as print.difftime.
    if matches!(object, RObject::Real(_) | RObject::Integer(_)) && is_difftime(heap, interner, value) {
        let nums: Vec<f64> = match object {
            RObject::Real(vals) => vals.clone(),
            RObject::Integer(vals) => vals.iter().map(|&v| v as f64).collect(),
            _ => Vec::new(),
        };
        let first = nums.first().copied().unwrap_or(0.0);
        let unit = super::primitives::get_attrib(heap, interner, value, "units")
            .and_then(|id| match heap.object(id) {
                Some(RObject::String(vals)) if !vals.is_empty() => {
                    Some(interner.string_text(vals[0]).to_string())
                }
                _ => None,
            })
            .unwrap_or_else(|| "days".to_string());
        let amount = if first.fract() == 0.0 && !first.is_nan() {
            format!("{}", first as i64)
        } else {
            format_real(first)
        };
        return format!("Time difference of {amount} {unit}");
    }
    // data.frame printing: classed lists lay out as tables — column names
    /// right-justified over per-column widths, row names on the left,
    /// character cells unquoted exactly as print.data.frame.
    if matches!(object, RObject::List(_)) && is_data_frame(heap, interner, value) {
        return format_data_frame(heap, interner, value);
    }
    let length = match object {
        RObject::Real(v) => v.len(),
        RObject::Integer(v) => v.len(),
        RObject::Logical(v) => v.len(),
        RObject::String(v) => v.len(),
        RObject::Complex(v) => v.len(),
        RObject::Raw(v) => v.len(),
        RObject::Nil => return "NULL".to_string(),
        RObject::List(items) => {
            // List printing in GNU is a recursive [[i]] layout — the slice
            // prints the flat element count shape for scalarness checks.
            // Named elements print under `$name` headers; unnamed ones keep
            /// the positional [[i]] form — GNU's mixed-header rule.
            let names = super::primitives::get_attrib(heap, interner, value, "names");
            let names_vec: Option<Vec<String>> = names.and_then(|id| match heap.object(id) {
                Some(RObject::String(values)) => Some(
                    values
                        .iter()
                        .map(|v| interner.string_text(*v).to_string())
                        .collect(),
                ),
                _ => None,
            });
            let mut out = String::new();
            for (i, item) in items.iter().enumerate() {
                let name = names_vec
                    .as_ref()
                    .and_then(|v| v.get(i))
                    .filter(|n| !n.is_empty() && *n != "\0NA");
                match name {
                    Some(n) => out.push_str(&format!("${n}\n")),
                    None => out.push_str(&format!("[[{}]]\n", i + 1)),
                }
                out.push_str(&format_value(heap, interner, *item));
                // GNU separates list elements with a blank line.
                out.push_str("\n\n");
            }
            return out.trim_end().to_string();
        }
        RObject::Closure { .. } => return "<function>".to_string(),
        RObject::Environment { .. } => {
            // Named environments print their name exactly as GNU's
            /// <environment: R_GlobalEnv> — the name rides the attribute
            /// chain in the new world.
            let name = super::primitives::get_attrib(heap, interner, value, "name")
                .and_then(|id| match heap.object(id) {
                    Some(RObject::String(values)) if !values.is_empty() => {
                        Some(interner.string_text(values[0]).to_string())
                    }
                    _ => None,
                });
            return match name {
                Some(n) if !n.is_empty() => format!("<environment: {n}>"),
                _ => "<environment>".to_string(),
            };
        }
        _ => return "<object>".to_string(),
    };
    if length == 0 {
        // GNU prefixes "named" when the empty vector carries a names
        // attribute exactly as print.default's empty-shape rendering.
        let named = super::primitives::get_attrib(heap, interner, value, "names").is_some();
        let prefix = if named { "named " } else { "" };
        return match object {
            RObject::Real(_) => format!("{prefix}numeric(0)"),
            RObject::Integer(_) => format!("{prefix}integer(0)"),
            RObject::Logical(_) => "logical(0)".to_string(),
            RObject::String(_) => "character(0)".to_string(),
            RObject::Complex(_) => "complex(0)".to_string(),
            RObject::Raw(_) => "raw(0)".to_string(),
            _ => "NULL".to_string(),
        };
    }
    // Matrix layout: a two-length integer dim attribute lays the vector out
    /// column-major under [,j] column headers and [i,] row labels exactly as
    /// GNU's print.matrix does.
    if let Some(dim) = dim_attribute(heap, interner, value) {
        if dim.len() == 2 && dim[0] > 0 && dim[1] > 0 && dim[0] * dim[1] == length as i32 {
            let (row_names, col_names) = dimnames_of(heap, interner, value);
            return format_matrix_inner(
                heap,
                interner,
                object,
                dim[0] as usize,
                dim[1] as usize,
                row_names,
                col_names,
            );
        }
    }
    // GNU right-justifies every element to the widest element text so the
    /// columnar reading holds — NA padding is the visible consequence.
    // Real vectors share one decimal count across all elements — GNU's
    /// format.c uniformity rule: the maximum decimals any fixed-window
    /// element needs applies to all, so 100.5 drags 200 into 200.0.
    let uniform_decimals: Option<usize> = match object {
        RObject::Real(values) => {
            let mut max_dec = 0usize;
            let mut all_fixed = true;
            for v in values.iter() {
                if v.is_nan() || v.is_infinite() {
                    continue;
                }
                match decimals_used(*v) {
                    Some(d) => max_dec = max_dec.max(d),
                    None => {
                        all_fixed = false;
                        break;
                    }
                }
            }
            if all_fixed && max_dec > 0 {
                Some(max_dec)
            } else {
                None
            }
        }
        _ => None,
    };
    let elements: Vec<String> = (0..length)
        .map(|i| {
            if let (Some(dec), RObject::Real(values)) = (uniform_decimals, object) {
                let v = values[i];
                if v.is_nan() || v.is_infinite() {
                    format_real(v)
                } else {
                    fixed_exact(v, dec)
                }
            } else {
                format_element(heap, interner, object, i)
            }
        })
        .collect();
    let width = if matches!(object, RObject::String(_)) {
        // GNU does not column-align character vectors — quoting delimits.
        0
    } else {
        elements.iter().map(|e| e.len()).max().unwrap_or(0)
    };
    // GNU named-vector layout: the names print as a header row over the
    /// values, each column right-justified to the wider of name and value,
    /// with no [1] tags — the two-row reading is the tell.
    let names_id = super::primitives::get_attrib(heap, interner, value, "names");
    let names_vec: Option<Vec<String>> = names_id.and_then(|id| match heap.object(id) {
        Some(RObject::String(values)) if values.len() == length => Some(
            values
                .iter()
                .map(|v| interner.string_text(*v).to_string())
                .collect(),
        ),
        _ => None,
    });
    let mut out = String::new();
    match names_vec {
        Some(names) => {
            let col_widths: Vec<usize> = elements
                .iter()
                .zip(names.iter())
                .map(|(e, n)| e.len().max(n.len()))
                .collect();
            for (i, name) in names.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                out.push_str(&" ".repeat(col_widths[i] - name.len()));
                out.push_str(name);
            }
            out.push('\n');
            for (i, element) in elements.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                out.push_str(&" ".repeat(col_widths[i] - element.len()));
                out.push_str(element);
            }
        }
        None => {
            out.push_str("[1]");
            for element in &elements {
                out.push(' ');
                out.push_str(&" ".repeat(width.saturating_sub(element.len())));
                out.push_str(element);
            }
        }
    }
    // GNU attribute blocks: every attribute the layout itself did not
    /// consume prints as attr(,"name") followed by its own formatted body.
    out.push_str(&format_attribute_blocks(heap, interner, value));
    out
}

/// The trailing attr(,) blocks of a printed object — the attribute chain in
/// order, skipping the attributes the value layout consumes structurally
/// (names/dim/dimnames, and class/levels on factors).
fn format_attribute_blocks(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> String {
    let chain = heap.get(value).map(|slot| slot.attrib).unwrap_or(ValueId::NIL);
    if chain == ValueId::NIL {
        return String::new();
    }
    let factor = is_factor(heap, interner, value);
    let mut out = String::new();
    let mut cursor = chain;
    loop {
        let (car, cdr, tag) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
            _ => break,
        };
        let name = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
            _ => String::new(),
        };
        let consumed = matches!(name.as_str(), "names" | "dim" | "dimnames")
            || (factor && matches!(name.as_str(), "class" | "levels"));
        if !consumed && !name.is_empty() {
            out.push_str(&format!("\nattr(,\"{name}\")\n"));
            out.push_str(&format_value(heap, interner, car));
        }
        if cdr == ValueId::NIL {
            break;
        }
        cursor = cdr;
    }
    out
}

/// The dim attribute of an object as integer extents, when present.
fn dim_attribute(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> Option<Vec<i32>> {
    let dim_id = super::primitives::get_attrib(heap, interner, value, "dim")?;
    match heap.object(dim_id) {
        Some(RObject::Integer(values)) => Some(values.clone()),
        Some(RObject::Real(values)) => Some(values.iter().map(|v| *v as i32).collect()),
        _ => None,
    }
}

/// Column-major matrix print: header line indented by the row-label width,
/// every column right-justified to the wider of its widest element and its
/// header — GNU's print.matrix layout for the atomic types. Dimnames
/// replace the positional `[,j]`/`[i,]` labels with bare names.
fn format_matrix(
    heap: &Heap,
    interner: &SymbolInterner,
    object: &RObject,
    nrow: usize,
    ncol: usize,
) -> String {
    format_matrix_inner(heap, interner, object, nrow, ncol, None, None)
}

#[allow(clippy::too_many_arguments)]
fn format_matrix_inner(
    heap: &Heap,
    interner: &SymbolInterner,
    object: &RObject,
    nrow: usize,
    ncol: usize,
    row_names: Option<Vec<String>>,
    col_names: Option<Vec<String>>,
) -> String {
    let element = |i: usize, j: usize| format_element(heap, interner, object, i + j * nrow);
    // Character matrix columns left-align exactly as GNU's printMatrix —
    /// numeric columns right-align.
    let left_align = matches!(object, RObject::String(_));
    let headers: Vec<String> = match &col_names {
        Some(names) => names.clone(),
        None => (0..ncol).map(|j| format!("[,{}]", j + 1)).collect(),
    };
    let widths: Vec<usize> = (0..ncol)
        .map(|j| {
            let mut width = headers[j].len();
            for i in 0..nrow {
                width = width.max(element(i, j).len());
            }
            width
        })
        .collect();
    let row_labels: Vec<String> = match &row_names {
        Some(names) => names.clone(),
        None => (0..nrow).map(|i| format!("[{},]", i + 1)).collect(),
    };
    let label_width = row_labels.iter().map(|l| l.len()).max().unwrap_or(0);
    // Positional headers carry a leading space column the bare dimnames
    /// headers do not — GNU's alignment detail.
    let positional = row_names.is_none() && col_names.is_none();
    let mut out = String::new();
    out.push_str(&" ".repeat(label_width + 1));
    for (j, header) in headers.iter().enumerate() {
        if j > 0 {
            out.push(' ');
        }
        out.push_str(&" ".repeat(widths[j] - header.len()));
        out.push_str(header);
    }
    let _ = positional;
    for i in 0..nrow {
        out.push('\n');
        out.push_str(&row_labels[i]);
        out.push_str(&" ".repeat(label_width - row_labels[i].len() + 1));
        for j in 0..ncol {
            if j > 0 {
                out.push(' ');
            }
            let text = element(i, j);
            if left_align {
                out.push_str(&text);
                out.push_str(&" ".repeat(widths[j] - text.len()));
            } else {
                out.push_str(&" ".repeat(widths[j] - text.len()));
                out.push_str(&text);
            }
        }
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

/// Extract dimnames as unquoted row/column name vectors when present.
fn dimnames_of(
    heap: &Heap,
    interner: &SymbolInterner,
    value: ValueId,
) -> (Option<Vec<String>>, Option<Vec<String>>) {
    let id = match super::primitives::get_attrib(heap, interner, value, "dimnames") {
        Some(v) => v,
        None => return (None, None),
    };
    let items = match heap.object(id) {
        Some(RObject::List(items)) => items.clone(),
        _ => return (None, None),
    };
    let extract = |idx: usize| -> Option<Vec<String>> {
        let item = items.get(idx)?;
        match heap.object(*item) {
            // Empty vectors ride as absent exactly as GNU's NULL dimnames
            /// entries — positional labels take their place.
            Some(RObject::String(values)) if !values.is_empty() => Some(
                values
                    .iter()
                    .map(|v| interner.string_text(*v).to_string())
                    .collect(),
            ),
            _ => None,
        }
    };
    (extract(0), extract(1))
}

/// The class attribute holds data.frame — the object shape test.
fn is_data_frame(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> bool {
    match super::primitives::get_attrib(heap, interner, value, "class") {
        Some(id) => match heap.object(id) {
            Some(RObject::String(values)) => values
                .iter()
                .any(|v| interner.string_text(*v) == "data.frame"),
            _ => false,
        },
        None => false,
    }
}

/// Unquoted cell text for one column element — print.data.frame's shape.
fn format_cell(heap: &Heap, interner: &SymbolInterner, column: &RObject, index: usize) -> String {
    match column {
        RObject::String(values) => {
            let text = interner.string_text(values[index]);
            if text == "\0NA" {
                "NA".to_string()
            } else {
                text.to_string()
            }
        }
        _ => format_element(heap, interner, column, index),
    }
}

/// Tabular layout: headers right-justified over per-column widths, row
/// names leftmost — column-major cell extraction over the list shape.
fn format_data_frame(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> String {
    let items = match heap.object(value) {
        Some(RObject::List(items)) => items.clone(),
        _ => return "<data.frame>".to_string(),
    };
    let col_names: Vec<String> = match super::primitives::get_attrib(heap, interner, value, "names") {
        Some(id) => match heap.object(id) {
            Some(RObject::String(values)) => values
                .iter()
                .map(|v| interner.string_text(*v).to_string())
                .collect(),
            _ => Vec::new(),
        },
        None => Vec::new(),
    };
    let row_names: Vec<String> = match super::primitives::get_attrib(heap, interner, value, "row.names") {
        Some(id) => match heap.object(id) {
            Some(RObject::String(values)) => values
                .iter()
                .map(|v| interner.string_text(*v).to_string())
                .collect(),
            Some(RObject::Integer(values)) => values.iter().map(|v| v.to_string()).collect(),
            _ => Vec::new(),
        },
        None => Vec::new(),
    };
    let ncol = items.len();
    let nrow = items
        .first()
        .and_then(|c| match heap.object(*c) {
            Some(RObject::Real(v)) => Some(v.len()),
            Some(RObject::Integer(v)) => Some(v.len()),
            Some(RObject::Logical(v)) => Some(v.len()),
            Some(RObject::String(v)) => Some(v.len()),
            _ => None,
        })
        .unwrap_or(0);
    // Per-column widths: widest of header and every cell.
    let mut widths: Vec<usize> = Vec::new();
    for (j, col) in items.iter().enumerate() {
        let mut width = col_names.get(j).map(|n| n.len()).unwrap_or(0);
        if let Some(column) = heap.object(*col) {
            for i in 0..nrow {
                width = width.max(format_cell(heap, interner, column, i).len());
            }
        }
        widths.push(width);
    }
    let label_width = row_names.iter().map(|n| n.len()).max().unwrap_or(0);
    let mut out = String::new();
    out.push_str(&" ".repeat(label_width + 1));
    for (j, name) in col_names.iter().enumerate() {
        if j > 0 {
            out.push(' ');
        }
        out.push_str(&" ".repeat(widths[j] - name.len()));
        out.push_str(name);
    }
    for i in 0..nrow {
        out.push('\n');
        let label = row_names.get(i).cloned().unwrap_or_else(|| (i + 1).to_string());
        out.push_str(&label);
        out.push_str(&" ".repeat(label_width - label.len() + 1));
        for (j, col) in items.iter().enumerate() {
            if j > 0 {
                out.push(' ');
            }
            let cell = match heap.object(*col) {
                Some(column) => format_cell(heap, interner, column, i),
                None => "?".to_string(),
            };
            out.push_str(&" ".repeat(widths[j] - cell.len()));
            out.push_str(&cell);
        }
    }
    out
}

fn is_difftime(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> bool {
    super::primitives::get_attrib(heap, interner, value, "class")
        .and_then(|id| match heap.object(id) {
            Some(RObject::String(vals)) => Some(vals.iter().any(|v| interner.string_text(*v) == "difftime")),
            _ => None,
        })
        .unwrap_or(false)
}

fn is_factor(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> bool {
    match super::primitives::get_attrib(heap, interner, value, "class") {
        Some(id) => match heap.object(id) {
            Some(RObject::String(values)) => values
                .iter()
                .any(|v| interner.string_text(*v) == "factor"),
            _ => false,
        },
        None => false,
    }
}

/// `[1] b a b c` over unquoted labels, then `Levels: a b c` — NA codes
/// print as NA without a label exactly as GNU.
fn format_factor(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> String {
    let codes = match heap.object(value) {
        Some(RObject::Integer(values)) => values.clone(),
        _ => return "<factor>".to_string(),
    };
    let levels: Vec<String> = match super::primitives::get_attrib(heap, interner, value, "levels") {
        Some(id) => match heap.object(id) {
            Some(RObject::String(values)) => values
                .iter()
                .map(|v| interner.string_text(*v).to_string())
                .collect(),
            _ => Vec::new(),
        },
        None => Vec::new(),
    };
    let labels: Vec<String> = codes
        .iter()
        .map(|c| {
            if *c == crate::value::object::NA_INTEGER {
                "NA".to_string()
            } else {
                levels
                    .get((*c - 1) as usize)
                    .cloned()
                    .unwrap_or_else(|| "NA".to_string())
            }
        })
        .collect();
    let width = labels.iter().map(|l| l.len()).max().unwrap_or(0);
    let mut out = String::from("[1]");
    for label in &labels {
        out.push(' ');
        out.push_str(&" ".repeat(width - label.len()));
        out.push_str(label);
    }
    out.push_str("\nLevels:");
    for level in &levels {
        out.push(' ');
        out.push_str(level);
    }
    out
}

/// Deparse a language object into R source text exactly as GNU's
/// print.language — infix operators render infix, calls parenthesize
/// comma-space-separated arguments, missing arguments print empty, and
/// atomic literals carry their literal form.
pub fn deparse(heap: &Heap, interner: &SymbolInterner, value: ValueId) -> String {
    match heap.object(value) {
        Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
        Some(RObject::Language { car, cdr, .. }) => {
            let op_text = match heap.object(*car) {
                Some(RObject::Symbol { name, .. }) => interner.symbol_name(*name).to_string(),
                _ => return deparse(heap, interner, *car),
            };
            // Collect the argument expressions in order.
            let mut items: Vec<ValueId> = Vec::new();
            let mut item_tags: Vec<Option<String>> = Vec::new();
            let mut cursor = *cdr;
            for _ in 0..100_000 {
                if cursor == ValueId::NIL {
                    break;
                }
                match heap.object(cursor) {
                    Some(RObject::Pairlist { car, cdr, tag, .. }) => {
                        items.push(*car);
                        let tag_text = match heap.object(*tag) {
                            Some(RObject::Symbol { name, .. }) => {
                                let text = interner.symbol_name(*name).to_string();
                                if text.is_empty() { None } else { Some(text) }
                            }
                            _ => None,
                        };
                        item_tags.push(tag_text);
                        cursor = *cdr;
                    }
                    _ => break,
                }
            }
            const INFIX: &[&str] = &[
                "+", "-", "*", "/", "^", "%%", "<", ">", "<=", ">=", "==", "!=", "|", "&", "~", "=", "<-", ":=", "%in%", "|>", "||", "&&",
            ];
            if INFIX.contains(&op_text.as_str()) {
                if items.len() == 2 {
                    let left = deparse(heap, interner, items[0]);
                    let right = deparse(heap, interner, items[1]);
                    return format!("{} {} {}", left, op_text, right);
                }
                if items.len() == 1 {
                    return format!("{} {}", op_text, deparse(heap, interner, items[0]));
                }
            }
            if op_text == "(" {
                return format!("({})", items.first().map(|v| deparse(heap, interner, *v)).unwrap_or_default());
            }
            if op_text == "{" {
                let inner: Vec<String> = items.iter().map(|v| deparse(heap, interner, *v)).collect();
                return format!("{{{}}}", inner.join("; "));
            }
            let rendered: Vec<String> = items
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    // Tagged arguments render name = value exactly as GNU's
                    /// deparse of a call object.
                    let value_text = if *v == crate::value::id::ValueId::MISSING {
                        String::new()
                    } else {
                        deparse(heap, interner, *v)
                    };
                    match item_tags.get(i).cloned().flatten() {
                        Some(tag) => format!("{tag} = {value_text}"),
                        None => value_text,
                    }
                })
                .collect();
            format!("{}({})", op_text, rendered.join(", "))
        }
        Some(RObject::Real(vals)) if vals.len() == 1 => format_real(vals[0]),
        Some(RObject::Integer(vals)) if vals.len() == 1 => {
            if vals[0] == crate::value::object::NA_INTEGER {
                "NA_integer_".to_string()
            } else {
                format!("{}L", vals[0])
            }
        }
        Some(RObject::Logical(vals)) if vals.len() == 1 => {
            if vals[0] == crate::value::object::NA_INTEGER {
                "NA".to_string()
            } else if vals[0] != 0 {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        }
        Some(RObject::String(vals)) if vals.len() == 1 => {
            let text = interner.string_text(vals[0]);
            format!("\"{}\"", text)
        }
        Some(RObject::Nil) => "NULL".to_string(),
        // Multi-element atomic vectors deparse to their c() construction
        /// exactly as GNU — each element rendering through the scalar forms.
        Some(RObject::Real(vals)) if !vals.is_empty() => {
            let parts: Vec<String> = vals.iter().map(|v| format_real(*v)).collect();
            format!("c({})", parts.join(", "))
        }
        Some(RObject::Integer(vals)) if !vals.is_empty() => {
            let parts: Vec<String> = vals
                .iter()
                .map(|&v| {
                    if v == crate::value::object::NA_INTEGER {
                        "NA_integer_".to_string()
                    } else {
                        format!("{}L", v)
                    }
                })
                .collect();
            format!("c({})", parts.join(", "))
        }
        Some(RObject::Logical(vals)) if !vals.is_empty() => {
            let parts: Vec<String> = vals
                .iter()
                .map(|&v| {
                    if v == crate::value::object::NA_INTEGER {
                        "NA".to_string()
                    } else if v != 0 {
                        "TRUE".to_string()
                    } else {
                        "FALSE".to_string()
                    }
                })
                .collect();
            format!("c({})", parts.join(", "))
        }
        Some(RObject::String(vals)) if !vals.is_empty() => {
            let parts: Vec<String> = vals
                .iter()
                .map(|v| format!("\"{}\"", interner.string_text(*v)))
                .collect();
            format!("c({})", parts.join(", "))
        }
        _ => "?".to_string(),
    }
}
