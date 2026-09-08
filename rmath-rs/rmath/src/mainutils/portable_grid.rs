//! Portable grid frontend. Runtime values are decoded before borrowing a device;
//! viewport transforms and graphical parameters are owned session state.
use crate::eval::attrib_core::{R_NamesSymbol, getAttrib};
use crate::mainutils::essentials::{arg_by_name_or_position, base_error, elt_to_string};
use crate::sexp::{
    accessors::*,
    constructors::*,
    ffi::{SEXP, SEXPTYPE},
    globals::R_NilValue,
    instance::with_required_current_instance,
};
use r_graphics_engine::{
    Color, DashPattern, DrawTarget, Path, PathCommand, PlotParameters, Point, Stroke, TextAnchor,
};

// Unit computations keep R allocations rooted and numeric work in owned vectors.
// Mixed dimensions need a deferred unit expression tree; reject them until that
// representation exists rather than silently interpreting everything as npc.
unsafe fn unit_kind(x: SEXP) -> String {
    unsafe {
        let u = field(x, "units");
        let name = string(u, "");
        if name.is_empty() || (0..XLENGTH(u)).any(|i| elt_to_string(u, i) != name) {
            base_error("arithmetic on mixed grid units is not supported");
        }
        name
    }
}
unsafe fn unit_values(x: SEXP) -> Vec<f64> {
    unsafe {
        match SEXPTYPE(TYPEOF(x)) {
            SEXPTYPE::REALSXP => (0..XLENGTH(x)).map(|i| *REAL(x).add(i as usize)).collect(),
            SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => (0..XLENGTH(x))
                .map(|i| {
                    let n = if TYPEOF(x) == SEXPTYPE::INTSXP {
                        *INTEGER(x).add(i as usize)
                    } else {
                        *LOGICAL(x).add(i as usize)
                    };
                    if n == i32::MIN {
                        crate::sexp::ffi::NA_REAL
                    } else {
                        f64::from(n)
                    }
                })
                .collect(),
            _ => base_error("unit arithmetic requires numeric values"),
        }
    }
}
unsafe fn unit_result(template: SEXP, values: &[f64]) -> SEXP {
    unsafe {
        use crate::sexp::protect::protect;
        let _template = protect(template);
        let value = result(values);
        let _value = protect(value);
        let obj = crate::mainutils::seq::Rf_shallow_duplicate(template);
        let _obj = protect(obj);
        // Preserve attributes, unit names and string/grob data while replacing
        // only the owned numeric coefficient vector.
        let names = getAttrib(obj, R_NamesSymbol());
        for i in 0..XLENGTH(obj).min(XLENGTH(names)) {
            if elt_to_string(names, i) == "value" {
                SET_VECTOR_ELT(obj, i, value);
                return obj;
            }
        }
        base_error("invalid grid unit: missing values")
    }
}
pub unsafe fn unit_binary(op: &str, a: SEXP, b: SEXP) -> Option<SEXP> {
    unsafe {
        let au = crate::mainutils::essentials::sexp_has_class(a, "unit");
        let bu = crate::mainutils::essentials::sexp_has_class(b, "unit");
        if !au && !bu {
            return None;
        }
        let unary = b == R_NilValue();
        match op {
            "+" | "-" if au && (bu || unary) => {}
            "*" if au != bu && !unary => {}
            "/" if au && !bu && !unary => {}
            _ => base_error("invalid unit arithmetic operands"),
        }
        let template = if au { a } else { b };
        let kind = unit_kind(template);
        if au && bu {
            if unit_kind(b) != kind {
                base_error("unit arithmetic requires matching units");
            }
            if field(a, "data") != R_NilValue() || field(b, "data") != R_NilValue() {
                base_error("combining data-dependent units is not supported");
            }
        }
        let av = unit_values(if au { field(a, "value") } else { a });
        let out = if unary {
            av.into_iter()
                .map(|v| if op == "-" { -v } else { v })
                .collect()
        } else {
            let bv = unit_values(if bu { field(b, "value") } else { b });
            let n = if av.is_empty() || bv.is_empty() {
                0
            } else {
                av.len().max(bv.len())
            };
            (0..n)
                .map(|i| {
                    let x = av[i % av.len()];
                    let y = bv[i % bv.len()];
                    match op {
                        "+" => x + y,
                        "-" => x - y,
                        "*" => x * y,
                        "/" => x / y,
                        _ => unreachable!(),
                    }
                })
                .collect::<Vec<_>>()
        };
        Some(unit_result(template, &out))
    }
}
pub unsafe fn unit_summary(op: &str, args: SEXP) -> Option<SEXP> {
    unsafe {
        let na_tag = crate::sexp::symbol::Rf_install(c"na.rm".as_ptr());
        let mut p = args;
        let mut template = R_NilValue();
        while p != R_NilValue() {
            if TAG(p) != na_tag && crate::mainutils::essentials::sexp_has_class(CAR(p), "unit") {
                template = CAR(p);
                break;
            }
            p = CDR(p);
        }
        if template == R_NilValue() {
            return None;
        }
        if !matches!(op, "sum" | "min" | "max") {
            base_error("unit summary is not supported");
        }
        let kind = unit_kind(template);
        let mut values = Vec::new();
        p = args;
        while p != R_NilValue() {
            if TAG(p) != na_tag {
                let x = CAR(p);
                if !crate::mainutils::essentials::sexp_has_class(x, "unit") || unit_kind(x) != kind
                {
                    base_error("unit summary requires matching units");
                }
                if field(x, "data") != R_NilValue() {
                    base_error("summary of data-dependent units is not supported");
                }
                values.extend(unit_values(field(x, "value")));
            }
            p = CDR(p);
        }
        // GNU R grid propagates missing coefficients even with na.rm=TRUE.
        let v = if values
            .iter()
            .any(|v| v.to_bits() == crate::sexp::ffi::NA_REAL.to_bits())
        {
            crate::sexp::ffi::NA_REAL
        } else if values.iter().any(|v| v.is_nan()) {
            f64::NAN
        } else {
            match op {
                "sum" => values.iter().sum(),
                "min" => values.iter().copied().fold(f64::INFINITY, f64::min),
                "max" => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                _ => unreachable!(),
            }
        };
        Some(unit_result(template, &[v]))
    }
}

