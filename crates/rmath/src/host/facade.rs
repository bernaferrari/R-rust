//! The public host session facade: owned-value evaluation results, error
//! adaptation, cancellation, value handles, and interactive graphics.
//!
//! These types and methods are the canonical public embedding surface —
//! what `r-embed` re-exports — merged onto the one host `RSession` so the
//! layering collapses to core session + public session exactly as the
//! pure-Rust foundation plan pins.

use super::{RSession, RValue};
use std::sync::atomic::{AtomicU64, Ordering};

/// Process-wide counter assigning each session a unique id.
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

/// Allocate the next session identity. Wraps at u64 exhaustion — the
/// facade never exposes failure for what is unreachable in practice.
pub(crate) fn next_session_id() -> u64 {
    NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed)
}

/// Errors that can occur during R session operations.
#[derive(Debug)]
pub enum RSessionError {
    /// Session initialization failed.
    InitFailed(String),
    /// Evaluation produced an R error.
    EvalError(String),
    /// Evaluation errored after producing output and possibly drawing.
    EvalErrorWithOutput {
        message: String,
        output: String,
        png: Option<Vec<u8>>,
    },
    /// Graphics rendering failed.
    RenderError(String),
}

impl std::fmt::Display for RSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RSessionError::InitFailed(message) => {
                write!(f, "Failed to initialize R session: {message}")
            }
            RSessionError::EvalError(message) => write!(f, "Evaluation error: {message}"),
            RSessionError::EvalErrorWithOutput { message, .. } => {
                write!(f, "Evaluation error: {message}")
            }
            RSessionError::RenderError(message) => write!(f, "Render error: {message}"),
        }
    }
}

impl std::error::Error for RSessionError {}

/// Owned result of an evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct EvalOutput {
    pub output: String,
    pub value: RValue,
}

/// The combined result of one interactive evaluation.
#[cfg(feature = "renderplot-device")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractiveOutput {
    pub output: String,
    pub png: Option<Vec<u8>>,
}

/// Derived runtime paths for app-private embedding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePaths {
    pub app_files_dir: String,
    pub cache_dir: String,
    pub bundled_library_dir: Option<String>,
}

impl RuntimePaths {
    pub fn new(
        app_files_dir: impl Into<String>,
        cache_dir: impl Into<String>,
        bundled_library_dir: Option<impl Into<String>>,
    ) -> Self {
        Self {
            app_files_dir: app_files_dir.into(),
            cache_dir: cache_dir.into(),
            bundled_library_dir: bundled_library_dir.map(Into::into),
        }
    }

    pub fn user_library_dir(&self) -> String {
        std::path::PathBuf::from(&self.app_files_dir)
            .join("R")
            .join("library")
            .to_string_lossy()
            .into_owned()
    }

    pub fn temp_dir(&self) -> String {
        std::path::PathBuf::from(&self.cache_dir)
            .join("Rtmp")
            .to_string_lossy()
            .into_owned()
    }

    pub fn library_paths(&self) -> Vec<String> {
        let mut paths = vec![self.user_library_dir()];
        if let Some(path) = &self.bundled_library_dir {
            paths.push(path.clone());
        }
        paths
    }
}

/// Back-compatible name from the Android-first embedding era.
pub type AndroidRuntimePaths = RuntimePaths;

/// Cooperative cancellation handle for an embedded evaluation.
///
/// The token is cheap to clone and can be cancelled from another thread. It is
/// scoped to explicit evaluations that receive it; cancelling one token does
/// not affect other sessions.
#[derive(Debug, Clone)]
pub struct HostCancellationToken {
    inner: crate::sexp::session::CancellationToken,
}

impl HostCancellationToken {
    pub fn new() -> Self {
        Self {
            inner: crate::sexp::session::CancellationToken::new(),
        }
    }

    pub fn cancel(&self) {
        self.inner.request();
    }

    /// The shared cancellation cell for the pure world's evaluator —
    /// crate-visible exactly as the migration boundary needs.
    pub(crate) fn shared_flag(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        self.inner.shared_flag()
    }

    pub fn reset(&self) {
        self.inner.reset();
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.is_requested()
    }

    fn token(&self) -> crate::sexp::session::CancellationToken {
        self.inner.clone()
    }
}

impl Default for HostCancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl RSession {
    /// Unique id assigned at construction from a process-wide counter.
    pub fn session_id(&self) -> u64 {
        self.session_id
    }

    /// Evaluate an R expression, returning the output as a string.
    pub fn eval(&mut self, code: &str) -> Result<String, RSessionError> {
        if !self.is_active() {
            return Err(RSessionError::EvalError("Session closed".into()));
        }
        let result = self.eval_display(code);
        match result.typed {
            RValue::Error(message) => Err(RSessionError::EvalError(message)),
            _ => Ok(result.output),
        }
    }

