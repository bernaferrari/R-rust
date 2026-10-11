//! GNU graphics natives drawn through the portable renderplot engine.
//!
//! Ports the `.External.graphics` drawing contracts from
//! r-source/src/library/graphics/src/plot.c (`C_path`, `C_symbols`,
//! `C_xspline`, `C_erase`, `C_dend`, `C_dendwindow`) plus the interaction
//! natives (`C_locator`, `C_identify`): typed argument decode, engine state,
//! then DrawOperations recorded on the scene-backed renderer. The scene
//! device has no `dev->locator` callback, so both interaction natives return
//! GNU's no-interaction results (`GLocator` fails on the first click).
//!
//! `persp_render`/`filledcontour_render` are the drawing halves of
//! plot3d.rs's `C_persp`/`C_filledcontour` (the transformation and clipping
//! mathematics stays there).

use super::*;
use crate::library::graphics::plot3d::{
    AXIS_START, EDGE, FACE, FacetShade, FindPolygonVertices, TICK_VECTOR, VERTEX,
};
use crate::library::graphics::xspline;
use crate::main::coerce::{asInteger, asLogical, asReal};
use crate::mainutils::essentials::base_error;
use crate::mainutils::plotmath::Label;
#[allow(unused_imports)]
use crate::sexp::accessors::*;
use crate::sexp::constructors::{Rf_allocVector3, Rf_mkChar};
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_NaString, R_NilValue};
use crate::sexp::protect::protect;
use r_graphics_engine::{Color, Path, PathCommand, Point, Stroke, TextAnchor};

/// Dispatch the natives implemented here. Returns `None` for names handled
/// by the ordinary portable draw path.
pub(crate) unsafe fn dispatch(name: &str, args: SEXP) -> Option<SEXP> {
    unsafe {
        match name {
            "erase" => Some(erase(args)),
            "path" => Some(path(args)),
            "symbols" => Some(symbols(args)),
            "xspline" => Some(xspline(args)),
            "dend" => Some(dend(args)),
            "dendwindow" => Some(dendwindow(args)),
            "locator" => Some(locator(args)),
            "identify" => Some(identify(args)),
            _ => None,
        }
    }
}

fn ints(x: SEXP) -> Vec<i32> {
    unsafe {
        if x == R_NilValue() {
            return vec![];
        }
        if !matches!(
            SEXPTYPE(TYPEOF(x)),
            SEXPTYPE::INTSXP | SEXPTYPE::REALSXP | SEXPTYPE::LGLSXP
        ) {
            base_error("plot data must be numeric");
        }
        (0..XLENGTH(x))
            .map(|i| {
                if TYPEOF(x) == SEXPTYPE::REALSXP {
                    let v = *REAL(x).add(i as usize);
                    if v.is_nan() || !v.is_finite() {
                        NA_INTEGER
                    } else {
                        v as i32
                    }
                } else {
                    INTEGER_ELT(x, i as i32)
                }
            })
            .collect()
    }
}

/// GNU `asRboolean` with NA collapsing to false.
fn logical_flag(x: SEXP) -> bool {
    unsafe {
        if x == R_NilValue() {
            return false;
        }
        asLogical(x) != 0 && asLogical(x) != NA_LOGICAL
    }
}

/// GNU `SymbolRange`: the finite maximum/minimum pair is valid when the
/// values are ordered and non-negative.
fn symbol_range(values: &[f64]) -> Option<(f64, f64)> {
    let mut max = -f64::MAX;
    let mut min = f64::MAX;
    for &v in values {
        if v.is_finite() {
            max = max.max(v);
            min = min.min(v);
        }
    }
    (max >= min && min >= 0.).then_some((max, min))
}

/// User units per inch along one axis (linear view; symbols on logarithmic
/// axes resolve radii through the identity scale, matching GConvertXUnits'
/// linear difference behavior).
fn user_per_inch(c: &Coordinates, vertical: bool) -> f64 {
    if vertical {
        (c.limits[3] - c.limits[2]) / f64::from(c.rect[3] - c.rect[1]) * 72.
    } else {
        (c.limits[1] - c.limits[0]) / f64::from(c.rect[2] - c.rect[0]) * 72.
    }
}

/// GNU `C_erase` — fill the whole device (NDC 0..1) with `col`. The default
/// `R_TRANWHITE` fill is transparent, so nothing is drawn.
unsafe fn erase(args: SEXP) -> SEXP {
    unsafe {
        let fill = colors(arg(args, "col"), transparent()).remove(0);
        if fill.a != 0 {
            let target = &mut *renderer();
            let (w, h) = target.dimensions();
            target.set_clip(None);
            let mut rect = Path::rect(0., 0., w as f32, h as f32);
            rect.fill = fill;
            rect.stroke = Stroke::new(0., transparent());
            target.draw_path(&rect);
        }
        invisible()
    }
}

/// GNU `C_path` — filled subpaths split by `nper` counts. The engine's Path
/// carries a single implicit fill rule, so the winding/even-odd distinction
/// of `rule` is decoded (and honored where subpaths are disjoint) but not
/// re-expressed for overlapping holes.
unsafe fn path(args: SEXP) -> SEXP {
    unsafe {
        let c = current();
        let b = bindings(args, &["x", "y", "nper", "rule", "col", "border"]);
        let x = values(b[0]);
        let y = values(b[1]);
        let nper = ints(b[2]);
        let _rule = ints(b[3]).first().copied().unwrap_or(0);
        let fill = colors(b[4], transparent());
        let border = colors(b[5], par_color("fg", Color::BLACK));
        if x.len() != y.len() {
            base_error("invalid 'x' or 'y' (in 'GPath')");
        }
        let stroke = style(args, point_size(args));
        let target = &mut *renderer();
        target.set_clip(Some(clip_rect(c, args)));
        let mut start = 0usize;
        for (index, count) in nper.iter().map(|v| *v as usize).enumerate() {
            if count == 0 {
                continue;
            }
            if start + count > x.len() {
                base_error("invalid 'x' or 'y' (in 'GPath')");
            }
            let mut commands = Vec::new();
            for k in 0..count {
                let p = c.map(x[start + k], y[start + k]);
                if !p.x.is_finite() || !p.y.is_finite() {
                    base_error("invalid 'x' or 'y' (in 'GPath')");
                }
                commands.push(if k == 0 {
                    PathCommand::MoveTo(p.x, p.y)
                } else {
                    PathCommand::LineTo(p.x, p.y)
                });
            }
            commands.push(PathCommand::Close);
            let mut subpath = Path {
                commands,
                fill: fill[0],
                stroke: stroke.stroke(index),
                anti_alias: true,
            };
            subpath.stroke.color = border[index % border.len()];
            target.draw_path(&subpath);
            start += count;
        }
        target.set_clip(None);
        invisible()
    }
}

