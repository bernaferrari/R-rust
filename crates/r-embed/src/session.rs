//! The embedded R session facade: evaluation, configuration, and rendering.

use r_device_android_headless::AndroidHeadlessRenderer;
use r_graphics_engine::Color;
use rmath::android::{RArenaStats, RResourceLimits, RRuntimeInfo, RValue};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

// Accounting allows for geometric growth of the operation vector. Renderer
// scratch buffers and allocator overhead are outside this retained-data budget.
const INTERACTIVE_SCENE_BUDGET: usize = 16 * 1024 * 1024;

use crate::RSessionError;
use crate::packages::{
    RPackageInfo, installed_packages_from_library_paths, package_info_from_path,
};

/// An embedded R session.
///
/// This provides a handle to an R interpreter instance that can
/// evaluate expressions and render plots. Internally uses the rmath
/// crate for the interpreter backend.
pub struct RSession {
    session_id: u64,
    active: bool,
    inner: rmath::android::RSession,
    /// Interactive graphics are recorded into a session-owned scene so a
    /// later command (for example `lines()`) can draw on the prior plot.
    interactive_scene: Option<r_graphics_engine::Scene>,
}

/// Owned result of an evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct EvalOutput {
    pub output: String,
    pub value: RValue,
}

/// The combined result of one interactive evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractiveOutput {
    pub output: String,
    pub png: Option<Vec<u8>>,
}

/// DrawTarget adapter that distinguishes a cleared device from actual drawing.
struct TrackingDrawTarget<'a> {
    target: &'a mut dyn r_graphics_engine::DrawTarget,
    drew: bool,
    retained_bytes: usize,
    budget_exceeded: bool,
}

impl TrackingDrawTarget<'_> {
    fn new(target: &mut r_graphics_engine::Scene) -> TrackingDrawTarget<'_> {
        TrackingDrawTarget {
            retained_bytes: scene_retained_bytes(target),
            target,
            drew: false,
            budget_exceeded: false,
        }
    }

    fn reserve(&mut self, bytes: usize) -> bool {
        let Some(total) = self.retained_bytes.checked_add(bytes) else {
            self.budget_exceeded = true;
            return false;
        };
        if total > INTERACTIVE_SCENE_BUDGET {
            self.budget_exceeded = true;
            false
        } else {
            self.retained_bytes = total;
            true
        }
    }
}

fn scene_retained_bytes(scene: &r_graphics_engine::Scene) -> usize {
    scene
        .operations()
        .iter()
        .map(operation_retained_bytes)
        .fold(0usize, |total, bytes| total.saturating_add(bytes))
}

fn operation_retained_bytes(operation: &r_graphics_engine::DrawOperation) -> usize {
    use r_graphics_engine::DrawOperation;
    let base = 2 * std::mem::size_of::<DrawOperation>();
    match operation {
        DrawOperation::Clear(_) => base,
        DrawOperation::Clip(_) => base,
        DrawOperation::Path(path) => path_operation_retained_bytes(path, path.commands.capacity()),
        DrawOperation::Text { text, .. } => base.saturating_add(text.len()),
        DrawOperation::DrawImage { image, .. } => base.saturating_add(image.pixels().len()),
    }
}

fn path_operation_retained_bytes(path: &r_graphics_engine::Path, command_count: usize) -> usize {
    (2 * std::mem::size_of::<r_graphics_engine::DrawOperation>())
        .saturating_add(std::mem::size_of::<r_graphics_engine::Path>())
        .saturating_add(
            command_count.saturating_mul(std::mem::size_of::<r_graphics_engine::PathCommand>()),
        )
        .saturating_add(path.stroke.dash_pattern.as_ref().map_or(0, |dash| {
            dash.intervals
                .capacity()
                .saturating_mul(std::mem::size_of::<f32>())
        }))
}