    /// Evaluate a multi-expression R script.
    pub fn eval_script(&mut self, code: &str) -> Result<EvalOutput, RSessionError> {
        self.eval_script_with_cancel(code, None)
    }

    fn eval_script_with_cancel(
        &mut self,
        code: &str,
        cancellation: Option<crate::sexp::session::CancellationToken>,
    ) -> Result<EvalOutput, RSessionError> {
        if !self.is_active() {
            return Err(RSessionError::EvalError("Session closed".into()));
        }

        let result = self.eval_script_with_cancellation_token(code, cancellation);

        match result.typed {
            RValue::Error(message) => {
                if message == "operation cancelled" {
                    Err(RSessionError::EvalError("operation cancelled".to_string()))
                } else {
                    Err(RSessionError::EvalError(message))
                }
            }
            value => Ok(EvalOutput {
                output: result.output,
                value,
            }),
        }
    }

    /// Evaluate an R expression, returning both display output and an owned
    /// typed value.
    pub fn eval_result(&mut self, code: &str) -> Result<EvalOutput, RSessionError> {
        self.eval_result_with_cancel(code, None)
    }

    /// Evaluate an R expression with a cooperative cancellation token.
    pub fn eval_result_cancellable(
        &mut self,
        code: &str,
        cancellation: &HostCancellationToken,
    ) -> Result<EvalOutput, RSessionError> {
        self.eval_result_with_cancel(code, Some(cancellation.token()))
    }

    fn eval_result_with_cancel(
        &mut self,
        code: &str,
        cancellation: Option<crate::sexp::session::CancellationToken>,
    ) -> Result<EvalOutput, RSessionError> {
        if !self.is_active() {
            return Err(RSessionError::EvalError("Session closed".into()));
        }

        let result = self.eval_with_cancellation_token(code, cancellation);

        match result.typed {
            RValue::Error(message) => {
                if message == "operation cancelled" {
                    Err(RSessionError::EvalError("operation cancelled".to_string()))
                } else {
                    Err(RSessionError::EvalError(message))
                }
            }
            value => Ok(EvalOutput {
                output: result.output,
                value,
            }),
        }
    }

    /// Return whether `code` is a syntactically complete R input.
    pub fn is_input_complete(&mut self, code: &str) -> Result<bool, RSessionError> {
        if !self.is_active() {
            return Err(RSessionError::EvalError("Session closed".into()));
        }
        self.is_syntax_complete(code)
            .map_err(RSessionError::EvalError)
    }

    /// Configure app-private R runtime paths.
    pub fn configure_runtime_paths(
        &mut self,
        app_files_dir: &str,
        cache_dir: &str,
        bundled_library_dir: Option<&str>,
    ) -> Result<(), RSessionError> {
        self.configure_paths(app_files_dir, cache_dir, bundled_library_dir)
            .map_err(RSessionError::InitFailed)
    }

    /// Back-compatible Android-era name for runtime path configuration.
    pub fn configure_android_paths(
        &mut self,
        app_files_dir: &str,
        cache_dir: &str,
        bundled_library_dir: Option<&str>,
    ) -> Result<(), RSessionError> {
        self.configure_runtime_paths(app_files_dir, cache_dir, bundled_library_dir)
    }

    /// Configure paths from a single helper value with derived runtime
    /// locations for package libraries and temp files.
    pub fn configure_runtime(&mut self, paths: &RuntimePaths) -> Result<(), RSessionError> {
        self.configure_runtime_paths(
            &paths.app_files_dir,
            &paths.cache_dir,
            paths.bundled_library_dir.as_deref(),
        )
    }

    /// Back-compatible Android-era name.
    pub fn configure_android_runtime(
        &mut self,
        paths: &RuntimePaths,
    ) -> Result<(), RSessionError> {
        self.configure_runtime(paths)
    }

    /// Return metadata for a package if it is visible in this session's
    /// configured library paths.
    pub fn package_info(&self, package: &str) -> Option<super::packages::RPackageInfo> {
        let package_path = self.package_path(package)?;
        let library_paths = self.runtime_info().library_paths;
        super::packages::package_info_from_path(
            package,
            &std::path::PathBuf::from(&package_path),
            &library_paths,
        )
    }

    /// Return metadata for installed packages visible in this session.
    pub fn installed_packages(&self) -> Vec<super::packages::RPackageInfo> {
        super::packages::installed_packages_from_library_paths(&self.runtime_info().library_paths)
    }

