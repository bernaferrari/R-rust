//! R session context — an explicit context for R operations.
//!
//! Instead of relying solely on thread_local! globals, this struct
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
use std::sync::{Arc, atomic::AtomicBool};

use super::context::{RError, RSignal};
use super::ffi::{SEXP, SEXPTYPE};
use super::globals::{
    R_BaseEnv, R_GlobalEnv, R_MissingArg, R_NilValue, R_RestartToken, R_UnboundValue,
};
use super::instance::{
    RInstance, clear_current_instance_if, replace_current_instance, set_current_instance,
};
use super::memory::{ArenaBudget, RArena};
use super::object::Sexp;
use super::protect::{R_ProtectCount, Rf_protect, Rf_unprotect};

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
pub type CancellationFlag = Arc<AtomicBool>;

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

fn expr_or_nil(expr: SEXP) -> SEXP {
    if expr.is_null() {
        unsafe { R_NilValue() }
    } else {
        expr
    }
}

struct CurrentInstanceGuard {
    previous: Option<*mut RInstance>,
}

impl Drop for CurrentInstanceGuard {
    fn drop(&mut self) {
        unsafe {
            replace_current_instance(self.previous);
        }
    }
}

/// An R interpreter session with its own isolated instance state.
///
/// Each `RSession` owns an [`RInstance`] containing a private arena,
/// environment chain, and protection stack. When a session is active,
/// all global accessor functions (`R_GlobalEnv`, `Rf_protect`, `with_arena`,
/// etc.) dispatch to the session's instance.
///
/// # Thread Safety
///
/// `RSession` is thread-confined. Each worker thread should create and keep its
/// own session instance; moving a live session across threads would invalidate
/// the thread-local compatibility dispatch pointer.
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
        super::context::install_r_panic_hook();
        let mut instance = Box::new(RInstance::new());
        unsafe {
            set_current_instance(&mut *instance as *mut RInstance);
        }
        RSession {
            active: true,
            instance,
        }
    }

    fn instance_ptr(&self) -> *mut RInstance {
        (&*self.instance as *const RInstance).cast_mut()
    }

    fn activate(&self) -> CurrentInstanceGuard {
        let previous = unsafe { replace_current_instance(Some(self.instance_ptr())) };
        CurrentInstanceGuard { previous }
    }

    fn with_active<F, T>(&self, f: F) -> T
    where
        F: FnOnce() -> T,
    {
        let _guard = self.activate();
        f()
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
        self.sexp(self.instance.global_env)
    }

    /// Get the base environment.
    ///
    /// Returns `None` if the base environment pointer is null.
    pub fn base_env(&self) -> Option<Sexp<'_>> {
        self.sexp(self.instance.base_env)
    }

    /// Wrap a raw pointer if it belongs to this session.
    ///
    /// This is the safe public boundary for turning C-shaped `SEXP` values into
    /// Rust `Sexp` handles. Unknown pointers are rejected; accepted pointers are
    /// owned by this session's arena or persistent instance storage, or are one
    /// of R's process-wide immutable sentinels such as `R_NilValue`.
    pub fn sexp(&self, ptr: SEXP) -> Option<Sexp<'_>> {
        if ptr.is_null() {
            return None;
        }
        if self.instance.owns_sexp(ptr) || is_immutable_singleton(ptr) {
            Sexp::from_raw(ptr)
        } else {
            None
        }
    }

    /// Evaluate an expression in this session's global environment.
    ///
    /// # Errors
    ///
    /// Returns an error if the session is closed or if evaluation
    /// triggers an R error (e.g., undefined variable, type error).
    ///
    /// Raw pointers that do not belong to this session are rejected before
    /// evaluation. Prefer [`RSession::eval_sexp`] when the caller already has a
    /// lifetime-bound [`Sexp`] handle.
    pub fn eval(&self, expr: SEXP) -> RResult<SEXP> {
        self.eval_sexp_raw(expr).map(Sexp::as_raw)
    }

    /// Evaluate a raw expression pointer after proving it belongs to this session.
    pub fn eval_sexp_raw(&self, expr: SEXP) -> RResult<Sexp<'_>> {
        let expr = expr_or_nil(expr);
        let expr = self.sexp(expr).ok_or_else(|| REvalError {
            message: "expression does not belong to this session".to_string(),
        })?;
        self.eval_sexp(expr)
    }

    /// Evaluate an expression and return a session-scoped safe wrapper.
    pub fn eval_sexp<'session>(&'session self, expr: Sexp<'_>) -> RResult<Sexp<'session>> {
        if !self.active {
            return Err(REvalError {
                message: "session is closed".to_string(),
            });
        }

        self.with_active(|| {
            let expr = self.sexp(expr.as_raw()).ok_or_else(|| REvalError {
                message: "expression does not belong to this session".to_string(),
            })?;
            let env = self.global_env().ok_or_else(|| REvalError {
                message: "session has no global environment".to_string(),
            })?;
            catch_eval_result(|| crate::eval::eval::EvalContext::new(env).eval(expr))
        })
    }

    /// Evaluate an expression while capturing output and the final visibility flag.
    ///
    /// This mirrors the top-level embedding contract: explicit output produced by
    /// functions such as `print()` and `cat()` is captured separately from the
    /// implicit printing controlled by `R_Visible`.
    pub fn eval_with_output_capture(
        &self,
        expr: SEXP,
    ) -> (RResult<SEXP>, super::output::RCapturedOutput, bool) {
        let Some(expr) = self.sexp(expr_or_nil(expr)) else {
            return (
                Err(REvalError {
                    message: "expression does not belong to this session".to_string(),
                }),
                super::output::RCapturedOutput::default(),
                false,
            );
        };
        let (result, output, visible) = self.eval_sexp_with_output_capture(expr);
        (result.map(Sexp::as_raw), output, visible)
    }

    /// Evaluate an expression while capturing output and returning a typed
    /// session-scoped result.
    pub fn eval_sexp_with_output_capture<'session>(
        &'session self,
        expr: Sexp<'_>,
    ) -> (
        RResult<Sexp<'session>>,
        super::output::RCapturedOutput,
        bool,
    ) {
        if !self.active {
            return (
                Err(REvalError {
                    message: "session is closed".to_string(),
                }),
                super::output::RCapturedOutput::default(),
                false,
            );
        }
        self.with_active(|| {
            super::output::start_capture();
            let result = self.eval_sexp(expr);
            let visible = super::globals::R_Visible() != 0;
            let output = super::output::stop_capture();
            (result, output, visible)
        })
    }

    /// Parse and evaluate source code while capturing output and visibility.
    ///
    /// This keeps embedders on the owner-checked `Sexp` path instead of asking
    /// them to parse into a raw `SEXP` and then prove ownership themselves.
    pub fn eval_code_with_output_capture<'session>(
        &'session mut self,
        code: &str,
    ) -> (
        RResult<Sexp<'session>>,
        super::output::RCapturedOutput,
        bool,
    ) {
        if !self.active {
            return (
                Err(REvalError {
                    message: "session is closed".to_string(),
                }),
                super::output::RCapturedOutput::default(),
                false,
            );
        }

        let raw_expr = {
            let _guard = self.activate();
            crate::eval::parser::parse(code, &mut self.instance.arena)
        };
        let raw_expr = match raw_expr {
            Ok(expr) => expr_or_nil(expr),
            Err(err) => {
                return (
                    Err(REvalError {
                        message: err.to_string(),
                    }),
                    super::output::RCapturedOutput::default(),
                    false,
                );
            }
        };
        let Some(expr) = self.sexp(raw_expr) else {
            return (
                Err(REvalError {
                    message: "parsed expression does not belong to this session".to_string(),
                }),
                super::output::RCapturedOutput::default(),
                false,
            );
        };
        self.eval_sexp_with_output_capture(expr)
    }

    /// Evaluate an expression with a custom environment.
    ///
    /// # Errors
    ///
    /// Returns an error if the session is closed or if evaluation
    /// triggers an R error.
    ///
    /// Raw pointers that do not belong to this session are rejected before
    /// evaluation. Prefer [`RSession::eval_sexp_in`] when the caller already has
    /// lifetime-bound [`Sexp`] handles.
    pub fn eval_in(&self, expr: SEXP, env: SEXP) -> RResult<SEXP> {
        let expr = self.sexp(expr_or_nil(expr)).ok_or_else(|| REvalError {
            message: "expression does not belong to this session".to_string(),
        })?;
        let env = self.sexp(env).ok_or_else(|| REvalError {
            message: "environment does not belong to this session".to_string(),
        })?;
        self.eval_sexp_in(expr, env).map(Sexp::as_raw)
    }

    /// Evaluate an expression in a custom environment and return a
    /// session-scoped safe wrapper.
    pub fn eval_sexp_in<'session>(
        &'session self,
        expr: Sexp<'_>,
        env: Sexp<'_>,
    ) -> RResult<Sexp<'session>> {
        if !self.active {
            return Err(REvalError {
                message: "session is closed".to_string(),
            });
        }

        self.with_active(|| {
            let expr = self.sexp(expr.as_raw()).ok_or_else(|| REvalError {
                message: "expression does not belong to this session".to_string(),
            })?;
            let env = self.sexp(env.as_raw()).ok_or_else(|| REvalError {
                message: "environment does not belong to this session".to_string(),
            })?;
            catch_eval_result(|| crate::eval::eval::EvalContext::new(env).eval(expr))
        })
    }

    /// Find a variable by name in the global environment.
    ///
    /// Returns `None` if the variable is not found, is unbound, or
    /// is `R_NilValue`.
    ///
    /// Names with interior NUL bytes are rejected.
    pub fn find_var(&self, name: &str) -> Option<Sexp<'_>> {
        self.with_active(|| {
            let symbol = install_symbol(name)?;
            let result = unsafe { crate::sexp::envir::R_findVar(symbol, self.instance.global_env) };
            if result == unsafe { R_UnboundValue() } || result == unsafe { R_NilValue() } {
                None
            } else {
                self.sexp(result)
            }
        })
    }

    /// Define a variable in the global environment.
    ///
    /// This is a no-op if the session is closed or if the symbol
    /// cannot be interned.
    ///
    /// Names with interior NUL bytes are rejected.
    ///
    /// The value must belong to this session, except for immutable singleton
    /// sentinels such as `NULL`.
    pub fn define_var(&self, name: &str, value: Sexp<'_>) -> bool {
        if self.sexp(value.as_raw()).is_none() {
            return false;
        }
        unsafe { self.define_var_raw(name, value.as_raw()) }
    }

    /// Define a variable from a raw pointer for internal compatibility paths.
    ///
    /// Safe Rust code should use [`define_var`](Self::define_var), which
    /// checks that the value is owned by this session before installing it.
    pub(crate) unsafe fn define_var_raw(&self, name: &str, value: SEXP) -> bool {
        if !self.active {
            return false;
        }
        self.with_active(|| {
            let Some(symbol) = install_symbol(name) else {
                return false;
            };
            unsafe {
                crate::sexp::envir::defineVar(symbol, value, self.instance.global_env);
            }
            true
        })
    }

    /// Run a closure with mutable access to this session's arena.
    pub fn with_arena<F, T>(&mut self, f: F) -> Option<T>
    where
        F: FnOnce(&mut RArena) -> T,
    {
        if !self.active {
            return None;
        }
        let _guard = self.activate();
        Some(f(&mut self.instance.arena))
    }

    /// Return the current arena budget for this session.
    pub fn arena_budget(&self) -> ArenaBudget {
        self.instance.arena.budget()
    }

    /// Set the arena budget for this session.
    ///
    /// Existing allocations are kept; future allocations fail if retained arena
    /// memory or active node count would exceed the configured limit.
    pub fn set_arena_budget(&mut self, budget: ArenaBudget) {
        self.instance.arena.set_budget(budget);
    }

    /// Return this session's configured R library search paths.
    pub fn library_paths(&self) -> Vec<std::path::PathBuf> {
        self.instance.path_policy.library_paths().to_vec()
    }

    /// Find an installed package in this session's library search paths.
    pub fn find_package_path(&self, package: &str) -> Option<std::path::PathBuf> {
        self.instance.path_policy.find_package_path(package)
    }

    /// Replace this session's R library search paths.
    pub fn set_library_paths<I, P>(&mut self, paths: I)
    where
        I: IntoIterator<Item = P>,
        P: Into<std::path::PathBuf>,
    {
        self.instance.path_policy.set_library_paths(paths);
    }

    /// Configure Android app-private runtime paths for this session.
    ///
    /// `app_files_dir` owns the user library, `cache_dir` owns `tempdir()`,
    /// and `bundled_library_dir` points at the read-only package library
    /// shipped with the app, when present.
    pub fn configure_android_paths(
        &mut self,
        app_files_dir: impl Into<std::path::PathBuf>,
        cache_dir: impl Into<std::path::PathBuf>,
        bundled_library_dir: Option<impl Into<std::path::PathBuf>>,
    ) -> std::io::Result<()> {
        self.instance.path_policy = crate::mainutils::paths::RuntimePathPolicy::for_android_app(
            app_files_dir,
            cache_dir,
            bundled_library_dir,
        )?;
        Ok(())
    }

    /// Return the session-specific temporary directory used by `tempdir()`.
    pub fn temp_dir(&self) -> &std::path::Path {
        self.instance.path_policy.temp_dir()
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
        self.with_active(|| {
            let depth = R_ProtectCount();
            let result = f();
            let new_depth = R_ProtectCount();
            if new_depth > depth {
                unsafe {
                    Rf_unprotect((new_depth - depth) as c_int);
                }
            }
            result
        })
    }

    /// Run the garbage collector.
    ///
    /// Performs a minor GC on the young generation.
    pub fn gc(&self) {
        self.with_active(super::gengc::minor_gc);
    }

    /// Generate a uniform random number using this session's RNG state.
    pub fn unif_rand(&self) -> f64 {
        self.with_active(crate::rng::unif_rand)
    }

    /// Set this session's RNG seed state.
    pub fn set_seed(&self, i1: u32, i2: u32) {
        self.with_active(|| crate::rng::set_seed(i1, i2));
    }

    /// Set or clear this session's cooperative cancellation flag.
    ///
    /// Evaluator loop checks read this flag through the active `RInstance`.
    /// Sharing an `Arc<AtomicBool>` lets an embedding host request cancellation
    /// from another thread without exposing runtime internals.
    pub fn set_cancellation_flag(&mut self, flag: Option<CancellationFlag>) {
        self.instance.eval_state.cancellation = flag;
    }

    /// Generate a standard normal random number using this session's RNG state.
    pub fn norm_rand(&self) -> f64 {
        self.with_active(crate::dist::normal::norm_rand)
    }

    /// Run a closure while capturing this session's stdout/stderr buffers.
    pub fn with_output_capture<F, T>(&self, f: F) -> (T, super::output::RCapturedOutput)
    where
        F: FnOnce() -> T,
    {
        self.with_active(|| {
            super::output::start_capture();
            let value = f();
            let output = super::output::stop_capture();
            (value, output)
        })
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

fn is_immutable_singleton(ptr: SEXP) -> bool {
    unsafe {
        ptr == R_NilValue()
            || ptr == R_UnboundValue()
            || ptr == R_MissingArg()
            || ptr == R_RestartToken()
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
    use crate::sexp::instance::{current_instance_ptr, with_current_instance};
    use crate::sexp::memory::with_arena;
    use crate::sexp::protect::{R_PreserveObject, R_ReleaseObject, with_preserved_objects};

    #[test]
    fn test_session_creation() {
        let session = RSession::new();
        assert!(session.is_active());
        assert!(session.global_env().is_some());
        assert!(session.base_env().is_some());
    }

    #[test]
    fn test_session_sexp_rejects_foreign_arena_pointer() {
        let mut left = RSession::new();
        let right = RSession::new();

        let ptr = left
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            .expect("left session should be active");

        assert!(left.sexp(ptr).is_some());
        assert!(right.sexp(ptr).is_none());
    }

    #[test]
    fn test_session_eval_sexp_returns_session_scoped_wrapper() {
        let mut session = RSession::new();
        let expr = session
            .with_arena(|arena| {
                crate::sexp::builder::scalar_integer_in(arena, 7)
                    .expect("scalar allocation should succeed")
                    .as_raw()
            })
            .expect("session should be active");
        let expr = session.sexp(expr).expect("expr belongs to session");

        let result = session
            .eval_sexp(expr)
            .expect("self-evaluating scalar should evaluate");

        assert_eq!(result.integer_elt(0), Some(7));
    }

    #[test]
    fn test_session_eval_with_output_capture_returns_typed_wrapper() {
        let mut session = RSession::new();
        let expr = session
            .with_arena(|arena| {
                crate::sexp::builder::scalar_integer_in(arena, 8)
                    .expect("scalar allocation should succeed")
                    .as_raw()
            })
            .expect("session should be active");
        let expr = session.sexp(expr).expect("expr belongs to session");

        let (result, output, visible) = session.eval_sexp_with_output_capture(expr);
        let result = result.expect("self-evaluating scalar should evaluate");

        assert_eq!(result.integer_elt(0), Some(8));
        assert!(output.stdout.is_empty());
        assert!(visible);
    }

    #[test]
    fn test_session_eval_code_with_output_capture_keeps_parsing_session_owned() {
        let mut session = RSession::new();

        let (result, output, visible) = session.eval_code_with_output_capture("print(1); 2");
        let result = result.expect("source should parse and evaluate");

        assert_eq!(output.stdout, "[1] 1\n");
        assert_eq!(result.real_elt(0), Some(2.0));
        assert!(visible);
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
    fn test_session_method_activation_restores_previous_instance() {
        let mut older = RSession::new();
        let older_instance = &*older.instance as *const RInstance;
        let newer = RSession::new();
        let newer_instance = &*newer.instance as *const RInstance;

        assert!(
            current_instance_ptr()
                .map(|ptr| std::ptr::eq(ptr as *const RInstance, newer_instance))
                .unwrap_or(false)
        );

        let activated_older = older.with_arena(|_| {
            current_instance_ptr()
                .map(|ptr| std::ptr::eq(ptr as *const RInstance, older_instance))
                .unwrap_or(false)
        });

        assert_eq!(activated_older, Some(true));
        assert!(
            current_instance_ptr()
                .map(|ptr| std::ptr::eq(ptr as *const RInstance, newer_instance))
                .unwrap_or(false)
        );
    }

    #[test]
    fn test_session_output_capture_is_local_on_same_thread() {
        let left = RSession::new();
        let right = RSession::new();

        let (_, left_output) = left.with_output_capture(|| {
            crate::sexp::output::capture_stdout("left out");
            crate::sexp::output::capture_stderr("left err");
        });
        let (_, right_output) = right.with_output_capture(|| {
            crate::sexp::output::capture_stdout("right out");
            crate::sexp::output::capture_stderr("right err");
        });

        assert_eq!(left_output.stdout, "left out");
        assert_eq!(left_output.stderr, "left err");
        assert_eq!(right_output.stdout, "right out");
        assert_eq!(right_output.stderr, "right err");
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

        let value = session.sexp(value).expect("value belongs to session");
        assert!(session.define_var("session_defined_value", value));

        let found = session
            .find_var("session_defined_value")
            .expect("defined value should be found");
        assert_eq!(found.integer_elt(0), Some(42));
    }

    #[test]
    fn test_session_define_var_interns_symbol_in_target_session() {
        let mut older = RSession::new();
        let newer = RSession::new();

        let value = older
            .with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
            .expect("older session should be active");
        let sexp = Sexp::from_raw(value).expect("integer vector allocation failed");
        assert!(sexp.set_integer_elt(0, 123));

        let value = older.sexp(value).expect("value belongs to older session");
        assert!(older.define_var("session_local_symbol", value));

        let found = older
            .find_var("session_local_symbol")
            .expect("older session should own symbol binding");
        assert_eq!(found.integer_elt(0), Some(123));
        assert!(newer.find_var("session_local_symbol").is_none());
    }

    #[test]
    fn test_session_define_var_rejects_foreign_value() {
        let mut owner = RSession::new();
        let other = RSession::new();

        let value = owner
            .with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
            .expect("owner session should be active");
        let value = owner.sexp(value).expect("value belongs to owner");

        assert!(!other.define_var("foreign_value", value));
        assert!(other.find_var("foreign_value").is_none());
    }

    #[test]
    fn test_session_rejects_interior_nul_symbol_names() {
        let session = RSession::new();
        let value = with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1));
        let sexp = Sexp::from_raw(value).expect("integer vector allocation failed");
        assert!(sexp.set_integer_elt(0, 99));

        let value = session.sexp(value).expect("value belongs to session");
        assert!(!session.define_var("session_bad\0name", value));

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
    fn test_session_protect_and_preserve_are_local_on_same_thread() {
        let mut left = RSession::new();
        let mut right = RSession::new();
        let protected = 0x1 as SEXP;
        let preserved = 0x2 as SEXP;

        left.with_arena(|_| unsafe {
            Rf_protect(protected);
            R_PreserveObject(preserved);
            assert_eq!(R_ProtectCount(), 1);
            with_preserved_objects(|objects| assert_eq!(objects, &[preserved]));
        });

        right.with_arena(|_| {
            assert_eq!(R_ProtectCount(), 0);
            with_preserved_objects(|objects| assert!(objects.is_empty()));
        });

        left.with_arena(|_| unsafe {
            Rf_unprotect(1);
            R_ReleaseObject(preserved);
            assert_eq!(R_ProtectCount(), 0);
            with_preserved_objects(|objects| assert!(objects.is_empty()));
        });
    }

    #[test]
    fn test_session_gc() {
        let session = RSession::new();
        // Should not panic
        session.gc();
    }

    #[test]
    fn test_session_arena_budget_controls_future_allocations() {
        let mut session = RSession::new();
        let node_bytes = std::mem::size_of::<crate::sexp::ffi::SexprecCore>();
        let current_bytes = session
            .with_arena(|arena| arena.total_bytes_allocated())
            .unwrap();
        let current_nodes = session.with_arena(|arena| arena.node_count()).unwrap();
        let budget = ArenaBudget::new(current_bytes + node_bytes, current_nodes + 1);
        session.set_arena_budget(budget);
        assert_eq!(session.arena_budget(), budget);

        session
            .with_arena(|arena| {
                assert!(!arena.alloc_node(SEXPTYPE::INTSXP).is_null());
                assert!(arena.alloc_node(SEXPTYPE::REALSXP).is_null());
            })
            .unwrap();
    }

    #[test]
    fn test_session_default() {
        let session = RSession::default();
        assert!(session.is_active());
    }
}