const DPI: f64 = 96.;
const CLEAR: Color = Color {
    r: 0,
    g: 0,
    b: 0,
    a: 0,
};
#[derive(Clone)]
struct Gp {
    col: Vec<Color>,
    fill: Vec<Color>,
    width: f64,
    size: f64,
    alpha: f64,
    face: r_graphics_engine::FontFace,
    dash: Option<DashPattern>,
}
impl Default for Gp {
    fn default() -> Self {
        Self {
            col: vec![Color::BLACK],
            fill: vec![CLEAR],
            width: 1.,
            size: 12. * DPI / 72.,
            alpha: 1.,
            face: r_graphics_engine::FontFace::Plain,
            dash: None,
        }
    }
}
#[derive(Clone)]
struct Frame {
    width: f64,
    height: f64,
    matrix: [f64; 6],
    scale: [[f64; 2]; 2],
    clip: Option<[f32; 4]>,
    gp: Gp,
    layout: Option<Layout>,
    name: Option<String>,
}
#[derive(Clone)]
struct Layout {
    widths: Vec<f64>,
    heights: Vec<f64>,
    respect: bool,
    offset_x: f64,
    offset_y: f64,
}
#[derive(Clone, Default)]
pub(crate) struct GridState {
    frames: Vec<Frame>,
    dimensions: (u32, u32),
}
impl GridState {
    fn ensure(&mut self, dims: (u32, u32)) {
        if self.frames.is_empty() || self.dimensions != dims {
            self.dimensions = dims;
            self.frames = vec![Frame {
                width: dims.0 as f64,
                height: dims.1 as f64,
                matrix: [1., 0., 0., -1., 0., dims.1 as f64],
                scale: [[0., 1.], [0., 1.]],
                clip: None,
                gp: Gp::default(),
                layout: None,
                name: None,
            }];
        }
    }
}
impl Frame {
    fn map(&self, x: f64, y: f64) -> Point {
        let m = self.matrix;
        let point = Point {
            x: (m[0] * x + m[2] * y + m[4]) as f32,
            y: (m[1] * x + m[3] * y + m[5]) as f32,
        };
        if !point.x.is_finite() || !point.y.is_finite() {
            base_error("grid coordinate exceeds device range");
        }
        point
    }
    fn extent(&self, axis: usize) -> f64 {
        if axis == 0 { self.width } else { self.height }
    }
    fn unit_factor(&self, u: &str, axis: usize, dimension: bool) -> Result<(f64, f64), String> {
        let extent = self.extent(axis);
        let scale = self.scale[axis];
        Ok(match u {
            "npc" => (extent, 0.),
            "snpc" => (self.width.min(self.height), 0.),
            "native" => (
                extent / (scale[1] - scale[0]),
                if dimension {
                    0.
                } else {
                    -scale[0] * extent / (scale[1] - scale[0])
                },
            ),
            "inches" => (DPI, 0.),
            "cm" => (DPI / 2.54, 0.),
            "mm" => (DPI / 25.4, 0.),
            "points" => (DPI / 72.27, 0.),
            "bigpts" => (DPI / 72., 0.),
            "picas" => (DPI * 12. / 72.27, 0.),
            "dida" => (DPI / 72.27 * 1157. / 1238., 0.),
            "cicero" => (DPI / 72.27 * 1157. / 1238. * 12., 0.),
            "scaledpts" => (DPI / 72.27 / 65536., 0.),
            "lines" => (self.gp.size * 1.2, 0.),
            "char" => (self.gp.size, 0.),
            "strwidth" | "strheight" => (self.gp.size * 0.6, 0.),
            _ => return Err(format!("grid unit '{u}' is not supported")),
        })
    }
}
fn device() -> *mut dyn DrawTarget {
    with_required_current_instance(|p| unsafe { (*p).current_renderplot_backend })
        .unwrap_or_else(|| base_error("grid requires an active graphics device"))
}
unsafe fn field(x: SEXP, name: &str) -> SEXP {
    unsafe {
        if x == R_NilValue() {
            return R_NilValue();
        }
        if TYPEOF(x) != SEXPTYPE::VECSXP {
            base_error("invalid grid object");
        }
        let names = getAttrib(x, R_NamesSymbol());
        if TYPEOF(names) != SEXPTYPE::STRSXP {
            return R_NilValue();
        }
        for i in 0..XLENGTH(x).min(XLENGTH(names)) {
            if elt_to_string(names, i) == name {
                return VECTOR_ELT(x, i);
            }
        }
        R_NilValue()
    }
}
unsafe fn numbers(x: SEXP) -> Vec<f64> {
    unsafe {
        if x == R_NilValue() {
            return vec![];
        }
        if !matches!(
            SEXPTYPE(TYPEOF(x)),
            SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP
        ) {
            base_error("grid expects numeric coordinates");
        }
        (0..XLENGTH(x))
            .map(|i| {
                let v = if TYPEOF(x) == SEXPTYPE::REALSXP {
                    *REAL(x).add(i as usize)
                } else {
                    let n = if TYPEOF(x) == SEXPTYPE::INTSXP {
                        *INTEGER(x).add(i as usize)
                    } else {
                        *LOGICAL(x).add(i as usize)
                    };
                    if n == i32::MIN { f64::NAN } else { n as f64 }
                };
                if !v.is_finite() {
                    base_error("grid coordinates must be finite");
                }
                v
            })
            .collect()
    }
}
unsafe fn num(x: SEXP, default: f64) -> f64 {
    unsafe {
        let v = numbers(x);
        if v.is_empty() {
            default
        } else if v.len() == 1 {
            v[0]
        } else {
            base_error("grid expected one numeric value")
        }
    }
}
unsafe fn string(x: SEXP, default: &str) -> String {
    unsafe {
        if x == R_NilValue() || XLENGTH(x) == 0 {
            default.into()
        } else {
            elt_to_string(x, 0)
        }
    }
}
unsafe fn result(v: &[f64]) -> SEXP {
    unsafe {
        let x = Rf_allocVector3(SEXPTYPE::REALSXP, v.len() as i64);
        for (i, value) in v.iter().enumerate() {
            *REAL(x).add(i) = *value;
        }
        x
    }
}
unsafe fn units(x: SEXP, default: &str, frame: &Frame, axis: usize, dimension: bool) -> Vec<f64> {
    unsafe {
        let (values, names, data) = if TYPEOF(x) == SEXPTYPE::VECSXP {
            (
                numbers(field(x, "value")),
                field(x, "units"),
                field(x, "data"),
            )
        } else {
            (numbers(x), R_NilValue(), R_NilValue())
        };
        if values.is_empty() {
            base_error("grid unit must have positive length");
        }
        values
            .into_iter()
            .enumerate()
            .map(|(i, v)| {
                let name = if names == R_NilValue() {
                    default.into()
                } else {
                    if TYPEOF(names) != SEXPTYPE::STRSXP || XLENGTH(names) == 0 {
                        base_error("invalid grid unit names");
                    }
                    elt_to_string(names, (i as i64) % XLENGTH(names))
                };
                let mut v = v;
                if name == "strwidth" || name == "strheight" {
                    if data == R_NilValue() {
                        base_error("data is required for stringWidth/stringHeight units");
                    }
                    let text = if TYPEOF(data) == SEXPTYPE::STRSXP {
                        elt_to_string(data, (i as i64) % XLENGTH(data))
                    } else {
                        base_error("string units require character data")
                    };
                    let metrics = r_graphics_engine::default_font_book().measure_text(
                        &text,
                        frame.gp.size as f32,
                        frame.gp.face,
                    );
                    v *= if name == "strwidth" {
                        metrics.width as f64
                    } else {
                        (metrics.ascent + metrics.descent) as f64
                    };
                }
                let (f, o) = frame
                    .unit_factor(&name, axis, dimension)
                    .unwrap_or_else(|e| base_error(e));
                let resolved = v * f + o;
                if !resolved.is_finite() {
                    base_error("grid unit conversion overflow");
                }
                resolved
            })
            .collect()
    }
}
unsafe fn color_values(x: SEXP, old: &[Color]) -> Vec<Color> {
    unsafe {
        if x == R_NilValue() {
            return old.to_vec();
        }
        if XLENGTH(x) == 0 {
            base_error("grid color cannot be empty");
        }
        if !matches!(
            SEXPTYPE(TYPEOF(x)),
            SEXPTYPE::STRSXP | SEXPTYPE::INTSXP | SEXPTYPE::REALSXP | SEXPTYPE::LGLSXP
        ) {
            base_error("invalid grid color");
        }
        (0..XLENGTH(x))
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
unsafe fn gp(x: SEXP, old: &Gp) -> Gp {
    unsafe {
        let mut p = old.clone();
        if x == R_NilValue() {
            return p;
        }
        let names = getAttrib(x, R_NamesSymbol());
        for i in 0..XLENGTH(names) {
            let name = elt_to_string(names, i);
            if !matches!(
                name.as_str(),
                "col"
                    | "fill"
                    | "lwd"
                    | "fontsize"
                    | "cex"
                    | "alpha"
                    | "fontface"
                    | "fontfamily"
                    | "lty"
                    | "lineheight"
            ) {
                base_error(format!("grid gpar '{name}' is not supported"));
            }
        }
        p.col = color_values(field(x, "col"), &p.col);
        p.fill = color_values(field(x, "fill"), &p.fill);
        p.width = num(field(x, "lwd"), p.width);
        p.size =
            num(field(x, "fontsize"), p.size * 72. / DPI) * DPI / 72. * num(field(x, "cex"), 1.);
        p.alpha *= num(field(x, "alpha"), 1.);
        if p.width < 0.
            || p.width > f32::MAX as f64
            || p.size <= 0.
            || p.size > f32::MAX as f64
            || !p.size.is_finite()
            || !(0. ..=1.).contains(&p.alpha)
        {
            base_error("invalid grid graphical parameters");
        }
        let face = string(field(x, "fontface"), "");
        if !face.is_empty() {
            p.face = match face.as_str() {
                "plain" | "1" => r_graphics_engine::FontFace::Plain,
                "bold" | "2" => r_graphics_engine::FontFace::Bold,
                "italic" | "3" => r_graphics_engine::FontFace::Italic,
                "bold.italic" | "4" => r_graphics_engine::FontFace::BoldItalic,
                _ => base_error("invalid grid fontface"),
            };
        }
        let family = string(field(x, "fontfamily"), "");
        if !family.is_empty() && family != "sans" {
            base_error("grid currently supports the device sans font only");
        }
        let lty = string(field(x, "lty"), "solid");
        p.dash = match lty.as_str() {
            "solid" | "1" => None,
            "dashed" | "2" => Some(DashPattern {
                intervals: vec![6., 4.],
                offset: 0.,
            }),
            "dotted" | "3" => Some(DashPattern {
                intervals: vec![1., 3.],
                offset: 0.,
            }),
            "dotdash" | "4" => Some(DashPattern {
                intervals: vec![1., 3., 6., 3.],
                offset: 0.,
            }),
            "longdash" | "5" => Some(DashPattern {
                intervals: vec![10., 4.],
                offset: 0.,
            }),
            "twodash" | "6" => Some(DashPattern {
                intervals: vec![8., 4., 2., 4.],
                offset: 0.,
            }),
            _ => base_error("invalid grid line type"),
        };
        if field(x, "lineheight") != R_NilValue() && num(field(x, "lineheight"), 1.2) != 1.2 {
            base_error("custom grid lineheight is not supported");
        }
        p
    }
}
fn alpha(mut c: Color, a: f64) -> Color {
    c.a = (c.a as f64 * a).round() as u8;
    c
}
fn style_path(frame: &Frame, commands: Vec<PathCommand>, i: usize, filled: bool) -> Path {
    Path {
        commands,
        fill: if filled {
            alpha(frame.gp.fill[i % frame.gp.fill.len()], frame.gp.alpha)
        } else {
            CLEAR
        },
        stroke: Stroke {
            dash_pattern: frame.gp.dash.clone(),
            ..Stroke::new(
                frame.gp.width as f32,
                alpha(frame.gp.col[i % frame.gp.col.len()], frame.gp.alpha),
            )
        },
        anti_alias: true,
    }
}
unsafe fn justification(x: SEXP) -> [f64; 2] {
    unsafe {
        if x == R_NilValue() {
            return [0.5, 0.5];
        }
        if TYPEOF(x) != SEXPTYPE::STRSXP {
            let v = numbers(x);
            return match v.as_slice() {
                [v] => [*v, *v],
                [x, y] => [*x, *y],
                _ => base_error("invalid grid justification"),
            };
        }
        let mut out = [0.5, 0.5];
        for i in 0..XLENGTH(x) {
            match elt_to_string(x, i).as_str() {
                "left" => out[0] = 0.,
                "right" => out[0] = 1.,
                "bottom" => out[1] = 0.,
                "top" => out[1] = 1.,
                "centre" | "center" => {}
                _ => base_error("invalid grid justification"),
            }
        }
        out
    }
}
fn compose(a: [f64; 6], b: [f64; 6]) -> [f64; 6] {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}
unsafe fn layout_lengths(x: SEXP, n: usize, frame: &Frame, axis: usize) -> Vec<f64> {
    unsafe {
        let values = numbers(field(x, "value"));
        let names = field(x, "units");
        if values.is_empty() || XLENGTH(names) == 0 {
            base_error("invalid layout units");
        }
        let mut out = vec![0.; n];
        let mut weights = vec![0.; n];
        for i in 0..n {
            let v = values[i % values.len()];
            if v < 0. {
                base_error("layout dimensions cannot be negative");
            }
            let name = elt_to_string(names, i as i64 % XLENGTH(names));
            if name == "null" {
                weights[i] = v;
            } else {
                out[i] = v * frame
                    .unit_factor(&name, axis, true)
                    .unwrap_or_else(|e| base_error(e))
                    .0;
            }
        }
        let available = (frame.extent(axis) - out.iter().sum::<f64>()).max(0.);
        let weight: f64 = weights.iter().sum();
        if weight > 0. {
            for i in 0..n {
                out[i] += weights[i] / weight * available;
            }
        }
        out
    }
}
unsafe fn push(data: SEXP, parent: &Frame) -> Frame {
    unsafe {
        let mut width = units(field(data, "width"), "npc", parent, 0, true)[0];
        let mut height = units(field(data, "height"), "npc", parent, 1, true)[0];
        let mut x = units(field(data, "x"), "npc", parent, 0, false)[0];
        let mut y = units(field(data, "y"), "npc", parent, 1, false)[0];
        let mut just = justification(field(data, "just"));
        let row = numbers(field(data, "row"));
        let col = numbers(field(data, "col"));
        if !row.is_empty() || !col.is_empty() {
            let layout = parent
                .layout
                .as_ref()
                .unwrap_or_else(|| base_error("viewport layout position requires parent layout"));
            let span = |v: &[f64], n: usize| -> (usize, usize) {
                if v.is_empty() {
                    return (0, n);
                }
                if v.len() > 2
                    || v.iter()
                        .any(|x| x.fract() != 0. || *x < 1. || *x > n as f64)
                {
                    base_error("invalid viewport layout position");
                }
                let a = v[0] as usize - 1;
                let b = *v.last().unwrap() as usize;
                if b <= a {
                    base_error("invalid viewport layout position");
                }
                (a, b)
            };
            let (r0, r1) = span(&row, layout.heights.len());
            let (c0, c1) = span(&col, layout.widths.len());
            x = layout.offset_x + layout.widths[..c0].iter().sum::<f64>();
            y = parent.height - layout.offset_y - layout.heights[..r1].iter().sum::<f64>();
            width = layout.widths[c0..c1].iter().sum();
            height = layout.heights[r0..r1].iter().sum();
            just = [0., 0.];
        }
        if width <= 0. || height <= 0. {
            base_error("viewport dimensions must be positive");
        }
        let angle = num(field(data, "angle"), 0.).to_radians();
        let (c, s) = (angle.cos(), angle.sin());
        let local = [
            c,
            s,
            -s,
            c,
            x - c * just[0] * width + s * just[1] * height,
            y - s * just[0] * width - c * just[1] * height,
        ];
        let mut f = Frame {
            width,
            height,
            matrix: compose(parent.matrix, local),
            scale: parent.scale,
            clip: parent.clip,
            gp: gp(field(data, "gp"), &parent.gp),
            layout: None,
            name: None,
        };
        if f.matrix.iter().any(|v| !v.is_finite()) {
            base_error("viewport transform overflow");
        }
        for (axis, name) in ["xscale", "yscale"].iter().enumerate() {
            let v = numbers(field(data, name));
            if v.len() != 2 || v[0] == v[1] || !(v[1] - v[0]).is_finite() {
                base_error("viewport scales require two different finite values");
            }
            f.scale[axis] = [v[0], v[1]];
        }
        match string(field(data, "clip"), "inherit").as_str() {
            "inherit" => {}
            "off" | "FALSE" => f.clip = None,
            "on" | "TRUE" => {
                if f.matrix[1].abs() > 1e-8 || f.matrix[2].abs() > 1e-8 {
                    base_error("clipping rotated grid viewports is not supported");
                }
                let a = f.map(0., 0.);
                let b = f.map(width, height);
                let mut r = [a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y)];
                if let Some(p) = parent.clip {
                    r = [
                        r[0].max(p[0]),
                        r[1].max(p[1]),
                        r[2].min(p[2]),
                        r[3].min(p[3]),
                    ];
                    r[2] = r[2].max(r[0]);
                    r[3] = r[3].max(r[1]);
                }
                f.clip = Some(r);
            }
            _ => base_error("invalid viewport clip"),
        }
        let layout = field(data, "layout");
        if layout != R_NilValue() {
            let rows = num(field(layout, "nrow"), 1.) as usize;
            let cols = num(field(layout, "ncol"), 1.) as usize;
            if rows == 0 || cols == 0 || rows > 10000 || cols > 10000 {
                base_error("invalid grid layout dimensions");
            }
            let mut widths = layout_lengths(field(layout, "widths"), cols, &f, 0);
            let mut heights = layout_lengths(field(layout, "heights"), rows, &f, 1);
            let respect = num(field(layout, "respect"), 0.) != 0.;
            let mut offset_x = 0.;
            let mut offset_y = 0.;
            if respect {
                let sx = f.width / widths.iter().sum::<f64>().max(1e-12);
                let sy = f.height / heights.iter().sum::<f64>().max(1e-12);
                let scale = sx.min(sy);
                for v in &mut widths {
                    *v *= scale;
                }
                for v in &mut heights {
                    *v *= scale;
                }
                offset_x = (f.width - widths.iter().sum::<f64>()).max(0.) / 2.;
                offset_y = (f.height - heights.iter().sum::<f64>()).max(0.) / 2.;
            }
            f.layout = Some(Layout {
                widths,
                heights,
                respect,
                offset_x,
                offset_y,
            });
        }
        let name = string(field(data, "name"), "");
        f.name = if name.is_empty() { None } else { Some(name) };
        f
    }
}

/// Internal evaluated dispatcher, fed by ordinary R closures below.
pub unsafe fn dispatch(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let operation = string(arg_by_name_or_position(args, &["operation"], 0), "");
        let data = arg_by_name_or_position(args, &["data"], 1);
        let target = device();
        let dims = (&*target).dimensions();
        let mut state = with_required_current_instance(|p| (*p).portable_grid.clone());
        state.ensure(dims);
        let parent = state.frames.last().unwrap().clone();
        match operation.as_str() {
            "newpage" => {
                state = GridState::default();
                state.ensure(dims);
                (&mut *target).clear(Color::WHITE);
            }
            "push" => {
                let frame = push(data, &parent);
                state.frames.push(frame);
            }
            "pop" => {
                let n = num(data, 1.);
                if n < 0. || n.fract() != 0. {
                    base_error("invalid viewport pop count");
                }
                let n = if n == 0. {
                    state.frames.len() - 1
                } else {
                    n as usize
                };
                if n >= state.frames.len() {
                    base_error("cannot pop the top-level viewport");
                }
                state.frames.truncate(state.frames.len() - n);
            }
            "up" => {
                let n = num(data, 1.);
                if n < 0. || n.fract() != 0. {
                    base_error("invalid viewport up count");
                }
                let n = if n == 0. {
                    state.frames.len() - 1
                } else {
                    n as usize
                };
                if n >= state.frames.len() {
                    base_error("cannot move above top-level viewport");
                }
                state.frames.truncate(state.frames.len() - n);
            }
            "seek" => {
                let name = string(field(data, "name"), "");
                if name.is_empty() {
                    base_error("viewport name must not be empty");
                }
                let index = state
                    .frames
                    .iter()
                    .rposition(|f| f.name.as_deref() == Some(name.as_str()))
                    .unwrap_or_else(|| base_error("named viewport was not found"));
                state.frames.truncate(index + 1);
            }
            "convert" => {
                let axis = num(field(data, "axis"), 0.);
                let dimension = num(field(data, "dimension"), 0.);
                if ![0., 1.].contains(&axis) || ![0., 1.].contains(&dimension) {
                    base_error("grid conversion axis and dimension must be 0 or 1");
                }
                let axis = axis as usize;
                let dimension = dimension != 0.;
                let values = units(field(data, "x"), "npc", &parent, axis, dimension);
                let to = string(field(data, "to"), "npc");
                let (f, o) = parent
                    .unit_factor(&to, axis, dimension)
                    .unwrap_or_else(|e| base_error(e));
                let values: Vec<_> = values.into_iter().map(|v| (v - o) / f).collect();
                return result(&values);
            }
            "draw" => {
                let mut frame = parent;
                frame.gp = gp(field(data, "gp"), &frame.gp);
                let primitive = string(field(data, "primitive"), "");
                let drawing = field(data, "data");
                let commands = draw_commands(&primitive, drawing, &frame);
                let target = &mut *target;
                target.set_clip(frame.clip);
                for c in commands {
                    match c {
                        Drawing::Path(p) => target.draw_path(&p),
                        Drawing::Text(t, p, params) => t.draw(target, p, &params),
                    }
                }
            }
            _ => base_error("unknown grid operation"),
        }
        with_required_current_instance(|p| (*p).portable_grid = state);
        crate::eval::runtime::set_visible(0);
        R_NilValue()
    }
}
enum Drawing {
    Path(Path),
    Text(crate::mainutils::plotmath::Label, Point, PlotParameters),
}
fn polygon(frame: &Frame, points: Vec<Point>, closed: bool, i: usize) -> Drawing {
    let mut c = vec![];
    for (j, p) in points.into_iter().enumerate() {
        c.push(if j == 0 {
            PathCommand::MoveTo(p.x, p.y)
        } else {
            PathCommand::LineTo(p.x, p.y)
        });
    }
    if closed {
        c.push(PathCommand::Close);
    }
    Drawing::Path(style_path(frame, c, i, closed))
}

