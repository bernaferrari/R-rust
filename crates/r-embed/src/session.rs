//! The embedded R session facade: evaluation, configuration, and rendering.

use r_device_android_headless::AndroidHeadlessRenderer;
use r_graphics_engine::{Color, RenderPlot};
use rmath::android::{RArenaStats, RResourceLimits, RRuntimeInfo, RValue};
use std::path::PathBuf;

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
    active: bool,
    inner: rmath::android::RSession,
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

impl RSession {
    /// Create a new R session.
    ///
    /// Initializes an isolated rmath session with its own arena, protection
    /// stack, environments, RNG state, and output capture.
    pub fn new() -> Result<Self, RSessionError> {
        Ok(RSession {
            active: true,
            inner: rmath::android::RSession::new(),
        })
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

    /// Enable `system()` and `pipe()` for desktop-style hosts.
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
