//! `wasm-bindgen` session boundary over [`r_embed::RSession`] (M2).
//!
//! This crate is the browser/Node entry point to the R interpreter:
//! [`WasmRSession`] exposes `eval` (display output), `is_input_complete`
//! (continuation-prompt probe), `global_binding_names` (owned snapshot), and
//! `close`. No UniFFI, no async runtime, and no raw `SEXP` crosses the
//! boundary — every value is an owned Rust/JS string.
//!
//! Console wiring: output capture is session-owned inside `r-embed` (the same
//! channel the native oracle test uses), so `eval` returns exactly the text a
//! native embed sees; no JS console callbacks are required.
//!
//! Native `cargo check -p r-wasm` type-checks the same session code through a
//! cfg-gated non-bindgen shim (`JsError` stand-in), since `wasm-bindgen` is a
//! wasm32-only dependency here.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(all(target_arch = "wasm32", feature = "vello-gpu"))]
use web_sys::HtmlCanvasElement;

#[cfg(not(target_arch = "wasm32"))]
mod native_shim {
    //! Stand-in for `wasm_bindgen::JsError` so the crate type-checks on
    //! native targets (see the crate docs).

    #[derive(Debug, Clone)]
    pub struct JsError(pub String);

    impl std::fmt::Display for JsError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl std::error::Error for JsError {}

    impl JsError {
        pub fn new(message: &str) -> Self {
            JsError(message.into())
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
use native_shim::JsError;

/// A wasm-boundary R session.
///
/// Thin wrapper around [`r_embed::RSession`]; one instance owns one isolated
/// interpreter (arena, environments, RNG, output capture). Create it on the
/// JS side with `new WasmRSession()`.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct WasmRSession {
    // Every access is exclusive and catches unexpected panics below. Such a
    // panic closes the session before another JavaScript request can enter.
    inner: std::panic::AssertUnwindSafe<Option<r_embed::RSession>>,
}

const WASM_OUTPUT_LIMIT_BYTES: usize = 1024 * 1024;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl WasmRSession {
    /// Create a session.
    ///
    /// Throws a `JsError` when interpreter initialization fails.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new() -> Result<Self, JsError> {
        if cfg!(all(target_arch = "wasm32", panic = "abort")) {
            return Err(JsError::new(
                "R requires Wasm exception handling; build with scripts/build_wasm_runtime.sh",
            ));
        }
        let mut inner = r_embed::RSession::new().map_err(|e| JsError::new(&e.to_string()))?;
        inner.set_output_limit(Some(WASM_OUTPUT_LIMIT_BYTES));
        Ok(WasmRSession {
            inner: std::panic::AssertUnwindSafe(Some(inner)),
        })
    }

    /// Evaluate R code and return its display output.
    ///
    /// The string is the same text a native embed sees (printed output, then
    /// the auto-printed value of the final visible expression). A failed
    /// evaluation returns the rendered error text as the string.
    pub fn eval(&mut self, code: &str) -> String {
        self.with_session(|session| {
            Ok(match session.eval(code) {
                Ok(output) => output,
                Err(e) => render_error(e),
            })
        })
        .unwrap_or_else(|_| "Error: session closed after an unexpected failure".to_owned())
    }

    /// Evaluate, rejecting the JavaScript promise when R reports an error.
    pub fn eval_checked(&mut self, code: &str) -> Result<String, JsError> {
        self.with_session(|session| {
            session
                .eval(code)
                .map_err(|e| JsError::new(&render_error(e)))
        })
    }

    /// Return an owned scalar character value without parsing console output.
    pub fn eval_string(&mut self, code: &str) -> Result<String, JsError> {
        let result = self.with_session(|session| {
            session
                .eval_result(code)
                .map_err(|e| JsError::new(&render_error(e)))
        })?;
        match result.value {
            r_embed::RValue::StringVector(mut values) if values.len() == 1 => values
                .pop()
                .flatten()
                .ok_or_else(|| JsError::new("Expected a non-NA character scalar")),
            _ => Err(JsError::new("Expected a character scalar")),
        }
    }

    /// Render ordinary R evaluation into an owned PNG.
    pub fn render_png(&mut self, code: &str, width: u32, height: u32) -> Result<Vec<u8>, JsError> {
        self.with_session(|session| {
            session
                .render_with_dimensions(code, width, height)
                .map_err(|e| JsError::new(&e.to_string()))
        })
    }

