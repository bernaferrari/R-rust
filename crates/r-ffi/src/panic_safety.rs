//! Panic safety for FFI boundaries.
//!
//! Ensures that panics never escape into C code, which is undefined behavior.

use std::io::Write;
use std::panic::{self, UnwindSafe};

/// Wraps an FFI entry point function with panic catching.
///
/// Returns `default_value` if a panic occurs.
#[inline(always)]
pub fn ffi_catch_unwind<F, R>(f: F, default_value: R) -> R
where
    F: FnOnce() -> R + UnwindSafe,
{
    match panic::catch_unwind(f) {
        Ok(result) => result,
        Err(payload) => {
            let _ = std::io::stderr().write_all(b"\nERROR: Panic caught at FFI boundary.\n");
            if let Some(msg) = payload.downcast_ref::<&str>() {
                let _ = writeln!(std::io::stderr(), "Panic message: {}", msg);
            } else if let Some(msg) = payload.downcast_ref::<String>() {
                let _ = writeln!(std::io::stderr(), "Panic message: {}", msg);
            }
            default_value
        }
    }
}

/// Wraps an FFI entry point that returns void.
#[inline(always)]
pub fn ffi_catch_unwind_void<F>(f: F)
where
    F: FnOnce() + UnwindSafe,
{
    if let Err(payload) = panic::catch_unwind(f) {
        let _ = std::io::stderr().write_all(b"\nERROR: Panic caught at FFI boundary.\n");
        if let Some(msg) = payload.downcast_ref::<&str>() {
            let _ = writeln!(std::io::stderr(), "Panic message: {}", msg);
        } else if let Some(msg) = payload.downcast_ref::<String>() {
            let _ = writeln!(std::io::stderr(), "Panic message: {}", msg);
        }
    }
}

/// Macro to safely wrap an FFI function body.
#[macro_export]
macro_rules! ffi_boundary {
    ($body:block, $default:expr) => {
        match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| $body)) {
            Ok(result) => result,
            Err(payload) => {
                let _ = ::std::io::stderr().write_all(b"\nFATAL: Panic escaped to FFI boundary!\n");
                if let Some(msg) = payload.downcast_ref::<&str>() {
                    let _ = writeln!(::std::io::stderr(), "Panic: {}", msg);
                }
                $default
            }
        }
    };
    ($body:block) => {
        if let Err(payload) = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| $body)) {
            let _ = ::std::io::stderr().write_all(b"\nFATAL: Panic escaped to FFI boundary!\n");
            if let Some(msg) = payload.downcast_ref::<&str>() {
                let _ = writeln!(::std::io::stderr(), "Panic: {}", msg);
            }
        }
    };
}