fn point_symbol(frame: &Frame, x: f64, y: f64, size: f64, pch: f64, i: usize) -> Drawing {
    let radius = size * 0.375;
    let code = pch as i32;
    if matches!(code, 3 | 4 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14) {
        let q = |dx: f64, dy: f64| frame.map(x + dx, y + dy);
        let mut commands = Vec::new();
        let lines: &[(f64, f64, f64, f64)] = match code {
            3 => &[(-radius, 0., radius, 0.)],
            4 => &[
                (-radius, -radius, radius, radius),
                (-radius, radius, radius, -radius),
            ],
            7 => &[(-radius, 0., radius, 0.), (0., -radius, 0., radius)],
            8 => &[
                (-radius, 0., radius, 0.),
                (0., -radius, 0., radius),
                (-radius * 0.7, -radius * 0.7, radius * 0.7, radius * 0.7),
                (-radius * 0.7, radius * 0.7, radius * 0.7, -radius * 0.7),
            ],
            9 => &[
                (-radius, -radius, radius, radius),
                (-radius, radius, radius, -radius),
            ],
            10 => &[(-radius, 0., radius, 0.), (0., -radius, 0., radius)],
            11 => &[(-radius, -radius, radius, radius)],
            12 => &[(-radius, radius, radius, -radius)],
            13 => &[(-radius, 0., radius, 0.)],
            _ => &[(0., -radius, 0., radius)],
        };
        for (x0, y0, x1, y1) in lines {
            commands.push(PathCommand::MoveTo(q(*x0, *y0).x, q(*x0, *y0).y));
            commands.push(PathCommand::LineTo(q(*x1, *y1).x, q(*x1, *y1).y));
        }
        return Drawing::Path(style_path(frame, commands, i, false));
    }
    let n = match pch as i32 {
        0 | 1 | 2 | 5 | 6 | 15 | 16 | 17 | 18 | 19 | 20 | 21 | 22 | 23 | 24 | 25 => 4,
        _ => 32,
    };
    let points: Vec<_> = match pch as i32 {
        0 => (0..4)
            .map(|j| {
                let a = std::f64::consts::FRAC_PI_4 + j as f64 * std::f64::consts::FRAC_PI_2;
                frame.map(x + radius * a.cos(), y + radius * a.sin())
            })
            .collect(),
        1 => (0..4)
            .map(|j| {
                let a = std::f64::consts::FRAC_PI_4 + j as f64 * std::f64::consts::FRAC_PI_2;
                frame.map(x + radius * a.cos(), y + radius * a.sin())
            })
            .collect(),
        2 => (0..4)
            .map(|j| {
                let a = j as f64 * std::f64::consts::FRAC_PI_2;
                frame.map(x + radius * a.cos(), y + radius * a.sin())
            })
            .collect(),
        5 => (0..4)
            .map(|j| {
                let a = j as f64 * std::f64::consts::FRAC_PI_2;
                frame.map(x + radius * a.cos(), y + radius * a.sin())
            })
            .collect(),
        6 => (0..4)
            .map(|j| {
                let a = std::f64::consts::FRAC_PI_4 + j as f64 * std::f64::consts::FRAC_PI_2;
                frame.map(x + radius * a.cos(), y + radius * a.sin())
            })
            .collect(),
        15 | 16 | 17 | 18 | 19 | 20 | 21 | 22 | 23 | 24 | 25 => (0..n)
            .map(|j| {
                let a = j as f64 * std::f64::consts::TAU / n as f64;
                frame.map(x + radius * a.cos(), y + radius * a.sin())
            })
            .collect(),
        _ => (0..n)
            .map(|j| {
                let a = j as f64 * std::f64::consts::TAU / n as f64;
                frame.map(x + radius * a.cos(), y + radius * a.sin())
            })
            .collect(),
    };
    let mut d = polygon(frame, points, true, i);
    if let Drawing::Path(ref mut p) = d {
        match pch as i32 {
            1 | 2 | 5 | 6 | 15 | 16 | 17 | 18 => p.fill = CLEAR,
            3 => {
                p.fill = CLEAR;
            }
            4 => {
                p.fill = CLEAR;
            }
            _ => {}
        }
    }
    d
}

