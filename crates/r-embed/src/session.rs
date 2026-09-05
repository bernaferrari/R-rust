//! The embedded R session facade: evaluation, configuration, and rendering.

use r_device_android_headless::AndroidHeadlessRenderer;
use r_graphics_engine::{Color, RenderPlot};
use rmath::android::{RArenaStats, RResourceLimits, RRuntimeInfo, RValue};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::RSessionError;
use crate::packages::{
    RPackageInfo, installed_packages_from_library_paths, package_info_from_path,
};
use crate::plot::{PlotSeries, draw_series, numeric_series, parse_plot_call};

/// An embedded R session.
///
/// This provides a handle to an R interpreter instance that can
/// evaluate expressions and render plots. Internally uses the rmath
/// crate for the interpreter backend.
pub struct RSession {
    session_id: u64,
    active: bool,
    inner: rmath::android::RSession,
    /// Live handle slots: index = slot id, value = current generation.
    /// Removed slots keep their entry with a bumped generation so stale
    /// handles are rejected; ids are never reused.
    handle_slot_states: Vec<u32>,
}

/// Owned result of an evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct EvalOutput {
    pub output: String,
    pub value: RValue,
}

/// Derived Android runtime paths for app-private embedding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidRuntimePaths {
    pub app_files_dir: String,
    pub cache_dir: String,
    pub bundled_library_dir: Option<String>,
}

