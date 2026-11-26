//! Interactive graphics capture for the host session — PNG rasterization
//! through the headless renderer and scene recording, exactly the contract
//! the embedding facade pinned.

use super::{RSession, RValue};
use super::facade::{EvalOutput, InteractiveOutput, RSessionError};
use r_device_android_headless::AndroidHeadlessRenderer;
use r_graphics_engine::Color;

// Accounting allows for geometric growth of the operation vector. Renderer
// scratch buffers and allocator overhead are outside this retained-data budget.
const INTERACTIVE_SCENE_BUDGET: usize = 16 * 1024 * 1024;

/// DrawTarget adapter that distinguishes a cleared device from actual drawing.
struct TrackingDrawTarget<'a> {
    target: &'a mut dyn r_graphics_engine::DrawTarget,
    drew: bool,
    retained_bytes: usize,
    reservation: &'a mut super::RWorkspaceReservation,
    budget_exceeded: bool,
}

impl TrackingDrawTarget<'_> {
    fn new<'a>(
        target: &'a mut r_graphics_engine::Scene,
        reservation: &'a mut super::RWorkspaceReservation,
    ) -> TrackingDrawTarget<'a> {
        TrackingDrawTarget {
            retained_bytes: scene_retained_bytes(target),
            target,
            reservation,
            drew: false,
            budget_exceeded: false,
        }
    }

    fn reserve(&mut self, bytes: usize) -> bool {
        let Some(total) = self.retained_bytes.checked_add(bytes) else {
            self.budget_exceeded = true;
            return false;
        };
        if total > INTERACTIVE_SCENE_BUDGET || self.reservation.try_resize(total).is_err() {
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
        .max(if scene.operations().is_empty() {
            0
        } else {
            4 * std::mem::size_of::<r_graphics_engine::DrawOperation>()
        })
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
        let bytes = 4 * std::mem::size_of::<r_graphics_engine::DrawOperation>();
        if self.reservation.try_resize(bytes).is_err() {
            self.budget_exceeded = true;
            return;
        }
        self.target.clear(background);
        self.retained_bytes = bytes;
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

impl RSession {
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
        let bytes = AndroidHeadlessRenderer::workspace_bytes(width, height)
            .map_err(RSessionError::RenderError)?;
        let _workspace = self
            .reserve_workspace(bytes)
            .map_err(RSessionError::RenderError)?;
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
        if !self.is_active() {
            return Err(RSessionError::EvalError("Session closed".into()));
        }
        if width < 32 || height < 32 {
            return Err(RSessionError::RenderError(
                "plot width and height must be at least 32 pixels".into(),
            ));
        }
        // Validate canvas limits before evaluating user code or retaining a scene.
        let bytes = AndroidHeadlessRenderer::workspace_bytes(width, height)
            .map_err(RSessionError::RenderError)?;
        let _workspace = self
            .reserve_workspace(bytes)
            .map_err(RSessionError::RenderError)?;
        let mut renderer =
            AndroidHeadlessRenderer::try_new(width, height).map_err(RSessionError::RenderError)?;
        if self.interactive_scene_reservation.is_none() {
            self.interactive_scene_reservation = Some(
                self.reserve_workspace(0)
                    .map_err(RSessionError::RenderError)?,
            );
        }
        // The scene borrows move out of self for the evaluation window —
        /// the merged session evaluates through &mut self, so disjoint-field
        /// borrowing is unavailable exactly where the facade once split
        /// inner from scene. Both are restored on every path except the
        /// budget drop.
        let mut reservation = self
            .interactive_scene_reservation
            .take()
            .expect("reserved scene");
        let mut scene = self
            .interactive_scene
            .take()
            .unwrap_or_else(|| r_graphics_engine::Scene::new(width, height));
        let (result, budget_exceeded, drew) = {
            let mut target = TrackingDrawTarget::new(&mut scene, &mut reservation);
            let result = self.eval_script_with_renderplot_backend(code, &mut target);
            (result, target.budget_exceeded, target.drew)
        };
        if budget_exceeded {
            // A partial retained scene at the budget is unusable for future
            // drawing. Drop it before returning so a new plot can recover.
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
            self.interactive_scene = Some(scene);
            self.interactive_scene_reservation = Some(reservation);
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
        self.interactive_scene = Some(scene);
        self.interactive_scene_reservation = Some(reservation);
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
        if !self.is_active() {
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
    # The renderplot backend can create its own device on first graphics use.
    # Avoid opening the default PDF device when there is no current device;
    # custom device options and renders with an existing device keep GNU's
    # normal dev.new() path.
    device <- getOption("device")
    skip_default_device <- old == 1L && identical(device, "pdf") &&
      !exists("pdf", envir = .GlobalEnv, inherits = FALSE)
    newd <- if (skip_default_device) {{
      invisible(grDevices::dev.size(units = "px"))
      old
    }} else tryCatch(grDevices::dev.new(noRStudioGD = TRUE), error = function(e) old)
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
        let result = self.eval_script_with_renderplot_backend(&wrapped, target);
        if let RValue::Error(message) = &result.typed {
            return Err(RSessionError::RenderError(message.clone()));
        }
        Ok(EvalOutput {
            output: result.output,
            value: result.typed,
        })
    }
}
