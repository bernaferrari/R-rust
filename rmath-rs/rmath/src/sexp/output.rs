//! R output capture for embedding.
//!
//! Captures Rprintf, REprintf, and other R output functions
//! so they can be returned to the caller instead of printing
//! to stdout/stderr.

use std::cell::Cell;
use std::sync::Mutex;

use super::ffi::{SEXP, SEXPTYPE};
use super::safe::Sexp;

/// Captured R output.
#[derive(Debug, Clone, Default)]
pub struct RCapturedOutput {
    pub stdout: String,
    pub stderr: String,
}

thread_local! {
    static CAPTURE_STDOUT: Mutex<Option<String>> = const { Mutex::new(None) };
    static CAPTURE_STDERR: Mutex<Option<String>> = const { Mutex::new(None) };
    static IS_CAPTURING: Cell<bool> = const { Cell::new(false) };
}

/// Start capturing R output.
pub fn start_capture() {
    CAPTURE_STDOUT.with(|c| *c.lock().expect("c lock poisoned") = Some(String::new()));
    CAPTURE_STDERR.with(|c| *c.lock().expect("c lock poisoned") = Some(String::new()));
    IS_CAPTURING.with(|c| c.set(true));
}

/// Stop capturing and return the captured output.
pub fn stop_capture() -> RCapturedOutput {
    let stdout = CAPTURE_STDOUT.with(|c| {
        c.lock()
            .expect("c lock poisoned")
            .take()
            .unwrap_or_default()
    });
    let stderr = CAPTURE_STDERR.with(|c| {
        c.lock()
            .expect("c lock poisoned")
            .take()
            .unwrap_or_default()
    });
    IS_CAPTURING.with(|c| c.set(false));
    RCapturedOutput { stdout, stderr }
}

/// Check if output capture is active.
pub fn is_capturing() -> bool {
    IS_CAPTURING.with(|c| c.get())
}

/// Append to captured stdout. Called by the Rprintf hook.
pub fn capture_stdout(msg: &str) {
    if is_capturing() {
        CAPTURE_STDOUT.with(|c| {
            if let Some(s) = c.lock().expect("c lock poisoned").as_mut() {
                s.push_str(msg);
            }
        });
    }
}

/// Append to captured stderr. Called by the REprintf hook.
pub fn capture_stderr(msg: &str) {
    if is_capturing() {
        CAPTURE_STDERR.with(|c| {
            if let Some(s) = c.lock().expect("c lock poisoned").as_mut() {
                s.push_str(msg);
            }
        });
    }
}

/// Print an R object to the captured output (or stdout if not capturing).
///
/// This is the Rust implementation of R's Rf_PrintValue.
pub fn print_value(x: Sexp<'_>) {
    let type_name = match x.typeof_() {
        SEXPTYPE::NILSXP => "NULL",
        SEXPTYPE::INTSXP => "integer",
        SEXPTYPE::REALSXP => "double",
        SEXPTYPE::LGLSXP => "logical",
        SEXPTYPE::STRSXP => "character",
        SEXPTYPE::VECSXP => "list",
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

    if is_capturing() {
        capture_stdout(&output);
        capture_stdout("\n");
    } else {
        println!("{}", output);
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_PrintValue(x: SEXP) {
    if let Some(s) = Sexp::from_raw(x) {
        print_value(s);
    }
}

/// FFI function: Rf_PrintValueEnv (print with environment context)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_PrintValueEnv(x: SEXP, _env: SEXP) {
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
}