/// GNU `C_symbols` — circles, squares, rectangles, stars, thermometers and
/// boxplots, with GNU's inches/NDC size conversions.
unsafe fn symbols(args: SEXP) -> SEXP {
    unsafe {
        let c = current();
        let b = bindings(args, &["x", "y", "type", "data", "inches", "bg", "fg"]);
        let x = values(b[0]);
        let y = values(b[1]);
        if x.is_empty() || y.is_empty() || x.len() != y.len() {
            base_error("invalid symbol coordinates");
        }
        let symbol_type = asInteger(b[2]);
        let data = values(b[3]);
        let dim = crate::attrib_core::getAttrib(b[3], crate::attrib_core::R_DimSymbol());
        let (nr, nc) = if dim == R_NilValue() || LENGTH(dim) == 0 {
            (data.len(), 1)
        } else {
            let d0 = INTEGER_ELT(dim, 0);
            let d1 = if LENGTH(dim) > 1 {
                INTEGER_ELT(dim, 1)
            } else {
                1
            };
            (d0.max(0) as usize, d1.max(0) as usize)
        };
        if nr == 0 || nc == 0 {
            base_error("invalid symbol parameter vector");
        }
        if data.len() < nr * nc || nr != x.len() {
            base_error("x/y/parameter length mismatch");
        }
        // R matrices are column-major: p[i + j * nr].
        let p = |i: usize, j: usize| data[i + j * nr];
        let mut inches = asReal(b[4]);
        if !inches.is_finite() || inches < 0. {
            inches = 0.;
        }
        let bg = colors(b[5], transparent());
        let fg = colors(b[6], transparent());
        let target = &mut *renderer();
        target.set_clip(Some(clip_rect(c, args)));
        match symbol_type {
            1 => {
                // circles: radius in inches
                if nc != 1 {
                    base_error("invalid circles data");
                }
                let radii: Vec<f64> = (0..nr).map(|i| p(i, 0)).collect();
                let Some((pmax, _)) = symbol_range(&radii) else {
                    base_error("invalid symbol parameter");
                };
                for i in 0..nr {
                    if ![x[i], y[i], radii[i]].iter().all(|v| v.is_finite()) {
                        continue;
                    }
                    let rx = if inches > 0. {
                        radii[i] * inches / pmax
                    } else {
                        radii[i] / user_per_inch(&c, false)
                    };
                    // GCircle maps a zero radius to one device pixel.
                    let r = (rx * 72.).max(1.) as f32;
                    let center = c.map(x[i], y[i]);
                    let mut circle = Path {
                        commands: vec![
                            PathCommand::MoveTo(center.x + r, center.y),
                            PathCommand::ArcTo {
                                rx: r,
                                ry: r,
                                x: center.x - r,
                                y: center.y,
                            },
                            PathCommand::ArcTo {
                                rx: r,
                                ry: r,
                                x: center.x + r,
                                y: center.y,
                            },
                            PathCommand::Close,
                        ],
                        fill: bg[i % bg.len()],
                        stroke: Stroke::new(1., fg[i % fg.len()]),
                        anti_alias: true,
                    };
                    circle.stroke.color = fg[i % fg.len()];
                    target.draw_path(&circle);
                }
            }
            2 => {
                // squares
                if nc != 1 {
                    base_error("invalid squares data");
                }
                let sizes: Vec<f64> = (0..nr).map(|i| p(i, 0)).collect();
                let Some((pmax, _)) = symbol_range(&sizes) else {
                    base_error("invalid symbol parameter");
                };
                for i in 0..nr {
                    if ![x[i], y[i], sizes[i]].iter().all(|v| v.is_finite()) {
                        continue;
                    }
                    let side = if inches > 0. {
                        sizes[i] * inches / pmax
                    } else {
                        sizes[i] / user_per_inch(&c, false)
                    };
                    let rx = (0.5 * side * 72.) as f32;
                    let center = c.map(x[i], y[i]);
                    draw_rect_symbol(
                        target,
                        center.x - rx,
                        center.y - rx,
                        center.x + rx,
                        center.y + rx,
                        bg[i % bg.len()],
                        fg[i % fg.len()],
                    );
                }
            }
            3 => {
                // rectangles
                if nc != 2 {
                    base_error("invalid rectangles data (need 2 columns)");
                }
                let flat: Vec<f64> = (0..2 * nr).map(|i| data[i]).collect();
                let Some((pmax, _)) = symbol_range(&flat) else {
                    base_error("invalid symbol parameter");
                };
                for i in 0..nr {
                    if ![x[i], y[i], p(i, 0), p(i, 1)].iter().all(|v| v.is_finite()) {
                        continue;
                    }
                    let (mut p0, mut p1) = (p(i, 0), p(i, 1));
                    if inches > 0. {
                        p0 *= inches / pmax;
                        p1 *= inches / pmax;
                    }
                    let rx = (0.5 * p0 * 72.) as f32;
                    let ry = (0.5 * p1 * 72.) as f32;
                    let center = c.map(x[i], y[i]);
                    draw_rect_symbol(
                        target,
                        center.x - rx,
                        center.y - ry,
                        center.x + rx,
                        center.y + ry,
                        bg[i % bg.len()],
                        fg[i % fg.len()],
                    );
                }
            }
            4 => {
                // stars
                if nc < 3 {
                    base_error("invalid stars data");
                }
                let flat: Vec<f64> = (0..nc * nr).map(|i| data[i]).collect();
                let Some((pmax, _)) = symbol_range(&flat) else {
                    base_error("invalid symbol parameter");
                };
                let step = 2.0 * std::f64::consts::PI / nc as f64;
                for i in 0..nr {
                    if !x[i].is_finite() || !y[i].is_finite() {
                        continue;
                    }
                    let center = c.map(x[i], y[i]);
                    let mut commands = Vec::new();
                    for j in 0..nc {
                        let mut v = p(i, j);
                        if !v.is_finite() {
                            v = 0.;
                        }
                        let length = if inches > 0. {
                            v / pmax * inches
                        } else {
                            v / user_per_inch(&c, false)
                        };
                        let px = center.x + (length * (j as f64 * step).cos() * 72.) as f32;
                        let py = center.y - (length * (j as f64 * step).sin() * 72.) as f32;
                        commands.push(if j == 0 {
                            PathCommand::MoveTo(px, py)
                        } else {
                            PathCommand::LineTo(px, py)
                        });
                    }
                    commands.push(PathCommand::Close);
                    let mut star = Path {
                        commands,
                        fill: bg[i % bg.len()],
                        stroke: Stroke::new(1., fg[i % fg.len()]),
                        anti_alias: true,
                    };
                    star.stroke.color = fg[i % fg.len()];
                    target.draw_path(&star);
                }
            }
            5 => {
                // thermometers: width, height, fraction(s) in [0,1]
                if nc != 3 && nc != 4 {
                    base_error("invalid thermometers data (need 3 or 4 columns)");
                }
                // GNU errors only when no thermometer fraction is finite.
                let fractions: Vec<f64> = (0..nr).map(|i| p(i, 2)).collect();
                if fractions.iter().all(|v| !v.is_finite()) {
                    base_error(if nc == 4 {
                        "invalid 'thermometers[, 3:4]'"
                    } else {
                        "invalid 'thermometers[, 3]'"
                    });
                }
                let flat: Vec<f64> = (0..2 * nr).map(|i| data[i]).collect();
                let Some((pmax, _)) = symbol_range(&flat) else {
                    base_error("invalid 'thermometers[, 1:2]'");
                };
                for i in 0..nr {
                    if ![x[i], y[i], p(i, 0), p(i, 1), p(i, 2)]
                        .iter()
                        .all(|v| v.is_finite())
                    {
                        continue;
                    }
                    let mut p2 = p(i, 2).clamp(0., 1.);
                    let mut p3 = if nc == 4 { p(i, 3).clamp(0., 1.) } else { 0. };
                    if !p3.is_finite() {
                        p3 = 0.;
                    }
                    if !p2.is_finite() {
                        p2 = 0.;
                    }
                    let (mut p0, mut p1) = (p(i, 0), p(i, 1));
                    if inches > 0. {
                        p0 *= inches / pmax;
                        p1 *= inches / pmax;
                    }
                    let rx = (0.5 * p0 * 72.) as f32;
                    let ry = (0.5 * p1 * 72.) as f32;
                    let center = c.map(x[i], y[i]);
                    let fg_i = fg[i % fg.len()];
                    draw_rect_symbol(
                        target,
                        center.x - rx,
                        center.y - ry,
                        center.x + rx,
                        center.y + ry,
                        bg[i % bg.len()],
                        fg_i,
                    );
                    let ya = center.y - (1. - 2. * p2 as f32) * ry;
                    let yb = center.y - (1. - 2. * p3 as f32) * ry;
                    let mut bulb = Path::rect(center.x - rx, ya.min(yb), 2. * rx, (yb - ya).abs());
                    bulb.fill = fg_i;
                    bulb.stroke = Stroke::new(0., fg_i);
                    target.draw_path(&bulb);
                    line(
                        target,
                        Point {
                            x: center.x - rx,
                            y: center.y,
                        },
                        Point {
                            x: center.x - 1.5 * rx,
                            y: center.y,
                        },
                        Stroke::new(1., fg_i),
                    );
                    line(
                        target,
                        Point {
                            x: center.x + rx,
                            y: center.y,
                        },
                        Point {
                            x: center.x + 1.5 * rx,
                            y: center.y,
                        },
                        Stroke::new(1., fg_i),
                    );
                }
            }
            6 => {
                // boxplots: width, height, lower/upper whisker, median proportion
                if nc != 5 {
                    base_error("invalid 'boxplots' data (need 5 columns)");
                }
                let flat: Vec<f64> = (0..4 * nr).map(|i| data[i]).collect();
                let Some((pmax, _)) = symbol_range(&flat) else {
                    base_error("invalid 'boxplots[, 1:4]'");
                };
                for i in 0..nr {
                    if ![x[i], y[i], p(i, 0), p(i, 1), p(i, 2), p(i, 3), p(i, 4)]
                        .iter()
                        .all(|v| v.is_finite())
                    {
                        continue;
                    }
                    let (mut p0, mut p1, mut p2, mut p3) = (p(i, 0), p(i, 1), p(i, 2), p(i, 3));
                    let p4 = p(i, 4);
                    if inches > 0. {
                        let f = inches / pmax * 72.;
                        p0 *= f;
                        p1 *= f;
                        p2 *= f;
                        p3 *= f;
                    } else {
                        // USER extents become device extents through the
                        // 72-units-per-inch metric, like GConvertXUnits.
                        p0 *= 72. / user_per_inch(&c, false);
                        p1 *= 72. / user_per_inch(&c, true);
                        p2 *= 72. / user_per_inch(&c, true);
                        p3 *= 72. / user_per_inch(&c, true);
                    }
                    let center = c.map(x[i], y[i]);
                    let rx = (0.5 * p0) as f32;
                    let ry = (0.5 * p1) as f32;
                    let median = ((1. - p4) * (0. - ry as f64) + p4 * (ry as f64)) as f32;
                    let fg_i = fg[i % fg.len()];
                    draw_rect_symbol(
                        target,
                        center.x - rx,
                        center.y - ry,
                        center.x + rx,
                        center.y + ry,
                        bg[i % bg.len()],
                        fg_i,
                    );
                    let whisker = Stroke::new(1., fg_i);
                    line(
                        target,
                        Point {
                            x: center.x - rx,
                            y: center.y + median,
                        },
                        Point {
                            x: center.x + rx,
                            y: center.y + median,
                        },
                        whisker.clone(),
                    );
                    line(
                        target,
                        Point {
                            x: center.x,
                            y: center.y - ry,
                        },
                        Point {
                            x: center.x,
                            y: center.y - ry - p2 as f32,
                        },
                        whisker.clone(),
                    );
                    line(
                        target,
                        Point {
                            x: center.x,
                            y: center.y + ry,
                        },
                        Point {
                            x: center.x,
                            y: center.y + ry + p3 as f32,
                        },
                        whisker.clone(),
                    );
                }
            }
            _ => base_error("invalid symbol type"),
        }
        target.set_clip(None);
        invisible()
    }
}

