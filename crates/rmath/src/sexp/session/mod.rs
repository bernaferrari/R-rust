//! R session context — an explicit context for R operations.
//!
//! Instead of relying solely on scattered thread-local globals, this struct
//! provides a unified interface to all R interpreter state.
//!
//! # Overview
//!
//! [`RSession`] encapsulates an [`RInstance`] that owns its own arena,
//! protection stack, and environment state, enabling multiple independent
//! R sessions to coexist within the same process when each session stays on
//! the thread that created it.
//!
//! # Examples
//!
//! ```text
//! use crate::sexp::RSession;
//!
//! let session = RSession::new();
//! assert!(session.is_active());
//! assert!(session.global_env().is_some());
//! ```
//!
//! # Lifecycle
//!
//! Sessions are created with [`RSession::new`] and can be closed with
//! [`RSession::close`]. Once closed, evaluation and variable definition
//! operations become no-ops or return errors.

use std::cell::UnsafeCell;
use std::ffi::CString;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
#[cfg(test)]
use std::ptr;
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::context::{RError, RSignal};
use super::envir::Environment;
use super::ffi::SEXP;
#[cfg(test)]
use super::ffi::SEXPTYPE;
use super::globals::{R_NilValue, R_UnboundValue};
use super::instance::{
    RInstance, clear_current_instance_if, replace_current_instance, set_current_instance,
};
use super::memory::{ArenaBudget, RArena};
use super::object::{SessionNodeFactory, Sexp};
#[cfg(test)]
use super::protect::R_ProtectCount;
#[cfg(test)]
use super::protect::protect;
use rmath_nmath::rng::{detach_rng, install_rng};
use rmath_nmath::{MathState, RngState, detach_state, install_state};

mod retained;
pub use retained::RetainedValueId;

/// Error returned by safe session evaluation APIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct REvalError {
    pub message: String,
}

impl std::fmt::Display for REvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for REvalError {}

pub type RResult<T> = Result<T, REvalError>;

/// Shareable cooperative cancellation handle for an evaluation.
///
/// The atomic storage is intentionally private: embedding layers get an
/// ownership-oriented token with simple request/reset/query operations instead
/// of depending on the interpreter's synchronization detail.
#[derive(Clone, Debug)]
pub struct CancellationToken {
    requested: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Create a token with cancellation initially clear.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a token that has already requested cancellation.
    pub fn cancelled() -> Self {
        let token = Self::new();
        token.request();
        token
    }

    /// Request cancellation. Active evaluations observe this cooperatively.
    pub fn request(&self) {
        self.requested.store(true, Ordering::Relaxed);
    }

    /// Clear a previous cancellation request so the token can be reused.
    pub fn reset(&self) {
        self.requested.store(false, Ordering::Relaxed);
    }

    /// Return whether cancellation has been requested.
    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Relaxed)
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        CancellationToken {
            requested: Arc::new(AtomicBool::new(false)),
        }
    }
}

fn install_symbol(name: &str) -> Option<SEXP> {
    let name = CString::new(name).ok()?;
    let symbol = unsafe { crate::sexp::symbol::Rf_install(name.as_ptr()) };
    if symbol.is_null() { None } else { Some(symbol) }
}

fn catch_eval_result<'a, F>(f: F) -> RResult<Sexp<'a>>
where
    F: FnOnce() -> Result<Sexp<'a>, String>,
{
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => value.map_err(|message| REvalError { message }),
        Err(payload) => match payload.downcast::<RSignal>() {
            Ok(signal) => match *signal {
                RSignal::Error { message } => Err(REvalError { message }),
                // A break/next escaping every loop context is an R-level
                // error (upstream: "no loop for break/next, jumping to
                // the top level"), never an escaping Rust panic — the
                RSignal::Break | RSignal::Next => Err(REvalError {
                    message: "no loop for break/next, jumping to the top level".to_string(),
                }),
                // Top-level return(value) is likewise an R-level error
                // (upstream: "no function to return from, jumping to
                // top level"), never an escaping panic.
                RSignal::Return(ticket) => {
                    ticket.discard();
                    Err(REvalError {
                        message: "no function to return from, jumping to top level".to_string(),
                    })
                }
                RSignal::Abort => Err(REvalError {
                    message: "execution aborted".to_string(),
                }),
                // Restart requests own thread-confined GC guards. Even an
                // invalid transfer must be consumed while this session is active,
                // never exposed as a movable panic payload to safe host code.
                RSignal::Restart(ticket) => {
                    ticket.discard();
                    Err(REvalError {
                        message: "restart not on stack".to_string(),
                    })
                }
                RSignal::Jump(ticket) | RSignal::ExitingHandler(ticket) => {
                    ticket.discard();
                    Err(REvalError {
                        message: "unhandled nonlocal evaluation transfer".to_string(),
                    })
                }
                other => std::panic::panic_any(other),
            },
            Err(payload) => match payload.downcast::<RError>() {
                Ok(err) => Err(REvalError {
                    message: err.message.clone(),
                }),
                Err(payload) => std::panic::resume_unwind(payload),
            },
        },
    }
}

fn print_panic_r_error(payload: &(dyn std::any::Any + Send)) -> Option<REvalError> {
    let message = if let Some(err) = payload.downcast_ref::<RError>() {
        err.message.clone()
    } else if let Some(RSignal::Error { message }) = payload.downcast_ref::<RSignal>() {
        message.clone()
    } else {
        return None;
    };
    Some(REvalError { message })
}

fn remember_last_value(value: SEXP) {
    unsafe {
        // GNU main.c REPL: SET_SYMVALUE(R_LastvalueSymbol, value). The value
        // lives on the symbol's value cell, not in a globalenv binding, so
        // `ls()` never lists `.Last.value` (oracle Rscript: character(0)).
        crate::sexp::accessors::SET_SYMVALUE(
            crate::sexp::symbol::Rf_install(c".Last.value".as_ptr()),
            value,
        );
    }
}

fn expr_or_nil(expr: SEXP) -> SEXP {
    if expr.is_null() {
        unsafe { R_NilValue() }
    } else {
        expr
    }
}

mod guards;
mod rsession;

pub(crate) use guards::*;

pub use rsession::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "../gc_owner_teardown_tests.rs"]
mod gc_owner_teardown_tests;

#[cfg(test)]
mod public_capture_tests;
