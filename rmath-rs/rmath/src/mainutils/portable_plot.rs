//! Portable base plotting: owned coordinate/style state, ordinary R dispatch,
//! and device drawing. R values are decoded before a renderer is borrowed.
use crate::library::graphics::par::{ParValue, parameter, set_plot_parameter};
use crate::mainutils::essentials::{arg_by_name_or_position, base_error, elt_to_string};
use crate::sexp::{
    accessors::*,
    ffi::{SEXP, SEXPTYPE},
    globals::R_NilValue,
    instance::with_required_current_instance,
};
use r_graphics_engine::{
    Color, DashPattern, DrawTarget, Path, PathCommand, PlotParameters, Point, Stroke, TextAnchor,
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Coordinates {
    pub limits: [f64; 4],
    pub rect: [f32; 4],
    pub log: [bool; 2],
}
#[derive(Default)]
pub(crate) struct GraphicsState {
    pub current: Option<Coordinates>,
    layout: [usize; 2],
    next: usize,
}
impl Coordinates {
    fn map(self, x: f64, y: f64) -> Point {
        let x = if self.log[0] { x.log10() } else { x };
        let y = if self.log[1] { y.log10() } else { y };
        Point {
            x: (self.rect[0] as f64
                + (x - self.limits[0]) / (self.limits[1] - self.limits[0])
                    * (self.rect[2] - self.rect[0]) as f64) as f32,
            y: (self.rect[3] as f64
                - (y - self.limits[2]) / (self.limits[3] - self.limits[2])
                    * (self.rect[3] - self.rect[1]) as f64) as f32,
        }
    }
    fn raw(self, axis: usize, v: f64) -> f64 {
        if self.log[axis] { 10f64.powf(v) } else { v }
    }
}
#[derive(Clone)]
struct Style {
    colors: Vec<Color>,
    background: Vec<Color>,
    symbols: Vec<i32>,
    width: f32,
    size: f32,
    dash: Option<DashPattern>,
    blank: bool,
}
impl Style {
    fn color(&self, i: usize) -> Color {
        self.colors[i % self.colors.len()]
    }
    fn stroke(&self, i: usize) -> Stroke {
        let mut stroke = Stroke::new(if self.blank { 0. } else { self.width }, self.color(i));
        stroke.dash_pattern.clone_from(&self.dash);
        stroke
    }
}
fn transparent() -> Color {
    Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    }
}
fn par_numbers(name: &str) -> Vec<f64> {
    match parameter(name) {
        ParValue::Real(v) => v,
        ParValue::Integer(v) | ParValue::Logical(v) => v.into_iter().map(f64::from).collect(),
        _ => vec![],
    }
}
unsafe fn values(x: SEXP) -> Vec<f64> {
    unsafe {
        if x == R_NilValue() {
            return vec![];
        }
        if !matches!(
            SEXPTYPE(TYPEOF(x)),
            SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP
        ) {
            base_error("plot data must be numeric");
        }
        (0..XLENGTH(x))
            .map(|i| {
                if TYPEOF(x) == SEXPTYPE::REALSXP {
                    *REAL(x).add(i as usize)
                } else {
                    let v = if TYPEOF(x) == SEXPTYPE::INTSXP {
                        *INTEGER(x).add(i as usize)
                    } else {
                        *LOGICAL(x).add(i as usize)
                    };
                    if v == i32::MIN { f64::NAN } else { v as f64 }
                }
            })
            .collect()
    }
}
unsafe fn bindings(args: SEXP, formals: &[&str]) -> Vec<SEXP> {
    unsafe {
        let mut slots = vec![None; formals.len()];
        let mut positional = vec![];
        let mut p = args;
        while !p.is_null() && p != R_NilValue() {
            let tag = TAG(p);
            if tag.is_null() || tag == R_NilValue() {
                positional.push(CAR(p));
            } else {
                let name = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag))).to_string_lossy();
                if let Some(i) = formals.iter().position(|x| *x == name) {
                    if slots[i].replace(CAR(p)).is_some() {
                        base_error("formal argument matched by multiple actual arguments");
                    }
                }
            }
            p = CDR(p);
        }
        let mut positional = positional.into_iter();
        for slot in &mut slots {
            if slot.is_none() {
                *slot = positional.next();
            }
        }
        slots
            .into_iter()
            .map(|v| v.unwrap_or_else(|| R_NilValue()))
            .collect()
    }
}
fn arg(args: SEXP, name: &str) -> SEXP {
    arg_by_name_or_position(args, &[name], usize::MAX)
}
unsafe fn scalar(args: SEXP, name: &str, default: f64) -> f64 {
    unsafe {
        let x = arg(args, name);
        if x == R_NilValue() {
            return default;
        }
        let v = values(x);
        if v.len() != 1 || !v[0].is_finite() {
            base_error(format!("invalid '{name}'"));
        }
        v[0]
    }
}
unsafe fn label(args: SEXP, name: &str) -> Option<String> {
    unsafe {
        let x = arg(args, name);
        if x == R_NilValue() {
            None
        } else if XLENGTH(x) > 0 {
            Some(elt_to_string(x, 0))
        } else {
            Some(String::new())
        }
    }
}
unsafe fn colors(x: SEXP, default: Color) -> Vec<Color> {
    unsafe {
        if x == R_NilValue() {
            return vec![default];
        }
        let n = XLENGTH(x);
        if n == 0 {
            base_error("invalid color specification");
        }
        if !matches!(
            SEXPTYPE(TYPEOF(x)),
            SEXPTYPE::STRSXP | SEXPTYPE::INTSXP | SEXPTYPE::REALSXP | SEXPTYPE::LGLSXP
        ) {
            base_error("invalid color specification");
        }
        (0..n)
            .map(|i| {
                let c = crate::library::grdevices::colors::inRGBpar3(x, i as i32, 0x00ffffff);
                Color {
                    r: c as u8,
                    g: (c >> 8) as u8,
                    b: (c >> 16) as u8,
                    a: (c >> 24) as u8,
                }
            })
            .collect()
    }
}
unsafe fn style(args: SEXP) -> Style {
    unsafe {
        let foreground = colors(arg(args, "col"), Color::BLACK);
        let background = colors(arg(args, "bg"), transparent());
        let pch = arg(args, "pch");
        let symbols: Vec<i32> = if pch == R_NilValue() {
            par_numbers("pch").iter().map(|v| *v as i32).collect()
        } else if TYPEOF(pch) == SEXPTYPE::STRSXP {
            (0..XLENGTH(pch))
                .map(|i| {
                    elt_to_string(pch, i)
                        .chars()
                        .next()
                        .map_or(i32::MIN, |c| -(c as i32))
                })
                .collect()
        } else {
            values(pch)
                .into_iter()
                .map(|v| if v.is_nan() { i32::MIN } else { v as i32 })
                .collect()
        };
        let width = scalar(
            args,
            "lwd",
            par_numbers("lwd").first().copied().unwrap_or(1.),
        ) as f32;
        let size = scalar(
            args,
            "cex",
            par_numbers("cex").first().copied().unwrap_or(1.),
        ) as f32
            * 3.;
        if width < 0. || size < 0. {
            base_error("invalid line width or point size");
        }
        let lty = arg(args, "lty");
        let lty = if lty == R_NilValue() {
            "solid".to_owned()
        } else {
            elt_to_string(lty, 0)
        };
        let pattern = match lty.as_str() {
            "0" | "blank" => vec![],
            "1" | "solid" => vec![],
            "2" | "dashed" => vec![4., 4.],
            "3" | "dotted" => vec![1., 3.],
            "4" | "dotdash" => vec![1., 3., 4., 3.],
            "5" | "longdash" => vec![7., 3.],
            "6" | "twodash" => vec![2., 2., 6., 2.],
            s => {
                if s.len() % 2 != 0 || s.len() > 8 {
                    base_error("invalid line type");
                }
                s.chars()
                    .map(|c| {
                        c.to_digit(16)
                            .filter(|v| *v > 0)
                            .unwrap_or_else(|| base_error("invalid line type"))
                            as f32
                    })
                    .collect()
            }
        };
        Style {
            colors: foreground,
            background,
            symbols: if symbols.is_empty() {
                vec![i32::MIN]
            } else {
                symbols
            },
            width,
            size,
            dash: if pattern.is_empty() {
                None
            } else {
                Some(DashPattern {
                    intervals: pattern.into_iter().map(|v| v * width.max(1.)).collect(),
                    offset: 0.,
                })
            },
            blank: matches!(lty.as_str(), "0" | "blank"),
        }
    }
}
unsafe fn xy(args: SEXP) -> (Vec<f64>, Vec<f64>) {
    unsafe {
        let a = bindings(args, &["x", "y"]);
        let mut x = a[0];
        let mut y = a[1];
        if y == R_NilValue() && TYPEOF(x) == SEXPTYPE::VECSXP {
            let names =
                crate::eval::attrib_core::getAttrib(x, crate::eval::attrib_core::R_NamesSymbol());
            if names != R_NilValue() {
                for i in 0..XLENGTH(x) {
                    match elt_to_string(names, i).as_str() {
                        "x" => x = VECTOR_ELT(a[0], i),
                        "y" => y = VECTOR_ELT(a[0], i),
                        _ => {}
                    }
                }
            }
        }
        if y == R_NilValue() && TYPEOF(x) == SEXPTYPE::CPLXSXP {
            return (
                (0..XLENGTH(x))
                    .map(|i| (*COMPLEX(x).add(i as usize)).r)
                    .collect(),
                (0..XLENGTH(x))
                    .map(|i| (*COMPLEX(x).add(i as usize)).i)
                    .collect(),
            );
        }
        let mut xv = values(x);
        let yv = if y == R_NilValue() {
            let dim =
                crate::eval::attrib_core::getAttrib(x, crate::eval::attrib_core::R_DimSymbol());
            if dim != R_NilValue() && XLENGTH(dim) == 2 && *INTEGER(dim).add(1) == 2 {
                let second = xv.split_off(*INTEGER(dim) as usize);
                second
            } else {
                let y = xv;
                xv = (1..=y.len()).map(|i| i as f64).collect();
                y
            }
        } else {
            values(y)
        };
        if xv.len() != yv.len() {
            base_error("'x' and 'y' lengths differ");
        }
        (xv, yv)
    }
}
fn renderer() -> *mut dyn DrawTarget {
    with_required_current_instance(|inst| unsafe { (*inst).current_renderplot_backend })
        .unwrap_or_else(|| base_error("plot requires an active graphics device"))
}
fn current() -> Coordinates {
    with_required_current_instance(|inst| unsafe { (*inst).portable_graphics.current })
        .unwrap_or_else(|| base_error("plot.new has not been called yet"))
}
fn install(coords: Coordinates) {
    with_required_current_instance(|inst| unsafe {
        (*inst).portable_graphics.current = Some(coords);
    });
    set_plot_parameter("usr", ParValue::Real(coords.limits.to_vec()));
    set_plot_parameter("xlog", ParValue::Logical(vec![i32::from(coords.log[0])]));
    set_plot_parameter("ylog", ParValue::Logical(vec![i32::from(coords.log[1])]));
}
unsafe fn coordinates(args: SEXP, x: &[f64], y: &[f64], new: bool) -> (Coordinates, bool) {
    unsafe {
        let log = label(args, "log").unwrap_or_default();
        if !log.chars().all(|c| c == 'x' || c == 'y') {
            base_error("invalid 'log' specification");
        }
        let logs = [log.contains('x'), log.contains('y')];
        let mut limits = [0.; 4];
        for (axis, (name, v)) in [("xlim", x), ("ylim", y)].into_iter().enumerate() {
            let explicit = values(arg(args, name));
            let (mut lo, mut hi) = if explicit.is_empty() {
                let finite: Vec<_> = v
                    .iter()
                    .copied()
                    .filter(|v| v.is_finite() && (!logs[axis] || *v > 0.))
                    .collect();
                (
                    finite.iter().copied().fold(f64::INFINITY, f64::min),
                    finite.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                )
            } else {
                if explicit.len() != 2 {
                    base_error(format!("invalid '{name}' value"));
                }
                (explicit[0], explicit[1])
            };
            if logs[axis] {
                lo = lo.log10();
                hi = hi.log10();
            }
            if !lo.is_finite() || !hi.is_finite() {
                base_error(format!("need finite '{name}' values"));
            }
            if lo == hi {
                let delta = if lo == 0. { 1. } else { lo.abs() * 0.4 };
                lo -= delta;
                hi += delta;
            }
            let axs = label(args, if axis == 0 { "xaxs" } else { "yaxs" }).unwrap_or_else(|| {
                match parameter(if axis == 0 { "xaxs" } else { "yaxs" }) {
                    ParValue::String(s) => s,
                    _ => "r".into(),
                }
            });
            if axs == "r" {
                let extra = (hi - lo) * 0.04;
                lo -= extra;
                hi += extra;
            } else if axs != "i" {
                base_error("only axis styles 'r' and 'i' are supported");
            }
            limits[axis * 2] = lo;
            limits[axis * 2 + 1] = hi;
        }
        if !new {
            let existing = current();
            return (
                Coordinates {
                    limits,
                    rect: existing.rect,
                    log: logs,
                },
                false,
            );
        }
        let (w, h) = (&*renderer()).dimensions();
        let layout = par_numbers("mfrow");
        let layout = [
            layout.first().copied().unwrap_or(1.).max(1.) as usize,
            layout.get(1).copied().unwrap_or(1.).max(1.) as usize,
        ];
        let panels = layout[0]
            .checked_mul(layout[1])
            .unwrap_or_else(|| base_error("graphics layout is too large"));
        let index = with_required_current_instance(|inst| {
            let state = &mut (*inst).portable_graphics;
            if state.layout != layout {
                state.layout = layout;
                state.next = 0;
            }
            let index = if new {
                let i = state.next;
                state.next = (state.next + 1) % panels;
                i
            } else {
                state.next.saturating_sub(1)
            };
            index
        });
        let pw = w as f32 / layout[1] as f32;
        let ph = h as f32 / layout[0] as f32;
        let ox = (index % layout[1]) as f32 * pw;
        let oy = (index / layout[1]) as f32 * ph;
        let left = ox + 58.;
        let right = (ox + pw - 24.).max(left + 1.);
        let top = oy
            + if label(args, "main").is_some() {
                48.
            } else {
                34.
            };
        let bottom = (oy + ph - 62.).max(top + 1.);
        (
            Coordinates {
                limits,
                rect: [left, top, right, bottom],
                log: logs,
            },
            new && index == 0,
        )
    }
}
fn line(target: &mut dyn DrawTarget, a: Point, b: Point, stroke: Stroke) {
    if [a.x, a.y, b.x, b.y].iter().any(|v| !v.is_finite()) {
        return;
    }
    target.draw_path(&Path {
        commands: vec![PathCommand::MoveTo(a.x, a.y), PathCommand::LineTo(b.x, b.y)],
        fill: transparent(),
        stroke,
        anti_alias: true,
    });
}
fn point(target: &mut dyn DrawTarget, p: Point, style: &Style, i: usize) {
    if !p.x.is_finite() || !p.y.is_finite() {
        return;
    }
    let symbol = style.symbols[i % style.symbols.len()];
    let r = style.size;
    let color = style.color(i);
    let bg = style.background[i % style.background.len()];
    let stroke = Stroke::new(style.width, color);
    if symbol < 0 || symbol >= 32 {
        let ch = if symbol < 0 {
            symbol.checked_neg().and_then(|c| char::from_u32(c as u32))
        } else {
            char::from_u32(symbol as u32)
        };
        if let Some(ch) = ch {
            target.draw_text(
                &ch.to_string(),
                p,
                &PlotParameters {
                    font_size: r * 4.,
                    text_color: color,
                    text_anchor: TextAnchor::Middle,
                    ..Default::default()
                },
            );
        }
        return;
    }
    let fill = if (15..=20).contains(&symbol) {
        color
    } else if symbol >= 21 {
        bg
    } else {
        transparent()
    };
    let shape = match symbol {
        0 | 7 | 12 | 14 | 15 | 22 => 0,
        2 | 11 | 17 | 24 => 2,
        5 | 9 | 18 | 23 => 5,
        6 | 25 => 6,
        3 | 4 | 8 => -1,
        _ => 1,
    };
    if shape >= 0 {
        let mut path = match shape {
            0 => Path::rect(p.x - r, p.y - r, r * 2., r * 2.),
            2 | 6 => {
                let sign = if shape == 6 { -1. } else { 1. };
                Path {
                    commands: vec![
                        PathCommand::MoveTo(p.x, p.y - sign * r),
                        PathCommand::LineTo(p.x + r, p.y + sign * r),
                        PathCommand::LineTo(p.x - r, p.y + sign * r),
                        PathCommand::Close,
                    ],
                    ..Default::default()
                }
            }
            5 => Path {
                commands: vec![
                    PathCommand::MoveTo(p.x, p.y - r),
                    PathCommand::LineTo(p.x + r, p.y),
                    PathCommand::LineTo(p.x, p.y + r),
                    PathCommand::LineTo(p.x - r, p.y),
                    PathCommand::Close,
                ],
                ..Default::default()
            },
            _ => Path::circle(p.x, p.y, if symbol == 20 { r * 0.6 } else { r }),
        };
        path.fill = fill;
        path.stroke = stroke.clone();
        path.anti_alias = true;
        target.draw_path(&path);
    }
    if matches!(symbol, 3 | 7 | 8 | 9 | 10 | 12) {
        line(
            target,
            Point { x: p.x - r, y: p.y },
            Point { x: p.x + r, y: p.y },
            stroke.clone(),
        );
        line(
            target,
            Point { x: p.x, y: p.y - r },
            Point { x: p.x, y: p.y + r },
            stroke.clone(),
        );
    }
    if matches!(symbol, 4 | 8 | 13) {
        line(
            target,
            Point {
                x: p.x - r,
                y: p.y - r,
            },
            Point {
                x: p.x + r,
                y: p.y + r,
            },
            stroke.clone(),
        );
        line(
            target,
            Point {
                x: p.x - r,
                y: p.y + r,
            },
            Point {
                x: p.x + r,
                y: p.y - r,
            },
            stroke,
        );
    }
}
fn draw_xy(
    target: &mut dyn DrawTarget,
    coords: Coordinates,
    x: &[f64],
    y: &[f64],
    kind: &str,
    style: &Style,
) {
    target.set_clip(Some(coords.rect));
    let mut previous: Option<Point> = None;
    for (i, (x, y)) in x.iter().zip(y).enumerate() {
        let p = coords.map(*x, *y);
        if !p.x.is_finite() || !p.y.is_finite() {
            previous = None;
            continue;
        }
        if kind == "h" {
            line(target, coords.map(*x, 0.), p, style.stroke(0));
        } else if let Some(mut old) = previous {
            if matches!(kind, "l" | "o" | "b" | "c" | "s" | "S") {
                let mut end = p;
                if kind == "s" {
                    let mid = Point { x: p.x, y: old.y };
                    line(target, old, mid, style.stroke(0));
                    old = mid;
                }
                if kind == "S" {
                    let mid = Point { x: old.x, y: p.y };
                    line(target, old, mid, style.stroke(0));
                    old = mid;
                }
                if kind == "b" || kind == "c" {
                    let dx = p.x - old.x;
                    let dy = p.y - old.y;
                    let len = dx.hypot(dy);
                    if len > style.size * 2. {
                        let t = style.size / len;
                        old = Point {
                            x: old.x + dx * t,
                            y: old.y + dy * t,
                        };
                        end = Point {
                            x: p.x - dx * t,
                            y: p.y - dy * t,
                        };
                    } else {
                        previous = Some(p);
                        if kind == "b" {
                            point(target, p, style, i);
                        }
                        continue;
                    }
                }
                line(target, old, end, style.stroke(0));
            }
        }
        if matches!(kind, "p" | "b" | "o") {
            point(target, p, style, i);
        }
        previous = Some(p);
    }
    target.set_clip(None);
}
fn axis(
    target: &mut dyn DrawTarget,
    c: Coordinates,
    side: usize,
    at: &[f64],
    labels: &[String],
    color: Color,
) {
    let horizontal = side == 1 || side == 3;
    let coord = if horizontal { 0 } else { 1 };
    let positions: Vec<_> = if at.is_empty() {
        (0..5)
            .map(|i| {
                c.raw(
                    coord,
                    c.limits[coord * 2]
                        + (c.limits[coord * 2 + 1] - c.limits[coord * 2]) * i as f64 / 4.,
                )
            })
            .collect()
    } else {
        at.to_vec()
    };
    let (a, b) = if horizontal {
        (
            Point {
                x: c.rect[0],
                y: if side == 1 { c.rect[3] } else { c.rect[1] },
            },
            Point {
                x: c.rect[2],
                y: if side == 1 { c.rect[3] } else { c.rect[1] },
            },
        )
    } else {
        (
            Point {
                x: if side == 2 { c.rect[0] } else { c.rect[2] },
                y: c.rect[1],
            },
            Point {
                x: if side == 2 { c.rect[0] } else { c.rect[2] },
                y: c.rect[3],
            },
        )
    };
    line(target, a, b, Stroke::new(1., color));
    for (i, v) in positions.iter().enumerate() {
        let p = c.map(
            if horizontal {
                *v
            } else {
                c.raw(0, c.limits[0])
            },
            if horizontal {
                c.raw(1, c.limits[2])
            } else {
                *v
            },
        );
        let mut tick = if horizontal {
            Point { x: p.x, y: a.y }
        } else {
            Point { x: a.x, y: p.y }
        };
        let start = tick;
        if horizontal {
            tick.y += if side == 1 { 5. } else { -5. };
        } else {
            tick.x += if side == 2 { -5. } else { 5. };
        }
        line(target, start, tick, Stroke::new(1., color));
        let label = if labels.is_empty() {
            format!("{v:.3}")
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_owned()
        } else {
            labels[i % labels.len()].clone()
        };
        let pos = if horizontal {
            Point {
                x: p.x,
                y: tick.y + if side == 1 { 16. } else { -5. },
            }
        } else {
            Point {
                x: tick.x + if side == 2 { -5. } else { 5. },
                y: p.y + 4.,
            }
        };
        target.draw_text(
            &label,
            pos,
            &PlotParameters {
                font_size: 11.,
                text_color: color,
                text_anchor: if horizontal {
                    TextAnchor::Middle
                } else if side == 2 {
                    TextAnchor::End
                } else {
                    TextAnchor::Start
                },
                ..Default::default()
            },
        );
    }
}
fn box_path(target: &mut dyn DrawTarget, c: Coordinates, color: Color) {
    let mut path = Path::rect(
        c.rect[0],
        c.rect[1],
        c.rect[2] - c.rect[0],
        c.rect[3] - c.rect[1],
    );
    path.fill = transparent();
    path.stroke = Stroke::new(1., color);
    target.draw_path(&path);
}
unsafe fn titles(target: &mut dyn DrawTarget, c: Coordinates, args: SEXP) {
    unsafe {
        for (name, position, angle, size) in [
            (
                "main",
                Point {
                    x: (c.rect[0] + c.rect[2]) / 2.,
                    y: c.rect[1] - 18.,
                },
                0.,
                16.,
            ),
            (
                "xlab",
                Point {
                    x: (c.rect[0] + c.rect[2]) / 2.,
                    y: c.rect[3] + 45.,
                },
                0.,
                12.,
            ),
            (
                "ylab",
                Point {
                    x: c.rect[0] - 44.,
                    y: (c.rect[1] + c.rect[3]) / 2.,
                },
                90.,
                12.,
            ),
            (
                "sub",
                Point {
                    x: (c.rect[0] + c.rect[2]) / 2.,
                    y: c.rect[3] + 59.,
                },
                0.,
                11.,
            ),
        ] {
            if let Some(text) = label(args, name) {
                target.draw_text(
                    &text,
                    position,
                    &PlotParameters {
                        font_size: size,
                        text_color: Color::BLACK,
                        text_angle: angle,
                        text_anchor: TextAnchor::Middle,
                        ..Default::default()
                    },
                );
            }
        }
    }
}
fn invisible() -> SEXP {
    crate::sexp::globals::set_R_Visible(0);
    unsafe { R_NilValue() }
}