fn draw_rect_symbol(
    target: &mut dyn r_graphics_engine::DrawTarget,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    fill: Color,
    border: Color,
) {
    let mut rect = Path::rect(x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs());
    rect.fill = fill;
    rect.stroke = Stroke::new(1., border);
    target.draw_path(&rect);
}

/// GNU `C_xspline` — X-spline evaluation through the vendored plot.c/
/// engine xspline code, computed in device space like `GEXspline`.
unsafe fn xspline(args: SEXP) -> SEXP {
    unsafe {
        let c = current();
        let b = bindings(
            args,
            &["x", "y", "s", "open", "repEnds", "draw", "col", "border"],
        );
        let x = values(b[0]);
        let y = values(b[1]);
        let shape = values(b[2]);
        let open = logical_flag(b[3]);
        let rep_ends = logical_flag(b[4]);
        let draw = logical_flag(b[5]);
        let col = colors(b[6], transparent());
        let border = colors(b[7], par_color("fg", Color::BLACK));
        if x.len() != y.len() || shape.len() != x.len() {
            base_error("invalid 'x' or 'y'");
        }
        let device: Vec<(f64, f64)> = x
            .iter()
            .zip(&y)
            .map(|(ux, uy)| {
                let p = c.map(*ux, *uy);
                (f64::from(p.x), f64::from(p.y))
            })
            .collect();
        let ddx: Vec<std::os::raw::c_double> = device.iter().map(|p| p.0).collect();
        let ddy: Vec<std::os::raw::c_double> = device.iter().map(|p| p.1).collect();
        let points = if open {
            xspline::compute_open_spline_points(
                &ddx,
                &ddy,
                &shape,
                rep_ends,
                xspline::LOW_PRECISION,
                std::ptr::null_mut(),
            )
        } else {
            xspline::compute_closed_spline_points(
                &ddx,
                &ddy,
                &shape,
                xspline::LOW_PRECISION,
                std::ptr::null_mut(),
            )
        };
        if !draw {
            let nx = points.len();
            let sx = Rf_allocVector3(SEXPTYPE::REALSXP, nx as R_xlen_t);
            let _sx = protect(sx);
            let sy = Rf_allocVector3(SEXPTYPE::REALSXP, nx as R_xlen_t);
            let _sy = protect(sy);
            for (i, p) in points.iter().enumerate() {
                *REAL(sx).add(i) = device_x_to_user(&c, p.x);
                *REAL(sy).add(i) = device_y_to_user(&c, p.y);
            }
            let ans = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            let _ans = protect(ans);
            SET_VECTOR_ELT(ans, 0, sx);
            SET_VECTOR_ELT(ans, 1, sy);
            let names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
            let _names = protect(names);
            SET_STRING_ELT(names, 0, Rf_mkChar(b"x\0".as_ptr().cast()));
            SET_STRING_ELT(names, 1, Rf_mkChar(b"y\0".as_ptr().cast()));
            crate::attrib_core::setAttrib(ans, crate::attrib_core::R_NamesSymbol(), names);
            crate::sexp::globals::set_R_Visible(1);
            return ans;
        }
        let stroke = style(args, point_size(args)).stroke(0);
        let target = &mut *renderer();
        target.set_clip(Some(clip_rect(c, args)));
        let mut commands = Vec::new();
        for (i, p) in points.iter().enumerate() {
            let point = Point {
                x: p.x as f32,
                y: p.y as f32,
            };
            commands.push(if i == 0 {
                PathCommand::MoveTo(point.x, point.y)
            } else {
                PathCommand::LineTo(point.x, point.y)
            });
        }
        if !open {
            commands.push(PathCommand::Close);
        }
        let mut spline = Path {
            commands,
            fill: if open { transparent() } else { col[0] },
            stroke,
            anti_alias: true,
        };
        spline.stroke.color = border[0];
        target.draw_path(&spline);
        target.set_clip(None);
        invisible()
    }
}

