//! R interpreter embedding library.
//!
//! Provides the `RSession` type for embedding the R interpreter into Rust
//! applications. This crate is the safe boundary used by desktop hosts and
//! UniFFI bindings: it exposes owned Rust values and delegates runtime work to
//! rmath's per-session interpreter, never to process-global `SEXP` state.

use r_device_android_headless::AndroidHeadlessRenderer;
use r_graphics_engine::{Color, Path, PathCommand, PlotParameters, Point, RenderPlot, Stroke};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub use rmath::android::{
    RArenaStats, RAttribute, RComplexValue, RMetadata, RResourceLimits, RRuntimeInfo, RValue,
};

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

/// Metadata for an installed R package visible to an embedded session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RPackageInfo {
    pub name: String,
    pub version: String,
    pub title: String,
    pub description: String,
    pub license: String,
    pub depends: String,
    pub imports: String,
    pub suggests: String,
    pub needs_compilation: bool,
    pub path: String,
    pub library_path: String,
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
        let library_paths = self.runtime_info().library_paths;
        let mut packages: Vec<RPackageInfo> = Vec::new();
        for library_path in &library_paths {
            let Ok(entries) = std::fs::read_dir(library_path) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                let package_dir = entry.path();
                if !package_dir.is_dir() || !package_dir.join("DESCRIPTION").is_file() {
                    continue;
                }
                let Some(package_name) = package_dir
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_string)
                else {
                    continue;
                };
                if packages.iter().any(|pkg| pkg.name == package_name) {
                    continue;
                }
                if let Some(info) =
                    package_info_from_path(&package_name, &package_dir, &library_paths)
                {
                    packages.push(info);
                }
            }
        }
        packages.sort_by(|left, right| left.name.cmp(&right.name));
        packages
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
        if width < 32 || height < 32 {
            return Err(RSessionError::RenderError(
                "plot width and height must be at least 32 pixels".to_string(),
            ));
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
        let values = match call.positional.as_slice() {
            [expr] => {
                let y = numeric_series(self.eval_result(expr)?.value)?;
                let x = (1..=y.len()).map(|value| value as f64).collect();
                PlotSeries {
                    x,
                    y,
                    options: call.options.with_default_labels("Index", expr),
                }
            }
            [x_expr, y_expr, ..] => PlotSeries {
                x: numeric_series(self.eval_result(x_expr)?.value)?,
                y: numeric_series(self.eval_result(y_expr)?.value)?,
                options: call.options.with_default_labels(x_expr, y_expr),
            },
            [] => {
                return Err(RSessionError::RenderError(
                    "plot requires at least one numeric expression".to_string(),
                ));
            }
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

fn package_info_from_path(
    fallback_name: &str,
    package_path: &std::path::Path,
    library_paths: &[String],
) -> Option<RPackageInfo> {
    let description = std::fs::read_to_string(package_path.join("DESCRIPTION")).ok()?;
    let fields = description_fields(&description);
    let name = fields
        .get("Package")
        .cloned()
        .unwrap_or_else(|| fallback_name.into());
    let version = fields.get("Version").cloned().unwrap_or_default();
    let title = fields.get("Title").cloned().unwrap_or_default();
    let description = fields.get("Description").cloned().unwrap_or_default();
    let license = fields.get("License").cloned().unwrap_or_default();
    let depends = fields.get("Depends").cloned().unwrap_or_default();
    let imports = fields.get("Imports").cloned().unwrap_or_default();
    let suggests = fields.get("Suggests").cloned().unwrap_or_default();
    let needs_compilation = fields.get("NeedsCompilation").is_some_and(|value| {
        value.eq_ignore_ascii_case("yes") || value.eq_ignore_ascii_case("true")
    });
    let package_path_string = package_path.to_string_lossy().into_owned();
    let library_path = library_paths
        .iter()
        .find(|library| {
            package_path
                .parent()
                .is_some_and(|parent| parent == std::path::Path::new(library.as_str()))
        })
        .cloned()
        .unwrap_or_else(|| {
            package_path
                .parent()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    Some(RPackageInfo {
        name,
        version,
        title,
        description,
        license,
        depends,
        imports,
        suggests,
        needs_compilation,
        path: package_path_string,
        library_path,
    })
}

fn description_fields(description: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::<String, String>::new();
    let mut current_key: Option<String> = None;

    for line in description.lines() {
        if line.trim().is_empty() {
            break;
        }

        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(key) = current_key.as_ref()
                && let Some(value) = fields.get_mut(key)
            {
                if !value.is_empty() {
                    value.push('\n');
                }
                value.push_str(line.trim());
            }
            continue;
        }

        let Some((key, value)) = line.split_once(':') else {
            current_key = None;
            continue;
        };
        let key = key.trim();
        if key.is_empty() || key.chars().any(char::is_whitespace) {
            current_key = None;
            continue;
        }
        let key = key.to_string();
        fields.insert(key.clone(), value.trim().to_string());
        current_key = Some(key);
    }

    fields
}

struct PlotSeries {
    x: Vec<f64>,
    y: Vec<f64>,
    options: PlotOptions,
}

struct PlotCall<'a> {
    positional: Vec<&'a str>,
    options: PlotOptions,
}

#[derive(Debug, Clone)]
struct PlotOptions {
    main: Option<String>,
    xlab: Option<String>,
    ylab: Option<String>,
    color: Color,
    plot_type: PlotType,
    line_width: f32,
    point_radius: f32,
}

impl Default for PlotOptions {
    fn default() -> Self {
        Self {
            main: None,
            xlab: None,
            ylab: None,
            color: Color::BLUE,
            plot_type: PlotType::Both,
            line_width: 1.5,
            point_radius: 2.5,
        }
    }
}

impl PlotOptions {
    fn with_default_labels(mut self, xlab: &str, ylab: &str) -> Self {
        if self.xlab.is_none() {
            self.xlab = Some(short_label(xlab));
        }
        if self.ylab.is_none() {
            self.ylab = Some(short_label(ylab));
        }
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlotType {
    Points,
    Lines,
    Both,
}

fn parse_plot_call(code: &str) -> PlotCall<'_> {
    let trimmed = code.trim();
    let Some(inner) = trimmed
        .strip_prefix("plot(")
        .and_then(|value| value.strip_suffix(')'))
    else {
        return PlotCall {
            positional: vec![trimmed],
            options: PlotOptions::default(),
        };
    };

    let mut positional = Vec::new();
    let mut options = PlotOptions::default();
    for arg in split_top_level_args(inner) {
        let arg = arg.trim();
        if arg.is_empty() {
            continue;
        }
        if let Some((name, value)) = split_top_level_equals(arg) {
            apply_plot_option(&mut options, name.trim(), value.trim());
        } else {
            positional.push(arg);
        }
    }
    PlotCall {
        positional,
        options,
    }
}

fn split_top_level_comma(input: &str) -> Option<(&str, &str)> {
    split_top_level_at(input, ',')
}

fn split_top_level_args(input: &str) -> Vec<&str> {
    let mut args = Vec::new();
    let mut rest = input;
    while let Some((head, tail)) = split_top_level_comma(rest) {
        args.push(head);
        rest = tail;
    }
    args.push(rest);
    args
}

fn split_top_level_equals(input: &str) -> Option<(&str, &str)> {
    split_top_level_at(input, '=')
}

fn split_top_level_at(input: &str, needle: char) -> Option<(&str, &str)> {
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
            ch if ch == needle && depth == 0 => {
                return Some((&input[..idx], &input[idx + ch.len_utf8()..]));
            }
            _ => {}
        }
    }
    None
}

fn apply_plot_option(options: &mut PlotOptions, name: &str, value: &str) {
    match name {
        "main" => options.main = string_literal(value),
        "xlab" => options.xlab = string_literal(value),
        "ylab" => options.ylab = string_literal(value),
        "col" => {
            if let Some(color) = string_literal(value).as_deref().and_then(parse_color) {
                options.color = color;
            }
        }
        "type" => {
            if let Some(plot_type) = string_literal(value).as_deref().and_then(parse_plot_type) {
                options.plot_type = plot_type;
            }
        }
        "lwd" => {
            if let Some(width) = numeric_literal(value).filter(|width| *width > 0.0) {
                options.line_width = width;
            }
        }
        "cex" => {
            if let Some(scale) = numeric_literal(value).filter(|scale| *scale > 0.0) {
                options.point_radius = 2.5 * scale;
            }
        }
        _ => {}
    }
}

fn numeric_literal(value: &str) -> Option<f32> {
    value.trim().parse::<f32>().ok()
}

fn string_literal(value: &str) -> Option<String> {
    let value = value.trim();
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        Some(
            value[1..value.len() - 1]
                .replace("\\\"", "\"")
                .replace("\\'", "'"),
        )
    } else {
        None
    }
}