unsafe fn arrow_heads(frame: &Frame, arrow: SEXP, a: Point, b: Point, i: usize) -> Vec<Drawing> {
    unsafe {
        if arrow == R_NilValue() {
            return vec![];
        }
        let ends = string(field(arrow, "ends"), "last");
        let kind = string(field(arrow, "type"), "open");
        if kind != "open" && kind != "closed" {
            base_error("grid arrow type must be open or closed");
        }
        let angle = num(field(arrow, "angle"), 30.).to_radians();
        let length = units(field(arrow, "length"), "inches", frame, 0, true)
            .first()
            .copied()
            .unwrap_or(0.25 * DPI);
        if !length.is_finite() || length <= 0. {
            base_error("grid arrow length must be positive");
        }
        let mut out = Vec::new();
        for (tip, from, enabled) in [
            (b, a, ends == "last" || ends == "both"),
            (a, b, ends == "first" || ends == "both"),
        ] {
            if !enabled {
                continue;
            }
            let dx = (from.x - tip.x) as f64;
            let dy = (from.y - tip.y) as f64;
            let scale = (dx * dx + dy * dy).sqrt();
            if scale == 0. {
                continue;
            }
            let (ux, uy) = (dx / scale, dy / scale);
            let (c, s) = (angle.cos(), angle.sin());
            let left = Point {
                x: (tip.x as f64 + length * (ux * c - uy * s)) as f32,
                y: (tip.y as f64 + length * (ux * s + uy * c)) as f32,
            };
            let right = Point {
                x: (tip.x as f64 + length * (ux * c + uy * s)) as f32,
                y: (tip.y as f64 + length * (ux * -s + uy * c)) as f32,
            };
            let mut d = polygon(frame, vec![tip, left, right], kind == "closed", i);
            if kind == "open" {
                if let Drawing::Path(ref mut p) = d {
                    p.fill = CLEAR;
                }
            }
            out.push(d);
        }
        out
    }
}
unsafe fn draw_commands(kind: &str, data: SEXP, frame: &Frame) -> Vec<Drawing> {
    unsafe {
        let default = string(field(data, "default.units"), "npc");
        let coordinate = |name, axis, dim| units(field(data, name), &default, frame, axis, dim);
        let mut out = vec![];
        if kind == "segments" {
            let x0 = coordinate("x0", 0, false);
            let y0 = coordinate("y0", 1, false);
            let x1 = coordinate("x1", 0, false);
            let y1 = coordinate("y1", 1, false);
            for i in 0..x0.len().max(y0.len()).max(x1.len()).max(y1.len()) {
                let a = frame.map(x0[i % x0.len()], y0[i % y0.len()]);
                let b = frame.map(x1[i % x1.len()], y1[i % y1.len()]);
                out.push(polygon(frame, vec![a, b], false, i));
                out.extend(arrow_heads(frame, field(data, "arrow"), a, b, i));
            }
            return out;
        }
        let x = coordinate("x", 0, false);
        let y = coordinate("y", 1, false);
        let n = x.len().max(y.len());
        match kind {
            "lines" | "polygon" => {
                if string(field(data, "rule"), "winding") != "winding" {
                    base_error("grid polygon fill rule is not supported");
                }
                let ids = numbers(field(data, "id"));
                let lengths = numbers(field(data, "id.lengths"));
                if !ids.is_empty() && !lengths.is_empty() {
                    base_error("grid polygon cannot specify both id and id.lengths");
                }
                if !lengths.is_empty() {
                    let mut at = 0usize;
                    for (g, len) in lengths.iter().enumerate() {
                        let len = *len as usize;
                        if len == 0 || at + len > n {
                            base_error("invalid grid polygon id.lengths");
                        }
                        let points = (at..at + len)
                            .map(|i| frame.map(x[i % x.len()], y[i % y.len()]))
                            .collect();
                        out.push(polygon(frame, points, kind == "polygon", g));
                        at += len;
                    }
                } else if !ids.is_empty() {
                    let mut groups: Vec<(i32, Vec<Point>)> = Vec::new();
                    for i in 0..n {
                        let id = ids[i % ids.len()] as i32;
                        if id <= 0 {
                            continue;
                        }
                        if let Some((_, points)) = groups.iter_mut().find(|(g, _)| *g == id) {
                            points.push(frame.map(x[i % x.len()], y[i % y.len()]));
                        } else {
                            groups.push((id, vec![frame.map(x[i % x.len()], y[i % y.len()])]));
                        }
                    }
                    for (g, (_, points)) in groups.into_iter().enumerate() {
                        if points.len() >= 2 {
                            out.push(polygon(frame, points, kind == "polygon", g));
                        }
                    }
                } else {
                    let points = (0..n)
                        .map(|i| frame.map(x[i % x.len()], y[i % y.len()]))
                        .collect();
                    let points: Vec<Point> = points;
                    out.push(polygon(frame, points.clone(), kind == "polygon", 0));
                    if kind == "lines" && points.len() >= 2 {
                        out.extend(arrow_heads(
                            frame,
                            field(data, "arrow"),
                            *points.first().unwrap(),
                            *points.last().unwrap(),
                            0,
                        ));
                    }
                }
            }
            "rect" => {
                let w = coordinate("width", 0, true);
                let h = coordinate("height", 1, true);
                let mut just = justification(field(data, "just"));
                just[0] = num(field(data, "hjust"), just[0]);
                just[1] = num(field(data, "vjust"), just[1]);
                for i in 0..n.max(w.len()).max(h.len()) {
                    let (w, h) = (w[i % w.len()], h[i % h.len()]);
                    let (x, y) = (x[i % x.len()] - just[0] * w, y[i % y.len()] - just[1] * h);
                    out.push(polygon(
                        frame,
                        vec![
                            frame.map(x, y),
                            frame.map(x + w, y),
                            frame.map(x + w, y + h),
                            frame.map(x, y + h),
                        ],
                        true,
                        i,
                    ));
                }
            }
            "circle" => {
                let r = units(field(data, "r"), &default, frame, 0, true);
                for i in 0..n.max(r.len()) {
                    let radius = r[i % r.len()];
                    if radius < 0. {
                        base_error("grid circle radius must not be negative");
                    }
                    let points = (0..96)
                        .map(|j| {
                            let a = j as f64 * std::f64::consts::TAU / 96.;
                            frame.map(
                                x[i % x.len()] + radius * a.cos(),
                                y[i % y.len()] + radius * a.sin(),
                            )
                        })
                        .collect();
                    out.push(polygon(frame, points, true, i));
                }
            }
            "points" => {
                let pch = numbers(field(data, "pch"));
                if pch.is_empty()
                    || pch
                        .iter()
                        .any(|p| !p.is_finite() || *p < 0. || *p > 25. || p.fract() != 0.)
                {
                    base_error("grid.points pch must be an integer from 0 through 25");
                }
                let sizes = coordinate("size", 0, true);
                for i in 0..n.max(sizes.len()).max(pch.len()) {
                    out.push(point_symbol(
                        frame,
                        x[i % x.len()],
                        y[i % y.len()],
                        sizes[i % sizes.len()],
                        pch[i % pch.len()],
                        i,
                    ));
                }
            }
            "text" => {
                if num(field(data, "check.overlap"), 0.) != 0. {
                    base_error("grid text overlap checking is not supported");
                }
                let labels = crate::mainutils::plotmath::labels(field(data, "label"));
                let count = labels.len();
                if count == 0 {
                    return out;
                }
                let mut just = justification(field(data, "just"));
                just[0] = num(field(data, "hjust"), just[0]);
                just[1] = num(field(data, "vjust"), just[1]);
                if ![0., 0.5, 1.].contains(&just[0]) {
                    base_error("grid text horizontal justification must be 0, 0.5 or 1");
                }
                let rotation = num(field(data, "rot"), 0.);
                let params = PlotParameters {
                    font_face: frame.gp.face,
                    font_size: frame.gp.size as f32,
                    text_color: alpha(frame.gp.col[0], frame.gp.alpha),
                    dpi: DPI as f32,
                    text_anchor: if just[0] == 0. {
                        TextAnchor::Start
                    } else if just[0] == 1. {
                        TextAnchor::End
                    } else {
                        TextAnchor::Middle
                    },
                    text_angle: (rotation - frame.matrix[1].atan2(frame.matrix[0]).to_degrees())
                        as f32,
                };
                for i in 0..n.max(count) {
                    let label = labels[i % count].clone();
                    let mut p = frame.map(x[i % x.len()], y[i % y.len()]);
                    p.y += ((0.5 - just[1]) * frame.gp.size) as f32;
                    let mut params = params.clone();
                    params.text_color = alpha(frame.gp.col[i % frame.gp.col.len()], frame.gp.alpha);
                    out.push(Drawing::Text(label, p, params));
                }
            }
            _ => base_error(format!("grid primitive '{kind}' is not supported")),
        }
        out
    }
}

