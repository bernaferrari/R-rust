//! R output capture for embedding.
//!
//! Captures Rprintf, REprintf, and other R output functions
//! so they can be returned to the caller instead of printing
//! to stdout/stderr.

use super::ffi::{NA_INTEGER, R_IsNA, R_IsNaN, R_xlen_t, SEXP, SEXPTYPE};
use super::globals::R_NaString;
use super::object::Sexp;

/// Captured R output.
#[derive(Debug, Clone, Default)]
pub struct RCapturedOutput {
    pub stdout: String,
    pub stderr: String,
}

/// Start capturing R output.
pub fn start_capture() {
    super::instance::with_required_current_instance(|inst| {
        let outer = (inst.capture_stdout.take(), inst.capture_stderr.take());
        if outer.0.is_some() || outer.1.is_some() {
            inst.capture_stack.push(outer);
        }
        inst.capture_stdout = Some(String::new());
        inst.capture_stderr = Some(String::new());
    });
}

/// Stop capturing and return the captured output.
pub fn stop_capture() -> RCapturedOutput {
    super::instance::with_required_current_instance(|inst| {
        let stdout = inst.capture_stdout.take().unwrap_or_default();
        let stderr = inst.capture_stderr.take().unwrap_or_default();
        if let Some((outer_stdout, outer_stderr)) = inst.capture_stack.pop() {
            inst.capture_stdout = outer_stdout;
            inst.capture_stderr = outer_stderr;
        }
        RCapturedOutput { stdout, stderr }
    })
}

/// Check if output capture is active.
pub fn is_capturing() -> bool {
    super::instance::with_current_instance(|inst| {
        inst.capture_stdout.is_some() || inst.capture_stderr.is_some()
    })
    .unwrap_or(false)
}

/// Append to captured stdout. Called by the Rprintf hook.
pub fn capture_stdout(msg: &str) {
    if is_capturing() {
        super::instance::with_current_instance(|inst| {
            if let Some(s) = inst.capture_stdout.as_mut() {
                s.push_str(msg);
            }
        });
    }
}

/// Append to captured stderr. Called by the REprintf hook.
pub fn capture_stderr(msg: &str) {
    if is_capturing() {
        super::instance::with_current_instance(|inst| {
            if let Some(s) = inst.capture_stderr.as_mut() {
                s.push_str(msg);
            }
        });
    }
}

pub(crate) fn format_sexp(x: SEXP) -> String {
    if x.is_null() {
        return "NULL".to_string();
    }
    if let Some(sexp) = crate::sexp::object::Sexp::from_raw(x) {
        format_sexp_direct(sexp)
    } else {
        "NULL".to_string()
    }
}

