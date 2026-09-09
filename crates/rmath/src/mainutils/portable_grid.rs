//! Portable grid frontend. Runtime values are decoded before borrowing a device;
//! viewport transforms and graphical parameters are owned session state.
mod layout;

use crate::eval::attrib_core::{R_ClassSymbol, R_NamesSymbol, getAttrib, setAttrib};
use crate::mainutils::errors::Rf_warning1;
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
use serde::{Deserialize, Serialize};

// Unit computations keep R allocations rooted and numeric work in owned vectors.
// Mixed dimensions remain deferred expressions until viewport conversion.
unsafe fn unit_kind(x: SEXP) -> String {
    unsafe {
        let u = field(x, "units");
        let name = string(u, "");
        if name.is_empty() {
            base_error("invalid grid unit names");
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

// Build the small unit object used by the portable grid R wrappers.  Arithmetic
// units use `units` = the operation name and `data` = a list of scalar units;
// this mirrors grid's unit_v2 expression nodes without making the rest of the
// portable API depend on grid's private representation.
unsafe fn scalar_unit(value: f64, name: &str, data: SEXP) -> SEXP {
    unsafe {
        let _data_arg = crate::sexp::protect::protect(data);
        let out = Rf_allocVector(SEXPTYPE::VECSXP, 3);
        let _out = crate::sexp::protect::protect(out);
        let names = Rf_allocVector(SEXPTYPE::STRSXP, 3);
        let _names = crate::sexp::protect::protect(names);
        for (i, n) in ["value", "units", "data"].iter().enumerate() {
            SET_STRING_ELT(
                names,
                i as i64,
                Rf_mkCharLen(n.as_ptr() as *const _, n.len() as i32),
            );
        }
        setAttrib(out, R_NamesSymbol(), names);
        SET_VECTOR_ELT(out, 0, Rf_ScalarReal(value));
        let unit_name = Rf_allocVector(SEXPTYPE::STRSXP, 1);
        let _unit_name = crate::sexp::protect::protect(unit_name);
        SET_STRING_ELT(
            unit_name,
            0,
            Rf_mkCharLen(name.as_ptr() as *const _, name.len() as i32),
        );
        SET_VECTOR_ELT(out, 1, unit_name);
        SET_VECTOR_ELT(out, 2, data);
        let class = Rf_allocVector(SEXPTYPE::STRSXP, 1);
        let _class = crate::sexp::protect::protect(class);
        SET_STRING_ELT(class, 0, Rf_mkChar(c"unit".as_ptr()));
        setAttrib(out, R_ClassSymbol(), class);
        out
    }
}

unsafe fn unit_element(x: SEXP, i: usize) -> SEXP {
    unsafe {
        let values = field(x, "value");
        let names = field(x, "units");
        let data = field(x, "data");
        let value = if XLENGTH(values) == 0 {
            0.
        } else if TYPEOF(values) == SEXPTYPE::REALSXP {
            *REAL(values).add(i % XLENGTH(values) as usize)
        } else if TYPEOF(values) == SEXPTYPE::INTSXP {
            *INTEGER(values).add(i % XLENGTH(values) as usize) as f64
        } else {
            base_error("invalid grid unit values");
        };
        let name = if XLENGTH(names) == 0 {
            "npc".into()
        } else {
            elt_to_string(names, (i as i64) % XLENGTH(names))
        };
        let datum = if data == R_NilValue() || XLENGTH(data) == 0 {
            R_NilValue()
        } else if TYPEOF(data) == SEXPTYPE::STRSXP {
            let d = Rf_allocVector(SEXPTYPE::STRSXP, 1);
            let _d = crate::sexp::protect::protect(d);
            SET_STRING_ELT(d, 0, STRING_ELT(data, (i as i64) % XLENGTH(data)));
            d
        } else {
            VECTOR_ELT(data, (i as i64) % XLENGTH(data))
        };
        scalar_unit(value, &name, datum)
    }
}
unsafe fn unit_name_at(x: SEXP, i: usize) -> String {
    unsafe {
        let names = field(x, "units");
        if TYPEOF(names) != SEXPTYPE::STRSXP || XLENGTH(names) == 0 {
            base_error("invalid grid unit names");
        }
        elt_to_string(names, (i as i64) % XLENGTH(names))
    }
}
unsafe fn unit_data_at(x: SEXP, i: usize) -> SEXP {
    unsafe {
        let data = field(x, "data");
        if data == R_NilValue() || XLENGTH(data) == 0 {
            return R_NilValue();
        }
        if TYPEOF(data) == SEXPTYPE::VECSXP {
            VECTOR_ELT(data, (i as i64) % XLENGTH(data))
        } else {
            data
        }
    }
}

unsafe fn expression_unit(op: &str, terms: &[SEXP]) -> SEXP {
    unsafe {
        let _terms: Vec<_> = terms
            .iter()
            .map(|term| crate::sexp::protect::protect(*term))
            .collect();
        let data = Rf_allocVector(SEXPTYPE::VECSXP, terms.len() as i32);
        let _data = crate::sexp::protect::protect(data);
        for (i, term) in terms.iter().enumerate() {
            SET_VECTOR_ELT(data, i as i64, *term);
        }
        scalar_unit(1., op, data)
    }
}
enum UnitTerm {
    Scalar(f64, String, SEXP),
    Expr(SEXP),
}
unsafe fn unit_terms_result(template: SEXP, terms: &[UnitTerm]) -> SEXP {
    unsafe {
        let _template = crate::sexp::protect::protect(template);
        let _exprs: Vec<_> = terms
            .iter()
            .map(|term| match term {
                UnitTerm::Expr(e) | UnitTerm::Scalar(_, _, e) => crate::sexp::protect::protect(*e),
            })
            .collect();
        let values = Rf_allocVector(SEXPTYPE::REALSXP, terms.len() as i32);
        let _values = crate::sexp::protect::protect(values);
        let names = Rf_allocVector(SEXPTYPE::STRSXP, terms.len() as i32);
        let _names = crate::sexp::protect::protect(names);
        let data = Rf_allocVector(SEXPTYPE::VECSXP, terms.len() as i32);
        let _data = crate::sexp::protect::protect(data);
        let mut any_data = false;
        for (i, term) in terms.iter().enumerate() {
            let (v, n, d) = match term {
                UnitTerm::Scalar(v, n, d) => (*v, n.as_str(), *d),
                UnitTerm::Expr(e) => (1., "sum", *e),
            };
            *REAL(values).add(i) = v;
            SET_STRING_ELT(
                names,
                i as i64,
                Rf_mkCharLen(n.as_ptr() as *const _, n.len() as i32),
            );
            SET_VECTOR_ELT(data, i as i64, d);
            any_data |= d != R_NilValue();
        }
        let obj = crate::mainutils::seq::Rf_shallow_duplicate(template);
        let nms = getAttrib(obj, R_NamesSymbol());
        for i in 0..XLENGTH(obj).min(XLENGTH(nms)) {
            match elt_to_string(nms, i).as_str() {
                "value" => SET_VECTOR_ELT(obj, i, values),
                "units" => SET_VECTOR_ELT(obj, i, names),
                "data" => SET_VECTOR_ELT(obj, i, if any_data { data } else { R_NilValue() }),
                _ => {}
            }
        }
        obj
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
        let mut expr_roots = Vec::new();
        let av = unit_values(if au { field(a, "value") } else { a });
        let out: Vec<UnitTerm> = if unary {
            av.into_iter()
                .enumerate()
                .map(|(i, v)| {
                    UnitTerm::Scalar(
                        if op == "-" { -v } else { v },
                        unit_name_at(template, i),
                        unit_data_at(template, i),
                    )
                })
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
                    if au && bu {
                        let an = elt_to_string(
                            field(a, "units"),
                            (i as i64) % XLENGTH(field(a, "units")),
                        );
                        let bn = elt_to_string(
                            field(b, "units"),
                            (i as i64) % XLENGTH(field(b, "units")),
                        );
                        if an != bn
                            || field(a, "data") != R_NilValue()
                            || field(b, "data") != R_NilValue()
                        {
                            let left = unit_element(a, i);
                            let _left = crate::sexp::protect::protect(left);
                            let mut right = unit_element(b, i);
                            let _right = crate::sexp::protect::protect(right);
                            if op == "-" {
                                let right_data = field(right, "data");
                                right = scalar_unit(-y, &bn, right_data);
                            }
                            let _right_adjusted = crate::sexp::protect::protect(right);
                            let expr = expression_unit("sum", &[left, right]);
                            expr_roots.push(crate::sexp::protect::protect(expr));
                            return UnitTerm::Expr(expr);
                        }
                    }
                    UnitTerm::Scalar(
                        match op {
                            "+" => x + y,
                            "-" => x - y,
                            "*" => x * y,
                            "/" => x / y,
                            _ => unreachable!(),
                        },
                        if au {
                            unit_name_at(a, i)
                        } else {
                            unit_name_at(b, i)
                        },
                        if au && !bu {
                            unit_data_at(a, i)
                        } else if bu && !au {
                            unit_data_at(b, i)
                        } else {
                            R_NilValue()
                        },
                    )
                })
                .collect::<Vec<_>>()
        };
        Some(unit_terms_result(template, &out))
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
        let mut values = Vec::new();
        let mut expression_terms = Vec::new();
        let mut expression_roots = Vec::new();
        let mut mixed = false;
        let first_kind = unit_kind(template);
        p = args;
        while p != R_NilValue() {
            if TAG(p) != na_tag {
                let x = CAR(p);
                if !crate::mainutils::essentials::sexp_has_class(x, "unit") {
                    base_error("unit summary requires matching units");
                }
                mixed |= unit_kind(x) != first_kind || field(x, "data") != R_NilValue();
                for i in 0..XLENGTH(field(x, "value")) {
                    mixed |= unit_name_at(x, i as usize) != first_kind;
                    let term = unit_element(x, i as usize);
                    expression_roots.push(crate::sexp::protect::protect(term));
                    expression_terms.push(term);
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
        if mixed {
            Some(expression_unit(op, &expression_terms))
        } else {
            Some(unit_result(template, &[v]))
        }
    }
}

const DPI: f64 = 96.;
const CLEAR: Color = Color {
    r: 0,
    g: 0,
    b: 0,
    a: 0,
};
#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct Gp {
    col: Vec<Color>,
    fill: Vec<Color>,
    width: f64,
    size: f64,
    alpha: f64,
    face: r_graphics_engine::FontFace,
    dash: Option<DashPattern>,
    lineheight: f64,
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
            lineheight: 1.2,
        }
    }
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct Layout {
    widths: Vec<f64>,
    heights: Vec<f64>,
    offset_x: f64,
    offset_y: f64,
}
#[derive(Clone, Serialize, Deserialize, Default)]
pub(crate) struct GridState {
    #[serde(default)]
    version: u8,
    frames: Vec<Frame>,
    nodes: Vec<ViewportNode>,
    active: Vec<usize>,
    dimensions: (u32, u32),
}
#[derive(Clone, Serialize, Deserialize)]
struct ViewportNode {
    parent: Option<usize>,
    frame: Frame,
    children: Vec<usize>,
}
impl GridState {
    fn compact_tree(&mut self) {
        let mut keep = Vec::new();
        let mut stack = vec![0usize];
        while let Some(id) = stack.pop() {
            keep.push(id);
            stack.extend(self.nodes[id].children.iter().copied());
        }
        keep.sort_unstable();
        let mut remap = vec![usize::MAX; self.nodes.len()];
        for (new, old) in keep.iter().enumerate() {
            remap[*old] = new;
        }
        let mut nodes = Vec::with_capacity(keep.len());
        for old in keep {
            let mut node = self.nodes[old].clone();
            node.parent = node.parent.map(|p| remap[p]);
            node.children = node
                .children
                .into_iter()
                .filter_map(|c| (remap[c] != usize::MAX).then_some(remap[c]))
                .collect();
            nodes.push(node);
        }
        self.active = self.active.iter().map(|id| remap[*id]).collect();
        self.nodes = nodes;
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
    pub(crate) fn encode_snapshot(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }
    pub(crate) fn decode_snapshot(bytes: &[u8]) -> Result<Self, &'static str> {
        const INVALID: &str = "recorded plot has invalid grid state metadata";
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(INVALID);
        }
        let state: Self = serde_json::from_slice(bytes).map_err(|_| INVALID)?;
        if state.version > 1 || state.nodes.len() > 10000 {
            return Err(INVALID);
        }
        // Base-only recordings legitimately have no initialized grid device.
        if state.frames.is_empty() && state.nodes.is_empty() && state.active.is_empty() {
            return Ok(state);
        }
        if state.nodes.is_empty()
            || state.active.first() != Some(&0)
            || state.frames.len() != state.active.len()
            || state.dimensions.0 == 0
            || state.dimensions.1 == 0
            || state.nodes[0].parent.is_some()
        {
            return Err(INVALID);
        }
        // Visit each reachable node once. Duplicate child edges, cycles and
        // disconnected subtrees all fail without indexing an unchecked ID.
        let mut seen = vec![false; state.nodes.len()];
        let mut pending = vec![0usize];
        while let Some(id) = pending.pop() {
            if id >= state.nodes.len() || seen[id] {
                return Err(INVALID);
            }
            seen[id] = true;
            let node = &state.nodes[id];
            if !node.frame.valid() {
                return Err(INVALID);
            }
            for &child in &node.children {
                if child >= state.nodes.len() || state.nodes[child].parent != Some(id) {
                    return Err(INVALID);
                }
                pending.push(child);
            }
        }
        if seen.iter().any(|v| !v) {
            return Err(INVALID);
        }
        for (depth, (&id, frame)) in state.active.iter().zip(&state.frames).enumerate() {
            let node = state.nodes.get(id).ok_or(INVALID)?;
            if frame != &node.frame || (depth > 0 && node.parent != Some(state.active[depth - 1])) {
                return Err(INVALID);
            }
        }
        Ok(state)
    }
    pub(crate) fn scale_snapshot(
        &mut self,
        source: (u32, u32),
        target: (u32, u32),
    ) -> Result<(), &'static str> {
        if self.is_empty() {
            self.dimensions = target;
            return Ok(());
        }
        if self.dimensions != source {
            return Err("recorded plot has inconsistent grid dimensions");
        }
        let sx = target.0 as f64 / source.0.max(1) as f64;
        let sy = target.1 as f64 / source.1.max(1) as f64;
        let scale = |f: &mut Frame| {
            f.width *= sx;
            f.height *= sy;
            f.matrix[0] *= sx;
            f.matrix[2] *= sx;
            f.matrix[4] *= sx;
            f.matrix[1] *= sy;
            f.matrix[3] *= sy;
            f.matrix[5] *= sy;
            if let Some(c) = &mut f.clip {
                c[0] *= sx as f32;
                c[2] *= sx as f32;
                c[1] *= sy as f32;
                c[3] *= sy as f32;
            }
            if let Some(l) = &mut f.layout {
                l.widths.iter_mut().for_each(|v| *v *= sx);
                l.heights.iter_mut().for_each(|v| *v *= sy);
                l.offset_x *= sx;
                l.offset_y *= sy;
            }
        };
        self.frames.iter_mut().for_each(scale);
        self.nodes.iter_mut().for_each(|n| scale(&mut n.frame));
        self.dimensions = target;
        if self
            .frames
            .iter()
            .chain(self.nodes.iter().map(|n| &n.frame))
            .any(|f| !f.valid())
        {
            return Err("recorded plot grid scaling exceeds device range");
        }
        Ok(())
    }
    fn ensure(&mut self, dims: (u32, u32)) {
        if self.frames.is_empty() || self.dimensions != dims {
            self.dimensions = dims;
            let root = Frame {
                width: dims.0 as f64,
                height: dims.1 as f64,
                matrix: [1., 0., 0., -1., 0., dims.1 as f64],
                scale: [[0., 1.], [0., 1.]],
                clip: None,
                gp: Gp::default(),
                layout: None,
                name: None,
            };
            self.frames = vec![root.clone()];
            self.nodes = vec![ViewportNode {
                parent: None,
                frame: root,
                children: vec![],
            }];
            self.active = vec![0];
        }
    }
}
impl Frame {
    fn valid(&self) -> bool {
        let gp = &self.gp;
        self.width.is_finite()
            && self.width >= 0.
            && self.height.is_finite()
            && self.height >= 0.
            && self.matrix.iter().all(|v| v.is_finite())
            && self
                .scale
                .iter()
                .all(|v| v.iter().all(|n| n.is_finite()) && v[0] != v[1])
            && self
                .clip
                .is_none_or(|c| c.iter().all(|v| v.is_finite()) && c[0] <= c[2] && c[1] <= c[3])
            && !gp.col.is_empty()
            && !gp.fill.is_empty()
            && gp.width.is_finite()
            && gp.width >= 0.
            && gp.size.is_finite()
            && gp.size > 0.
            && gp.lineheight.is_finite()
            && gp.lineheight >= 0.
            && gp.alpha.is_finite()
            && (0. ..=1.).contains(&gp.alpha)
            && gp.dash.as_ref().is_none_or(|d| {
                d.offset.is_finite()
                    && !d.intervals.is_empty()
                    && d.intervals.iter().all(|v| v.is_finite() && *v > 0.)
            })
            && self.layout.as_ref().is_none_or(|l| {
                l.offset_x.is_finite()
                    && l.offset_y.is_finite()
                    && !l.widths.is_empty()
                    && !l.heights.is_empty()
                    && l.widths
                        .iter()
                        .chain(&l.heights)
                        .all(|v| v.is_finite() && *v >= 0.)
            })
    }
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
            "lines" => (self.gp.size * self.gp.lineheight, 0.),
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
unsafe fn grob_extent(grob: SEXP, frame: &Frame, axis: usize) -> f64 {
    unsafe {
        if grob == R_NilValue() || !crate::mainutils::essentials::sexp_has_class(grob, "grob") {
            base_error("grob unit data must be a grob");
        }
        let kind = string(field(grob, "primitive"), "");
        let data = field(grob, "data");
        match (kind.as_str(), axis) {
            ("rect", 0) => units(field(data, "width"), "npc", frame, 0, true)[0],
            ("rect", 1) => units(field(data, "height"), "npc", frame, 1, true)[0],
            ("circle", _) => 2. * units(field(data, "r"), "snpc", frame, axis, true)[0],
            ("text", 0) => {
                let labels = field(data, "label");
                (0..XLENGTH(labels))
                    .map(|i| {
                        r_graphics_engine::default_font_book()
                            .measure_text(
                                &elt_to_string(labels, i),
                                frame.gp.size as f32,
                                frame.gp.face,
                            )
                            .width as f64
                    })
                    .fold(0., f64::max)
            }
            ("text", 1) => {
                let labels = field(data, "label");
                (0..XLENGTH(labels))
                    .map(|i| {
                        let m = r_graphics_engine::default_font_book().measure_text(
                            &elt_to_string(labels, i),
                            frame.gp.size as f32,
                            frame.gp.face,
                        );
                        (m.ascent + m.descent) as f64
                    })
                    .fold(0., f64::max)
            }
            _ => base_error(format!("grob unit is not supported for primitive '{kind}'")),
        }
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
                if matches!(name.as_str(), "sum" | "min" | "max")
                    && TYPEOF(data) == SEXPTYPE::VECSXP
                    && XLENGTH(data) > 0
                {
                    let mut parts = Vec::new();
                    for j in 0..XLENGTH(data) {
                        parts.extend(units(VECTOR_ELT(data, j), default, frame, axis, dimension));
                    }
                    return match name.as_str() {
                        "sum" => parts.iter().sum(),
                        "min" => parts.into_iter().fold(f64::INFINITY, f64::min),
                        _ => parts.into_iter().fold(f64::NEG_INFINITY, f64::max),
                    };
                }
                if name == "grobwidth" || name == "grobheight" {
                    if data == R_NilValue() {
                        base_error("grob units require grob data");
                    }
                    let extent = grob_extent(data, frame, if name == "grobwidth" { 0 } else { 1 });
                    let (f, _) = frame.unit_factor("inches", axis, dimension).unwrap();
                    return v * extent / f;
                }
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
        if field(x, "lineheight") != R_NilValue() {
            p.lineheight = num(field(x, "lineheight"), 1.2);
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
unsafe fn layout_axis(x: SEXP, n: usize, frame: &Frame, axis: usize) -> layout::Axis {
    unsafe {
        let values = numbers(field(x, "value"));
        let names = field(x, "units");
        if values.is_empty() || XLENGTH(names) == 0 {
            base_error("invalid layout units");
        }
        let terms = (0..n)
            .map(|i| {
                let v = values[i % values.len()];
                if !v.is_finite() {
                    base_error("layout dimensions must be finite");
                }
                let name = elt_to_string(names, i as i64 % XLENGTH(names));
                if name == "null" {
                    layout::Length::Null(v)
                } else {
                    layout::Length::Fixed(
                        v * frame
                            .unit_factor(&name, axis, true)
                            .unwrap_or_else(|e| base_error(e))
                            .0,
                    )
                }
            })
            .collect();
        layout::Axis::new(terms, frame.extent(axis))
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
        if !width.is_finite() || !height.is_finite() {
            base_error("viewport dimensions must be finite");
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
                    // GNU R cannot clip to a rotated viewport.  It warns and
                    // leaves the parent's clip region active; retain that
                    // behavior instead of silently clipping to the rotated
                    // viewport's axis-aligned bounds.
                    Rf_warning1(c"cannot clip to rotated viewport".as_ptr());
                    f.clip = parent.clip;
                } else {
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
            let x = layout_axis(field(layout, "widths"), cols, &f, 0);
            let y = layout_axis(field(layout, "heights"), rows, &f, 1);
            let respect = numbers(field(layout, "respect"));
            let matrix = num(field(layout, "respect.matrix"), 0.) != 0.;
            let mut respected_cols = vec![false; cols];
            let mut respected_rows = vec![false; rows];
            if matrix {
                if respect.len() != rows * cols {
                    base_error("respect matrix must match layout dimensions");
                }
                for c in 0..cols {
                    for r in 0..rows {
                        if respect[c * rows + r] != 0. {
                            respected_cols[c] = true;
                            respected_rows[r] = true;
                        }
                    }
                }
            } else if respect.first().is_some_and(|v| *v != 0.) {
                respected_cols.fill(true);
                respected_rows.fill(true);
            }
            let (widths, heights) = layout::allocate(x, y, &respected_cols, &respected_rows);
            let just = justification(field(layout, "just"));
            // The y offset is measured down from the top; R's justification is from the bottom.
            let offset_x = (f.width - widths.iter().sum::<f64>()) * just[0];
            let offset_y = (f.height - heights.iter().sum::<f64>()) * (1. - just[1]);
            f.layout = Some(Layout {
                widths,
                heights,
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
                let parent_id = *state.active.last().unwrap();
                let id = state.nodes.len();
                state.nodes.push(ViewportNode {
                    parent: Some(parent_id),
                    frame: state.frames.last().unwrap().clone(),
                    children: vec![],
                });
                state.nodes[parent_id].children.push(id);
                state.active.push(id);
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
                let removed = state.active.split_off(state.frames.len());
                if let Some(id) = removed.first().copied() {
                    if let Some(parent) = state.nodes[id].parent {
                        state.nodes[parent].children.retain(|child| *child != id);
                    }
                }
                state.compact_tree();
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
                let keep = state.frames.len() - n;
                state.frames.truncate(keep);
                state.active.truncate(keep);
            }
            "seek" => {
                let name = string(field(data, "name"), "");
                if name.is_empty() {
                    base_error("viewport name must not be empty");
                }
                let parts = name
                    .split("::")
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>();
                let path_names = |mut id: usize| {
                    let mut names = Vec::new();
                    loop {
                        if let Some(name) = state.nodes[id].frame.name.as_deref() {
                            names.push(name);
                        }
                        let Some(parent) = state.nodes[id].parent else {
                            break;
                        };
                        id = parent;
                    }
                    names.reverse();
                    names
                };
                let id = state
                    .nodes
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(id, node)| {
                        node.frame.name.as_deref() == Some(name.as_str())
                            || path_names(*id).iter().copied().eq(parts.iter().copied())
                    })
                    .map(|(id, _)| id)
                    .unwrap_or_else(|| base_error("named viewport was not found"));
                let mut path = vec![];
                let mut cur = Some(id);
                while let Some(node) = cur {
                    path.push(node);
                    cur = state.nodes[node].parent;
                }
                path.reverse();
                state.active = path;
                state.frames = state
                    .active
                    .iter()
                    .map(|id| state.nodes[*id].frame.clone())
                    .collect();
            }
            "down" => {
                let name = string(field(data, "name"), "");
                if name.is_empty() {
                    base_error("viewport name must not be empty");
                }
                let strict = num(field(data, "strict"), 1.) != 0.;
                let parts = name
                    .split("::")
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>();
                let start = *state.active.last().unwrap();
                let mut found = None;
                let mut queue = vec![start];
                while let Some(id) = queue.pop() {
                    for child in state.nodes[id].children.iter().rev() {
                        let direct =
                            state.nodes[*child].frame.name.as_deref() == Some(name.as_str());
                        let mut path = vec![];
                        let mut cursor = Some(*child);
                        while let Some(node) = cursor {
                            if let Some(n) = state.nodes[node].frame.name.as_deref() {
                                path.push(n);
                            }
                            cursor = state.nodes[node].parent;
                        }
                        path.reverse();
                        if direct || (!parts.is_empty() && path.ends_with(&parts)) {
                            found = Some(*child);
                            break;
                        }
                        if !strict {
                            queue.push(*child);
                        }
                    }
                    if found.is_some() {
                        break;
                    }
                }
                let id = found.unwrap_or_else(|| base_error("named child viewport was not found"));
                let mut path = vec![];
                let mut cur = Some(id);
                while let Some(node) = cur {
                    path.push(node);
                    cur = state.nodes[node].parent;
                }
                path.reverse();
                state.active = path;
                state.frames = state
                    .active
                    .iter()
                    .map(|node| state.nodes[*node].frame.clone())
                    .collect();
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
                let commands = draw_commands(&primitive, drawing, &frame, &*target);
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
    let regular = |n: usize, phase: f64, r: f64| {
        (0..n)
            .map(|j| {
                let a = phase + j as f64 * std::f64::consts::TAU / n as f64;
                frame.map(x + r * a.cos(), y + r * a.sin())
            })
            .collect::<Vec<_>>()
    };
    let points: Vec<_> = match code {
        // R's pch 0 is a square, 1 a circle, 2 an up triangle, 5 a diamond,
        // and 6 a down triangle.  These are outline symbols.
        0 => vec![
            frame.map(x - radius, y - radius),
            frame.map(x + radius, y - radius),
            frame.map(x + radius, y + radius),
            frame.map(x - radius, y + radius),
        ],
        1 => regular(32, 0., radius),
        2 | 17 | 24 => vec![
            frame.map(x, y + 1.5551203 * radius),
            frame.map(x + 1.3467737 * radius, y - 0.77756015 * radius),
            frame.map(x - 1.3467737 * radius, y - 0.77756015 * radius),
        ],
        5 => regular(4, 0., radius),
        6 | 25 => vec![
            frame.map(x, y - 1.5551203 * radius),
            frame.map(x + 1.3467737 * radius, y + 0.77756015 * radius),
            frame.map(x - 1.3467737 * radius, y + 0.77756015 * radius),
        ],
        // 15--20 are solid symbols; 21--25 use gp$fill for their interior.
        15 => vec![
            frame.map(x - radius, y - radius),
            frame.map(x + radius, y - radius),
            frame.map(x + radius, y + radius),
            frame.map(x - radius, y + radius),
        ],
        16 => regular(32, 0., radius),
        18 | 23 => regular(4, 0., radius),
        19 | 21 => regular(32, 0., radius),
        20 => regular(32, 0., radius * 0.25),
        22 => vec![
            frame.map(x - radius, y - radius),
            frame.map(x + radius, y - radius),
            frame.map(x + radius, y + radius),
            frame.map(x - radius, y + radius),
        ],
        _ => regular(32, 0., radius),
    };
    let mut d = polygon(frame, points, true, i);
    if let Drawing::Path(ref mut p) = d {
        match code {
            0 | 1 | 2 | 5 | 6 => p.fill = CLEAR,
            15 | 16 | 17 | 18 | 19 | 20 => {
                p.fill = alpha(frame.gp.col[i % frame.gp.col.len()], frame.gp.alpha)
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
unsafe fn draw_commands(
    kind: &str,
    data: SEXP,
    frame: &Frame,
    target: &dyn DrawTarget,
) -> Vec<Drawing> {
    unsafe {
        let default = string(field(data, "default.units"), "npc");
        let empty = |x: SEXP| x != R_NilValue() && XLENGTH(x) == 0;
        let coordinate = |name, axis, dim| units(field(data, name), &default, frame, axis, dim);
        let mut out = vec![];
        if kind == "segments" {
            if ["x0", "y0", "x1", "y1"]
                .iter()
                .any(|name| empty(field(data, name)))
            {
                return out;
            }
            let x0 = coordinate("x0", 0, false);
            let y0 = coordinate("y0", 1, false);
            let x1 = coordinate("x1", 0, false);
            let y1 = coordinate("y1", 1, false);
            if x0.is_empty() || y0.is_empty() || x1.is_empty() || y1.is_empty() {
                return out;
            }
            for i in 0..x0.len().max(y0.len()).max(x1.len()).max(y1.len()) {
                let a = frame.map(x0[i % x0.len()], y0[i % y0.len()]);
                let b = frame.map(x1[i % x1.len()], y1[i % y1.len()]);
                out.push(polygon(frame, vec![a, b], false, i));
                out.extend(arrow_heads(frame, field(data, "arrow"), a, b, i));
            }
            return out;
        }
        if empty(field(data, "x")) || empty(field(data, "y")) {
            return out;
        }
        let x = coordinate("x", 0, false);
        let y = coordinate("y", 1, false);
        if x.is_empty() || y.is_empty() {
            return out;
        }
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
                let check_overlap = num(field(data, "check.overlap"), 0.) != 0.;
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
                let mut accepted = Vec::<[[f32; 2]; 4]>::new();
                let edges_intersect = |a: [f32; 2], b: [f32; 2], c: [f32; 2], d: [f32; 2]| {
                    let cross = |u: [f32; 2], v: [f32; 2]| u[0] * v[1] - u[1] * v[0];
                    let ab = [b[0] - a[0], b[1] - a[1]];
                    let cd = [d[0] - c[0], d[1] - c[1]];
                    let denom = cross(ab, cd);
                    if denom.abs() < f32::EPSILON {
                        return false;
                    }
                    let ac = [c[0] - a[0], c[1] - a[1]];
                    let ua = cross(ac, cd) / denom;
                    let ub = cross(ac, ab) / denom;
                    ua > 0. && ua < 1. && ub > 0. && ub < 1.
                };
                let intersects = |a: [[f32; 2]; 4], b: [[f32; 2]; 4]| {
                    (0..4).any(|i| {
                        (0..4).any(|j| edges_intersect(a[i], a[(i + 1) % 4], b[j], b[(j + 1) % 4]))
                    })
                };
                for i in 0..n.max(count) {
                    let label = labels[i % count].clone();
                    let mut p = frame.map(x[i % x.len()], y[i % y.len()]);
                    p.y += ((0.5 - just[1]) * frame.gp.size) as f32;
                    if check_overlap {
                        let (width, height) = label.dimensions(target, &params);
                        let angle = f64::from(rotation).to_radians();
                        let (c, s) = (angle.cos() as f32, angle.sin() as f32);
                        let corners = [
                            [-just[0] as f32 * width, -just[1] as f32 * height],
                            [(1. - just[0]) as f32 * width, -just[1] as f32 * height],
                            [
                                (1. - just[0]) as f32 * width,
                                (1. - just[1]) as f32 * height,
                            ],
                            [-just[0] as f32 * width, (1. - just[1]) as f32 * height],
                        ]
                        .map(|q| [p.x + c * q[0] - s * q[1], p.y + s * q[0] + c * q[1]]);
                        if accepted.iter().copied().any(|old| intersects(corners, old)) {
                            continue;
                        }
                        accepted.push(corners);
                    }
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
wrapper!(do_unit_c, "unit.c", "portable_grid/unit_c.R");
wrapper!(do_ops_unit, "Ops.unit", "portable_grid/ops_unit.R");
wrapper!(
    do_summary_unit,
    "Summary.unit",
    "portable_grid/summary_unit.R"
);
wrapper!(do_is_unit, "is.unit", "portable_grid/is_unit.R");
/// Number of coordinates in the portable unit representation, not list slots.
pub(crate) unsafe fn unit_value_length(x: SEXP) -> SEXP {
    unsafe { crate::sexp::constructors::Rf_ScalarInteger(XLENGTH(field(x, "value")) as i32) }
}
wrapper!(do_length_unit, "length.unit", "portable_grid/length_unit.R");
wrapper!(do_gpar, "gpar", "portable_grid/gpar.R");
wrapper!(do_viewport, "viewport", "portable_grid/viewport.R");
wrapper!(do_vp_path, "vpPath", "portable_grid/vp_path.R");
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
    do_down_viewport,
    "downViewport",
    "portable_grid/down_viewport.R"
);
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
wrapper!(do_gpath, "gPath", "portable_grid/gpath.R");
wrapper!(do_get_grob, "getGrob", "portable_grid/get_grob.R");
wrapper!(do_edit_grob, "editGrob", "portable_grid/edit_grob.R");
wrapper!(do_is_grob, "is.grob", "portable_grid/is_grob.R");
wrapper!(do_grob_width, "grobWidth", "portable_grid/grob_width.R");
wrapper!(do_grob_height, "grobHeight", "portable_grid/grob_height.R");
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
    "unit.c",
    "Ops.unit",
    "Summary.unit",
    "is.unit",
    "length.unit",
    "gpar",
    "viewport",
    "vpPath",
    "grid.layout",
    "pushViewport",
    "popViewport",
    "upViewport",
    "downViewport",
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
    "gPath",
    "getGrob",
    "editGrob",
    "is.grob",
    "grobWidth",
    "grobHeight",
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

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    #[test]
    fn malformed_grid_snapshots_are_rejected_without_panicking() {
        let mut state = GridState::default();
        state.ensure((200, 100));
        let valid = serde_json::to_value(&state).unwrap();
        assert!(GridState::decode_snapshot(&serde_json::to_vec(&valid).unwrap()).is_ok());
        for (path, value) in [
            ("/nodes/0/parent", serde_json::json!(999999)),
            ("/nodes/0/children", serde_json::json!([0])),
            ("/active/0", serde_json::json!(100)),
            ("/frames/0/width", serde_json::json!(300)),
            ("/nodes/0/frame/gp/col", serde_json::json!([])),
            ("/nodes/0/frame/scale", serde_json::json!([[0, 0], [0, 1]])),
            ("/nodes/0/frame/gp/lineheight", serde_json::json!(-1)),
        ] {
            let mut bad = valid.clone();
            *bad.pointer_mut(path).unwrap() = value;
            assert!(
                GridState::decode_snapshot(&serde_json::to_vec(&bad).unwrap()).is_err(),
                "{path}"
            );
        }
        let mut bad = valid.clone();
        let duplicate = bad["nodes"][0].clone();
        bad["nodes"].as_array_mut().unwrap().push(duplicate);
        assert!(GridState::decode_snapshot(&serde_json::to_vec(&bad).unwrap()).is_err());
        assert!(
            GridState::decode_snapshot(&GridState::default().encode_snapshot().unwrap())
                .unwrap()
                .is_empty()
        );
    }
}