macro_rules! wrapper {
    ($name:ident, $public:literal, $file:literal) => {
        pub unsafe fn $name(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
            unsafe {
                crate::mainutils::base_wrappers::apply_in_environment(
                    $public,
                    include_str!($file),
                    args,
                    rho,
                    false,
                    namespace(),
                )
            }
        }
    };
}
wrapper!(do_unit, "unit", "portable_grid/unit.R");
wrapper!(do_ops_unit, "Ops.unit", "portable_grid/ops_unit.R");
wrapper!(
    do_summary_unit,
    "Summary.unit",
    "portable_grid/summary_unit.R"
);
wrapper!(do_is_unit, "is.unit", "portable_grid/is_unit.R");
wrapper!(do_gpar, "gpar", "portable_grid/gpar.R");
wrapper!(do_viewport, "viewport", "portable_grid/viewport.R");
wrapper!(do_grid_layout, "grid.layout", "portable_grid/grid_layout.R");
wrapper!(
    do_push_viewport,
    "pushViewport",
    "portable_grid/push_viewport.R"
);
wrapper!(
    do_pop_viewport,
    "popViewport",
    "portable_grid/pop_viewport.R"
);
wrapper!(do_up_viewport, "upViewport", "portable_grid/up_viewport.R");
wrapper!(
    do_seek_viewport,
    "seekViewport",
    "portable_grid/seek_viewport.R"
);
wrapper!(
    do_grid_newpage,
    "grid.newpage",
    "portable_grid/grid_newpage.R"
);
wrapper!(do_convert_x, "convertX", "portable_grid/convert_x.R");
wrapper!(do_convert_y, "convertY", "portable_grid/convert_y.R");
wrapper!(
    do_convert_width,
    "convertWidth",
    "portable_grid/convert_width.R"
);
wrapper!(
    do_convert_height,
    "convertHeight",
    "portable_grid/convert_height.R"
);
wrapper!(do_grid_draw, "grid.draw", "portable_grid/grid_draw.R");
wrapper!(do_glist, "gList", "portable_grid/glist.R");
wrapper!(do_gtree, "gTree", "portable_grid/gtree.R");
wrapper!(do_grobtree, "grobTree", "portable_grid/grobtree.R");
wrapper!(do_is_grob, "is.grob", "portable_grid/is_grob.R");
wrapper!(do_rect_grob, "rectGrob", "portable_grid/rect_grob.R");
wrapper!(do_grid_rect, "grid.rect", "portable_grid/grid_rect.R");
wrapper!(do_circle_grob, "circleGrob", "portable_grid/circle_grob.R");
wrapper!(do_grid_circle, "grid.circle", "portable_grid/grid_circle.R");
wrapper!(do_lines_grob, "linesGrob", "portable_grid/lines_grob.R");
wrapper!(do_grid_lines, "grid.lines", "portable_grid/grid_lines.R");
wrapper!(
    do_polygon_grob,
    "polygonGrob",
    "portable_grid/polygon_grob.R"
);
wrapper!(
    do_grid_polygon,
    "grid.polygon",
    "portable_grid/grid_polygon.R"
);
wrapper!(
    do_segments_grob,
    "segmentsGrob",
    "portable_grid/segments_grob.R"
);
wrapper!(
    do_grid_segments,
    "grid.segments",
    "portable_grid/grid_segments.R"
);
wrapper!(do_text_grob, "textGrob", "portable_grid/text_grob.R");
wrapper!(do_grid_text, "grid.text", "portable_grid/grid_text.R");
wrapper!(do_points_grob, "pointsGrob", "portable_grid/points_grob.R");
wrapper!(do_grid_points, "grid.points", "portable_grid/grid_points.R");