fn format_aligned_values(vals: Vec<String>) -> String {
    let width = vals.iter().map(|v| v.len()).max().unwrap_or(0);
    vals.into_iter()
        .map(|v| format!("{v:>width$}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_named_values(names: &[String], values: &[String]) -> String {
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

fn format_integer_value(v: i32) -> String {
    if v == NA_INTEGER {
        "NA".to_string()
    } else {
        v.to_string()
    }
}

fn format_real_value(v: f64) -> String {
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
    } else if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format_r_default_real(v)
    }
}

fn is_finite_r_number(v: f64) -> bool {
    !R_IsNA(v) && !R_IsNaN(v) && v.is_finite()
}

fn format_real_value_for_vector(v: f64, force_decimal_for_whole: bool) -> String {
    if force_decimal_for_whole && is_finite_r_number(v) && v.fract() == 0.0 {
        format!("{v:.1}")
    } else {
        format_real_value(v)
    }
}

fn format_real_vector_values(x: Sexp<'_>, limit: R_xlen_t) -> Vec<String> {
    let values: Vec<_> = (0..x.len().min(limit)).map(|i| x.try_real_elt(i)).collect();
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

fn trim_float(s: String) -> String {
    let (mut mantissa, exponent) = match s.find(['e', 'E']) {
        Some(idx) => (s[..idx].to_string(), &s[idx..]),
        None => (s, ""),
    };
    if mantissa.contains('.') {
        while mantissa.ends_with('0') {
            mantissa.pop();
        }
        if mantissa.ends_with('.') {
            mantissa.pop();
        }
    }
    format!("{mantissa}{exponent}")
}

fn format_r_default_real(v: f64) -> String {
    let digits = 7i32;
    let abs = v.abs();
    if abs == 0.0 {
        return "0".to_string();
    }

    let exponent = abs.log10().floor() as i32;
    if !(-4..digits).contains(&exponent) {
        return trim_float(format!("{v:.6e}"));
    }

    let decimals = if exponent >= 0 {
        (digits - exponent - 1).max(0) as usize
    } else {
        (digits - exponent - 1) as usize
    };
    trim_float(format!("{v:.decimals$}"))
}

fn format_logical_value(v: i32) -> String {
    match v {
        0 => "FALSE".to_string(),
        1 => "TRUE".to_string(),
        _ => "NA".to_string(),
    }
}

fn format_complex_value(v: super::ffi::Rcomplex) -> String {
    if R_IsNA(v.r) || R_IsNA(v.i) {
        return "NA".to_string();
    }
    let real = format_real_value(v.r);
    let imaginary = format_real_value(v.i.abs());
    if v.i.is_sign_negative() {
        format!("{real}-{imaginary}i")
    } else {
        format!("{real}+{imaginary}i")
    }
}

fn format_access_error(err: impl std::fmt::Display) -> String {
    format!("<{err}>")
}

fn format_integer_element(x: Sexp<'_>, i: R_xlen_t) -> String {
    x.try_integer_elt(i)
        .map(format_integer_value)
        .unwrap_or_else(format_access_error)
}

fn format_real_element(x: Sexp<'_>, i: R_xlen_t) -> String {
    x.try_real_elt(i)
        .map(format_real_value)
        .unwrap_or_else(format_access_error)
}

fn format_logical_element(x: Sexp<'_>, i: R_xlen_t) -> String {
    x.try_logical_elt(i)
        .map(format_logical_value)
        .unwrap_or_else(format_access_error)
}

fn format_complex_element(x: Sexp<'_>, i: R_xlen_t) -> String {
    x.try_complex_elt(i)
        .map(format_complex_value)
        .unwrap_or_else(format_access_error)
}

fn format_raw_value(v: u8) -> String {
    format!("{v:02x}")
}

fn get_named_attribute<'a>(x: Sexp<'a>, name: &str) -> Option<Sexp<'a>> {
    let cname = std::ffi::CString::new(name).ok()?;
    unsafe {
        let attr = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::symbol::Rf_install(cname.as_ptr()),
        );
        if attr == crate::sexp::globals::R_NilValue() {
            return None;
        }
        Sexp::from_raw(attr)
    }
}

fn has_regexpr_attributes(x: Sexp<'_>) -> bool {
    get_named_attribute(x, "match.length").is_some()
        || get_named_attribute(x, "index.type").is_some()
        || get_named_attribute(x, "useBytes").is_some()
}

fn format_regexpr_attributes(x: Sexp<'_>) -> String {
    let mut out = String::new();
    for name in ["match.length", "index.type", "useBytes"] {
        if let Some(value) = get_named_attribute(x, name) {
            out.push('\n');
            out.push_str(&format!("attr(,\"{name}\")\n"));
            out.push_str(&format_sexp_direct(value));
        }
    }
    out
}

fn matrix_dims(x: Sexp<'_>) -> Option<(usize, usize)> {
    unsafe {
        let dim = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_DimSymbol(),
        );
        let dim = Sexp::from_raw(dim)?;
        if dim.typeof_() != SEXPTYPE::INTSXP || dim.len() != 2 {
            return None;
        }
        let nrow = dim.integer_elt(0)? as usize;
        let ncol = dim.integer_elt(1)? as usize;
        if nrow.checked_mul(ncol)? > x.len() as usize {
            return None;
        }
        Some((nrow, ncol))
    }
}

fn matrix_dimnames(
    x: Sexp<'_>,
    nrow: usize,
    ncol: usize,
) -> (Option<Vec<String>>, Option<Vec<String>>) {
    unsafe {
        let dimnames = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_DimNamesSymbol(),
        );
        let Some(dimnames) = Sexp::from_raw(dimnames) else {
            return (None, None);
        };
        if dimnames.typeof_() != SEXPTYPE::VECSXP || dimnames.len() < 2 {
            return (None, None);
        }
        let row_names =
            string_vector_values(crate::sexp::accessors::VECTOR_ELT(dimnames.as_raw(), 0))
                .filter(|names| names.len() == nrow);
        let col_names =
            string_vector_values(crate::sexp::accessors::VECTOR_ELT(dimnames.as_raw(), 1))
                .filter(|names| names.len() == ncol);
        (row_names, col_names)
    }
}