pub(crate) unsafe fn plot_default(_: SEXP, _: SEXP, args: SEXP, _: SEXP) -> SEXP {
    unsafe {
        let (x, y) = xy(args);
        let style = style(args);
        let kind = label(args, "type").unwrap_or_else(|| "p".into());
        if !["p", "l", "b", "c", "o", "h", "s", "S", "n"].contains(&kind.as_str()) {
            base_error("invalid plot type");
        }
        let (c, clear) = coordinates(args, &x, &y, true);
        install(c);
        let axes = scalar(args, "axes", 1.) != 0.;
        let frame = scalar(args, "frame.plot", if axes { 1. } else { 0. }) != 0.;
        let target = &mut *renderer();
        if clear {
            target.clear(Color::WHITE);
        }
        if axes {
            axis(target, c, 1, &[], &[], Color::BLACK);
            axis(target, c, 2, &[], &[], Color::BLACK);
        }
        if frame {
            box_path(target, c, Color::BLACK);
        }
        draw_xy(target, c, &x, &y, &kind, &style);
        titles(target, c, args);
        invisible()
    }
}

pub(crate) unsafe fn draw_builtin(name: &str, args: SEXP) -> SEXP {
    unsafe {
        if name == "plot.new" {
            let (c, clear) = coordinates(args, &[0., 1.], &[0., 1.], true);
            install(c);
            let target = &mut *renderer();
            target.set_clip(None);
            if clear {
                target.clear(Color::WHITE);
            }
            return invisible();
        }
        if name == "plot.window" {
            let (c, _) = coordinates(args, &[], &[], false);
            install(c);
            return invisible();
        }
        let c = current();
        let style = style(args);
        match name {
            "lines" | "lines.default" | "points" | "points.default" => {
                let (x, y) = xy(args);
                let kind = label(args, "type").unwrap_or_else(|| {
                    if name.starts_with("lines") {
                        "l".into()
                    } else {
                        "p".into()
                    }
                });
                if !["p", "l", "b", "c", "o", "h", "s", "S", "n"].contains(&kind.as_str()) {
                    base_error("invalid plot type");
                }
                draw_xy(&mut *renderer(), c, &x, &y, &kind, &style);
            }
            "segments" | "arrows" => {
                let a = bindings(args, &["x0", "y0", "x1", "y1"]);
                let cols: Vec<_> = a.iter().map(|v| values(*v)).collect();
                let n = cols.iter().map(Vec::len).max().unwrap_or(0);
                if cols.iter().any(Vec::is_empty) {
                    return invisible();
                }
                let length = scalar(args, "length", 0.25) as f32 * 96.;
                let angle = scalar(args, "angle", 30.).to_radians() as f32;
                let code = scalar(args, "code", 2.) as i32;
                let target = &mut *renderer();
                target.set_clip(Some(c.rect));
                for i in 0..n {
                    let p = c.map(cols[0][i % cols[0].len()], cols[1][i % cols[1].len()]);
                    let q = c.map(cols[2][i % cols[2].len()], cols[3][i % cols[3].len()]);
                    line(target, p, q, style.stroke(i));
                    if name == "arrows" {
                        for (a, b, mask) in [(p, q, 1), (q, p, 2)] {
                            if code & mask != 0 {
                                let theta = (b.y - a.y).atan2(b.x - a.x);
                                for sign in [-1., 1.] {
                                    let t = theta + sign * angle;
                                    line(
                                        target,
                                        a,
                                        Point {
                                            x: a.x + length * t.cos(),
                                            y: a.y + length * t.sin(),
                                        },
                                        style.stroke(i),
                                    );
                                }
                            }
                        }
                    }
                }
                target.set_clip(None);
            }
            "abline" => {
                let a = bindings(args, &["a", "b"]);
                let av = values(a[0]);
                let bv = values(a[1]);
                let h = values(arg(args, "h"));
                let v = values(arg(args, "v"));
                let target = &mut *renderer();
                target.set_clip(Some(c.rect));
                if !av.is_empty() {
                    let b = if !bv.is_empty() {
                        bv[0]
                    } else if av.len() >= 2 {
                        av[1]
                    } else {
                        base_error("'a' and 'b' must be specified");
                    };
                    let x0 = c.raw(0, c.limits[0]);
                    let x1 = c.raw(0, c.limits[1]);
                    line(
                        target,
                        c.map(x0, av[0] + b * x0),
                        c.map(x1, av[0] + b * x1),
                        style.stroke(0),
                    );
                }
                for (i, y) in h.iter().enumerate() {
                    line(
                        target,
                        c.map(c.raw(0, c.limits[0]), *y),
                        c.map(c.raw(0, c.limits[1]), *y),
                        style.stroke(i),
                    );
                }
                for (i, x) in v.iter().enumerate() {
                    line(
                        target,
                        c.map(*x, c.raw(1, c.limits[2])),
                        c.map(*x, c.raw(1, c.limits[3])),
                        style.stroke(i),
                    );
                }
                target.set_clip(None);
            }
            "rect" => {
                let a = bindings(args, &["xleft", "ybottom", "xright", "ytop"]);
                let cols: Vec<_> = a.iter().map(|v| values(*v)).collect();
                if cols.iter().any(Vec::is_empty) {
                    return invisible();
                }
                let fill = colors(arg(args, "col"), transparent());
                let border = colors(arg(args, "border"), Color::BLACK);
                let n = cols.iter().map(Vec::len).max().unwrap();
                let target = &mut *renderer();
                target.set_clip(Some(c.rect));
                for i in 0..n {
                    let a = c.map(cols[0][i % cols[0].len()], cols[1][i % cols[1].len()]);
                    let b = c.map(cols[2][i % cols[2].len()], cols[3][i % cols[3].len()]);
                    let mut path = Path::rect(
                        a.x.min(b.x),
                        a.y.min(b.y),
                        (b.x - a.x).abs(),
                        (b.y - a.y).abs(),
                    );
                    path.fill = fill[i % fill.len()];
                    path.stroke = Stroke::new(style.width, border[i % border.len()]);
                    target.draw_path(&path);
                }
                target.set_clip(None);
            }
            "polygon" => {
                let (x, y) = xy(args);
                let fill = colors(arg(args, "col"), transparent());
                let border = colors(arg(args, "border"), Color::BLACK);
                let target = &mut *renderer();
                target.set_clip(Some(c.rect));
                let mut commands = vec![];
                let mut polygon = 0;
                for (i, (x, y)) in x
                    .iter()
                    .zip(&y)
                    .chain(std::iter::once((&f64::NAN, &f64::NAN)))
                    .enumerate()
                {
                    let p = c.map(*x, *y);
                    if p.x.is_finite() && p.y.is_finite() {
                        commands.push(if commands.is_empty() {
                            PathCommand::MoveTo(p.x, p.y)
                        } else {
                            PathCommand::LineTo(p.x, p.y)
                        });
                    } else if !commands.is_empty() {
                        commands.push(PathCommand::Close);
                        target.draw_path(&Path {
                            commands: std::mem::take(&mut commands),
                            fill: fill[polygon % fill.len()],
                            stroke: Stroke::new(style.width, border[polygon % border.len()]),
                            anti_alias: true,
                        });
                        polygon += 1;
                    }
                    let _ = i;
                }
                target.set_clip(None);
            }
            "text" | "text.default" => {
                let a = bindings(args, &["x", "y", "labels"]);
                let x = values(a[0]);
                let y = values(a[1]);
                let labels = a[2];
                if x.is_empty() || y.is_empty() || labels == R_NilValue() {
                    return invisible();
                }
                let text: Vec<_> = (0..XLENGTH(labels))
                    .map(|i| elt_to_string(labels, i))
                    .collect();
                if text.is_empty() {
                    return invisible();
                }
                let angle = scalar(args, "srt", 0.) as f32;
                let target = &mut *renderer();
                target.set_clip(Some(c.rect));
                for i in 0..x.len().max(y.len()) {
                    target.draw_text(
                        &text[i % text.len()],
                        c.map(x[i % x.len()], y[i % y.len()]),
                        &PlotParameters {
                            font_size: style.size * 4.,
                            text_color: style.color(i),
                            text_angle: angle,
                            text_anchor: TextAnchor::Middle,
                            ..Default::default()
                        },
                    );
                }
                target.set_clip(None);
            }
            "title" => titles(&mut *renderer(), c, args),
            "box" => box_path(&mut *renderer(), c, style.color(0)),
            "axis" => {
                let a = bindings(args, &["side", "at", "labels"]);
                let side = values(a[0]);
                if side.len() != 1 || !(1. ..=4.).contains(&side[0]) {
                    base_error("invalid axis side");
                }
                let at = values(a[1]);
                let labels = if a[2] != R_NilValue() && TYPEOF(a[2]) == SEXPTYPE::STRSXP {
                    (0..XLENGTH(a[2])).map(|i| elt_to_string(a[2], i)).collect()
                } else {
                    vec![]
                };
                axis(
                    &mut *renderer(),
                    c,
                    side[0] as usize,
                    &at,
                    &labels,
                    style.color(0),
                );
            }
            _ => base_error(format!("graphics primitive '{name}' is not implemented")),
        }
        invisible()
    }
}