/// GNU `C_locator` — the scene device has no locator callback, so `GLocator`
/// fails on the first click: NA vectors and a zero count. The result
/// builder (shared with the device-less profile) lives in the graphics
/// registry module.
unsafe fn locator(args: SEXP) -> SEXP {
    unsafe {
        let _ = current();
        crate::library::graphics::locator_result(args)
    }
}

/// GNU `C_identify` — zero clicks identifies nothing: all-false `ind` with
/// zero `pos`/`order` (the R wrapper reduces this to integer(0)).
unsafe fn identify(args: SEXP) -> SEXP {
    unsafe {
        let _ = current();
        crate::library::graphics::identify_result(args)
    }
}

/// Dendrogram drawing state (GNU's static dnd_* globals, per call).
struct Dendrogram {
    left: Vec<i32>,
    right: Vec<i32>,
    heights: Vec<f64>,
    xpos: Vec<f64>,
    hang: f64,
    offset: f64,
}

impl Dendrogram {
    /// GNU `drawdend`: recurse left and right, draw the U-shaped segment,
    /// and return the merge anchor.
    unsafe fn draw(
        &self,
        node: usize,
        labels: &[Option<String>],
        target: &mut dyn r_graphics_engine::DrawTarget,
        c: Coordinates,
        stroke: Stroke,
        text: &r_graphics_engine::PlotParameters,
        text_color: Color,
    ) -> (f64, f64) {
        unsafe {
            let y = self.heights[node - 1];
            let leaf = |k: i32, y: f64| -> (f64, f64) {
                let x = self.xpos[(-k - 1) as usize];
                let ly = if self.hang >= 0. { y - self.hang } else { 0. };
                (x, ly)
            };
            let (xl, yl) = if self.left[node - 1] > 0 {
                self.draw(
                    self.left[node - 1] as usize,
                    labels,
                    target,
                    c,
                    stroke.clone(),
                    text,
                    text_color,
                )
            } else {
                let (x, ly) = leaf(self.left[node - 1], y);
                let leaf_index = (-(self.left[node - 1]) - 1) as usize;
                self.draw_label(labels, leaf_index, x, ly, target, c, text, text_color);
                (x, ly)
            };
            let (xr, yr) = if self.right[node - 1] > 0 {
                self.draw(
                    self.right[node - 1] as usize,
                    labels,
                    target,
                    c,
                    stroke.clone(),
                    text,
                    text_color,
                )
            } else {
                let (x, ry) = leaf(self.right[node - 1], y);
                let leaf_index = (-(self.right[node - 1]) - 1) as usize;
                self.draw_label(labels, leaf_index, x, ry, target, c, text, text_color);
                (x, ry)
            };
            let a = c.map(xl, yl);
            let b = c.map(xl, y);
            let d = c.map(xr, y);
            let e = c.map(xr, yr);
            line(target, a, b, stroke.clone());
            line(target, b, d, stroke.clone());
            line(target, d, e, stroke.clone());
            (0.5 * (xl + xr), y)
        }
    }