fn format_matrix_with<F>(x: Sexp<'_>, nrow: usize, ncol: usize, value_at: F) -> String
where
    F: Fn(usize, usize) -> String,
{
    let (row_names, col_names) = matrix_dimnames(x, nrow, ncol);
    let row_labels: Vec<String> = (0..nrow)
        .map(|r| {
            row_names
                .as_ref()
                .and_then(|names| names.get(r))
                .cloned()
                .unwrap_or_else(|| format!("[{},]", r + 1))
        })
        .collect();
    let col_labels: Vec<String> = (0..ncol)
        .map(|c| {
            col_names
                .as_ref()
                .and_then(|names| names.get(c))
                .cloned()
                .unwrap_or_else(|| format!("[,{}]", c + 1))
        })
        .collect();
    let row_width = row_labels.iter().map(String::len).max().unwrap_or(0);
    let mut values = vec![vec![String::new(); ncol]; nrow];
    let mut widths = Vec::with_capacity(ncol);
    for c in 0..ncol {
        let mut width = col_labels[c].len().max(1);
        for r in 0..nrow {
            let value = value_at(r, c);
            width = width.max(value.len());
            values[r][c] = value;
        }
        widths.push(width);
    }

    let mut lines = Vec::with_capacity(nrow + 1);
    let mut header = " ".repeat(row_width + 1);
    for (c, width) in widths.iter().enumerate() {
        header.push_str(&format!("{:>width$}", col_labels[c]));
        if c + 1 < ncol {
            header.push(' ');
        }
    }
    lines.push(header);

    for r in 0..nrow {
        let mut line = format!("{:<row_width$}", row_labels[r]);
        for (c, width) in widths.iter().enumerate() {
            line.push(' ');
            line.push_str(&format!("{:>width$}", values[r][c]));
        }
        lines.push(line);
    }
    lines.join("\n")
}

fn format_matrix(x: Sexp<'_>) -> Option<String> {
    let (nrow, ncol) = matrix_dims(x)?;
    match x.typeof_() {
        SEXPTYPE::INTSXP => Some(format_matrix_with(x, nrow, ncol, |r, c| {
            format_integer_element(x, (r + c * nrow) as i64)
        })),
        SEXPTYPE::REALSXP => Some(format_matrix_with(x, nrow, ncol, |r, c| {
            format_real_element(x, (r + c * nrow) as i64)
        })),
        SEXPTYPE::LGLSXP => Some(format_matrix_with(x, nrow, ncol, |r, c| {
            format_logical_element(x, (r + c * nrow) as i64)
        })),
        SEXPTYPE::CPLXSXP => Some(format_matrix_with(x, nrow, ncol, |r, c| {
            format_complex_element(x, (r + c * nrow) as i64)
        })),
        _ => None,
    }
}

fn factor_levels(x: Sexp<'_>) -> Option<Vec<String>> {
    unsafe {
        let class = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        if !string_vector_contains(class, "factor") {
            return None;
        }

        let levels = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_LevelsSymbol(),
        );
        string_vector_values(levels).filter(|levels| !levels.is_empty())
    }
}

fn string_vector_contains(x: SEXP, needle: &str) -> bool {
    string_vector_values(x)
        .map(|values| values.iter().any(|value| value == needle))
        .unwrap_or(false)
}

fn string_vector_values(x: SEXP) -> Option<Vec<String>> {
    let sexp = Sexp::from_raw(x)?;
    if sexp.typeof_() != SEXPTYPE::STRSXP {
        return None;
    }
    let mut values = Vec::with_capacity(sexp.len() as usize);
    for i in 0..sexp.len() {
        values.push(string_element_text(sexp, i).flatten()?.to_string());
    }
    Some(values)
}

fn string_vector_labels(x: SEXP) -> Option<Vec<String>> {
    let sexp = Sexp::from_raw(x)?;
    if sexp.typeof_() != SEXPTYPE::STRSXP {
        return None;
    }
    let mut values = Vec::with_capacity(sexp.len() as usize);
    for i in 0..sexp.len() {
        values.push(match string_element_text(sexp, i) {
            Some(Some(value)) => value.to_string(),
            Some(None) | None => "<NA>".to_string(),
        });
    }
    Some(values)
}

fn vector_print_names(x: Sexp<'_>) -> Option<Vec<String>> {
    unsafe {
        let names = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        let names = string_vector_labels(names)?;
        if names.len() != x.len() as usize || names.iter().all(|name| name.is_empty()) {
            None
        } else {
            Some(names)
        }
    }
}

fn format_named_atomic_vector(x: Sexp<'_>, values: Vec<String>) -> Option<String> {
    let mut names = vector_print_names(x)?;
    let limit = values.len();
    names.truncate(limit);
    Some(format_named_values(&names, &values))
}

