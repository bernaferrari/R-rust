//! Parsing and drawing of simple `plot()` calls on the headless renderer.


use r_device_android_headless::AndroidHeadlessRenderer;
use r_graphics_engine::{Color, Path, PathCommand, PlotParameters, Point, RenderPlot, Stroke};

use rmath::android::RValue;

use crate::RSessionError;
pub(crate) struct PlotSeries {
    pub(crate) x: Vec<f64>,
    pub(crate) y: Vec<f64>,
    pub(crate) options: PlotOptions,
}

pub(crate) struct PlotCall<'a> {
    pub(crate) positional: Vec<&'a str>,
    pub(crate) options: PlotOptions,
}

#[derive(Debug, Clone)]
pub(crate) struct PlotOptions {
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
    pub(crate) fn with_default_labels(mut self, xlab: &str, ylab: &str) -> Self {
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

pub(crate) fn parse_plot_call(code: &str) -> PlotCall<'_> {
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
    let lower = value.to_ascii_lowercase();
    if let Some(hex) = lower.strip_prefix('#') {
        return parse_hex_color(hex);
    }
    match lower.as_str() {
        "black" => Some(Color {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
        }),
        "red" => Some(Color {
            r: 255,
            g: 0,
            b: 0,
            a: 255,
        }),
        "green" | "green3" => Some(Color {
            r: 0,
            g: 205,
            b: 0,
            a: 255,
        }),
        "blue" => Some(Color {
            r: 0,
            g: 0,
            b: 255,
            a: 255,
        }),
        "cyan" => Some(Color {
            r: 0,
            g: 255,
            b: 255,
            a: 255,
        }),
        "magenta" => Some(Color {
            r: 255,
            g: 0,
            b: 255,
            a: 255,
        }),
        "yellow" => Some(Color {
            r: 255,
            g: 255,
            b: 0,
            a: 255,
        }),
        "gray" | "grey" => Some(Color {
            r: 190,
            g: 190,
            b: 190,
            a: 255,
        }),
        "white" => Some(Color {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        }),
        "orange" => Some(Color {
            r: 255,
            g: 165,
            b: 0,
            a: 255,
        }),
        "purple" => Some(Color {
            r: 160,
            g: 32,
            b: 240,
            a: 255,
        }),
        "brown" => Some(Color {
            r: 165,
            g: 42,
            b: 42,
            a: 255,
        }),
        "pink" => Some(Color {
            r: 255,
            g: 192,
            b: 203,
            a: 255,
        }),
        "darkgreen" => Some(Color {
            r: 0,
            g: 100,
            b: 0,
            a: 255,
        }),
        "darkblue" | "navy" => Some(Color {
            r: 0,
            g: 0,
            b: 128,
            a: 255,
        }),
        "darkred" => Some(Color {
            r: 139,
            g: 0,
            b: 0,
            a: 255,
        }),
        "lightblue" => Some(Color {
            r: 173,
            g: 216,
            b: 230,
            a: 255,
        }),
        "lightgreen" => Some(Color {
            r: 144,
            g: 238,
            b: 144,
            a: 255,
        }),
        "gold" => Some(Color {
            r: 255,
            g: 215,
            b: 0,
            a: 255,
        }),
        _ => None,
    }
}

fn parse_hex_color(hex: &str) -> Option<Color> {
    let len = hex.len();
    if len != 6 && len != 8 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    let a = if len == 8 {
        u8::from_str_radix(&hex[6..8], 16).ok()?
    } else {
        255
    };
    Some(Color { r, g, b, a })
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

pub(crate) fn numeric_series(value: RValue) -> Result<Vec<f64>, RSessionError> {
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

pub(crate) fn draw_series(
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
        ..Default::default()
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
                ..Default::default()
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
                ..Default::default()
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
                ..Default::default()
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
            ..Default::default()
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
    renderer.draw_path(&Path::circle(x, y, radius).with_fill(color));
}

#[cfg(test)]
mod tests {
    use super::*;

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
                g: 205,
                b: 0,
                a: 255,
            }
        );
        assert_eq!(styled.options.line_width, 3.0);
        assert_eq!(styled.options.point_radius, 3.75);
    }
}