    unsafe fn draw_label(
        &self,
        labels: &[Option<String>],
        leaf: usize,
        x: f64,
        y: f64,
        target: &mut dyn r_graphics_engine::DrawTarget,
        c: Coordinates,
        text: &r_graphics_engine::PlotParameters,
        text_color: Color,
    ) {
        unsafe {
            let Some(label) = &labels[leaf] else { return };
            let mut params = text.clone();
            params.text_angle = 90.;
            params.text_anchor = TextAnchor::End;
            params.text_color = text_color;
            Label::Text(label.clone()).draw(target, c.map(x, y - self.offset), &params);
        }
    }
}

/// GNU `C_dend` — draw the (vertical) hclust dendrogram.
unsafe fn dend(args: SEXP) -> SEXP {
    unsafe {
        let c = current();
        let b = bindings(args, &["n", "merge", "height", "ord", "hang", "labels"]);
        let n = asInteger(b[0]);
        if n == NA_INTEGER || n < 2 {
            base_error("invalid dendrogram input");
        }
        let n = n as usize;
        if SEXPTYPE(TYPEOF(b[1])) != SEXPTYPE::INTSXP || XLENGTH(b[1]) as usize != 2 * n {
            base_error("invalid dendrogram input");
        }
        let merge = ints(b[1]);
        if SEXPTYPE(TYPEOF(b[2])) != SEXPTYPE::REALSXP || XLENGTH(b[2]) as usize != n {
            base_error("invalid dendrogram input");
        }
        let heights = values(b[2]);
        if XLENGTH(b[3]) as usize != n + 1 {
            base_error("invalid dendrogram input");
        }
        let ord = values(b[3]);
        let hang = asReal(b[4]);
        if !hang.is_finite() {
            base_error("invalid dendrogram input");
        }
        if SEXPTYPE(TYPEOF(b[5])) != SEXPTYPE::STRSXP || XLENGTH(b[5]) as usize != n + 1 {
            base_error("invalid dendrogram input");
        }
        let labels: Vec<Option<String>> = (0..=n)
            .map(|i| {
                if STRING_ELT(b[5], i as R_xlen_t) == R_NaString() {
                    None
                } else {
                    Some(crate::mainutils::essentials::elt_to_string(
                        b[5],
                        i as R_xlen_t,
                    ))
                }
            })
            .collect();
        let dend = Dendrogram {
            left: merge[..n].to_vec(),
            right: merge[n..].to_vec(),
            hang: hang * (heights[n - 1] - heights[0]),
            offset: 0.,
            heights,
            xpos: ord,
        };
        let style = style(args, point_size(args));
        let mut params =
            crate::library::graphics::text_metrics::drawing_parameters(args, 1).remove(0);
        params.text_color = style.color(0);
        let target = &mut *renderer();
        // The label offset is the width of "m" converted to user units.
        let m_width = Label::Text("m".into()).dimensions(target, &params).0 as f64;
        let offset = m_width / 72. * user_per_inch(&c, true);
        let dend = Dendrogram { offset, ..dend };
        // GNU overrides par("xpd") up to the figure region.
        target.set_clip(Some(xpd_region(c, 1)));
        dend.draw(
            n,
            &labels,
            target,
            c,
            style.stroke(0),
            &params,
            style.color(0),
        );
        target.set_clip(None);
        invisible()
    }
}

/// GNU `C_dendwindow` — install the dendrogram's user window, leaving room
/// for the deepest leaf label.
unsafe fn dendwindow(args: SEXP) -> SEXP {
    unsafe {
        let c = current();
        let b = bindings(args, &["n", "merge", "height", "hang", "labels"]);
        let n = asInteger(b[0]);
        if n == NA_INTEGER || n < 2 {
            base_error("invalid dendrogram input");
        }
        let n = n as usize;
        if SEXPTYPE(TYPEOF(b[1])) != SEXPTYPE::INTSXP || XLENGTH(b[1]) as usize != 2 * n {
            base_error("invalid dendrogram input");
        }
        let merge = ints(b[1]);
        if SEXPTYPE(TYPEOF(b[2])) != SEXPTYPE::REALSXP || XLENGTH(b[2]) as usize != n {
            base_error("invalid dendrogram input");
        }
        let heights = values(b[2]);
        let hang = asReal(b[3]);
        if !hang.is_finite() {
            base_error("invalid dendrogram input");
        }
        if SEXPTYPE(TYPEOF(b[4])) != SEXPTYPE::STRSXP || XLENGTH(b[4]) as usize != n + 1 {
            base_error("invalid dendrogram input");
        }
        let params = crate::library::graphics::text_metrics::drawing_parameters(args, 1).remove(0);
        let target = &mut *renderer();
        let offset = Label::Text("m".into()).dimensions(target, &params).0 as f64 / 72.;
        let mut ymax = heights[0];
        let mut ymin = heights[0];
        for &h in &heights {
            ymax = ymax.max(h);
            ymin = ymin.min(h);
        }
        let pin = f64::from(c.rect[3] - c.rect[1]) / 72.;
        let mut ll = Vec::with_capacity(n + 1);
        for i in 0..=n {
            let string = STRING_ELT(b[4], i as R_xlen_t);
            ll.push(if string == R_NaString() {
                0.
            } else {
                Label::Text(crate::mainutils::essentials::elt_to_string(
                    b[4],
                    i as R_xlen_t,
                ))
                .dimensions(target, &params)
                .0 as f64
                    / 72.
                    + offset
            });
        }
        let mut leaf_height = vec![0f64; n + 1];
        let (mut imax, mut yval, yrange);
        if hang >= 0. {
            ymin = ymax - (1. + hang) * (ymax - ymin);
            yrange = ymax - ymin;
            for i in 0..n {
                if merge[i] < 0 {
                    leaf_height[(-merge[i] - 1) as usize] = heights[i];
                }
                if merge[i + n] < 0 {
                    leaf_height[(-merge[i + n] - 1) as usize] = heights[i];
                }
            }
            imax = -1i32;
            yval = -f64::MAX;
            for i in 0..=n {
                let tmp = ((ymax - leaf_height[i]) / yrange) * pin + ll[i];
                if tmp > yval {
                    yval = tmp;
                    imax = i as i32;
                }
            }
        } else {
            yrange = ymax;
            imax = -1i32;
            yval = -f64::MAX;
            for i in 0..=n {
                let tmp = pin + ll[i];
                if tmp > yval {
                    yval = tmp;
                    imax = i as i32;
                }
            }
        }
        let ymin = ymax - (pin / (pin - ll[imax as usize])) * yrange;
        install(Coordinates {
            limits: [1., (n + 1) as f64, ymin, ymax],
            rect: c.rect,
            figure: c.figure,
            device: c.device,
            log: [false, false],
        });
        invisible()
    }
}