fn string_element_text<'a>(x: Sexp<'a>, i: R_xlen_t) -> Option<Option<&'a str>> {
    let charsxp = x.try_string_elt(i).ok()?;
    if charsxp.as_raw() == unsafe { R_NaString() } {
        Some(None)
    } else {
        charsxp.try_as_str().ok().map(Some)
    }
}

fn format_string_element(x: Sexp<'_>, i: R_xlen_t) -> String {
    match string_element_text(x, i) {
        Some(Some(value)) => format!("\"{}\"", value),
        Some(None) | None => "NA".to_string(),
    }
}

fn format_string_vector(x: Sexp<'_>) -> String {
    if x.len() == 0 {
        return "character(0)".to_string();
    }
    let vals: Vec<String> = (0..x.len().min(10))
        .map(|i| format_string_element(x, i))
        .collect();
    let suffix = if x.len() > 10 { " ..." } else { "" };
    format!("[1] {}{}", vals.join(" "), suffix)
}

fn format_factor(x: Sexp<'_>) -> Option<String> {
    let levels = factor_levels(x)?;
    let vals: Vec<String> = x
        .iter_integer()
        .take(10)
        .map(|code| {
            if code == NA_INTEGER {
                "NA".to_string()
            } else {
                levels
                    .get((code - 1) as usize)
                    .cloned()
                    .unwrap_or_else(|| code.to_string())
            }
        })
        .collect();
    let suffix = if x.len() > 10 { " ..." } else { "" };
    Some(format!(
        "[1] {}{}\nLevels: {}",
        vals.join(" "),
        suffix,
        levels.join(" ")
    ))
}

fn table_names(x: Sexp<'_>) -> Option<Vec<String>> {
    unsafe {
        let class = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        if !string_vector_contains(class, "table") {
            return None;
        }
        let names = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        string_vector_values(names).filter(|names| names.len() == x.len() as usize)
    }
}

fn table_title(x: Sexp<'_>) -> Option<String> {
    unsafe {
        let title = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::symbol::Rf_install(c"table.name".as_ptr()),
        );
        let title = Sexp::from_raw(title)?;
        if title.typeof_() != SEXPTYPE::STRSXP || title.len() == 0 {
            return None;
        }
        string_element_text(title, 0).flatten().map(str::to_string)
    }
}

fn format_table(x: Sexp<'_>) -> Option<String> {
    let names = table_names(x)?;
    let values: Vec<String> = match x.typeof_() {
        SEXPTYPE::INTSXP => (0..x.len()).map(|i| format_integer_element(x, i)).collect(),
        SEXPTYPE::REALSXP => (0..x.len()).map(|i| format_real_element(x, i)).collect(),
        _ => return None,
    };
    if let Some(title) = table_title(x) {
        let widths: Vec<usize> = names
            .iter()
            .zip(&values)
            .map(|(name, value)| name.len().max(value.len()).max(4))
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
        Some(format!("{title}\n{name_line}\n{value_line}"))
    } else {
        Some(format!("\n{}\n{}", names.join(" "), values.join(" ")))
    }
}

fn list_names(x: Sexp<'_>) -> Vec<String> {
    unsafe {
        let names = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        string_vector_values(names).unwrap_or_default()
    }
}

fn has_class(x: Sexp<'_>, class_name: &str) -> bool {
    unsafe {
        let class = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        string_vector_contains(class, class_name)
    }
}

fn data_frame_nrows(x: Sexp<'_>) -> R_xlen_t {
    unsafe {
        let row_names = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::symbol::Rf_install(c"row.names".as_ptr()),
        );
        if let Some(row_names) = Sexp::from_raw(row_names)
            && row_names.typeof_() == SEXPTYPE::INTSXP
            && row_names.len() == 2
            && let Some(values) = row_names.as_integer_slice()
            && values[0] == NA_INTEGER
            && values[1] < 0
        {
            return (-values[1]) as R_xlen_t;
        }
    }
    x.iter_vector().map(|col| col.len()).max().unwrap_or(0)
}

fn format_data_frame_cell(x: Sexp<'_>, row: R_xlen_t) -> String {
    if x.len() == 0 {
        return "NA".to_string();
    }
    let i = row % x.len();
    match x.typeof_() {
        SEXPTYPE::INTSXP => format_integer_element(x, i),
        SEXPTYPE::REALSXP => format_real_element(x, i),
        SEXPTYPE::LGLSXP => format_logical_element(x, i),
        SEXPTYPE::STRSXP => match string_element_text(x, i) {
            Some(Some(value)) => value.to_string(),
            Some(None) | None => "NA".to_string(),
        },
        _ => format_sexp_direct(x),
    }
}

