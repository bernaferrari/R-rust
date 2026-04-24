//! R session context — an explicit context for R operations.
//!
//! Instead of relying solely on thread_local! globals, this struct
//! provides a unified interface to all R interpreter state.
//!
//! # Overview
//!
//! [`RSession`] encapsulates an [`RInstance`] that owns its own arena,
//! protection stack, and environment state, enabling multiple independent
//! R sessions to coexist within the same process.
//!
//! # Examples
//!
//! ```
//! use rmath::sexp::RSession;
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

use std::ffi::CString;
use std::os::raw::c_int;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

use super::context::{RError, RSignal};
use super::ffi::{SEXP, SEXPTYPE};
use super::globals::{R_BaseEnv, R_GlobalEnv, R_NilValue, R_UnboundValue};
use super::instance::{RInstance, clear_current_instance_if, set_current_instance};
use super::memory::RArena;
use super::protect::{R_ProtectCount, Rf_protect, Rf_unprotect};
use super::safe::Sexp;

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

fn install_symbol(name: &str) -> Option<SEXP> {
    let name = CString::new(name).ok()?;
    let symbol = unsafe { crate::sexp::symbol::Rf_install(name.as_ptr()) };
    if symbol.is_null() { None } else { Some(symbol) }
}

fn catch_eval<F>(f: F) -> RResult<SEXP>
where
    F: FnOnce() -> SEXP,
{
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => Ok(value),
        Err(payload) => match payload.downcast::<RSignal>() {
            Ok(signal) => match *signal {
                RSignal::Error { message } => Err(REvalError { message }),
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

/// An R interpreter session with its own isolated instance state.
///
/// Each `RSession` owns an [`RInstance`] containing a private arena,
/// environment chain, and protection stack. When a session is active,
/// all global accessor functions (`R_GlobalEnv`, `Rf_protect`, `with_arena`,
/// etc.) dispatch to the session's instance instead of the process-wide
/// fallbacks.
///
/// # Thread Safety
///
/// `RSession` is `Send` but not `Sync`. Each thread should create its
/// own session instance.
pub struct RSession {
    /// Whether this session is active.
    active: bool,
    /// The owned R instance with isolated state.
    instance: Box<RInstance>,
}

impl RSession {
    /// Create a new R session with its own isolated instance.
    ///
    /// Initializes a fresh [`RInstance`] with its own arena and environment
    /// chain, and sets it as the current thread-local instance.
    pub fn new() -> Self {
        let mut instance = Box::new(RInstance::new());
        unsafe {
            set_current_instance(&mut *instance as *mut RInstance);
        }
        RSession {
            active: true,
            instance,
        }
    }

    /// Check if this session is active.
    ///
    /// Returns `false` after [`RSession::close`] has been called.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Get the global environment.
    ///
    /// Returns `None` if the global environment pointer is null.
    pub fn global_env(&self) -> Option<Sexp<'_>> {
        Sexp::from_raw(self.instance.global_env)
    }

    /// Get the base environment.
    ///
    /// Returns `None` if the base environment pointer is null.
    pub fn base_env(&self) -> Option<Sexp<'_>> {
        Sexp::from_raw(self.instance.base_env)
    }

    /// Evaluate an expression in this session's global environment.
    ///
    /// # Errors
    ///
    /// Returns an error if the session is closed or if evaluation
    /// triggers an R error (e.g., undefined variable, type error).
    ///
    /// # Safety
    ///
    /// The `expr` pointer must be a valid SEXP or null.
    pub fn eval(&self, expr: SEXP) -> RResult<SEXP> {
        if !self.active {
            return Err(REvalError {
                message: "session is closed".to_string(),
            });
        }
        catch_eval(|| unsafe { crate::eval::eval::Rf_eval(expr, self.instance.global_env) })
    }

    /// Evaluate an expression with a custom environment.
    ///
    /// # Errors
    ///
    /// Returns an error if the session is closed or if evaluation
    /// triggers an R error.
    ///
    /// # Safety
    ///
    /// Both `expr` and `env` pointers must be valid SEXPs or null.
    pub fn eval_in(&self, expr: SEXP, env: SEXP) -> RResult<SEXP> {
        if !self.active {
            return Err(REvalError {
                message: "session is closed".to_string(),
            });
        }
        catch_eval(|| unsafe { crate::eval::eval::Rf_eval(expr, env) })
    }

    /// Find a variable by name in the global environment.
    ///
    /// Returns `None` if the variable is not found, is unbound, or
    /// is `R_NilValue`.
    ///
    /// Names with interior NUL bytes are rejected.
    pub fn find_var(&self, name: &str) -> Option<Sexp<'_>> {
        let symbol = install_symbol(name)?;
        let result = unsafe { crate::sexp::envir::R_findVar(symbol, self.instance.global_env) };
        if result == unsafe { R_UnboundValue() } || result == unsafe { R_NilValue() } {
            None
        } else {
            Sexp::from_raw(result)
        }
    }

    /// Define a variable in the global environment.
    ///
    /// This is a no-op if the session is closed or if the symbol
    /// cannot be interned.
    ///
    /// Names with interior NUL bytes are rejected.
    ///
    /// The `value` pointer must be a valid SEXP or null.
    pub fn define_var(&self, name: &str, value: SEXP) {
        if !self.active {
            return;
        }
        let Some(symbol) = install_symbol(name) else {
            return;
        };
        unsafe {
            crate::sexp::envir::defineVar(symbol, value, self.instance.global_env);
        }
    }

    /// Run a function in a protected scope.
    ///
    /// The protection count is saved before calling `f` and any
    /// additional protections added during `f` are automatically
    /// removed after `f` returns. This prevents protection stack
    /// leaks.
    ///
    /// # Examples
    ///
    /// ```
    /// use rmath::sexp::RSession;
    /// use rmath::sexp::protect::Rf_protect;
    /// use std::ptr;
    ///
    /// let session = RSession::new();
    /// session.with_protected(|| {
    ///     unsafe { Rf_protect(ptr::null_mut()); }
    /// });
    /// ```
    pub fn with_protected<F, T>(&self, f: F) -> T
    where
        F: FnOnce() -> T,
    {
        let depth = R_ProtectCount();
        let result = f();
        let new_depth = R_ProtectCount();
        if new_depth > depth {
            unsafe {
                Rf_unprotect((new_depth - depth) as c_int);
            }
        }
        result
    }

    /// Run the garbage collector.
    ///
    /// Performs a minor GC on the young generation.
    pub fn gc(&self) {
        super::gengc::minor_gc();
    }

    /// Close this session.
    ///
    /// After closing, [`is_active`](RSession::is_active) returns `false`
    /// and evaluation methods return errors. The current thread-local
    /// instance is cleared only if this session still owns it.
    pub fn close(&mut self) {
        self.active = false;
        clear_current_instance_if(&*self.instance);
    }
}