    /// Capture an owned graphics scene synchronously before any GPU work.
    /// The returned scene contains no interpreter references. JavaScript may
    /// continue evaluating or close this session while the GPU renders it.
    #[cfg(feature = "vello-gpu")]
    pub fn record_scene(
        &mut self,
        code: &str,
        width: u32,
        height: u32,
    ) -> Result<WasmPlotScene, JsError> {
        self.with_session(|session| {
            session
                .record_scene(code, width, height)
                .map(|inner| WasmPlotScene { inner })
                .map_err(|error| JsError::new(&error.to_string()))
        })
    }

    /// Report whether `code` is syntactically complete R input.
    ///
    /// Incomplete input (`f <- function(x) {`) reports `false` so hosts show
    /// a continuation prompt. Complete-but-malformed input (a stray `)`)
    /// reports `true` and lets `eval` produce the upstream-shaped parse
    /// error. A closed or failed session reports `false`.
    pub fn is_input_complete(&mut self, code: &str) -> bool {
        self.with_session(|session| {
            session
                .is_input_complete(code)
                .map_err(|e| JsError::new(&e.to_string()))
        })
        .unwrap_or(false)
    }

    /// Snapshot the global environment's binding names.
    ///
    /// Owned strings, sorted, `ls(all.names = TRUE)` semantics minus the
    /// engine-internal handle environment.
    pub fn global_binding_names(&mut self) -> Vec<String> {
        self.with_session(|session| {
            session
                .global_binding_names()
                .map_err(|e| JsError::new(&e.to_string()))
        })
        .unwrap_or_default()
    }

    /// Close the session and release its interpreter resources.
    pub fn close(&mut self) {
        if let Some(mut session) = self.inner.0.take() {
            session.close();
        }
    }
}

impl WasmRSession {
    fn with_session<T>(
        &mut self,
        f: impl FnOnce(&mut r_embed::RSession) -> Result<T, JsError>,
    ) -> Result<T, JsError> {
        let session = self
            .inner
            .as_mut()
            .ok_or_else(|| JsError::new("Session closed"))?;
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(session))) {
            Ok(result) => result,
            Err(_) => {
                self.close();
                Err(JsError::new(
                    "Unexpected interpreter panic; session closed. Reset the worker before continuing.",
                ))
            }
        }
    }
}

/// An owned drawing captured from R, safe to retain after its session closes.
#[cfg(feature = "vello-gpu")]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct WasmPlotScene {
    inner: r_graphics_engine::Scene,
}

#[cfg(feature = "vello-gpu")]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl WasmPlotScene {
    pub fn width(&self) -> u32 {
        self.inner.dimensions().0
    }
    pub fn height(&self) -> u32 {
        self.inner.dimensions().1
    }
}

/// Reusable asynchronous GPU renderer. This is opt-in through `vello-gpu`.
///
/// JavaScript: `const gpu = await WasmGpuRenderer.create();` then
/// `await gpu.render_png(session.record_scene(code, 640, 480));`.
/// Initialization rejects when WebGPU or the required adapter is unavailable;
/// callers can explicitly choose the existing synchronous CPU `render_png`.
#[cfg(feature = "vello-gpu")]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct WasmGpuRenderer {
    // GPU work takes ownership before awaiting. A panic drops that renderer,
    // leaving None; callers cannot reuse partially unwound GPU state.
    inner: std::panic::AssertUnwindSafe<
        std::rc::Rc<std::cell::RefCell<Option<r_device_vello_gpu::GpuRenderer>>>,
    >,
    #[cfg(target_arch = "wasm32")]
    canvas: std::panic::AssertUnwindSafe<
        std::rc::Rc<std::cell::RefCell<Option<r_device_vello_gpu::CanvasSurface>>>,
    >,
}

#[cfg(all(feature = "vello-gpu", target_arch = "wasm32"))]
#[wasm_bindgen]
impl WasmGpuRenderer {
    pub fn create() -> js_sys::Promise {
        wasm_bindgen_futures::future_to_promise(std::panic::AssertUnwindSafe(async {
            let renderer = r_device_vello_gpu::GpuRenderer::new()
                .await
                .map_err(|error| JsError::new(&error.to_string()))?;
            Ok(Self {
                inner: std::panic::AssertUnwindSafe(std::rc::Rc::new(std::cell::RefCell::new(
                    Some(renderer),
                ))),
                canvas: std::panic::AssertUnwindSafe(std::rc::Rc::new(std::cell::RefCell::new(
                    None,
                ))),
            }
            .into())
        }))
    }