impl r_graphics_engine::DrawTarget for TrackingDrawTarget<'_> {
    fn dimensions(&self) -> (u32, u32) {
        self.target.dimensions()
    }
    fn clear(&mut self, background: Color) {
        self.target.clear(background);
        self.retained_bytes = 2 * std::mem::size_of::<r_graphics_engine::DrawOperation>();
    }
    fn set_clip(&mut self, rect: Option<[f32; 4]>) {
        if self.reserve(2 * std::mem::size_of::<r_graphics_engine::DrawOperation>()) {
            self.target.set_clip(rect);
        }
    }
    fn draw_path(&mut self, path: &r_graphics_engine::Path) {
        let operation_bytes = path_operation_retained_bytes(path, path.commands.len());
        if !self.reserve(operation_bytes) {
            return;
        }
        self.drew = true;
        self.target.draw_path(path);
    }
    fn draw_text(
        &mut self,
        text: &str,
        position: r_graphics_engine::Point,
        params: &r_graphics_engine::PlotParameters,
    ) {
        if !self.reserve(
            (2 * std::mem::size_of::<r_graphics_engine::DrawOperation>())
                .saturating_add(text.len()),
        ) {
            return;
        }
        self.drew = true;
        self.target.draw_text(text, position, params);
    }
    fn measure_text(
        &self,
        text: &str,
        params: &r_graphics_engine::PlotParameters,
    ) -> r_graphics_engine::TextMetrics {
        self.target.measure_text(text, params)
    }
    fn measure_math_text(
        &self,
        text: &str,
        params: &r_graphics_engine::PlotParameters,
    ) -> r_graphics_engine::TextMetrics {
        self.target.measure_math_text(text, params)
    }
    fn draw_image(
        &mut self,
        image: &r_graphics_engine::RasterImage,
        transform: [f64; 6],
        interpolate: bool,
    ) {
        if !self.reserve(
            (2 * std::mem::size_of::<r_graphics_engine::DrawOperation>())
                .saturating_add(image.pixels().len()),
        ) {
            return;
        }
        self.drew = true;
        self.target.draw_image(image, transform, interpolate);
    }
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
    inner: rmath::CancellationToken,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            inner: rmath::CancellationToken::new(),
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

    fn token(&self) -> rmath::CancellationToken {
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
    /// Bound captured console output for this session. `None` keeps native
    /// embedding behavior unchanged; Wasm hosts use a finite limit.
    pub fn set_output_limit(&mut self, max_bytes: Option<usize>) {
        self.inner.set_output_limit(max_bytes);
    }

    /// Limit result export before formatting or recursively copying values.
    /// The budget includes conservative per-element overhead; oversized or
    /// deeply nested values produce a recoverable error. None is unlimited.
    pub fn set_result_limit(&mut self, max_bytes: Option<usize>) {
        self.inner.set_result_limit(max_bytes);
    }

    /// Create a new R session.
    ///
    /// Initializes an isolated rmath session with its own arena, protection
    /// stack, environments, RNG state, and output capture.
    pub fn new() -> Result<Self, RSessionError> {
        Self::construct(rmath::android::RSession::new)
    }

    /// Initialize with explicit paths, without discovering host R libraries.
    pub fn new_with_path_policy(
        policy: rmath::android::RuntimePathPolicy,
    ) -> Result<Self, RSessionError> {
        Self::construct(|| rmath::android::RSession::new_with_path_policy(policy))
    }

    fn construct(
        constructor: impl FnOnce() -> rmath::android::RSession,
    ) -> Result<Self, RSessionError> {
        let session_id = NEXT_SESSION_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| RSessionError::InitFailed("session identities exhausted".into()))?;
        Ok(RSession {
            session_id,
            active: true,
            inner: constructor(),
            interactive_scene: None,
        })
    }

    /// Unique id assigned at construction from a process-wide counter.
    pub fn session_id(&self) -> u64 {
        self.session_id
    }
    pub fn enable_browser_files(&mut self) {
        self.inner.enable_browser_files();
    }
    pub fn import_file(&mut self, path: &str, bytes: &[u8]) -> Result<(), RSessionError> {
        self.inner
            .import_file(path, bytes)
            .map_err(RSessionError::EvalError)
    }
    pub fn export_file(&mut self, path: &str) -> Result<Vec<u8>, RSessionError> {
        self.inner
            .export_file(path)
            .ok_or_else(|| RSessionError::EvalError(format!("browser file not found: {path}")))
    }
    pub fn list_files(&mut self) -> Result<Vec<String>, RSessionError> {
        Ok(self.inner.list_files())
    }
    pub fn remove_file(&mut self, path: &str) -> Result<bool, RSessionError> {
        Ok(self.inner.remove_file(path))
    }

    /// Evaluate an R expression, returning the output as a string.
    ///
    /// Parses the code string using rmath's parser and evaluates it
    /// in the global environment. The result is formatted as a string
    /// using rmath's output subsystem.
    pub fn eval(&mut self, code: &str) -> Result<String, RSessionError> {
        if !self.active {
            return Err(RSessionError::EvalError("Session closed".into()));
        }
        let result = self.inner.eval_display(code);
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
        cancellation: Option<rmath::CancellationToken>,
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
    /// Hosts use this for tab completion. It is a direct snapshot of the
    /// environment frame (`ls(all.names=TRUE)` semantics) — no evaluation
    /// runs, so the session state is untouched.
    pub fn global_binding_names(&mut self) -> Result<Vec<String>, RSessionError> {
        if !self.active {
            return Err(RSessionError::EvalError("Session closed".into()));
        }
        Ok(self.inner.global_binding_names())
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
        cancellation: Option<rmath::CancellationToken>,
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

    /// Evaluate R graphics code in the session's global environment and
    /// capture device drawing into a PNG of the requested dimensions.
    /// Function lookup, method dispatch, promises and side effects use the
    /// ordinary evaluator. Device bookkeeping runs in a private local frame.
    pub fn render_with_dimensions(
        &mut self,
        code: &str,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, RSessionError> {
        if width < 32 || height < 32 {
            return Err(RSessionError::RenderError(
                "plot width and height must be at least 32 pixels".into(),
            ));
        }
        let mut renderer =
            AndroidHeadlessRenderer::try_new(width, height).map_err(RSessionError::RenderError)?;
        self.render_to(code, &mut renderer)?;
        renderer
            .try_finish()
            .map_err(|e| RSessionError::RenderError(e.to_string()))
    }

    /// Evaluate once, returning captured console output and a PNG only when
    /// the evaluation issued at least one drawing operation.
    pub fn eval_interactive(
        &mut self,
        code: &str,
        width: u32,
        height: u32,
    ) -> Result<InteractiveOutput, RSessionError> {
        if !self.active {
            return Err(RSessionError::EvalError("Session closed".into()));
        }
        if width < 32 || height < 32 {
            return Err(RSessionError::RenderError(
                "plot width and height must be at least 32 pixels".into(),
            ));
        }
        // Validate canvas limits before evaluating user code or retaining a scene.
        let mut renderer =
            AndroidHeadlessRenderer::try_new(width, height).map_err(RSessionError::RenderError)?;
        let scene = self
            .interactive_scene
            .get_or_insert_with(|| r_graphics_engine::Scene::new(width, height));
        let (result, budget_exceeded, drew) = {
            let mut target = TrackingDrawTarget::new(scene);
            let result = self
                .inner
                .eval_script_with_renderplot_backend(code, &mut target);
            (result, target.budget_exceeded, target.drew)
        };
        if budget_exceeded {
            return Err(RSessionError::RenderError(
                "interactive graphics scene exceeds the 16 MiB memory budget".into(),
            ));
        }
        if let RValue::Error(message) = &result.typed {
            let output = result.output.clone();
            let png = if drew {
                scene.replay_scaled(&mut renderer);
                Some(
                    renderer
                        .try_finish()
                        .map_err(|e| RSessionError::RenderError(e.to_string()))?,
                )
            } else {
                None
            };
            return Err(RSessionError::EvalErrorWithOutput {
                message: message.clone(),
                output,
                png,
            });
        }
        let png = if drew {
            scene.replay_scaled(&mut renderer);
            Some(
                renderer
                    .try_finish()
                    .map_err(|e| RSessionError::RenderError(e.to_string()))?,
            )
        } else {
            None
        };
        Ok(InteractiveOutput {
            output: result.output,
            png,
        })
    }

    /// Evaluate into an owned scene that can be sent to another thread or GPU.
    pub fn record_scene(
        &mut self,
        code: &str,
        width: u32,
        height: u32,
    ) -> Result<r_graphics_engine::Scene, RSessionError> {
        let mut scene = r_graphics_engine::Scene::new(width, height);
        scene
            .validate()
            .map_err(|e| RSessionError::RenderError(e.into()))?;
        if u64::from(width) * u64::from(height) > 16_777_216 {
            return Err(RSessionError::RenderError(
                "plot exceeds 16M-pixel limit".into(),
            ));
        }
        self.render_to(code, &mut scene)?;
        Ok(scene)
    }

    /// Draw synchronously into a caller-owned device. The device is detached on
    /// success, R errors and unwinding, before this method returns.
    pub fn render_to(
        &mut self,
        code: &str,
        target: &mut dyn r_graphics_engine::DrawTarget,
    ) -> Result<(), RSessionError> {
        if !self.active {
            return Err(RSessionError::RenderError("Session closed".into()));
        }
        let (width, height) = target.dimensions();
        if width < 32 || height < 32 {
            return Err(RSessionError::RenderError(
                "plot width and height must be at least 32 pixels".into(),
            ));
        }
        target.clear(Color::WHITE);
        if code.trim().is_empty() {
            return Ok(());
        }

        let _ = self.eval_with_render_target(code, target)?;
        Ok(())
    }

    fn eval_with_render_target(
        &mut self,
        code: &str,
        target: &mut dyn r_graphics_engine::DrawTarget,
    ) -> Result<EvalOutput, RSessionError> {
        // Evaluate R code through the interpreter while the portable device is installed.
        let wrapped = format!(
            r#"
local({{
  old <- tryCatch(grDevices::dev.cur(), error = function(e) 1L)
  result <- tryCatch({{
    newd <- tryCatch(grDevices::dev.new(noRStudioGD = TRUE), error = function(e) old)
    withVisible(eval(quote({{ {} }}), envir = globalenv()))
  }}, error = function(e) {{
    e
  }}, finally = {{
    try({{ if (grDevices::dev.cur() != old) grDevices::dev.off() }}, silent = TRUE)
    try({{ if (old > 1) grDevices::dev.set(old) }}, silent = TRUE)
  }})
  if (inherits(result, "error")) stop(conditionMessage(result))
  if (isTRUE(result$visible)) print(result$value)
  invisible(NULL)
}})
"#,
            code
        );
        let result = self
            .inner
            .eval_script_with_renderplot_backend(&wrapped, target);
        if let RValue::Error(message) = &result.typed {
            return Err(RSessionError::RenderError(message.clone()));
        }
        Ok(EvalOutput {
            output: result.output,
            value: result.typed,
        })
    }

    /// Evaluate `expr` globally and retain its successful result privately.
    /// The owning root survives evaluation and collection until removal or close.
    pub fn define_handle(&mut self, expr: &str) -> Result<ValueHandle, RSessionError> {
        let value = self
            .inner
            .define_retained(expr)
            .map_err(RSessionError::EvalError)?;
        Ok(ValueHandle {
            session_id: self.session_id,
            value,
        })
    }

    /// Borrow a directly copied typed snapshot. Reading never evaluates R code
    /// or invokes an R printing method.
    pub fn read_handle<'s>(
        &'s mut self,
        handle: &ValueHandle,
    ) -> Result<ReadGuard<'s>, RSessionError> {
        self.validate_handle(handle)?;
        let value = self
            .inner
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
        self.inner
            .remove_retained(handle.value)
            .map_err(RSessionError::EvalError)
    }

    fn validate_handle(&self, handle: &ValueHandle) -> Result<(), RSessionError> {
        if !self.active {
            return Err(RSessionError::EvalError("Session closed".into()));
        }
        if handle.session_id != self.session_id {
            return Err(RSessionError::EvalError(format!(
                "value handle belongs to session {}, used on session {}",
                handle.session_id, self.session_id
            )));
        }
        self.inner
            .validate_retained(handle.value)
            .map_err(RSessionError::EvalError)
    }

    /// Close the session.
    pub fn close(&mut self) {
        if self.active {
            self.inner.close();
            self.interactive_scene = None;
            self.active = false;
        }
    }
}

impl Drop for RSession {
    fn drop(&mut self) {
        self.close();
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
    value: rmath::android::RetainedValueId,
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
    value: rmath::android::RetainedValueId,
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
            .inner
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
            .inner
            .update_retained(self.value, expr)
            .map_err(RSessionError::EvalError)
    }
}

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
