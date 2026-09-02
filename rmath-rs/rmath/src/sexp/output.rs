//! R output capture for embedding.
//!
//! Captures Rprintf, REprintf, and other R output functions
//! so they can be returned to the caller instead of printing
//! to stdout/stderr.

use super::accessors::{ATTRIB, CAR, CDR, CHAR, PRINTNAME, STRING_ELT, TAG, VECTOR_ELT, XLENGTH};
use super::ffi::{NA_INTEGER, R_IsNA, R_IsNaN, R_xlen_t, SEXP, SEXPTYPE};
use super::globals::R_NilValue;
use super::instance::RInstance;
use super::object::Sexp;

/// Captured R output.
#[derive(Debug, Clone, Default)]
pub struct RCapturedOutput {
    pub stdout: String,
    pub stderr: String,
}

/// Per-session output capture buffers.
#[derive(Debug, Default)]
pub(crate) struct OutputCaptureState {
    stdout: Option<String>,
    stderr: Option<String>,
    stack: Vec<(Option<String>, Option<String>)>,
}

impl OutputCaptureState {
    pub(crate) fn start(&mut self) {
        let outer = (self.stdout.take(), self.stderr.take());
        if outer.0.is_some() || outer.1.is_some() {
            self.stack.push(outer);
        }
        self.stdout = Some(String::new());
        self.stderr = Some(String::new());
    }

    pub(crate) fn stop(&mut self) -> RCapturedOutput {
        let stdout = self.stdout.take().unwrap_or_default();
        let stderr = self.stderr.take().unwrap_or_default();
        if let Some((outer_stdout, outer_stderr)) = self.stack.pop() {
            self.stdout = outer_stdout;
            self.stderr = outer_stderr;
        }
        RCapturedOutput { stdout, stderr }
    }

    pub(crate) fn is_capturing(&self) -> bool {
        self.stdout.is_some() || self.stderr.is_some()
    }

    pub(crate) fn capture_stdout(&mut self, msg: &str) {
        if let Some(stdout) = self.stdout.as_mut() {
            stdout.push_str(msg);
        }
    }

    pub(crate) fn capture_stderr(&mut self, msg: &str) {
        if let Some(stderr) = self.stderr.as_mut() {
            stderr.push_str(msg);
        }
    }

    /// Append to the captured stdout buffer, bypassing any active output
    /// sink. The deferred-warning flush uses this: upstream warnings are
    /// stderr traffic and are not diverted by `sink()`, but the session's
    /// single interleaved output stream is the stdout buffer.
    pub(crate) fn capture_stdout_bypassing_sink(&mut self, msg: &str) {
        if let Some(stdout) = self.stdout.as_mut() {
            stdout.push_str(msg);
        }
    }
}

/// Start capturing R output.
pub fn start_capture() {
    super::instance::with_required_current_instance(start_capture_in);
}

pub(crate) fn start_capture_in(inst: &mut RInstance) {
    inst.output_capture.borrow_mut().start();
}

/// Stop capturing and return the captured output.
pub fn stop_capture() -> RCapturedOutput {
    super::instance::with_required_current_instance(stop_capture_in)
}

pub(crate) fn stop_capture_in(inst: &mut RInstance) -> RCapturedOutput {
    inst.output_capture.borrow_mut().stop()
}

/// Check if output capture is active.
pub fn is_capturing() -> bool {
    super::instance::with_current_instance(is_capturing_in).unwrap_or(false)
}

pub(crate) fn is_capturing_in(inst: &mut RInstance) -> bool {
    inst.output_capture.borrow().is_capturing()
        || crate::mainutils::connections::output_sink_active_in(inst)
}

/// Append to captured stdout. Called by the Rprintf hook.
pub fn capture_stdout(msg: &str) {
    super::instance::with_current_instance(|inst| capture_stdout_in(inst, msg));
}

pub(crate) fn capture_stdout_in(inst: &mut RInstance, msg: &str) {
    if crate::mainutils::connections::write_output_sink_in(inst, msg.as_bytes()) {
        return;
    }
    let mut capture = inst.output_capture.borrow_mut();
    if capture.stdout.is_some() {
        capture.capture_stdout(msg);
        return;
    }
    drop(capture);
    print!("{msg}");
}

/// Append to the session's single interleaved output stream — the stdout
/// capture buffer, bypassing any `sink()` diversion — falling back to real
/// stderr when no capture is active. Signal-time message() emission uses
/// this: upstream writes messages to stderr and the terminal interleaves
/// the two streams in real time; the session model keeps one ordered stream
/// so the text lands in statement order between print() side effects,
/// deferred warnings, and auto-printed values.
pub(crate) fn capture_interleaved(msg: &str) {
    super::instance::with_current_instance(|inst| {
        if inst.output_capture.borrow().stdout.is_some() {
            inst.output_capture
                .borrow_mut()
                .capture_stdout_bypassing_sink(msg);
        } else {
            eprint!("{msg}");
        }
    });
}
/// Append to captured stderr. Called by the REprintf hook.
pub fn capture_stderr(msg: &str) {
    super::instance::with_current_instance(|inst| capture_stderr_in(inst, msg));
}

pub(crate) fn capture_stderr_in(inst: &mut RInstance, msg: &str) {
    inst.output_capture.borrow_mut().capture_stderr(msg);
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
    // IEEE allows signed zeros; print them as plain 0 like stock R's
    // EncodeReal0 ("if (x == 0.0) x = 0.0").
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

fn printable_attribute_name(attr: SEXP) -> Option<String> {
    unsafe {
        let tag = TAG(attr);
        if tag.is_null() || tag == R_NilValue() {
            return None;
        }
        let print_name = PRINTNAME(tag);
        if print_name.is_null() || print_name == R_NilValue() {
            return None;
        }
        let chars = CHAR(print_name);
        if chars.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr(chars)
            .to_str()
            .ok()
            .map(str::to_string)
    }
}

fn is_structural_print_attribute(name: &str) -> bool {
    matches!(name, "names" | "dim" | "dimnames" | "class" | "row.names")
}

fn format_printable_attributes(x: Sexp<'_>) -> String {
    unsafe {
        let mut attrs = ATTRIB(x.as_raw());
        let mut visible = Vec::new();
        while !attrs.is_null() && attrs != R_NilValue() {
            if let Some(name) = printable_attribute_name(attrs)
                && !is_structural_print_attribute(&name)
            {
                let value = CAR(attrs);
                if !value.is_null() && value != R_NilValue() {
                    visible.push((name, value));
                }
            }
            attrs = CDR(attrs);
        }

        let mut out = String::new();
        for (name, value) in visible {
            out.push('\n');
            out.push_str(&format!("attr(,\"{name}\")\n"));
            if let Some(value) = Sexp::from_raw(value) {
                out.push_str(&format_sexp_direct(value));
            } else {
                out.push_str("NULL");
            }
        }
        out
    }
}

fn format_with_printable_attributes(base: String, x: Sexp<'_>) -> String {
    format!("{base}{}", format_printable_attributes(x))
}

fn matrix_dims(x: Sexp<'_>) -> Option<(usize, usize)> {
    unsafe {
        let dim = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::attrib_core::R_DimSymbol(),
        );
        let dim = Sexp::from_raw(dim)?;
        if dim.clone().typeof_() != SEXPTYPE::INTSXP || dim.clone().len() != 2 {
            return None;
        }
        let nrow = dim.clone().integer_elt(0)? as usize;
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
        if dimnames.clone().typeof_() != SEXPTYPE::VECSXP || dimnames.clone().len() < 2 {
            return (None, None);
        }
        let row_names = string_vector_values(crate::sexp::accessors::VECTOR_ELT(
            dimnames.clone().as_raw(),
            0,
        ))
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

fn format_character_matrix_with<F>(x: Sexp<'_>, nrow: usize, ncol: usize, value_at: F) -> String
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
            line.push_str(&format!("{:<width$}", values[r][c]));
        }
        lines.push(line);
    }
    lines.join("\n")
}

