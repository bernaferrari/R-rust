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

mod byte_buffer;
#[cfg(test)]
mod byte_buffer_tests;
mod conditions;
mod native;
mod nmath_bridge_buffer;
#[cfg(test)]
mod nmath_bridge_tests;
pub(crate) use native::raise_owned_diagnostic;
pub(crate) use native::try_catch_owned;
pub use native::{NativeBody, NativeFinally, NativeHandler};
#[cfg(test)]
mod default_byte_tests;
mod deferred;
mod do_fns;
mod format;
mod helpers;
mod messages;
mod render;
mod rendered_cache;
pub(crate) use rendered_cache::RenderedMessage;
#[cfg(test)]
mod rendered_cache_tests;
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
    let pin = state::error_scope_pin();
    let pointer = pin.as_ptr();
    // Immutable sibling ledger cells need no arena lend. Every in-tree emitter
    // supplies static, stack, or Rust-owned text, valid through host callbacks.
    let owner = unsafe { crate::sexp::owner::OwnerToken::from_raw(pointer) };
    nmath_bridge_checked(owner.require_active());
    let ledger = unsafe { (*pointer).heap_identity.byte_ledger() };
    // Ordinary warningcall chooses attribution before its first allocation/GC.
    // Snapshot and root that same choice before admission can collect/retry.
    let call = unsafe {
        let call = mathlib_warning_call();
        let call = if !call.is_null() && call != globals::R_NilValue() {
            call
        } else {
            render::getCurrentCall()
        };
        let call = render::effective_warning_call(call);
        if call.is_null() {
            globals::R_NilValue()
        } else {
            call
        }
    };
    let call = nmath_bridge_checked(
        owner
            .sexp(call)
            .and_then(crate::sexp::object::Sexp::into_owned),
    );
    let mut result = nmath_bridge_buffer::WarningBuffer::copy(&ledger, message);
    if matches!(
        &result,
        Err(nmath_bridge_buffer::BufferError::AdmissionDenied)
    ) && !crate::sexp::memory::is_arena_lent(pointer)
    {
        nmath_bridge_checked(owner.full_gc());
        result = nmath_bridge_buffer::WarningBuffer::copy(&ledger, message);
    }
    let buffer = nmath_bridge_checked(result.map_err(nmath_bridge_buffer::BufferError::into_sexp));
    nmath_bridge_checked(owner.require_active());
    unsafe {
        // Skip both redundant CString recopy and a second mutable attribution
        // lookup: handlers and default emission use the rooted original call.
        render::vsignalWarning(call.as_raw(), buffer.as_c_str().as_ptr());
    }
    nmath_bridge_checked(owner.require_active());
}

fn nmath_bridge_checked<T>(result: crate::sexp::object::SexpResult<T>) -> T {
    result.unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

/// Match GNU error()'s initial bounded message, before rendering a call prefix.
pub(crate) fn diagnostic_message_limit() -> usize {
    8192usize
        .min(state::r_warn_length().max(0) as usize)
        .saturating_sub(1)
}