    /// Create a renderer using an adapter compatible with the supplied canvas.
    /// Prefer this constructor for browser presentation when the host canvas
    /// is available before GPU initialization.
    pub fn create_for_canvas(
        canvas: HtmlCanvasElement,
        width: u32,
        height: u32,
    ) -> js_sys::Promise {
        wasm_bindgen_futures::future_to_promise(std::panic::AssertUnwindSafe(async move {
            let (renderer, surface) =
                r_device_vello_gpu::GpuRenderer::new_for_canvas(canvas, width, height)
                    .await
                    .map_err(|error| JsError::new(&error.to_string()))?;
            Ok(Self {
                inner: std::panic::AssertUnwindSafe(std::rc::Rc::new(std::cell::RefCell::new(
                    Some(renderer),
                ))),
                canvas: std::panic::AssertUnwindSafe(std::rc::Rc::new(std::cell::RefCell::new(
                    Some(surface),
                ))),
            }
            .into())
        }))
    }
    /// Start rendering an owned snapshot without retaining any JavaScript or
    /// interpreter borrow. Concurrent requests reject explicitly. If a panic
    /// unwinds this future, the removed renderer is dropped and stays closed.
    pub fn render_png(&self, scene: &WasmPlotScene) -> js_sys::Promise {
        let inner = self.inner.clone();
        let scene = scene.inner.clone();
        let renderer = inner.borrow_mut().take();
        wasm_bindgen_futures::future_to_promise(std::panic::AssertUnwindSafe(async move {
            let mut renderer =
                renderer.ok_or_else(|| JsError::new("GPU renderer busy or closed"))?;
            let result = renderer.render_png(&scene).await;
            *inner.borrow_mut() = Some(renderer);
            let bytes = result.map_err(|error| JsError::new(&error.to_string()))?;
            Ok(wasm_bindgen::Clamped(bytes).into())
        }))
    }
    pub fn adapter_name(&self) -> Result<String, JsError> {
        self.inner
            .borrow()
            .as_ref()
            .map(|renderer| renderer.adapter_info().name.clone())
            .ok_or_else(|| JsError::new("GPU renderer busy or closed"))
    }

    /// Attach a browser canvas for direct WebGPU presentation.
    ///
    /// The canvas is configured with the supplied backing-pixel dimensions;
    /// call `resize_canvas` after a DPR or layout change. Frames are presented
    /// directly to the canvas and never read back through JavaScript.
    pub fn attach_canvas(
        &self,
        canvas: HtmlCanvasElement,
        width: u32,
        height: u32,
    ) -> Result<(), JsError> {
        let renderer = self.inner.borrow();
        let renderer = renderer
            .as_ref()
            .ok_or_else(|| JsError::new("GPU renderer busy or closed"))?;
        let surface = renderer
            .attach_canvas(canvas, width, height)
            .map_err(|error| JsError::new(&error.to_string()))?;
        *self.canvas.0.borrow_mut() = Some(surface);
        Ok(())
    }

    pub fn resize_canvas(&self, width: u32, height: u32) -> Result<(), JsError> {
        let renderer = self.inner.borrow();
        let renderer = renderer
            .as_ref()
            .ok_or_else(|| JsError::new("GPU renderer busy or closed"))?;
        let mut canvas = self.canvas.0.borrow_mut();
        let canvas = canvas
            .as_mut()
            .ok_or_else(|| JsError::new("No canvas is attached"))?;
        renderer
            .resize_canvas(canvas, width, height)
            .map_err(|error| JsError::new(&error.to_string()))
    }

