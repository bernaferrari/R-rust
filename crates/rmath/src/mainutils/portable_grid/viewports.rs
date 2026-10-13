use super::*;

pub(crate) fn alpha(mut c: Color, a: f64) -> Color {
    c.a = (c.a as f64 * a).round() as u8;
    c
}
pub(crate) fn style_path(
    frame: &Frame,
    commands: Vec<PathCommand>,
    i: usize,
    filled: bool,
) -> Path {
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
pub(crate) unsafe fn justification(x: SEXP) -> [f64; 2] {
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
pub(crate) fn compose(a: [f64; 6], b: [f64; 6]) -> [f64; 6] {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}
pub(crate) unsafe fn layout_axis(x: SEXP, n: usize, frame: &Frame, axis: usize) -> layout::Axis {
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
pub(crate) unsafe fn push(data: SEXP, parent: &Frame) -> Frame {
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
            anchor_x: x,
            anchor_y: y,
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

pub(crate) fn named_children(state: &GridState, id: usize) -> Vec<usize> {
    // GNU keeps one child per name (the latest push) and searches `ls()` order.
    let mut last: Vec<(String, usize)> = Vec::new();
    for &child in &state.nodes[id].children {
        let Some(name) = state.nodes[child].frame.name.clone() else {
            continue;
        };
        if let Some(pos) = last.iter().position(|(existing, _)| existing == &name) {
            last[pos].1 = child;
        } else {
            last.push((name, child));
        }
    }
    last.sort_by(|a, b| a.0.cmp(&b.0));
    last.into_iter().map(|(_, child)| child).collect()
}

pub(crate) fn path_match(path: &str, so_far: Option<&str>, strict: bool) -> bool {
    // grid::pathMatch uses grepl. Strict anchors the pattern; otherwise it is unanchored.
    let Some(so_far) = so_far else {
        return false;
    };
    let pattern = if strict {
        format!("^{path}$")
    } else {
        path.to_string()
    };
    let expression = fancy_regex::Regex::new(&pattern).unwrap_or_else(|_| {
        base_error(format!("invalid viewport path regular expression '{path}'"))
    });
    expression.is_match(so_far).unwrap_or_else(|_| {
        base_error(format!("invalid viewport path regular expression '{path}'"))
    })
}

pub(crate) fn find_name(
    state: &GridState,
    vp: usize,
    name: &str,
    strict: bool,
    depth: i32,
) -> Option<(usize, i32)> {
    let children = named_children(state, vp);
    if children.is_empty() {
        return None;
    }
    if let Some(child) = children
        .iter()
        .copied()
        .find(|&child| state.nodes[child].frame.name.as_deref() == Some(name))
    {
        return Some((child, depth));
    }
    if strict {
        return None;
    }
    children
        .into_iter()
        .find_map(|child| find_name(state, child, name, strict, depth + 1))
}

pub(crate) fn find_vppath(
    state: &GridState,
    vp: usize,
    path: &str,
    name: &str,
    strict: bool,
    path_so_far: Option<&str>,
    depth: i32,
) -> Option<(usize, i32)> {
    let children = named_children(state, vp);
    if children.is_empty() {
        return None;
    }
    if children
        .iter()
        .any(|&child| state.nodes[child].frame.name.as_deref() == Some(name))
        && path_match(path, path_so_far, strict)
    {
        let child = children
            .into_iter()
            .find(|child| state.nodes[*child].frame.name.as_deref() == Some(name))
            .unwrap_or_else(|| base_error("viewport path child was not found"));
        return Some((child, depth));
    }
    for child in children {
        let child_name = state.nodes[child].frame.name.clone().unwrap_or_default();
        let grown = match path_so_far {
            None => child_name,
            Some(so_far) => format!("{so_far}::{child_name}"),
        };
        if let Some(found) = find_vppath(
            state,
            child,
            path,
            name,
            strict,
            Some(grown.as_str()),
            depth + 1,
        ) {
            return Some(found);
        }
    }
    None
}

pub(crate) fn activate(state: &mut GridState, id: usize) {
    let mut path = Vec::new();
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

pub(crate) fn current_path(state: &GridState) -> Option<String> {
    let mut names = Vec::new();
    let mut id = *state
        .active
        .last()
        .unwrap_or_else(|| base_error("viewport stack is empty"));
    while state.nodes[id].parent.is_some() {
        if let Some(name) = state.nodes[id].frame.name.clone() {
            names.push(name);
        }
        id = state.nodes[id].parent.unwrap();
    }
    names.reverse();
    if names.is_empty() {
        None
    } else {
        Some(names.join("::"))
    }
}

pub(crate) fn current_name(state: &GridState) -> String {
    let id = *state
        .active
        .last()
        .unwrap_or_else(|| base_error("viewport stack is empty"));
    if state.nodes[id].parent.is_none() {
        "ROOT".to_string()
    } else {
        state.nodes[id]
            .frame
            .name
            .clone()
            .unwrap_or_else(|| "ROOT".to_string())
    }
}

pub(crate) fn current_inches(state: &GridState) -> [f64; 4] {
    let frame = state
        .frames
        .last()
        .unwrap_or_else(|| base_error("viewport stack is empty"));
    let (x_px, y_px) = if state.frames.len() == 1 {
        (frame.anchor_x, frame.anchor_y)
    } else {
        let parent = &state.frames[state.frames.len() - 2];
        let m = parent.matrix;
        let x = m[0] * frame.anchor_x + m[2] * frame.anchor_y + m[4];
        let y_down = m[1] * frame.anchor_x + m[3] * frame.anchor_y + m[5];
        (x, state.dimensions.1 as f64 - y_down)
    };
    [
        x_px / DPI,
        y_px / DPI,
        frame.width / DPI,
        frame.height / DPI,
    ]
}

pub(crate) fn listing_indent(depth: f64) -> String {
    // R's rep() truncates a fractional depth toward 0.
    let steps = if depth.is_finite() && depth > 0. {
        depth.trunc() as usize
    } else {
        0
    };
    "  ".repeat(steps)
}

pub(crate) fn listing_count(n: f64) -> String {
    if n.is_finite() && n.fract() == 0. && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

pub(crate) fn listing_label(class_name: &str, name: &str, full_names: bool) -> String {
    if full_names {
        format!("{class_name}[{name}]")
    } else {
        name.to_string()
    }
}

pub(crate) fn retreat_listing_depth(depth: &mut f64, n: f64) {
    // The printed count is subtracted as a real number. Integer pops land on
    // an integer depth; popViewport(0.1) from depth 5 leaves 4.9, which
    // indents like 4.
    *depth -= n;
}

pub(crate) fn append_listing(
    grob: &ListedGrob,
    depth: f64,
    full_names: bool,
    viewports: bool,
    grobs: bool,
    out: &mut Vec<String>,
) {
    let down = viewports && !grob.vp_down.is_empty();
    let viewport = viewports && !down && !grob.vp_name.is_empty();
    let vp_depth = if down {
        grob.vp_down.len() as f64
    } else if viewport {
        1.
    } else {
        0.
    };
    if down {
        for (i, part) in grob.vp_down.iter().enumerate() {
            out.push(format!(
                "{}{}",
                listing_indent(depth + i as f64),
                listing_label("downViewport", part, full_names)
            ));
        }
    } else if viewport {
        out.push(format!(
            "{}{}",
            listing_indent(depth),
            listing_label("viewport", &grob.vp_name, full_names)
        ));
    }
    let content_depth = depth + vp_depth;
    if grobs {
        append_grob_line(grob, content_depth, full_names, viewports, out);
    } else {
        // An omitted grob does not add an indent level. Its viewport path still prints.
        for child in &grob.children {
            append_listing(child, content_depth, full_names, viewports, false, out);
        }
    }
    if down || viewport {
        let count = if down {
            grob.vp_down.len().to_string()
        } else {
            "1".to_string()
        };
        out.push(format!(
            "{}{}",
            listing_indent(content_depth),
            listing_label("upViewport", &count, full_names)
        ));
    }
}

pub(crate) fn append_grob_line(
    grob: &ListedGrob,
    depth: f64,
    full_names: bool,
    viewports: bool,
    out: &mut Vec<String>,
) {
    let label = if full_names && !grob.class_name.is_empty() {
        listing_label(&grob.class_name, &grob.name, true)
    } else {
        grob.name.clone()
    };
    out.push(format!("{}{label}", listing_indent(depth)));
    for child in &grob.children {
        append_listing(child, depth + 1., full_names, viewports, true, out);
    }
}

pub(crate) fn append_display_list(
    entries: &[DlEntry],
    full_names: bool,
    viewports: bool,
    grobs: bool,
    out: &mut Vec<String>,
) {
    if viewports {
        out.push(listing_label("viewport", "ROOT", full_names));
        let mut depth = 1.0_f64;
        for entry in entries {
            match entry {
                DlEntry::Grob { grob } => {
                    append_listing(grob, depth, full_names, true, grobs, out);
                }
                DlEntry::Push { name } => {
                    out.push(format!(
                        "{}{}",
                        listing_indent(depth),
                        listing_label("viewport", name, full_names)
                    ));
                    depth += 1.;
                }
                DlEntry::Pop { n } => {
                    out.push(format!(
                        "{}{}",
                        listing_indent(depth),
                        listing_label("popViewport", &listing_count(*n), full_names)
                    ));
                    retreat_listing_depth(&mut depth, *n);
                }
                DlEntry::Up { n } => {
                    out.push(format!(
                        "{}{}",
                        listing_indent(depth),
                        listing_label("upViewport", &listing_count(*n), full_names)
                    ));
                    retreat_listing_depth(&mut depth, *n);
                }
                DlEntry::Down { name } => {
                    out.push(format!(
                        "{}{}",
                        listing_indent(depth),
                        listing_label("downViewport", name, full_names)
                    ));
                    depth += 1.;
                }
            }
        }
    } else if grobs {
        for entry in entries {
            if let DlEntry::Grob { grob } = entry {
                append_listing(grob, 0., full_names, false, true, out);
            }
        }
    }
}

pub(crate) unsafe fn listed_grob(x: SEXP) -> ListedGrob {
    unsafe {
        if x == R_NilValue() || TYPEOF(x) != SEXPTYPE::VECSXP {
            base_error("grid.draw requires a grob");
        }
        let mut children = Vec::new();
        let kids = field(x, "children");
        if kids != R_NilValue() && TYPEOF(kids) == SEXPTYPE::VECSXP {
            for i in 0..XLENGTH(kids) {
                children.push(listed_grob(VECTOR_ELT(kids, i)));
            }
        }
        let (vp_name, vp_down) = listed_viewport(field(x, "vp"));
        ListedGrob {
            name: string(field(x, "name"), ""),
            class_name: grob_listing_class(x),
            vp_name,
            vp_down,
            children,
        }
    }
}

pub(crate) unsafe fn listed_viewport(vp: SEXP) -> (String, Vec<String>) {
    unsafe {
        if vp == R_NilValue() {
            return (String::new(), Vec::new());
        }
        if TYPEOF(vp) == SEXPTYPE::STRSXP {
            let parts = string(vp, "")
                .split("::")
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect();
            return (String::new(), parts);
        }
        if TYPEOF(vp) == SEXPTYPE::VECSXP {
            return (string(field(vp, "name"), ""), Vec::new());
        }
        (String::new(), Vec::new())
    }
}

pub(crate) unsafe fn grob_listing_class(x: SEXP) -> String {
    unsafe {
        let class = getAttrib(x, R_ClassSymbol());
        if class == R_NilValue() || TYPEOF(class) != SEXPTYPE::STRSXP || XLENGTH(class) < 1 {
            String::new()
        } else {
            elt_to_string(class, 0)
        }
    }
}

pub(crate) unsafe fn strings(values: &[String]) -> SEXP {
    unsafe {
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, values.len() as i64);
        let _guard = crate::sexp::protect::protect(out);
        for (i, value) in values.iter().enumerate() {
            let text = std::ffi::CString::new(value.as_str())
                .unwrap_or_else(|_| base_error("grid listing contains an embedded nul"));
            SET_STRING_ELT(out, i as i64, Rf_mkChar(text.as_ptr()));
        }
        out
    }
}

pub(crate) unsafe fn logical_flag(x: SEXP, default: bool) -> bool {
    unsafe {
        if x == R_NilValue() || XLENGTH(x) == 0 {
            return default;
        }
        match SEXPTYPE(TYPEOF(x)) {
            SEXPTYPE::LGLSXP => *LOGICAL(x) != 0,
            SEXPTYPE::INTSXP => *INTEGER(x) != 0,
            SEXPTYPE::REALSXP => {
                let value = *REAL(x);
                value.is_nan() || value != 0.
            }
            _ => base_error("invalid 'strict' value"),
        }
    }
}

pub(crate) unsafe fn mk_string(value: &str) -> SEXP {
    unsafe {
        let text = std::ffi::CString::new(value)
            .unwrap_or_else(|_| base_error("viewport name contains an embedded nul"));
        Rf_mkString(text.as_ptr())
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
        let mut returned = R_NilValue();
        match operation.as_str() {
            "newpage" => {
                // recording=FALSE still clears the page; only the display list is kept.
                let kept = if logical_flag(data, true) {
                    Vec::new()
                } else {
                    std::mem::take(&mut state.display_list)
                };
                state = GridState::default();
                state.ensure(dims);
                state.display_list = kept;
                (&mut *target).clear(Color::WHITE);
            }
            "push" => {
                let frame = push(data, &parent);
                let parent_id = *state.active.last().unwrap();
                let mut replaced = false;
                if let Some(name) = frame.name.clone() {
                    let keep: Vec<usize> = state.nodes[parent_id]
                        .children
                        .iter()
                        .copied()
                        .filter(|&child| {
                            state.nodes[child].frame.name.as_deref() != Some(name.as_str())
                        })
                        .collect();
                    replaced = keep.len() != state.nodes[parent_id].children.len();
                    state.nodes[parent_id].children = keep;
                }
                state.frames.push(frame.clone());
                let id = state.nodes.len();
                state.nodes.push(ViewportNode {
                    parent: Some(parent_id),
                    frame,
                    children: vec![],
                });
                state.nodes[parent_id].children.push(id);
                state.active.push(id);
                if replaced {
                    state.compact_tree();
                }
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
                    base_error(
                        "cannot pop the top-level viewport ('grid' and 'graphics' output mixed?)",
                    );
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
                    base_error(
                        "cannot pop the top-level viewport ('grid' and 'graphics' output mixed?)",
                    );
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
                let parts = name
                    .split("::")
                    .filter(|part| !part.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                if parts.is_empty() {
                    base_error("a viewport path must contain at least one viewport name");
                }
                let strict = logical_flag(field(data, "strict"), false);
                let start = *state
                    .active
                    .last()
                    .unwrap_or_else(|| base_error("viewport stack is empty"));
                let final_name = parts.last().cloned().unwrap_or_else(|| {
                    base_error("a viewport path must contain at least one viewport name")
                });
                let found = if parts.len() == 1 {
                    find_name(&state, start, &final_name, strict, 1)
                } else {
                    let path = parts[..parts.len() - 1].join("::");
                    find_vppath(&state, start, &path, &final_name, strict, None, 1)
                };
                let (id, depth) = found.unwrap_or_else(|| {
                    base_error(format!("Viewport '{final_name}' was not found"));
                });
                activate(&mut state, id);
                returned = Rf_ScalarInteger(depth);
            }
            "current.name" => returned = mk_string(&current_name(&state)),
            "current.inches" => returned = result(&current_inches(&state)),
            "record" => state.display_list.push(DlEntry::Grob {
                grob: listed_grob(data),
            }),
            "record.push" => state.display_list.push(DlEntry::Push {
                name: string(data, ""),
            }),
            "record.pop" => state.display_list.push(DlEntry::Pop { n: num(data, 1.) }),
            "record.up" => state.display_list.push(DlEntry::Up { n: num(data, 1.) }),
            "record.down" => {
                let path = string(data, "");
                for part in path.split("::").filter(|part| !part.is_empty()) {
                    state.display_list.push(DlEntry::Down {
                        name: part.to_string(),
                    });
                }
            }
            "ls" => {
                let full_names = logical_flag(field(data, "fullNames"), false);
                let viewports = logical_flag(field(data, "viewports"), false);
                let grobs = logical_flag(field(data, "grobs"), true);
                let mut lines = Vec::new();
                append_display_list(
                    &state.display_list,
                    full_names,
                    viewports,
                    grobs,
                    &mut lines,
                );
                returned = strings(&lines);
            }
            "ls.grob" => {
                let full_names = logical_flag(field(data, "fullNames"), false);
                let viewports = logical_flag(field(data, "viewports"), false);
                let grobs = logical_flag(field(data, "grobs"), true);
                let grob = listed_grob(field(data, "x"));
                let mut lines = Vec::new();
                append_listing(&grob, 0., full_names, viewports, grobs, &mut lines);
                returned = strings(&lines);
            }
            "current.path" => {
                returned = match current_path(&state) {
                    Some(path) => mk_string(&path),
                    None => R_NilValue(),
                };
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
        returned
    }
}