/// Decoded drawing inputs handed over by plot3d's `C_persp`.
pub(crate) struct PerspInput<'a> {
    pub x: &'a [f64],
    pub y: &'a [f64],
    /// Column-major surface matrix with `x.len()` rows.
    pub z: &'a [f64],
    pub xlim: [f64; 2],
    pub ylim: [f64; 2],
    pub zlim: [f64; 2],
    pub col: SEXP,
    pub border: SEXP,
    pub dobox: bool,
    pub doaxes: bool,
    pub n_ticks: i32,
    pub tick_type: i32,
    pub xlab: SEXP,
    pub ylab: SEXP,
    pub zlab: SEXP,
    pub indx: &'a [i32],
    pub vt: [[f64; 4]; 4],
    /// `1/xs`, `1/ys`, `expand/zs` for facet lighting.
    pub inv_scales: [f64; 3],
    pub lighting: bool,
}

fn project(vt: &[[f64; 4]; 4], p: [f64; 3]) -> (f64, f64) {
    let u = [p[0], p[1], p[2], 1.];
    let mut v = [0f64; 4];
    for i in 0..4 {
        let mut sum = 0.;
        for j in 0..4 {
            sum += u[j] * vt[j][i];
        }
        v[i] = sum;
    }
    (v[0] / v[3], v[1] / v[3])
}

/// The drawing half of GNU `C_persp`: `PerspWindow`, back box faces, axes,
/// painter-ordered facets and the dotted front edges.
pub(crate) unsafe fn persp_render(input: &PerspInput) {
    unsafe {
        if !renderplot_backend_active() {
            // Null-device semantics: the transformation still exists, but
            // there is nothing to draw on.
            return;
        }
        let cols = colors(input.col, par_color("bg", Color::WHITE));
        if cols.is_empty() {
            base_error("invalid 'col' specification");
        }
        let borders = colors(input.border, par_color("fg", Color::BLACK));
        if borders.is_empty() {
            base_error("invalid 'border' specification");
        }
        let lighting = input.lighting && cols[0].a != 0;
        // PerspWindow: map the box corners, pick a symmetric window.
        let base = current();
        let vt = input.vt;
        let (mut xmin, mut xmax, mut ymin, mut ymax) = (0f64, 0f64, 0f64, 0f64);
        for &x in &input.xlim {
            for &y in &input.ylim {
                for &z in &input.zlim {
                    let (px, py) = project(&vt, [x, y, z]);
                    xmin = xmin.min(px);
                    xmax = xmax.max(px);
                    ymin = ymin.min(py);
                    ymax = ymax.max(py);
                }
            }
        }
        let pin1 = f64::from(base.rect[2] - base.rect[0]) / 72.;
        let pin2 = f64::from(base.rect[3] - base.rect[1]) / 72.;
        let xdelta = (xmax - xmin).abs();
        let ydelta = (ymax - ymin).abs();
        let scale = (pin1 / xdelta).min(pin2 / ydelta);
        let xadd = 0.5 * (pin1 / scale - xdelta);
        let yadd = 0.5 * (pin2 / scale - ydelta);
        let c = Coordinates {
            limits: [xmin - xadd, xmax + xadd, ymin - yadd, ymax + yadd],
            rect: base.rect,
            figure: base.figure,
            device: base.device,
            log: [false, false],
        };
        install(c);
        let target = &mut *renderer();
        target.set_clip(Some(c.rect));
        let line_color = par_color("col", Color::BLACK);
        let mut edge_done = [false; 12];
        if input.dobox {
            persp_box(
                target,
                c,
                input,
                0,
                &mut edge_done,
                Stroke::new(1., line_color),
            );
            if input.doaxes {
                persp_axes(target, c, input);
            }
        }
        // DrawFacets, back to front.
        let nx1 = input.x.len() - 1;
        let ny1 = input.y.len() - 1;
        for &facet in input.indx {
            let facet = facet as usize;
            let i = facet % nx1;
            let j = facet / nx1;
            let icol = facet % cols.len();
            let corners = [
                [input.x[i], input.y[j], input.z[i + j * input.x.len()]],
                [
                    input.x[i + 1],
                    input.y[j],
                    input.z[i + 1 + j * input.x.len()],
                ],
                [
                    input.x[i + 1],
                    input.y[j + 1],
                    input.z[i + 1 + (j + 1) * input.x.len()],
                ],
                [
                    input.x[i],
                    input.y[j + 1],
                    input.z[i + (j + 1) * input.x.len()],
                ],
            ];
            if corners.iter().any(|p| p.iter().any(|v| !v.is_finite())) {
                continue;
            }
            let mut shade = 0.;
            if lighting {
                let u = [
                    input.inv_scales[0] * (input.x[i + 1] - input.x[i]),
                    input.inv_scales[1] * (input.y[j] - input.y[j + 1]),
                    input.inv_scales[2]
                        * (input.z[i + 1 + j * input.x.len()]
                            - input.z[i + (j + 1) * input.x.len()]),
                ];
                let v = [
                    input.inv_scales[0] * (input.x[i + 1] - input.x[i]),
                    input.inv_scales[1] * (input.y[j + 1] - input.y[j]),
                    input.inv_scales[2]
                        * (input.z[i + 1 + (j + 1) * input.x.len()]
                            - input.z[i + j * input.x.len()]),
                ];
                shade = FacetShade(&u, &v);
            }
            let projected: Vec<(f64, f64)> = corners.iter().map(|p| project(&vt, *p)).collect();
            if projected.len() < 3 {
                continue;
            }
            let mut fill = cols[icol];
            if lighting && shade.is_finite() {
                fill.r = (shade * f64::from(fill.r)) as u8;
                fill.g = (shade * f64::from(fill.g)) as u8;
                fill.b = (shade * f64::from(fill.b)) as u8;
            } else if lighting && !shade.is_finite() {
                continue;
            }
            let mut commands = Vec::new();
            for (k, (px, py)) in projected.iter().enumerate() {
                let point = c.map(*px, *py);
                commands.push(if k == 0 {
                    PathCommand::MoveTo(point.x, point.y)
                } else {
                    PathCommand::LineTo(point.x, point.y)
                });
            }
            commands.push(PathCommand::Close);
            let border = borders[0];
            let mut facet_path = Path {
                commands,
                fill,
                stroke: Stroke::new(if border.a == 0 { 0. } else { 1. }, border),
                anti_alias: true,
            };
            facet_path.stroke.color = border;
            target.draw_path(&facet_path);
        }
        if input.dobox {
            let mut dotted = Stroke::new(1., line_color);
            dotted.dash_pattern = Some(r_graphics_engine::DashPattern {
                intervals: vec![1., 3.],
                offset: 0.,
            });
            persp_box(target, c, input, 1, &mut edge_done, dotted);
        }
        target.set_clip(None);
    }
}

