//! R output capture for embedding.
//!
//! Captures Rprintf, REprintf, and other R output functions
//! so they can be returned to the caller instead of printing
//! to stdout/stderr.

use std::cell::Cell;
use std::sync::Mutex;

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
    CAPTURE_STDOUT.with(|c| *c.lock().unwrap() = Some(String::new()));
    CAPTURE_STDERR.with(|c| *c.lock().unwrap() = Some(String::new()));
    IS_CAPTURING.with(|c| c.set(true));
}

/// Stop capturing and return the captured output.
pub fn stop_capture() -> RCapturedOutput {
    let stdout = CAPTURE_STDOUT.with(|c| c.lock().unwrap().take().unwrap_or_default());
    let stderr = CAPTURE_STDERR.with(|c| c.lock().unwrap().take().unwrap_or_default());
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
            if let Some(s) = c.lock().unwrap().as_mut() {
                s.push_str(msg);
            }
        });
    }
}

/// Append to captured stderr. Called by the REprintf hook.
pub fn capture_stderr(msg: &str) {
    if is_capturing() {
        CAPTURE_STDERR.with(|c| {
            if let Some(s) = c.lock().unwrap().as_mut() {
                s.push_str(msg);
            }
        });
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