fn parse_color(value: &str) -> Option<Color> {
    match value.to_ascii_lowercase().as_str() {
        "black" => Some(Color::BLACK),
        "blue" => Some(Color::BLUE),
        "red" => Some(Color::RED),
        "gray" | "grey" => Some(Color {
            r: 128,
            g: 128,
            b: 128,
            a: 255,
        }),
        "darkgreen" | "green" => Some(Color {
            r: 0,
            g: 128,
            b: 0,
            a: 255,
        }),
        _ => None,
    }
}

fn parse_plot_type(value: &str) -> Option<PlotType> {
    match value {
        "p" => Some(PlotType::Points),
        "l" => Some(PlotType::Lines),
        "b" | "o" => Some(PlotType::Both),
        _ => None,
    }
}

fn short_label(expr: &str) -> String {
    let label = expr.trim();
    let char_count = label.chars().count();
    if char_count > 28 {
        let mut shortened = label.chars().take(25).collect::<String>();
        shortened.push_str("...");
        shortened
    } else {
        label.to_string()
    }
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
        RValue::Attributed { value, .. } => return numeric_series(*value),
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

    let left = 58.0f32;
    let right = (width as f32 - 24.0).max(left + 1.0);
    let top = if series.options.main.is_some() {
        48.0
    } else {
        34.0
    };
    let bottom = (height as f32 - 62.0).max(top + 1.0);
    let xmin = min_max(&series.x[..n]).0;
    let xmax = min_max(&series.x[..n]).1;
    let ymin = min_max(&series.y[..n]).0;
    let ymax = min_max(&series.y[..n]).1;

    let text_params = PlotParameters {
        font_size: 11.0,
        text_color: Color::BLACK,
        dpi: 96.0,
    };

    draw_line(renderer, left, bottom, right, bottom, Color::BLACK, 1.5);
    draw_line(renderer, left, top, left, bottom, Color::BLACK, 1.5);
    draw_line(renderer, right, top, right, bottom, Color::BLACK, 0.75);
    draw_line(renderer, left, top, right, top, Color::BLACK, 0.75);

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
        draw_line(renderer, x, bottom, x, bottom + 4.0, Color::BLACK, 1.0);
        draw_line(renderer, left - 4.0, y, left, y, Color::BLACK, 1.0);

        let x_value = xmin + (xmax - xmin) * t as f64;
        let y_value = ymax - (ymax - ymin) * t as f64;
        let x_label = tick_label(x_value);
        let y_label = tick_label(y_value);
        renderer.draw_text(
            &x_label,
            Point {
                x: x - estimated_text_width(&x_label, 11.0) / 2.0,
                y: bottom + 17.0,
            },
            &text_params,
        );
        renderer.draw_text(
            &y_label,
            Point {
                x: (left - estimated_text_width(&y_label, 11.0) - 8.0).max(0.0),
                y: y + 4.0,
            },
            &text_params,
        );
    }

    let mut prev = None;
    for i in 0..n {
        let x = map_value(series.x[i], xmin, xmax, left, right);
        let y = map_value(series.y[i], ymin, ymax, bottom, top);
        if series.options.plot_type != PlotType::Points
            && let Some((px, py)) = prev
        {
            draw_line(
                renderer,
                px,
                py,
                x,
                y,
                series.options.color,
                series.options.line_width,
            );
        }
        if series.options.plot_type != PlotType::Lines {
            draw_point(
                renderer,
                x,
                y,
                series.options.color,
                series.options.point_radius,
            );
        }
        prev = Some((x, y));
    }

    if let Some(main) = &series.options.main {
        renderer.draw_text(
            main,
            Point {
                x: centered_text_x(main, width as f32, 16.0),
                y: 24.0,
            },
            &PlotParameters {
                font_size: 16.0,
                text_color: Color::BLACK,
                dpi: 96.0,
            },
        );
    }

    if let Some(xlab) = &series.options.xlab {
        renderer.draw_text(
            xlab,
            Point {
                x: centered_text_x(xlab, width as f32, 12.0),
                y: height as f32 - 22.0,
            },
            &PlotParameters {
                font_size: 12.0,
                text_color: Color::BLACK,
                dpi: 96.0,
            },
        );
    }

    if let Some(ylab) = &series.options.ylab {
        renderer.draw_text(
            ylab,
            Point {
                x: 6.0,
                y: (top + bottom) / 2.0,
            },
            &PlotParameters {
                font_size: 12.0,
                text_color: Color::BLACK,
                dpi: 96.0,
            },
        );
    }

    let count_label = format!("n = {n}");
    renderer.draw_text(
        &count_label,
        Point {
            x: right - estimated_text_width(&count_label, 12.0),
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

fn tick_label(value: f64) -> String {
    if value == 0.0 {
        "0".to_string()
    } else if value.abs() >= 10_000.0 || value.abs() < 0.01 {
        format!("{value:.1e}")
    } else {
        let mut label = format!("{value:.2}");
        while label.contains('.') && label.ends_with('0') {
            label.pop();
        }
        if label.ends_with('.') {
            label.pop();
        }
        label
    }
}

fn estimated_text_width(text: &str, font_size: f32) -> f32 {
    text.chars().count() as f32 * font_size * 0.56
}

fn centered_text_x(text: &str, width: f32, font_size: f32) -> f32 {
    ((width - estimated_text_width(text, font_size)) / 2.0).max(0.0)
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

fn draw_point(renderer: &mut AndroidHeadlessRenderer, x: f32, y: f32, color: Color, radius: f32) {
    renderer.draw_path(
        &Path::rect(x - radius, y - radius, radius * 2.0, radius * 2.0).with_fill(color),
    );
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
    use std::io::Cursor;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Barrier};

    struct DecodedPng {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    }

    impl DecodedPng {
        fn non_white_in_region(&self, x0: u32, y0: u32, x1: u32, y1: u32) -> usize {
            let x1 = x1.min(self.width);
            let y1 = y1.min(self.height);
            let mut count = 0;
            for y in y0.min(self.height)..y1 {
                for x in x0.min(self.width)..x1 {
                    let offset = ((y * self.width + x) * 4) as usize;
                    let pixel = &self.rgba[offset..offset + 4];
                    if pixel != [255, 255, 255, 255] {
                        count += 1;
                    }
                }
            }
            count
        }

        fn red_pixels(&self) -> usize {
            self.pixels_matching(|rgba| rgba[0] > 180 && rgba[1] < 120 && rgba[2] < 120)
        }

        fn green_pixels(&self) -> usize {
            self.pixels_matching(|rgba| rgba[0] < 120 && rgba[1] > 100 && rgba[2] < 120)
        }

        fn pixels_matching(&self, matches: impl Fn(&[u8]) -> bool) -> usize {
            self.rgba
                .chunks_exact(4)
                .filter(|rgba| matches(rgba) && rgba[3] > 0)
                .count()
        }
    }

    fn decode_png_rgba(png_bytes: &[u8]) -> DecodedPng {
        let decoder = png::Decoder::new(Cursor::new(png_bytes));
        let mut reader = decoder.read_info().expect("png reader");
        let mut buffer = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buffer).expect("png frame");
        let bytes = &buffer[..info.buffer_size()];
        let rgba = match info.color_type {
            png::ColorType::Rgba => bytes.to_vec(),
            png::ColorType::Rgb => bytes
                .chunks_exact(3)
                .flat_map(|rgb| [rgb[0], rgb[1], rgb[2], 255])
                .collect(),
            other => panic!("unexpected png color type: {other:?}"),
        };
        DecodedPng {
            width: info.width,
            height: info.height,
            rgba,
        }
    }

    fn unique_test_root(root_name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "{root_name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ))
    }

    fn make_test_package(root_name: &str) -> (PathBuf, PathBuf) {
        make_test_package_with_source(
            root_name,
            "export(tiny_value)\n",
            "tiny_value <- function() 42L\n",
        )
    }

    fn make_test_package_with_source(
        root_name: &str,
        namespace: &str,
        source: &str,
    ) -> (PathBuf, PathBuf) {
        let root = unique_test_root(root_name);
        let bundled = root.join("bundled-library");
        let pkg = write_fixture_package(
            &bundled,
            FixturePackage {
                name: "tiny",
                description: concat!(
                    "Package: tiny\n",
                    "Version: 0.0.1\n",
                    "Title: Tiny Test Package\n",
                    "Description: Tiny package for Android runtime tests\n",
                    "License: MIT\n",
                    "Depends: R (>= 4.0.0)\n",
                    "Imports: depall, depfrom\n",
                    "Suggests: testthat\n",
                    "NeedsCompilation: no\n",
                ),
                namespace,
                sources: &[("tiny.R", source)],
                data_sources: &[],
                extra_files: &[],
            },
        );
        (root, pkg)
    }

    struct FixturePackage<'a> {
        name: &'a str,
        description: &'a str,
        namespace: &'a str,
        sources: &'a [(&'a str, &'a str)],
        data_sources: &'a [(&'a str, &'a str)],
        extra_files: &'a [(&'a str, &'a [u8])],
    }

    fn write_fixture_package(library: &Path, package: FixturePackage<'_>) -> PathBuf {
        let pkg = library.join(package.name);
        let r_dir = pkg.join("R");
        std::fs::create_dir_all(&r_dir).expect("package R dir");
        std::fs::write(pkg.join("DESCRIPTION"), package.description).expect("description");
        std::fs::write(pkg.join("NAMESPACE"), package.namespace).expect("namespace");
        for (file, source) in package.sources {
            std::fs::write(r_dir.join(file), source).expect("R source");
        }
        if !package.data_sources.is_empty() {
            let data_dir = pkg.join("data");
            std::fs::create_dir_all(&data_dir).expect("data dir");
            for (file, source) in package.data_sources {
                std::fs::write(data_dir.join(file), source).expect("data source");
            }
        }
        for (file, bytes) in package.extra_files {
            let path = pkg.join(file);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("extra file parent");
            }
            std::fs::write(path, bytes).expect("extra file");
        }
        pkg
    }

    fn android_paths_for(root: &Path) -> AndroidRuntimePaths {
        AndroidRuntimePaths::new(
            root.join("files").to_str().expect("utf8 files path"),
            root.join("cache").to_str().expect("utf8 cache path"),
            Some(
                root.join("bundled-library")
                    .to_str()
                    .expect("utf8 bundled path"),
            ),
        )
    }

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

        let strings = session
            .eval_result("c(\"a\", NA_character_)")
            .expect("eval strings");
        assert_eq!(
            strings.value,
            RValue::StringVector(vec![Some("a".to_string()), None])
        );
    }

    #[test]
    fn resource_limits_are_session_owned_and_enforced() {
        let mut limited = RSession::new().expect("limited session");
        let mut normal = RSession::new().expect("normal session");

        limited
            .set_resource_limits(RResourceLimits {
                max_eval_depth: 1,
                max_execution_time_ms: 0,
                max_alloc_bytes: 0,
                max_arena_nodes: 0,
            })
            .expect("set limits");

        let limits = limited.resource_limits();
        assert_eq!(limits.max_eval_depth, 1);
        assert_eq!(normal.resource_limits().max_eval_depth, 500);

        let err = limited
            .eval_result("{ 1 + 1 }")
            .expect_err("depth limit should reject nested eval");
        assert!(err.to_string().contains("too deeply"));
        assert_eq!(normal.eval("1 + 1").expect("normal eval"), "[1] 2");
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
        let paths = AndroidRuntimePaths::new(
            files.to_str().expect("utf8 files path"),
            cache.to_str().expect("utf8 cache path"),
            Some(bundled.to_str().expect("utf8 bundled path")),
        );

        assert_eq!(
            paths.user_library_dir(),
            files
                .join("R")
                .join("library")
                .to_string_lossy()
                .into_owned()
        );
        assert_eq!(
            paths.temp_dir(),
            cache.join("Rtmp").to_string_lossy().into_owned()
        );
        assert_eq!(
            paths.library_paths(),
            vec![
                files
                    .join("R")
                    .join("library")
                    .to_string_lossy()
                    .into_owned(),
                bundled.to_string_lossy().into_owned()
            ]
        );

        let mut session = RSession::new().expect("session");
        session
            .configure_android_runtime(&paths)
            .expect("path config");

        let result = session.eval_result(".libPaths()").expect("lib paths");
        assert_eq!(
            result.value,
            RValue::StringVector(vec![
                Some(
                    files
                        .join("R")
                        .join("library")
                        .to_string_lossy()
                        .into_owned()
                ),
                Some(bundled.to_string_lossy().into_owned())
            ])
        );
        assert_eq!(
            session.runtime_info(),
            RRuntimeInfo {
                is_active: true,
                library_paths: vec![
                    files
                        .join("R")
                        .join("library")
                        .to_string_lossy()
                        .into_owned(),
                    bundled.to_string_lossy().into_owned()
                ],
                temp_dir: cache.join("Rtmp").to_string_lossy().into_owned(),
            }
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn package_helpers_load_android_library_package() {
        let (root, pkg) = make_test_package("rport-embed-package");
        let files = root.join("files");
        let cache = root.join("cache");
        let bundled = root.join("bundled-library");
        let paths = AndroidRuntimePaths::new(
            files.to_str().expect("utf8 files path"),
            cache.to_str().expect("utf8 cache path"),
            Some(bundled.to_str().expect("utf8 bundled path")),
        );

        let mut session = RSession::new().expect("session");
        session
            .configure_android_runtime(&paths)
            .expect("path config");

        assert!(session.package_available("tiny"));
        assert_eq!(
            session.package_path("tiny"),
            Some(pkg.to_string_lossy().into_owned())
        );
        assert_eq!(
            session.package_info("tiny"),
            Some(RPackageInfo {
                name: "tiny".to_string(),
                version: "0.0.1".to_string(),
                title: "Tiny Test Package".to_string(),
                description: "Tiny package for Android runtime tests".to_string(),
                license: "MIT".to_string(),
                depends: "R (>= 4.0.0)".to_string(),
                imports: "depall, depfrom".to_string(),
                suggests: "testthat".to_string(),
                needs_compilation: false,
                path: pkg.to_string_lossy().into_owned(),
                library_path: bundled.to_string_lossy().into_owned(),
            })
        );
        assert_eq!(session.installed_packages().len(), 1);
        assert_eq!(session.installed_packages()[0].name, "tiny");
        assert!(!session.package_available("../tiny"));
        session.load_package("tiny").expect("load package");
        assert_eq!(session.eval("tiny_value()").expect("eval"), "[1] 42");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pure_r_package_corpus_smoke_lists_loads_and_runs_supported_packages() {
        let root = unique_test_root("rport-embed-corpus");
        let bundled = root.join("bundled-library");

        let base_pkg = write_fixture_package(
            &bundled,
            FixturePackage {
                name: "corpbase",
                description: concat!(
                    "Package: corpbase\n",
                    "Version: 0.1.0\n",
                    "Title: Corpus Base Package\n",
                    "Description: Exercises exports, S3 methods, and source-form data.\n",
                    "License: MIT\n",
                    "NeedsCompilation: no\n",
                ),
                namespace: "export(base_value, make_corp, corp_generic)\nS3method(corp_generic, corpclass)\n",
                sources: &[(
                    "base.R",
                    concat!(
                        "base_value <- function() 10L\n",
                        "make_corp <- function() { x <- 1L; class(x) <- \"corpclass\"; x }\n",
                        "corp_generic <- function(x) UseMethod(\"corp_generic\", x)\n",
                        "corp_generic.corpclass <- function(x) 123L\n",
                    ),
                )],
                data_sources: &[("corp_data.R", "corp_data <- 55L\n")],
                extra_files: &[],
            },
        );
        write_fixture_package(
            &bundled,
            FixturePackage {
                name: "corpimport",
                description: concat!(
                    "Package: corpimport\n",
                    "Version: 0.1.0\n",
                    "Title: Corpus Import Package\n",
                    "Description: Exercises whole-package namespace imports.\n",
                    "License: MIT\n",
                    "Imports: corpbase\n",
                    "NeedsCompilation: no\n",
                ),
                namespace: "import(corpbase)\nexport(import_value)\n",
                sources: &[("import.R", "import_value <- function() base_value() + 5L\n")],
                data_sources: &[],
                extra_files: &[],
            },
        );
        write_fixture_package(
            &bundled,
            FixturePackage {
                name: "corpfrom",
                description: concat!(
                    "Package: corpfrom\n",
                    "Version: 0.1.0\n",
                    "Title: Corpus ImportFrom Package\n",
                    "Description: Exercises selective namespace imports.\n",
                    "License: MIT\n",
                    "Imports: corpbase\n",
                    "NeedsCompilation: no\n",
                ),
                namespace: "importFrom(corpbase, base_value)\nexport(from_value)\n",
                sources: &[("from.R", "from_value <- function() base_value() + 7L\n")],
                data_sources: &[],
                extra_files: &[],
            },
        );
        write_fixture_package(
            &bundled,
            FixturePackage {
                name: "corppattern",
                description: concat!(
                    "Package: corppattern\n",
                    "Version: 0.1.0\n",
                    "Title: Corpus Export Pattern Package\n",
                    "Description: Exercises NAMESPACE exportPattern handling.\n",
                    "License: MIT\n",
                    "NeedsCompilation: no\n",
                ),
                namespace: "exportPattern(\"^pat_\")\n",
                sources: &[(
                    "pattern.R",
                    "pat_value <- function() 31L\nhidden_value <- function() 99L\n",
                )],
                data_sources: &[],
                extra_files: &[],
            },
        );
        write_fixture_package(
            &bundled,
            FixturePackage {
                name: "corps4",
                description: concat!(
                    "Package: corps4\n",
                    "Version: 0.1.0\n",
                    "Title: Corpus S4 Package\n",
                    "Description: Exercises pure-R package S4 class creation and slot access.\n",
                    "License: MIT\n",
                    "NeedsCompilation: no\n",
                ),
                namespace: "export(make_person, person_name, person_slots)\n",
                sources: &[(
                    "s4.R",
                    concat!(
                        "setClass(\"CorpusPerson\", name = \"character\", score = \"numeric\")\n",
                        "make_person <- function() new(\"CorpusPerson\", name = \"Ada\", score = 42)\n",
                        "person_name <- function(x) slot(x, \"name\")\n",
                        "person_slots <- function() slotNames(\"CorpusPerson\")\n",
                    ),
                )],
                data_sources: &[],
                extra_files: &[],
            },
        );
        write_fixture_package(
            &bundled,
            FixturePackage {
                name: "corpdataenv",
                description: concat!(
                    "Package: corpdataenv\n",
                    "Version: 0.1.0\n",
                    "Title: Corpus Data Environment Package\n",
                    "Description: Exercises data(..., envir=) package loading.\n",
                    "License: MIT\n",
                    "NeedsCompilation: no\n",
                ),
                namespace: "export(dataenv_value)\n",
                sources: &[("dataenv.R", "dataenv_value <- function() 5L\n")],
                data_sources: &[("env_data.R", "env_data <- 88L\n")],
                extra_files: &[],
            },
        );
        write_fixture_package(
            &bundled,
            FixturePackage {
                name: "corppaths",
                description: concat!(
                    "Package: corppaths\n",
                    "Version: 0.1.0\n",
                    "Title: Corpus Runtime Paths Package\n",
                    "Description: Exercises package-visible Android library paths.\n",
                    "License: MIT\n",
                    "NeedsCompilation: no\n",
                ),
                namespace: "export(corpus_lib_paths)\n",
                sources: &[("paths.R", "corpus_lib_paths <- function() .libPaths()\n")],
                data_sources: &[],
                extra_files: &[],
            },
        );
        write_fixture_package(
            &bundled,
            FixturePackage {
                name: "corpnative",
                description: concat!(
                    "Package: corpnative\n",
                    "Version: 0.1.0\n",
                    "Title: Corpus Native Policy Package\n",
                    "Description: Exercises explicit native-code rejection.\n",
                    "License: MIT\n",
                    "NeedsCompilation: no\n",
                ),
                namespace: "useDynLib(corpnative)\nexport(native_value)\n",
                sources: &[("native.R", "native_value <- function() 1L\n")],
                data_sources: &[],
                extra_files: &[],
            },
        );
        write_fixture_package(
            &bundled,
            FixturePackage {
                name: "corpcompiled",
                description: concat!(
                    "Package: corpcompiled\n",
                    "Version: 0.1.0\n",
                    "Title: Corpus Compiled Policy Package\n",
                    "Description: Exercises DESCRIPTION NeedsCompilation rejection.\n",
                    "License: MIT\n",
                    "NeedsCompilation: yes\n",
                ),
                namespace: "export(compiled_value)\n",
                sources: &[("compiled.R", "compiled_value <- function() 1L\n")],
                data_sources: &[],
                extra_files: &[],
            },
        );
        write_fixture_package(
            &bundled,
            FixturePackage {
                name: "corplazydata",
                description: concat!(
                    "Package: corplazydata\n",
                    "Version: 0.1.0\n",
                    "Title: Corpus Lazy Data Policy Package\n",
                    "Description: Exercises serialized data rejection.\n",
                    "License: MIT\n",
                    "NeedsCompilation: no\n",
                ),
                namespace: "export(lazy_value)\n",
                sources: &[("lazy.R", "lazy_value <- function() 1L\n")],
                data_sources: &[],
                extra_files: &[("data/lazy_data.rda", b"unsupported serialized data")],
            },
        );

        let paths = android_paths_for(&root);
        let mut session = RSession::new().expect("session");
        session
            .configure_android_runtime(&paths)
            .expect("path config");

        let installed_names = session
            .installed_packages()
            .into_iter()
            .map(|package| package.name)
            .collect::<Vec<_>>();
        assert_eq!(
            installed_names,
            vec![
                "corpbase",
                "corpcompiled",
                "corpdataenv",
                "corpfrom",
                "corpimport",
                "corplazydata",
                "corpnative",
                "corppaths",
                "corppattern",
                "corps4",
            ]
        );
        assert_eq!(
            session.package_path("corpbase"),
            Some(base_pkg.to_string_lossy().into_owned())
        );

        session.load_package("corpbase").expect("load corpbase");
        assert_eq!(session.eval("base_value()").expect("base value"), "[1] 10");
        assert_eq!(
            session
                .eval("corp_generic(make_corp())")
                .expect("s3 dispatch"),
            "[1] 123"
        );
        assert_eq!(
            session
                .eval("data(package = \"corpbase\")")
                .expect("list data"),
            "[1] \"corp_data\""
        );
        assert_eq!(
            session
                .eval("data(\"corp_data\", package = \"corpbase\")\ncorp_data")
                .expect("load data"),
            "[1] 55"
        );

        session.load_package("corpimport").expect("load import");
        assert_eq!(
            session.eval("import_value()").expect("import value"),
            "[1] 15"
        );
        session.load_package("corpfrom").expect("load importFrom");
        assert_eq!(session.eval("from_value()").expect("from value"), "[1] 17");
        session
            .load_package("corppattern")
            .expect("load exportPattern");
        assert_eq!(
            session.eval("pat_value()").expect("pattern value"),
            "[1] 31"
        );
        let hidden = session
            .eval("hidden_value")
            .expect_err("hidden pattern symbol should not be attached");
        assert!(hidden.to_string().contains("not found"), "{hidden}");

        session.load_package("corps4").expect("load S4 package");
        assert_eq!(
            session
                .eval("p <- make_person(); all(c(isS4(p), is(p, \"CorpusPerson\"), person_name(p) == \"Ada\", all(person_slots() == c(\"name\", \"score\"))))")
                .expect("S4 package value"),
            "[1] TRUE"
        );

        session
            .load_package("corpdataenv")
            .expect("load data env package");
        assert_eq!(
            session
                .eval("e <- new.env(); data(\"env_data\", package = \"corpdataenv\", envir = e); c(exists(\"env_data\", envir = e), exists(\"env_data\"), get(\"env_data\", envir = e))")
                .expect("data envir"),
            "[1]  1  0 88"
        );

        session
            .load_package("corppaths")
            .expect("load paths package");
        assert_eq!(
            session
                .eval_result("corpus_lib_paths()")
                .expect("package-visible library paths")
                .value,
            RValue::StringVector(paths.library_paths().into_iter().map(Some).collect())
        );

        let native = session
            .load_package("corpnative")
            .expect_err("native package should be rejected");
        assert!(
            native.to_string().contains("useDynLib(corpnative)"),
            "{native}"
        );
        assert!(
            native.to_string().contains("pure-R Android runtime"),
            "{native}"
        );
        let compiled = session
            .load_package("corpcompiled")
            .expect_err("compiled package should be rejected");
        assert!(
            compiled.to_string().contains("NeedsCompilation: yes"),
            "{compiled}"
        );
        session
            .load_package("corplazydata")
            .expect("load lazy-data package namespace");
        let lazy = session
            .eval_result("data(\"lazy_data\", package = \"corplazydata\")")
            .expect_err("serialized lazy data should be rejected");
        assert!(
            lazy.to_string()
                .contains("unsupported serialized/lazy data"),
            "{lazy}"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pure_r_package_corpus_keeps_same_named_packages_isolated_by_session() {
        let make_root = |name: &str, value: i32| {
            let root = unique_test_root(name);
            let bundled = root.join("bundled-library");
            write_fixture_package(
                &bundled,
                FixturePackage {
                    name: "corpbase",
                    description: concat!(
                        "Package: corpbase\n",
                        "Version: 0.1.0\n",
                        "Title: Corpus Base Package\n",
                        "Description: Same package name, different library path.\n",
                        "License: MIT\n",
                        "NeedsCompilation: no\n",
                    ),
                    namespace: "export(base_value)\n",
                    sources: &[("base.R", &format!("base_value <- function() {value}L\n"))],
                    data_sources: &[],
                    extra_files: &[],
                },
            );
            root
        };
        let left_root = make_root("rport-embed-corpus-left", 21);
        let right_root = make_root("rport-embed-corpus-right", 84);

        let mut left = RSession::new().expect("left session");
        left.configure_android_runtime(&android_paths_for(&left_root))
            .expect("left paths");
        let mut right = RSession::new().expect("right session");
        right
            .configure_android_runtime(&android_paths_for(&right_root))
            .expect("right paths");

        left.load_package("corpbase").expect("left load");
        right.load_package("corpbase").expect("right load");
        assert_eq!(left.eval("base_value()").expect("left value"), "[1] 21");
        assert_eq!(right.eval("base_value()").expect("right value"), "[1] 84");

        let _ = std::fs::remove_dir_all(left_root);
        let _ = std::fs::remove_dir_all(right_root);
    }

    #[test]
    fn parallel_sessions_keep_android_runtime_state_isolated() {
        const WORKERS: usize = 4;

        let barrier = Arc::new(Barrier::new(WORKERS));
        let handles = (0..WORKERS)
            .map(|index| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let value = 100 + index as i32;
                    let namespace =
                        "export(tiny_value, make_tiny, tiny_generic)\nS3method(tiny_generic, tinything)\n";
                    let source = format!(
                        r#"
tiny_value <- function() {value}L
make_tiny <- function() {{
    x <- {value}L
    class(x) <- "tinything"
    x
}}
tiny_generic <- function(x) UseMethod("tiny_generic", x)
tiny_generic.tinything <- function(x) {value}L
"#
                    );
                    let (root, _pkg) = make_test_package_with_source(
                        &format!("rport-embed-parallel-{index}"),
                        namespace,
                        &source,
                    );
                    let files = root.join("files");
                    let cache = root.join("cache");
                    let bundled = root.join("bundled-library");
                    let paths = AndroidRuntimePaths::new(
                        files.to_str().expect("utf8 files path"),
                        cache.to_str().expect("utf8 cache path"),
                        Some(bundled.to_str().expect("utf8 bundled path")),
                    );

                    let mut session = RSession::new().expect("session");
                    session
                        .configure_android_runtime(&paths)
                        .expect("path config");

                    barrier.wait();

                    session.load_package("tiny").expect("load package");
                    assert_eq!(
                        session.eval("tiny_value()").expect("tiny value"),
                        format!("[1] {value}")
                    );

                    assert_eq!(
                        session
                            .eval("tiny_generic(make_tiny())")
                            .expect("s3 dispatch"),
                        format!("[1] {value}")
                    );

                    let captured = session
                        .eval("capture.output({ cat(\"session local\\n\") })")
                        .expect("capture output");
                    assert!(captured.contains("session local"), "{captured}");

                    let err = session
                        .eval("unknown_symbol")
                        .expect_err("undefined symbol should fail");
                    assert!(err.to_string().contains("not found"));
                    assert_eq!(
                        session.eval("tiny_value()").expect("eval after error"),
                        format!("[1] {value}")
                    );

                    let png = session
                        .render_with_dimensions(
                            &format!(
                                "plot(c(1, 2, 3), c({value}, {next}, {last}), main = \"session {index}\", col = \"red\", type = \"l\")",
                                next = value + 1,
                                last = value + 2,
                            ),
                            240,
                            180,
                        )
                        .expect("render");
                    let decoded = decode_png_rgba(&png);
                    assert!(decoded.red_pixels() > 5);
                    assert!(decoded.non_white_in_region(0, 0, decoded.width, 40) > 5);

                    let _ = std::fs::remove_dir_all(root);
                    value
                })
            })
            .collect::<Vec<_>>();

        let mut values = handles
            .into_iter()
            .map(|handle| handle.join().expect("worker should not panic"))
            .collect::<Vec<_>>();
        values.sort_unstable();
        assert_eq!(values, vec![100, 101, 102, 103]);
    }

    #[test]
    fn android_runtime_paths_without_bundled_library_only_returns_user_library() {
        let paths = AndroidRuntimePaths::new("/tmp/app-files", "/tmp/app-cache", None::<&str>);

        assert_eq!(paths.user_library_dir(), "/tmp/app-files/R/library");
        assert_eq!(paths.temp_dir(), "/tmp/app-cache/Rtmp");
        assert_eq!(paths.library_paths(), vec!["/tmp/app-files/R/library"]);
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
    fn plot_call_parser_handles_common_named_options() {
        let call = parse_plot_call(
            "plot(c(1, 2, 3), c(4, 5, 6), main = \"Revenue μ\", xlab = 'day', ylab = \"value\", col = \"red\", type = \"l\")",
        );

        assert_eq!(call.positional, vec!["c(1, 2, 3)", "c(4, 5, 6)"]);
        assert_eq!(call.options.main.as_deref(), Some("Revenue μ"));
        assert_eq!(call.options.xlab.as_deref(), Some("day"));
        assert_eq!(call.options.ylab.as_deref(), Some("value"));
        assert_eq!(call.options.color, Color::RED);
        assert_eq!(call.options.plot_type, PlotType::Lines);
        assert_eq!(call.options.line_width, 1.5);

        let styled = parse_plot_call(
            "plot(c(1, 2, 3), c(4, 5, 6), type = \"p\", col = \"green\", lwd = 3, cex = 1.5)",
        );
        assert_eq!(styled.options.plot_type, PlotType::Points);
        assert_eq!(
            styled.options.color,
            Color {
                r: 0,
                g: 128,
                b: 0,
                a: 255,
            }
        );
        assert_eq!(styled.options.line_width, 3.0);
        assert_eq!(styled.options.point_radius, 3.75);
    }

    #[test]
    fn render_honors_plot_labels_and_color() {
        let mut session = RSession::new().expect("session");
        let png = session
            .render_with_dimensions(
                "plot(c(1, 2, 3, 4), c(1, 4, 9, 16), main = \"Revenue μ\", xlab = \"day\", ylab = \"value\", col = \"red\", type = \"l\")",
                360,
                260,
            )
            .expect("render");
        let decoded = decode_png_rgba(&png);

        assert!(decoded.red_pixels() > 10);
        assert!(decoded.non_white_in_region(0, 0, decoded.width, 42) > 5);
        assert!(
            decoded.non_white_in_region(0, decoded.height - 40, decoded.width, decoded.height) > 5
        );
        assert!(decoded.non_white_in_region(0, 58, 48, decoded.height - 52) > 5);
    }

    #[test]
    fn render_supports_point_mode_and_responsive_dimensions() {
        let mut session = RSession::new().expect("session");
        let small = session
            .render_with_dimensions(
                "plot(c(1, 2, 3), c(3, 1, 2), type = \"p\", col = \"green\", cex = 1.4)",
                96,
                96,
            )
            .expect("small render");
        let small = decode_png_rgba(&small);
        assert_eq!(small.width, 96);
        assert_eq!(small.height, 96);
        assert!(small.green_pixels() > 5);
        assert!(small.non_white_in_region(0, 0, small.width, small.height) > 20);

        let large = session
            .render_with_dimensions(
                "plot(c(1, 2, 3, 4, 5), c(1, 4, 9, 16, 25), main = \"Large plot\", type = \"b\", col = \"blue\", lwd = 2)",
                1024,
                640,
            )
            .expect("large render");
        let large = decode_png_rgba(&large);
        assert_eq!(large.width, 1024);
        assert_eq!(large.height, 640);
        assert!(large.non_white_in_region(0, 0, large.width, 56) > 20);
        assert!(large.non_white_in_region(0, 0, large.width, large.height) > 100);
    }

    #[test]
    fn render_reports_actionable_plot_errors() {
        let mut session = RSession::new().expect("session");

        let too_small = session
            .render_with_dimensions("plot(c(1), c(1))", 0, 120)
            .expect_err("zero width should fail");
        assert!(too_small.to_string().contains("at least 32 pixels"));

        let non_numeric = session
            .render_with_dimensions("plot(c(\"a\", \"b\"))", 320, 240)
            .expect_err("non-numeric plot should fail");
        assert!(non_numeric.to_string().contains("numeric"));

        let non_finite = session
            .render_with_dimensions("plot(c(1, Inf))", 320, 240)
            .expect_err("non-finite plot should fail");
        assert!(non_finite.to_string().contains("finite"));
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
