#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/errors.c — error handling utilities.
//!
//! This module provides real error/warning handling using `std::panic::catch_unwind`
//! with a custom `RError` panic payload, replacing C's setjmp/longjmp mechanism.
//!
//! Key design:
//! - `Rf_error()` / `errorcall()` panic with `RError` payload
//! - `Rf_warning()` / `warningcall()` print to stderr or collect warnings
//! - `jump_to_top_ex()` panics with `RError` to unwind to top level
//! - Warning collection with configurable `warn` option (0=collect, 1=print, 2=error)
//! - Condition handler/restart stacks for tryCatch/withCallingHandler
//! - Traceback support

#[allow(unused_imports)]
use std::ffi::CStr;
#[allow(unused_imports)]
use std::os::raw::{c_char, c_int, c_void};
#[allow(unused_imports)]
use std::ptr;

use crate::sexp::context::{RError, RSignal};
use crate::sexp::ffi::{R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals;
use crate::sexp::instance::{self, ErrorState};

// Re-export common accessors/constructors for convenience
#[allow(unused_imports)]
use crate::eval::attrib_core::{R_ClassSymbol, R_NamesSymbol, getAttrib};
#[allow(unused_imports)]
use crate::mainutils::coerce::coerceVector;
#[allow(unused_imports)]
use crate::sexp::accessors::*;
#[allow(unused_imports)]
use crate::sexp::constructors::*;
#[allow(unused_imports)]
use crate::sexp::protect::protect;
#[allow(unused_imports)]
use crate::sexp::symbol::Rf_install;
pub use crate::special::mlutils::REprintf;

// PRINTNAME is re-exported from inlined.rs
#[allow(unused_imports)]
use crate::mainutils::inlined::PRINTNAME;

mod conditions;
mod deferred;
mod do_fns;
mod format;
mod helpers;
mod messages;
mod render;
mod restarts;
mod state;
#[cfg(test)]
mod tests;
mod traceback;

pub use self::conditions::*;
pub use self::deferred::*;
pub use self::do_fns::*;
pub use self::format::*;
use self::helpers::*;
pub use self::messages::*;
pub use self::render::*;
pub use self::restarts::*;
pub use self::state::*;
pub use self::traceback::*;

/// Bridge installed as the nmath crate's [`rmath_nmath::error::WarningHook`].
///
/// nmath.h maps `MATHLIB_WARNING(fmt, x)` to R's `warning(fmt, x)` in
/// integrated mode, so mathlib warnings (ML_WARNING's range/precision
/// messages, dpq.h's "non-integer x = %f") become regular, catchable,
/// deferred-printed R warnings attributed to the current call. C's
/// `warning()` strips one trailing newline from the formatted message
/// before recording it (errors.c); mirror that so deferred rendering
/// matches stock byte-for-byte.
pub fn nmath_warning_hook(message: &str) {
    let msg = message.strip_suffix('\n').unwrap_or(message);
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    unsafe {
        // Attribute to the dpq builtin in scope, falling back to the
        // current call (upstream warning() semantics).
        let call = mathlib_warning_call();
        if !call.is_null() && call != globals::R_NilValue() {
            Rf_warningcall1(call, c_msg.as_ptr());
        } else {
            Rf_warning1(c_msg.as_ptr());
        }
    }
}
