//! Owned mathematical typesetting. Coordinates use a baseline and a downward Y axis.
use crate::{DrawTarget, Path, PathCommand, PlotParameters, Point, Stroke, TextAnchor};

#[derive(Clone, Debug, PartialEq)]
pub enum MathExpr {
    Text(String),
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
    Space(f32),
    Phantom(Box<MathExpr>),
}
#[derive(Clone, Debug)]
enum Mark {
    Text(String, Point, f32, crate::FontFace),
    Line(Point, Point, f32),
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
        let size = params.font_size;
        match self {
            Self::Text(text) => {
                let m = target.measure_text(text, params);
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
            Self::Style(value, face) => value.layout(
                target,
                &PlotParameters {
                    font_face: *face,
                    ..params.clone()
                },
            ),
            Self::Space(em) => MathLayout {
                width: em * size,
                ..Default::default()
            },
            Self::Phantom(value) => {
                let mut l = value.layout(target, params);
                l.marks.clear();
                l
            }
            Self::Row(values) => {
                let mut out = MathLayout::default();
                for value in values {
                    let item = value.layout(target, params);
                    let width = item.width;
                    out.append(item, out.width, 0.);
                    out.width += width;
                }
                out
            }
            Self::Fraction(a, b) | Self::Atop(a, b) => {
                let a = a.layout(
                    target,
                    &PlotParameters {
                        font_size: size * 0.9,
                        ..params.clone()
                    },
                );
                let b = b.layout(
                    target,
                    &PlotParameters {
                        font_size: size * 0.9,
                        ..params.clone()
                    },
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
                let mut out = base.layout(target, params);
                let x = out.width + size * 0.05;
                let mut extra: f32 = 0.;
                if let Some(sup) = sup {
                    let l = sup.layout(
                        target,
                        &PlotParameters {
                            font_size: size * 0.7,
                            ..params.clone()
                        },
                    );
                    extra = extra.max(l.width);
                    let y = -(out.ascent * 0.65).max(size * 0.5) - l.descent;
                    out.append(l, x, y);
                }
                if let Some(sub) = sub {
                    let l = sub.layout(
                        target,
                        &PlotParameters {
                            font_size: size * 0.7,
                            ..params.clone()
                        },
                    );
                    extra = extra.max(l.width);
                    let y = (size * 0.25).max(l.ascent * 0.5);
                    out.append(l, x, y);
                }
                out.width = x + extra;
                out
            }
            Self::Radical(value) => {
                let l = value.layout(target, params);
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
