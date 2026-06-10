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

use std::cell::RefCell;
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
use super::globals::{R_MissingArg, R_NilValue, R_RestartToken, R_UnboundValue};
use super::instance::{
    RInstance, clear_current_instance_if, replace_current_instance, set_current_instance,
};
use super::memory::{ArenaBudget, RArena};
use super::object::Sexp;
use super::protect::protect_sexp;
#[cfg(test)]
use super::protect::protect;
#[cfg(test)]
use super::protect::{R_ProtectCount, protect_n};

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

struct ProtectScope<'a> {
    stack: &'a RefCell<Vec<SEXP>>,
    depth: usize,
}

impl<'a> ProtectScope<'a> {
    fn new(stack: &'a RefCell<Vec<SEXP>>) -> Self {
        let depth = stack.borrow().len();
        Self { stack, depth }
    }
}

impl Drop for ProtectScope<'_> {
    fn drop(&mut self) {
        self.stack.borrow_mut().truncate(self.depth);
    }
}

/// An R interpreter session with its own isolated instance state.
///
/// Each `RSession` owns an [`RInstance`] containing a private arena,
/// environment chain, and protection stack. When a session is active,
/// all compatibility accessor functions (`R_GlobalEnv`, protection APIs,
/// `with_arena`, etc.) dispatch to the session's instance.
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
    /// Marker that keeps sessions thread-confined at compile time.
    _thread_confined: PhantomData<Rc<()>>,
}

impl RSession {
    /// Create a new R session with its own isolated instance.
    ///
    /// Initializes a fresh [`RInstance`] with its own arena and environment
    /// chain, and installs it as the current compatibility instance on this
    /// thread. Session methods still scope activation explicitly, so nested
    /// operations restore the previous instance.
    pub fn new() -> Self {
        super::context::install_r_panic_hook();
        let mut instance = Box::new(RInstance::new());
        unsafe {
            set_current_instance(&mut *instance as *mut RInstance);
        }
        RSession {
            active: true,
            instance,
            _thread_confined: PhantomData,
        }
    }

    /// Create a session without leaving it installed as the thread's ambient
    /// compatibility instance.
    ///
    /// This is the preferred constructor for app-facing embedding layers.
    /// Session methods still activate the instance for legacy internals, but
    /// merely constructing an Android worker session no longer changes what
    /// unrelated translated code on the same thread sees as current.
    pub(crate) fn new_detached() -> Self {
        let previous = unsafe { replace_current_instance(None) };
        let session = Self::new();
        clear_current_instance_if(&*session.instance);
        unsafe {
            replace_current_instance(previous);
        }
        session
    }

    fn instance_ptr(&self) -> *mut RInstance {
        (&*self.instance as *const RInstance).cast_mut()
    }

    fn activate(&self) -> CurrentInstanceGuard {
        let previous = unsafe { replace_current_instance(Some(self.instance_ptr())) };
        CurrentInstanceGuard { previous }
    }

