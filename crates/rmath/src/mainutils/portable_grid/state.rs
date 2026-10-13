use super::*;

pub(crate) const DPI: f64 = 96.;
pub(crate) const CLEAR: Color = Color {
    r: 0,
    g: 0,
    b: 0,
    a: 0,
};
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Gp {
    pub(crate) col: Vec<Color>,
    pub(crate) fill: Vec<Color>,
    pub(crate) width: f64,
    pub(crate) size: f64,
    pub(crate) alpha: f64,
    pub(crate) face: r_graphics_engine::FontFace,
    pub(crate) dash: Option<DashPattern>,
    pub(crate) lineheight: f64,
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
pub(crate) struct Frame {
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) matrix: [f64; 6],
    pub(crate) scale: [[f64; 2]; 2],
    pub(crate) clip: Option<[f32; 4]>,
    pub(crate) gp: Gp,
    pub(crate) layout: Option<Layout>,
    pub(crate) name: Option<String>,
    // Parent-user pixels of the viewport anchor. Justification moves `matrix`, not this point.
    #[serde(default)]
    pub(crate) anchor_x: f64,
    #[serde(default)]
    pub(crate) anchor_y: f64,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Layout {
    pub(crate) widths: Vec<f64>,
    pub(crate) heights: Vec<f64>,
    pub(crate) offset_x: f64,
    pub(crate) offset_y: f64,
}
#[derive(Clone, Serialize, Deserialize, Default)]
pub(crate) struct GridState {
    #[serde(default)]
    pub(crate) version: u8,
    pub(crate) frames: Vec<Frame>,
    pub(crate) nodes: Vec<ViewportNode>,
    pub(crate) active: Vec<usize>,
    pub(crate) dimensions: (u32, u32),
    // Grobs drawn with recording=TRUE. newpage(recording=FALSE) keeps this list.
    #[serde(default)]
    pub(crate) display_list: Vec<DlEntry>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct ListedGrob {
    pub(crate) name: String,
    // First class, so fullNames can print text[label]. Old recordings omit it.
    #[serde(default)]
    pub(crate) class_name: String,
    // Viewport object name. Empty means the grob has no viewport() slot.
    #[serde(default)]
    pub(crate) vp_name: String,
    // Character or vpPath components. These print as downViewport, not viewport.
    #[serde(default)]
    pub(crate) vp_down: Vec<String>,
    #[serde(default)]
    pub(crate) children: Vec<ListedGrob>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub(crate) enum DlEntry {
    Grob { grob: ListedGrob },
    Push { name: String },
    Pop { n: f64 },
    Up { n: f64 },
    Down { name: String },
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct ViewportNode {
    pub(crate) parent: Option<usize>,
    pub(crate) frame: Frame,
    pub(crate) children: Vec<usize>,
}
impl GridState {
    pub(crate) fn compact_tree(&mut self) {
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
        // Root anchor is device pixels; current.inches reads it without the
        // matrix. Nested anchors stay in parent-user pixels, and the parent
        // matrix scale already applies this resize.
        if let Some(root) = self.frames.first_mut() {
            root.anchor_x *= sx;
            root.anchor_y *= sy;
        }
        if let Some(root) = self.nodes.first_mut() {
            root.frame.anchor_x *= sx;
            root.frame.anchor_y *= sy;
        }
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
    pub(crate) fn ensure(&mut self, dims: (u32, u32)) {
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
                anchor_x: dims.0 as f64 / 2.,
                anchor_y: dims.1 as f64 / 2.,
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
    pub(crate) fn valid(&self) -> bool {
        let gp = &self.gp;
        self.width.is_finite()
            && self.width >= 0.
            && self.height.is_finite()
            && self.height >= 0.
            && self.anchor_x.is_finite()
            && self.anchor_y.is_finite()
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
    pub(crate) fn map(&self, x: f64, y: f64) -> Point {
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
    pub(crate) fn extent(&self, axis: usize) -> f64 {
        if axis == 0 { self.width } else { self.height }
    }
    pub(crate) fn unit_factor(
        &self,
        u: &str,
        axis: usize,
        dimension: bool,
    ) -> Result<(f64, f64), String> {
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
pub(crate) fn device() -> *mut dyn DrawTarget {
    with_required_current_instance(|p| unsafe { (*p).current_renderplot_backend })
        .unwrap_or_else(|| base_error("grid requires an active graphics device"))
}
pub(crate) unsafe fn field(x: SEXP, name: &str) -> SEXP {
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
pub(crate) unsafe fn numbers(x: SEXP) -> Vec<f64> {
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
pub(crate) unsafe fn num(x: SEXP, default: f64) -> f64 {
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
pub(crate) unsafe fn string(x: SEXP, default: &str) -> String {
    unsafe {
        if x == R_NilValue() || XLENGTH(x) == 0 {
            default.into()
        } else {
            elt_to_string(x, 0)
        }
    }
}
pub(crate) unsafe fn result(v: &[f64]) -> SEXP {
    unsafe {
        let x = Rf_allocVector3(SEXPTYPE::REALSXP, v.len() as i64);
        for (i, value) in v.iter().enumerate() {
            *REAL(x).add(i) = *value;
        }
        x
    }
}
pub(crate) fn plot_params(style: &Gp) -> PlotParameters {
    PlotParameters {
        font_face: style.face,
        font_size: style.size as f32,
        text_color: Color::BLACK,
        dpi: DPI as f32,
        text_anchor: TextAnchor::Start,
        text_angle: 0.,
    }
}
/// Width and height, in device pixels, of one label at `index`.
/// A list (stringWidth's data) selects one element; a character or
/// expression vector selects one label. An empty string still has font height.
pub(crate) unsafe fn label_box(item: SEXP, index: usize, style: &Gp) -> (f64, f64) {
    unsafe {
        if item == R_NilValue() {
            base_error("data is required for stringWidth/stringHeight units");
        }
        let params = plot_params(style);
        let owned = if TYPEOF(item) == SEXPTYPE::VECSXP {
            if XLENGTH(item) == 0 {
                return (0., 0.);
            }
            crate::mainutils::plotmath::labels(VECTOR_ELT(item, (index as i64) % XLENGTH(item)))
        } else {
            crate::mainutils::plotmath::labels(item)
        };
        if owned.is_empty() {
            return (0., 0.);
        }
        let label = if TYPEOF(item) == SEXPTYPE::VECSXP {
            &owned[0]
        } else {
            &owned[index % owned.len()]
        };
        let (w, h) = label.dimensions(&*device(), &params);
        let (w, h) = (f64::from(w), f64::from(h));
        if !w.is_finite() || !h.is_finite() {
            base_error("grid unit conversion overflow");
        }
        (w, h)
    }
}
/// GNU sizes text by the positions actually drawn, which is
/// max(length(x), length(y)), recycling labels. One shared anchor therefore
/// measures only the first label. The result is the axis-aligned box after
/// the grob's own rotation, using the grob gpar over the viewport gpar.
pub(crate) unsafe fn text_grob_extent(grob: SEXP, data: SEXP, frame: &Frame, axis: usize) -> f64 {
    unsafe {
        let style = gp(field(grob, "gp"), &frame.gp);
        let labels = field(data, "label");
        if labels == R_NilValue() || XLENGTH(labels) == 0 {
            return 0.;
        }
        let xs = units(field(data, "x"), "npc", frame, 0, false);
        let ys = units(field(data, "y"), "npc", frame, 1, false);
        if xs.is_empty() || ys.is_empty() {
            base_error("grid text requires coordinates");
        }
        let n = xs.len().max(ys.len());
        let mut just = justification(field(data, "just"));
        just[0] = num(field(data, "hjust"), just[0]);
        just[1] = num(field(data, "vjust"), just[1]);
        let rot = num(field(data, "rot"), 0.).to_radians();
        let (c, s) = (rot.cos(), rot.sin());
        let mut xmin = f64::INFINITY;
        let mut xmax = f64::NEG_INFINITY;
        let mut ymin = f64::INFINITY;
        let mut ymax = f64::NEG_INFINITY;
        for i in 0..n {
            let (w, h) = label_box(labels, i, &style);
            let x = xs[i % xs.len()];
            let y = ys[i % ys.len()];
            for (cx, cy) in [(0., 0.), (w, 0.), (w, h), (0., h)] {
                let px = cx - just[0] * w;
                let py = cy - just[1] * h;
                let rx = c * px - s * py + x;
                let ry = s * px + c * py + y;
                xmin = xmin.min(rx);
                xmax = xmax.max(rx);
                ymin = ymin.min(ry);
                ymax = ymax.max(ry);
            }
        }
        let extent = if axis == 0 { xmax - xmin } else { ymax - ymin };
        if !extent.is_finite() {
            base_error("grid unit conversion overflow");
        }
        extent
    }
}
pub(crate) unsafe fn grob_extent(grob: SEXP, frame: &Frame, axis: usize) -> f64 {
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
            ("text", _) => text_grob_extent(grob, data, frame, axis),
            _ => base_error(format!("grob unit is not supported for primitive '{kind}'")),
        }
    }
}
pub(crate) unsafe fn units(
    x: SEXP,
    default: &str,
    frame: &Frame,
    axis: usize,
    dimension: bool,
) -> Vec<f64> {
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
                if name == ".rport-expression" {
                    if TYPEOF(data) != SEXPTYPE::VECSXP || XLENGTH(data) == 0 {
                        base_error("invalid grid expression data");
                    }
                    let child = VECTOR_ELT(data, (i as i64) % XLENGTH(data));
                    let resolved = units(child, default, frame, axis, dimension);
                    if resolved.len() != 1 {
                        base_error("grid coordinate expression must resolve to one value");
                    }
                    return v * resolved[0];
                }
                if matches!(name.as_str(), "sum" | "min" | "max")
                    && TYPEOF(data) == SEXPTYPE::VECSXP
                    && XLENGTH(data) > 0
                {
                    let mut parts = Vec::new();
                    for j in 0..XLENGTH(data) {
                        parts.push(units(VECTOR_ELT(data, j), default, frame, axis, dimension));
                    }
                    let terms = parts.into_iter().flatten();
                    return v * match name.as_str() {
                        "sum" => terms.sum(),
                        "min" => terms.fold(f64::INFINITY, f64::min),
                        _ => terms.fold(f64::NEG_INFINITY, f64::max),
                    };
                }
                if name == "grobwidth" || name == "grobheight" {
                    if data == R_NilValue() {
                        base_error("grob units require grob data");
                    }
                    // grob_extent is already device pixels. convertWidth divides
                    // by the destination unit, so do not also divide by DPI here.
                    let extent = grob_extent(data, frame, if name == "grobwidth" { 0 } else { 1 });
                    let resolved = v * extent;
                    if !resolved.is_finite() {
                        base_error("grid unit conversion overflow");
                    }
                    return resolved;
                }
                if name == "strwidth" || name == "strheight" {
                    // Measured advances are device pixels. The strwidth factor
                    // below is a 0.6em stand-in and must not scale them again.
                    let (width, height) = label_box(data, i, &frame.gp);
                    let resolved = v * if name == "strwidth" { width } else { height };
                    if !resolved.is_finite() {
                        base_error("grid unit conversion overflow");
                    }
                    return resolved;
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
pub(crate) unsafe fn color_values(x: SEXP, old: &[Color]) -> Vec<Color> {
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
pub(crate) unsafe fn gp(x: SEXP, old: &Gp) -> Gp {
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