impl AndroidRuntimePaths {
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
        PathBuf::from(&self.app_files_dir)
            .join("R")
            .join("library")
            .to_string_lossy()
            .into_owned()
    }

    pub fn temp_dir(&self) -> String {
        PathBuf::from(&self.cache_dir)
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

/// Cooperative cancellation handle for an embedded evaluation.
///
/// The token is cheap to clone and can be cancelled from another thread. It is
/// scoped to explicit evaluations that receive it; cancelling one token does
/// not affect other sessions.
#[derive(Debug, Clone)]
pub struct CancellationToken {
    inner: rmath::sexp::CancellationToken,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            inner: rmath::sexp::CancellationToken::new(),
        }
    }

    pub fn cancel(&self) {
        self.inner.request();
    }

    pub fn reset(&self) {
        self.inner.reset();
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.is_requested()
    }

    fn token(&self) -> rmath::sexp::CancellationToken {
        self.inner.clone()
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

/// Process-wide counter assigning each `RSession` a unique id.
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

impl RSession {
    /// Create a new R session.
    ///
    /// Initializes an isolated rmath session with its own arena, protection
    /// stack, environments, RNG state, and output capture.
    pub fn new() -> Result<Self, RSessionError> {
        Ok(RSession {
            session_id: NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed),
            active: true,
            inner: rmath::android::RSession::new(),
            handle_slot_states: Vec::new(),
        })
    }

    /// Unique id assigned at construction from a process-wide counter.
    pub fn session_id(&self) -> u64 {
        self.session_id
    }

    /// Evaluate an R expression, returning the output as a string.
    ///
    /// Parses the code string using rmath's parser and evaluates it
    /// in the global environment. The result is formatted as a string
    /// using rmath's output subsystem.
    pub fn eval(&mut self, code: &str) -> Result<String, RSessionError> {
        self.eval_script(code).map(|result| result.output)
    }

    /// Evaluate a multi-expression R script.
    pub fn eval_script(&mut self, code: &str) -> Result<EvalOutput, RSessionError> {
        self.eval_script_with_cancel(code, None)
    }

    fn eval_script_with_cancel(
        &mut self,
        code: &str,
        cancellation: Option<rmath::sexp::CancellationToken>,
    ) -> Result<EvalOutput, RSessionError> {
        if !self.active {
            return Err(RSessionError::EvalError("Session closed".into()));
        }

        let result = self
            .inner
            .eval_script_with_cancellation_token(code, cancellation);

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
        cancellation: &CancellationToken,
    ) -> Result<EvalOutput, RSessionError> {
        self.eval_result_with_cancel(code, Some(cancellation.token()))
    }

    /// Return the names of bindings in the global environment.
    ///
    /// Hosts use this for tab completion. It goes through the same
    /// evaluator path as `ls(all.names=TRUE)` and never mutates the
    /// session.
    pub fn global_binding_names(&mut self) -> Result<Vec<String>, RSessionError> {
        if !self.active {
            return Err(RSessionError::EvalError("Session closed".into()));
        }
        let value = self.eval_result("ls(all.names=TRUE)")?.value;
        // The reserved handle environment is engine-internal: hosts listing
        // bindings for tab completion never see it.
        let names = match value {
            RValue::StringVector(names) => names,
            RValue::Attributed { value, .. } => match *value {
                RValue::StringVector(names) => names,
                other => {
                    return Err(RSessionError::EvalError(format!(
                        "ls(all.names=TRUE) returned unexpected value: {other:?}"
                    )))
                }
            },
            other => {
                return Err(RSessionError::EvalError(format!(
                    "ls(all.names=TRUE) returned unexpected value: {other:?}"
                )))
            }
        };
        Ok(names
            .into_iter()
            .flatten()
            .filter(|name| name != HANDLE_ENV_NAME)
            .collect())
    }

    /// Return whether `code` is a syntactically complete R input.
    ///
    /// Interactive hosts call this to decide between evaluating immediately
    /// and showing a continuation prompt: input is incomplete when the parser
    /// reports an unexpected end of input (unmatched braces or parentheses, a
    /// trailing binary operator, ...). Complete-but-malformed input (e.g. a
    /// stray `)`) reports `true` so evaluation produces the upstream-shaped
    /// parse error.
    pub fn is_input_complete(&mut self, code: &str) -> Result<bool, RSessionError> {
        if !self.active {
            return Err(RSessionError::EvalError("Session closed".into()));
        }
        self.inner
            .is_syntax_complete(code)
            .map_err(RSessionError::EvalError)
    }

    /// Configure Android app-private R runtime paths.
    pub fn configure_android_paths(
        &mut self,
        app_files_dir: &str,
        cache_dir: &str,
        bundled_library_dir: Option<&str>,
    ) -> Result<(), RSessionError> {
        self.inner
            .configure_paths(app_files_dir, cache_dir, bundled_library_dir)
            .map_err(RSessionError::InitFailed)
    }

    /// Configure Android paths from a single helper value with derived runtime
    /// locations for package libraries and temp files.
    pub fn configure_android_runtime(
        &mut self,
        paths: &AndroidRuntimePaths,
    ) -> Result<(), RSessionError> {
        self.configure_android_paths(
            &paths.app_files_dir,
            &paths.cache_dir,
            paths.bundled_library_dir.as_deref(),
        )
    }

    /// Return host-visible runtime path/session state.
    pub fn runtime_info(&self) -> RRuntimeInfo {
        self.inner.runtime_info()
    }

    /// Return this session's Android-facing resource limits.
    pub fn resource_limits(&self) -> RResourceLimits {
        self.inner.resource_limits()
    }

    /// Return a snapshot of this session's arena allocator.
    pub fn arena_stats(&mut self) -> RArenaStats {
        self.inner.arena_stats()
    }

    /// Set this session's Android-facing resource limits.
    pub fn set_resource_limits(&mut self, limits: RResourceLimits) -> Result<(), RSessionError> {
        if !self.active {
            return Err(RSessionError::EvalError("Session closed".into()));
        }
        self.inner.set_resource_limits(limits);
        Ok(())
    }

    /// Enable trusted host-process features (`system`, pipes, and native
    /// extensions) for desktop-style embedders.
    ///
    /// Embedded mobile and WASM sessions keep these disabled by default.
    pub fn enable_host_process_capabilities(&mut self) {
        self.inner.enable_host_process_capabilities();
    }

    /// Return true when a package exists in this session's configured library paths.
    pub fn package_available(&self, package: &str) -> bool {
        self.inner.package_available(package)
    }

    /// Return the resolved package directory for a package, if available.
    pub fn package_path(&self, package: &str) -> Option<String> {
        self.inner.package_path(package)
    }

    /// Return metadata for a package if it is visible in this session's
    /// configured library paths.
    pub fn package_info(&self, package: &str) -> Option<RPackageInfo> {
        let package_path = self.package_path(package)?;
        let library_paths = self.runtime_info().library_paths;
        package_info_from_path(package, &PathBuf::from(&package_path), &library_paths)
    }

    /// Return metadata for installed packages visible in this session.
    pub fn installed_packages(&self) -> Vec<RPackageInfo> {
        installed_packages_from_library_paths(&self.runtime_info().library_paths)
    }

    /// Load a pure-R package into this session.
    pub fn load_package(&mut self, package: &str) -> Result<(), RSessionError> {
        self.inner
            .load_package(package)
            .map_err(RSessionError::EvalError)
    }

    fn eval_result_with_cancel(
        &mut self,
        code: &str,
        cancellation: Option<rmath::sexp::CancellationToken>,
    ) -> Result<EvalOutput, RSessionError> {
        if !self.active {
            return Err(RSessionError::EvalError("Session closed".into()));
        }

        let result = self.inner.eval_with_cancellation_token(code, cancellation);

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

    /// Render an R expression (or full graphics-producing code) as a plot, returning pixel data.
    ///
    /// This now drives *real* R graphics for perfect fidelity: the provided code is evaluated
    /// (e.g. "plot(1:10, main='hi')", "grid::grid.text(...)", ggplot2/lattice if loaded, etc.).
    /// A fresh device is ensured, the graphics are drawn through the portable headless
    /// DeviceRegistry (with text/labels support), the result is captured via dev.capture/GECap,
    /// and encoded to PNG at the requested dimensions (nearest-neighbor scale from the
    /// device's native raster).
    ///
    /// For the highest-quality simple numeric plots you can still rely on direct skia in
    /// some paths, but this unified path gives full R semantics, all high-level graphics/grid,
    /// and works for arbitrary code on Android (and the internal device on other hosts).
    pub fn render_with_dimensions(
        &mut self,
        code: &str,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, RSessionError> {
        if !self.active {
            return Err(RSessionError::RenderError("Session closed".into()));
        }
        if width < 32 || height < 32 {
            return Err(RSessionError::RenderError(
                "plot width and height must be at least 32 pixels".to_string(),
            ));
        }

        let mut renderer = AndroidHeadlessRenderer::new(width, height);
        renderer.clear(Color::WHITE);

        if code.trim().is_empty() {
            return Ok(renderer.finish());
        }

        if let Some(series) = self.simple_plot_series(code)? {
            draw_series(&mut renderer, width, height, &series);
            return Ok(renderer.finish());
        }

        // Run the graphics-producing code through real R for full fidelity.
        let wrapped = format!(
            r#"
{{
  old <- tryCatch(grDevices::dev.cur(), error = function(e) 1L)
  result <- tryCatch({{
    newd <- tryCatch(grDevices::dev.new(noRStudioGD = TRUE), error = function(e) old)
    {}
    NULL
  }}, error = function(e) {{
    e
  }}, finally = {{
    try({{ if (grDevices::dev.cur() != old) grDevices::dev.off() }}, silent = TRUE)
    try({{ if (old > 1) grDevices::dev.set(old) }}, silent = TRUE)
  }})
  if (inherits(result, "error")) stop(conditionMessage(result))
  NULL
}}
"#,
            code
        );
        let result = self
            .inner
            .eval_script_with_renderplot_backend(&wrapped, &mut renderer);
        if let RValue::Error(message) = result.typed {
            return Err(RSessionError::RenderError(message));
        }

        Ok(renderer.finish())
    }

    fn simple_plot_series(&mut self, code: &str) -> Result<Option<PlotSeries>, RSessionError> {
        let trimmed = code.trim();
        if !trimmed.starts_with("plot(") || !trimmed.ends_with(')') {
            return Ok(None);
        }

        let call = parse_plot_call(trimmed);
        if call.positional.is_empty() {
            return Err(RSessionError::RenderError(
                "plot requires numeric data".to_string(),
            ));
        }

        let y_expr = if call.positional.len() >= 2 {
            call.positional[1]
        } else {
            call.positional[0]
        };
        let y = numeric_series(self.eval_result(y_expr)?.value)?;
        let x = if call.positional.len() >= 2 {
            numeric_series(self.eval_result(call.positional[0])?.value)?
        } else {
            (1..=y.len()).map(|value| value as f64).collect()
        };
        let options = call.options.with_default_labels(call.positional[0], y_expr);

        Ok(Some(PlotSeries { x, y, options }))
    }
    /// Evaluate `expr` and keep the resulting value rooted in the session's
    /// reserved handle environment, returning an opaque [`ValueHandle`].
    ///
    /// The value survives later evaluations and `gc()` because it stays
    /// bound in the reserved environment until [`RSession::remove_handle`]
    /// or session close. `expr` is wrapped in `{ }`, so multi-statement
    /// expressions work.
    pub fn define_handle(&mut self, expr: &str) -> Result<ValueHandle, RSessionError> {
        self.ensure_handle_env()?;
        self.handle_slot_states.push(0);
        let slot = self.handle_slot_states.len() as u32 - 1;
        self.assign_handle_slot(slot, expr)?;
        Ok(ValueHandle {
            session_id: self.session_id,
            slot,
            generation: 0,
        })
    }

    /// Borrow the value behind `handle` for reading.
    ///
    /// The [`ReadGuard`] holds an owned snapshot and exclusively borrows the
    /// session: no evaluation can run while it is alive, so the snapshot
    /// cannot be invalidated underneath the reader.
    pub fn read_handle<'s>(
        &'s mut self,
        handle: &ValueHandle,
    ) -> Result<ReadGuard<'s>, RSessionError> {
        let slot = self.validate_handle(handle)?;
        let expr = format!("{}$h{slot}", HANDLE_ENV_NAME);
        let snapshot = self.eval_result(&expr)?;
        if matches!(snapshot.value, RValue::Null | RValue::Error(_)) && !self.slot_exists(slot)? {
            return Err(RSessionError::EvalError(
                "stale value handle: slot binding vanished".into(),
            ));
        }
        Ok(ReadGuard {
            session: self,
            snapshot,
        })
    }

    /// Borrow the slot behind `handle` for writing.
    ///
    /// The [`WriteGuard`] exclusively borrows the session: at most one write
    /// guard (and no evaluation) exists at any time.
    pub fn write_handle<'s>(
        &'s mut self,
        handle: &ValueHandle,
    ) -> Result<WriteGuard<'s>, RSessionError> {
        let slot = self.validate_handle(handle)?;
        if !self.slot_exists(slot)? {
            return Err(RSessionError::EvalError(
                "stale value handle: slot binding vanished".into(),
            ));
        }
        Ok(WriteGuard {
            session: self,
            slot,
        })
    }

    /// Drop the slot binding and invalidate every handle referring to it.
    ///
    /// Later reads or writes through those handles fail as stale.
    pub fn remove_handle(&mut self, handle: &ValueHandle) -> Result<(), RSessionError> {
        let slot = self.validate_handle(handle)?;
        let expr = format!("rm(h{slot}, envir = {HANDLE_ENV_NAME})");
        self.eval(&expr)?;
        if let Some(generation) = self.handle_slot_states.get_mut(slot as usize) {
            *generation += 1;
        }
        Ok(())
    }

    fn validate_handle(&self, handle: &ValueHandle) -> Result<u32, RSessionError> {
        if handle.session_id != self.session_id {
            return Err(RSessionError::EvalError(format!(
                "value handle belongs to session {}, used on session {}",
                handle.session_id, self.session_id
            )));
        }
        match self.handle_slot_states.get(handle.slot as usize) {
            None => Err(RSessionError::EvalError(
                "stale value handle: slot never existed".into(),
            )),
            Some(generation) if *generation != handle.generation => Err(
                RSessionError::EvalError("stale value handle: slot was removed".into()),
            ),
            Some(_) => Ok(handle.slot),
        }
    }

    fn ensure_handle_env(&mut self) -> Result<(), RSessionError> {
        let expr = format!(
            "if (!exists(\"{HANDLE_ENV_NAME}\", envir = globalenv(), inherits = FALSE)) \
             assign(\"{HANDLE_ENV_NAME}\", new.env(parent = emptyenv()), envir = globalenv())"
        );
        self.eval(&expr).map(|_| ())
    }

    fn slot_exists(&mut self, slot: u32) -> Result<bool, RSessionError> {
        let expr = format!("exists(\"h{slot}\", envir = {HANDLE_ENV_NAME})");
        Ok(self.eval(&expr)?.trim() == "[1] TRUE")
    }

    fn assign_handle_slot(&mut self, slot: u32, expr: &str) -> Result<(), RSessionError> {
        let wrapped = format!("{{ {}$h{slot} <- {{ {} }} }}", HANDLE_ENV_NAME, expr.trim());
        self.eval(&wrapped).map(|_| ())
    }

    fn update_handle_slot(&mut self, slot: u32, expr: &str) -> Result<(), RSessionError> {
        let wrapped = format!(
            "local({{ . <- {HANDLE_ENV_NAME}$h{slot}; {HANDLE_ENV_NAME}$h{slot} <- {{ {} }} }})",
            expr.trim()
        );
        self.eval(&wrapped).map(|_| ())
    }



    /// Close the session.
    pub fn close(&mut self) {
        if self.active {
            self.inner.close();
            self.active = false;
        }
    }
}