fn format_data_frame(x: Sexp<'_>) -> Option<String> {
    if !has_class(x, "data.frame") {
        return None;
    }
    let names = list_names(x);
    let nrow = data_frame_nrows(x);
    let columns: Vec<Sexp<'_>> = x.iter_vector().collect();
    let header = format!("  {}", names.join(" "));
    let mut lines = Vec::with_capacity(nrow as usize + 1);
    lines.push(header);
    for row in 0..nrow {
        let mut parts = Vec::with_capacity(columns.len() + 1);
        parts.push((row + 1).to_string());
        for col in &columns {
            parts.push(format_data_frame_cell(*col, row));
        }
        lines.push(parts.join(" "));
    }
    Some(lines.join("\n"))
}

fn list_element_header(index: usize, names: &[String]) -> String {
    match names.get(index) {
        Some(name) if !name.is_empty() => format!("${name}"),
        _ => format!("[[{}]]", index + 1),
    }
}

fn format_list(x: Sexp<'_>) -> String {
    if x.len() == 0 {
        return "list()".to_string();
    }

    let names = list_names(x);
    let mut sections = Vec::with_capacity(x.len() as usize);
    for (index, elem) in x.iter_vector().enumerate() {
        sections.push(format!(
            "{}\n{}",
            list_element_header(index, &names),
            format_sexp_direct(elem)
        ));
    }
    sections.join("\n\n")
}

/// Print an R object to the captured output (or stdout if not capturing).
///
/// This is the Rust implementation of R's Rf_PrintValue. For Android
/// embedding, use [`start_capture`] before evaluation and [`stop_capture`]
/// after to collect printed output as a string.
pub fn print_value(x: Sexp<'_>) {
    match x.typeof_() {
        SEXPTYPE::NILSXP => {
            emit("NULL\n");
        }
        SEXPTYPE::INTSXP => {
            if let Some(output) = format_matrix(x) {
                emit(&format!("{output}\n"));
                return;
            }
            if let Some(output) = format_table(x) {
                emit(&format!("{output}\n"));
                return;
            }
            if let Some(output) = format_factor(x) {
                emit(&format!("{output}\n"));
                return;
            }
            if has_regexpr_attributes(x) {
                emit(&format!("{}\n", format_sexp_direct(x)));
                return;
            }
            if x.len() == 1 {
                emit(&format!("[1] {}\n", format_integer_element(x, 0)));
            } else {
                let vals: Vec<String> = (0..x.len().min(10))
                    .map(|i| format_integer_element(x, i))
                    .collect();
                let suffix = if x.len() > 10 { " ..." } else { "" };
                if let Some(output) = format_named_atomic_vector(x, vals.clone()) {
                    emit(&format!("{output}{suffix}\n"));
                } else {
                    emit(&format!("[1] {}{}\n", format_aligned_values(vals), suffix));
                }
            }
        }
        SEXPTYPE::REALSXP => {
            if let Some(output) = format_matrix(x) {
                emit(&format!("{output}\n"));
                return;
            }
            if let Some(output) = format_table(x) {
                emit(&format!("{output}\n"));
                return;
            }
            if x.len() == 1 {
                emit(&format!("[1] {}\n", format_real_element(x, 0)));
            } else {
                let vals = format_real_vector_values(x, 10);
                let suffix = if x.len() > 10 { " ..." } else { "" };
                if let Some(output) = format_named_atomic_vector(x, vals.clone()) {
                    emit(&format!("{output}{suffix}\n"));
                } else {
                    emit(&format!("[1] {}{}\n", format_aligned_values(vals), suffix));
                }
            }
        }
        SEXPTYPE::LGLSXP => {
            if let Some(output) = format_matrix(x) {
                emit(&format!("{output}\n"));
                return;
            }
            let vals: Vec<String> = (0..x.len().min(10))
                .map(|i| format_logical_element(x, i))
                .collect();
            let suffix = if x.len() > 10 { " ..." } else { "" };
            if let Some(output) = format_named_atomic_vector(x, vals.clone()) {
                emit(&format!("{output}{suffix}\n"));
            } else {
                emit(&format!("[1] {}{}\n", format_aligned_values(vals), suffix));
            }
        }
        SEXPTYPE::CPLXSXP => {
            if let Some(output) = format_matrix(x) {
                emit(&format!("{output}\n"));
                return;
            }
            let vals: Vec<String> = (0..x.len().min(10))
                .map(|i| format_complex_element(x, i))
                .collect();
            let suffix = if x.len() > 10 { " ..." } else { "" };
            if let Some(output) = format_named_atomic_vector(x, vals.clone()) {
                emit(&format!("{output}{suffix}\n"));
            } else {
                emit(&format!("[1] {}{}\n", format_aligned_values(vals), suffix));
            }
        }
        SEXPTYPE::STRSXP => {
            let vals: Vec<String> = (0..x.len().min(10))
                .map(|i| format_string_element(x, i))
                .collect();
            let suffix = if x.len() > 10 { " ..." } else { "" };
            if let Some(output) = format_named_atomic_vector(x, vals) {
                emit(&format!("{output}{suffix}\n"));
            } else {
                emit(&format!("{}\n", format_string_vector(x)));
            }
        }
        SEXPTYPE::RAWSXP => {
            let vals: Vec<String> = x.iter_raw().take(10).map(format_raw_value).collect();
            let suffix = if x.len() > 10 { " ..." } else { "" };
            emit(&format!("[1] {}{}\n", vals.join(" "), suffix));
        }
        SEXPTYPE::VECSXP => {
            if let Some(output) = format_data_frame(x) {
                emit(&format!("{output}\n"));
                return;
            }
            emit(&format!("{}\n", format_list(x)));
        }
        tp => {
            let type_name = match tp {
                SEXPTYPE::EXPRSXP => "expression",
                SEXPTYPE::RAWSXP => "raw",
                SEXPTYPE::CPLXSXP => "complex",
                SEXPTYPE::SYMSXP => "symbol",
                SEXPTYPE::CLOSXP => "closure",
                SEXPTYPE::ENVSXP => "environment",
                SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP => "pairlist",
                SEXPTYPE::CHARSXP => "charsxp",
                SEXPTYPE::PROMSXP => "promise",
                SEXPTYPE::DOTSXP => "...",
                SEXPTYPE::SPECIALSXP => "special",
                SEXPTYPE::BUILTINSXP => "builtin",
                SEXPTYPE::EXTPTRSXP => "externalptr",
                SEXPTYPE::WEAKREFSXP => "weakref",
                SEXPTYPE::BCODESXP => "bytecode",
                SEXPTYPE::OBJSXP => "object",
                _ => "unknown",
            };
            let output = format!("[{}; length={}]", type_name, x.len());
            emit(&output);
            emit("\n");
        }
    }
}

