//! R output capture for embedding.
//!
//! Captures Rprintf, REprintf, and other R output functions
//! so they can be returned to the caller instead of printing
//! to stdout/stderr.

use std::cell::{Cell, RefCell};

use super::ffi::{NA_INTEGER, R_IsNA, R_IsNaN, SEXP, SEXPTYPE};
use super::safe::Sexp;

/// Captured R output.
#[derive(Debug, Clone, Default)]
pub struct RCapturedOutput {
    pub stdout: String,
    pub stderr: String,
}

thread_local! {
    static CAPTURE_STDOUT: RefCell<Option<String>> = const { RefCell::new(None) };
    static CAPTURE_STDERR: RefCell<Option<String>> = const { RefCell::new(None) };
    static IS_CAPTURING: Cell<bool> = const { Cell::new(false) };
}

/// Start capturing R output.
pub fn start_capture() {
    if super::instance::with_current_instance(|inst| {
        inst.capture_stdout = Some(String::new());
        inst.capture_stderr = Some(String::new());
    })
    .is_some()
    {
        return;
    }
    CAPTURE_STDOUT.with(|c| *c.borrow_mut() = Some(String::new()));
    CAPTURE_STDERR.with(|c| *c.borrow_mut() = Some(String::new()));
    IS_CAPTURING.with(|c| c.set(true));
}

/// Stop capturing and return the captured output.
pub fn stop_capture() -> RCapturedOutput {
    if let Some(output) = super::instance::with_current_instance(|inst| RCapturedOutput {
        stdout: inst.capture_stdout.take().unwrap_or_default(),
        stderr: inst.capture_stderr.take().unwrap_or_default(),
    }) {
        return output;
    }
    let stdout = CAPTURE_STDOUT.with(|c| c.borrow_mut().take().unwrap_or_default());
    let stderr = CAPTURE_STDERR.with(|c| c.borrow_mut().take().unwrap_or_default());
    IS_CAPTURING.with(|c| c.set(false));
    RCapturedOutput { stdout, stderr }
}

/// Check if output capture is active.
pub fn is_capturing() -> bool {
    if let Some(is_capturing) = super::instance::with_current_instance(|inst| {
        inst.capture_stdout.is_some() || inst.capture_stderr.is_some()
    }) {
        return is_capturing;
    }
    IS_CAPTURING.with(|c| c.get())
}

/// Append to captured stdout. Called by the Rprintf hook.
pub fn capture_stdout(msg: &str) {
    if is_capturing() {
        if super::instance::with_current_instance(|inst| {
            if let Some(s) = inst.capture_stdout.as_mut() {
                s.push_str(msg);
            }
        })
        .is_some()
        {
            return;
        }
        CAPTURE_STDOUT.with(|c| {
            if let Some(s) = c.borrow_mut().as_mut() {
                s.push_str(msg);
            }
        });
    }
}

/// Append to captured stderr. Called by the REprintf hook.
pub fn capture_stderr(msg: &str) {
    if is_capturing() {
        if super::instance::with_current_instance(|inst| {
            if let Some(s) = inst.capture_stderr.as_mut() {
                s.push_str(msg);
            }
        })
        .is_some()
        {
            return;
        }
        CAPTURE_STDERR.with(|c| {
            if let Some(s) = c.borrow_mut().as_mut() {
                s.push_str(msg);
            }
        });
    }
}

