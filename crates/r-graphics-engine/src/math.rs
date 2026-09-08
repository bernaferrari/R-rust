//! Owned mathematical typesetting. Coordinates use a baseline and a downward Y axis.
use crate::{DrawTarget, Path, PathCommand, PlotParameters, Point, Stroke, TextAnchor};

#[derive(Clone, Debug, PartialEq)]
pub enum MathExpr {
    Text(String),
    Variable(String),
    Upright(String),
    Row(Vec<MathExpr>),
    Fraction(Box<MathExpr>, Box<MathExpr>),
    Atop(Box<MathExpr>, Box<MathExpr>),
    Style(Box<MathExpr>, crate::FontFace),
    Scripts {
        base: Box<MathExpr>,
        sub: Option<Box<MathExpr>>,
        sup: Option<Box<MathExpr>>,
    },
    Radical(Box<MathExpr>),
    Accent(Box<MathExpr>, String),
    Underline(Box<MathExpr>),
    Space(f32),
    Phantom(Box<MathExpr>),
    /// A delimiter pair whose ink grows to contain the body.
    BGroup {
        left: String,
        body: Box<MathExpr>,
        right: String,
    },
    /// An accent whose rule/tilde is widened to the body's ink width.
    WideAccent(Box<MathExpr>, String),
    /// A display operator with limits centered above and below the symbol.
    DisplayOperator {
        symbol: String,
        body: Box<MathExpr>,
        sub: Option<Box<MathExpr>>,
        sup: Option<Box<MathExpr>>,
    },
}
#[derive(Clone, Debug)]
enum Mark {
    Text(String, Point, f32, crate::FontFace),
    Line(Point, Point, f32),
    Curve(Vec<Point>, f32),
}
#[derive(Clone, Debug, Default)]
pub struct MathLayout {
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
    marks: Vec<Mark>,
}
impl MathLayout {
    fn append(&mut self, other: Self, x: f32, y: f32) {
        self.ascent = self.ascent.max(other.ascent - y);
        self.descent = self.descent.max(other.descent + y);
        for mark in other.marks {
            self.marks.push(match mark {
                Mark::Text(t, p, s, face) => Mark::Text(
                    t,
                    Point {
                        x: p.x + x,
                        y: p.y + y,
                    },
                    s,
                    face,
                ),
                Mark::Curve(points, w) => Mark::Curve(
                    points
                        .into_iter()
                        .map(|p| Point {
                            x: p.x + x,
                            y: p.y + y,
                        })
                        .collect(),
                    w,
                ),
                Mark::Line(a, b, w) => Mark::Line(
                    Point {
                        x: a.x + x,
                        y: a.y + y,
                    },
                    Point {
                        x: b.x + x,
                        y: b.y + y,
                    },
                    w,
                ),
            });
        }
    }
    fn delimiter(&mut self, symbol: &str, x: f32, size: f32, top: f32, bottom: f32) {
        if symbol.is_empty() || symbol == "." {
            return;
        }
        let stroke = (size * 0.055).max(0.5);
        let left = matches!(symbol, "(" | "[" | "{");
        let a = x + size * 0.08;
        let b = x + size * 0.34;
        let (outer, inner) = if left { (a, b) } else { (b, a) };
        let mid = (top + bottom) / 2.;
        let point = |x, y| Point { x, y };
        match symbol {
            "(" | ")" => self.marks.push(Mark::Curve(
                vec![
                    point(inner, top),
                    point(outer, top + (bottom - top) * 0.16),
                    point(outer, bottom - (bottom - top) * 0.16),
                    point(inner, bottom),
                ],
                stroke,
            )),
            "[" | "]" => {
                self.marks
                    .push(Mark::Line(point(inner, top), point(outer, top), stroke));
                self.marks
                    .push(Mark::Line(point(outer, top), point(outer, bottom), stroke));
                self.marks.push(Mark::Line(
                    point(outer, bottom),
                    point(inner, bottom),
                    stroke,
                ));
            }
            "{" | "}" => {
                let bend = (outer + inner) / 2.;
                let quarter = (bottom - top) / 4.;
                for points in [
                    vec![
                        point(inner, top),
                        point(bend, top),
                        point(bend, top),
                        point(bend, mid - quarter),
                    ],
                    vec![
                        point(bend, mid - quarter),
                        point(bend, mid),
                        point(bend, mid),
                        point(outer, mid),
                    ],
                    vec![
                        point(outer, mid),
                        point(bend, mid),
                        point(bend, mid),
                        point(bend, mid + quarter),
                    ],
                    vec![
                        point(bend, mid + quarter),
                        point(bend, bottom),
                        point(bend, bottom),
                        point(inner, bottom),
                    ],
                ] {
                    self.marks.push(Mark::Curve(points, stroke));
                }
            }
            "|" | "||" => self.marks.push(Mark::Line(
                point((a + b) / 2., top),
                point((a + b) / 2., bottom),
                stroke,
            )),
            _ => {}
        }
    }
    pub fn draw(&self, target: &mut dyn DrawTarget, origin: Point, params: &PlotParameters) {
        let offset = match params.text_anchor {
            TextAnchor::Start => 0.,
            TextAnchor::Middle => self.width / 2.,
            TextAnchor::End => self.width,
        };
        let (sin, cos) = (-params.text_angle.to_radians()).sin_cos();
        let transform = |p: Point| Point {
            x: origin.x + (p.x - offset) * cos - p.y * sin,
            y: origin.y + (p.x - offset) * sin + p.y * cos,
        };
        for mark in &self.marks {
            match mark {
                Mark::Text(text, p, size, face) => target.draw_text(
                    text,
                    transform(*p),
                    &PlotParameters {
                        font_size: *size,
                        font_face: *face,
                        text_anchor: TextAnchor::Start,
                        ..params.clone()
                    },
                ),
                Mark::Curve(points, width) => {
                    let points: Vec<_> = points.iter().copied().map(transform).collect();
                    let [start, c1, c2, end] = points.as_slice() else {
                        continue;
                    };
                    target.draw_path(&Path {
                        commands: vec![
                            PathCommand::MoveTo(start.x, start.y),
                            PathCommand::CubicTo(c1.x, c1.y, c2.x, c2.y, end.x, end.y),
                        ],
                        stroke: Stroke::new(*width, params.text_color),
                        anti_alias: true,
                        ..Default::default()
                    });
                }
                Mark::Line(a, b, width) => {
                    let a = transform(*a);
                    let b = transform(*b);
                    target.draw_path(&Path {
                        commands: vec![
                            PathCommand::MoveTo(a.x, a.y),
                            PathCommand::LineTo(b.x, b.y),
                        ],
                        stroke: Stroke::new(*width, params.text_color),
                        anti_alias: true,
                        ..Default::default()
                    });
                }
            }
        }
    }
}
impl MathExpr {
    pub fn layout(&self, target: &dyn DrawTarget, params: &PlotParameters) -> MathLayout {
        self.layout_inner(target, params, false)
    }
    fn layout_inner(
        &self,
        target: &dyn DrawTarget,
        params: &PlotParameters,
        explicit_face: bool,
    ) -> MathLayout {
        let size = params.font_size;
        match self {
            Self::Upright(text) => Self::Text(text.clone()).layout_inner(
                target,
                &PlotParameters {
                    font_face: crate::FontFace::Plain,
                    ..params.clone()
                },
                true,
            ),
            Self::Variable(text) => {
                let face = if explicit_face {
                    params.font_face
                } else if params.font_face.is_bold() {
                    crate::FontFace::BoldItalic
                } else {
                    crate::FontFace::Italic
                };
                Self::Text(text.clone()).layout_inner(
                    target,
                    &PlotParameters {
                        font_face: face,
                        ..params.clone()
                    },
                    true,
                )
            }
            Self::Text(text) => {
                let m = target.measure_math_text(text, params);
                MathLayout {
                    width: m.width,
                    ascent: m.ascent,
                    descent: m.descent,
                    marks: vec![Mark::Text(
                        text.clone(),
                        Point::default(),
                        size,
                        params.font_face,
                    )],
                }
            }
            Self::Style(value, face) => value.layout_inner(
                target,
                &PlotParameters {
                    font_face: *face,
                    ..params.clone()
                },
                true,
            ),
            Self::Accent(value, accent) => {
                let mut out = value.layout_inner(target, params, explicit_face);
                let mut mark =
                    MathExpr::Text(accent.clone()).layout_inner(target, params, explicit_face);
                if accent == "¯" {
                    mark.width = out.width;
                    mark.marks = vec![Mark::Line(
                        Point { x: 0., y: 0. },
                        Point {
                            x: out.width,
                            y: 0.,
                        },
                        (size * 0.055).max(0.5),
                    )];
                    mark.ascent = size * 0.05;
                    mark.descent = 0.;
                }
                let x = (out.width - mark.width) / 2.;
                let y = -out.ascent - size * 0.1 - mark.descent;
                out.append(mark, x, y);
                out
            }
            Self::Underline(value) => {
                let mut out = value.layout_inner(target, params, explicit_face);
                let y = out.descent + size * 0.1;
                out.marks.push(Mark::Line(
                    Point { x: 0., y },
                    Point { x: out.width, y },
                    (size * 0.055).max(0.5),
                ));
                out.descent = y + size * 0.03;
                out
            }
            Self::Space(em) => MathLayout {
                width: em * size,
                ..Default::default()
            },
            Self::Phantom(value) => {
                let mut l = value.layout_inner(target, params, explicit_face);
                l.marks.clear();
                l
            }
            Self::BGroup { left, body, right } => {
                let body = body.layout_inner(target, params, explicit_face);
                let delimiter_width = |s: &str| {
                    if s.is_empty() || s == "." {
                        0.
                    } else {
                        size * 0.45
                    }
                };
                let lw = delimiter_width(left);
                let rw = delimiter_width(right);
                let mut out = MathLayout {
                    width: lw + body.width + rw,
                    ..Default::default()
                };
                out.append(body, lw, 0.);
                let top = -out.ascent - size * 0.08;
                let bottom = out.descent + size * 0.08;
                let right_x = out.width - rw;
                out.delimiter(left, 0., size, top, bottom);
                out.delimiter(right, right_x, size, top, bottom);
                if lw > 0. || rw > 0. {
                    out.ascent = -top + size * 0.03;
                    out.descent = bottom + size * 0.03;
                }
                out
            }
            Self::WideAccent(value, accent) => {
                let mut out = value.layout_inner(target, params, explicit_face);
                let y = -out.ascent - size * 0.14;
                let stroke = (size * 0.055).max(0.5);
                let height = size * 0.16;
                if accent == "tilde" {
                    out.marks.push(Mark::Curve(
                        vec![
                            Point { x: 0., y },
                            Point {
                                x: out.width / 3.,
                                y: y - height * 2.,
                            },
                            Point {
                                x: out.width * 2. / 3.,
                                y: y + height * 2.,
                            },
                            Point { x: out.width, y },
                        ],
                        stroke,
                    ));
                } else {
                    out.marks.push(Mark::Line(
                        Point { x: 0., y },
                        Point {
                            x: out.width / 2.,
                            y: y - height,
                        },
                        stroke,
                    ));
                    out.marks.push(Mark::Line(
                        Point {
                            x: out.width / 2.,
                            y: y - height,
                        },
                        Point { x: out.width, y },
                        stroke,
                    ));
                }
                out.ascent = -y + height + stroke / 2.;
                out
            }
            Self::DisplayOperator {
                symbol,
                body,
                sub,
                sup,
            } => {
                let op = Self::Text(symbol.clone()).layout_inner(target, params, true);
                let body = body.layout_inner(target, params, explicit_face);
                let script_params = PlotParameters {
                    font_size: size * 0.7,
                    ..params.clone()
                };
                let upper = sup
                    .as_ref()
                    .map(|v| v.layout_inner(target, &script_params, explicit_face));
                let lower = sub
                    .as_ref()
                    .map(|v| v.layout_inner(target, &script_params, explicit_face));
                let column = op
                    .width
                    .max(upper.as_ref().map_or(0., |l| l.width))
                    .max(lower.as_ref().map_or(0., |l| l.width));
                let op_ascent = op.ascent;
                let op_descent = op.descent;
                let op_x = (column - op.width) / 2.;
                let mut out = MathLayout {
                    width: column + size * 0.2 + body.width,
                    ..Default::default()
                };
                out.append(op, op_x, 0.);
                if let Some(l) = upper {
                    let x = (column - l.width) / 2.;
                    let y = -op_ascent - size * 0.15 - l.descent;
                    out.append(l, x, y);
                }
                if let Some(l) = lower {
                    let x = (column - l.width) / 2.;
                    let y = op_descent + size * 0.15 + l.ascent;
                    out.append(l, x, y);
                }
                out.append(body, column + size * 0.2, 0.);
                out
            }
            Self::Row(values) => {
                let mut out = MathLayout::default();
                for value in values {
                    let item = value.layout_inner(target, params, explicit_face);
                    let width = item.width;
                    out.append(item, out.width, 0.);
                    out.width += width;
                }
                out
            }
            Self::Fraction(a, b) | Self::Atop(a, b) => {
                let a = a.layout_inner(
                    target,
                    &PlotParameters {
                        font_size: size * 0.9,
                        ..params.clone()
                    },
                    explicit_face,
                );
                let b = b.layout_inner(
                    target,
                    &PlotParameters {
                        font_size: size * 0.9,
                        ..params.clone()
                    },
                    explicit_face,
                );
                let width = a.width.max(b.width) + size * 0.3;
                let mut out = MathLayout {
                    width,
                    ..Default::default()
                };
                let ay = -size * 0.3 - a.descent;
                let by = size * 0.15 + b.ascent;
                out.append(a.clone(), (width - a.width) / 2., ay);
                out.append(b.clone(), (width - b.width) / 2., by);
                if matches!(self, Self::Fraction(..)) {
                    out.marks.push(Mark::Line(
                        Point {
                            x: 0.,
                            y: -size * 0.15,
                        },
                        Point {
                            x: width,
                            y: -size * 0.15,
                        },
                        (size * 0.055).max(0.5),
                    ));
                }
                out
            }
            Self::Scripts { base, sub, sup } => {
                let mut out = base.layout_inner(target, params, explicit_face);
                // GNU R's plotmath applies an italic correction before a
                // script.  Using the ink ascent keeps the correction tied to
                // the actual glyph box instead of the font's line height.
                // Variables are auto-italic in plotmath unless an explicit
                // face was supplied.  The correction belongs to the base
                // glyph, so looking only at params.font_face misses the
                // normal `f[i]` case (where the caller leaves the face
                // Plain).
                let auto_italic_variable =
                    matches!(base.as_ref(), MathExpr::Variable(_)) && !explicit_face;
                let italic_correction = if params.font_face.is_italic() || auto_italic_variable {
                    out.ascent * 0.15
                } else {
                    0.
                };
                let x = out.width + italic_correction + size * 0.05;
                let script_params = PlotParameters {
                    font_size: size * 0.7,
                    ..params.clone()
                };
                let upper = sup
                    .as_ref()
                    .map(|value| value.layout_inner(target, &script_params, explicit_face));
                let lower = sub
                    .as_ref()
                    .map(|value| value.layout_inner(target, &script_params, explicit_face));
                // These are the GNU R TeX parameters (sigma5/13/16/17/18/19)
                // expressed against the bundled font's measured x/X heights.
                // The max terms retain R's protection against tall script ink.
                let x_height = target.measure_math_text("x", params).ascent;
                let cap_height = target.measure_math_text("X", params).ascent;
                let mut upper_y = 0.;
                let mut lower_y = 0.;
                if let Some(ref value) = upper {
                    upper_y = (out.ascent - 0.386_111 * x_height)
                        .max(0.95 * x_height)
                        .max(value.descent + 0.25 * x_height);
                }
                if let Some(ref value) = lower {
                    lower_y = (out.descent + 0.05 * x_height)
                        .max(0.35 * x_height)
                        .max(value.ascent - 0.8 * cap_height);
                }
                if let (Some(up), Some(down)) = (&upper, &lower) {
                    let rule = (size * 0.015).max(0.5);
                    // In our baseline coordinates the upper ink's bottom is
                    // `-upper_y + descent`, while the lower ink's top is
                    // `lower_y - ascent`.  Move the two scripts apart until
                    // GNU R's four-rule minimum is met.  Adjusting one up
                    // and the other down is important: changing the signs in
                    // the old branch could leave this gap unchanged.
                    let upper_bottom = -upper_y + up.descent;
                    let lower_top = lower_y - down.ascent;
                    let gap = lower_top - upper_bottom;
                    let minimum_gap = 4. * rule;
                    if gap < minimum_gap {
                        let delta = (minimum_gap - gap) * 0.5;
                        upper_y += delta;
                        lower_y += delta;
                    }
                }
                let extra = upper
                    .as_ref()
                    .map_or(0., |value| value.width)
                    .max(lower.as_ref().map_or(0., |value| value.width));
                if let Some(value) = upper {
                    out.append(value, x, -upper_y);
                }
                if let Some(value) = lower {
                    out.append(value, x, lower_y);
                }
                out.width = x + extra;
                out
            }
            Self::Radical(value) => {
                let l = value.layout_inner(target, params, explicit_face);
                let mut out = MathLayout {
                    width: l.width + size * 0.75,
                    ..Default::default()
                };
                let top = -l.ascent - size * 0.1;
                let points = [
                    Point {
                        x: 0.,
                        y: -size * 0.1,
                    },
                    Point {
                        x: size * 0.15,
                        y: -size * 0.2,
                    },
                    Point {
                        x: size * 0.3,
                        y: size * 0.1,
                    },
                    Point {
                        x: size * 0.6,
                        y: top,
                    },
                    Point {
                        x: out.width,
                        y: top,
                    },
                ];
                for pair in points.windows(2) {
                    out.marks
                        .push(Mark::Line(pair[0], pair[1], (size * 0.055).max(0.5)));
                }
                out.append(l, size * 0.7, 0.);
                out.ascent = out.ascent.max(-top);
                out.descent = out.descent.max(size * 0.1);
                out
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Scene};
    #[test]
    fn display_limits_do_not_overlap_body() {
        let target = Scene::new(400, 200);
        let params = PlotParameters {
            font_size: 20.,
            ..Default::default()
        };
        let l = MathExpr::DisplayOperator {
            symbol: "∑".into(),
            body: Box::new(MathExpr::Text("body".into())),
            sub: Some(Box::new(MathExpr::Text("lower".into()))),
            sup: Some(Box::new(MathExpr::Text("upper".into()))),
        }
        .layout(&target, &params);
        let mut right: f32 = 0.;
        let mut body_x = 0.;
        for mark in &l.marks {
            if let Mark::Text(t, p, size, face) = mark {
                if t == "body" {
                    body_x = p.x;
                } else {
                    right = right.max(
                        p.x + crate::default_font_book()
                            .measure_text(t, *size, *face)
                            .width,
                    );
                }
            }
        }
        assert!(body_x > right, "body begins after operator and limits");
    }
    #[test]
    fn wide_accents_and_delimiters_have_distinct_geometry() {
        let target = Scene::new(400, 200);
        let params = PlotParameters {
            font_size: 20.,
            ..Default::default()
        };
        let text = || Box::new(MathExpr::Text("xyz".into()));
        let hat = MathExpr::WideAccent(text(), "hat".into()).layout(&target, &params);
        assert!(
            hat.marks
                .iter()
                .any(|m| matches!(m, Mark::Line(a,b,_) if a.y != b.y))
        );
        let tilde = MathExpr::WideAccent(text(), "tilde".into()).layout(&target, &params);
        assert!(
            tilde
                .marks
                .iter()
                .any(|m| matches!(m, Mark::Curve(p,_) if p[1].y != p[2].y))
        );
        let group = |left: &str| {
            MathExpr::BGroup {
                left: left.into(),
                body: text(),
                right: "".into(),
            }
            .layout(&target, &params)
        };
        assert_eq!(group("").marks.len(), 1);
        assert_eq!(group(".").width, group("").width);
        assert_eq!(group("[").marks.len(), 4);
        assert_eq!(group("{").marks.len(), 5);
        assert!(
            group("(")
                .marks
                .iter()
                .any(|m| matches!(m, Mark::Curve(..)))
        );
    }
    #[test]
    fn math_uses_ink_height_instead_of_font_line_height() {
        let target = Scene::new(200, 200);
        let params = PlotParameters {
            font_size: 20.,
            ..Default::default()
        };
        let small = MathExpr::Text("x".into()).layout(&target, &params);
        let tall = MathExpr::Text("X".into()).layout(&target, &params);
        assert!(small.ascent < tall.ascent);
        assert_eq!(small.descent, 0.);
        assert!(MathExpr::Text("g".into()).layout(&target, &params).descent > 0.);
    }
    #[test]
    fn fractions_and_scripts_expand_bounds_and_emit_geometry() {
        let mut scene = Scene::new(300, 200);
        let x = MathExpr::Text("x".into());
        let fraction = MathExpr::Fraction(Box::new(x.clone()), Box::new(x.clone())).layout(
            &scene,
            &PlotParameters {
                font_size: 20.,
                ..Default::default()
            },
        );
        let plain = x.layout(
            &scene,
            &PlotParameters {
                font_size: 20.,
                ..Default::default()
            },
        );
        assert!(fraction.ascent > plain.ascent && fraction.descent > plain.descent);
        fraction.draw(
            &mut scene,
            Point { x: 100., y: 100. },
            &PlotParameters {
                font_size: 20.,
                text_color: Color::BLACK,
                ..Default::default()
            },
        );
        assert_eq!(scene.operations().len(), 3);
        let scripts = MathExpr::Scripts {
            base: Box::new(x.clone()),
            sub: Some(Box::new(x.clone())),
            sup: Some(Box::new(x)),
        }
        .layout(
            &scene,
            &PlotParameters {
                font_size: 20.,
                ..Default::default()
            },
        );
        assert!(
            scripts.width > plain.width
                && scripts.ascent > plain.ascent
                && scripts.descent > plain.descent
        );
    }

    #[test]
    fn italic_scripts_receive_a_glyph_based_correction() {
        let target = Scene::new(300, 200);
        let params = PlotParameters {
            font_size: 20.,
            font_face: crate::FontFace::Italic,
            ..Default::default()
        };
        let base = MathExpr::Variable("f".into()).layout(&target, &params);
        let scripts = MathExpr::Scripts {
            base: Box::new(MathExpr::Variable("f".into())),
            sub: None,
            sup: Some(Box::new(MathExpr::Text("x".into()))),
        }
        .layout(&target, &params);
        assert!(scripts.width > base.width + params.font_size * 0.05);
    }

    #[test]
    fn auto_italic_variables_receive_script_correction() {
        let target = Scene::new(300, 200);
        let params = PlotParameters {
            font_size: 20.,
            ..Default::default()
        };
        let base = MathExpr::Variable("f".into()).layout(&target, &params);
        let scripts = MathExpr::Scripts {
            base: Box::new(MathExpr::Variable("f".into())),
            sub: None,
            sup: Some(Box::new(MathExpr::Text("x".into()))),
        }
        .layout(&target, &params);
        assert!(scripts.width > base.width + params.font_size * 0.05);
    }
    #[test]
    fn phantom_preserves_bounds_without_marks() {
        let scene = Scene::new(20, 20);
        let l = MathExpr::Phantom(Box::new(MathExpr::Text("hidden".into()))).layout(
            &scene,
            &PlotParameters {
                font_size: 12.,
                ..Default::default()
            },
        );
        assert!(l.width > 0.);
        assert!(l.marks.is_empty());
    }
}