fn emit(msg: &str) {
    if is_capturing() {
        capture_stdout(msg);
    } else {
        print!("{}", msg);
    }
}

pub fn format_sexp_direct(x: Sexp<'_>) -> String {
    match x.typeof_() {
        SEXPTYPE::NILSXP => "NULL".to_string(),
        SEXPTYPE::INTSXP => {
            if x.len() == 0 {
                return "integer(0)".to_string();
            }
            if let Some(output) = format_matrix(x) {
                return output;
            }
            if let Some(output) = format_factor(x) {
                return output;
            }
            let base = if x.len() == 1 {
                format!("[1] {}", format_integer_element(x, 0))
            } else {
                let vals: Vec<String> = (0..x.len().min(10))
                    .map(|i| format_integer_element(x, i))
                    .collect();
                let suffix = if x.len() > 10 { " ..." } else { "" };
                format_named_atomic_vector(x, vals.clone())
                    .map(|output| format!("{output}{suffix}"))
                    .unwrap_or_else(|| format!("[1] {}{}", format_aligned_values(vals), suffix))
            };
            format!("{base}{}", format_regexpr_attributes(x))
        }
        SEXPTYPE::REALSXP => {
            if x.len() == 0 {
                return "numeric(0)".to_string();
            }
            if let Some(output) = format_matrix(x) {
                return output;
            }
            if x.len() == 1 {
                format!("[1] {}", format_real_element(x, 0))
            } else {
                let vals = format_real_vector_values(x, 10);
                let suffix = if x.len() > 10 { " ..." } else { "" };
                format_named_atomic_vector(x, vals.clone())
                    .map(|output| format!("{output}{suffix}"))
                    .unwrap_or_else(|| format!("[1] {}{}", format_aligned_values(vals), suffix))
            }
        }
        SEXPTYPE::LGLSXP => {
            if x.len() == 0 {
                return "logical(0)".to_string();
            }
            if let Some(output) = format_matrix(x) {
                return output;
            }
            let vals: Vec<String> = (0..x.len().min(10))
                .map(|i| format_logical_element(x, i))
                .collect();
            let suffix = if x.len() > 10 { " ..." } else { "" };
            format_named_atomic_vector(x, vals.clone())
                .map(|output| format!("{output}{suffix}"))
                .unwrap_or_else(|| format!("[1] {}{}", format_aligned_values(vals), suffix))
        }
        SEXPTYPE::CPLXSXP => {
            if x.len() == 0 {
                return "complex(0)".to_string();
            }
            if let Some(output) = format_matrix(x) {
                return output;
            }
            let vals: Vec<String> = (0..x.len().min(10))
                .map(|i| format_complex_element(x, i))
                .collect();
            let suffix = if x.len() > 10 { " ..." } else { "" };
            format_named_atomic_vector(x, vals.clone())
                .map(|output| format!("{output}{suffix}"))
                .unwrap_or_else(|| format!("[1] {}{}", format_aligned_values(vals), suffix))
        }
        SEXPTYPE::STRSXP => format_string_vector(x),
        SEXPTYPE::RAWSXP => {
            if x.len() == 0 {
                return "raw(0)".to_string();
            }
            let vals: Vec<String> = x.iter_raw().take(10).map(format_raw_value).collect();
            let suffix = if x.len() > 10 { " ..." } else { "" };
            format!("[1] {}{}", vals.join(" "), suffix)
        }
        SEXPTYPE::VECSXP => format_list(x),
        tp => {
            let type_name = match tp {
                SEXPTYPE::EXPRSXP => "expression",
                SEXPTYPE::RAWSXP => "raw",
                SEXPTYPE::CPLXSXP => "complex",
                SEXPTYPE::SYMSXP => "symbol",
                SEXPTYPE::CLOSXP => "closure",
                SEXPTYPE::ENVSXP => "environment",
                SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP => "pairlist",
                SEXPTYPE::CHARSXP => "charsxp",
                _ => "unknown",
            };
            format!("[{}; length={}]", type_name, x.len())
        }
    }
}