pub fn format_sexp(x: SEXP) -> String {
    if x.is_null() {
        return "NULL".to_string();
    }
    if let Some(sexp) = crate::sexp::safe::Sexp::from_raw(x) {
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

fn format_matrix_with<F>(nrow: usize, ncol: usize, value_at: F) -> String
where
    F: Fn(usize, usize) -> String,
{
    let mut values = vec![vec![String::new(); ncol]; nrow];
    let mut widths = Vec::with_capacity(ncol);
    for c in 0..ncol {
        let mut width = format!("[,{}]", c + 1).len().max(4);
        for r in 0..nrow {
            let value = value_at(r, c);
            width = width.max(value.len());
            values[r][c] = value;
        }
        widths.push(width);
    }

    let mut lines = Vec::with_capacity(nrow + 1);
    let mut header = format!("{:5}", "");
    for (c, width) in widths.iter().enumerate() {
        header.push_str(&format!("{:>width$}", format!("[,{}]", c + 1)));
        if c + 1 < ncol {
            header.push(' ');
        }
    }
    lines.push(header);

    for r in 0..nrow {
        let mut line = format!("{:<5}", format!("[{},]", r + 1));
        for (c, width) in widths.iter().enumerate() {
            line.push_str(&format!("{:>width$}", values[r][c]));
            if c + 1 < ncol {
                line.push(' ');
            }
        }
        lines.push(line);
    }
    lines.join("\n")
}

fn format_matrix(x: Sexp<'_>) -> Option<String> {
    let (nrow, ncol) = matrix_dims(x)?;
    match x.typeof_() {
        SEXPTYPE::INTSXP => Some(format_matrix_with(nrow, ncol, |r, c| {
            format_integer_value(x.integer_elt((r + c * nrow) as i64).unwrap_or(NA_INTEGER))
        })),
        SEXPTYPE::REALSXP => Some(format_matrix_with(nrow, ncol, |r, c| {
            format_real_value(x.real_elt((r + c * nrow) as i64).unwrap_or(f64::NAN))
        })),
        SEXPTYPE::LGLSXP => Some(format_matrix_with(nrow, ncol, |r, c| {
            format_logical_value(x.logical_elt((r + c * nrow) as i64).unwrap_or(NA_INTEGER))
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
        let value = sexp
            .string_elt(i)
            .and_then(|charsxp| charsxp.as_str())
            .unwrap_or("")
            .to_string();
        values.push(value);
    }
    Some(values)
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

fn list_names(x: Sexp<'_>) -> Vec<String> {
    unsafe {
        let names = crate::sexp::attrib_core::getAttrib(
            x.as_raw(),
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        string_vector_values(names).unwrap_or_default()
    }
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
            if let Some(output) = format_factor(x) {
                emit(&format!("{output}\n"));
                return;
            }
            if x.len() == 1 {
                let v = x.integer_elt(0).unwrap_or(0);
                emit(&format!("[1] {}\n", format_integer_value(v)));
            } else {
                let vals: Vec<String> = x
                    .iter_integer()
                    .take(10)
                    .map(format_integer_value)
                    .collect();
                let suffix = if x.len() > 10 { " ..." } else { "" };
                emit(&format!("[1] {}{}\n", format_aligned_values(vals), suffix));
            }
        }
        SEXPTYPE::REALSXP => {
            if let Some(output) = format_matrix(x) {
                emit(&format!("{output}\n"));
                return;
            }
            if x.len() == 1 {
                let v = x.real_elt(0).unwrap_or(0.0);
                emit(&format!("[1] {}\n", format_real_value(v)));
            } else {
                let vals: Vec<String> = x.iter_real().take(10).map(format_real_value).collect();
                let suffix = if x.len() > 10 { " ..." } else { "" };
                emit(&format!("[1] {}{}\n", format_aligned_values(vals), suffix));
            }
        }
        SEXPTYPE::LGLSXP => {
            if let Some(output) = format_matrix(x) {
                emit(&format!("{output}\n"));
                return;
            }
            let vals: Vec<String> = x
                .iter_logical()
                .take(10)
                .map(format_logical_value)
                .collect();
            let suffix = if x.len() > 10 { " ..." } else { "" };
            emit(&format!("[1] {}{}\n", format_aligned_values(vals), suffix));
        }
        SEXPTYPE::STRSXP => {
            if x.len() == 1 {
                if let Some(charsxp) = x.string_elt(0) {
                    let raw = unsafe { super::accessors::CHAR(charsxp.as_raw()) };
                    let s = if raw.is_null() {
                        ""
                    } else {
                        unsafe { std::ffi::CStr::from_ptr(raw).to_str().unwrap_or("") }
                    };
                    emit(&format!("[1] \"{}\"\n", s));
                } else {
                    emit("[1] \"\"\n");
                }
            } else {
                let vals: Vec<String> = (0..x.len().min(10))
                    .map(|i| {
                        if let Some(charsxp) = x.string_elt(i) {
                            let raw = unsafe { super::accessors::CHAR(charsxp.as_raw()) };
                            if raw.is_null() {
                                "\"\"".to_string()
                            } else {
                                format!("\"{}\"", unsafe {
                                    std::ffi::CStr::from_ptr(raw).to_str().unwrap_or("")
                                })
                            }
                        } else {
                            "\"\"".to_string()
                        }
                    })
                    .collect();
                let suffix = if x.len() > 10 { " ..." } else { "" };
                emit(&format!("[1] {}{}\n", vals.join(" "), suffix));
            }
        }
        SEXPTYPE::VECSXP => {
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
            if let Some(output) = format_matrix(x) {
                return output;
            }
            if let Some(output) = format_factor(x) {
                return output;
            }
            if x.len() == 1 {
                let v = x.integer_elt(0).unwrap_or(0);
                format!("[1] {}", format_integer_value(v))
            } else {
                let vals: Vec<String> = x
                    .iter_integer()
                    .take(10)
                    .map(format_integer_value)
                    .collect();
                let suffix = if x.len() > 10 { " ..." } else { "" };
                format!("[1] {}{}", format_aligned_values(vals), suffix)
            }
        }
        SEXPTYPE::REALSXP => {
            if let Some(output) = format_matrix(x) {
                return output;
            }
            if x.len() == 1 {
                let v = x.real_elt(0).unwrap_or(0.0);
                format!("[1] {}", format_real_value(v))
            } else {
                let vals: Vec<String> = x.iter_real().take(10).map(format_real_value).collect();
                let suffix = if x.len() > 10 { " ..." } else { "" };
                format!("[1] {}{}", format_aligned_values(vals), suffix)
            }
        }
        SEXPTYPE::LGLSXP => {
            if let Some(output) = format_matrix(x) {
                return output;
            }
            let vals: Vec<String> = x
                .iter_logical()
                .take(10)
                .map(format_logical_value)
                .collect();
            let suffix = if x.len() > 10 { " ..." } else { "" };
            format!("[1] {}{}", format_aligned_values(vals), suffix)
        }
        SEXPTYPE::STRSXP => {
            if x.len() == 1 {
                if let Some(charsxp) = x.string_elt(0) {
                    let s = charsxp.as_str().unwrap_or("");
                    format!("[1] \"{}\"", s)
                } else {
                    "[1] \"\"".to_string()
                }
            } else {
                let vals: Vec<String> = (0..x.len().min(10))
                    .map(|i| {
                        if let Some(charsxp) = x.string_elt(i) {
                            format!("\"{}\"", charsxp.as_str().unwrap_or(""))
                        } else {
                            "\"\"".to_string()
                        }
                    })
                    .collect();
                let suffix = if x.len() > 10 { " ..." } else { "" };
                format!("[1] {}{}", vals.join(" "), suffix)
            }
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
pub unsafe fn Rf_PrintValue(x: SEXP) {
    if let Some(s) = Sexp::from_raw(x) {
        print_value(s);
    }
}

/// FFI function: Rf_PrintValueEnv (print with environment context)
pub unsafe fn Rf_PrintValueEnv(x: SEXP, _env: SEXP) {
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

    #[test]
    fn test_capture_lifecycle() {
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
        start_capture();
        let output = stop_capture();
        assert_eq!(output.stdout, "");
        assert_eq!(output.stderr, "");
    }

    #[test]
    fn test_nested_capture() {
        start_capture();
        capture_stdout("outer ");
        let output = stop_capture();
        assert_eq!(output.stdout, "outer ");
    }

    #[test]
    fn test_print_logical_vector() {
        let mut arena = crate::sexp::memory::RArena::new();
        let ptr = arena.alloc_vector(SEXPTYPE::LGLSXP, 3);
        let sexp = Sexp::from_raw(ptr).expect("logical vector allocation failed");
        assert!(sexp.set_logical_elt(0, 0));
        assert!(sexp.set_logical_elt(1, 1));
        assert!(sexp.set_logical_elt(2, crate::sexp::ffi::NA_LOGICAL));

        start_capture();
        print_value(sexp);
        let output = stop_capture();
        assert_eq!(output.stdout, "[1] FALSE  TRUE    NA\n");
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
}
