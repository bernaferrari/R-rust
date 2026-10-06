//! GNU margin text admission, recycling and device-space placement.
//! Decode owned arguments before borrowing the host's drawing target.
use super::*;
use crate::mainutils::{coerce::coerceVector, plotmath::Label};
use crate::sexp::{
    object::{Sexp, SexpResult},
    owner::OwnerToken,
};

const FORMALS: &[&str] = &[
    "text", "side", "line", "outer", "at", "adj", "padj", "cex", "col", "font",
];

fn checked<T>(value: SexpResult<T>) -> T {
    value.unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
}

fn length(value: &Sexp<'_>) -> usize {
    let n = usize::try_from(value.len()).unwrap_or_else(|_| base_error("invalid graphics length"));
    // GNU C_mtext uses LENGTH, not XLENGTH, for these arguments.
    if n > i32::MAX as usize {
        base_error("long vectors not supported yet");
    }
    n
}

unsafe fn coerced(value: &Sexp<'_>, kind: SEXPTYPE, owner: OwnerToken) -> Sexp<'static> {
    unsafe {
        checked(owner.require_active());
        let raw = coerceVector(value.as_raw(), kind.0);
        checked(owner.require_active());
        checked(checked(owner.sexp(raw)).into_owned())
    }
}

fn numbers(value: &Sexp<'_>, owner: OwnerToken) -> Vec<f64> {
    let n = length(value);
    let mut result = Vec::new();
    result
        .try_reserve_exact(n)
        .unwrap_or_else(|_| base_error("cannot reserve margin text arguments"));
    for i in 0..n {
        if i % 256 == 0 {
            crate::eval::limits::poll_computation();
        }
        let element = if value.typeof_() == SEXPTYPE::REALSXP {
            checked(value.try_real_elt(i as i64))
        } else {
            let element = checked(value.try_integer_elt(i as i64));
            if element == crate::sexp::ffi::NA_INTEGER {
                f64::NAN
            } else {
                f64::from(element)
            }
        };
        checked(owner.require_active());
        result.push(element);
    }
    result
}

unsafe fn color_values(value: &Sexp<'_>, owner: OwnerToken) -> Vec<Color> {
    unsafe {
        let fallback = par_color("col", Color::BLACK);
        if value.is_nil() {
            return vec![fallback];
        }
        let n = length(value);
        if n == 0 {
            return vec![Color {
                r: 255,
                g: 255,
                b: 255,
                a: 0,
            }];
        }
        let mut colors = super::colors(value.as_raw(), fallback);
        checked(owner.require_active());
        for (i, color) in colors.iter_mut().enumerate() {
            let missing = match value.typeof_() {
                SEXPTYPE::STRSXP => {
                    let text = checked(value.try_string_elt(i as i64));
                    text.is_na_string() || checked(text.try_char_eq(b"NA"))
                }
                SEXPTYPE::REALSXP => !checked(value.try_real_elt(i as i64)).is_finite(),
                SEXPTYPE::INTSXP => checked(value.try_integer_elt(i as i64)) == i32::MIN,
                SEXPTYPE::LGLSXP => checked(value.try_logical_elt(i as i64)) == i32::MIN,
                _ => base_error("invalid color specification"),
            };
            checked(owner.require_active());
            if missing {
                *color = fallback;
            }
        }
        colors
    }
}

fn default_adj(side: i32, las: i32) -> f64 {
    match (las, side) {
        (0, _) | (1, 1 | 3) | (3, 2 | 4) => 0.5,
        (1, 2) | (2, 1 | 2) | (3, 1) => 1.,
        (1, 4) | (2, 3 | 4) | (3, 3) => 0.,
        _ => f64::NAN,
    }
}

fn default_padj(side: i32, las: i32) -> f64 {
    match (las, side) {
        (0, _) | (1, 1 | 3) | (3, 2 | 4) => 0.,
        (2, _) | (1, 2 | 4) | (3, 1 | 3) => 0.5,
        _ => f64::NAN,
    }
}

fn position(c: Coordinates, side: i32, outer: i32, at: f64, line: f64, inner: [f32; 4]) -> Point {
    let along = if outer != 0 {
        if side % 2 == 0 {
            inner[3] as f64 - at * f64::from(inner[3] - inner[1])
        } else {
            inner[0] as f64 + at * f64::from(inner[2] - inner[0])
        }
    } else if side % 2 == 0 {
        c.map(0., at).y as f64
    } else {
        c.map(at, 0.).x as f64
    };
    let rect = if outer != 0 { inner } else { c.rect };
    match side {
        1 => Point {
            x: along as f32,
            y: (rect[3] as f64 + line) as f32,
        },
        2 => Point {
            x: (rect[0] as f64 - line) as f32,
            y: along as f32,
        },
        3 => Point {
            x: along as f32,
            y: (rect[1] as f64 - line) as f32,
        },
        4 => Point {
            x: (rect[2] as f64 + line) as f32,
            y: along as f32,
        },
        // GNU admits other side codes and leaves the coordinate system DEVICE.
        _ => Point {
            x: c.device[0] + at as f32,
            y: c.device[3] - line as f32,
        },
    }
}

pub(super) unsafe fn draw(c: Coordinates, args: SEXP) -> SEXP {
    unsafe {
        let owner = checked(OwnerToken::current());
        let _pin = checked(owner.pin());
        let _arguments = checked(checked(owner.sexp(args)).into_owned());
        if crate::sexp::constructors::Rf_length(args) < 9 {
            base_error("too few arguments");
        }
        let input: Vec<_> = bindings(args, FORMALS)
            .into_iter()
            .map(|value| checked(checked(owner.sexp(value)).into_owned()))
            .collect();
        let text = match input[0].typeof_() {
            SEXPTYPE::EXPRSXP => input[0].clone(),
            SEXPTYPE::LANGSXP | SEXPTYPE::SYMSXP => coerced(&input[0], SEXPTYPE::EXPRSXP, owner),
            _ => coerced(&input[0], SEXPTYPE::STRSXP, owner),
        };
        let ntext = length(&text);
        if ntext == 0 {
            base_error("zero-length 'text' specified");
        }
        let mut numeric = Vec::new();
        let mut count = ntext;
        for (index, name) in FORMALS[1..7].iter().enumerate() {
            let kind = if matches!(*name, "side" | "outer") {
                SEXPTYPE::INTSXP
            } else {
                SEXPTYPE::REALSXP
            };
            let value = coerced(&input[index + 1], kind, owner);
            let n = length(&value);
            if n == 0 {
                base_error(format!("zero-length '{name}' specified"));
            }
            count = count.max(n);
            numeric.push(value);
        }
        for value in &input[7..] {
            count = count.max(length(value).max(1));
        }
        if length(&input[7]) > 0
            && !matches!(
                input[7].typeof_(),
                SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP
            )
        {
            base_error("invalid 'cex' value");
        }
        let numeric_bytes = numeric.iter().try_fold(0usize, |sum, value| {
            length(value).checked_mul(8)?.checked_add(sum)
        });
        let bytes = count
            .checked_mul(
                std::mem::size_of::<PlotParameters>()
                    + std::mem::size_of::<r_graphics_engine::DrawOperation>(),
            )
            .and_then(|bytes| bytes.checked_add(numeric_bytes?))
            .and_then(|bytes| {
                bytes.checked_add(ntext.checked_mul(std::mem::size_of::<Label>() + 1)?)
            })
            .unwrap_or_else(|| base_error("margin text size overflow"));
        let _buffers = crate::sexp::memory::reserve_transient_in(owner.as_ptr(), bytes)
            .unwrap_or_else(|| base_error("margin text exceeds the heap budget"));
        let colors = color_values(&input[8], owner);
        if length(&input[9]) > 0
            && !matches!(
                input[9].typeof_(),
                SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP
            )
        {
            base_error("invalid font specification");
        }
        let las_value = arg(args, "las");
        let las = if las_value == R_NilValue() {
            par_numbers("las").first().copied().unwrap_or(0.) as i32
        } else {
            if XLENGTH(las_value) != 1 {
                base_error("graphical parameter \"las\" has the wrong length");
            }
            crate::mainutils::coerce::asInteger(las_value)
        };
        checked(owner.require_active());
        if !(0..=3).contains(&las) {
            base_error("invalid value specified for graphical parameter \"las\"");
        }
        let parameters = crate::library::graphics::text_metrics::margin_parameters(
            input[7].as_raw(),
            input[9].as_raw(),
            args,
            count,
        );
        checked(owner.require_active());
        let numeric: Vec<_> = numeric.iter().map(|value| numbers(value, owner)).collect();
        let mut missing = vec![false; ntext];
        let mut text_bytes = 0usize;
        if text.typeof_() == SEXPTYPE::STRSXP {
            for (i, missing) in missing.iter_mut().enumerate() {
                let character = checked(text.try_string_elt(i as i64));
                *missing = character.is_na_string();
                if !*missing {
                    text_bytes = text_bytes
                        .checked_add(
                            usize::try_from(checked(character.try_char_len()))
                                .unwrap_or_else(|_| base_error("margin text size overflow")),
                        )
                        .unwrap_or_else(|| base_error("margin text size overflow"));
                }
                checked(owner.require_active());
            }
        }
        // Include the decoded label and its recycled device copies. Lossy or
        // Latin-1 conversion can expand one source byte to three UTF-8 bytes.
        let bytes = text_bytes
            .checked_mul(3)
            .and_then(|bytes| bytes.checked_mul(count.div_ceil(ntext).checked_add(1)?))
            .unwrap_or_else(|| base_error("margin text size overflow"));
        let _text_bytes = crate::sexp::memory::reserve_transient_in(owner.as_ptr(), bytes)
            .unwrap_or_else(|| base_error("margin text exceeds the heap budget"));
        let labels = crate::mainutils::plotmath::labels(text.as_raw());
        checked(owner.require_active());
        let mex = par_numbers("mex").first().copied().unwrap_or(1.);
        let line_height = par_numbers("csi").first().copied().unwrap_or(0.2) * 72. * mex;
        let bias = par_numbers("ylbias").first().copied().unwrap_or(0.2);
        let lheight = par_numbers("lheight").first().copied().unwrap_or(1.);
        let oma = par_numbers("oma");
        let margin = |i| oma.get(i).copied().unwrap_or(0.) as f32 * line_height as f32;
        let inner = [
            c.device[0] + margin(1),
            c.device[1] + margin(2),
            c.device[2] - margin(3),
            c.device[3] - margin(0),
        ];
        let target = &mut *renderer();
        // C_mtext promotes its clip to the whole device, even for inner labels.
        target.set_clip(Some(c.device));
        for (i, mut params) in parameters.into_iter().enumerate() {
            crate::eval::limits::poll_computation();
            checked(owner.require_active());
            if missing[i % ntext] {
                continue;
            }
            let get = |index: usize| numeric[index][i % numeric[index].len()];
            let side = if get(0).is_finite() {
                get(0) as i32
            } else {
                i32::MIN
            };
            let outer = if get(2).is_finite() { get(2) as i32 } else { 0 };
            let vertical = matches!((side, las), (1 | 3, 2 | 3) | (2 | 4, 0 | 3));
            let mut adj = get(4);
            if !adj.is_finite() {
                adj = default_adj(side, las);
            }
            let mut padj = get(5);
            if !padj.is_finite() {
                padj = default_padj(side, las);
            }
            let mut at = get(3);
            if !at.is_finite() {
                let parallel = las == 0 || matches!((side, las), (1 | 3, 1) | (2 | 4, 3));
                let fraction = if parallel { adj } else { 0.5 };
                let axis = if side % 2 == 0 { 1 } else { 0 };
                at = if outer > 0 {
                    fraction
                } else {
                    c.raw(
                        axis,
                        c.limits[2 * axis]
                            + fraction * (c.limits[2 * axis + 1] - c.limits[2 * axis]),
                    )
                };
            }
            let mut line = get(1);
            match (side, vertical) {
                (1, false) | (4, true) => line += (1. - bias) / mex,
                (2, true) | (3, false) => line += bias / mex,
                _ => {}
            }
            let position = position(c, side, outer, at, line * line_height, inner);
            // Missing positions produce no ink; a portable scene stores finite coordinates.
            if !position.x.is_finite() || !position.y.is_finite() {
                continue;
            }
            params.text_angle = if vertical { 90. } else { 0. };
            params.text_anchor = TextAnchor::Start;
            params.text_color = colors[i % colors.len()];
            let (sin, cos) = (-params.text_angle.to_radians()).sin_cos();
            match &labels[i % ntext] {
                Label::Text(text) => {
                    if text.is_empty() {
                        continue;
                    }
                    let height = target.measure_math_text("M", &params).ascent;
                    checked(owner.require_active());
                    let lines = text.split('\n');
                    let nlines = lines.clone().count();
                    for (index, text) in lines.enumerate() {
                        let width = target.measure_text(text, &params).width;
                        checked(owner.require_active());
                        let x = -adj as f32 * width;
                        let y = padj as f32 * height
                            - ((1. - padj) * (nlines - 1) as f64 - index as f64) as f32
                                * params.font_size
                                * lheight as f32
                                * 1.2;
                        target.draw_text(
                            text,
                            Point {
                                x: position.x + x * cos - y * sin,
                                y: position.y + x * sin + y * cos,
                            },
                            &params,
                        );
                        checked(owner.require_active());
                    }
                }
                Label::Math(expr) => {
                    let layout = expr.layout(target, &params);
                    checked(owner.require_active());
                    let x = -adj as f32 * layout.width;
                    let y = padj as f32 * (layout.ascent + layout.descent) - layout.descent;
                    layout.draw(
                        target,
                        Point {
                            x: position.x + x * cos - y * sin,
                            y: position.y + x * sin + y * cos,
                        },
                        &params,
                    );
                    checked(owner.require_active());
                }
            }
        }
        target.set_clip(None);
        checked(owner.require_active());
        invisible()
    }
}