/// Print an R object's structure (like str()).
pub fn print_structure(x: Sexp<'_>, indent: usize) {
    let prefix = "  ".repeat(indent);

    match x.typeof_() {
        SEXPTYPE::INTSXP => {
            let vals: Vec<_> = x.iter_integer().take(10).collect();
            let suffix = if x.len() > 10 { ", ..." } else { "" };
            let output = format!("{}int [{}]: {:?}{}", prefix, x.len(), vals, suffix);
            if is_capturing() {
                capture_stdout(&output);
                capture_stdout("\n");
            } else {
                println!("{}", output);
            }
        }
        SEXPTYPE::REALSXP => {
            let vals: Vec<_> = x.iter_real().take(10).collect();
            let suffix = if x.len() > 10 { ", ..." } else { "" };
            let output = format!("{}double [{}]: {:?}{}", prefix, x.len(), vals, suffix);
            if is_capturing() {
                capture_stdout(&output);
                capture_stdout("\n");
            } else {
                println!("{}", output);
            }
        }
        SEXPTYPE::STRSXP => {
            let output = format!("{}character [{}]", prefix, x.len());
            if is_capturing() {
                capture_stdout(&output);
                capture_stdout("\n");
            } else {
                println!("{}", output);
            }
        }
        SEXPTYPE::VECSXP => {
            let output = format!("{}list [{}]", prefix, x.len());
            if is_capturing() {
                capture_stdout(&output);
                capture_stdout("\n");
            } else {
                println!("{}", output);
            }
            for (i, elem) in x.iter_vector().take(5).enumerate() {
                print_structure(elem, indent + 1);
            }
        }
        _ => {
            print_value(x);
        }
    }
}

/// FFI function: Rf_PrintValue
pub(crate) unsafe fn Rf_PrintValue(x: SEXP) {
    if let Some(s) = Sexp::from_raw(x) {
        print_value(s);
    }
}