fn format_matrix(x: Sexp<'_>) -> Option<String> {
    let (nrow, ncol) = matrix_dims(x.clone())?;
    match x.clone().typeof_() {
        SEXPTYPE::INTSXP => Some(format_matrix_with(x.clone(), nrow, ncol, |r, c| {
            format_integer_element(x.clone(), (r + c * nrow) as i64)
        })),
        SEXPTYPE::REALSXP => Some(format_matrix_with(x.clone(), nrow, ncol, |r, c| {
            format_real_element(x.clone(), (r + c * nrow) as i64)
        })),
        SEXPTYPE::LGLSXP => Some(format_matrix_with(x.clone(), nrow, ncol, |r, c| {
            format_logical_element(x.clone(), (r + c * nrow) as i64)
        })),
        SEXPTYPE::CPLXSXP => Some(format_matrix_with(x.clone(), nrow, ncol, |r, c| {
            format_complex_element(x.clone(), (r + c * nrow) as i64)
        })),
        SEXPTYPE::STRSXP => Some(format_character_matrix_with(
            x.clone(),
            nrow,
            ncol,
            |r, c| format_string_element(x.clone(), (r + c * nrow) as i64),
        )),
        _ => None,
    }
}

fn factor_levels(x: Sexp<'_>) -> Option<Vec<String>> {
    unsafe {
        let class = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
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
    if sexp.clone().typeof_() != SEXPTYPE::STRSXP {
        return None;
    }
    let mut values = Vec::with_capacity(sexp.clone().len() as usize);
    for i in 0..sexp.clone().len() {
        values.push(string_element_text(sexp.clone(), i).flatten()?.to_string());
    }
    Some(values)
}

fn string_vector_labels(x: SEXP) -> Option<Vec<String>> {
    let sexp = Sexp::from_raw(x)?;
    if sexp.clone().typeof_() != SEXPTYPE::STRSXP {
        return None;
    }
    let mut values = Vec::with_capacity(sexp.clone().len() as usize);
    for i in 0..sexp.clone().len() {
        values.push(match string_element_text(sexp.clone(), i) {
            Some(Some(value)) => value.to_string(),
            Some(None) | None => "<NA>".to_string(),
        });
    }
    Some(values)
}

fn vector_print_names(x: Sexp<'_>) -> Option<Vec<String>> {
    unsafe {
        let names = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
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

fn has_names_attribute(x: Sexp<'_>) -> bool {
    unsafe {
        let names = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        Sexp::from_raw(names).is_some_and(|names| names.typeof_() == SEXPTYPE::STRSXP)
    }
}

fn format_named_atomic_vector(x: Sexp<'_>, values: Vec<String>) -> Option<String> {
    let mut names = vector_print_names(x)?;
    let limit = values.len();
    names.truncate(limit);
    Some(format_named_values(&names, &values))
}

fn string_element_text<'a>(x: Sexp<'a>, i: R_xlen_t) -> Option<Option<&'a str>> {
    x.string_text_elt(i)
}

fn format_string_element(x: Sexp<'_>, i: R_xlen_t) -> String {
    match string_element_text(x, i) {
        Some(Some(value)) => format!("\"{}\"", escape_printed_string(value)),
        Some(None) | None => "NA".to_string(),
    }
}

fn escape_printed_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn format_string_vector(x: Sexp<'_>) -> String {
    if x.clone().len() == 0 {
        return "character(0)".to_string();
    }
    let vals: Vec<String> = (0..x.clone().len().min(10))
        .map(|i| format_string_element(x.clone(), i))
        .collect();
    let suffix = if x.len() > 10 { " ..." } else { "" };
    format!("[1] {}{}", vals.join(" "), suffix)
}

fn format_date_element(x: Sexp<'_>, i: R_xlen_t) -> String {
    x.try_real_elt(i)
        .ok()
        .and_then(crate::mainutils::essentials::date_days_to_iso)
        .map(|value| format!("\"{}\"", escape_printed_string(&value)))
        .unwrap_or_else(|| "NA".to_string())
}

fn format_date_vector(x: Sexp<'_>) -> String {
    if x.clone().len() == 0 {
        return "Date of length 0".to_string();
    }
    let vals: Vec<String> = (0..x.clone().len().min(10))
        .map(|i| format_date_element(x.clone(), i))
        .collect();
    let suffix = if x.clone().len() > 10 { " ..." } else { "" };
    format_named_atomic_vector(x, vals.clone())
        .map(|output| format!("{output}{suffix}"))
        .unwrap_or_else(|| format!("[1] {}{}", vals.join(" "), suffix))
}

fn format_posixct_element(x: Sexp<'_>, i: R_xlen_t, include_tz: bool, force_time: bool) -> String {
    x.try_real_elt(i)
        .ok()
        .and_then(|seconds| {
            crate::mainutils::essentials::posix_seconds_to_iso_with_time(
                seconds, include_tz, force_time,
            )
        })
        .map(|value| format!("\"{}\"", escape_printed_string(&value)))
        .unwrap_or_else(|| "NA".to_string())
}

fn posixct_vector_needs_time(x: Sexp<'_>) -> bool {
    (0..x.clone().len()).any(|i| {
        x.clone().try_real_elt(i).ok().is_some_and(|seconds| {
            !R_IsNA(seconds) && seconds.is_finite() && seconds.floor() as i64 % 86_400 != 0
        })
    })
}

fn format_posixct_vector(x: Sexp<'_>, include_tz: bool) -> String {
    if x.clone().len() == 0 {
        return "POSIXct of length 0".to_string();
    }
    let force_time = posixct_vector_needs_time(x.clone());
    let vals: Vec<String> = (0..x.clone().len().min(10))
        .map(|i| format_posixct_element(x.clone(), i, include_tz, force_time))
        .collect();
    let suffix = if x.clone().len() > 10 { " ..." } else { "" };
    format_named_atomic_vector(x, vals.clone())
        .map(|output| format!("{output}{suffix}"))
        .unwrap_or_else(|| format!("[1] {}{}", vals.join(" "), suffix))
}

fn difftime_units(x: Sexp<'_>) -> String {
    unsafe {
        let units = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::symbol::Rf_install(c"units".as_ptr()),
        );
        if let Some(units) = Sexp::from_raw(units)
            && units.clone().typeof_() == SEXPTYPE::STRSXP
            && units.clone().len() > 0
            && let Some(Some(value)) = string_element_text(units, 0)
        {
            return value.to_string();
        }
        "secs".to_string()
    }
}