/// Exported portable grid surface; namespace lookup is restricted to these names.
pub(crate) const EXPORTS: &[&str] = &[
    "unit",
    "Ops.unit",
    "Summary.unit",
    "is.unit",
    "gpar",
    "viewport",
    "grid.layout",
    "pushViewport",
    "popViewport",
    "upViewport",
    "seekViewport",
    "grid.newpage",
    "convertX",
    "convertY",
    "convertWidth",
    "convertHeight",
    "grid.draw",
    "gList",
    "gTree",
    "grobTree",
    "is.grob",
    "rectGrob",
    "grid.rect",
    "circleGrob",
    "grid.circle",
    "linesGrob",
    "grid.lines",
    "polygonGrob",
    "grid.polygon",
    "segmentsGrob",
    "grid.segments",
    "textGrob",
    "grid.text",
    "pointsGrob",
    "grid.points",
];

unsafe fn new_grid_environment() -> SEXP {
    unsafe {
        use crate::sexp::{
            envir::defineVar, globals::R_BaseEnv, protect::protect, symbol::Rf_install,
        };
        let env = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), R_BaseEnv(), R_NilValue());
        let _guard = protect(env);
        crate::mainutils::essentials::define_package_metadata("grid", env);
        for name in EXPORTS {
            let symbol_name = std::ffi::CString::new(*name).expect("static grid export");
            let symbol = Rf_install(symbol_name.as_ptr());
            let value = crate::eval::primitive::make_primitive_binding(name, SEXPTYPE::BUILTINSXP);
            let _value_guard = protect(value);
            defineVar(symbol, value, env);
        }
        env
    }
}
pub(crate) unsafe fn namespace() -> SEXP {
    unsafe {
        let cached = with_required_current_instance(|p| {
            (*p).package_namespace_cache
                .get("grid")
                .map(|(_, env)| *env)
        });
        if let Some(env) = cached {
            return env;
        }
        let env = new_grid_environment();
        with_required_current_instance(|p| {
            (*p).package_namespace_cache.insert(
                "grid".into(),
                (std::path::PathBuf::from("<builtin:grid>"), env),
            );
        });
        env
    }
}
pub(crate) unsafe fn attach() {
    unsafe {
        if !crate::mainutils::essentials::package_attached("grid") {
            namespace();
            let env = new_grid_environment();
            let _guard = crate::sexp::protect::protect(env);
            crate::mainutils::essentials::attach_package_env(env);
        }
    }
}