/// FFI function: Rf_PrintValueEnv (print with environment context)
pub(crate) unsafe fn Rf_PrintValueEnv(x: SEXP, _env: SEXP) {
    if let Some(s) = Sexp::from_raw(x) {
        print_value(s);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::session::RSession;

    #[test]
    fn test_capture_lifecycle() {
        let _session = RSession::new();
        assert!(!is_capturing());
        start_capture();
        assert!(is_capturing());
        capture_stdout("hello ");
        capture_stdout("world\n");
        capture_stderr("warning!\n");
        let output = stop_capture();
        assert_eq!(output.stdout, "hello world\n");
        assert_eq!(output.stderr, "warning!\n");
        assert!(!is_capturing());
    }

    #[test]
    fn test_capture_empty() {
        let _session = RSession::new();
        start_capture();
        let output = stop_capture();
        assert_eq!(output.stdout, "");
        assert_eq!(output.stderr, "");
    }

    #[test]
    fn test_nested_capture() {
        let _session = RSession::new();
        start_capture();
        capture_stdout("outer ");
        capture_stderr("outer err ");

        start_capture();
        capture_stdout("inner ");
        capture_stderr("inner err ");
        let inner = stop_capture();
        assert_eq!(inner.stdout, "inner ");
        assert_eq!(inner.stderr, "inner err ");

        assert!(is_capturing());
        capture_stdout("resumed");
        capture_stderr("resumed err");

        let outer = stop_capture();
        assert_eq!(outer.stdout, "outer resumed");
        assert_eq!(outer.stderr, "outer err resumed err");
        assert!(!is_capturing());
    }

    #[test]
    fn test_print_logical_vector() {
        let mut session = RSession::new();
        session
            .with_arena(|arena| {
                let ptr = arena.alloc_vector(SEXPTYPE::LGLSXP, 3);
                let sexp = Sexp::from_raw(ptr).expect("logical vector allocation failed");
                assert!(sexp.set_logical_elt(0, 0));
                assert!(sexp.set_logical_elt(1, 1));
                assert!(sexp.set_logical_elt(2, crate::sexp::ffi::NA_LOGICAL));

                start_capture();
                print_value(sexp);
                let output = stop_capture();
                assert_eq!(output.stdout, "[1] FALSE  TRUE    NA\n");
            })
            .unwrap();
    }

    #[test]
    fn test_string_output_uses_safe_charsxp_access_and_preserves_na() {
        let mut session = RSession::new();
        session
            .with_arena(|arena| {
                let ptr = arena.alloc_vector(SEXPTYPE::STRSXP, 2);
                let sexp = Sexp::from_raw(ptr).expect("string vector allocation failed");
                let value = Sexp::from_raw(arena.alloc_charsxp(b"a")).expect("CHARSXP");
                let missing = Sexp::from_raw(unsafe { R_NaString() }).expect("NA_STRING");
                sexp.try_set_string_elt(0, value).expect("set string");
                sexp.try_set_string_elt(1, missing).expect("set string");

                assert_eq!(format_sexp_direct(sexp), "[1] \"a\" NA");

                start_capture();
                print_value(sexp);
                let output = stop_capture();
                assert_eq!(output.stdout, "[1] \"a\" NA\n");
            })
            .unwrap();
    }

    #[test]
    fn test_atomic_element_formatting_reports_access_errors() {
        let mut session = RSession::new();
        session
            .with_arena(|arena| {
                let real = Sexp::from_raw(arena.alloc_vector(SEXPTYPE::REALSXP, 1))
                    .expect("real vector allocation failed");

                assert!(format_integer_element(real, 0).contains("expected integer vector"));
                assert!(format_real_element(real, 2).contains("outside vector length"));
            })
            .unwrap();
    }

    #[test]
    fn test_format_atomic_na_values() {
        assert_eq!(format_integer_value(crate::sexp::ffi::NA_INTEGER), "NA");
        assert_eq!(format_real_value(crate::sexp::ffi::NA_REAL), "NA");
        assert_eq!(format_real_value(f64::NAN), "NaN");
        assert_eq!(format_real_value(f64::INFINITY), "Inf");
        assert_eq!(format_real_value(f64::NEG_INFINITY), "-Inf");
    }

    #[test]
    fn test_numeric_vector_alignment_matches_r_simple_output() {
        let vals = vec!["2".to_string(), "NA".to_string(), "4".to_string()];
        assert_eq!(format_aligned_values(vals), " 2 NA  4");
    }

    #[test]
    fn test_real_vector_alignment_keeps_decimal_column() {
        let mut session = RSession::new();
        session
            .with_arena(|arena| {
                let ptr = arena.alloc_vector(SEXPTYPE::REALSXP, 3);
                let sexp = Sexp::from_raw(ptr).expect("real vector allocation failed");
                sexp.try_set_real_elt(0, 200.0).expect("set real");
                sexp.try_set_real_elt(1, 80200.0).expect("set real");
                sexp.try_set_real_elt(2, 100.5).expect("set real");

                assert_eq!(format_sexp_direct(sexp), "[1]   200.0 80200.0   100.5");

                start_capture();
                print_value(sexp);
                let output = stop_capture();
                assert_eq!(output.stdout, "[1]   200.0 80200.0   100.5\n");
            })
            .unwrap();
    }
}
