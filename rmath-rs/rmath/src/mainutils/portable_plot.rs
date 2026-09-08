//! Portable numeric plot.default implementation, below R evaluation and S3 dispatch.
use crate::mainutils::essentials::{arg_by_name_or_position, base_error, elt_to_string};
use crate::sexp::{
    accessors::*,
    ffi::{SEXP, SEXPTYPE},
    globals::R_NilValue,
};
use r_graphics_engine::{Color, Path, PathCommand, PlotParameters, Point, Stroke};
pub(crate) struct PlotSeries {
    pub(crate) x: Vec<f64>,
    pub(crate) y: Vec<f64>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlotType {
    Points,
    Lines,
    Both,
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

pub(crate) fn draw_series(
    renderer: &mut dyn r_graphics_engine::DrawTarget,
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
    renderer: &mut dyn r_graphics_engine::DrawTarget,
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

fn draw_point(
    renderer: &mut dyn r_graphics_engine::DrawTarget,
    x: f32,
    y: f32,
    color: Color,
    radius: f32,
) {
    renderer.draw_path(&Path::circle(x, y, radius).with_fill(color));
}

pub(crate) unsafe fn plot_default(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = arg_by_name_or_position(args, &["x"], 0);
        let y_arg = arg_by_name_or_position(args, &["y"], 1);
        let values = |x: SEXP| -> Vec<f64> {
            if !matches!(
                SEXPTYPE(TYPEOF(x)),
                SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP | SEXPTYPE::REALSXP
            ) {
                base_error("plot data must be numeric".to_owned());
            }
            let values: Vec<_> = (0..XLENGTH(x)).map(|i| elt_to_real(x, i)).collect();
            if values.iter().any(|x| !x.is_finite()) {
                base_error("plot data must contain only finite values".to_owned());
            }
            values
        };
        let (x, y) = if y_arg == R_NilValue() {
            let y = values(x_arg);
            ((1..=y.len()).map(|i| i as f64).collect(), y)
        } else {
            (values(x_arg), values(y_arg))
        };
        if x.len() != y.len() {
            base_error("'x' and 'y' lengths differ".to_owned());
        }
        let mut options = PlotOptions {
            color: Color::BLACK,
            plot_type: PlotType::Points,
            ..Default::default()
        };
        for (name, field) in [
            ("main", &mut options.main),
            ("xlab", &mut options.xlab),
            ("ylab", &mut options.ylab),
        ] {
            let v = arg_by_name_or_position(args, &[name], usize::MAX);
            if v != R_NilValue() {
                *field = Some(elt_to_string(v, 0));
            }
        }
        if options.xlab.is_none() {
            options.xlab = Some("x".to_owned());
        }
        if options.ylab.is_none() {
            options.ylab = Some("y".to_owned());
        }
        let col = arg_by_name_or_position(args, &["col"], usize::MAX);
        if col != R_NilValue() {
            options.color = parse_color(&elt_to_string(col, 0))
                .unwrap_or_else(|| base_error("invalid color specification".to_owned()));
        }
        let ty = arg_by_name_or_position(args, &["type"], usize::MAX);
        if ty != R_NilValue() {
            options.plot_type = parse_plot_type(&elt_to_string(ty, 0))
                .unwrap_or_else(|| base_error("unsupported plot type".to_owned()));
        }
        for (name, field, scale) in [
            ("lwd", &mut options.line_width, 1.0),
            ("cex", &mut options.point_radius, 2.5),
        ] {
            let v = arg_by_name_or_position(args, &[name], usize::MAX);
            if v != R_NilValue() {
                *field = elt_to_real(v, 0) as f32 * scale;
            }
        }
        let backend = crate::sexp::instance::with_required_current_instance(|inst| {
            (*inst).current_renderplot_backend
        })
        .unwrap_or_else(|| base_error("plot requires an active graphics device".to_owned()));
        let renderer = &mut *backend;
        let (width, height) = renderer.dimensions();
        draw_series(renderer, width, height, &PlotSeries { x, y, options });
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

unsafe fn elt_to_real(value: SEXP, i: i64) -> f64 {
    unsafe {
        match SEXPTYPE(TYPEOF(value)) {
            SEXPTYPE::REALSXP => *REAL(value).add(i as usize),
            SEXPTYPE::INTSXP => integer_as_real(*INTEGER(value).add(i as usize)),
            SEXPTYPE::LGLSXP => integer_as_real(*LOGICAL(value).add(i as usize)),
            _ => f64::NAN,
        }
    }
}

fn integer_as_real(v: i32) -> f64 {
    if v == i32::MIN { f64::NAN } else { v as f64 }
}