    /// Evaluate `expr` globally and retain its successful result privately.
    /// The owning root survives evaluation and collection until removal or close.
    pub fn define_handle(&mut self, expr: &str) -> Result<ValueHandle, RSessionError> {
        let value = self.define_retained(expr).map_err(RSessionError::EvalError)?;
        Ok(ValueHandle {
            session_id: self.session_id,
            value,
        })
    }

    /// Borrow a directly copied typed snapshot. Reading never evaluates R code
    /// or invokes an R printing method.
    pub fn read_handle<'s>(&'s mut self, handle: &ValueHandle) -> Result<ReadGuard<'s>, RSessionError> {
        self.validate_handle(handle)?;
        let value = self
            .retained_snapshot(handle.value)
            .map_err(RSessionError::EvalError)?;
        Ok(ReadGuard {
            session: self,
            snapshot: EvalOutput {
                output: String::new(),
                value,
            },
        })
    }

    /// Exclusively borrow the checked identity for subsequent writes.
    pub fn write_handle<'s>(
        &'s mut self,
        handle: &ValueHandle,
    ) -> Result<WriteGuard<'s>, RSessionError> {
        self.validate_handle(handle)?;
        Ok(WriteGuard {
            session: self,
            value: handle.value,
        })
    }

    /// Release the private owning root and invalidate every copy of the handle.
    pub fn remove_handle(&mut self, handle: &ValueHandle) -> Result<(), RSessionError> {
        self.validate_handle(handle)?;
        self.remove_retained(handle.value)
            .map_err(RSessionError::EvalError)
    }

    fn validate_handle(&self, handle: &ValueHandle) -> Result<(), RSessionError> {
        if !self.is_active() {
            return Err(RSessionError::EvalError("Session closed".into()));
        }
        if handle.session_id != self.session_id {
            return Err(RSessionError::EvalError(format!(
                "value handle belongs to session {}, used on session {}",
                handle.session_id, self.session_id
            )));
        }
        self.validate_retained(handle.value)
            .map_err(RSessionError::EvalError)
    }
}

/// Opaque, copyable identity for a privately retained R value.
///
/// Engine validation rejects foreign sessions, removed generations and closed
/// sessions before accessing the value. Owning roots are independent of R
/// bindings. No raw heap references cross the host boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ValueHandle {
    session_id: u64,
    value: super::RetainedValueId,
}

impl ValueHandle {
    /// The session this handle was created by.
    pub fn owning_session(&self) -> u64 {
        self.session_id
    }
}

/// Shared read access to a handle's value, scoped to the session borrow.
///
/// The guard materializes an owned [`RValue`] snapshot when created; while it
/// is alive the session is exclusively borrowed, so no evaluation can run
/// concurrently and the observed snapshot cannot be invalidated.
pub struct ReadGuard<'s> {
    session: &'s mut RSession,
    snapshot: EvalOutput,
}

impl std::ops::Deref for ReadGuard<'_> {
    type Target = RValue;

    fn deref(&self) -> &RValue {
        &self.snapshot.value
    }
}

impl std::fmt::Debug for ReadGuard<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadGuard")
            .field("snapshot", &self.snapshot)
            .finish()
    }
}

impl ReadGuard<'_> {
    /// Reads produce no console output; this always returns an empty string.
    pub fn output(&self) -> &str {
        &self.snapshot.output
    }

    /// The id of the session this guard borrows.
    pub fn session_id(&self) -> u64 {
        self.session.session_id()
    }

    /// The owned value snapshot.
    pub fn value(&self) -> &RValue {
        &self.snapshot.value
    }
}

/// Exclusive write access to a handle's slot, scoped to the session borrow.
///
/// Dropping the guard releases the session borrow; its owning root persists
/// until [`RSession::remove_handle`] or session close.
pub struct WriteGuard<'s> {
    session: &'s mut RSession,
    value: super::RetainedValueId,
}

impl std::fmt::Debug for WriteGuard<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriteGuard")
            .field("value", &self.value)
            .finish()
    }
}

impl WriteGuard<'_> {
    /// Replace the slot's value with the result of evaluating `expr`.
    ///
    /// On error the slot keeps its previous value. Ordinary R side effects remain.
    pub fn set(&mut self, expr: &str) -> Result<(), RSessionError> {
        self.session
            .set_retained(self.value, expr)
            .map_err(RSessionError::EvalError)
    }

    /// Evaluate `expr` with the slot's current value bound to `.`.
    ///
    /// The expression runs in a fresh private child frame. Its successful result
    /// replaces the root; failed evaluation keeps the previous slot. Shared
    /// reference objects retain ordinary R mutation semantics.
    pub fn update(&mut self, expr: &str) -> Result<(), RSessionError> {
        self.session
            .update_retained(self.value, expr)
            .map_err(RSessionError::EvalError)
    }
}