/// GNU `PerspBox`: draw the box edges of the faces facing (front=1) or
/// facing away from (front=0) the viewer, each edge only once.
unsafe fn persp_box(
    target: &mut dyn r_graphics_engine::DrawTarget,
    c: Coordinates,
    input: &PerspInput,
    front: u8,
    edge_done: &mut [bool; 12],
    stroke: Stroke,
) {
    unsafe {
        let vt = input.vt;
        for f in 0..6 {
            let p: Vec<(f64, f64)> = (0..4)
                .map(|k| {
                    let idx = FACE[f][k] as usize;
                    project(
                        &vt,
                        [
                            pick(input.xlim, VERTEX[idx][0] as usize),
                            pick(input.ylim, VERTEX[idx][1] as usize),
                            pick(input.zlim, VERTEX[idx][2] as usize),
                        ],
                    )
                })
                .collect();
            let d = [
                p[1].0 - p[0].0,
                p[1].1 - p[0].1,
                p[2].0 - p[1].0,
                p[2].1 - p[1].1,
            ];
            let nearby = (d[0] * d[3] - d[1] * d[2]) < 0.;
            if (front == 1 && nearby) || (front == 0 && !nearby) {
                for e in 0..4 {
                    let edge = EDGE[f][e] as usize;
                    if edge_done[edge] {
                        continue;
                    }
                    edge_done[edge] = true;
                    let a = c.map(p[e].0, p[e].1);
                    let b = c.map(p[(e + 1) % 4].0, p[(e + 1) % 4].1);
                    line(target, a, b, stroke.clone());
                }
            }
        }
    }
}

fn pick(lim: [f64; 2], which: usize) -> f64 {
    if which == 0 { lim[0] } else { lim[1] }
}

fn label_angle(x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let dx = (x2 - x1).abs();
    let dy = if x2 > x1 { y2 - y1 } else { y1 - y2 };
    if dx == 0. {
        if dy > 0. { 90. } else { 270. }
    } else {
        (180. / std::f64::consts::PI) * dy.atan2(dx)
    }
}

fn lowest(y1: f64, y2: f64, y3: f64, y4: f64) -> bool {
    y1 <= y2 && y1 <= y3 && y1 <= y4
}

/// GNU `PerspAxes`: label the lowest x/y corners and the leftmost z edge.
unsafe fn persp_axes(
    target: &mut dyn r_graphics_engine::DrawTarget,
    c: Coordinates,
    input: &PerspInput,
) {
    unsafe {
        let vt = input.vt;
        let v0 = project(&vt, [input.xlim[0], input.ylim[0], input.zlim[0]]);
        let v1 = project(&vt, [input.xlim[1], input.ylim[0], input.zlim[0]]);
        let v2 = project(&vt, [input.xlim[0], input.ylim[1], input.zlim[0]]);
        let v3 = project(&vt, [input.xlim[1], input.ylim[1], input.zlim[0]]);
        let (x_axis, y_axis) = if lowest(v0.1, v1.1, v2.1, v3.1) {
            (0, 1)
        } else if lowest(v1.1, v0.1, v2.1, v3.1) {
            (0, 3)
        } else if lowest(v2.1, v1.1, v0.1, v3.1) {
            (2, 1)
        } else if lowest(v3.1, v1.1, v2.1, v0.1) {
            (2, 3)
        } else {
            (0, 1)
        };
        let z_axis = if lowest(v0.0, v1.0, v2.0, v3.0) {
            4
        } else if lowest(v1.0, v0.0, v2.0, v3.0) {
            5
        } else if lowest(v2.0, v1.0, v0.0, v3.0) {
            6
        } else if lowest(v3.0, v1.0, v2.0, v0.0) {
            7
        } else {
            4
        };
        let lab = |s: SEXP| -> String {
            if XLENGTH(s) == 0 || STRING_ELT(s, 0) == R_NaString() {
                String::new()
            } else {
                crate::mainutils::essentials::elt_to_string(s, 0)
            }
        };
        persp_axis(target, c, input, x_axis, 0, &lab(input.xlab));
        persp_axis(target, c, input, y_axis, 1, &lab(input.ylab));
        persp_axis(target, c, input, z_axis, 2, &lab(input.zlab));
    }
}