fn format_difftime_vector(x: Sexp<'_>) -> String {
    let units = difftime_units(x.clone());
    if x.clone().len() == 0 {
        return format!("Time difference of 0 {units}");
    }
    let value = x
        .try_real_elt(0)
        .map(format_real_value)
        .unwrap_or_else(format_access_error);
    format!("Time difference of {value} {units}")
}

fn format_factor(x: Sexp<'_>) -> Option<String> {
    let levels = factor_levels(x.clone())?;
    let vals: Vec<String> = x
        .clone()
        .iter_integer()
        .take(10)
        .map(|code| {
            if code == NA_INTEGER {
                "<NA>".to_string()
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
            x.clone().as_raw(),
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        if !string_vector_contains(class, "table") {
            return None;
        }
        let names = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
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
        if title.clone().typeof_() != SEXPTYPE::STRSXP || title.clone().len() == 0 {
            return None;
        }
        string_element_text(title, 0).flatten().map(str::to_string)
    }
}

fn format_table(x: Sexp<'_>) -> Option<String> {
    let names = table_names(x.clone())?;
    let values: Vec<String> = match x.clone().typeof_() {
        SEXPTYPE::INTSXP => (0..x.clone().len())
            .map(|i| format_integer_element(x.clone(), i))
            .collect(),
        SEXPTYPE::REALSXP => (0..x.clone().len())
            .map(|i| format_real_element(x.clone(), i))
            .collect(),
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

fn format_summary_real_value(x: Sexp<'_>, i: R_xlen_t) -> String {
    if let Some(values) = x.as_real_slice() {
        let value = values[i as usize];
        if R_IsNA(value) {
            return "NA".to_string();
        }
        if R_IsNaN(value) {
            return "NaN".to_string();
        }
        return format!("{value:.1}");
    }
    "NA".to_string()
}

fn format_summary_default(x: Sexp<'_>) -> Option<String> {
    if !has_class(x.clone(), "summaryDefault") || !has_class(x.clone(), "table") {
        return None;
    }
    let names = table_names(x.clone())?;
    let values: Vec<String> = match x.clone().typeof_() {
        SEXPTYPE::REALSXP => (0..x.clone().len())
            .map(|i| {
                if matches!(names.get(i as usize).map(String::as_str), Some("NAs")) {
                    x.clone()
                        .as_real_slice()
                        .map(|values| format!("{}", values[i as usize] as i64))
                        .unwrap_or_else(|| format_summary_real_value(x.clone(), i))
                } else {
                    format_summary_real_value(x.clone(), i)
                }
            })
            .collect(),
        SEXPTYPE::INTSXP => (0..x.clone().len())
            .map(|i| {
                if matches!(
                    names.get(i as usize).map(String::as_str),
                    Some("Min.nchar" | "Max.nchar")
                ) && format_integer_element(x.clone(), i) == "NA"
                {
                    String::new()
                } else {
                    format_integer_element(x.clone(), i)
                }
            })
            .collect(),
        SEXPTYPE::STRSXP => (0..x.clone().len())
            .map(|i| match string_element_text(x.clone(), i) {
                Some(Some(value)) => value.to_string(),
                Some(None) | None => "NA".to_string(),
            })
            .collect(),
        _ => return None,
    };
    let min_width = match x.typeof_() {
        SEXPTYPE::REALSXP | SEXPTYPE::STRSXP => 7,
        SEXPTYPE::INTSXP => 8,
        _ => 0,
    };
    let widths: Vec<usize> = names
        .iter()
        .zip(&values)
        .map(|(name, value)| name.len().max(value.len()).max(min_width))
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
    Some(format!("{name_line}\n{value_line}"))
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
            x.clone().as_raw(),
            crate::sexp::symbol::Rf_install(c"row.names".as_ptr()),
        );
        if let Some(row_names) = Sexp::from_raw(row_names)
            && row_names.clone().typeof_() == SEXPTYPE::INTSXP
            && row_names.clone().len() == 2
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
    if x.clone().len() == 0 {
        return "NA".to_string();
    }
    let i = row % x.clone().len();
    match x.clone().typeof_() {
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
    if !has_class(x.clone(), "data.frame") {
        return None;
    }
    let names = list_names(x.clone());
    let nrow = data_frame_nrows(x.clone());
    let columns: Vec<Sexp<'_>> = x.iter_vector().collect();
    let row_width = nrow.to_string().len().max(1);
    let widths: Vec<usize> = columns
        .iter()
        .enumerate()
        .map(|(i, col)| {
            let name_width = names.get(i).map(String::len).unwrap_or(0);
            let value_width = (0..nrow)
                .map(|row| format_data_frame_cell(col.clone(), row).len())
                .max()
                .unwrap_or(0);
            name_width.max(value_width)
        })
        .collect();
    let header = format!(
        "{} {}",
        " ".repeat(row_width),
        names
            .iter()
            .zip(&widths)
            .map(|(name, width)| format!("{name:>width$}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let mut lines = Vec::with_capacity(nrow as usize + 1);
    lines.push(header);
    for row in 0..nrow {
        let row_name = format!("{:>row_width$}", row + 1);
        let cells = columns
            .iter()
            .zip(&widths)
            .map(|(col, width)| format!("{:>width$}", format_data_frame_cell(col.clone(), row)))
            .collect::<Vec<_>>()
            .join(" ");
        lines.push(format!("{row_name} {cells}"));
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
    if x.clone().len() == 0 {
        return "list()".to_string();
    }

    let names = list_names(x.clone());
    let mut sections = Vec::with_capacity(x.clone().len() as usize);
    for (index, elem) in x.iter_vector().enumerate() {
        sections.push(format!(
            "{}\n{}",
            list_element_header(index, &names),
            format_sexp_direct(elem)
        ));
    }
    sections.join("\n\n")
}

/// Format a value for top-level emission, excluding the caller-owned final
/// line terminator.
///
/// `printList()` emits a separator newline after every non-empty list
/// element, including the last one.  String contexts deliberately use
/// [`format_sexp_direct`] without that trailing separator, while both
/// `print()` and the REPL/script auto-print path need it before they append
/// their ordinary final newline.
pub(crate) fn format_sexp_top_level(x: Sexp<'_>) -> String {
    let mut rendered = format_sexp_direct(x.clone());
    if x.clone().typeof_() == SEXPTYPE::VECSXP
        && x.clone().len() != 0
        && format_data_frame(x).is_none()
    {
        rendered.push('\n');
    }
    rendered
}

fn deparse_expression_one(expr: SEXP) -> String {
    unsafe {
        let text = crate::mainutils::deparse::deparse1line(expr, false);
        if text.is_null() || XLENGTH(text) == 0 {
            return String::new();
        }
        let charsxp = STRING_ELT(text, 0);
        if charsxp.is_null() {
            return String::new();
        }
        let chars = CHAR(charsxp);
        if chars.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(chars)
                .to_string_lossy()
                .into_owned()
        }
    }
}

fn format_expression_vector(x: Sexp<'_>) -> String {
    unsafe {
        let raw = x.as_raw();
        let n = XLENGTH(raw);
        if n == 0 {
            return "expression()".to_string();
        }
        let parts = (0..n)
            .map(|i| deparse_expression_one(VECTOR_ELT(raw, i)))
            .collect::<Vec<_>>();
        format!("expression({})", parts.join(", "))
    }
}

// ---------------------------------------------------------------------------
// Faithful ports of stock printvector.c: printVector / printNamedVector.
//
// Field widths come from format.rs (formatRealS & co.) and each element is
// encoded through the printutils Encode* primitives at the COMMON width, so
// all elements of a vector share one number of decimals and one field width,
// exactly like stock. Lines wrap at options("width") with "[i]" index labels.
// ---------------------------------------------------------------------------

const OUT_DEC: *const std::os::raw::c_char = b".\0".as_ptr() as *const std::os::raw::c_char;

unsafe fn encode_cstr(p: *const std::os::raw::c_char) -> String {
    // SAFETY: this unsafe function requires `p` to be null or a live,
    // NUL-terminated C string for the duration of the conversion.
    unsafe {
        if p.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
}

/// (R_print.width, R_print.gap, R_print.max) from the current options.
fn vector_print_settings() -> (std::os::raw::c_int, std::os::raw::c_int, i64) {
    unsafe {
        let width = crate::mainutils::options::GetOptionWidth();
        let max = crate::mainutils::options::GetOptionMaxPrint();
        let gap = crate::mainutils::printutils::get_R_print().gap;
        (width, gap, max as i64)
    }
}

/// Stock VectorIndex: right-justify "[i]" in `labwidth` columns.
fn vector_index(i: R_xlen_t, labwidth: usize) -> String {
    format!("{:>labwidth$}", format!("[{i}]"))
}

/// The stock type-specific field width `w` (before the gap is added by the
/// callers that add it), computed over the first `n` elements.
unsafe fn type_field_width(
    raw: SEXP,
    tp: SEXPTYPE,
    n: R_xlen_t,
    quote: bool,
) -> (
    std::os::raw::c_int,
    std::os::raw::c_int,
    std::os::raw::c_int,
) {
    // SAFETY: callers provide a live atomic-vector SEXP whose runtime tag is
    // `tp`; the formatting routines only read it and write to stack locals.
    unsafe {
        // (w, d, e); only real/complex use d/e.
        let _ = quote;
        match tp {
            SEXPTYPE::LGLSXP => {
                let mut w = 0;
                crate::mainutils::format::formatLogicalS(raw, n, &mut w);
                (w, 0, 0)
            }
            SEXPTYPE::INTSXP => {
                let mut w = 0;
                crate::mainutils::format::formatIntegerS(raw, n, &mut w);
                (w, 0, 0)
            }
            SEXPTYPE::REALSXP => {
                let mut w = 0;
                let mut d = 0;
                let mut e = 0;
                crate::mainutils::format::formatRealS(raw, n, &mut w, &mut d, &mut e, 0);
                (w, d, e)
            }
            SEXPTYPE::CPLXSXP => {
                let mut wr = 0;
                let mut dr = 0;
                let mut er = 0;
                let mut wi = 0;
                let mut di = 0;
                let mut ei = 0;
                crate::mainutils::format::formatComplexS(
                    raw, n, &mut wr, &mut dr, &mut er, &mut wi, &mut di, &mut ei, 0,
                );
                (wr + wi + 2, dr, er)
            }
            SEXPTYPE::STRSXP => {
                let mut w = 0;
                crate::mainutils::format::formatStringS(
                    raw,
                    n,
                    &mut w,
                    quote as std::os::raw::c_int,
                );
                (w, 0, 0)
            }
            SEXPTYPE::RAWSXP => {
                let mut w = 0;
                crate::mainutils::format::formatRawS(raw, n, &mut w);
                (w, 0, 0)
            }
            _ => (0, 0, 0),
        }
    }
}

/// formatComplexS results, computed once per vector.
#[derive(Clone, Copy)]
struct ComplexFmt {
    wr: std::os::raw::c_int,
    dr: std::os::raw::c_int,
    er: std::os::raw::c_int,
    wi: std::os::raw::c_int,
    di: std::os::raw::c_int,
    ei: std::os::raw::c_int,
}

unsafe fn complex_fmt(raw: SEXP, n: R_xlen_t) -> ComplexFmt {
    // SAFETY: callers guarantee that `raw` is a live CPLXSXP with at least
    // `n` elements; all output pointers refer to initialized stack locals.
    unsafe {
        let (mut wr, mut dr, mut er, mut wi, mut di, mut ei) = (0, 0, 0, 0, 0, 0);
        crate::mainutils::format::formatComplexS(
            raw, n, &mut wr, &mut dr, &mut er, &mut wi, &mut di, &mut ei, 0,
        );
        ComplexFmt {
            wr,
            dr,
            er,
            wi,
            di,
            ei,
        }
    }
}

fn part_is_na(v: f64) -> bool {
    v.is_nan() && v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
}

fn part_is_nan(v: f64) -> bool {
    v.is_nan()
}

/// Encode element `i` at the common width, per stock print*Vector tight loops.
unsafe fn encode_element_at(
    raw: SEXP,
    tp: SEXPTYPE,
    i: R_xlen_t,
    w: std::os::raw::c_int,
    d: std::os::raw::c_int,
    e: std::os::raw::c_int,
    quote: bool,
    gap: std::os::raw::c_int,
    cfmt: ComplexFmt,
) -> String {
    // SAFETY: `encode_element_adj` inherits this function's live-SEXP,
    // matching-tag, and in-bounds-index requirements.
    unsafe {
        encode_element_adj(
            raw,
            tp,
            i,
            w,
            d,
            e,
            quote,
            gap,
            cfmt,
            crate::mainutils::printutils::Rprt_adj::right,
        )
    }
}

/// Like encode_element_at, with an explicit justification for STRSXP
/// elements: stock printVector left-adjusts (R_print.right = FALSE by
/// default), printNamedVector right-adjusts (Rprt_adj_right).
unsafe fn encode_element_adj(
    raw: SEXP,
    tp: SEXPTYPE,
    i: R_xlen_t,
    w: std::os::raw::c_int,
    d: std::os::raw::c_int,
    e: std::os::raw::c_int,
    quote: bool,
    gap: std::os::raw::c_int,
    cfmt: ComplexFmt,
    str_adj: crate::mainutils::printutils::Rprt_adj,
) -> String {
    // SAFETY: callers guarantee `raw` is live, `tp` is its actual runtime
    // tag, and `i` is in bounds; formatter-returned C strings are borrowed
    // only until the next formatter call and are copied immediately.
    unsafe {
        let i32i = i as std::os::raw::c_int;
        match tp {
            SEXPTYPE::LGLSXP => encode_cstr(crate::mainutils::printutils::EncodeLogical(
                crate::sexp::accessors::LOGICAL_ELT(raw, i32i),
                w,
            )),
            SEXPTYPE::INTSXP => encode_cstr(crate::mainutils::printutils::EncodeInteger(
                crate::sexp::accessors::INTEGER_ELT(raw, i32i),
                w,
            )),
            SEXPTYPE::REALSXP => encode_cstr(crate::mainutils::printutils::EncodeReal0(
                crate::sexp::accessors::REAL_ELT(raw, i32i),
                w,
                d,
                e,
                OUT_DEC,
            )),
            SEXPTYPE::CPLXSXP => {
                let c = crate::sexp::accessors::COMPLEX_ELT(raw, i32i);
                if part_is_na(c.r) || part_is_na(c.i) {
                    // stock: NA parts render as NA over the total width
                    encode_cstr(crate::mainutils::printutils::EncodeReal0(
                        crate::sexp::ffi::NA_REAL,
                        w,
                        0,
                        0,
                        OUT_DEC,
                    ))
                } else {
                    encode_cstr(crate::mainutils::printutils::EncodeComplex(
                        c,
                        cfmt.wr + gap,
                        cfmt.dr,
                        cfmt.er,
                        cfmt.wi,
                        cfmt.di,
                        cfmt.ei,
                        OUT_DEC,
                    ))
                }
            }
            SEXPTYPE::STRSXP => {
                let quote_ch = if quote {
                    b'"' as std::os::raw::c_int
                } else {
                    0
                };
                encode_cstr(crate::mainutils::printutils::EncodeString(
                    crate::sexp::accessors::STRING_ELT(raw, i),
                    w,
                    quote_ch,
                    str_adj,
                ))
            }
            SEXPTYPE::RAWSXP => encode_cstr(crate::mainutils::printutils::EncodeRaw(
                crate::sexp::accessors::RAW_ELT(raw, i32i),
                std::ptr::null(),
            )),
            _ => String::new(),
        }
    }
}

/// Stock printVector: unnamed vector with "[i]" index labels, wrapping at
/// options("width"). Every element occupies the common field width.
pub(crate) unsafe fn print_vector_stock(x: Sexp, quote: bool, n_pr: R_xlen_t) -> String {
    // SAFETY: `x` is a rooted live SEXP and `n_pr` is bounded by its length;
    // all raw access remains read-only for the duration of this call.
    unsafe {
        let raw = x.clone().as_raw();
        let tp = x.typeof_();
        let (print_width, gap, _max) = vector_print_settings();
        let (mut w, d, e) = type_field_width(raw, tp, n_pr, quote);
        let mut out = String::new();
        let labwidth = crate::mainutils::printutils::IndexWidth_xlen(n_pr) + 2;
        let mut width = labwidth;
        out.push_str(&vector_index(1, labwidth as usize));
        if !matches!(tp, SEXPTYPE::STRSXP | SEXPTYPE::RAWSXP) {
            w += gap;
        }
        let is_str = matches!(tp, SEXPTYPE::STRSXP);
        let gap_prefixed = is_str || tp == SEXPTYPE::RAWSXP;
        let cfmt = if tp == SEXPTYPE::CPLXSXP {
            complex_fmt(raw, n_pr)
        } else {
            ComplexFmt {
                wr: 0,
                dr: 0,
                er: 0,
                wi: 0,
                di: 0,
                ei: 0,
            }
        };
        for i in 0..n_pr {
            if i > 0 {
                // stock wrap conditions: char adds the gap to the check and to
                // the accumulated width; raw prefixes the gap but counts only w.
                let wrap = if is_str {
                    width + w + gap > print_width
                } else {
                    width + w > print_width
                };
                if wrap {
                    out.push('\n');
                    out.push_str(&vector_index(i + 1, labwidth as usize));
                    width = labwidth;
                }
            }
            if gap_prefixed {
                out.push_str(&" ".repeat(gap as usize));
            }
            out.push_str(&encode_element_adj(
                raw,
                tp,
                i,
                w,
                d,
                e,
                quote,
                gap,
                cfmt,
                crate::mainutils::printutils::Rprt_adj::left,
            ));
            width += if is_str { w + gap } else { w };
        }
        out.push('\n');
        out
    }
}
/// Stock printNamedVector: names line(s) right-justified over the value
/// column(s), every column at the common width `w`, gap-separated.
pub(crate) unsafe fn print_named_vector_stock(
    x: Sexp,
    names: Sexp,
    quote: bool,
    n_pr: R_xlen_t,
) -> String {
    // SAFETY: `x` and `names` are rooted live SEXPs with compatible lengths;
    // element access is read-only and bounded by `n_pr`.
    unsafe {
        let raw = x.clone().as_raw();
        let names_raw = names.as_raw();
        let tp = x.typeof_();
        let (print_width, gap, _max) = vector_print_settings();
        let (mut w, d, e) = type_field_width(raw, tp, n_pr, quote);
        let mut wn = 0;
        let cfmt0 = if tp == SEXPTYPE::CPLXSXP {
            complex_fmt(raw, n_pr)
        } else {
            ComplexFmt {
                wr: 0,
                dr: 0,
                er: 0,
                wi: 0,
                di: 0,
                ei: 0,
            }
        };
        crate::mainutils::format::formatStringS(names_raw, n_pr, &mut wn, 0);
        if w < wn {
            w = wn;
        }
        let nperline = ((print_width / (w + gap)).max(1)) as R_xlen_t;
        let nlines = n_pr / nperline + R_xlen_t::from(n_pr % nperline != 0);
        let mut out = String::new();
        for i in 0..nlines {
            if i != 0 {
                out.push('\n');
            }
            for j in 0..nperline {
                let k = i * nperline + j;
                if k >= n_pr {
                    break;
                }
                out.push_str(&encode_cstr(crate::mainutils::printutils::EncodeString(
                    crate::sexp::accessors::STRING_ELT(names_raw, k),
                    w,
                    0,
                    crate::mainutils::printutils::Rprt_adj::right,
                )));
                out.push_str(&" ".repeat(gap as usize));
            }
            out.push('\n');
            for j in 0..nperline {
                let k = i * nperline + j;
                if k >= n_pr {
                    break;
                }
                let i32k = k as std::os::raw::c_int;
                if matches!(tp, SEXPTYPE::CPLXSXP) {
                    if j != 0 {
                        out.push_str(&" ".repeat(gap as usize));
                    }
                    let c = crate::sexp::accessors::COMPLEX_ELT(raw, i32k);
                    if part_is_na(c.r) || part_is_na(c.i) {
                        out.push_str(&encode_cstr(crate::mainutils::printutils::EncodeReal0(
                            crate::sexp::ffi::NA_REAL,
                            w,
                            0,
                            0,
                            OUT_DEC,
                        )));
                    } else {
                        out.push_str(&encode_cstr(crate::mainutils::printutils::EncodeReal0(
                            c.r, cfmt0.wr, cfmt0.dr, cfmt0.er, OUT_DEC,
                        )));
                        if part_is_nan(c.i) {
                            out.push_str("+NaNi");
                        } else if c.i >= 0.0 {
                            out.push('+');
                            out.push_str(&encode_cstr(crate::mainutils::printutils::EncodeReal0(
                                c.i, cfmt0.wi, cfmt0.di, cfmt0.ei, OUT_DEC,
                            )));
                            out.push('i');
                        } else {
                            out.push('-');
                            out.push_str(&encode_cstr(crate::mainutils::printutils::EncodeReal0(
                                -c.i, cfmt0.wi, cfmt0.di, cfmt0.ei, OUT_DEC,
                            )));
                            out.push('i');
                        }
                    }
                } else if matches!(tp, SEXPTYPE::RAWSXP) {
                    // stock: "%*s%s%*s" with w-2, raw, gap
                    out.push_str(&" ".repeat((w - 2).max(0) as usize));
                    out.push_str(&encode_element_at(raw, tp, k, w, d, e, quote, gap, cfmt0));
                    out.push_str(&" ".repeat(gap as usize));
                } else {
                    let str_quote = if matches!(tp, SEXPTYPE::STRSXP) {
                        quote
                    } else {
                        false
                    };
                    out.push_str(&encode_element_at(
                        raw, tp, k, w, d, e, str_quote, gap, cfmt0,
                    ));
                    out.push_str(&" ".repeat(gap as usize));
                }
            }
        }
        out.push('\n');
        out
    }
}

/// Render an atomic vector exactly like stock print.default: named vectors go
/// through printNamedVector, others through printVector with index labels;
/// both honour options("max.print") truncation.
pub(crate) unsafe fn format_vector_stock(x: Sexp, quote: bool) -> String {
    // SAFETY: `x` is a rooted live atomic-vector SEXP; the delegated stock
    // formatters preserve that lifetime and perform bounded read-only access.
    unsafe {
        let n = x.clone().len();
        if n == 0 {
            // stock printVector PRINT_V_0. Callers own the final newline,
            // exactly like the non-empty paths (whose closing newline is
            // popped below).
            return match x.typeof_() {
                SEXPTYPE::LGLSXP => "logical(0)".to_string(),
                SEXPTYPE::INTSXP => "integer(0)".to_string(),
                SEXPTYPE::REALSXP => "numeric(0)".to_string(),
                SEXPTYPE::CPLXSXP => "complex(0)".to_string(),
                SEXPTYPE::STRSXP => "character(0)".to_string(),
                SEXPTYPE::RAWSXP => "raw(0)".to_string(),
                _ => String::new(),
            };
        }
        let (_width, _gap, max) = vector_print_settings();
        let n_pr = if n <= (max + 1) as R_xlen_t {
            n
        } else {
            max as R_xlen_t
        };
        let mut out = match names_sexp(x.clone()) {
            Some(names) => print_named_vector_stock(x, names, quote, n_pr),
            None => print_vector_stock(x, quote, n_pr),
        };
        if n_pr < n {
            out.push_str(&format!(
                " [ reached 'max' / getOption(\"max.print\") -- omitted {} entries ]\n",
                n - n_pr
            ));
        }
        // Callers own the final newline (print_value appends it; string contexts
        // embed the rendering without one).
        if out.ends_with('\n') {
            out.pop();
        }
        out
    }
}

/// The raw names attribute when it is a character vector of full length.
fn names_sexp(x: Sexp<'_>) -> Option<Sexp<'_>> {
    unsafe {
        let names = crate::sexp::attrib_core::getAttrib(
            x.clone().as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        let names = Sexp::from_raw(names)?;
        if names.clone().typeof_() == SEXPTYPE::STRSXP && names.clone().len() == x.len() {
            Some(names)
        } else {
            None
        }
    }
}

/// Print an R object to the captured output (or stdout if not capturing).
///
/// This is the Rust implementation of R's Rf_PrintValue. For Android
/// embedding, use [`start_capture`] before evaluation and [`stop_capture`]
/// after to collect printed output as a string.
pub fn print_value(x: Sexp<'_>) {
    // Objects of class "try-error" print per stock print.default: the
    // message string as a character vector plus class/condition attrs.
    // Condition objects print via print.condition.
    if has_class(x.clone(), "try-error") {
        unsafe { emit(&format!("{}\n", format_try_error(x))) };
        return;
    }
    if has_class(x.clone(), "condition") {
        unsafe { emit(&format!("{}\n", format_condition(x))) };
        return;
    }
    match x.clone().typeof_() {
        SEXPTYPE::SYMSXP
        | SEXPTYPE::LANGSXP
        | SEXPTYPE::CLOSXP
        | SEXPTYPE::SPECIALSXP
        | SEXPTYPE::BUILTINSXP => {
            // print.c PrintValueRec deparses these language objects: a
            // closure prints as `function (x) ...`, a primitive as
            // `.Primitive("sin")`.
            emit(&format!("{}\n", deparse_expression_one(x.as_raw())));
        }
        SEXPTYPE::NILSXP => {
            emit("NULL\n");
        }
        SEXPTYPE::INTSXP => {
            if x.clone().len() == 0 {
                let empty = if has_names_attribute(x.clone()) {
                    "named integer(0)"
                } else {
                    "integer(0)"
                };
                emit(&format!(
                    "{}\n",
                    format_with_printable_attributes(empty.to_string(), x)
                ));
                return;
            }
            if let Some(output) = format_matrix(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            if let Some(output) = format_summary_default(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            if let Some(output) = format_table(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            if let Some(output) = format_factor(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            emit(&format!("{}\n", format_with_printable_attributes(base, x)));
        }
        SEXPTYPE::REALSXP => {
            if x.clone().len() == 0
                && !has_class(x.clone(), "difftime")
                && !has_class(x.clone(), "POSIXct")
                && !has_class(x.clone(), "Date")
            {
                emit(&format!(
                    "{}\n",
                    format_with_printable_attributes("numeric(0)".to_string(), x)
                ));
                return;
            }
            if let Some(output) = format_matrix(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            if let Some(output) = format_summary_default(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            if let Some(output) = format_table(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            if has_class(x.clone(), "difftime") {
                emit(&format!("{}\n", format_difftime_vector(x)));
                return;
            }
            if has_class(x.clone(), "POSIXct") {
                emit(&format!("{}\n", format_posixct_vector(x, true)));
                return;
            }
            if has_class(x.clone(), "Date") {
                emit(&format!("{}\n", format_date_vector(x)));
                return;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            emit(&format!("{}\n", format_with_printable_attributes(base, x)));
        }
        SEXPTYPE::LGLSXP => {
            if x.clone().len() == 0 {
                emit(&format!(
                    "{}\n",
                    format_with_printable_attributes("logical(0)".to_string(), x)
                ));
                return;
            }
            if let Some(output) = format_matrix(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            emit(&format!("{}\n", format_with_printable_attributes(base, x)));
        }
        SEXPTYPE::CPLXSXP => {
            if x.clone().len() == 0 {
                emit(&format!(
                    "{}\n",
                    format_with_printable_attributes("complex(0)".to_string(), x)
                ));
                return;
            }
            if let Some(output) = format_matrix(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            emit(&format!("{}\n", format_with_printable_attributes(base, x)));
        }
        SEXPTYPE::STRSXP => {
            if let Some(output) = format_matrix(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            if let Some(output) = format_summary_default(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            emit(&format!("{}\n", format_with_printable_attributes(base, x)));
        }
        SEXPTYPE::RAWSXP => {
            if x.clone().len() == 0 {
                emit(&format!(
                    "{}\n",
                    format_with_printable_attributes("raw(0)".to_string(), x)
                ));
                return;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            emit(&format!("{}\n", format_with_printable_attributes(base, x)));
        }
        SEXPTYPE::VECSXP => {
            if let Some(output) = format_data_frame(x.clone()) {
                emit(&format!("{output}\n"));
                return;
            }
            emit(&format!("{}\n", format_sexp_top_level(x)));
        }
        SEXPTYPE::EXPRSXP => {
            emit(&format!("{}\n", format_expression_vector(x)));
        }
        SEXPTYPE::ENVSXP => {
            emit(&format!("{}\n", format_environment(x)));
        }
        tp => {
            let type_name = match tp {
                SEXPTYPE::RAWSXP => "raw",
                SEXPTYPE::CPLXSXP => "complex",
                SEXPTYPE::SYMSXP => "symbol",
                SEXPTYPE::CLOSXP => "closure",
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
    if has_class(x.clone(), "try-error") {
        return unsafe { format_try_error(x) };
    }
    if has_class(x.clone(), "condition") {
        return unsafe { format_condition(x) };
    }
    match x.clone().typeof_() {
        SEXPTYPE::NILSXP => "NULL".to_string(),
        SEXPTYPE::INTSXP => {
            if x.clone().len() == 0 {
                let empty = if has_names_attribute(x.clone()) {
                    "named integer(0)"
                } else {
                    "integer(0)"
                };
                return format_with_printable_attributes(empty.to_string(), x);
            }
            if let Some(output) = format_matrix(x.clone()) {
                return output;
            }
            if let Some(output) = format_factor(x.clone()) {
                return output;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            format_with_printable_attributes(base, x)
        }
        SEXPTYPE::REALSXP => {
            if has_class(x.clone(), "difftime") {
                return format_difftime_vector(x);
            }
            if has_class(x.clone(), "POSIXct") {
                return format_posixct_vector(x, true);
            }
            if has_class(x.clone(), "Date") {
                return format_date_vector(x);
            }
            if x.clone().len() == 0 {
                return format_with_printable_attributes("numeric(0)".to_string(), x);
            }
            if let Some(output) = format_matrix(x.clone()) {
                return output;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            format_with_printable_attributes(base, x)
        }
        SEXPTYPE::LGLSXP => {
            if x.clone().len() == 0 {
                return format_with_printable_attributes("logical(0)".to_string(), x);
            }
            if let Some(output) = format_matrix(x.clone()) {
                return output;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            format_with_printable_attributes(base, x)
        }
        SEXPTYPE::CPLXSXP => {
            if x.clone().len() == 0 {
                return format_with_printable_attributes("complex(0)".to_string(), x);
            }
            if let Some(output) = format_matrix(x.clone()) {
                return output;
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            format_with_printable_attributes(base, x)
        }
        SEXPTYPE::STRSXP => {
            let base = unsafe { format_vector_stock(x.clone(), true) };
            format_with_printable_attributes(base, x)
        }
        SEXPTYPE::RAWSXP => {
            if x.clone().len() == 0 {
                return format_with_printable_attributes("raw(0)".to_string(), x);
            }
            let base = unsafe { format_vector_stock(x.clone(), true) };
            format_with_printable_attributes(base, x)
        }
        SEXPTYPE::VECSXP => format_list(x),
        SEXPTYPE::EXPRSXP => format_expression_vector(x),
        SEXPTYPE::SYMSXP
        | SEXPTYPE::LANGSXP
        | SEXPTYPE::CLOSXP
        | SEXPTYPE::SPECIALSXP
        | SEXPTYPE::BUILTINSXP => deparse_expression_one(x.as_raw()),
        SEXPTYPE::ENVSXP => format_environment(x),
        tp => {
            let type_name = match tp {
                SEXPTYPE::RAWSXP => "raw",
                SEXPTYPE::CPLXSXP => "complex",
                SEXPTYPE::CLOSXP => "closure",
                SEXPTYPE::LISTSXP => "pairlist",
                SEXPTYPE::CHARSXP => "charsxp",
                _ => "unknown",
            };
            format!("[{}; length={}]", type_name, x.len())
        }
    }
}
/// First class string of an object, for print.condition-style rendering.
unsafe fn first_class_string(x: Sexp<'_>) -> Option<String> {
    unsafe {
        let klass = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        let klass = Sexp::from_raw(klass)?;
        if klass.clone().typeof_() != SEXPTYPE::STRSXP || klass.clone().len() == 0 {
            return None;
        }
        let s = crate::sexp::accessors::STRING_ELT(klass.as_raw(), 0);
        if s.is_null() {
            return None;
        }
        let chars = crate::sexp::accessors::CHAR(s);
        if chars.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr(chars)
            .to_str()
            .ok()
            .map(str::to_string)
    }
}

/// Stock print.condition: `<class: msg>` or `<class in <deparsed call>: msg>`.
/// Conditions are `list(message, call, ...)` per R_makeErrorCondition.
unsafe fn format_condition(x: Sexp<'_>) -> String {
    // SAFETY: callers provide a rooted live condition object. Each optional
    // list/string field is tag-checked before raw element access.
    unsafe {
        let raw = x.clone().as_raw();
        let class = first_class_string(x.clone()).unwrap_or_else(|| "condition".to_string());
        let mut message = String::new();
        let mut call_text = String::new();
        if x.clone().typeof_() == SEXPTYPE::VECSXP && x.clone().len() >= 1 {
            let msg = crate::sexp::accessors::VECTOR_ELT(raw, 0);
            if !msg.is_null() && msg != crate::sexp::globals::R_NilValue() {
                if let Some(sexp) = Sexp::from_raw(msg) {
                    if sexp.clone().typeof_() == SEXPTYPE::STRSXP && sexp.clone().len() >= 1 {
                        let elt = crate::sexp::accessors::STRING_ELT(sexp.as_raw(), 0);
                        if !elt.is_null() {
                            let chars = crate::sexp::accessors::CHAR(elt);
                            if !chars.is_null() {
                                if let Ok(s) = std::ffi::CStr::from_ptr(chars).to_str() {
                                    message = s.to_string();
                                }
                            }
                        }
                    }
                }
            }
            if x.len() >= 2 {
                let call = crate::sexp::accessors::VECTOR_ELT(raw, 1);
                if !call.is_null() && call != crate::sexp::globals::R_NilValue() {
                    let dcall = crate::mainutils::deparse::deparse1s(call);
                    if !dcall.is_null() && dcall != crate::sexp::globals::R_NilValue() {
                        let elt = crate::sexp::accessors::STRING_ELT(dcall, 0);
                        if !elt.is_null() {
                            let chars = crate::sexp::accessors::CHAR(elt);
                            if !chars.is_null() {
                                if let Ok(s) = std::ffi::CStr::from_ptr(chars).to_str() {
                                    call_text = format!(" in {s}");
                                }
                            }
                        }
                    }
                }
            }
        }
        format!("<{class}{call_text}: {message}>")
    }
}

/// Stock print.default on a try-error object: the message string rendered as
/// a character vector, then the class and condition attributes.
unsafe fn format_try_error(x: Sexp<'_>) -> String {
    // SAFETY: callers provide a rooted live try-error object; attribute and
    // vector reads remain within the lifetime carried by `x`.
    unsafe {
        let mut out = if x.clone().typeof_() == SEXPTYPE::STRSXP {
            format_vector_stock(x.clone(), true)
        } else {
            format!("{}\n", format_sexp_direct(x.clone()))
        };
        out.push('\n');
        unsafe {
            let klass = crate::sexp::attrib_core::getAttrib(
                x.clone().as_raw(),
                crate::sexp::attrib_core::R_ClassSymbol(),
            );
            let has_condition = {
                let cond_sym = crate::sexp::symbol::Rf_install(
                    b"condition\0".as_ptr() as *const std::os::raw::c_char
                );
                let cond = crate::sexp::attrib_core::getAttrib(x.clone().as_raw(), cond_sym);
                !cond.is_null() && cond != R_NilValue()
            };
            if let Some(klass) = Sexp::from_raw(klass) {
                if klass.clone().typeof_() == SEXPTYPE::STRSXP {
                    out.push_str("attr(,\"class\")\n");
                    out.push_str(&format_vector_stock(klass, true));
                    // print_value owns the final newline; add the separator
                    // between attribute sections here.
                    if has_condition {
                        out.push('\n');
                    }
                }
            }
            let cond_sym = crate::sexp::symbol::Rf_install(
                b"condition\0".as_ptr() as *const std::os::raw::c_char
            );
            let cond = crate::sexp::attrib_core::getAttrib(x.as_raw(), cond_sym);
            if let Some(cond) = Sexp::from_raw(cond) {
                if cond.clone().typeof_() != SEXPTYPE::NILSXP {
                    out.push_str("attr(,\"condition\")\n");
                    out.push_str(&format_condition(cond));
                }
            }
        }
        out
    }
}

fn format_environment(x: Sexp<'_>) -> String {
    let raw = x.as_raw();
    let name = unsafe {
        if raw == crate::sexp::globals::R_GlobalEnv() {
            "R_GlobalEnv".to_string()
        } else if raw == crate::sexp::globals::R_BaseEnv() {
            "base".to_string()
        } else if raw == crate::sexp::globals::R_EmptyEnv() {
            "R_EmptyEnv".to_string()
        } else {
            format!("{raw:p}")
        }
    };
    format!("<environment: {name}>")
}

/// Print an R object's structure (like str()).
pub fn print_structure(x: Sexp<'_>, indent: usize) {
    let prefix = "  ".repeat(indent);

    match x.clone().typeof_() {
        SEXPTYPE::INTSXP => {
            let vals: Vec<_> = x.clone().iter_integer().take(10).collect();
            let suffix = if x.clone().len() > 10 { ", ..." } else { "" };
            let output = format!("{}int [{}]: {:?}{}", prefix, x.len(), vals, suffix);
            if is_capturing() {
                capture_stdout(&output);
                capture_stdout("\n");
            } else {
                println!("{}", output);
            }
        }
        SEXPTYPE::REALSXP => {
            let vals: Vec<_> = x.clone().iter_real().take(10).collect();
            let suffix = if x.clone().len() > 10 { ", ..." } else { "" };
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
            let output = format!("{}list [{}]", prefix, x.clone().len());
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
#[allow(deprecated)] // translated tests exercise the Sexp compat setters
mod tests {
    use super::*;
    use crate::sexp::instance::RInstance;
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
    fn test_capture_can_target_instance_explicitly() {
        let mut left = RInstance::new();
        let mut right = RInstance::new();

        start_capture_in(&mut left);
        capture_stdout_in(&mut left, "left");
        capture_stderr_in(&mut left, "left err");
        assert!(is_capturing_in(&mut left));
        assert!(!is_capturing_in(&mut right));

        start_capture_in(&mut right);
        capture_stdout_in(&mut right, "right");
        let right_output = stop_capture_in(&mut right);
        assert_eq!(right_output.stdout, "right");
        assert_eq!(right_output.stderr, "");

        let left_output = stop_capture_in(&mut left);
        assert_eq!(left_output.stdout, "left");
        assert_eq!(left_output.stderr, "left err");
    }

    #[test]
    fn test_print_logical_vector() {
        let mut session = RSession::new();
        session
            .with_arena(|arena| {
                let ptr = arena.alloc_vector(SEXPTYPE::LGLSXP, 3);
                let sexp = Sexp::from_raw(ptr).expect("logical vector allocation failed");
                assert!(sexp.clone().set_logical_elt(0, 0));
                assert!(sexp.clone().set_logical_elt(1, 1));
                assert!(
                    sexp.clone()
                        .set_logical_elt(2, crate::sexp::ffi::NA_LOGICAL)
                );

                start_capture();
                print_value(sexp);
                let output = stop_capture();
                assert_eq!(output.stdout, "[1] FALSE  TRUE    NA\n");
            })
            .unwrap();
    }

    #[test]
    fn test_print_named_list_keeps_final_stock_separator_line() {
        let mut session = RSession::new();
        let (result, output, _) =
            session.eval_code_with_output_capture("print(list(class = 'ts'))");

        assert!(result.is_ok(), "named list should evaluate: {result:?}");
        assert_eq!(output.stdout, "$class\n[1] \"ts\"\n\n");
    }

    #[test]
    fn test_string_output_uses_safe_charsxp_access_and_preserves_na() {
        let mut session = RSession::new();
        session
            .with_arena(|arena| {
                let ptr = arena.alloc_vector(SEXPTYPE::STRSXP, 2);
                let sexp = Sexp::from_raw(ptr).expect("string vector allocation failed");
                let value = Sexp::from_raw(arena.alloc_charsxp(b"a")).expect("CHARSXP");
                let missing = Sexp::from_raw(unsafe { crate::sexp::globals::R_NaString() })
                    .expect("NA_STRING");
                sexp.clone()
                    .try_set_string_elt(0, value)
                    .expect("set string");
                sexp.clone()
                    .try_set_string_elt(1, missing)
                    .expect("set string");

                assert_eq!(format_sexp_direct(sexp.clone()), "[1] \"a\" NA ");

                start_capture();
                print_value(sexp);
                let output = stop_capture();
                // Stock R pads the final NA to the field width even in last
                // position (verified against R: `print(c("a", NA))` emits
                // "[1] \"a\" NA \n" with the trailing space).
                assert_eq!(output.stdout, "[1] \"a\" NA \n");
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

                assert!(
                    format_integer_element(real.clone(), 0).contains("expected integer vector")
                );
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
                sexp.clone().try_set_real_elt(0, 200.0).expect("set real");
                sexp.clone().try_set_real_elt(1, 80200.0).expect("set real");
                sexp.clone().try_set_real_elt(2, 100.5).expect("set real");

                assert_eq!(
                    format_sexp_direct(sexp.clone()),
                    "[1]   200.0 80200.0   100.5"
                );

                start_capture();
                print_value(sexp);
                let output = stop_capture();
                assert_eq!(output.stdout, "[1]   200.0 80200.0   100.5\n");
            })
            .unwrap();
    }
}
