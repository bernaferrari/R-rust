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
    // factor printing: classed integer codes display as their level labels
    /// unquoted, with the Levels line trailing exactly as print.factor.
    if matches!(object, RObject::Integer(_)) && is_factor(heap, interner, value) {
        return format_factor(heap, interner, value);
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
    let elements: Vec<String> = (0..length)
        .map(|i| format_element(heap, interner, object, i))
        .collect();
    let width = elements.iter().map(|e| e.len()).max().unwrap_or(0);
    let mut out = String::from("[1]");
    for element in &elements {
        out.push(' ');
        out.push_str(&" ".repeat(width - element.len()));
        out.push_str(element);
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
            out.push_str(&" ".repeat(widths[j] - text.len()));
            out.push_str(&text);
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
            Some(RObject::String(values)) => Some(
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
