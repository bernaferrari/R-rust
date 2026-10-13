use super::*;

pub(crate) enum Drawing {
    Path(Path),
    Text(crate::mainutils::plotmath::Label, Point, PlotParameters),
}
pub(crate) fn polygon(frame: &Frame, points: Vec<Point>, closed: bool, i: usize) -> Drawing {
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

pub(crate) fn point_symbol(
    frame: &Frame,
    x: f64,
    y: f64,
    size: f64,
    pch: f64,
    i: usize,
) -> Drawing {
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

pub(crate) unsafe fn arrow_heads(
    frame: &Frame,
    arrow: SEXP,
    a: Point,
    b: Point,
    i: usize,
) -> Vec<Drawing> {
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
pub(crate) unsafe fn draw_commands(
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
wrapper!(do_unit, "unit", "../portable_grid/unit.R");
wrapper!(do_unit_c, "unit.c", "../portable_grid/unit_c.R");
wrapper!(do_ops_unit, "Ops.unit", "../portable_grid/ops_unit.R");
wrapper!(
    do_summary_unit,
    "Summary.unit",
    "../portable_grid/summary_unit.R"
);
wrapper!(do_is_unit, "is.unit", "../portable_grid/is_unit.R");
/// Number of coordinates in the portable unit representation, not list slots.
pub(crate) unsafe fn unit_value_length(x: SEXP) -> SEXP {
    unsafe { crate::sexp::constructors::Rf_ScalarInteger(XLENGTH(field(x, "value")) as i32) }
}
wrapper!(
    do_length_unit,
    "length.unit",
    "../portable_grid/length_unit.R"
);
wrapper!(do_gpar, "gpar", "../portable_grid/gpar.R");
wrapper!(do_viewport, "viewport", "../portable_grid/viewport.R");
wrapper!(do_vp_path, "vpPath", "../portable_grid/vp_path.R");
wrapper!(
    do_grid_layout,
    "grid.layout",
    "../portable_grid/grid_layout.R"
);
wrapper!(
    do_push_viewport,
    "pushViewport",
    "../portable_grid/push_viewport.R"
);
wrapper!(
    do_pop_viewport,
    "popViewport",
    "../portable_grid/pop_viewport.R"
);
wrapper!(
    do_up_viewport,
    "upViewport",
    "../portable_grid/up_viewport.R"
);
wrapper!(
    do_down_viewport,
    "downViewport",
    "../portable_grid/down_viewport.R"
);
wrapper!(
    do_seek_viewport,
    "seekViewport",
    "../portable_grid/seek_viewport.R"
);
wrapper!(
    do_grid_newpage,
    "grid.newpage",
    "../portable_grid/grid_newpage.R"
);
wrapper!(do_convert_x, "convertX", "../portable_grid/convert_x.R");
wrapper!(do_convert_y, "convertY", "../portable_grid/convert_y.R");
wrapper!(
    do_convert_width,
    "convertWidth",
    "../portable_grid/convert_width.R"
);
wrapper!(
    do_convert_height,
    "convertHeight",
    "../portable_grid/convert_height.R"
);
wrapper!(do_grid_draw, "grid.draw", "../portable_grid/grid_draw.R");
wrapper!(do_glist, "gList", "../portable_grid/glist.R");
wrapper!(do_gtree, "gTree", "../portable_grid/gtree.R");
wrapper!(do_grobtree, "grobTree", "../portable_grid/grobtree.R");
wrapper!(do_gpath, "gPath", "../portable_grid/gpath.R");
wrapper!(do_get_grob, "getGrob", "../portable_grid/get_grob.R");
wrapper!(do_edit_grob, "editGrob", "../portable_grid/edit_grob.R");
wrapper!(do_is_grob, "is.grob", "../portable_grid/is_grob.R");
wrapper!(do_grob_width, "grobWidth", "../portable_grid/grob_width.R");
wrapper!(
    do_grob_height,
    "grobHeight",
    "../portable_grid/grob_height.R"
);
wrapper!(do_rect_grob, "rectGrob", "../portable_grid/rect_grob.R");
wrapper!(do_grid_rect, "grid.rect", "../portable_grid/grid_rect.R");
wrapper!(
    do_circle_grob,
    "circleGrob",
    "../portable_grid/circle_grob.R"
);
wrapper!(
    do_grid_circle,
    "grid.circle",
    "../portable_grid/grid_circle.R"
);
wrapper!(do_lines_grob, "linesGrob", "../portable_grid/lines_grob.R");
wrapper!(do_grid_lines, "grid.lines", "../portable_grid/grid_lines.R");
wrapper!(
    do_polygon_grob,
    "polygonGrob",
    "../portable_grid/polygon_grob.R"
);
wrapper!(
    do_grid_polygon,
    "grid.polygon",
    "../portable_grid/grid_polygon.R"
);
wrapper!(
    do_segments_grob,
    "segmentsGrob",
    "../portable_grid/segments_grob.R"
);
wrapper!(
    do_grid_segments,
    "grid.segments",
    "../portable_grid/grid_segments.R"
);
wrapper!(do_text_grob, "textGrob", "../portable_grid/text_grob.R");
wrapper!(do_grid_text, "grid.text", "../portable_grid/grid_text.R");
wrapper!(
    do_points_grob,
    "pointsGrob",
    "../portable_grid/points_grob.R"
);
wrapper!(
    do_grid_points,
    "grid.points",
    "../portable_grid/grid_points.R"
);

/// Exported portable grid surface; namespace lookup is restricted to these names.
pub(crate) const EXPORTS: &[&str] = &[
    "unit",
    "unit.c",
    "Ops.unit",
    "Summary.unit",
    "is.unit",
    "editDetails",
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
    "current.viewport",
    "current.vpPath",
    "grid.newpage",
    "grid.ls",
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
    "stringWidth",
    "stringHeight",
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

pub(crate) unsafe fn new_grid_environment() -> SEXP {
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
            let value = if let Some(source) = grid_closure_source(name) {
                let parser_factory = crate::eval::parser::active_factory();
                let parsed = crate::sexp::memory::with_arena(|arena| {
                    crate::eval::parser::parse(source, arena, parser_factory.clone())
                })
                .expect("checked-in grid function must parse");
                crate::eval::eval::Rf_eval(parsed.clone().as_raw(), env)
            } else {
                crate::eval::primitive::make_primitive_binding(name, SEXPTYPE::BUILTINSXP)
            };
            let _value_guard = protect(value);
            defineVar(symbol, value, env);
        }
        let auto_name = std::ffi::CString::new(".rport.vp.autoname").expect("static grid counter");
        let auto_symbol = Rf_install(auto_name.as_ptr());
        let auto_value = crate::sexp::constructors::Rf_ScalarInteger(0);
        let _auto_value = protect(auto_value);
        defineVar(auto_symbol, auto_value, env);
        env
    }
}

pub(crate) fn grid_closure_source(name: &str) -> Option<&'static str> {
    match name {
        "editDetails" => Some(include_str!("../portable_grid/edit_details.R")),
        "current.viewport" => Some(include_str!("../portable_grid/current_viewport.R")),
        "current.vpPath" => Some(include_str!("../portable_grid/current_vp_path.R")),
        "grid.ls" => Some(include_str!("../portable_grid/grid_ls.R")),
        "stringWidth" => Some(include_str!("../portable_grid/string_width.R")),
        "stringHeight" => Some(include_str!("../portable_grid/string_height.R")),
        _ => None,
    }
}
pub(crate) unsafe fn namespace() -> SEXP {
    unsafe {
        let cached = with_required_current_instance(|p| {
            (*p).package_namespace_cache
                .get("grid")
                .and_then(|(dir, env)| (dir.as_os_str() == "<builtin:grid>").then_some(*env))
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