    pub(crate) fn with_active<F, T>(&self, f: F) -> T
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
        if is_immutable_singleton(ptr) {
            Some(unsafe { Sexp::from_static_raw_unchecked(ptr) })
        } else if self.instance.arena.contains(ptr) {
            Sexp::from_arena_raw(ptr, &self.instance.arena).ok()
        } else if self.instance.owns_sexp(ptr) {
            Sexp::from_session_raw(ptr, &self.instance).ok()
        } else {
            None
        }
    }

    fn owned_sexp<'session>(
        &'session self,
        ptr: SEXP,
        description: &'static str,
    ) -> RResult<Sexp<'session>> {
        self.sexp(ptr).ok_or_else(|| REvalError {
            message: format!("{description} does not belong to this session"),
        })
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
    pub(crate) fn eval(&self, expr: SEXP) -> RResult<SEXP> {
        self.eval_sexp_raw(expr).map(Sexp::as_raw)
    }

    /// Evaluate a raw expression pointer after proving it belongs to this session.
    pub(crate) fn eval_sexp_raw(&self, expr: SEXP) -> RResult<Sexp<'_>> {
        let expr = expr_or_nil(expr);
        let expr = self.owned_sexp(expr, "expression")?;
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
            let expr = self.owned_sexp(expr.as_raw(), "expression")?;
            let env = self.global_env().ok_or_else(|| REvalError {
                message: "session has no global environment".to_string(),
            })?;
            let result = catch_eval_result(|| crate::eval::eval::EvalContext::new(env).eval(expr))?;
            self.owned_sexp(result.as_raw(), "evaluation result")
        })
    }

    /// Evaluate an expression while capturing output and the final visibility flag.
    ///
    /// This mirrors the top-level embedding contract: explicit output produced by
    /// functions such as `print()` and `cat()` is captured separately from the
    /// implicit printing controlled by `R_Visible`.
    pub(crate) fn eval_with_output_capture(
        &self,
        expr: SEXP,
    ) -> (RResult<SEXP>, super::output::RCapturedOutput, bool) {
        let expr = match self.owned_sexp(expr_or_nil(expr), "expression") {
            Ok(expr) => expr,
            Err(err) => {
                return (Err(err), super::output::RCapturedOutput::default(), false);
            }
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
            self.instance.output_capture.borrow_mut().start();
            let result = self.eval_sexp(expr);
            let visible = self.instance.eval_state.visible != 0;
            let output = self.instance.output_capture.borrow_mut().stop();
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
        self.eval_code_with_output_capture_then(code, |result, output, visible| {
            (result, output, visible)
        })
    }

    /// Parse and evaluate source code, then map the result while this session
    /// is still the scoped active instance.
    ///
    /// Embedding facades use this to convert a borrowed `Sexp` into owned
    /// values without depending on any ambient current session after the eval
    /// call has returned.
    /// Parse and evaluate a multi-expression R script.
    pub fn eval_script_with_output_capture<'session>(
        &'session mut self,
        code: &str,
    ) -> (
        RResult<Sexp<'session>>,
        super::output::RCapturedOutput,
        bool,
    ) {
        self.eval_script_with_output_capture_then(code, |result, output, visible| {
            (result, output, visible)
        })
    }

    pub(crate) fn eval_script_with_output_capture_then<'session, T, F>(
        &'session mut self,
        code: &str,
        f: F,
    ) -> T
    where
        F: FnOnce(RResult<Sexp<'session>>, super::output::RCapturedOutput, bool) -> T,
    {
        if !self.active {
            return f(
                Err(REvalError {
                    message: "session is closed".to_string(),
                }),
                super::output::RCapturedOutput::default(),
                false,
            );
        }

        let expressions = {
            let _guard = self.activate();
            crate::eval::parser::parse_expressions(code, &mut self.instance.arena)
        };
        let expressions = match expressions {
            Ok(exprs) => exprs,
            Err(err) => {
                return f(
                    Err(REvalError {
                        message: err.to_string(),
                    }),
                    super::output::RCapturedOutput::default(),
                    false,
                );
            }
        };

        self.with_active(|| {
            self.instance.output_capture.borrow_mut().start();
            let mut result: RResult<Sexp<'session>> =
                Ok(unsafe { Sexp::from_raw_unchecked(R_NilValue()) });
            for raw_expr in expressions {
                let expr = match self.owned_sexp(expr_or_nil(raw_expr), "parsed expression") {
                    Ok(expr) => expr,
                    Err(err) => {
                        result = Err(err);
                        break;
                    }
                };
                result = self.eval_sexp(expr);
                let _expr_guard = result
                    .as_ref()
                    .ok()
                    .map(|value| protect_sexp(*value));
                crate::sexp::gengc::run_pending_gc_if_quiescent();
            }
            let _result_guard = result
                .as_ref()
                .ok()
                .map(|value| protect_sexp(*value));
            crate::sexp::gengc::run_pending_gc_if_quiescent();
            let visible = self.instance.eval_state.visible != 0;
            let output = self.instance.output_capture.borrow_mut().stop();
            f(result, output, visible)
        })
    }

    pub(crate) fn eval_code_with_output_capture_then<'session, T, F>(
        &'session mut self,
        code: &str,
        f: F,
    ) -> T
    where
        F: FnOnce(RResult<Sexp<'session>>, super::output::RCapturedOutput, bool) -> T,
    {
        if !self.active {
            return f(
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
                return f(
                    Err(REvalError {
                        message: err.to_string(),
                    }),
                    super::output::RCapturedOutput::default(),
                    false,
                );
            }
        };
        let expr = match self.owned_sexp(raw_expr, "parsed expression") {
            Ok(expr) => expr,
            Err(err) => {
                return f(Err(err), super::output::RCapturedOutput::default(), false);
            }
        };
        self.with_active(|| {
            self.instance.output_capture.borrow_mut().start();
            let result = self.eval_sexp(expr);
            let _result_guard = result
                .as_ref()
                .ok()
                .map(|value| protect_sexp(*value));
            crate::sexp::gengc::run_pending_gc_if_quiescent();
            let visible = self.instance.eval_state.visible != 0;
            let output = self.instance.output_capture.borrow_mut().stop();
            f(result, output, visible)
        })
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
    pub(crate) fn eval_in(&self, expr: SEXP, env: SEXP) -> RResult<SEXP> {
        let expr = self.owned_sexp(expr_or_nil(expr), "expression")?;
        let env = self.owned_sexp(env, "environment")?;
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
            let expr = self.owned_sexp(expr.as_raw(), "expression")?;
            let env = self.owned_sexp(env.as_raw(), "environment")?;
            let result = catch_eval_result(|| crate::eval::eval::EvalContext::new(env).eval(expr))?;
            self.owned_sexp(result.as_raw(), "evaluation result")
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
            let symbol = self.sexp(install_symbol(name)?)?;
            let env = Environment::new(self.global_env()?).ok()?;
            let result = env.find(symbol).ok().flatten()?;
            if result.as_raw() == unsafe { R_UnboundValue() }
                || result.as_raw() == unsafe { R_NilValue() }
            {
                None
            } else {
                Some(result)
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
        self.define_var_raw(name, value.as_raw())
    }

    /// Define a variable from a raw pointer for internal compatibility paths.
    ///
    /// The raw value is accepted only after proving it belongs to this session
    /// or is one of R's immutable singleton sentinels.
    fn define_var_raw(&self, name: &str, value: SEXP) -> bool {
        if !self.active {
            return false;
        }
        self.with_active(|| {
            let Some(value) = self.sexp(value) else {
                return false;
            };
            let Some(symbol) = install_symbol(name) else {
                return false;
            };
            let Some(symbol) = self.sexp(symbol) else {
                return false;
            };
            let Some(env) = self.global_env() else {
                return false;
            };
            Environment::new(env)
                .and_then(|env| env.define(symbol, value))
                .is_ok()
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
    /// use rmath::sexp::Sexp;
    /// use rmath::sexp::protect::protect_sexp;
    ///
    /// let session = RSession::new();
    /// session.with_protected(|| {
    ///     let _guard = protect_sexp(Sexp::nil());
    /// });
    /// ```
    pub fn with_protected<F, T>(&self, f: F) -> T
    where
        F: FnOnce() -> T,
    {
        self.with_active(|| {
            let _scope = ProtectScope::new(&self.instance.protect_stack);
            f()
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

    /// Set or clear this session's cooperative cancellation token.
    ///
    /// Evaluator loop checks read this token through the active `RInstance`.
    /// The token can be cloned and held by an embedding host so cancellation
    /// remains session-scoped while the synchronization detail stays private.
    pub fn set_cancellation_token(&mut self, token: Option<CancellationToken>) {
        if self.active {
            self.instance.eval_state.cancellation = token;
        }
    }

    /// Replace this session's cooperative cancellation token, returning the old one.
    ///
    /// This gives embedders a scoped, owner-checked way to install a
    /// cancellation token for one evaluation and then restore the previous
    /// state. Closed sessions ignore new flags.
    pub fn replace_cancellation_token(
        &mut self,
        token: Option<CancellationToken>,
    ) -> Option<CancellationToken> {
        if self.active {
            std::mem::replace(&mut self.instance.eval_state.cancellation, token)
        } else {
            None
        }
    }

    /// Return this session's current evaluation limits.
    pub fn eval_limits(&self) -> crate::eval::eval::EvalLimits {
        self.instance.eval_state.limits
    }

    /// Set this session's evaluation limits.
    ///
    /// This is the session-owned facade for the evaluator's historical
    /// current-instance limit accessors.
    pub fn set_eval_limits(&mut self, limits: crate::eval::eval::EvalLimits) {
        if self.active {
            self.instance.eval_state.limits = limits;
        }
    }

    /// Reset this session's evaluation limits to the evaluator defaults.
    pub fn reset_eval_limits(&mut self) {
        self.set_eval_limits(crate::eval::eval::EvalLimits::default());
    }

    /// Return this session's capability flags for host-process operations.
    pub fn capabilities(&self) -> super::instance::SessionCapabilities {
        self.instance.eval_state.capabilities
    }

    /// Configure which host-process operations this session may invoke.
    pub fn set_capabilities(&mut self, capabilities: super::instance::SessionCapabilities) {
        if self.active {
            self.instance.eval_state.capabilities = capabilities;
        }
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
            self.instance.output_capture.borrow_mut().start();
            let value = f();
            let output = self.instance.output_capture.borrow_mut().stop();
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
    use crate::sexp::instance::{
        current_instance_ptr, replace_current_instance, with_current_instance,
    };
    use crate::sexp::protect::{R_PreserveObject, R_ReleaseObject, with_preserved_objects};

    #[test]
    fn test_session_creation() {
        let session = RSession::new();
        assert!(session.is_active());
        assert!(session.global_env().is_some());
        assert!(session.base_env().is_some());
        assert!(matches!(
            session
                .global_env()
                .expect("session has global env")
                .owner(),
            crate::sexp::object::SexpOwner::Session(_)
        ));
    }

    #[test]
    fn test_detached_session_constructor_restores_current_instance() {
        let current = RSession::new();
        let current_ptr = current.instance_ptr();

        let detached = RSession::new_detached();
        assert!(detached.is_active());
        assert_eq!(current_instance_ptr(), Some(current_ptr));

        let result = detached.with_output_capture(|| {
            crate::sexp::output::capture_stdout("detached");
        });
        assert_eq!(result.1.stdout, "detached");
        assert_eq!(current_instance_ptr(), Some(current_ptr));
    }

    #[test]
    fn test_session_sexp_rejects_foreign_arena_pointer() {
        let mut left = RSession::new();
        let right = RSession::new();

        let ptr = left
            .with_arena(|arena| arena.alloc_node(SEXPTYPE::INTSXP))
            .expect("left session should be active");

        let left_value = left.sexp(ptr).expect("left owns pointer");
        assert!(matches!(
            left_value.owner(),
            crate::sexp::object::SexpOwner::Arena(_)
        ));
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
        assert!(result.is_owner_scoped());
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

        let previous = unsafe { replace_current_instance(Some(newer.instance_ptr())) };
        older.close();

        assert!(
            with_current_instance(|inst| std::ptr::eq(inst as *const RInstance, newer_instance))
                .unwrap_or(false)
        );
        assert!(newer.is_active());
        unsafe {
            replace_current_instance(previous);
        }
    }

    #[test]
    fn test_session_drop_non_current_keeps_current_instance() {
        let older = RSession::new();
        let newer = RSession::new();
        let newer_instance = &*newer.instance as *const RInstance;

        let previous = unsafe { replace_current_instance(Some(newer.instance_ptr())) };
        drop(older);

        assert!(
            with_current_instance(|inst| std::ptr::eq(inst as *const RInstance, newer_instance))
                .unwrap_or(false)
        );
        assert!(newer.is_active());
        unsafe {
            replace_current_instance(previous);
        }
    }

    #[test]
    fn test_session_method_activation_restores_previous_instance() {
        let mut older = RSession::new();
        let older_instance = &*older.instance as *const RInstance;
        let newer = RSession::new();
        let newer_instance = &*newer.instance as *const RInstance;

        let previous = unsafe { replace_current_instance(Some(newer.instance_ptr())) };
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
        unsafe {
            replace_current_instance(previous);
        }
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
    fn test_session_eval_limits_are_local_on_same_thread() {
        let mut left = RSession::new();
        let mut right = RSession::new();

        left.set_eval_limits(crate::eval::eval::EvalLimits {
            max_eval_depth: 7,
            max_execution_time_ms: 0,
            max_alloc_bytes: 0,
        });
        right.set_eval_limits(crate::eval::eval::EvalLimits {
            max_eval_depth: 19,
            max_execution_time_ms: 0,
            max_alloc_bytes: 0,
        });

        assert_eq!(left.eval_limits().max_eval_depth, 7);
        assert_eq!(right.eval_limits().max_eval_depth, 19);
        assert_eq!(
            left.with_active(crate::eval::eval::get_eval_limits)
                .max_eval_depth,
            7
        );
        assert_eq!(
            right
                .with_active(crate::eval::eval::get_eval_limits)
                .max_eval_depth,
            19
        );

        left.reset_eval_limits();
        assert_eq!(left.eval_limits(), crate::eval::eval::EvalLimits::default());
        assert_eq!(right.eval_limits().max_eval_depth, 19);
    }

    #[test]
    fn test_eval_depth_guard_drops_against_original_session() {
        let left = RSession::new();
        let right = RSession::new();

        left.with_active(|| {
            let guard = crate::eval::eval::check_eval_depth().expect("left depth should increment");
            assert_eq!(left.instance.eval_state.eval_depth, 1);
            assert_eq!(right.instance.eval_state.eval_depth, 0);

            let previous = unsafe { replace_current_instance(Some(right.instance_ptr())) };
            drop(guard);
            assert_eq!(left.instance.eval_state.eval_depth, 0);
            assert_eq!(right.instance.eval_state.eval_depth, 0);
            unsafe {
                replace_current_instance(previous);
            }
        });
    }

    #[test]
    fn test_eval_timer_guard_drops_against_original_session() {
        let left = RSession::new();
        let right = RSession::new();

        left.with_active(|| {
            let guard = crate::eval::eval::EvalTimerGuard::start_if_needed();
            assert!(left.instance.eval_state.start_time.is_some());
            assert!(right.instance.eval_state.start_time.is_none());

            let previous = unsafe { replace_current_instance(Some(right.instance_ptr())) };
            drop(guard);
            assert!(left.instance.eval_state.start_time.is_none());
            assert!(right.instance.eval_state.start_time.is_none());
            unsafe {
                replace_current_instance(previous);
            }
        });
    }

    #[test]
    fn test_eval_with_limits_restores_session_state() {
        let mut session = RSession::new();
        let original_limits = crate::eval::eval::EvalLimits {
            max_eval_depth: 11,
            max_execution_time_ms: 0,
            max_alloc_bytes: 0,
        };
        session.set_eval_limits(original_limits);
        session.instance.eval_state.start_time = Some(std::time::Instant::now());

        let expr = session
            .with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
            .expect("session should be active");
        let expr = session.sexp(expr).expect("expr belongs to session");
        assert!(expr.set_integer_elt(0, 7));
        let env = session.global_env().expect("session has global env");

        let result = session.with_active(|| {
            crate::eval::eval::eval_with_limits(
                expr,
                env,
                crate::eval::eval::EvalLimits {
                    max_eval_depth: 3,
                    max_execution_time_ms: 1_000,
                    max_alloc_bytes: 0,
                },
            )
        });
        let result = result.expect("self-evaluating vector should evaluate");
        assert_eq!(result.integer_elt(0), Some(7));
        assert_eq!(session.instance.eval_state.limits, original_limits);
        assert!(session.instance.eval_state.start_time.is_some());
    }

    #[test]
    fn test_session_cancellation_token_is_session_owned() {
        let mut cancelled = RSession::new();
        let mut active = RSession::new();
        let token = CancellationToken::cancelled();

        cancelled.set_cancellation_token(Some(token));
        let (result, _, _) = cancelled.eval_code_with_output_capture("1 + 1");
        let err = result.expect_err("cancelled session should reject eval");
        assert_eq!(err.message, "operation cancelled");

        let (result, _, _) = active.eval_code_with_output_capture("1 + 1");
        let result = result.expect("second session should not inherit cancellation");
        assert_eq!(result.real_elt(0), Some(2.0));
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
        let mut session = RSession::new();
        let value = session
            .with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
            .expect("session should be active");
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
        let mut session = RSession::new();
        let value = session
            .with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
            .expect("session should be active");
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
        let depth_before = session.with_active(R_ProtectCount);
        session.with_protected(|| {
            std::mem::forget(protect(0x1 as SEXP));
            std::mem::forget(protect(0x2 as SEXP));
        });
        let depth_after = session.with_active(R_ProtectCount);
        assert_eq!(depth_before, depth_after);
    }

    #[test]
    fn test_session_protected_scope_unwinds_on_panic() {
        let session = RSession::new();
        let depth_before = session.with_active(R_ProtectCount);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session.with_protected(|| {
                std::mem::forget(protect(0x1 as SEXP));
                panic!("forced protected-scope unwind");
            });
        }));

        assert!(result.is_err());
        let depth_after = session.with_active(R_ProtectCount);
        assert_eq!(depth_before, depth_after);
    }

    #[test]
    fn test_session_protect_and_preserve_are_local_on_same_thread() {
        let mut left = RSession::new();
        let mut right = RSession::new();
        let protected = 0x1 as SEXP;
        let preserved = 0x2 as SEXP;

        left.with_arena(|_| {
            std::mem::forget(protect(protected));
            unsafe {
                R_PreserveObject(preserved);
            }
            assert_eq!(R_ProtectCount(), 1);
            with_preserved_objects(|objects| assert_eq!(objects, &[preserved]));
        });

        right.with_arena(|_| {
            assert_eq!(R_ProtectCount(), 0);
            with_preserved_objects(|objects| assert!(objects.is_empty()));
        });

        left.with_arena(|_| {
            drop(protect_n(1));
            unsafe {
                R_ReleaseObject(preserved);
            }
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

    // -----------------------------------------------------------------------
    // Concurrent session stress tests
    // -----------------------------------------------------------------------

    #[test]
    fn concurrent_sessions_are_independent() {
        // Spawn 4 threads, each creating their own RSession, defining a
        // unique variable, evaluating code, and verifying isolation.
        let handles: Vec<_> = (0..4)
            .map(|i| {
                std::thread::spawn(move || {
                    let mut session = RSession::new();
                    assert!(session.is_active());
                    assert!(session.global_env().is_some());

                    // Define a variable unique to this thread/session
                    let var_name = format!("thread_var_{i}");
                    let value = session
                        .with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1))
                        .expect("session should be active");
                    let sexp = Sexp::from_raw(value).expect("allocation failed");
                    assert!(sexp.set_integer_elt(0, (i + 100) as i32));

                    let value = session.sexp(value).expect("value belongs to session");
                    assert!(session.define_var(&var_name, value));

                    // Verify the variable
                    let found = session
                        .find_var(&var_name)
                        .expect("should find the variable we just defined");
                    assert_eq!(
                        found.integer_elt(0),
                        Some((i + 100) as i32),
                        "thread {i} got wrong value"
                    );

                    // Verify other threads' variables are NOT visible
                    for j in 0..4 {
                        if j != i {
                            let other_name = format!("thread_var_{j}");
                            assert!(
                                session.find_var(&other_name).is_none(),
                                "thread {i} should NOT see thread {j}'s variable"
                            );
                        }
                    }

                    session.close();
                    assert!(!session.is_active());
                    (i, true)
                })
            })
            .collect();

        for h in handles {
            let (thread_id, ok) = h.join().expect("thread should not panic");
            assert!(ok, "thread {thread_id} failed");
        }
    }

    #[test]
    fn concurrent_sessions_eval_code_independently() {
        // Each thread evaluates R code and checks isolation of captured output.
        let handles: Vec<_> = (0..4)
            .map(|i| {
                std::thread::spawn(move || {
                    let mut session = RSession::new();

                    let code = format!("{i} + 100");
                    let (result, _output, _visible) =
                        session.eval_code_with_output_capture(&code);
                    let result = result.expect("eval should succeed");
                    assert_eq!(
                        result.real_elt(0),
                        Some((i + 100) as f64),
                        "thread {i} computed wrong result"
                    );
                    session.close();
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread should not panic");
        }
    }

    #[test]
    fn concurrent_sessions_output_capture_isolated() {
        // Verify that output capture in one session doesn't leak to another
        // session on a different thread.
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let barrier_clone = barrier.clone();

        let h1 = std::thread::spawn(move || {
            let session = RSession::new();
            barrier_clone.wait(); // synchronize start
            let (_, output) = session.with_output_capture(|| {
                crate::sexp::output::capture_stdout("thread_one_output");
            });
            assert_eq!(output.stdout, "thread_one_output");
            // Must NOT contain thread_two's output
            assert!(
                !output.stdout.contains("thread_two"),
                "output leaked between threads"
            );
        });

        let h2 = std::thread::spawn(move || {
            let session = RSession::new();
            barrier.wait(); // synchronize start
            let (_, output) = session.with_output_capture(|| {
                crate::sexp::output::capture_stdout("thread_two_output");
            });
            assert_eq!(output.stdout, "thread_two_output");
            assert!(
                !output.stdout.contains("thread_one"),
                "output leaked between threads"
            );
        });

        h1.join().expect("thread 1 panicked");
        h2.join().expect("thread 2 panicked");
    }

    #[test]
    fn concurrent_sessions_rng_independent() {
        // RNG state should be per-session. Two threads with the same seed
        // should get the same sequence, and different threads should not
        // interfere with each other.
        let handles: Vec<_> = (0..2)
            .map(|i| {
                std::thread::spawn(move || {
                    let session = RSession::new();
                    session.set_seed(42, 42);
                    let r1 = session.unif_rand();
                    let r2 = session.unif_rand();
                    // Same seed should give same first value across sessions
                    (i, r1, r2)
                })
            })
            .collect();

        let results: Vec<_> = handles
            .into_iter()
            .map(|h| h.join().expect("thread panicked"))
            .collect();

        // Both threads used seed (42,42), so they should get the same sequence
        assert_eq!(
            results[0].1, results[1].1,
            "same seed should produce same first random number"
        );
        assert_eq!(
            results[0].2, results[1].2,
            "same seed should produce same second random number"
        );
    }
}
