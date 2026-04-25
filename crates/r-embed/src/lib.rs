//! R interpreter embedding library.
//!
//! Provides the `RSession` type for embedding the R interpreter into Rust
//! applications. This crate is the safe boundary used by desktop hosts and
//! UniFFI bindings: it exposes owned Rust values and delegates runtime work to
//! rmath's per-session interpreter, never to process-global `SEXP` state.

use r_device_android_headless::AndroidHeadlessRenderer;
use r_graphics_engine::{Color, Path, PathCommand, PlotParameters, Point, RenderPlot, Stroke};
use std::sync::{Arc, atomic::AtomicBool};

pub use rmath::android::{RComplexValue, RValue};

use thiserror::Error;

/// Errors that can occur during R session operations.
#[derive(Debug, Error)]
pub enum RSessionError {
    #[error("Failed to initialize R session: {0}")]
    InitFailed(String),
    #[error("Evaluation error: {0}")]
    EvalError(String),
    #[error("Render error: {0}")]
    RenderError(String),
}

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

/// Cooperative cancellation handle for an embedded evaluation.
///
/// The token is cheap to clone and can be cancelled from another thread. It is
/// scoped to explicit evaluations that receive it; cancelling one token does
/// not affect other sessions.
#[derive(Debug, Clone)]
pub struct CancellationToken {
    inner: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.inner.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn reset(&self) {
        self.inner.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn flag(&self) -> Arc<AtomicBool> {
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
        self.eval_result(code).map(|result| result.output)
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
        self.eval_result_with_cancel(code, Some(cancellation.flag()))
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

    fn eval_result_with_cancel(
        &mut self,
        code: &str,
        cancellation: Option<Arc<AtomicBool>>,
    ) -> Result<EvalOutput, RSessionError> {
        if !self.active {
            return Err(RSessionError::EvalError("Session closed".into()));
        }

        self.inner.set_cancellation_flag(cancellation);
        let result = self.inner.eval(code);
        self.inner.set_cancellation_flag(None);

        if let Some(message) = result.output.strip_prefix("Error: ") {
            if message == "operation cancelled" {
                Err(RSessionError::EvalError("operation cancelled".to_string()))
            } else {
                Err(RSessionError::EvalError(message.to_string()))
            }
        } else {
            Ok(EvalOutput {
                output: result.output,
                value: result.typed,
            })
        }
    }

    /// Render an R expression as a plot, returning pixel data.
    pub fn render_with_dimensions(
        &mut self,
        code: &str,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, RSessionError> {
        if !self.active {
            return Err(RSessionError::RenderError("Session closed".into()));
        }
        let mut renderer = AndroidHeadlessRenderer::new(width, height);
        renderer.clear(Color::WHITE);
        if !code.trim().is_empty() {
            let series = self.plot_series(code)?;
            draw_series(&mut renderer, width, height, &series);
        }
        Ok(renderer.finish())
    }

    fn plot_series(&mut self, code: &str) -> Result<PlotSeries, RSessionError> {
        let call = parse_plot_call(code);
        let values = match call {
            PlotCall::One(expr) => {
                let y = numeric_series(self.eval_result(expr)?.value)?;
                let x = (1..=y.len()).map(|value| value as f64).collect();
                PlotSeries { x, y }
            }
            PlotCall::Two(x_expr, y_expr) => PlotSeries {
                x: numeric_series(self.eval_result(x_expr)?.value)?,
                y: numeric_series(self.eval_result(y_expr)?.value)?,
            },
        };

        if values.x.is_empty() || values.y.is_empty() {
            return Err(RSessionError::RenderError(
                "plot data must not be empty".to_string(),
            ));
        }
        Ok(values)
    }

    /// Close the session.
    pub fn close(&mut self) {
        if self.active {
            self.inner.close();
            self.active = false;
        }
    }
}

struct PlotSeries {
    x: Vec<f64>,
    y: Vec<f64>,
}

enum PlotCall<'a> {
    One(&'a str),
    Two(&'a str, &'a str),
}

fn parse_plot_call(code: &str) -> PlotCall<'_> {
    let trimmed = code.trim();
    let Some(inner) = trimmed
        .strip_prefix("plot(")
        .and_then(|value| value.strip_suffix(')'))
    else {
        return PlotCall::One(trimmed);
    };
    match split_top_level_comma(inner) {
        Some((x, y)) => PlotCall::Two(x.trim(), y.trim()),
        None => PlotCall::One(inner.trim()),
    }
}

fn split_top_level_comma(input: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    let mut in_string = None;
    let mut escaped = false;
    for (idx, ch) in input.char_indices() {
        if let Some(quote) = in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                in_string = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' => in_string = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => return Some((&input[..idx], &input[idx + ch.len_utf8()..])),
            _ => {}
        }
    }
    None
}

fn numeric_series(value: RValue) -> Result<Vec<f64>, RSessionError> {
    let values = match value {
        RValue::Integer(Some(value)) => vec![value as f64],
        RValue::Integer(None) => Vec::new(),
        RValue::Real(Some(value)) => vec![value],
        RValue::Real(None) => Vec::new(),
        RValue::IntegerVector(values) => values
            .into_iter()
            .filter_map(|value| value.map(|value| value as f64))
            .collect(),
        RValue::RealVector(values) => values.into_iter().flatten().collect(),
        other => {
            return Err(RSessionError::RenderError(format!(
                "plot data must be numeric, got {other:?}"
            )));
        }
    };

    if values.iter().all(|value| value.is_finite()) {
        Ok(values)
    } else {
        Err(RSessionError::RenderError(
            "plot data must contain only finite values".to_string(),
        ))
    }
}

fn draw_series(
    renderer: &mut AndroidHeadlessRenderer,
    width: u32,
    height: u32,
    series: &PlotSeries,
) {
    let n = series.x.len().min(series.y.len());
    if n == 0 {
        return;
    }

    let left = 54.0f32;
    let right = (width as f32 - 18.0).max(left + 1.0);
    let top = 24.0f32;
    let bottom = (height as f32 - 42.0).max(top + 1.0);
    let xmin = min_max(&series.x[..n]).0;
    let xmax = min_max(&series.x[..n]).1;
    let ymin = min_max(&series.y[..n]).0;
    let ymax = min_max(&series.y[..n]).1;

    draw_line(renderer, left, bottom, right, bottom, Color::BLACK, 1.5);
    draw_line(renderer, left, top, left, bottom, Color::BLACK, 1.5);

    for i in 0..5 {
        let t = i as f32 / 4.0;
        let x = left + (right - left) * t;
        let y = top + (bottom - top) * t;
        draw_line(
            renderer,
            x,
            top,
            x,
            bottom,
            Color {
                r: 224,
                g: 224,
                b: 224,
                a: 255,
            },
            0.75,
        );
        draw_line(
            renderer,
            left,
            y,
            right,
            y,
            Color {
                r: 224,
                g: 224,
                b: 224,
                a: 255,
            },
            0.75,
        );
    }

    let mut prev = None;
    for i in 0..n {
        let x = map_value(series.x[i], xmin, xmax, left, right);
        let y = map_value(series.y[i], ymin, ymax, bottom, top);
        if let Some((px, py)) = prev {
            draw_line(renderer, px, py, x, y, Color::BLUE, 1.25);
        }
        draw_point(renderer, x, y);
        prev = Some((x, y));
    }

    let text = format!("n = {n}");
    renderer.draw_text(
        &text,
        Point {
            x: left,
            y: height as f32 - 18.0,
        },
        &PlotParameters {
            font_size: 12.0,
            text_color: Color::BLACK,
            dpi: 96.0,
        },
    );
}

fn min_max(values: &[f64]) -> (f64, f64) {
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if min == max {
        (min - 1.0, max + 1.0)
    } else {
        (min, max)
    }
}

fn map_value(value: f64, min: f64, max: f64, out_min: f32, out_max: f32) -> f32 {
    let t = if min == max {
        0.5
    } else {
        ((value - min) / (max - min)).clamp(0.0, 1.0)
    };
    out_min + (out_max - out_min) * t as f32
}

fn draw_line(
    renderer: &mut AndroidHeadlessRenderer,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    color: Color,
    width: f32,
) {
    renderer.draw_path(&Path {
        commands: vec![PathCommand::MoveTo(x0, y0), PathCommand::LineTo(x1, y1)],
        fill: Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        },
        stroke: Stroke::new(width, color),
        anti_alias: true,
    });
}