unsafe fn persp_axis(
    target: &mut dyn r_graphics_engine::DrawTarget,
    c: Coordinates,
    input: &PerspInput,
    axis: usize,
    axis_type: usize,
    label: &str,
) {
    unsafe {
        let vt = input.vt;
        let tick_length = 0.03;
        let range = match axis_type {
            0 => input.xlim,
            1 => input.ylim,
            _ => input.zlim,
        };
        let ticks = pretty_linear_ticks(range[0], range[1], input.n_ticks.max(1) as f64);
        let (min, max) = (
            ticks.first().copied().unwrap_or(range[0]),
            ticks.last().copied().unwrap_or(range[1]),
        );
        let start = AXIS_START[axis] as usize;
        let mut base_point = [
            pick(input.xlim, VERTEX[start][0] as usize),
            pick(input.ylim, VERTEX[start][1] as usize),
            pick(input.zlim, VERTEX[start][2] as usize),
        ];
        match axis_type {
            0 => base_point[0] = min,
            1 => base_point[1] = min,
            _ => base_point[2] = min,
        }
        let tick = [
            tick_length * (input.xlim[1] - input.xlim[0]) * TICK_VECTOR[axis][0] as f64,
            tick_length * (input.ylim[1] - input.ylim[0]) * TICK_VECTOR[axis][1] as f64,
            tick_length * (input.zlim[1] - input.zlim[0]) * TICK_VECTOR[axis][2] as f64,
        ];
        let mut u1 = base_point;
        u1[0] += tick[0];
        u1[1] += tick[1];
        u1[2] += tick[2];
        let mut u2 = u1;
        match axis_type {
            0 => u2[0] = max,
            1 => u2[1] = max,
            _ => u2[2] = max,
        }
        let mut u3 = u1;
        let offset = if input.tick_type == 1 { 1. } else { 2.5 };
        u3[0] += offset * tick[0];
        u3[1] += offset * tick[1];
        u3[2] += offset * tick[2];
        match axis_type {
            0 => u3[0] = (min + max) / 2.,
            1 => u3[1] = (min + max) / 2.,
            _ => u3[2] = (min + max) / 2.,
        }
        let v1 = project(&vt, u1);
        let v2 = project(&vt, u2);
        let v3 = project(&vt, u3);
        let stroke = Stroke::new(1., par_color("col", Color::BLACK));
        let label_params = r_graphics_engine::PlotParameters {
            font_size: par_numbers("cex.lab").first().copied().unwrap_or(1.) as f32 * 12.,
            text_color: par_color("col.lab", par_color("fg", Color::BLACK)),
            text_anchor: TextAnchor::Middle,
            text_angle: label_angle(v1.0, v1.1, v2.0, v2.1) as f32,
            ..Default::default()
        };
        Label::Text(label.to_string()).draw(target, c.map(v3.0, v3.1), &label_params);
        if input.tick_type == 1 {
            // Arrow parallel to the axis, at the v2 end.
            let a = c.map(v1.0, v1.1);
            let b = c.map(v2.0, v2.1);
            line(target, a, b, stroke.clone());
            let theta = (b.y - a.y).atan2(b.x - a.x);
            for sign in [-1f32, 1.] {
                let t = theta + sign * 10f32.to_radians();
                line(
                    target,
                    b,
                    Point {
                        x: b.x + 0.1f32 * 72. * t.cos(),
                        y: b.y + 0.1f32 * 72. * t.sin(),
                    },
                    stroke.clone(),
                );
            }
        } else {
            let tick_params = r_graphics_engine::PlotParameters {
                font_size: par_numbers("cex.axis").first().copied().unwrap_or(1.) as f32 * 10.,
                text_color: par_color("col.axis", par_color("fg", Color::BLACK)),
                text_anchor: TextAnchor::Middle,
                ..Default::default()
            };
            for &value in &ticks {
                let mut t1 = base_point;
                match axis_type {
                    0 => t1[0] = value,
                    1 => t1[1] = value,
                    _ => t1[2] = value,
                }
                let mut t2 = t1;
                t2[0] += tick[0];
                t2[1] += tick[1];
                t2[2] += tick[2];
                let mut t3 = t2;
                t3[0] += tick[0];
                t3[1] += tick[1];
                t3[2] += tick[2];
                let w1 = project(&vt, t1);
                let w2 = project(&vt, t2);
                let w3 = project(&vt, t3);
                line(target, c.map(w1.0, w1.1), c.map(w2.0, w2.1), stroke.clone());
                let text = if (value - value.round()).abs() < 1e-9 {
                    format!("{}", value.round() as i64)
                } else {
                    format!("{value}")
                };
                Label::Text(text).draw(target, c.map(w3.0, w3.1), &tick_params);
            }
        }
    }
}

/// The drawing half of GNU `C_filledcontour`: colored cell bands between
/// consecutive contour levels (GNU forces clipping to the plot region).
pub(crate) unsafe fn filledcontour_render(
    x: &[f64],
    y: &[f64],
    z: &[f64],
    levels: &[f64],
    col: SEXP,
) {
    unsafe {
        if !renderplot_backend_active() {
            return;
        }
        let c = current();
        let ncol = XLENGTH(col) as usize;
        let cols: Vec<Color> = (0..ncol)
            .map(|i| {
                let v = INTEGER_ELT(col, i as i32) as u32;
                Color {
                    r: v as u8,
                    g: (v >> 8) as u8,
                    b: (v >> 16) as u8,
                    a: (v >> 24) as u8,
                }
            })
            .collect();
        let target = &mut *renderer();
        target.set_clip(Some(c.rect));
        let nx = x.len();
        let mut px = [0f64; 8];
        let mut py = [0f64; 8];
        let mut pz = [0f64; 8];
        let mut npt = 0i32;
        for i in 1..x.len() {
            for j in 1..y.len() {
                for k in 1..levels.len() {
                    FindPolygonVertices(
                        levels[k - 1],
                        levels[k],
                        x[i - 1],
                        x[i],
                        y[j - 1],
                        y[j],
                        z[i - 1 + (j - 1) * nx],
                        z[i + (j - 1) * nx],
                        z[i - 1 + j * nx],
                        z[i + j * nx],
                        &mut px,
                        &mut py,
                        &mut pz,
                        &mut npt,
                    );
                    if npt <= 2 {
                        continue;
                    }
                    let mut commands = Vec::new();
                    for m in 0..npt as usize {
                        let p = c.map(px[m], py[m]);
                        commands.push(if m == 0 {
                            PathCommand::MoveTo(p.x, p.y)
                        } else {
                            PathCommand::LineTo(p.x, p.y)
                        });
                    }
                    commands.push(PathCommand::Close);
                    let band = Path {
                        commands,
                        fill: cols[(k - 1) % ncol.max(1)],
                        stroke: Stroke::new(0., transparent()),
                        anti_alias: true,
                    };
                    target.draw_path(&band);
                }
            }
        }
        target.set_clip(None);
    }
}

/// Backend check shared with the plot3d drawing hooks.
pub(crate) fn renderplot_backend_active() -> bool {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).current_renderplot_backend.is_some()
    })
}
