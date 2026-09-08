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
    /// Fixed glyph magnification, used for ordinary GNU R group delimiters.
    Scale(Box<MathExpr>, f32),
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
    italic: f32,
    simple: bool,
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
#[derive(Clone, Copy)]
struct MathContext {
    level: u8,
    compact: bool,
    base_size: f32,
}
impl MathContext {
    fn size(self) -> f32 {
        self.base_size
            * match self.level {
                0 | 1 => 1.,
                2 => 0.7,
                _ => 0.5,
            }
    }
    fn script(self, compact: bool) -> Self {
        Self {
            level: (self.level + 1).clamp(2, 3),
            compact,
            ..self
        }
    }
    fn numerator(self) -> Self {
        if self.level == 0 {
            Self { level: 1, ..self }
        } else {
            self.script(self.compact)
        }
    }
    fn denominator(self) -> Self {
        if self.level == 0 {
            Self {
                level: 1,
                compact: true,
                ..self
            }
        } else {
            self.script(true)
        }
    }
    fn prime(self) -> Self {
        Self {
            compact: true,
            ..self
        }
    }
}
impl MathExpr {
    pub fn layout(&self, target: &dyn DrawTarget, params: &PlotParameters) -> MathLayout {
        self.layout_inner(
            target,
            params,
            MathContext {
                level: 0,
                compact: false,
                base_size: params.font_size,
            },
        )
    }
    fn layout_inner(
        &self,
        target: &dyn DrawTarget,
        params: &PlotParameters,
        context: MathContext,
    ) -> MathLayout {
        let params = &PlotParameters {
            font_size: context.size(),
            ..params.clone()
        };
        let size = params.font_size;
        match self {
            Self::Upright(text) => Self::Text(text.clone()).layout_inner(
                target,
                &PlotParameters {
                    font_face: crate::FontFace::Plain,
                    ..params.clone()
                },
                context,
            ),
            Self::Variable(text) => {
                let face = params.font_face;
                let mut out = MathLayout {
                    simple: true,
                    ..Default::default()
                };
                for ch in text.chars() {
                    let face = if ch.is_ascii_digit() {
                        crate::FontFace::Plain
                    } else {
                        face
                    };
                    let cp = PlotParameters {
                        font_face: face,
                        ..params.clone()
                    };
                    let mut item = Self::Text(ch.to_string()).layout_inner(target, &cp, context);
                    out.width += out.italic;
                    let width = item.width;
                    out.italic = if face.is_italic() {
                        0.15 * item.ascent
                    } else {
                        0.
                    };
                    item.italic = 0.;
                    out.append(item, out.width, 0.);
                    out.width += width;
                }
                out
            }
            Self::Text(text) => {
                let m = target.measure_math_text(text, params);
                MathLayout {
                    width: m.width,
                    ascent: m.ascent,
                    descent: m.descent,
                    marks: vec![Mark::Text(
                        text.clone(),
                        Point { x: 0., y: 0. },
                        size,
                        params.font_face,
                    )],
                    italic: if params.font_face.is_italic() {
                        0.15 * m.ascent
                    } else {
                        0.
                    },
                    simple: true,
                }
            }
            Self::Scale(value, scale) => value.layout_inner(
                target,
                params,
                MathContext {
                    base_size: context.base_size * scale,
                    ..context
                },
            ),
            Self::Style(value, face) => value.layout_inner(
                target,
                &PlotParameters {
                    font_face: *face,
                    ..params.clone()
                },
                context,
            ),
            Self::Accent(value, accent) => {
                let mut out = value.layout_inner(target, params, context);
                let mut mark = MathExpr::Text(accent.clone()).layout_inner(target, params, context);
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
                let mut out = value.layout_inner(target, params, context);
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
                width: if context.level >= 2 && *em >= 2. / 9. {
                    0.
                } else {
                    em * target.measure_math_text("M", params).width
                },
                ..Default::default()
            },
            Self::Phantom(value) => {
                let mut l = value.layout_inner(target, params, context);
                l.marks.clear();
                l
            }
            Self::BGroup { left, body, right } => {
                let mut body = body.layout_inner(target, params, context);
                body.width += body.italic;
                let axis = target.measure_math_text("+", params).ascent / 2.;
                let dist = (body.ascent - axis).max(body.descent + axis)
                    + 0.2 * target.measure_math_text("x", params).ascent;
                let delimiter = |symbol: &str| {
                    let (top, ext, bot) = match symbol {
                        "(" => ("⎛", "⎜", "⎝"),
                        ")" => ("⎞", "⎟", "⎠"),
                        "[" => ("⎡", "⎢", "⎣"),
                        "]" => ("⎤", "⎥", "⎦"),
                        "{" => ("⎧", "⎪", "⎩"),
                        "}" => ("⎫", "⎪", "⎭"),
                        "|" | "||" => ("⎪", "⎪", "⎪"),
                        _ => return MathLayout::default(),
                    };
                    let plain = PlotParameters {
                        font_face: crate::FontFace::Plain,
                        ..params.clone()
                    };
                    let top = Self::Text(top.into()).layout_inner(target, &plain, context);
                    let bot = Self::Text(bot.into()).layout_inner(target, &plain, context);
                    let ext = Self::Text(ext.into()).layout_inner(target, &plain, context);
                    let brace = matches!(symbol, "{" | "}");
                    let dist = if brace {
                        dist.max(1.2 * (top.ascent + top.descent))
                    } else {
                        dist.max(0.8 * (top.ascent + top.descent))
                    };
                    let top_y = -(dist - top.ascent + axis);
                    let bot_y = dist - bot.descent - axis;
                    let ytop = axis + dist - top.ascent - top.descent;
                    let ybot = axis - dist + bot.ascent + bot.descent;
                    let width = top.width.max(bot.width);
                    let mut result = MathLayout {
                        width,
                        ..Default::default()
                    };
                    result.append(top, 0., top_y);
                    result.append(bot, 0., bot_y);
                    if brace {
                        let mid = Self::Text(if symbol == "{" { "⎨" } else { "⎬" }.into())
                            .layout_inner(target, &plain, context);
                        let shift = axis - (mid.ascent - mid.descent) / 2.;
                        result.append(mid, 0., -shift);
                    }
                    let n = ((ytop - ybot) / (0.99 * (ext.ascent + ext.descent)))
                        .ceil()
                        .max(0.) as usize;
                    for i in 0..n {
                        let y = ybot + (i as f32 + 0.5) * (ytop - ybot) / n as f32
                            - (ext.ascent - ext.descent) / 2.;
                        result.append(ext.clone(), 0., -y);
                    }
                    result
                };
                let left = delimiter(left);
                let right = delimiter(right);
                let mut out = MathLayout {
                    width: left.width + body.width + right.width,
                    ..Default::default()
                };
                let bx = left.width;
                let rx = bx + body.width;
                out.append(left, 0., 0.);
                out.append(body, bx, 0.);
                out.append(right, rx, 0.);
                out
            }
            Self::WideAccent(value, accent) => {
                let mut out = value.layout_inner(target, params, context);
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
                let plain = PlotParameters {
                    font_face: crate::FontFace::Plain,
                    ..params.clone()
                };
                let integral = symbol == "∫";
                let mut op = if integral && context.level == 0 {
                    let top = Self::Text("⌠".into()).layout_inner(target, &plain, context);
                    let bottom = Self::Text("⌡".into()).layout_inner(target, &plain, context);
                    let axis = target.measure_math_text("+", params).ascent / 2.;
                    let ty = -(axis + 0.99 * top.descent);
                    let by = -(axis - 0.99 * bottom.ascent);
                    let mut op = MathLayout {
                        width: top.width.max(bottom.width),
                        ..Default::default()
                    };
                    op.append(top, 0., ty);
                    op.append(bottom, 0., by);
                    op
                } else {
                    let magnify =
                        context.level == 0 && matches!(symbol.as_str(), "∑" | "∏" | "∪" | "∩");
                    let ctx = if magnify {
                        MathContext {
                            base_size: context.base_size * 1.25,
                            ..context
                        }
                    } else {
                        context
                    };
                    let op = Self::Text(symbol.clone()).layout_inner(target, &plain, ctx);
                    if magnify {
                        let axis = target
                            .measure_math_text(
                                "+",
                                &PlotParameters {
                                    font_size: ctx.size(),
                                    ..plain.clone()
                                },
                            )
                            .ascent
                            / 2.;
                        let shift = (op.ascent - op.descent) / 2. - axis;
                        let mut shifted = MathLayout {
                            width: op.width,
                            ..Default::default()
                        };
                        shifted.append(op, 0., shift);
                        shifted
                    } else {
                        op
                    }
                };
                let lower = sub
                    .as_ref()
                    .map(|v| v.layout_inner(target, params, context.script(true)));
                let upper = sup
                    .as_ref()
                    .map(|v| v.layout_inner(target, params, context.script(context.compact)));
                let thin = target.measure_math_text("M", params).width / 6.;
                let space = 0.15 * target.measure_math_text("X", params).ascent;
                let column = op
                    .width
                    .max(lower.as_ref().map_or(0., |v| v.width))
                    .max(upper.as_ref().map_or(0., |v| v.width));
                let op_width = op.width;
                let ascent = op.ascent;
                let descent = op.descent;
                let mut out = MathLayout::default();
                if !integral {
                    op.ascent += space;
                    op.descent += space;
                    out.append(op, (column - op_width) / 2., 0.);
                    out.width = column;
                } else {
                    out.append(op, 0., 0.);
                    out.width = op_width;
                }
                if let Some(l) = lower {
                    let x = if integral {
                        op_width / 2. + thin
                    } else {
                        (column - l.width) / 2.
                    };
                    let y = if integral {
                        descent + (l.ascent - l.descent) / 2.
                    } else {
                        descent + l.ascent + space.max(space - l.ascent)
                    };
                    out.width = out.width.max(x + l.width);
                    out.append(l, x, y);
                }
                if let Some(u) = upper {
                    let x = if integral {
                        op_width + thin
                    } else {
                        (column - u.width) / 2.
                    };
                    let y = if integral {
                        -ascent + (u.ascent - u.descent) / 2.
                    } else {
                        -ascent - u.descent - space.max(space - u.descent)
                    };
                    out.width = out.width.max(x + u.width);
                    out.append(u, x, y);
                }
                if !integral {
                    // xi13 surrounds the entire operator and its limits.
                    out.ascent += space;
                    out.descent += space;
                    // The symbol itself was already padded above.
                    if sub.is_none() {
                        out.descent -= space;
                    }
                    if sup.is_none() {
                        out.ascent -= space;
                    }
                    out.width += thin;
                }
                let body = body.layout_inner(target, params, context);
                let bw = body.width;
                out.italic = body.italic;
                out.append(body, out.width, 0.);
                out.width += bw;
                out
            }
            Self::Row(values) => {
                let mut out = MathLayout::default();
                for value in values {
                    let item = value.layout_inner(target, params, context);
                    out.width += out.italic;
                    out.italic = item.italic;
                    let width = item.width;
                    out.append(item, out.width, 0.);
                    out.width += width;
                }
                out
            }
            Self::Fraction(a, b) | Self::Atop(a, b) => {
                let mut a = a.layout_inner(target, params, context.numerator());
                let mut b = b.layout_inner(target, params, context.denominator());
                a.width += a.italic;
                b.width += b.italic;
                let width = a.width.max(b.width);
                let axis = target.measure_math_text("+", params).ascent / 2.;
                let cap = target.measure_math_text("X", params).ascent;
                // GNU R's rule thickness is .015 inches, independent of cex.
                let theta = 0.015 * 72.;
                let (mut u, mut v, phi) = if context.level == 0 {
                    (
                        axis + 3.51 * theta
                            + 0.15 * cap
                            + 0.7 * target.measure_math_text("g", params).descent,
                        -axis
                            + 3.51 * theta
                            + 0.7 * target.measure_math_text("0", params).ascent
                            + 0.344444 * cap,
                        3. * theta,
                    )
                } else {
                    (
                        axis + 1.51 * theta + 0.08333333 * cap,
                        -axis
                            + 1.51 * theta
                            + 0.7 * target.measure_math_text("0", params).ascent
                            + 0.08333333 * cap,
                        theta,
                    )
                };
                u += (phi - (u - a.descent - axis - theta / 2.)).max(0.);
                v += (phi - (axis + theta / 2. - b.ascent + v)).max(0.);
                let mut out = MathLayout {
                    width,
                    ..Default::default()
                };
                let ax = (width - a.width) / 2.;
                let bx = (width - b.width) / 2.;
                out.append(a, ax, -u);
                out.append(b, bx, v);
                if matches!(self, Self::Fraction(..)) {
                    out.marks.push(Mark::Line(
                        Point { x: 0., y: -axis },
                        Point { x: width, y: -axis },
                        0.75,
                    ));
                }
                out
            }
            Self::Scripts { base, sub, sup } => {
                let mut out = base.layout_inner(target, params, context);
                let correction = out.italic;
                out.width += correction;
                out.italic = 0.;
                let x = out.width;
                let upper = sup
                    .as_ref()
                    .map(|v| v.layout_inner(target, params, context.script(context.compact)));
                let lower = sub
                    .as_ref()
                    .map(|v| v.layout_inner(target, params, context.script(true)));
                let xh = target.measure_math_text("x", params).ascent;
                let cap = target.measure_math_text("X", params).ascent;
                let mut u = if out.simple {
                    0.
                } else {
                    out.ascent - 0.3861111 * cap
                };
                let mut v = if out.simple {
                    0.
                } else {
                    out.descent + 0.05 * cap
                };
                if let Some(ref up) = upper {
                    let p = if context.level == 0 && !context.compact {
                        0.95
                    } else if context.compact {
                        0.7
                    } else {
                        0.825
                    };
                    u = u.max(p * xh).max(up.descent + 0.25 * xh);
                }
                if let Some(ref down) = lower {
                    if upper.is_some() {
                        v = v.max(0.45 * cap);
                    } else {
                        let script_xh = target
                            .measure_math_text(
                                "x",
                                &PlotParameters {
                                    font_size: context.script(true).size(),
                                    ..params.clone()
                                },
                            )
                            .ascent;
                        v = v.max(0.35 * xh).max(down.ascent - 0.8 * script_xh);
                    }
                }
                if let (Some(up), Some(down)) = (&upper, &lower)
                    && u - up.descent - down.ascent + v < 4. * 0.015 * 72.
                {
                    let psi = 0.8 * xh - (u - up.descent);
                    if psi > 0. {
                        u += psi;
                        v -= psi;
                    }
                }
                let mut width = x;
                if let Some(up) = upper {
                    let sx = x + if lower.is_some() { correction } else { 0. };
                    width = width.max(sx + up.width);
                    out.append(up, sx, -u);
                }
                if let Some(down) = lower {
                    width = width.max(x + down.width);
                    out.append(down, x, v);
                }
                out.width = width;
                out.simple = false;
                out
            }
            Self::Radical(value) => {
                let l = value.layout_inner(target, params, context.prime());
                // Match GNU R's RenderRadical constants: RADICAL_GAP=.4
                // x-height, RADICAL_SPACE=.2 x-height, and a mu-space trail.
                // The previous size-relative .75 em advance made radicals
                // drift with fonts whose x-height differs from the point
                // size (including the bundled DejaVu family).
                let x_height = target.measure_math_text("x", params).ascent;
                let cap_height = target.measure_math_text("X", params).ascent;
                let rad_width = 0.6 * cap_height;
                let rad_space = 0.2 * x_height;
                let rad_trail = 0.055_555_56 * target.measure_text("M", params).width;
                let mut out = MathLayout {
                    width: l.width + rad_width + rad_space + 2. * rad_trail,
                    ..Default::default()
                };
                let top = -l.ascent - 0.4 * x_height;
                let middle = (l.ascent - l.descent) / 2.;
                let points = [
                    Point {
                        x: 0.,
                        y: -0.8 * middle,
                    },
                    Point {
                        x: rad_width * 0.3,
                        y: -middle,
                    },
                    Point {
                        x: rad_width * 0.6,
                        y: l.descent,
                    },
                    Point {
                        x: rad_width,
                        y: top,
                    },
                    Point {
                        x: rad_width + rad_space + l.width + l.italic + rad_trail,
                        y: top,
                    },
                ];
                for pair in points.windows(2) {
                    out.marks.push(Mark::Line(pair[0], pair[1], 0.75));
                }
                out.append(l, rad_width + rad_space, 0.);
                out.ascent = out.ascent.max(-top);

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
        for (delimiter, glyph) in [("[", "⎡"), ("{", "⎨"), ("(", "⎛")] {
            assert!(
                group(delimiter)
                    .marks
                    .iter()
                    .any(|m| matches!(m, Mark::Text(t, ..) if t == glyph))
            );
        }
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
    fn default_variables_use_gnu_plain_face_without_italic_correction() {
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
        assert_eq!(base.italic, 0.);
        let x = scripts
            .marks
            .iter()
            .find_map(|m| match m {
                Mark::Text(t, p, _, _) if t == "x" => Some(p.x),
                _ => None,
            })
            .unwrap();
        assert_eq!(x, base.width);
    }

    #[test]
    fn plotmath_digits_use_plain_face_inside_italic_runs() {
        let target = Scene::new(300, 200);
        let params = PlotParameters {
            font_size: 20.,
            font_face: crate::FontFace::Italic,
            ..Default::default()
        };
        let layout = MathExpr::Variable("x12y".into()).layout(&target, &params);
        let faces: Vec<_> = layout
            .marks
            .iter()
            .filter_map(|mark| match mark {
                Mark::Text(text, _, _, face) => Some((text.as_str(), *face)),
                _ => None,
            })
            .collect();
        assert_eq!(
            faces,
            vec![
                ("x", crate::FontFace::Italic),
                ("1", crate::FontFace::Plain),
                ("2", crate::FontFace::Plain),
                ("y", crate::FontFace::Italic),
            ]
        );
    }

    #[test]
    fn subscript_without_superscript_uses_sigma16_x_height() {
        let target = Scene::new(300, 200);
        let params = PlotParameters {
            font_size: 20.,
            ..Default::default()
        };
        let layout = MathExpr::Scripts {
            base: Box::new(MathExpr::Text("X".into())),
            sub: Some(Box::new(MathExpr::Text("x".into()))),
            sup: None,
        }
        .layout(&target, &params);
        let baseline = layout
            .marks
            .iter()
            .find_map(|mark| match mark {
                Mark::Text(text, point, _, _) if text == "x" => Some(point.y),
                _ => None,
            })
            .expect("subscript mark");
        let xh = target.measure_math_text("x", &params).ascent;
        assert!((baseline - 0.35 * xh).abs() < 1e-5);
    }
    #[test]
    fn alpha_width_matches_same_font_gnu_oracle() {
        let target = Scene::new(300, 200);
        let layout = MathExpr::Text("α".into()).layout(
            &target,
            &PlotParameters {
                font_size: 12.,
                ..Default::default()
            },
        );
        // GNU R's metric-only DejaVu device reports 0.109863281 inches;
        // convert to points for the engine's point-sized layout.
        assert!((f64::from(layout.width) - 7.91015625).abs() < 1e-5);
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