fn draw_point(renderer: &mut AndroidHeadlessRenderer, x: f32, y: f32) {
    renderer.draw_path(&Path::rect(x - 2.5, y - 2.5, 5.0, 5.0).with_fill(Color::BLUE));
}

impl Default for RSession {
    fn default() -> Self {
        Self::new().expect("failed to create R session")
    }
}

impl Drop for RSession {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_uses_isolated_session_state() {
        let mut left = RSession::new().expect("left session");
        let mut right = RSession::new().expect("right session");

        assert_eq!(left.eval("x <- 11\nx").unwrap(), "[1] 11");
        assert_eq!(right.eval("x <- 29\nx").unwrap(), "[1] 29");
        assert_eq!(left.eval("x").unwrap(), "[1] 11");
        assert_eq!(right.eval("x").unwrap(), "[1] 29");
    }

    #[test]
    fn eval_result_returns_owned_typed_value() {
        let mut session = RSession::new().expect("session");
        let result = session.eval_result("c(1, 2, 3)").expect("eval");
        assert_eq!(result.output, "[1] 1 2 3");
        assert_eq!(
            result.value,
            RValue::RealVector(vec![Some(1.0), Some(2.0), Some(3.0)])
        );
    }

    #[test]
    fn configure_android_paths_reaches_embedded_runtime() {
        let root = std::env::temp_dir().join(format!(
            "rport-embed-paths-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ));
        let files = root.join("files");
        let cache = root.join("cache");
        let bundled = root.join("bundled-library");

        let mut session = RSession::new().expect("session");
        session
            .configure_android_paths(
                files.to_str().expect("utf8 files path"),
                cache.to_str().expect("utf8 cache path"),
                Some(bundled.to_str().expect("utf8 bundled path")),
            )
            .expect("path config");

        let result = session.eval_result(".libPaths()").expect("lib paths");
        assert_eq!(
            result.value,
            RValue::StringVector(vec![
                files
                    .join("R")
                    .join("library")
                    .to_string_lossy()
                    .into_owned(),
                bundled.to_string_lossy().into_owned()
            ])
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn render_evaluates_basic_plot_expression() {
        let mut session = RSession::new().expect("session");
        let png = session
            .render_with_dimensions("plot(c(1, 2, 3), c(1, 4, 9))", 320, 240)
            .expect("render");

        assert!(png.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
        assert!(png.len() > 256);
    }

    #[test]
    fn eval_reports_errors_without_panicking() {
        let mut session = RSession::new().expect("session");
        let err = session
            .eval("unknown_symbol")
            .expect_err("undefined symbol");
        let message = err.to_string();
        assert!(message.contains("object '"));
        assert!(message.contains("not found"));
    }

    #[test]
    fn close_makes_eval_fail() {
        let mut session = RSession::new().expect("session");
        session.close();
        assert!(session.eval("1 + 1").is_err());
    }

    #[test]
    fn eval_observes_pre_cancelled_token() {
        let mut session = RSession::new().expect("session");
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let err = session
            .eval_result_cancellable("repeat { 1 + 1 }", &cancellation)
            .expect_err("cancelled");
        assert!(err.to_string().contains("operation cancelled"));
    }

    #[test]
    fn eval_can_be_cancelled_from_another_thread() {
        let cancellation = CancellationToken::new();
        let worker_flag = cancellation.clone();
        let worker = std::thread::spawn(move || {
            let mut session = RSession::new().expect("session");
            session.eval_result_cancellable("repeat { 1 + 1 }", &worker_flag)
        });

        std::thread::sleep(std::time::Duration::from_millis(10));
        cancellation.cancel();

        let err = worker
            .join()
            .expect("worker should not panic")
            .expect_err("eval should be cancelled");
        assert!(err.to_string().contains("operation cancelled"));
    }

    #[test]
    fn cancellation_does_not_poison_sessions() {
        let mut cancelled_session = RSession::new().expect("cancelled session");
        let mut other_session = RSession::new().expect("other session");

        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let err = cancelled_session
            .eval_result_cancellable("repeat { 1 + 1 }", &cancellation)
            .expect_err("cancelled");
        assert!(err.to_string().contains("operation cancelled"));

        assert_eq!(other_session.eval("1 + 1").unwrap(), "[1] 2");
        assert_eq!(cancelled_session.eval("2 + 2").unwrap(), "[1] 4");
    }
}