    /// Render an owned scene directly to the attached canvas.
    pub fn render_canvas(&self, scene: &WasmPlotScene) -> js_sys::Promise {
        let inner = self.inner.clone();
        let canvas_state = self.canvas.0.clone();
        let scene = scene.inner.clone();
        if inner.borrow().is_none() {
            return wasm_bindgen_futures::future_to_promise(async {
                Err(JsError::new("GPU renderer busy or closed").into())
            });
        }
        if canvas_state.borrow().is_none() {
            return wasm_bindgen_futures::future_to_promise(async {
                Err(JsError::new("No canvas is attached").into())
            });
        }
        let renderer = inner.borrow_mut().take();
        let canvas = canvas_state.borrow_mut().take();
        wasm_bindgen_futures::future_to_promise(std::panic::AssertUnwindSafe(async move {
            let mut renderer =
                renderer.ok_or_else(|| JsError::new("GPU renderer busy or closed"))?;
            let canvas = canvas.ok_or_else(|| JsError::new("No canvas is attached"))?;
            let result = renderer.render_canvas(&canvas, &scene).await;
            *inner.borrow_mut() = Some(renderer);
            *canvas_state.borrow_mut() = Some(canvas);
            result.map_err(|error| JsError::new(&error.to_string()))?;
            Ok(JsValue::UNDEFINED)
        }))
    }
}

#[cfg(all(feature = "vello-gpu", not(target_arch = "wasm32")))]
impl WasmGpuRenderer {
    pub async fn create() -> Result<Self, JsError> {
        let renderer = r_device_vello_gpu::GpuRenderer::new()
            .await
            .map_err(|error| JsError::new(&error.to_string()))?;
        Ok(Self {
            inner: std::panic::AssertUnwindSafe(std::rc::Rc::new(std::cell::RefCell::new(Some(
                renderer,
            )))),
        })
    }
    pub async fn render_png(&self, scene: &WasmPlotScene) -> Result<Vec<u8>, JsError> {
        let mut renderer = self
            .inner
            .borrow_mut()
            .take()
            .ok_or_else(|| JsError::new("GPU renderer busy or closed"))?;
        let result = renderer.render_png(&scene.inner).await;
        *self.inner.borrow_mut() = Some(renderer);
        result.map_err(|error| JsError::new(&error.to_string()))
    }
    pub fn adapter_name(&self) -> Result<String, JsError> {
        self.inner
            .borrow()
            .as_ref()
            .map(|renderer| renderer.adapter_info().name.clone())
            .ok_or_else(|| JsError::new("GPU renderer busy or closed"))
    }
}

/// Render an evaluation error the way the console would.
fn render_error(e: r_embed::RSessionError) -> String {
    let text = e.to_string();
    match text.strip_prefix("Evaluation error: ") {
        Some(inner) if inner.starts_with("Error") => inner.to_string(),
        _ => format!("Error: {text}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unexpected_panic_closes_session_before_reuse() {
        let mut session = WasmRSession::new().unwrap();
        session.eval("x <- 41");
        let error = session
            .with_session::<()>(|_| panic!("injected host failure"))
            .unwrap_err();
        assert!(error.to_string().contains("session closed"));
        assert!(session.inner.0.is_none());
        assert!(
            session
                .eval_checked("x + 1")
                .unwrap_err()
                .to_string()
                .contains("Session closed")
        );
    }

    /// The native oracle the wasm boundary must satisfy (docs/web-architecture.md).
    #[test]
    fn wasm_m3_oracle_shape() {
        let mut session = WasmRSession::new().expect("session initializes");
        let out = session.eval("1+1");
        assert_eq!(out, "[1] 2");
        assert!(session.is_input_complete("1 + 1"));
        assert!(!session.is_input_complete("f <- function(x) {"));
        session.close();
    }

    #[test]
    fn global_binding_names_snapshot_is_owned_and_sorted() {
        let mut session = WasmRSession::new().expect("session initializes");
        session.eval("zzz <- 1; aaa <- 2");
        let names = session.global_binding_names();
        let pos_zzz = names.iter().position(|n| n == "zzz");
        let pos_aaa = names.iter().position(|n| n == "aaa");
        assert!(pos_zzz.is_some() && pos_aaa.is_some());
        assert!(pos_aaa.unwrap() < pos_zzz.unwrap(), "names are sorted");
        assert!(!names.iter().any(|n| n == "..rport_handles.."));
        session.close();
    }

    #[test]
    fn bounded_console_output_keeps_session_usable() {
        let mut session = WasmRSession::new().expect("session initializes");
        let output = session
            .eval_checked("cat(paste(rep('x', 2 * 1024 * 1024), collapse = ''))")
            .expect("large output remains a successful evaluation");
        assert!(output.contains("[captured console output truncated by runtime limit]"));
        assert!(output.len() <= WASM_OUTPUT_LIMIT_BYTES + 64);
        assert_eq!(session.eval_checked("1 + 1").unwrap(), "[1] 2");
    }
}