impl Default for RSession {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RSession {
    fn drop(&mut self) {
        if self.active {
            clear_current_instance_if(&*self.instance);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::instance::with_current_instance;
    use crate::sexp::memory::with_arena;

    #[test]
    fn test_session_creation() {
        let session = RSession::new();
        assert!(session.is_active());
        assert!(session.global_env().is_some());
        assert!(session.base_env().is_some());
    }

    #[test]
    fn test_session_close() {
        let mut session = RSession::new();
        assert!(session.is_active());
        session.close();
        assert!(!session.is_active());
    }

    #[test]
    fn test_session_close_non_current_keeps_current_instance() {
        let mut older = RSession::new();
        let newer = RSession::new();
        let newer_instance = &*newer.instance as *const RInstance;

        older.close();

        assert!(
            with_current_instance(|inst| std::ptr::eq(inst as *const RInstance, newer_instance))
                .unwrap_or(false)
        );
        assert!(newer.is_active());
    }

    #[test]
    fn test_session_drop_non_current_keeps_current_instance() {
        let older = RSession::new();
        let newer = RSession::new();
        let newer_instance = &*newer.instance as *const RInstance;

        drop(older);

        assert!(
            with_current_instance(|inst| std::ptr::eq(inst as *const RInstance, newer_instance))
                .unwrap_or(false)
        );
        assert!(newer.is_active());
    }

    #[test]
    fn test_session_eval_closed() {
        let mut session = RSession::new();
        session.close();
        let result = session.eval(ptr::null_mut());
        assert!(result.is_err());
    }

    #[test]
    fn test_session_define_and_find_var_with_rust_str() {
        let session = RSession::new();
        let value = with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1));
        let sexp = Sexp::from_raw(value).expect("integer vector allocation failed");
        assert!(sexp.set_integer_elt(0, 42));

        session.define_var("session_defined_value", value);

        let found = session
            .find_var("session_defined_value")
            .expect("defined value should be found");
        assert_eq!(found.integer_elt(0), Some(42));
    }

    #[test]
    fn test_session_rejects_interior_nul_symbol_names() {
        let session = RSession::new();
        let value = with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1));
        let sexp = Sexp::from_raw(value).expect("integer vector allocation failed");
        assert!(sexp.set_integer_elt(0, 99));

        session.define_var("session_bad\0name", value);

        assert!(session.find_var("session_bad\0name").is_none());
        assert!(session.find_var("session_bad").is_none());
    }

    #[test]
    fn test_session_protected_scope() {
        let session = RSession::new();
        let depth_before = R_ProtectCount();
        session.with_protected(|| unsafe {
            Rf_protect(ptr::null_mut());
            Rf_protect(ptr::null_mut());
        });
        let depth_after = R_ProtectCount();
        assert_eq!(depth_before, depth_after);
    }

    #[test]
    fn test_session_gc() {
        let session = RSession::new();
        // Should not panic
        session.gc();
    }

    #[test]
    fn test_session_default() {
        let session = RSession::default();
        assert!(session.is_active());
    }
}