impl Drop for RSession {
    fn drop(&mut self) {
        self.close();
    }
}

/// Opaque, session-scoped handle to a live R value kept rooted inside the
/// session's handle environment.
///
/// A handle is a plain `Copy` id (`session_id`, slot, generation): it holds
/// no reference into the R arena, so it can be stored anywhere and outlive
/// evaluations. Safety comes from validation at use time:
///
/// - a handle from another session is rejected (`foreign-session handle`),
/// - a handle whose slot was [`removed`](RSession::remove_handle) or never
///   existed is rejected as *stale* (slot ids are never reused; the
///   generation counter also catches internal reuse),
/// - the underlying R value stays garbage-collector-rooted because it lives
///   in the reserved `..rport_handles..` environment until removed or the
///   session closes.
///
/// There is deliberately no path from a `ValueHandle` back to raw `SEXP`
/// data: reads and writes go through [`RSession::read_handle`] and
/// [`RSession::write_handle`], which borrow the session for the guard's
/// lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ValueHandle {
    session_id: u64,
    slot: u32,
    generation: u32,
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
        f.debug_struct("ReadGuard").field("snapshot", &self.snapshot).finish()
    }
}

impl ReadGuard<'_> {
    /// The captured console output produced by the read evaluation.
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
/// Dropping the guard releases the session borrow; the slot binding itself
/// persists until [`RSession::remove_handle`] or session close.
pub struct WriteGuard<'s> {
    session: &'s mut RSession,
    slot: u32,
}

impl std::fmt::Debug for WriteGuard<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriteGuard").field("slot", &self.slot).finish()
    }
}

impl WriteGuard<'_> {
    /// Replace the slot's value with the result of evaluating `expr`.
    ///
    /// On error the slot keeps its previous binding.
    pub fn set(&mut self, expr: &str) -> Result<(), RSessionError> {
        self.session
            .assign_handle_slot(self.slot, expr)
    }

    /// Evaluate `expr` with the slot's current value bound to `.`.
    ///
    /// This is the in-place mutation form: the expression sees the live
    /// value through `.` and the slot is rebound to the expression's result.
    pub fn update(&mut self, expr: &str) -> Result<(), RSessionError> {
        self.session.update_handle_slot(self.slot, expr)
    }
}

/// Name of the reserved global binding holding the handle-slot environment.
const HANDLE_ENV_NAME: &str = "..rport_handles..";


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_session_binding_names_returns_err() {
        let mut session = RSession::new().expect("session starts");
        session.close();
        let err = session
            .global_binding_names()
            .expect_err("closed session must fail");
        assert!(
            matches!(&err, RSessionError::EvalError(msg) if msg == "Session closed"),
            "unexpected error: {err:?}"
        );
    }
}
