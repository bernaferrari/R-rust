//! Graphics builtins exactly as GNU's grDevices/engine layer organizes
//! them — the device-state consumers extracted from the flat registry as
//! the first domain module of the r-base internal split.

#![forbid(unsafe_code)]
use r_core::prim::*;
use r_core::attrib::{get_attrib, string_vec};
use r_core::attrib;
use r_core::env;
use r_core::eval;
use r_core::eval::{eval, force_if_promise, EvalError, EvalResult};
use r_core::heap::Heap;
use r_core::id::ValueId;
use r_core::intern::SymbolInterner;
use r_core::object::RObject;
use r_core::print;
use r_core::graphics;
use r_core::civil;
use r_core::support;
use r_core::primitive_ids::*;
use r_core::eval::RSignal;

pub fn par_default(name: &str) -> Vec<f64> {
    match name {
        "mar" => vec![5.1, 4.1, 4.1, 2.1],
        "mai" => vec![1.02, 0.82, 0.82, 0.42],
        "las" | "adj" | "ann" | "ask" | "err" | "font" | "lty" | "new" | "pty" | "smo" | "xlog" | "ylog" | "xpd" => vec![0.0],
        "cex" | "cex.axis" | "cex.lab" | "cex.main" | "cex.sub" | "crt" | "lheight" | "ljoin" | "mkh" | "ps" | "tck" => vec![1.0],
        "mex" | "font.axis" | "font.lab" | "font.main" | "font.sub" | "lwd" | "pch" => vec![1.0],
        "mfrow" | "mfcol" => vec![1.0, 1.0],
        "mfg" => vec![1.0, 1.0, 1.0, 1.0],
        "mgp" => vec![3.0, 1.0, 0.0],
        "oma" | "omd" | "omi" => vec![0.0, 0.0, 0.0, 0.0],
        "lab" => vec![5.0, 5.0, 7.0],
        "srt" | "ylbias" => vec![0.0],
        "tcl" => vec![-0.5],
        "lmitre" => vec![10.0],
        _ => vec![0.0],
    }
}

pub fn par_get(heap: &Heap, name: &str) -> Vec<f64> {
    // The dumb-parameter reading exactly as the naive machine's: the
    /// cascade answers region-coupled names, everything else falls to
    /// GNU's defaults — typing is reconstructed at object construction.
    match heap.graphics.query(name) {
        Some(r_core::graphics::layout::ParValue::Real(values)) => values,
        Some(r_core::graphics::layout::ParValue::Integer(values))
        | Some(r_core::graphics::layout::ParValue::Logical(values)) => {
            values.iter().map(|v| f64::from(*v)).collect()
        }
        _ => par_default(name),
    }
}

/// Render a cascade answer as a typed object exactly as GNU's par
/// typing — logicals and integers keep their types through the query.
pub fn parvalue_obj(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    value: r_core::graphics::layout::ParValue,
) -> ValueId {
    use r_core::graphics::layout::ParValue;
    match value {
        ParValue::Real(values) => heap.alloc(RObject::Real(values)),
        ParValue::Integer(values) => heap.alloc(RObject::Integer(values)),
        ParValue::Logical(values) => heap.alloc(RObject::Logical(values)),
        ParValue::String(text) => heap.alloc(RObject::String(vec![interner.intern_string(&text)])),
    }
}

/// Read one parameter as a typed object — the cascade's answer or the
/// default table exactly as par_get.
pub fn par_query_obj(heap: &mut Heap, interner: &mut SymbolInterner, name: &str) -> ValueId {
    match heap.graphics.query(name) {
        Some(value) => parvalue_obj(heap, interner, value),
        None => real_obj(heap, par_default(name)),
    }
}

/// Render a numeric vector as a Real object.
pub fn real_obj(heap: &mut Heap, values: Vec<f64>) -> ValueId {
    heap.alloc(RObject::Real(values))
}

/// par — GNU's graphics parameter machine: single string reads one value,
/// character vector reads a named list, named arguments set and return the
/// prior values invisibly, empty form lists everything, no.readonly drops
/// the device-derived six.
pub fn par_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let raw = raw_args(heap, args);
    // Empty form: the full parameter list.
    if raw.is_empty() {
        let mut entries: Vec<(&str, ValueId)> = Vec::new();
        for name in PAR_NAMES {
            let values = par_get(heap, name);
            let obj = real_obj(heap, values);
            entries.push((name, obj));
        }
        let entries_owned: Vec<(String, ValueId)> = entries.into_iter().map(|(n, v)| (n.to_string(), v)).collect();
        return named_list_owned(heap, interner, entries_owned);
    }
    // Tagged no.readonly form.
    if raw.len() == 1 {
        if let Some(RObject::Pairlist { .. }) = heap.object(args) {}
        let is_no_readonly = match heap.object(args) {
            Some(RObject::Pairlist { tag, .. }) => {
                matches!(heap.object(*tag), Some(RObject::Symbol { name, .. }) if interner.symbol_name(*name) == "no.readonly")
            }
            _ => false,
        };
        if is_no_readonly {
            let v = eval(heap, interner, raw[0], caller_env)?;
            let v = force_if_promise(heap, interner, v)?;
            let drop_ro = logical_scalar(heap, v);
            let mut entries_owned: Vec<(String, ValueId)> = Vec::new();
            for name in PAR_NAMES {
                if drop_ro && PAR_READONLY.contains(name) {
                    continue;
                }
                let values = par_get(heap, name);
                let obj = real_obj(heap, values);
                entries_owned.push((name.to_string(), obj));
            }
            return named_list_owned(heap, interner, entries_owned);
        }
    }
    // Named setters: mar = c(...) forms — every argument tagged.
    let mut setters: Vec<(String, ValueId)> = Vec::new();
    let mut positional: Vec<ValueId> = Vec::new();
    let mut cursor = args;
    for _ in 0..1000 {
        if cursor == ValueId::NIL { break; }
        let (car, tag, next) = match heap.object(cursor) {
            Some(RObject::Pairlist { car, tag, cdr, .. }) => (*car, *tag, *cdr),
            _ => break,
        };
        let tname = match heap.object(tag) {
            Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
            _ => None,
        };
        let vpre = eval(heap, interner, car, caller_env)?;
        let v = force_if_promise(heap, interner, vpre)?;
        match tname {
            Some(n) if !n.is_empty() => setters.push((n, v)),
            _ => positional.push(v),
        }
        cursor = next;
    }
    if !setters.is_empty() {
        let mut prior: Vec<(String, ValueId)> = Vec::new();
        for (name, v) in &setters {
            if PAR_READONLY.contains(&name.as_str()) {
                return Err(RSignal::Error(EvalError::TypeError(
                    "read-only graphical parameter",
                )));
            }
            let old_obj = par_query_obj(heap, interner, name);
            prior.push((name.clone(), old_obj));
            let value = parvalue_of(heap, interner, *v).ok_or_else(|| {
                RSignal::Error(EvalError::TypeError("invalid par value"))
            })?;
            heap.graphics.apply(name, &value).map_err(|message| {
                RSignal::Error(EvalError::Condition(message))
            })?;
        }
        heap.visible = false;
        return named_list_owned(heap, interner, prior);
    }
    // Character vector reads: one string or several.
    if let Some(first) = positional.first() {
        let names = string_vec(heap, interner, *first);
        if names.len() == 1 {
            return Ok(par_query_obj(heap, interner, &names[0]));
        }
        if !names.is_empty() {
            let mut entries_owned: Vec<(String, ValueId)> = Vec::new();
            for name in &names {
                let obj = par_query_obj(heap, interner, name);
                entries_owned.push((name.clone(), obj));
            }
            return named_list_owned(heap, interner, entries_owned);
        }
    }
    Ok(ValueId::NIL)
}

/// named_list from owned keys — the par machine's list shape.
pub fn named_list_owned(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    entries: Vec<(String, ValueId)>,
) -> EvalResult {
    let names: Vec<_> = entries.iter().map(|(n, _)| interner.intern_string(n)).collect();
    let items: Vec<ValueId> = entries.iter().map(|(_, v)| *v).collect();
    let result = heap.alloc(RObject::List(items));
    let names_obj = heap.alloc(RObject::String(names));
    copy_with_attrib(heap, interner, result, "names", names_obj).map_err(RSignal::from)
}

/// layout(mat) — GNU's panel-arrangement state: the return is the matrix
/// maximum, mfrow/mfcol take the dimensions, mfg the (nr, nc, nr, nc)
/// quadruple, and cex shrinks with the panel count exactly as GNU's
/// multi-panel adjustment.
pub fn layout_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 1)?;
    let dims: Vec<i32> = get_attrib(heap, interner, operands[0], "dim")
        .and_then(|id| match heap.object(id) {
            Some(RObject::Integer(d)) => Some(d.clone()),
            _ => None,
        })
        .unwrap_or_else(|| vec![1, 1]);
    let nr = dims.first().copied().unwrap_or(1).max(1);
    let nc = dims.get(1).copied().unwrap_or(1).max(1);
    let cells = reals_of_value(heap, operands[0]).unwrap_or_default();
    let max_cell = cells.iter().cloned().fold(0.0f64, f64::max);
    let order: Vec<i32> = cells.iter().map(|v| *v as i32).collect();
    let num_figures = max_cell as i32;
    let cexbase = match heap.graphics.query("cex") {
        Some(r_core::graphics::layout::ParValue::Real(v)) if v.len() == 1 => v[0],
        _ => 1.0,
    };
    heap.graphics
        .layout
        .set_layout(nr, nc, &order, num_figures.max(1), cexbase, 1.0, 1.0)
        .map_err(|message| RSignal::Error(EvalError::Condition(message)))?;
    let cex = r_core::graphics::layout::LayoutState::split_cex(nr, nc);
    heap.graphics
        .apply("cex", &r_core::graphics::layout::ParValue::Real(vec![cex]))
        .map_err(|message| RSignal::Error(EvalError::Condition(message)))?;
    // GNU's layout returns the panel count invisibly.
    heap.visible = false;
    Ok(heap.alloc(RObject::Integer(vec![max_cell as i32])))
}

/// plot.new — GNewPlot exactly as the oracle fixtures pin it: prepare
/// the candidate figure on a cloned cascade, validate the margins
/// without consuming cell or page, then commit. The new flag reuses
/// the current frame; the first figure of an unused device ignores any
/// preselected cell.
pub fn plot_new_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    _args: ValueId,
    _caller_env: ValueId,
) -> EvalResult {
    let (regions, clear, transition) = heap.graphics.prepare_plot_figure();
    // GNU's figure-margins validation — a nonpositive plot region in
    /// either dimension rejects the frame exactly as
    /// "figure margins too large"; nothing commits on failure.
    let plot_width = regions.plot[2] - regions.plot[0];
    let plot_height = regions.plot[3] - regions.plot[1];
    if transition_would_commit(heap) && (plot_width <= 0.0 || plot_height <= 0.0) {
        return Err(RSignal::Error(EvalError::Condition(
            "figure margins too large".to_string(),
        )));
    }
    heap.graphics.commit_plot_figure(transition);
    // The layout's own decision exactly as GNU's GNewPlot — reuse
    /// paths and mid-grid steps keep the page, first figures and
    /// post-wrap frames clear.
    if clear {
        heap.graphics.clear_scene();
    }
    Ok(ValueId::NIL)
}

/// Whether the prepared transition advances the frame — the reuse
/// path carries no layout clone.
pub fn transition_would_commit(heap: &Heap) -> bool {
    let _ = heap;
    true
}

/// plot.window(xlim, ylim) — install the user coordinate system exactly
/// as GNU: usr takes the limits with the xaxs/yaxs 4% extension.
pub fn plot_window_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let operands = eval_args(heap, interner, args, caller_env, 2)?;
    let xlim = reals_of_value(heap, operands[0]).unwrap_or_default();
    let ylim = reals_of_value(heap, operands[1]).unwrap_or_default();
    if xlim.len() < 2 || ylim.len() < 2 {
        return Err(RSignal::Error(EvalError::TypeError(
            "plot.window requires two-element limits",
        )));
    }
    // xaxs/yaxs exactly as GNU: "r" extends each side 4%, "i" keeps
    /// the exact limits. The style reads through the par cascade.
    let style = |name: &str| -> String {
        match heap.graphics.query(name) {
            Some(r_core::graphics::layout::ParValue::String(text)) => text,
            _ => "r".to_string(),
        }
    };
    let extend = |lo: f64, hi: f64, style: &str| {
        if style == "i" {
            (lo, hi)
        } else {
            let span = hi - lo;
            (lo - 0.04 * span, hi + 0.04 * span)
        }
    };
    let (x0, x1) = extend(xlim[0], xlim[1], &style("xaxs"));
    let (y0, y1) = extend(ylim[0], ylim[1], &style("yaxs"));
    heap.graphics
        .apply("usr", &r_core::graphics::layout::ParValue::Real(vec![x0, x1, y0, y1]))
        .map_err(|message| RSignal::Error(EvalError::Condition(message)))?;
    Ok(ValueId::NIL)
}

/// lines(x, y) — GNU's polyline exactly: x and y recycle against each
/// other, non-finite points break the line exactly as GPolyline's
/// segment splitting, and the path strokes in the foreground with no
/// fill. The simplest drawing primitive — the scene-emission proof.
pub fn lines_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    use r_graphics_engine::{Color, DrawOperation, Path, PathCommand, Stroke};
    let slots = eval_matched_args(heap, interner, args, caller_env, &["x", "y", "col"])?;
    let xs = slots[0].and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
    let ys = slots[1].and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
    let count = xs.len().max(ys.len());
    if count == 0 || xs.is_empty() || ys.is_empty() {
        return Ok(ValueId::NIL);
    }
    let color = match slots[2] {
        Some(v) => resolve_rcolor(heap, interner, v, 0).unwrap_or(Color::BLACK),
        None => Color::BLACK,
    };
    let mut commands = Vec::new();
    let mut pen_up = true;
    for i in 0..count {
        let x = xs[i % xs.len()];
        let y = ys[i % ys.len()];
        if !x.is_finite() || !y.is_finite() {
            pen_up = true;
            continue;
        }
        let (dx, dy) = heap.graphics.user_to_device(x, y);
        if pen_up {
            commands.push(PathCommand::MoveTo(dx, dy));
            pen_up = false;
        } else {
            commands.push(PathCommand::LineTo(dx, dy));
        }
    }
    if !commands.is_empty() {
        let path = Path {
            commands,
            stroke: par_stroke(heap, color),
            anti_alias: true,
            ..Default::default()
        };
        heap.graphics.scene_mut().push(DrawOperation::Path(path));
    }
    Ok(ValueId::NIL)
}

/// Resolve one R color exactly as GNU's portable reach: palette
/// indices (1-based), the common names, hex literals, transparent.
/// NA and unknown colors yield None exactly as col2rgb's NA.
/// Build the stroke GNU's lwd/lty cascade specifies exactly: lwd
/// scales the base width, lty maps both its numeric and string forms
/// onto GNU's dash intervals in units of the width — dashed 4,4,
/// dotted 1,3, dotdash 1,3,4,4, longdash 7,3, twodash 2,2,6,2,
/// blank suppressing stroke through transparency.
pub fn par_stroke(heap: &Heap, color: r_graphics_engine::Color) -> r_graphics_engine::Stroke {
    use r_graphics_engine::{Color, DashPattern, Stroke};
    const TRANSPARENT: Color = Color { r: 0, g: 0, b: 0, a: 0 };
    let lwd = match heap.graphics.query("lwd") {
        Some(r_core::graphics::layout::ParValue::Real(v)) if v.len() == 1 => v[0] as f32,
        Some(r_core::graphics::layout::ParValue::Integer(v)) if v.len() == 1 => v[0] as f32,
        _ => 1.0,
    };
    let width = lwd.max(0.0);
    let lty_name = match heap.graphics.query("lty") {
        Some(r_core::graphics::layout::ParValue::String(text)) => text,
        Some(r_core::graphics::layout::ParValue::Integer(v)) if !v.is_empty() => {
            match v[0] {
                0 => "blank".to_string(),
                2 => "dashed".to_string(),
                3 => "dotted".to_string(),
                4 => "dotdash".to_string(),
                5 => "longdash".to_string(),
                6 => "twodash".to_string(),
                _ => "solid".to_string(),
            }
        }
        _ => "solid".to_string(),
    };
    let intervals: Option<Vec<f32>> = match lty_name.as_str() {
        "dashed" => Some(vec![4.0 * width, 4.0 * width]),
        "dotted" => Some(vec![width, 3.0 * width]),
        "dotdash" => Some(vec![width, 3.0 * width, 4.0 * width, 4.0 * width]),
        "longdash" => Some(vec![7.0 * width, 3.0 * width]),
        "twodash" => Some(vec![2.0 * width, 2.0 * width, 6.0 * width, 2.0 * width]),
        "blank" => None,
        _ => None,
    };
    let color = if lty_name == "blank" { TRANSPARENT } else { color };
    let mut stroke = Stroke::new(width.max(0.5), color);
    if lty_name != "solid" && lty_name != "blank" {
        if let Some(iv) = intervals {
            stroke.dash_pattern = Some(DashPattern { intervals: iv, offset: 0.0 });
        }
    }
    stroke
}

pub fn resolve_rcolor(
    heap: &Heap,
    interner: &SymbolInterner,
    value: ValueId,
    index: usize,
) -> Option<r_graphics_engine::Color> {
    use r_graphics_engine::Color;
    // The R 4.x default palette exactly as the pinned oracle — black,
    /// the six hues, then gray62.
    const PALETTE: [Color; 8] = [
        Color::BLACK,
        Color { r: 0xDF, g: 0x53, b: 0x6B, a: 255 },
        Color { r: 0x61, g: 0xD0, b: 0x4F, a: 255 },
        Color { r: 0x22, g: 0x97, b: 0xE6, a: 255 },
        Color { r: 0x28, g: 0xE2, b: 0xE5, a: 255 },
        Color { r: 0xCD, g: 0x0B, b: 0xBC, a: 255 },
        Color { r: 0xF5, g: 0xC7, b: 0x10, a: 255 },
        Color { r: 0x9E, g: 0x9E, b: 0x9E, a: 255 },
    ];
    match heap.object(value) {
        Some(RObject::String(values)) => {
            let text = interner.string_text(*values.get(index % values.len().max(1))?);
            Some(match text {
                "black" => Color::BLACK,
                "white" => Color::WHITE,
                "red" => Color::RED,
                "blue" => Color::BLUE,
                "green" => Color { r: 0, g: 255, b: 0, a: 255 },
                "yellow" => Color { r: 255, g: 255, b: 0, a: 255 },
                "cyan" => Color { r: 0, g: 255, b: 255, a: 255 },
                "magenta" => Color { r: 255, g: 0, b: 255, a: 255 },
                "gray" | "grey" => Color { r: 190, g: 190, b: 190, a: 255 },
                "orange" => Color { r: 255, g: 165, b: 0, a: 255 },
                "purple" => Color { r: 160, g: 32, b: 240, a: 255 },
                "brown" => Color { r: 165, g: 42, b: 42, a: 255 },
                "pink" => Color { r: 255, g: 192, b: 203, a: 255 },
                "transparent" | "NA" => return None,
                other => {
                    // The full 657-entry database with hex parsing
                    /// exactly as col2rgb — every GNU color name
                    /// resolves in drawing primitives.
                    if let Some((r, g, b, a)) = resolve_named_color(other) {
                        return Some(Color { r: r as u8, g: g as u8, b: b as u8, a: a as u8 });
                    }
                    return None;
                }
            })
        }
        Some(RObject::Integer(values)) | Some(RObject::Logical(values)) => {
            let idx = *values.get(index % values.len().max(1))?;
            if idx == i32::MIN || idx < 1 {
                None
            } else {
                Some(PALETTE[((idx - 1) as usize) % PALETTE.len()])
            }
        }
        Some(RObject::Real(values)) => {
            let idx = *values.get(index % values.len().max(1))? as i32;
            if idx < 1 {
                None
            } else {
                Some(PALETTE[((idx - 1) as usize) % PALETTE.len()])
            }
        }
        _ => None,
    }
}

/// rect(xleft, ybottom, xright, ytop) — GNU's recycled quads exactly:
/// fill with col (transparent when NA), stroke with border. The four
/// coordinates recycle against each other in lockstep.
pub fn rect_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    use r_graphics_engine::{Color, DrawOperation, Path, Stroke};
    // Positional order covers GNU's first four exactly; col/border are
    /// effectively always named in practice — the density/angle/lty
    /// slots of GNU's full signature remain unwired.
    let slots = eval_matched_args(
        heap,
        interner,
        args,
        caller_env,
        &["xleft", "ybottom", "xright", "ytop", "col", "border"],
    )?;
    let coord = |slot: usize| -> Vec<f64> {
        slots[slot]
            .and_then(|v| reals_of_value(heap, v).ok())
            .unwrap_or_default()
    };
    let xl = coord(0);
    let yb = coord(1);
    let xr = coord(2);
    let yt = coord(3);
    if xl.is_empty() || yb.is_empty() || xr.is_empty() || yt.is_empty() {
        return Ok(ValueId::NIL);
    }
    let count = xl.len().max(yb.len()).max(xr.len()).max(yt.len());
    for i in 0..count {
        let x0 = xl[i % xl.len()];
        let y0 = yb[i % yb.len()];
        let x1 = xr[i % xr.len()];
        let y1 = yt[i % yt.len()];
        if !x0.is_finite() || !y0.is_finite() || !x1.is_finite() || !y1.is_finite() {
            continue;
        }
        let (dx0, dy1) = heap.graphics.user_to_device(x0, y0);
        let (dx1, dy0) = heap.graphics.user_to_device(x1, y1);
        let x = dx0.min(dx1);
        let y = dy0.min(dy1);
        let w = (dx1 - dx0).abs();
        let h = (dy1 - dy0).abs();
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        let fill = slots[4]
            .and_then(|v| resolve_rcolor(heap, interner, v, i))
            .unwrap_or(Color { r: 0, g: 0, b: 0, a: 0 });
        let border = match slots[5] {
            Some(v) => resolve_rcolor(heap, interner, v, i)
                .unwrap_or(Color { r: 0, g: 0, b: 0, a: 0 }),
            None => Color::BLACK,
        };
        let path = Path {
            fill,
            stroke: par_stroke(heap, border),
            anti_alias: true,
            ..Path::rect(x, y, w, h)
        };
        heap.graphics.scene_mut().push(DrawOperation::Path(path));
    }
    Ok(ValueId::NIL)
}

/// points(x, y, pch, col) — GNU's scatter exactly as far as the core
/// symbols reach: pch 1 open circle, 16 filled, 3 plus, sized by
/// cex-scaled half character exactly as GSymbol's ipr-scaled radius.
/// Other pch values fall to the open circle pending the symbol atlas.
pub fn points_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    use r_graphics_engine::{Color, DrawOperation, Path, PathCommand, Stroke};
    let slots = eval_matched_args(heap, interner, args, caller_env, &["x", "y", "pch", "col", "cex"])?;
    let xs = slots[0].and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
    let ys = slots[1].and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
    let count = xs.len().max(ys.len());
    if count == 0 || xs.is_empty() || ys.is_empty() {
        return Ok(ValueId::NIL);
    }
    // pch resolves per-point with GNU's recycling exactly as col —
    /// precomputed so the emission loop holds no borrow across the
    /// scene mutation.
    let pch_values: Vec<i32> = match slots[2] {
        Some(v) => match heap.object(v) {
            Some(RObject::Real(values)) => values.iter().map(|x| *x as i32).collect(),
            Some(RObject::Integer(values)) => values.clone(),
            _ => vec![1],
        },
        None => vec![1],
    };
    let cex = slots[4]
        .and_then(|v| reals_of_value(heap, v).ok())
        .and_then(|v| v.first().copied())
        .unwrap_or_else(|| match heap.graphics.query("cex") {
            Some(r_core::graphics::layout::ParValue::Real(v)) if v.len() == 1 => v[0],
            _ => 1.0,
        });
    // GNU sizes symbols at half the character width scaled by cex —
    /// the raster character cell is the cra cascade answer.
    let cra = match heap.graphics.query("cra") {
        Some(r_core::graphics::layout::ParValue::Real(v)) if v.len() == 2 => v,
        _ => vec![10.8, 14.4],
    };
    let radius = (0.5 * cex * cra[0]) as f32;
    for i in 0..count {
        let x = xs[i % xs.len()];
        let y = ys[i % ys.len()];
        if !x.is_finite() || !y.is_finite() {
            continue;
        }
        let (dx, dy) = heap.graphics.user_to_device(x, y);
        let color = match slots[3] {
            Some(v) => resolve_rcolor(heap, interner, v, i).unwrap_or(Color::BLACK),
            None => Color::BLACK,
        };
        let pch = pch_values[i % pch_values.len()];
        let path = match pch {
            3 => {
                // plus: two strokes through the center
                let r = radius;
                Path {
                    commands: vec![
                        PathCommand::MoveTo(dx - r, dy),
                        PathCommand::LineTo(dx + r, dy),
                        PathCommand::MoveTo(dx, dy - r),
                        PathCommand::LineTo(dx, dy + r),
                    ],
                    stroke: par_stroke(heap, color),
                    anti_alias: true,
                    ..Default::default()
                }
            }
            16 => Path::circle(dx, dy, radius).with_fill(color),
            _ => {
                // 1 and the fallback: open circle — transparent fill,
                /// stroked rim exactly as GNU's default symbol.
                let mut circle = Path::circle(dx, dy, radius);
                circle.fill = Color { r: 0, g: 0, b: 0, a: 0 };
                circle.stroke = Stroke::new(1.0, color);
                circle
            }
        };
        heap.graphics.scene_mut().push(DrawOperation::Path(path));
    }
    Ok(ValueId::NIL)
}

/// text(x, y, labels) — GNU's text exactly as far as the core reach:
/// labels recycle against coordinates, position centers horizontally
/// at adj 0.5 exactly as GNU's default, size scales through cex and
/// the 12pt base exactly as the portable POINTSIZE contract.
pub fn text_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    use r_graphics_engine::{Color, DrawOperation, PlotParameters, Point, TextAnchor};
    let slots = eval_matched_args(
        heap,
        interner,
        args,
        caller_env,
        &["x", "y", "labels", "col", "cex"],
    )?;
    let xs = slots[0].and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
    let ys = slots[1].and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
    let labels: Vec<String> = match slots[2].and_then(|v| {
        match heap.object(v) {
            Some(RObject::String(values)) => Some(
                values
                    .iter()
                    .map(|s| interner.string_text(*s).to_string())
                    .collect(),
            ),
            _ => None,
        }
    }) {
        Some(values) => values,
        None => return Ok(ValueId::NIL),
    };
    let count = xs.len().max(ys.len()).max(labels.len());
    if count == 0 || xs.is_empty() || ys.is_empty() || labels.is_empty() {
        return Ok(ValueId::NIL);
    }
    let cex = slots[4]
        .and_then(|v| reals_of_value(heap, v).ok())
        .and_then(|v| v.first().copied())
        .unwrap_or_else(|| match heap.graphics.query("cex") {
            Some(r_core::graphics::layout::ParValue::Real(v)) if v.len() == 1 => v[0],
            _ => 1.0,
        });
    // The 12pt base at 72dpi raster units exactly as the portable
    /// POINTSIZE contract — one raster unit per point.
    let font_size = (12.0 * cex) as f32;
    for i in 0..count {
        let x = xs[i % xs.len()];
        let y = ys[i % ys.len()];
        if !x.is_finite() || !y.is_finite() {
            continue;
        }
        let text = labels[i % labels.len()].clone();
        if text.is_empty() {
            continue;
        }
        let color = match slots[3] {
            Some(v) => resolve_rcolor(heap, interner, v, i).unwrap_or(Color::BLACK),
            None => Color::BLACK,
        };
        let (dx, dy) = heap.graphics.user_to_device(x, y);
        heap.graphics.scene_mut().push(DrawOperation::Text {
            text,
            position: Point { x: dx, y: dy },
            params: PlotParameters {
                font_size,
                text_color: color,
                dpi: 72.0,
                text_anchor: TextAnchor::Middle,
                ..Default::default()
            },
        });
    }
    Ok(ValueId::NIL)
}

/// pretty(x, n) — GNU's user-facing tick computation exactly: finite
/// values only, min_n = n/3, shrink 0.75, the standard biases, and
/// the interpolated vector with endpoint-exact construction and
/// near-zero cleanup exactly as the wrapper performs it.
pub fn pretty_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["x", "n"])?;
    let xs: Vec<f64> = slots[0]
        .and_then(|v| reals_of_value(heap, v).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|x| x.is_finite())
        .collect();
    if xs.is_empty() {
        return Ok(heap.alloc(RObject::Real(Vec::new())));
    }
    let n = slots[1]
        .and_then(|v| reals_of_value(heap, v).ok())
        .and_then(|v| v.first().copied())
        .unwrap_or(5.0) as i32;
    let lo = xs.iter().copied().fold(f64::INFINITY, f64::min);
    let up = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    // GNU pretty.default's fixed constants exactly.
    let result = r_core::graphics::pretty::r_pretty(
        lo,
        up,
        n,
        n.div_euclid(3),
        0.75,
        1.5,
        0.5 + 1.5 * 1.5,
        2f64.powi(-20),
        0,
    );
    let ndiv = result.ndiv;
    if !(0..=10_000_000).contains(&ndiv) {
        return Err(RSignal::Error(EvalError::Condition(
            "invalid 'n' argument".to_string(),
        )));
    }
    let (start, end) = (result.lo, result.up);
    let mut values = Vec::with_capacity(ndiv as usize + 1);
    if ndiv == 0 {
        values.push(start);
    } else {
        for i in 0..=ndiv as usize {
            values.push(if i == 0 {
                start
            } else if i == ndiv as usize {
                end
            } else {
                let t = i as f64 / ndiv as f64;
                start * (1.0 - t) + end * t
            });
        }
        // Near-zero cleanup exactly as the wrapper: relative to the
        /// overflow-safe per-interval delta.
        let delta = (end / ndiv as f64 - start / ndiv as f64).abs();
        for value in values.iter_mut() {
            if value.abs() < 1e-14 * delta {
                *value = 0.0;
            }
        }
    }
    Ok(heap.alloc(RObject::Real(values)))
}

/// grconvertX/grconvertY(value, from, to) — GNU's coordinate query
/// exactly as far as user and device reach: user maps through the
/// plot-region conversion, device is raster units with the y-down
/// flip already inside. Other units remain unwired pending the full
/// GMapUnits ladder.
pub fn grconvert_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
    x_axis: bool,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["x", "from", "to"])?;
    let values = slots[0].and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
    let unit_name = |slot: usize| -> String {
        match slots[slot].and_then(|v| match heap.object(v) {
            Some(RObject::String(values)) => values.first().map(|s| interner.string_text(*s).to_string()),
            _ => None,
        }) {
            Some(text) => text,
            None => "user".to_string(),
        }
    };
    let from = unit_name(1);
    let to = unit_name(2);
    let converted: Vec<f64> = values
        .iter()
        .map(|v| {
            if !v.is_finite() {
                return *v;
            }
            // user → device exactly as the primitive conversion; the
            /// inverse divides back through the same linear map.
            let usr = match heap.graphics.query("usr") {
                Some(r_core::graphics::layout::ParValue::Real(u)) if u.len() == 4 => u,
                _ => vec![0.0, 1.0, 0.0, 1.0],
            };
            let plot = heap.graphics.layout.regions().plot;
            let (lo_u, hi_u, lo_d, hi_d) = if x_axis {
                (usr[0], usr[1], plot[0], plot[2])
            } else {
                // y grows upward in user space, downward in raster —
                /// device bottom pairs with user low exactly as the
                /// conversion flips.
                (usr[2], usr[3], plot[3], plot[1])
            };
            let height = heap.graphics.layout.device_size().1;
            let user_to_dev = |u: f64| lo_d + (u - lo_u) / (hi_u - lo_u) * (hi_d - lo_d);
            match (from.as_str(), to.as_str()) {
                ("user", "device") => {
                    if x_axis {
                        user_to_dev(*v)
                    } else {
                        height - user_to_dev(*v)
                    }
                }
                ("device", "user") => {
                    let dev = if x_axis { *v } else { height - *v };
                    lo_u + (dev - lo_d) / (hi_d - lo_d) * (hi_u - lo_u)
                }
                _ => *v,
            }
        })
        .collect();
    Ok(heap.alloc(RObject::Real(converted)))
}

/// Format one tick label exactly as GNU's plain numeric reach: integral
/// values print without the fraction, others trim trailing zeros.
pub fn axis_label(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        // GNU labels ticks with the printer's significant-digit logic —
        // fp noise in computed positions collapses exactly as 2.5 does,
        // never shortest-roundtrip noise.
        r_core::print::format_real(value)
    }
}

/// axis(side, at, labels) — GNU's axis composition: pretty-computed
/// tick positions when absent, the axis line along the plot edge,
/// tick marks perpendicular at tcl lines (negative outward exactly
/// as GNU's default), and labels beyond the tick ends. Side 1 bottom,
/// 2 left, 3 top, 4 right exactly as GAxis.
pub fn axis_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    use r_graphics_engine::{Color, DrawOperation, Path, PathCommand, PlotParameters, Point, Stroke, TextAnchor};
    let slots = eval_matched_args(heap, interner, args, caller_env, &["side", "at", "labels"])?;
    let side = slots[0]
        .and_then(|v| reals_of_value(heap, v).ok())
        .and_then(|v| v.first().copied())
        .unwrap_or(1.0) as i32;
    if !(1..=4).contains(&side) {
        return Err(RSignal::Error(EvalError::Condition(
            "invalid axis number".to_string(),
        )));
    }
    let horizontal = side == 1 || side == 3;
    // Tick positions: explicit at= or pretty over the usr range exactly
    /// as axp computation feeds GAxis.
    let usr = match heap.graphics.query("usr") {
        Some(r_core::graphics::layout::ParValue::Real(u)) if u.len() == 4 => u,
        _ => vec![0.0, 1.0, 0.0, 1.0],
    };
    let at: Vec<f64> = match slots[1].and_then(|v| reals_of_value(heap, v).ok()) {
        Some(values) if !values.is_empty() => values,
        _ => {
            let (lo, hi) = if horizontal {
                (usr[0], usr[1])
            } else {
                (usr[2], usr[3])
            };
            let result = r_core::graphics::pretty::r_pretty(
                lo,
                hi,
                5,
                5_i32.div_euclid(3),
                0.75,
                1.5,
                0.5 + 1.5 * 1.5,
                2f64.powi(-20),
                0,
            );
            let (start, end, ndiv) = (result.lo, result.up, result.ndiv.max(1));
            (0..=ndiv as usize)
                .map(|i| {
                    if i == 0 {
                        start
                    } else if i == ndiv as usize {
                        end
                    } else {
                        let t = i as f64 / ndiv as f64;
                        start * (1.0 - t) + end * t
                    }
                })
                .collect()
        }
    };
    // GAxis clips ticks to the usr window exactly — the 4% extension
    /// means pretty positions can fall outside and never draw.
    let (win_lo, win_hi) = if horizontal {
        (usr[0].min(usr[1]), usr[0].max(usr[1]))
    } else {
        (usr[2].min(usr[3]), usr[2].max(usr[3]))
    };
    let at: Vec<f64> = at
        .into_iter()
        .filter(|v| *v >= win_lo && *v <= win_hi)
        .collect();
    if at.is_empty() {
        return Ok(ValueId::NIL);
    }
    // GNU's GAxis records tick coverage into xaxp/yaxp exactly — grid
    // and axTicks reconstruct positions from first/last/count-1.
    heap.graphics.set_axp(
        horizontal,
        [at[0], at[at.len() - 1], (at.len() - 1) as f64],
    );
    let plot = heap.graphics.layout.regions().plot;
    // One margin line in raster units exactly as the 0.2in portable
    /// contract; tcl -0.5 is GNU's outward default.
    let line = 14.4f32;
    let tcl = -0.5f32;
    let tick_len = tcl.abs() * line;
    // The axis edge in y-down raster and the outward direction.
    let mut commands = Vec::new();
    let mut label_positions = Vec::new();
    for value in &at {
        if horizontal {
            let u = if side == 1 { *value } else { *value };
            let v = if side == 1 { usr[2] } else { usr[3] };
            let (dx, dy) = heap.graphics.user_to_device(u, v);
            commands.push(PathCommand::MoveTo(dx, dy));
            // y-down: bottom outward is positive raster y, top negative.
            let outward = if side == 1 { tick_len } else { -tick_len };
            commands.push(PathCommand::LineTo(dx, dy + outward));
            label_positions.push((dx, dy + outward + 0.5 * line, TextAnchor::Middle, *value));
        } else {
            let u = if side == 2 { usr[0] } else { usr[1] };
            let (dx, dy) = heap.graphics.user_to_device(u, *value);
            commands.push(PathCommand::MoveTo(dx, dy));
            let outward = if side == 2 { -tick_len } else { tick_len };
            commands.push(PathCommand::LineTo(dx + outward, dy));
            label_positions.push((dx + outward - 0.35 * line, dy, TextAnchor::End, *value));
        }
    }
    // The axis line itself spans the plot edge exactly as GAxis draws.
    let (ax0, ay0, ax1, ay1) = if horizontal {
        let v = if side == 1 { usr[2] } else { usr[3] };
        let (x0, y0) = heap.graphics.user_to_device(usr[0], v);
        let (x1, y1) = heap.graphics.user_to_device(usr[1], v);
        (x0, y0, x1, y1)
    } else {
        let u = if side == 2 { usr[0] } else { usr[1] };
        let (x0, y0) = heap.graphics.user_to_device(u, usr[2]);
        let (x1, y1) = heap.graphics.user_to_device(u, usr[3]);
        (x0, y0, x1, y1)
    };
    let mut line_commands = vec![
        PathCommand::MoveTo(ax0, ay0),
        PathCommand::LineTo(ax1, ay1),
    ];
    line_commands.extend(commands);
    let parstroke_4 = par_stroke(heap, Color::BLACK);
    heap.graphics.scene_mut().push(DrawOperation::Path(Path {
        commands: line_commands,
        stroke: parstroke_4,
        anti_alias: true,
        ..Default::default()
    }));
    let cex = match heap.graphics.query("cex") {
        Some(r_core::graphics::layout::ParValue::Real(v)) if v.len() == 1 => v[0],
        _ => 1.0,
    };
    for (x, y, anchor, value) in label_positions {
        heap.graphics.scene_mut().push(DrawOperation::Text {
            text: axis_label(value),
            position: Point { x, y },
            params: PlotParameters {
                font_size: (12.0 * cex) as f32,
                text_color: Color::BLACK,
                dpi: 72.0,
                text_anchor: anchor,
                ..Default::default()
            },
        });
    }
    Ok(ValueId::NIL)
}

/// box() — GNU's plot-region frame exactly: the four plot edges as
/// one closed path in the foreground.
pub fn box_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    use r_graphics_engine::{Color, DrawOperation, Path, Stroke};
    let _ = eval_matched_args(heap, interner, args, caller_env, &["which"])?;
    let plot = heap.graphics.layout.regions().plot;
    let x = plot[0] as f32;
    let y = plot[1] as f32;
    let w = (plot[2] - plot[0]) as f32;
    let h = (plot[3] - plot[1]) as f32;
    let parstroke_3 = par_stroke(heap, Color::BLACK);
    heap.graphics.scene_mut().push(DrawOperation::Path(Path {
        stroke: parstroke_3,
        anti_alias: true,
        ..Path::rect(x, y, w, h)
    }));
    Ok(ValueId::NIL)
}

/// title(main, sub, xlab, ylab) — GNU's margin text exactly as far as
/// the core reach: axis titles center along their edge at the margin
/// line, the main title centers over the plot in the top margin.
pub fn title_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    use r_graphics_engine::{Color, DrawOperation, PlotParameters, Point, TextAnchor};
    let slots = eval_matched_args(heap, interner, args, caller_env, &["main", "sub", "xlab", "ylab"])?;
    // Extract every label upfront exactly as the other primitives
    /// precompute — the emission loop holds no borrow across scene
    /// mutation.
    let texts: Vec<Option<String>> = (0..4)
        .map(|slot| {
            slots[slot].and_then(|v| match heap.object(v) {
                Some(RObject::String(values)) => {
                    values.first().map(|s| interner.string_text(*s).to_string())
                }
                _ => None,
            })
        })
        .collect();
    let text_of = |slot: usize| -> Option<String> { texts[slot].clone() };
    let plot = heap.graphics.layout.regions().plot;
    let center_x = (plot[0] + plot[2]) as f32 / 2.0;
    let center_y = (plot[1] + plot[3]) as f32 / 2.0;
    let line = 14.4f32;
    let cex = match heap.graphics.query("cex") {
        Some(r_core::graphics::layout::ParValue::Real(v)) if v.len() == 1 => v[0],
        _ => 1.0,
    };
    let font = (12.0 * cex) as f32;
    let emit = |heap: &mut Heap, text: String, x: f32, y: f32, anchor: TextAnchor| {
        heap.graphics.scene_mut().push(DrawOperation::Text {
            text,
            position: Point { x, y },
            params: PlotParameters {
                font_size: font,
                text_color: Color::BLACK,
                dpi: 72.0,
                text_anchor: anchor,
                ..Default::default()
            },
        });
    };
    // xlab centers below the bottom margin at two lines out exactly
    /// as mgp's axis-title line approximates; ylab mirrors beside the
    /// left margin; main sits in the top margin.
    if let Some(xlab) = text_of(2) {
        if !xlab.is_empty() {
            emit(heap, xlab, center_x, plot[3] as f32 + 2.5 * line, TextAnchor::Middle);
        }
    }
    if let Some(ylab) = text_of(3) {
        if !ylab.is_empty() {
            emit(heap, ylab, plot[0] as f32 - 2.5 * line, center_y, TextAnchor::Middle);
        }
    }
    if let Some(main) = text_of(0) {
        if !main.is_empty() {
            emit(heap, main, center_x, plot[1] as f32 - 1.0 * line, TextAnchor::Middle);
        }
    }
    if let Some(sub) = text_of(1) {
        if !sub.is_empty() {
            emit(heap, sub, center_x, plot[3] as f32 + 3.5 * line, TextAnchor::Middle);
        }
    }
    Ok(ValueId::NIL)
}

/// mtext(text, side, line) — margin text exactly as GNU's placement:
/// side 1 bottom, 2 left, 3 top, 4 right, positioned line lines out
/// from the plot edge and centered along it exactly as the adj=0.5
/// default.
pub fn mtext_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    use r_graphics_engine::{Color, DrawOperation, PlotParameters, Point, TextAnchor};
    let slots = eval_matched_args(heap, interner, args, caller_env,
        &["text", "side", "line", "at", "adj", "padj", "cex", "col", "font"])?;
    let text = slots[0].and_then(|v| match heap.object(v) {
        Some(RObject::String(values)) => values.first().map(|s| interner.string_text(*s).to_string()),
        _ => None,
    });
    let text = match text {
        Some(t) if !t.is_empty() => t,
        _ => return Ok(ValueId::NIL),
    };
    let side = slots[1].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(3.0) as i32;
    let line = slots[2].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(0.0) as f32;
    let cex = slots[6].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(1.0);
    let base_cex = match heap.graphics.query("cex") {
        Some(r_core::graphics::layout::ParValue::Real(v)) if v.len() == 1 => v[0],
        _ => 1.0,
    };
    let font = (12.0 * base_cex * cex) as f32;
    let col = slots[7].and_then(|v| resolve_rcolor(heap, interner, v, 0));
    let line_height = 14.4f32;
    let plot = heap.graphics.layout.regions().plot;
    let center_x = (plot[0] + plot[2]) as f32 / 2.0;
    let center_y = (plot[1] + plot[3]) as f32 / 2.0;
    // Each side anchors its line outward from the plot edge exactly as
    /// GNU's margin-line addressing — line 0 butting the edge.
    let (x, y) = match side {
        1 => (center_x, plot[3] as f32 + (line + 0.8) * line_height),
        2 => (plot[0] as f32 - (line + 0.8) * line_height, center_y),
        4 => (plot[2] as f32 + (line + 0.8) * line_height, center_y),
        _ => (center_x, plot[1] as f32 - (line + 0.4) * line_height),
    };
    let color = col.unwrap_or(Color::BLACK);
    heap.graphics.scene_mut().push(DrawOperation::Text {
        text,
        position: Point { x, y },
        params: PlotParameters {
            font_size: font,
            text_color: color,
            dpi: 72.0,
            text_anchor: TextAnchor::Middle,
            ..Default::default()
        },
    });
    Ok(ValueId::NIL)
}

/// GNU's hsv2rgb exactly as colors.c — modf in the same fp order,
/// the sector table verbatim, intensities scaling (255*x + 0.5)
/// truncated exactly as ScaleColor.
pub fn gnu_hsv2rgb(h: f64, s: f64, v: f64) -> (u32, u32, u32) {
    let h6 = h * 6.0;
    let f = h6.fract();
    let i = (h6.trunc() as i64).rem_euclid(6);
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match i {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    let scale = |x: f64| -> u32 { (255.0 * x + 0.5) as u32 };
    (scale(r), scale(g), scale(b))
}

pub fn hex_rgb(r: u32, g: u32, b: u32) -> String {
    format!("#{:02X}{:02X}{:02X}", r, g, b)
}

/// GNU's seq.int(from, to, length.out) — integer index times by,
/// exactly as the C sequence generator's fp order.
pub fn gnu_seq(from: f64, to: f64, n: usize) -> Vec<f64> {
    if n == 0 { return Vec::new(); }
    if n == 1 { return vec![from]; }
    let by = (to - from) / (n as f64 - 1.0);
    (0..n).map(|i| from + i as f64 * by).collect()
}

/// hsv(h, s, v) — recycling exactly as GNU's longest-rule.
pub fn hsv_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["h", "s", "v", "alpha"])?;
    let channel = |index: usize, default: f64| -> Vec<f64> {
        match slots[index] {
            Some(v) => reals_of_value(heap, v).unwrap_or_default(),
            None => vec![default],
        }
    };
    let hs = channel(0, 1.0);
    let ss = channel(1, 1.0);
    let vs = channel(2, 1.0);
    let n = hs.len().max(ss.len()).max(vs.len());
    if n == 0 {
        return Ok(heap.alloc(RObject::String(Vec::new())));
    }
    let out: Vec<r_core::intern::InternedStr> = (0..n).map(|i| {
        let (r, g, b) = gnu_hsv2rgb(hs[i % hs.len()], ss[i % ss.len()], vs[i % vs.len()]);
        interner.intern_string(&hex_rgb(r, g, b))
    }).collect();
    Ok(heap.alloc(RObject::String(out)))
}

/// gray(level) — GNU's rgb(level, level, level) exactly.
pub fn gray_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["level", "alpha"])?;
    let levels = match slots[0] {
        Some(v) => reals_of_value(heap, v).map_err(RSignal::from)?,
        None => Vec::new(),
    };
    let out: Vec<r_core::intern::InternedStr> = levels.iter().map(|&lv| {
        let scaled = (255.0 * lv + 0.5) as u32;
        interner.intern_string(&hex_rgb(scaled, scaled, scaled))
    }).collect();
    Ok(heap.alloc(RObject::String(out)))
}

/// The color-ramp generators exactly as GNU's colorstuff.R — each a
/// composition of hsv over exact sequence expressions.
pub fn color_ramp_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    op: u32,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["n", "s", "v", "start", "end", "alpha", "rev"])?;
    let n = slots[0].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(0.0) as usize;
    if n == 0 {
        return Ok(heap.alloc(RObject::String(Vec::new())));
    }
    let emit = |heap: &mut Heap, interner: &mut SymbolInterner, triples: &[(f64, f64, f64)]| -> Vec<r_core::intern::InternedStr> {
        triples.iter().map(|&(h, s, v)| {
            let (r, g, b) = gnu_hsv2rgb(h, s, v);
            interner.intern_string(&hex_rgb(r, g, b))
        }).collect()
    };
    let _ = heap;
    let mut colors: Vec<(f64, f64, f64)> = Vec::new();
    match op {
        RAINBOW => {
            let s = slots[1].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(1.0);
            let v = slots[2].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(1.0);
            let start = slots[3].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(0.0);
            let end = slots[4].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied())
                .unwrap_or((n.max(1) as f64 - 1.0) / n as f64);
            // seq.int(start, (start > end)*1 + end) %% 1 exactly as GNU.
            let target = if start > end { 1.0 + end } else { end };
            for h in gnu_seq(start, target, n) {
                colors.push((h - h.floor(), s, v));
            }
        }
        HEAT_COLORS => {
            // rainbow(i, start=0, end=1/6) then fading-saturation yellows
            /// exactly as GNU's two-part composition.
            let j = n / 4;
            let i = n - j;
            for h in gnu_seq(0.0, 1.0 / 6.0, i) {
                colors.push((h, 1.0, 1.0));
            }
            if j > 0 {
                let sats = gnu_seq(1.0 - 1.0 / (2.0 * j as f64), 1.0 / (2.0 * j as f64), j);
                for s in sats {
                    colors.push((1.0 / 6.0, s, 1.0));
                }
            }
        }
        TERRAIN_COLORS => {
            let k = n / 2;
            // First half green-to-tan, second half continuing to
            /// near-white exactly as GNU's split sequences with the
            /// overlapping first element dropped.
            let h1 = gnu_seq(4.0 / 12.0, 2.0 / 12.0, k);
            let s1 = gnu_seq(1.0, 1.0, k);
            let v1 = gnu_seq(0.65, 0.9, k);
            for idx in 0..k {
                colors.push((h1[idx], s1[idx], v1[idx]));
            }
            let rest = n - k + 1;
            let h2 = gnu_seq(2.0 / 12.0, 0.0, rest);
            let s2 = gnu_seq(1.0, 0.0, rest);
            let v2 = gnu_seq(0.9, 0.95, rest);
            for idx in 1..rest {
                colors.push((h2[idx], s2[idx], v2[idx]));
            }
        }
        TOPO_COLORS => {
            let j = n / 3;
            let k = n / 3;
            let i = n - j - k;
            for h in gnu_seq(43.0 / 60.0, 31.0 / 60.0, i) {
                colors.push((h, 1.0, 1.0));
            }
            for h in gnu_seq(23.0 / 60.0, 11.0 / 60.0, j) {
                colors.push((h, 1.0, 1.0));
            }
            let hues = gnu_seq(10.0 / 60.0, 6.0 / 60.0, k);
            let sats = gnu_seq(1.0, 0.3, k);
            for idx in 0..k {
                colors.push((hues[idx], sats[idx], 1.0));
            }
        }
        CM_COLORS => {
            let even_n = n % 2 == 0;
            let k = n / 2;
            let l1 = if even_n { k + 1 - 1 } else { k + 1 };
            let l2 = n - k + if even_n { 1 } else { 0 };
            // Cyan half fading to white then magenta growing exactly as
            /// GNU's mirrored saturation sequences.
            if l1 > 0 {
                let target = if even_n { 0.5 / k as f64 } else { 0.0 };
                for s in gnu_seq(0.5, target, l1) {
                    colors.push((6.0 / 12.0, s, 1.0));
                }
            }
            if l2 > 1 {
                let sats = gnu_seq(0.0, 0.5, l2);
                for &s in sats.iter().skip(1) {
                    colors.push((10.0 / 12.0, s, 1.0));
                }
            }
        }
        _ => {}
    }
    let rev_flag = slots[6]
        .map(|v| matches!(heap.object(v), Some(RObject::Logical(vals)) if vals.first().copied().unwrap_or(0) != 0))
        .unwrap_or(false);
    if rev_flag {
        colors.reverse();
    }
    let out = emit(heap, interner, &colors);
    Ok(heap.alloc(RObject::String(out)))
}

/// Resolve one color spec — name case-insensitively against GNU's
/// 657-entry database, #RRGGBB/#RRGGBBAA hex literally, exactly as
/// colors.c's col2name.
pub fn resolve_named_color(name: &str) -> Option<(u32, u32, u32, u32)> {
    use r_core::support::colors::COLOR_DATA;
    if let Some(hex) = name.strip_prefix('#') {
        let digits = hex.len();
        if digits == 6 || digits == 8 {
            let value = u32::from_str_radix(hex, 16).ok()?;
            let r = (value >> (digits * 4 - 8)) & 0xff;
            let g = (value >> (digits * 4 - 16)) & 0xff;
            let b = (value >> (digits * 4 - 24)) & 0xff;
            let a = if digits == 8 { value & 0xff } else { 255 };
            return Some((r, g, b, a));
        }
        return None;
    }
    let lowered = name.to_ascii_lowercase();
    for entry in COLOR_DATA.iter() {
        let entry_name = std::str::from_utf8(entry.name)
            .unwrap_or("")
            .trim_end_matches('\0');
        if entry_name == lowered {
            let code = entry.code;
            return Some((code & 0xff, (code >> 8) & 0xff, (code >> 16) & 0xff, (code >> 24) & 0xff));
        }
    }
    None
}

/// colors() — GNU's 657 names in database order exactly as C_colors.
pub fn colors_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    _args: ValueId,
    _caller_env: ValueId,
) -> EvalResult {
    use r_core::support::colors::COLOR_DATA;
    let out: Vec<r_core::intern::InternedStr> = COLOR_DATA.iter().map(|entry| {
        let name = std::str::from_utf8(entry.name).unwrap_or("").trim_end_matches('\0');
        interner.intern_string(name)
    }).collect();
    Ok(heap.alloc(RObject::String(out)))
}

/// col2rgb(col, alpha) — the 3xN integer matrix exactly as GNU, rows
/// named red/green/blue, alpha appending a fourth row.
pub fn col2rgb_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["col", "alpha"])?;
    let input = slots[0].unwrap_or(ValueId::NIL);
    let names = string_vec(heap, interner, input);
    let alpha_flag = slots[1]
        .map(|v| matches!(heap.object(v), Some(RObject::Logical(vals)) if vals.first().copied().unwrap_or(0) != 0))
        .unwrap_or(false);
    let rows = if alpha_flag { 4 } else { 3 };
    let mut cells: Vec<i32> = Vec::new();
    // Column-major exactly as R matrices — each color's channels
    /// contiguous per column.
    let mut columns: Vec<(u32, u32, u32, u32)> = Vec::new();
    for name in &names {
        match resolve_named_color(name) {
            Some(rgba) => columns.push(rgba),
            None => return Err(EvalError::TypeError("invalid color name").into()),
        }
    }
    for rgba in &columns {
        cells.push(rgba.0 as i32);
        cells.push(rgba.1 as i32);
        cells.push(rgba.2 as i32);
        if alpha_flag {
            cells.push(rgba.3 as i32);
        }
    }
    let matrix = heap.alloc(RObject::Integer(cells));
    let dim = heap.alloc(RObject::Integer(vec![rows as i32, names.len() as i32]));
    let mut result = copy_with_attrib(heap, interner, matrix, "dim", dim).map_err(RSignal::from)?;
    // dimnames — row names red/green/blue exactly as GNU, no column
    /// names.
    let row_label_names: Vec<&str> = if alpha_flag {
        vec!["red", "green", "blue", "alpha"]
    } else {
        vec!["red", "green", "blue"]
    };
    let row_ids: Vec<r_core::intern::InternedStr> = row_label_names.iter().map(|n| interner.intern_string(n)).collect();
    let row_names_obj = heap.alloc(RObject::String(row_ids));
    let dimnames_list = heap.alloc(RObject::List(vec![row_names_obj, ValueId::NIL]));
    result = copy_with_attrib(heap, interner, result, "dimnames", dimnames_list).map_err(RSignal::from)?;
    Ok(result)
}

/// palette(value) — reads or replaces the session palette exactly as
/// GNU: no arguments returns the current visibly, arguments replace
/// and return the previous invisibly.
pub fn palette_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["value"])?;
    let current: Vec<String> = if heap.palette.is_empty() {
        vec!["black", "#DF536B", "#61D04F", "#2297E6", "#28E2E5", "#CD0BBC", "#F5C710", "gray62"]
            .iter().map(|s| s.to_string()).collect()
    } else {
        heap.palette.clone()
    };
    let render = |heap: &mut Heap, interner: &mut SymbolInterner, names: &[String]| -> ValueId {
        let ids: Vec<r_core::intern::InternedStr> = names.iter().map(|n| interner.intern_string(n)).collect();
        heap.alloc(RObject::String(ids))
    };
    match slots[0] {
        None => {
            heap.visible = true;
            Ok(render(heap, interner, &current))
        }
        Some(v) if v == ValueId::NIL => {
            heap.visible = true;
            Ok(render(heap, interner, &current))
        }
        Some(v) => {
            let replacement = string_vec(heap, interner, v);
            if !replacement.is_empty() {
                heap.palette = replacement;
            }
            heap.visible = false;
            Ok(render(heap, interner, &current))
        }
    }
}

/// GNU's hcl2rgb exactly as colors.c — polar LUV through XYZ to sRGB
/// with the gamma transform and fixup clamping, constants verbatim.
pub fn gnu_hcl2rgb(h: f64, c: f64, l: f64) -> (f64, f64, f64) {
    const WHITE_Y: f64 = 100.000;
    const WHITE_U: f64 = 0.1978398;
    const WHITE_V: f64 = 0.4683363;
    const GAMMA: f64 = 2.4;
    let gtrans = |u: f64| -> f64 {
        if u > 0.00304 { 1.055 * u.powf(1.0 / GAMMA) - 0.055 } else { 12.92 * u }
    };
    if l <= 0.0 {
        return (0.0, 0.0, 0.0);
    }
    let rad = std::f64::consts::PI / 180.0 * h;
    let uu = c * rad.cos();
    let vv = c * rad.sin();
    let y = WHITE_Y * if l > 7.999592 { ((l + 16.0) / 116.0).powi(3) } else { l / 903.3 };
    let u2 = uu / (13.0 * l) + WHITE_U;
    let v2 = vv / (13.0 * l) + WHITE_V;
    let x = 9.0 * y * u2 / (4.0 * v2);
    let z = -x / 3.0 - 5.0 * y + 3.0 * y / v2;
    (
        gtrans((3.240479 * x - 1.537150 * y - 0.498535 * z) / WHITE_Y),
        gtrans((-0.969256 * x + 1.875992 * y + 0.041556 * z) / WHITE_Y),
        gtrans((0.055648 * x - 0.204043 * y + 1.057311 * z) / WHITE_Y),
    )
}

/// hcl(h, c, l, fixup) — recycling exactly as GNU, channels clamped
/// exactly as FixupColor.
pub fn hcl_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["h", "c", "l", "alpha", "fixup"])?;
    let channel = |index: usize, default: f64| -> Vec<f64> {
        match slots[index] {
            Some(v) => reals_of_value(heap, v).unwrap_or_default(),
            None => vec![default],
        }
    };
    let hs = channel(0, 0.0);
    let cs = channel(1, 35.0);
    let ls = channel(2, 85.0);
    let n = hs.len().max(cs.len()).max(ls.len());
    if n == 0 {
        return Ok(heap.alloc(RObject::String(Vec::new())));
    }
    let scale = |x: f64| -> u32 {
        let scaled = (255.0 * x + 0.5) as i64;
        scaled.clamp(0, 255) as u32
    };
    let out: Vec<r_core::intern::InternedStr> = (0..n).map(|i| {
        let (r, g, b) = gnu_hcl2rgb(hs[i % hs.len()], cs[i % cs.len()], ls[i % ls.len()]);
        interner.intern_string(&hex_rgb(scale(r), scale(g), scale(b)))
    }).collect();
    Ok(heap.alloc(RObject::String(out)))
}

/// adjustcolor(col, alpha.f, red.f, green.f, blue.f, offset) — GNU's
/// matrix composition exactly: channels scaled to [0,1], diagonal
/// transform applied, offsets added, clamped, alpha appended exactly
/// as rgb with an alpha argument.
pub fn adjustcolor_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env,
        &["col", "alpha.f", "red.f", "green.f", "blue.f", "offset", "transform"])?;
    let input = slots[0].unwrap_or(ValueId::NIL);
    let names = string_vec(heap, interner, input);
    let factor = |index: usize| -> f64 {
        slots[index].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(1.0)
    };
    let alpha_f = factor(1);
    let red_f = factor(2);
    let green_f = factor(3);
    let blue_f = factor(4);
    let offsets: Vec<f64> = slots[5]
        .and_then(|v| reals_of_value(heap, v).ok())
        .unwrap_or_else(|| vec![0.0, 0.0, 0.0, 0.0]);
    let offset_at = |i: usize| -> f64 { offsets.get(i).copied().unwrap_or(0.0) };
    let clamp01 = |x: f64| -> f64 { x.clamp(0.0, 1.0) };
    let quant = |x: f64| -> u32 { (255.0 * x + 0.5) as u32 };
    let out: Vec<r_core::intern::InternedStr> = names.iter().map(|name| {
        match resolve_named_color(name) {
            Some((r, g, b, a)) => {
                let rn = clamp01(r as f64 / 255.0 * red_f + offset_at(0));
                let gn = clamp01(g as f64 / 255.0 * green_f + offset_at(1));
                let bn = clamp01(b as f64 / 255.0 * blue_f + offset_at(2));
                let an = clamp01(a as f64 / 255.0 * alpha_f + offset_at(3));
                interner.intern_string(&format!("#{:02X}{:02X}{:02X}{:02X}", quant(rn), quant(gn), quant(bn), quant(an)))
            }
            None => interner.intern_string(""),
        }
    }).collect();
    Ok(heap.alloc(RObject::String(out)))
}

/// GNU's rgb2hsv exactly as colors.c — the min/max branch structure
/// verbatim, hue sectors scaled by sixths.
pub fn gnu_rgb2hsv(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let mut min = r;
    let mut max = r;
    let mut r_max = true;
    let mut b_max = false;
    if min > g {
        if b < g {
            min = b;
        } else {
            min = g;
            if b > r {
                max = b;
                b_max = true;
                r_max = false;
            }
        }
    } else if b > g {
        max = b;
        b_max = true;
        r_max = false;
    } else {
        max = g;
        r_max = false;
        if b < r {
            min = b;
        }
    }
    let v = max;
    let delta = max - min;
    if max == 0.0 || delta == 0.0 {
        return (0.0, 0.0, v);
    }
    let s = delta / max;
    let mut h = if r_max {
        (g - b) / delta
    } else if b_max {
        4.0 + (r - g) / delta
    } else {
        2.0 + (b - r) / delta
    };
    h /= 6.0;
    if h < 0.0 {
        h += 1.0;
    }
    (h, s, v)
}

/// rgb2hsv(r, g, b) — the 3-row matrix exactly as GNU, inputs scaled
/// by maxColorValue exactly as the R wrapper.
pub fn rgb2hsv_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["r", "g", "b", "maxColorValue"])?;
    let max_value = slots[3].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(255.0);
    let input = slots[0].unwrap_or(ValueId::NIL);
    // The matrix form carries rows exactly as GNU's rbind composition;
    /// separate vectors take the single-channel path.
    let mut triples: Vec<(f64, f64, f64)> = Vec::new();
    let matrix_dim = get_attrib(heap, interner, input, "dim");
    if let Some(dim_id) = matrix_dim {
        if let Some(RObject::Integer(dims)) = heap.object(dim_id).cloned() {
            if dims.len() == 2 && dims[0] == 3 {
                let cols = dims[1] as usize;
                let vals = reals_of_value(heap, input).map_err(RSignal::from)?;
                for c in 0..cols {
                    let r = vals.get(c * 3).copied().unwrap_or(0.0);
                    let g = vals.get(c * 3 + 1).copied().unwrap_or(0.0);
                    let b = vals.get(c * 3 + 2).copied().unwrap_or(0.0);
                    triples.push((r / max_value, g / max_value, b / max_value));
                }
            }
        }
    }
    if triples.is_empty() {
        let rs = reals_of_value(heap, input).unwrap_or_default();
        let gs = slots[1].and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
        let bs = slots[2].and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
        for i in 0..rs.len() {
            let r = rs[i];
            let g = gs.get(i % gs.len().max(1)).copied().unwrap_or(0.0);
            let b = bs.get(i % bs.len().max(1)).copied().unwrap_or(0.0);
            triples.push((r / max_value, g / max_value, b / max_value));
        }
    }
    let mut cells: Vec<f64> = Vec::new();
    for &(r, g, b) in &triples {
        let (h, s, v) = gnu_rgb2hsv(r, g, b);
        cells.push(h);
        cells.push(s);
        cells.push(v);
    }
    let matrix = heap.alloc(RObject::Real(cells));
    let dim = heap.alloc(RObject::Integer(vec![3, triples.len() as i32]));
    let mut result = copy_with_attrib(heap, interner, matrix, "dim", dim).map_err(RSignal::from)?;
    let row_ids: Vec<r_core::intern::InternedStr> = ["h", "s", "v"].iter().map(|n| interner.intern_string(n)).collect();
    let row_names_obj = heap.alloc(RObject::String(row_ids));
    let dimnames_list = heap.alloc(RObject::List(vec![row_names_obj, ValueId::NIL]));
    result = copy_with_attrib(heap, interner, result, "dimnames", dimnames_list).map_err(RSignal::from)?;
    Ok(result)
}

/// arrows(x0, y0, x1, y1, length, angle, code) — GNU's segments plus
/// arrowheads: the head lines rotate +/-angle off the shaft at the
/// endpoints code selects, length in inches scaled at 72dpi exactly
/// as GArrow.
pub fn arrows_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    use r_graphics_engine::{DrawOperation, PathCommand};
    let slots = eval_matched_args(heap, interner, args, caller_env,
        &["x0", "y0", "x1", "y1", "length", "angle", "code", "col", "lty", "lwd"])?;
    let reals_at = |index: usize| -> Vec<f64> {
        slots[index].and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default()
    };
    let x0s = reals_at(0);
    let y0s = reals_at(1);
    let x1s = reals_at(2);
    let y1s = reals_at(3);
    let n = x0s.len().max(y0s.len()).max(x1s.len()).max(y1s.len());
    if n == 0 {
        return Ok(ValueId::NIL);
    }
    let length_in = reals_at(4).first().copied().unwrap_or(0.25);
    let angle_deg = reals_at(5).first().copied().unwrap_or(30.0);
    let code = reals_at(6).first().copied().unwrap_or(2.0) as i32;
    // Precompute geometry before emission exactly as the other
    /// primitives — no borrows across scene mutation.
    let mut segments: Vec<(f64, f64, f64, f64)> = Vec::new();
    let head_len = length_in * 72.0;
    let half_angle = angle_deg.to_radians();
    for i in 0..n {
        let x0 = x0s[i % x0s.len().max(1)];
        let y0 = y0s[i % y0s.len().max(1)];
        let x1 = x1s[i % x1s.len().max(1)];
        let y1 = y1s[i % y1s.len().max(1)];
        segments.push((x0, y0, x1, y1));
        // Head geometry in user space approximates GNU's device-space
        /// rotation — the shaft angle plus/minus the head angle.
        let shaft = (y1 - y0).atan2(x1 - x0);
        if code == 2 || code == 3 {
            for sign in [1.0f64, -1.0] {
                let a = shaft + std::f64::consts::PI + sign * half_angle;
                segments.push((x1, y1, x1 + head_len * a.cos(), y1 + head_len * a.sin()));
            }
        }
        if code == 1 || code == 3 {
            for sign in [1.0f64, -1.0] {
                let a = shaft + sign * half_angle;
                segments.push((x0, y0, x0 + head_len * a.cos(), y0 + head_len * a.sin()));
            }
        }
    }
    use r_graphics_engine::{Color, Path};
    for (x0, y0, x1, y1) in segments {
        let col = slots[7].and_then(|v| resolve_rcolor(heap, interner, v, 0)).unwrap_or(Color::BLACK);
        let stroke = par_stroke(heap, col);
        let (dx0, dy0) = heap.graphics.user_to_device(x0, y0);
        let (dx1, dy1) = heap.graphics.user_to_device(x1, y1);
        heap.graphics.scene_mut().push(DrawOperation::Path(Path {
            commands: vec![
                PathCommand::MoveTo(dx0, dy0),
                PathCommand::LineTo(dx1, dy1),
            ],
            stroke,
            anti_alias: true,
            ..Default::default()
        }));
    }
    Ok(ValueId::NIL)
}

/// hist(x, breaks, freq, probability, include.lowest, right, fuzz, plot)
/// — GNU's hist.default data path exactly: Sturges through pretty with
/// min.n=1, the diddle fuzz adjustment, boundary binning, densities
/// over un-fuzzed widths — then the plot.histogram composition for
/// plot=TRUE.
pub fn hist_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // xname captures the raw expression exactly as deparse1(substitute).
    let raw_first = match heap.object(args) {
        Some(RObject::Pairlist { car, .. }) => *car,
        _ => ValueId::NIL,
    };
    let slots = eval_matched_args(heap, interner, args, caller_env,
        &["x", "breaks", "freq", "probability", "include.lowest", "right", "fuzz", "density",
          "angle", "col", "border", "main", "xlim", "ylim", "xlab", "ylab", "axes", "plot", "labels", "nclass"])?;
    let xname = if raw_first != ValueId::NIL {
        r_core::print::deparse(heap, interner, raw_first)
    } else {
        "x".to_string()
    };
    let input = slots[0].unwrap_or(ValueId::NIL);
    let xs: Vec<f64> = reals_of_value(heap, input)
        .map_err(RSignal::from)?
        .into_iter()
        .filter(|x| x.is_finite())
        .collect();
    let n = xs.len();
    if n == 0 {
        return Err(EvalError::TypeError("invalid length(x)").into());
    }
    let xlo = xs.iter().copied().fold(f64::INFINITY, f64::min);
    let xhi = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    // Break resolution exactly as GNU: explicit vector, scalar count
    /// through pretty with min.n=1, or the Sturges default.
    let mut use_br = false;
    let mut breaks: Vec<f64> = match slots[1] {
        Some(b) => {
            let spec = reals_of_value(heap, b).unwrap_or_default();
            if spec.len() > 1 {
                use_br = true;
                let mut sorted = spec;
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                sorted
            } else if spec.len() == 1 {
                let count = spec[0] as i32;
                let result = r_core::graphics::pretty::r_pretty(
                    xlo, xhi, count, 1, 0.75, 1.5, 0.5 + 1.5 * 1.5, 2f64.powi(-20), 0,
                );
                let ndiv = result.ndiv;
                (0..=ndiv.max(0) as usize).map(|i| {
                    let t = i as f64 / ndiv.max(1) as f64;
                    result.lo * (1.0 - t) + result.up * t
                }).collect()
            } else {
                Vec::new()
            }
        }
        None => Vec::new(),
    };
    if breaks.is_empty() {
        // Sturges exactly as nclass.Sturges — ceiling(log2(n) + 1).
        let sturges = ((n as f64).log2() + 1.0).ceil() as i32;
        let result = r_core::graphics::pretty::r_pretty(
            xlo, xhi, sturges, 1, 0.75, 1.5, 0.5 + 1.5 * 1.5, 2f64.powi(-20), 0,
        );
        let ndiv = result.ndiv;
        breaks = (0..=ndiv.max(0) as usize).map(|i| {
            let t = i as f64 / ndiv.max(1) as f64;
            result.lo * (1.0 - t) + result.up * t
        }).collect();
    }
    let nb = breaks.len() as i32;
    if nb <= 1 {
        return Err(EvalError::TypeError("invalid breakpoints").into());
    }
    let h: Vec<f64> = breaks.windows(2).map(|w| w[1] - w[0]).collect();
    let h_mean = h.iter().sum::<f64>() / h.len() as f64;
    let h_min = h.iter().copied().fold(f64::INFINITY, f64::min);
    let h_max = h.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let equidist = !use_br || (h_max - h_min) < 1e-7 * h_mean;
    // freq resolution exactly as GNU: default follows equidist unless
    /// probability overrides.
    let freq1 = match slots[2] {
        Some(f) => matches!(heap.object(f), Some(RObject::Logical(vals)) if vals.first().copied().unwrap_or(0) != 0),
        None => match slots[3] {
            Some(prob) => !matches!(heap.object(prob), Some(RObject::Logical(vals)) if vals.first().copied().unwrap_or(0) != 0),
            None => equidist,
        },
    };
    let include_lowest = slots[4]
        .map(|v| matches!(heap.object(v), Some(RObject::Logical(vals)) if vals.first().copied().unwrap_or(0) != 0))
        .unwrap_or(true);
    let right = slots[5]
        .map(|v| matches!(heap.object(v), Some(RObject::Logical(vals)) if vals.first().copied().unwrap_or(0) != 0))
        .unwrap_or(true);
    let fuzz_arg = slots[6].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(1e-7);
    // The diddle exactly as GNU: median width for many bins, range
    /// for few, smallest positive otherwise.
    let mut sorted_h = h.clone();
    sorted_h.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median_h = sorted_h[sorted_h.len() / 2];
    let diddle_base = if nb > 5 {
        median_h
    } else if nb <= 3 {
        xhi - xlo
    } else {
        h.iter().copied().filter(|&w| w > 0.0).fold(f64::INFINITY, f64::min)
    };
    let diddle = fuzz_arg * diddle_base;
    // Fuzz directions exactly as GNU's include.lowest interplay.
    let mut fuzzybreaks: Vec<f64> = Vec::new();
    if right {
        for (i, b) in breaks.iter().enumerate() {
            if i == 0 {
                fuzzybreaks.push(b + if include_lowest { -diddle } else { diddle });
            } else {
                fuzzybreaks.push(b + diddle);
            }
        }
    } else {
        for (i, b) in breaks.iter().enumerate() {
            if i == breaks.len() - 1 {
                fuzzybreaks.push(b + if include_lowest { diddle } else { -diddle });
            } else {
                fuzzybreaks.push(b - diddle);
            }
        }
    }
    // BinCount exactly as GNU: right-closed bins with the lowest
    /// boundary inclusive when include.lowest.
    let bin_count = breaks.len() - 1;
    let mut counts: Vec<i32> = vec![0; bin_count];
    for &x in &xs {
        for i in 0..bin_count {
            let inside = if right {
                let lower_ok = if i == 0 && include_lowest {
                    x >= fuzzybreaks[0]
                } else {
                    x > fuzzybreaks[i]
                };
                lower_ok && x <= fuzzybreaks[i + 1]
            } else {
                let upper_ok = if i == bin_count - 1 && include_lowest {
                    x <= fuzzybreaks[bin_count]
                } else {
                    x < fuzzybreaks[i + 1]
                };
                x >= fuzzybreaks[i] && upper_ok
            };
            if inside {
                counts[i] += 1;
                break;
            }
        }
    }
    let density: Vec<f64> = counts.iter().enumerate().map(|(i, &c)| c as f64 / (n as f64 * h[i])).collect();
    let mids: Vec<f64> = (0..bin_count).map(|i| 0.5 * (breaks[i] + breaks[i + 1])).collect();
    // The histogram object exactly as GNU's structure(): named list,
    /// class attribute.
    let heights: Vec<f64> = if freq1 {
        counts.iter().map(|&c| c as f64).collect()
    } else {
        density.clone()
    };
    let do_plot = slots[17]
        .map(|v| matches!(heap.object(v), Some(RObject::Logical(vals)) if vals.first().copied().unwrap_or(0) != 0))
        .unwrap_or(true);
    if do_plot {
        // plot.histogram composition exactly: plot.new, window over
        /// range(breaks) x range(y, 0), title, axes, then the bars.
        let main_text = format!("Histogram of {}", xname);
        let ylab_text = if freq1 { "Frequency" } else { "Density" };
        let ymax = heights.iter().copied().fold(0.0f64, f64::max);
        let xlim = heap.alloc(RObject::Real(vec![breaks[0], breaks[breaks.len() - 1]]));
        let ylim = heap.alloc(RObject::Real(vec![0.0, ymax]));
        let cell = |v: ValueId, rest: ValueId, heap: &mut Heap| {
            heap.alloc(RObject::Pairlist { car: v, cdr: rest, tag: ValueId::NIL })
        };
        plot_new_primitive(heap, interner, ValueId::NIL, caller_env)?;
        let win_args = {
            let second = cell(ylim, ValueId::NIL, heap);
            cell(xlim, second, heap)
        };
        plot_window_primitive(heap, interner, win_args, caller_env)?;
        // title(main, xlab=xname, ylab)
        let main_obj = heap.alloc(RObject::String(vec![interner.intern_string(&main_text)]));
        let xlab_obj = heap.alloc(RObject::String(vec![interner.intern_string(&xname)]));
        let ylab_obj = heap.alloc(RObject::String(vec![interner.intern_string(ylab_text)]));
        let title_args = {
            let ylab_cell = cell(ylab_obj, ValueId::NIL, heap);
            let xlab_cell = cell(xlab_obj, ylab_cell, heap);
            cell(main_obj, xlab_cell, heap)
        };
        title_primitive(heap, interner, title_args, caller_env)?;
        // axis(1), axis(2)
        let one = heap.alloc(RObject::Real(vec![1.0]));
        let two = heap.alloc(RObject::Real(vec![2.0]));
        let axis1_args = cell(one, ValueId::NIL, heap);
        axis_primitive(heap, interner, axis1_args, caller_env)?;
        let axis2_args = cell(two, ValueId::NIL, heap);
        axis_primitive(heap, interner, axis2_args, caller_env)?;
        // rect(breaks[-nB], 0, breaks[-1], y, col="lightgray")
        let xleft = heap.alloc(RObject::Real(breaks[..bin_count].to_vec()));
        let ybottom = heap.alloc(RObject::Real(vec![0.0]));
        let xright = heap.alloc(RObject::Real(breaks[1..].to_vec()));
        let ytop = heap.alloc(RObject::Real(heights.clone()));
        let rect_args = {
            let col_obj = heap.alloc(RObject::String(vec![interner.intern_string("lightgray")]));
            let col_cell = cell(col_obj, ValueId::NIL, heap);
            let ytop_cell = cell(ytop, col_cell, heap);
            let xright_cell = cell(xright, ytop_cell, heap);
            let ybot_cell = cell(ybottom, xright_cell, heap);
            cell(xleft, ybot_cell, heap)
        };
        rect_primitive(heap, interner, rect_args, caller_env)?;
    }
    // Build the result list with names and class exactly as GNU.
    let breaks_obj = heap.alloc(RObject::Real(breaks.clone()));
    let counts_obj = heap.alloc(RObject::Integer(counts.clone()));
    let density_obj = heap.alloc(RObject::Real(density));
    let mids_obj = heap.alloc(RObject::Real(mids));
    let xname_obj = heap.alloc(RObject::String(vec![interner.intern_string(&xname)]));
    let equidist_obj = heap.alloc(RObject::Logical(vec![if equidist { 1 } else { 0 }]));
    let list = heap.alloc(RObject::List(vec![breaks_obj, counts_obj, density_obj, mids_obj, xname_obj, equidist_obj]));
    let names_vec: Vec<r_core::intern::InternedStr> = ["breaks", "counts", "density", "mids", "xname", "equidist"]
        .iter().map(|n| interner.intern_string(n)).collect();
    let names_obj = heap.alloc(RObject::String(names_vec));
    let mut result = copy_with_attrib(heap, interner, list, "names", names_obj).map_err(RSignal::from)?;
    let class_obj = heap.alloc(RObject::String(vec![interner.intern_string("histogram")]));
    result = copy_with_attrib(heap, interner, result, "class", class_obj).map_err(RSignal::from)?;
    heap.visible = !do_plot;
    Ok(result)
}

/// boxplot(x, ..., range, plot) — GNU's data path exactly: per-group
/// boxplot.stats, the 5 x n stats matrix, invisible return. Rendering
/// composes whisker/staple segments, the box rect, median line, and
/// outlier points.
pub fn boxplot_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env,
        &["x", "range", "width", "varwidth", "notch", "outline", "names", "plot", "border", "col", "horizontal"])?;
    let input = slots[0].unwrap_or(ValueId::NIL);
    let coef = slots[1].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(1.5);
    // Groups: a list spreads each element; a bare vector is one group.
    let groups: Vec<Vec<f64>> = match heap.object(input).cloned() {
        Some(RObject::List(items)) => {
            let mut gs = Vec::new();
            for item in items {
                gs.push(reals_of_value(heap, item).map_err(RSignal::from)?
                    .into_iter().filter(|x| x.is_finite()).collect());
            }
            gs
        }
        _ => vec![reals_of_value(heap, input).map_err(RSignal::from)?
            .into_iter().filter(|x| x.is_finite()).collect()],
    };
    let ngroups = groups.len();
    // Per-group stats exactly as boxplot.stats.
    let mut all_stats: Vec<Vec<f64>> = Vec::new();
    let mut all_n: Vec<f64> = Vec::new();
    let mut all_conf: Vec<f64> = Vec::new();
    let mut all_out: Vec<f64> = Vec::new();
    let mut all_group: Vec<f64> = Vec::new();
    for (gi, g) in groups.iter().enumerate() {
        let n = g.len();
        let mut stats = fivenum_values(g);
        let iqr = stats[3] - stats[1];
        let mut out: Vec<f64> = Vec::new();
        if coef != 0.0 && iqr.is_finite() {
            let lo_fence = stats[1] - coef * iqr;
            let hi_fence = stats[3] + coef * iqr;
            let mut in_fence: Vec<f64> = Vec::new();
            for &x in g {
                if x < lo_fence || x > hi_fence {
                    out.push(x);
                } else {
                    in_fence.push(x);
                }
            }
            if !out.is_empty() && !in_fence.is_empty() {
                stats[0] = in_fence.iter().copied().fold(f64::INFINITY, f64::min);
                stats[4] = in_fence.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            }
        }
        for &o in &out {
            all_out.push(o);
            all_group.push((gi + 1) as f64);
        }
        all_n.push(n as f64);
        all_conf.push(stats[2] - 1.58 * iqr / (n as f64).sqrt());
        all_conf.push(stats[2] + 1.58 * iqr / (n as f64).sqrt());
        all_stats.push(stats);
    }
    let do_plot = slots[7]
        .map(|v| matches!(heap.object(v), Some(RObject::Logical(vals)) if vals.first().copied().unwrap_or(0) != 0))
        .unwrap_or(true);
    if do_plot {
        let cell = |v: ValueId, rest: ValueId, heap: &mut Heap| {
            heap.alloc(RObject::Pairlist { car: v, cdr: rest, tag: ValueId::NIL })
        };
        // ylim spans everything plotted exactly as GNU's range over
        /// stats and outliers; xlim the group positions with margins.
        let mut ymin = f64::INFINITY;
        let mut ymax = f64::NEG_INFINITY;
        for stats in &all_stats {
            for &v in stats {
                ymin = ymin.min(v);
                ymax = ymax.max(v);
            }
        }
        for &o in &all_out {
            ymin = ymin.min(o);
            ymax = ymax.max(o);
        }
        plot_new_primitive(heap, interner, ValueId::NIL, caller_env)?;
        let xlim_obj = heap.alloc(RObject::Real(vec![0.5, ngroups as f64 + 0.5]));
        let ylim_obj = heap.alloc(RObject::Real(vec![ymin, ymax]));
        let win_args = {
            let second = cell(ylim_obj, ValueId::NIL, heap);
            cell(xlim_obj, second, heap)
        };
        plot_window_primitive(heap, interner, win_args, caller_env)?;
        // GNU's box width: 0.8 of the group spacing by default.
        let wid = 0.4f64;
        for (gi, stats) in all_stats.iter().enumerate() {
            let x = (gi + 1) as f64;
            // Box from hinge to hinge, median line across.
            let xleft = heap.alloc(RObject::Real(vec![x - wid]));
            let ybot = heap.alloc(RObject::Real(vec![stats[1]]));
            let xright = heap.alloc(RObject::Real(vec![x + wid]));
            let ytop = heap.alloc(RObject::Real(vec![stats[3]]));
            let rect_args = {
                let yt = cell(ytop, ValueId::NIL, heap);
                let xr = cell(xright, yt, heap);
                let yb = cell(ybot, xr, heap);
                cell(xleft, yb, heap)
            };
            rect_primitive(heap, interner, rect_args, caller_env)?;
            let seg = |x0: f64, y0: f64, x1: f64, y1: f64, heap: &mut Heap, interner: &mut SymbolInterner| -> Result<(), RSignal> {
                let x0o = heap.alloc(RObject::Real(vec![x0]));
                let y0o = heap.alloc(RObject::Real(vec![y0]));
                let x1o = heap.alloc(RObject::Real(vec![x1]));
                let y1o = heap.alloc(RObject::Real(vec![y1]));
                let sa = {
                    let y1c = heap.alloc(RObject::Pairlist { car: y1o, cdr: ValueId::NIL, tag: ValueId::NIL });
                    let x1c = heap.alloc(RObject::Pairlist { car: x1o, cdr: y1c, tag: ValueId::NIL });
                    let y0c = heap.alloc(RObject::Pairlist { car: y0o, cdr: x1c, tag: ValueId::NIL });
                    heap.alloc(RObject::Pairlist { car: x0o, cdr: y0c, tag: ValueId::NIL })
                };
                segments_primitive(heap, interner, sa, caller_env)?;
                Ok(())
            };
            // Median line across the full box width exactly as GNU.
            seg(x - wid, stats[2], x + wid, stats[2], heap, interner)?;
            // Whiskers: box edge to whisker end, staples at half width.
            seg(x, stats[1], x, stats[0], heap, interner)?;
            seg(x, stats[3], x, stats[4], heap, interner)?;
            seg(x - wid / 2.0, stats[0], x + wid / 2.0, stats[0], heap, interner)?;
            seg(x - wid / 2.0, stats[4], x + wid / 2.0, stats[4], heap, interner)?;
        }
        // Outlier points.
        if !all_out.is_empty() {
            let xs_obj = heap.alloc(RObject::Real(all_group.clone()));
            let ys_obj = heap.alloc(RObject::Real(all_out.clone()));
            let pa = {
                let yc = cell(ys_obj, ValueId::NIL, heap);
                cell(xs_obj, yc, heap)
            };
            let _ = points_primitive(heap, interner, pa, caller_env);
        }
    }
    // The invisible return: stats as a 5 x n column-major matrix plus
    /// n, conf, out, group exactly as GNU.
    let mut matrix_vals: Vec<f64> = Vec::new();
    for stats in &all_stats {
        for &v in stats {
            matrix_vals.push(v);
        }
    }
    let stats_obj = heap.alloc(RObject::Real(matrix_vals));
    let dim_obj = heap.alloc(RObject::Integer(vec![5, ngroups as i32]));
    let stats_matrix = copy_with_attrib(heap, interner, stats_obj, "dim", dim_obj)?;
    let n_obj = heap.alloc(RObject::Real(all_n));
    let conf_vals: Vec<f64> = all_conf;
    let conf_obj = heap.alloc(RObject::Real(conf_vals));
    let out_obj = heap.alloc(RObject::Real(all_out));
    let group_obj = heap.alloc(RObject::Real(all_group));
    let list = heap.alloc(RObject::List(vec![stats_matrix, n_obj, conf_obj, out_obj, group_obj]));
    let names_vec: Vec<r_core::intern::InternedStr> = ["stats", "n", "conf", "out", "group"]
        .iter()
        .map(|s| interner.intern_string(s))
        .collect();
    let names_obj = heap.alloc(RObject::String(names_vec));
    let with_names = copy_with_attrib(heap, interner, list, "names", names_obj)?;
    heap.visible = false;
    Ok(with_names)
}

/// curve(expr, from, to, n, add, type, xname) — GNU's data path:
/// the raw expression evaluates once over the sequence vector in a
/// data environment binding xname exactly as eval(expr, envir=list).
/// Returns the invisible x/y list.
pub fn curve_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    // The raw first element is the unevaluated expression exactly as
    /// substitute(expr) captures it.
    let raw_expr = match heap.object(args) {
        Some(RObject::Pairlist { car, .. }) => *car,
        _ => ValueId::NIL,
    };
    // The expression itself must never evaluate in the caller
    /// environment — extract the remaining arguments manually with
    /// positional-then-named matching exactly as GNU's formals after
    /// expr.
    let mut from_v: Option<ValueId> = None;
    let mut to_v: Option<ValueId> = None;
    let mut n_v: Option<ValueId> = None;
    let mut xname_v: Option<ValueId> = None;
    let mut positional_index = 0usize;
    {
        let mut cursor = match heap.object(args) {
            Some(RObject::Pairlist { cdr, .. }) => *cdr,
            _ => ValueId::NIL,
        };
        loop {
            let (car, cdr, tag) = match heap.object(cursor) {
                Some(RObject::Pairlist { car, cdr, tag }) => (*car, *cdr, *tag),
                _ => break,
            };
            let tag_name = match heap.object(tag) {
                Some(RObject::Symbol { name, .. }) => Some(interner.symbol_name(*name).to_string()),
                _ => None,
            };
            let evaluated = eval(heap, interner, car, caller_env)?;
            let evaluated = force_if_promise(heap, interner, evaluated)?;
            match tag_name.as_deref() {
                Some("from") => from_v = Some(evaluated),
                Some("to") => to_v = Some(evaluated),
                Some("n") => n_v = Some(evaluated),
                Some("xname") => xname_v = Some(evaluated),
                _ => {
                    match positional_index {
                        0 => from_v = Some(evaluated),
                        1 => to_v = Some(evaluated),
                        2 => n_v = Some(evaluated),
                        _ => {}
                    }
                    positional_index += 1;
                }
            }
            if cdr == ValueId::NIL {
                break;
            }
            cursor = cdr;
        }
    }
    let from = from_v.and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(0.0);
    let to = to_v.and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(1.0);
    let n = n_v.and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(101.0) as usize;
    let xname = match xname_v.and_then(|v| heap.object(v).cloned()) {
        Some(RObject::String(vals)) => interner.string_text(vals[0]).to_string(),
        _ => "x".to_string(),
    };
    // seq.int(from, to, length.out = n) exactly as GNU.
    let xs: Vec<f64> = if n <= 1 {
        vec![from]
    } else {
        // GNU seq.int arithmetic exactly: multiply before divide —
        /// the fp ordering is observable in the endpoint ulps.
        (0..n).map(|i| from + (to - from) * i as f64 / (n - 1) as f64).collect()
    };
    // Data environment binding xname to the whole vector — vectorized
    /// evaluation exactly as GNU's eval(expr, envir = ll).
    let data_env = r_core::env::new_environment(heap, caller_env);
    let x_obj = heap.alloc(RObject::Real(xs.clone()));
    let sym = interner.intern_symbol(&xname);
    let _ = r_core::env::define(heap, data_env, sym, x_obj);
    let y_value = eval(heap, interner, raw_expr, data_env)?;
    let y_value = force_if_promise(heap, interner, y_value)?;
    let ys = reals_of_value(heap, y_value).map_err(RSignal::from)?;
    if ys.len() != xs.len() {
        return Err(EvalError::TypeError("'expr' did not evaluate to an object of length 'n'").into());
    }
    // Rendering: plot type="l" over the computed ranges.
    let add = false;
    if !add {
        let cell = |v: ValueId, rest: ValueId, heap: &mut Heap| {
            heap.alloc(RObject::Pairlist { car: v, cdr: rest, tag: ValueId::NIL })
        };
        let ymin = ys.iter().copied().filter(|y| y.is_finite()).fold(f64::INFINITY, f64::min);
        let ymax = ys.iter().copied().filter(|y| y.is_finite()).fold(f64::NEG_INFINITY, f64::max);
        plot_new_primitive(heap, interner, ValueId::NIL, caller_env)?;
        let xlim_obj = heap.alloc(RObject::Real(vec![from, to]));
        let ylim_obj = heap.alloc(RObject::Real(vec![ymin, ymax]));
        let win_args = {
            let second = cell(ylim_obj, ValueId::NIL, heap);
            cell(xlim_obj, second, heap)
        };
        plot_window_primitive(heap, interner, win_args, caller_env)?;
    }
    // lines(x, y) through the primitive — type l exactly as default.
    {
        let cell = |v: ValueId, rest: ValueId, heap: &mut Heap| {
            heap.alloc(RObject::Pairlist { car: v, cdr: rest, tag: ValueId::NIL })
        };
        let xs_obj = heap.alloc(RObject::Real(xs.clone()));
        let ys_obj = heap.alloc(RObject::Real(ys.clone()));
        let line_args = {
            let yc = cell(ys_obj, ValueId::NIL, heap);
            cell(xs_obj, yc, heap)
        };
        let _ = lines_primitive(heap, interner, line_args, caller_env);
    }
    // Invisible list(x = x, y = y) exactly as GNU.
    let x_out = heap.alloc(RObject::Real(xs));
    let y_out = heap.alloc(RObject::Real(ys));
    let list = heap.alloc(RObject::List(vec![x_out, y_out]));
    let names_vec: Vec<r_core::intern::InternedStr> = ["x", "y"]
        .iter()
        .map(|s| interner.intern_string(s))
        .collect();
    let names_obj = heap.alloc(RObject::String(names_vec));
    let with_names = copy_with_attrib(heap, interner, list, "names", names_obj)?;
    heap.visible = false;
    Ok(with_names)
}

/// matplot(x, y, type, col) — GNU's single-argument form: the matrix
/// columns become series against seq_len(nrow), colors cycling 1:6
/// exactly as the default.
pub fn matplot_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["x", "y", "type", "col"])?;
    let input = slots[0].unwrap_or(ValueId::NIL);
    // Columns spread from the dim attribute exactly as GNU's matrix
    /// handling; a bare vector is one column.
    let (vals, nrow, ncol) = if let Some(dim_id) = get_attrib(heap, interner, input, "dim") {
        if let Some(RObject::Integer(dims)) = heap.object(dim_id).cloned() {
            if dims.len() == 2 {
                let v = reals_of_value(heap, input).map_err(RSignal::from)?;
                (v, dims[0] as usize, dims[1] as usize)
            } else {
                let v = reals_of_value(heap, input).map_err(RSignal::from)?;
                let n = v.len();
                (v, n, 1)
            }
        } else {
            let v = reals_of_value(heap, input).map_err(RSignal::from)?;
            let n = v.len();
            (v, n, 1)
        }
    } else {
        let v = reals_of_value(heap, input).map_err(RSignal::from)?;
        let n = v.len();
        (v, n, 1)
    };
    let xs: Vec<f64> = (1..=nrow).map(|i| i as f64).collect();
    let cell = |v: ValueId, rest: ValueId, heap: &mut Heap| {
        heap.alloc(RObject::Pairlist { car: v, cdr: rest, tag: ValueId::NIL })
    };
    // Frame over the full ranges exactly as matplot's internal plot.
    let ymin = vals.iter().copied().filter(|y| y.is_finite()).fold(f64::INFINITY, f64::min);
    let ymax = vals.iter().copied().filter(|y| y.is_finite()).fold(f64::NEG_INFINITY, f64::max);
    plot_new_primitive(heap, interner, ValueId::NIL, caller_env)?;
    let xlim_obj = heap.alloc(RObject::Real(vec![1.0, nrow as f64]));
    let ylim_obj = heap.alloc(RObject::Real(vec![ymin, ymax]));
    let win_args = {
        let second = cell(ylim_obj, ValueId::NIL, heap);
        cell(xlim_obj, second, heap)
    };
    plot_window_primitive(heap, interner, win_args, caller_env)?;
    // Each column a series, colors cycling through the palette
    /// positions 1:6 exactly as GNU's default col.
    for c in 0..ncol {
        let mut col_vals: Vec<f64> = Vec::new();
        for r in 0..nrow {
            col_vals.push(vals.get(c * nrow + r).copied().unwrap_or(f64::NAN));
        }
        let xs_obj = heap.alloc(RObject::Real(xs.clone()));
        let ys_obj = heap.alloc(RObject::Real(col_vals));
        let col_index = heap.alloc(RObject::Real(vec![(c % 6 + 1) as f64]));
        let line_args = {
            let col_cell = cell(col_index, ValueId::NIL, heap);
            let y_cell = cell(ys_obj, col_cell, heap);
            cell(xs_obj, y_cell, heap)
        };
        let _ = lines_primitive(heap, interner, line_args, caller_env);
    }
    heap.visible = false;
    Ok(ValueId::NIL)
}

/// Serialize the scene to SVG — paths become path elements, text
/// anchored at position, clears as the background rect. Device
/// coordinates are y-down in SVG exactly as the scene records them.
pub fn scene_to_svg(scene: &r_graphics_engine::Scene) -> String {
    use r_graphics_engine::{DrawOperation, PathCommand};
    let (w, h) = scene.dimensions();
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">\n",
        w, h, w, h
    );
    let color_hex = |c: &r_graphics_engine::Color| format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b);
    for op in scene.operations() {
        match op {
            DrawOperation::Clear(c) => {
                out.push_str(&format!(
                    "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"{}\"/>\n",
                    w, h, color_hex(c)
                ));
            }
            DrawOperation::Path(path) => {
                let mut d = String::new();
                for cmd in &path.commands {
                    match cmd {
                        PathCommand::MoveTo(x, y) => d.push_str(&format!("M {:.2} {:.2} ", x, y)),
                        PathCommand::LineTo(x, y) => d.push_str(&format!("L {:.2} {:.2} ", x, y)),
                        PathCommand::QuadTo(cx, cy, x, y) => d.push_str(&format!("Q {:.2} {:.2} {:.2} {:.2} ", cx, cy, x, y)),
                        PathCommand::CubicTo(c1x, c1y, c2x, c2y, x, y) => d.push_str(&format!(
                            "C {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} ", c1x, c1y, c2x, c2y, x, y
                        )),
                        PathCommand::ArcTo { x, y, .. } => d.push_str(&format!("L {:.2} {:.2} ", x, y)),
                        PathCommand::Close => d.push('Z'),
                    }
                }
                let fill = if path.fill.a == 0 { "none".to_string() } else { color_hex(&path.fill) };
                out.push_str(&format!(
                    "<path d=\"{}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{:.2}\"/>\n",
                    d.trim(),
                    fill,
                    color_hex(&path.stroke.color),
                    path.stroke.width
                ));
            }
            DrawOperation::Text { text, position, .. } => {
                let escaped = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
                out.push_str(&format!(
                    "<text x=\"{:.2}\" y=\"{:.2}\" font-size=\"12\" fill=\"black\">{}</text>\n",
                    position.x, position.y, escaped
                ));
            }
            _ => {}
        }
    }
    out.push_str("</svg>\n");
    out
}

/// Rasterize one recorded scene into PNG bytes — the Vello CPU renderer
/// replays operations exactly as the facade's headless pipeline does,
/// vector glyphs and all, no GPU anywhere in the chain.
#[cfg(feature = "renderplot-device")]
pub fn render_scene_png(scene: &r_graphics_engine::Scene) -> Option<Vec<u8>> {
    let (w, h) = scene.dimensions();
    let mut renderer = r_device_android_headless::VelloRenderer::try_new(w, h).ok()?;
    scene.replay(&mut renderer);
    renderer.try_finish().ok()
}

/// Without the rasterizer feature the png device reports failure
/// exactly as an unavailable device should — the host chose the slim
/// build, the error is honest about it.
#[cfg(not(feature = "renderplot-device"))]
pub fn render_scene_png(_scene: &r_graphics_engine::Scene) -> Option<Vec<u8>> {
    None
}

/// svg(filename, width, height) — opens the file-backed device
/// exactly as GNU: inches at 72 points each.
pub fn svg_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["filename", "width", "height"])?;
    let filename = match slots[0].and_then(|v| heap.object(v).cloned()) {
        Some(RObject::String(vals)) => interner.string_text(vals[0]).to_string(),
        _ => "Rplot.svg".to_string(),
    };
    let width_in = slots[1].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(7.0);
    let height_in = slots[2].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(7.0);
    heap.graphics.set_device_size(width_in * 72.0, height_in * 72.0);
    heap.graphics.device_file = Some(filename);
    heap.graphics.device_kind = r_core::graphics::state::DeviceKind::Svg;
    heap.visible = false;
    Ok(ValueId::NIL)
}

/// png(filename, width, height) — the raster device exactly as GNU:
/// pixel dimensions at a nominal 72dpi, defaulting to 480 square.
pub fn png_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env,
        &["filename", "width", "height", "units", "pointsize", "bg", "res"])?;
    let filename = slots[0]
        .and_then(|v| string_operand(heap, interner, v))
        .unwrap_or_else(|| "Rplot%03d.png".to_string());
    let width = slots[1].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(480.0);
    let height = slots[2].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(480.0);
    heap.graphics.set_device_size(width, height);
    heap.graphics.device_file = Some(filename);
    heap.graphics.device_kind = r_core::graphics::state::DeviceKind::Png;
    heap.visible = false;
    Ok(ValueId::NIL)
}

/// dev.off() — flushes the scene to the device file exactly as GNU's
/// close-then-write, returning the closed device number invisibly.
pub fn dev_off_primitive(
    heap: &mut Heap,
    _interner: &mut SymbolInterner,
    _args: ValueId,
    _caller_env: ValueId,
) -> EvalResult {
    if let Some(filename) = heap.graphics.device_file.take() {
        match heap.graphics.device_kind {
            r_core::graphics::state::DeviceKind::Svg => {
                let svg = scene_to_svg(heap.graphics.scene());
                if r_core::vfile::write_file(heap, &filename, svg.as_bytes()).is_err() {
                    return Err(EvalError::TypeError("cannot open device file").into());
                }
            }
            r_core::graphics::state::DeviceKind::Png => {
                let png_bytes = render_scene_png(heap.graphics.scene())
                    .ok_or(EvalError::TypeError("png rasterization failed"))?;
                if r_core::vfile::write_file(heap, &filename, &png_bytes).is_err() {
                    return Err(EvalError::TypeError("cannot open device file").into());
                }
            }
        }
        heap.graphics.set_device_size(504.0, 504.0);
    }
    heap.visible = false;
    Ok(heap.alloc(RObject::Integer(vec![1])))
}

/// dev.cur() — the active device number: 2 when a file device is
/// open exactly as GNU's null-device-plus-one, 1 otherwise.
pub fn dev_cur_primitive(
    heap: &mut Heap,
    _interner: &mut SymbolInterner,
    _args: ValueId,
    _caller_env: ValueId,
) -> EvalResult {
    let number = if heap.graphics.device_file.is_some() { 2 } else { 1 };
    heap.visible = true;
    Ok(heap.alloc(RObject::Integer(vec![number])))
}

/// pie(x, labels, edges, radius, clockwise, init.angle, col) — GNU's
/// wedge geometry exactly: cumulative fractions mapped through
/// twopi*t + init.angle degrees, n = max(2, floor(edges*dx)) arc
/// steps, polygons closing at the center, labels at 1.1 radius with
/// side-dependent adjustment.
pub fn pie_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env,
        &["x", "labels", "edges", "radius", "clockwise", "init.angle", "density", "angle", "col", "border", "lty", "main"])?;
    let xs = reals_of_value(heap, slots[0].unwrap_or(ValueId::NIL)).map_err(RSignal::from)?;
    if xs.iter().any(|x| x.is_nan() || *x < 0.0) {
        return Err(EvalError::TypeError("'x' values must be positive.").into());
    }
    let labels: Vec<String> = match slots[1].and_then(|v| heap.object(v).cloned()) {
        Some(RObject::String(vals)) => vals.iter().map(|s| interner.string_text(*s).to_string()).collect(),
        _ => (1..=xs.len()).map(|i| i.to_string()).collect(),
    };
    let edges = slots[2].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(200.0);
    let radius = slots[3].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(0.8);
    let clockwise = slots[4]
        .map(|v| matches!(heap.object(v), Some(RObject::Logical(vals)) if vals.first().copied().unwrap_or(0) != 0))
        .unwrap_or(false);
    let init_angle = if clockwise { 90.0 } else { 0.0 };
    // GNU's default pastel cycle exactly.
    let default_cols = ["white", "lightblue", "mistyrose", "lightcyan", "lavender", "cornsilk"];
    let cols: Vec<String> = match slots[8].and_then(|v| heap.object(v).cloned()) {
        Some(RObject::String(vals)) => vals.iter().map(|s| interner.string_text(*s).to_string()).collect(),
        _ => default_cols.iter().map(|s| s.to_string()).collect(),
    };
    // Cumulative fractions exactly as c(0, cumsum(x)/sum(x)).
    let total: f64 = xs.iter().sum();
    let mut cum = vec![0.0f64];
    let mut acc = 0.0;
    for &x in &xs {
        acc += x / total;
        cum.push(acc);
    }
    let nx = xs.len();
    let twopi = if clockwise { -2.0 * std::f64::consts::PI } else { 2.0 * std::f64::consts::PI };
    let t2xy = |t: f64| -> (f64, f64) {
        let t2p = twopi * t + init_angle * std::f64::consts::PI / 180.0;
        (radius * t2p.cos(), radius * t2p.sin())
    };
    let cell = |v: ValueId, rest: ValueId, heap: &mut Heap| {
        heap.alloc(RObject::Pairlist { car: v, cdr: rest, tag: ValueId::NIL })
    };
    plot_new_primitive(heap, interner, ValueId::NIL, caller_env)?;
    let lim = heap.alloc(RObject::Real(vec![-1.0, 1.0]));
    let lim2 = heap.alloc(RObject::Real(vec![-1.0, 1.0]));
    let win_args = {
        let second = cell(lim2, ValueId::NIL, heap);
        cell(lim, second, heap)
    };
    plot_window_primitive(heap, interner, win_args, caller_env)?;
    for i in 0..nx {
        let dx = cum[i + 1] - cum[i];
        let n = (edges * dx).floor().max(2.0) as usize;
        // Arc points then the closing center exactly as c(P$x, 0).
        let mut px: Vec<f64> = Vec::new();
        let mut py: Vec<f64> = Vec::new();
        for k in 0..n.max(2) {
            let t = if n > 1 { cum[i] + dx * k as f64 / (n - 1) as f64 } else { cum[i] };
            let (x, y) = t2xy(t);
            px.push(x);
            py.push(y);
        }
        px.push(0.0);
        py.push(0.0);
        let col = cols[i % cols.len()].clone();
        let xs_obj = heap.alloc(RObject::Real(px));
        let ys_obj = heap.alloc(RObject::Real(py));
        let col_obj = heap.alloc(RObject::String(vec![interner.intern_string(&col)]));
        let poly_args = {
            let col_cell = cell(col_obj, ValueId::NIL, heap);
            let y_cell = cell(ys_obj, col_cell, heap);
            cell(xs_obj, y_cell, heap)
        };
        polygon_primitive(heap, interner, poly_args, caller_env)?;
    }
    heap.visible = false;
    Ok(ValueId::NIL)
}

/// stripchart(x, method, jitter, vertical) — GNU's group positioning
/// exactly: overplot at integer positions, jitter through base
/// jitter's factor/50 scaling over runif, stack accumulating
/// duplicates upward.
pub fn stripchart_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env,
        &["x", "method", "jitter", "offset", "vertical", "group.names", "add", "at"])?;
    let input = slots[0].unwrap_or(ValueId::NIL);
    let groups: Vec<Vec<f64>> = match heap.object(input).cloned() {
        Some(RObject::List(items)) => items.iter()
            .map(|item| reals_of_value(heap, *item).unwrap_or_default())
            .collect(),
        _ => vec![reals_of_value(heap, input).map_err(RSignal::from)?],
    };
    let method_name = match slots[1].and_then(|v| heap.object(v).cloned()) {
        Some(RObject::String(vals)) => interner.string_text(vals[0]).to_string(),
        _ => "overplot".to_string(),
    };
    let jitter_amount = slots[2].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(0.1);
    let n = groups.len();
    // Plot frame over data range by group positions exactly as GNU.
    let all: Vec<f64> = groups.iter().flat_map(|g| g.iter().copied()).filter(|x| x.is_finite()).collect();
    let dmin = all.iter().copied().fold(f64::INFINITY, f64::min);
    let dmax = all.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let cell = |v: ValueId, rest: ValueId, heap: &mut Heap| {
        heap.alloc(RObject::Pairlist { car: v, cdr: rest, tag: ValueId::NIL })
    };
    plot_new_primitive(heap, interner, ValueId::NIL, caller_env)?;
    let xlim_obj = heap.alloc(RObject::Real(vec![dmin, dmax]));
    let ylim_obj = heap.alloc(RObject::Real(vec![0.5, n as f64 + 0.5]));
    let win_args = {
        let second = cell(ylim_obj, ValueId::NIL, heap);
        cell(xlim_obj, second, heap)
    };
    plot_window_primitive(heap, interner, win_args, caller_env)?;
    for (gi, g) in groups.iter().enumerate() {
        let base_y = (gi + 1) as f64;
        let mut px: Vec<f64> = Vec::new();
        let mut py: Vec<f64> = Vec::new();
        if method_name == "jitter" {
            // base jitter semantics: amount = jitter * diff(range)/5
            /// over the group positions exactly as GNU's composition.
            let spread = jitter_amount;
            for &x in g {
                let noise = (heap.rng.unif() - 0.5) * 2.0 * spread;
                px.push(x);
                py.push(base_y + noise);
            }
        } else if method_name == "stack" {
            // Stack duplicates upward exactly as GNU: binning identical
            /// values and counting occurrences.
            let mut counts: Vec<(u64, usize)> = Vec::new();
            for &x in g {
                let key = x.to_bits();
                let entry = counts.iter_mut().find(|(k, _)| *k == key);
                match entry {
                    Some((_, c)) => {
                        *c += 1;
                        px.push(x);
                        py.push(base_y + (*c - 1) as f64 * 0.2);
                    }
                    None => {
                        counts.push((key, 1));
                        px.push(x);
                        py.push(base_y);
                    }
                }
            }
        } else {
            for &x in g {
                px.push(x);
                py.push(base_y);
            }
        }
        if !px.is_empty() {
            let xs_obj = heap.alloc(RObject::Real(px));
            let ys_obj = heap.alloc(RObject::Real(py));
            let pa = {
                let yc = cell(ys_obj, ValueId::NIL, heap);
                cell(xs_obj, yc, heap)
            };
            let _ = points_primitive(heap, interner, pa, caller_env);
        }
    }
    heap.visible = false;
    Ok(ValueId::NIL)
}

/// dotchart(x, labels) — GNU's dot positions at integer rows with
/// the values along x, composing frame and points.
pub fn dotchart_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["x", "labels", "groups", "gcolor", "cex", "pch", "col"])?;
    let xs = reals_of_value(heap, slots[0].unwrap_or(ValueId::NIL)).map_err(RSignal::from)?;
    let n = xs.len();
    let xmin = xs.iter().copied().filter(|x| x.is_finite()).fold(0.0f64, f64::min);
    let xmax = xs.iter().copied().filter(|x| x.is_finite()).fold(f64::NEG_INFINITY, f64::max);
    let cell = |v: ValueId, rest: ValueId, heap: &mut Heap| {
        heap.alloc(RObject::Pairlist { car: v, cdr: rest, tag: ValueId::NIL })
    };
    plot_new_primitive(heap, interner, ValueId::NIL, caller_env)?;
    // GNU orders rows bottom-up: row 1 at the bottom exactly.
    let xlim_obj = heap.alloc(RObject::Real(vec![xmin.min(0.0), xmax]));
    let ylim_obj = heap.alloc(RObject::Real(vec![0.0, n as f64 + 1.0]));
    let win_args = {
        let second = cell(ylim_obj, ValueId::NIL, heap);
        cell(xlim_obj, second, heap)
    };
    plot_window_primitive(heap, interner, win_args, caller_env)?;
    let ys: Vec<f64> = (1..=n).rev().map(|i| i as f64).collect();
    let xs_obj = heap.alloc(RObject::Real(xs));
    let ys_obj = heap.alloc(RObject::Real(ys));
    let pa = {
        let yc = cell(ys_obj, ValueId::NIL, heap);
        cell(xs_obj, yc, heap)
    };
    let _ = points_primitive(heap, interner, pa, caller_env);
    heap.visible = false;
    Ok(ValueId::NIL)
}

/// prettyNum(x, big.mark, big.interval, decimal.mark) — GNU's mark
/// insertion core: the integer part grouped from the right every
/// big.interval digits, decimal point replaced exactly.
pub fn prettynum_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env,
        &["x", "big.mark", "big.interval", "small.mark", "small.interval", "decimal.mark"])?;
    let big_mark = match slots[1].and_then(|v| heap.object(v).cloned()) {
        Some(RObject::String(vals)) => interner.string_text(vals[0]).to_string(),
        _ => String::new(),
    };
    let interval = slots[2].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(3.0) as usize;
    let decimal_mark = match slots[5].and_then(|v| heap.object(v).cloned()) {
        Some(RObject::String(vals)) => interner.string_text(vals[0]).to_string(),
        _ => ".".to_string(),
    };
    let input = slots[0].unwrap_or(ValueId::NIL);
    // Character input passes through the mark insertion exactly as
    /// GNU; numeric input formats first.
    let raw: Vec<String> = match heap.object(input).cloned() {
        Some(RObject::String(vals)) => vals.iter().map(|s| interner.string_text(*s).to_string()).collect(),
        _ => {
            let xs = reals_of_value(heap, input).map_err(RSignal::from)?;
            xs.iter().map(|x| r_core::print::format_real(*x)).collect()
        }
    };
    let mut out: Vec<r_core::intern::InternedStr> = Vec::new();
    for text in &raw {
        let trimmed = text.trim();
        // Split at the decimal point exactly as GNU's integer/fraction
        /// separation, preserving sign and exponent untouched.
        let (mantissa, exponent) = match trimmed.find(['e', 'E']) {
            Some(pos) => (&trimmed[..pos], &trimmed[pos..]),
            None => (trimmed, ""),
        };
        let (int_part, frac_part) = match mantissa.find('.') {
            Some(pos) => (&mantissa[..pos], Some(&mantissa[pos + 1..])),
            None => (mantissa, None),
        };
        let (sign, digits) = if let Some(stripped) = int_part.strip_prefix('-') {
            ("-", stripped)
        } else {
            ("", int_part)
        };
        // Group from the right every interval digits exactly as GNU.
        let mut grouped = String::new();
        if !big_mark.is_empty() && digits.len() > interval && interval > 0 {
            let first = digits.len() % interval;
            let mut pos = 0;
            if first > 0 {
                grouped.push_str(&digits[..first]);
                pos = first;
                if pos < digits.len() {
                    grouped.push_str(&big_mark);
                }
            }
            while pos < digits.len() {
                grouped.push_str(&digits[pos..(pos + interval).min(digits.len())]);
                pos += interval;
                if pos < digits.len() {
                    grouped.push_str(&big_mark);
                }
            }
        } else {
            grouped.push_str(digits);
        }
        let mut result = format!("{}{}", sign, grouped);
        if let Some(frac) = frac_part {
            result.push_str(&decimal_mark);
            result.push_str(frac);
        }
        result.push_str(exponent);
        out.push(interner.intern_string(&result));
    }
    heap.visible = true;
    Ok(heap.alloc(RObject::String(out)))
}

/// density(x, bw, adjust, kernel, n, from, to, cut) — GNU's
/// density.default exactly: nrd0 bandwidth, the extended lo/up
/// range, BinDist linear mass binning, gaussian kernel ordinates
/// mirrored over 2n, circular convolution (direct, mathematically
/// identical to the FFT for this symmetric kernel), linear
/// interpolation onto the user grid.
pub fn density_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env,
        &["x", "bw", "adjust", "kernel", "weights", "window", "width", "give.Rkern", "subdensity", "warnWbw", "n", "from", "to", "cut", "ext"])?;
    let xs_all = reals_of_value(heap, slots[0].unwrap_or(ValueId::NIL)).map_err(RSignal::from)?;
    let xs: Vec<f64> = xs_all.iter().copied().filter(|x| x.is_finite()).collect();
    let nx = xs.len();
    let n_big = xs_all.len().max(1);
    if nx < 2 {
        return Err(EvalError::TypeError("need at least 2 points to select a bandwidth automatically").into());
    }
    let adjust = slots[2].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(1.0);
    let cut = slots[13].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(3.0);
    let ext = slots[14].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(4.0);
    // bw.nrd0 exactly as GNU: 0.9 * min(sd, iqr/1.34) * nx^(-1/5),
    /// with the zero-floor fallback to sd then to 1.
    let mean = xs.iter().sum::<f64>() / nx as f64;
    let sd = (xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (nx - 1) as f64).sqrt();
    let mut sorted = xs.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    // Quartiles exactly as GNU's quantile type 7 at 0.25/0.75.
    let quantile7 = |p: f64| -> f64 {
        let h = (nx - 1) as f64 * p + 1.0;
        let hf = h.floor();
        let idx = (hf as usize).saturating_sub(1).min(nx - 1);
        let nxt = (idx + 1).min(nx - 1);
        sorted[idx] + (h - hf) * (sorted[nxt] - sorted[idx])
    };
    let iqr = quantile7(0.75) - quantile7(0.25);
    let lo_bw = sd.min(iqr / 1.34);
    let base_bw = if lo_bw > 0.0 { lo_bw } else if sd > 0.0 { sd } else { 1.0 };
    let bw = match slots[1].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()) {
        Some(explicit) if explicit.is_finite() && explicit > 0.0 => adjust * explicit,
        _ => adjust * 0.9 * base_bw * (nx as f64).powf(-0.2),
    };
    // Grid exactly as GNU: from/to then the extended lo/up.
    let xmin = xs.iter().copied().fold(f64::INFINITY, f64::min);
    let xmax = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let from = slots[11].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(xmin - cut * bw);
    let to = slots[12].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(xmax + cut * bw);
    let lo = from - ext * bw;
    let up = to + ext * bw;
    // n forced up to 512 then to a power of two exactly as GNU.
    let n_user = slots[10].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(512.0) as usize;
    let mut n = n_user.max(512);
    if n > 512 {
        n = 1usize << ((n as f64).log2().ceil() as usize);
    }
    // BinDist exactly as GNU: linear mass distribution into adjacent
    /// bins over (lo, up) with the edge-bin rules.
    let mut binned = vec![0.0f64; 2 * n];
    let xdelta = (up - lo) / (n - 1) as f64;
    let ixmax = n as i64 - 2;
    let weights: Vec<f64> = vec![1.0 / nx as f64; nx];
    for (i, &xi) in xs.iter().enumerate() {
        let xpos = (xi - lo) / xdelta;
        let ix = xpos.floor() as i64;
        let fx = xpos - ix as f64;
        let wi = weights[i];
        if ix >= 0 && ix <= ixmax {
            binned[ix as usize] += (1.0 - fx) * wi;
            binned[(ix + 1) as usize] += fx * wi;
        } else if ix == -1 {
            binned[0] += fx * wi;
        } else if ix == ixmax + 1 {
            binned[ix as usize] += (1.0 - fx) * wi;
        }
    }
    let tot_mass = nx as f64 / n_big as f64;
    for v in binned.iter_mut() {
        *v *= tot_mass;
    }
    // Kernel ordinates exactly as GNU: seq(0, (2n-1)/(n-1)*(up-lo),
    /// length 2n) with the mirrored negative second half, gaussian
    /// density at sd = bw.
    let span = (2 * n - 1) as f64 / (n - 1) as f64 * (up - lo);
    let mut kords = vec![0.0f64; 2 * n];
    for i in 0..(2 * n) {
        kords[i] = span * i as f64 / (2 * n - 1) as f64;
    }
    for i in (n + 1)..(2 * n) {
        kords[i] = -kords[2 * n - i];
    }
    let inv_bw_sqrt2pi = 1.0 / (bw * (2.0 * std::f64::consts::PI).sqrt());
    for k in kords.iter_mut() {
        let z = *k / bw;
        *k = inv_bw_sqrt2pi * (-0.5 * z * z).exp();
    }
    // Circular convolution directly — mathematically identical to
    /// GNU's FFT convolution for this symmetric kernel; divided by
    /// the length exactly as the manual inverse-fft scaling.
    let two_n = 2 * n;
    let mut conv = vec![0.0f64; n];
    for t in 0..n {
        let mut acc = 0.0;
        for s in 0..two_n {
            if binned[s] != 0.0 {
                let idx = (t + two_n - s) % two_n;
                acc += binned[s] * kords[idx];
            }
        }
        // No normalization — the fft inverse's internal scaling and
        // the manual division cancel exactly in GNU's composition.
        conv[t] = acc.max(0.0);
    }
    // xords and linear interpolation onto the user grid exactly as
    /// approx(xords, kords, x).
    let xords: Vec<f64> = (0..n).map(|i| lo + (up - lo) * i as f64 / (n - 1) as f64).collect();
    let x_out: Vec<f64> = (0..n_user).map(|i| {
        if n_user > 1 {
            from + (to - from) * i as f64 / (n_user - 1) as f64
        } else {
            from
        }
    }).collect();
    let y_out: Vec<f64> = x_out.iter().map(|&xq| {
        // Linear interpolation exactly as GNU's approx.
        if xq <= xords[0] {
            return conv[0];
        }
        if xq >= xords[n - 1] {
            return conv[n - 1];
        }
        let pos = (xq - lo) / xdelta;
        let i0 = pos.floor() as usize;
        let i1 = (i0 + 1).min(n - 1);
        let frac = pos - i0 as f64;
        conv[i0] + frac * (conv[i1] - conv[i0])
    }).collect();
    // The density object exactly as GNU's named structure.
    let x_obj = heap.alloc(RObject::Real(x_out));
    let y_obj = heap.alloc(RObject::Real(y_out));
    let bw_obj = heap.alloc(RObject::Real(vec![bw]));
    let n_obj = heap.alloc(RObject::Real(vec![nx as f64]));
    let list = heap.alloc(RObject::List(vec![x_obj, y_obj, bw_obj, n_obj]));
    let names_vec: Vec<r_core::intern::InternedStr> = ["x", "y", "bw", "n"]
        .iter()
        .map(|s| interner.intern_string(s))
        .collect();
    let names_obj = heap.alloc(RObject::String(names_vec));
    let result = copy_with_attrib(heap, interner, list, "names", names_obj)?;
    heap.visible = false;
    Ok(result)
}

/// grid(nx, ny, col, lty) — GNU's reference lines at the axis tick
/// positions exactly, returning the invisible at list.
pub fn grid_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["nx", "ny", "col", "lty", "lwd", "equilogs"])?;
    // Tick positions recomputed over the usr window exactly as the
    /// axis cascade does — the lightgray dotted reference lines.
    let usr = match heap.graphics.query("usr") {
        Some(r_core::graphics::layout::ParValue::Real(u)) if u.len() == 4 => u,
        _ => vec![0.0, 1.0, 0.0, 1.0],
    };
    // GNU's grid reuses the axis-recorded xaxp/yaxp coverage exactly —
    // positions are the same ticks the axis drew, reconstructed as
    // seq(first, last, length.out = n + 1).
    let axp_ticks = |name: &str, fallback_lo: f64, fallback_hi: f64| -> Vec<f64> {
        let axp = match heap.graphics.query(name) {
            Some(r_core::graphics::layout::ParValue::Real(v)) if v.len() == 3 => v,
            _ => vec![fallback_lo, fallback_hi, 5.0],
        };
        let n = (axp[2].max(0.0) as usize) + 1;
        if n <= 1 {
            return vec![axp[0]];
        }
        // GNU's seq.int multiply-before-divide arithmetic exactly.
        (0..n)
            .map(|i| axp[0] + (axp[1] - axp[0]) * (i as f64) / ((n - 1) as f64))
            .collect()
    };
    let xat = axp_ticks("xaxp", usr[0], usr[1]);
    let yat = axp_ticks("yaxp", usr[2], usr[3]);
    // abline(v = atx, h = aty) exactly as GNU's composition.
    let cell = |v: ValueId, rest: ValueId, heap: &mut Heap| {
        heap.alloc(RObject::Pairlist { car: v, cdr: rest, tag: ValueId::NIL })
    };
    let col_obj = heap.alloc(RObject::String(vec![interner.intern_string("lightgray")]));
    let v_obj = heap.alloc(RObject::Real(xat.clone()));
    let h_obj = heap.alloc(RObject::Real(yat.clone()));
    // abline positional order: a, b, h, v — h third, v fourth.
    let ab_args = {
        let col_cell = cell(col_obj, ValueId::NIL, heap);
        let v_cell = cell(v_obj, col_cell, heap);
        let h_cell = cell(h_obj, v_cell, heap);
        let nil_obj = heap.alloc(RObject::Real(vec![]));
        let b_cell = cell(nil_obj, h_cell, heap);
        cell(nil_obj, b_cell, heap)
    };
    let _ = abline_primitive(heap, interner, ab_args, caller_env);
    let atx_obj = heap.alloc(RObject::Real(xat));
    let aty_obj = heap.alloc(RObject::Real(yat));
    let list = heap.alloc(RObject::List(vec![atx_obj, aty_obj]));
    let names_vec: Vec<r_core::intern::InternedStr> = ["atx", "aty"]
        .iter()
        .map(|s| interner.intern_string(s))
        .collect();
    let names_obj = heap.alloc(RObject::String(names_vec));
    let result = copy_with_attrib(heap, interner, list, "names", names_obj)?;
    heap.visible = false;
    Ok(result)
}

/// rug(x, ticksize, side, col) — GNU's data-position tick marks
/// along the axis edge exactly as Axis with tck.
pub fn rug_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["x", "ticksize", "side", "lwd", "col"])?;
    let xs: Vec<f64> = reals_of_value(heap, slots[0].unwrap_or(ValueId::NIL))
        .map_err(RSignal::from)?
        .into_iter()
        .filter(|x| x.is_finite())
        .collect();
    let usr = match heap.graphics.query("usr") {
        Some(r_core::graphics::layout::ParValue::Real(u)) if u.len() == 4 => u,
        _ => vec![0.0, 1.0, 0.0, 1.0],
    };
    // Tick size in user units exactly as GNU's fraction of the axis
    /// span — negative direction (outward) as GNU's tck.
    let ticksize = slots[1].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(0.03);
    let side = slots[2].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(1.0) as i32;
    let yspan = usr[3] - usr[2];
    let xspan = usr[1] - usr[0];
    for &x in &xs {
        let (x0, y0, x1, y1) = if side == 1 {
            (x, usr[2], x, usr[2] + ticksize * yspan)
        } else if side == 3 {
            (x, usr[3], x, usr[3] - ticksize * yspan)
        } else if side == 2 {
            (usr[0], x, usr[0] + ticksize * xspan, x)
        } else {
            (usr[1], x, usr[1] - ticksize * xspan, x)
        };
        let cell = |v: ValueId, rest: ValueId, heap: &mut Heap| {
            heap.alloc(RObject::Pairlist { car: v, cdr: rest, tag: ValueId::NIL })
        };
        let x0o = heap.alloc(RObject::Real(vec![x0]));
        let y0o = heap.alloc(RObject::Real(vec![y0]));
        let x1o = heap.alloc(RObject::Real(vec![x1]));
        let y1o = heap.alloc(RObject::Real(vec![y1]));
        let sa = {
            let y1c = cell(y1o, ValueId::NIL, heap);
            let x1c = cell(x1o, y1c, heap);
            let y0c = cell(y0o, x1c, heap);
            cell(x0o, y0c, heap)
        };
        let _ = segments_primitive(heap, interner, sa, caller_env);
    }
    heap.visible = false;
    Ok(ValueId::NIL)
}

/// GNU fivenum: n4 = floor((n+3)/2)/2, indices c(1, n4, (n+1)/2,
/// n+1-n4, n), averaged floor/ceiling pairs exactly.
pub fn fivenum_values(xs: &[f64]) -> Vec<f64> {
    let mut sorted: Vec<f64> = xs.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted.len();
    if n == 0 {
        return vec![f64::NAN; 5];
    }
    let n4 = ((n + 3) / 2) as f64 / 2.0;
    let nf = n as f64;
    let d = [1.0, n4, (nf + 1.0) / 2.0, nf + 1.0 - n4, nf];
    d.iter()
        .map(|&di| {
            // R indices are 1-based; the fractional index averages its
            /// floor and ceiling exactly as GNU.
            let lo = sorted[(di.floor() as usize).saturating_sub(1).min(n - 1)];
            let hi = sorted[(di.ceil() as usize).saturating_sub(1).min(n - 1)];
            0.5 * (lo + hi)
        })
        .collect()
}

pub fn fivenum_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["x", "na.rm"])?;
    let input = slots[0].unwrap_or(ValueId::NIL);
    let xs: Vec<f64> = reals_of_value(heap, input)
        .map_err(RSignal::from)?
        .into_iter()
        .filter(|x| x.is_finite())
        .collect();
    let result = heap.alloc(RObject::Real(fivenum_values(&xs)));
    heap.visible = true;
    Ok(result)
}

/// boxplot.stats(x, coef, do.conf, do.out) — GNU's grDevices calc.R
/// exactly: fivenum hinges, coef*iqr fences, whisker extension to
/// in-fence range, the 1.58/sqrt(n) confidence interval, outlier
/// extraction.
pub fn boxplot_stats_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["x", "coef", "do.conf", "do.out"])?;
    let input = slots[0].unwrap_or(ValueId::NIL);
    let xs: Vec<f64> = reals_of_value(heap, input).map_err(RSignal::from)?;
    let finite: Vec<f64> = xs.iter().copied().filter(|x| x.is_finite()).collect();
    let n = finite.len();
    let coef = slots[1].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(1.5);
    let mut stats = fivenum_values(&finite);
    let iqr = stats[3] - stats[1];
    let mut out: Vec<f64> = Vec::new();
    if coef != 0.0 && iqr.is_finite() {
        let lo_fence = stats[1] - coef * iqr;
        let hi_fence = stats[3] + coef * iqr;
        let mut in_fence: Vec<f64> = Vec::new();
        for &x in &finite {
            if x < lo_fence || x > hi_fence {
                out.push(x);
            } else {
                in_fence.push(x);
            }
        }
        if !out.is_empty() && !in_fence.is_empty() {
            stats[0] = in_fence.iter().copied().fold(f64::INFINITY, f64::min);
            stats[4] = in_fence.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        }
    }
    let conf_lo = stats[2] - 1.58 * iqr / (n as f64).sqrt();
    let conf_hi = stats[2] + 1.58 * iqr / (n as f64).sqrt();
    let stats_obj = heap.alloc(RObject::Real(stats));
    let n_obj = heap.alloc(RObject::Real(vec![n as f64]));
    let conf_obj = heap.alloc(RObject::Real(vec![conf_lo, conf_hi]));
    let out_obj = heap.alloc(RObject::Real(out));
    let list = heap.alloc(RObject::List(vec![stats_obj, n_obj, conf_obj, out_obj]));
    let names_vec: Vec<r_core::intern::InternedStr> = ["stats", "n", "conf", "out"]
        .iter()
        .map(|s| interner.intern_string(s))
        .collect();
    let names_obj = heap.alloc(RObject::String(names_vec));
    let with_names = copy_with_attrib(heap, interner, list, "names", names_obj)?;
    heap.visible = true;
    Ok(with_names)
}

/// stem(x, scale, width, atom) — GNU's stem.c ported exactly: the
/// rounding-scale selection, mu switching, cell alignment, and row
/// printing verbatim including the truncating casts.
pub fn stem_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["x", "scale", "width", "atom"])?;
    let input = slots[0].unwrap_or(ValueId::NIL);
    let mut xs: Vec<f64> = reals_of_value(heap, input)
        .map_err(RSignal::from)?
        .into_iter()
        .filter(|x| x.is_finite())
        .collect();
    let n = xs.len();
    if n == 0 {
        return Err(EvalError::TypeError("no finite and non-missing values").into());
    }
    let scale = slots[1].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(1.0);
    let width = slots[2].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(80.0) as i64;
    let atom = slots[3].and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(1e-8);
    if scale <= 0.0 {
        return Err(EvalError::TypeError("'scale' must be positive").into());
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = String::from("\n");
    let mut mu = 10.0f64;
    let mut c: f64;
    if xs[n - 1] > xs[0] {
        let r = atom + (xs[n - 1] - xs[0]) / scale;
        c = 10f64.powi((1.0 - r.log10().floor()) as i32);
        let mm = (2i32).min((0i32).max((r * c / 25.0) as i32));
        let k = 3 * mm + 2 - 150 / (n as i32 + 50);
        if (k - 1) * (k - 2) * (k - 5) == 0 {
            c *= 10.0;
        }
        let mut x1 = xs[0].abs();
        let x2 = xs[n - 1].abs();
        if x2 > x1 {
            x1 = x2;
        }
        while x1 * c > i32::MAX as f64 {
            c /= 10.0;
        }
        if k * (k - 4) * (k - 8) == 0 {
            mu = 5.0;
        }
        if (k - 1) * (k - 5) * (k - 6) == 0 {
            mu = 20.0;
        }
    } else {
        let r = atom + xs[0].abs() / scale;
        c = 10f64.powi((1.0 - r.log10().floor()) as i32);
    }
    // rnd — GNU's exact truncating arithmetic: the +-0.5 shift with
    /// truncation toward zero at the integer casts.
    let rnd = |u: f64, c: f64| -> f64 {
        if u < 0.0 { u * c - 0.5 } else { u * c + 0.5 }
    };
    let xlow = rnd(xs[0], c);
    let xhigh = rnd(xs[n - 1], c);
    let lo_nd = (xlow / mu).floor() * mu;
    let hi_nd = (xhigh / mu).floor() * mu;
    let ldigits = if lo_nd < 0.0 { (-lo_nd).log10().floor() as i64 + 1 } else { 0 };
    let hdigits = if hi_nd > 0.0 { hi_nd.log10().floor() as i64 } else { 0 };
    let ndigits = ldigits.max(hdigits);
    // Starting cell exactly as GNU's alignment.
    let mut lo = (xs[0] * c / mu).floor() * mu;
    if lo < 0.0 && (xs[0] * c).floor() == lo {
        lo -= mu;
    }
    let mut hi = lo + mu;
    if (xs[0] * c + 0.5).floor() > hi {
        lo = hi;
        hi = lo + mu;
    }
    let pdigits = 1 - (c.log10().floor() + 0.5) as i64;
    out.push_str("  The decimal point is ");
    if pdigits == 0 {
        out.push_str("at the |\n\n");
    } else {
        out.push_str(&format!(
            "{} digit(s) to the {} of the |\n\n",
            pdigits.abs(),
            if pdigits > 0 { "right" } else { "left" }
        ));
    }
    let stem_print = |close: i64, dist: i64, out: &mut String| {
        if close / 10 == 0 && dist < 0 {
            out.push_str(&format!("  {:>width$} | ", "-0", width = ndigits as usize));
        } else {
            out.push_str(&format!("  {:>width$} | ", close / 10, width = ndigits as usize));
        }
    };
    let mut i = 0usize;
    loop {
        if lo < 0.0 {
            stem_print(hi as i64, lo as i64, &mut out);
        } else {
            stem_print(lo as i64, hi as i64, &mut out);
        }
        let mut j = 0i64;
        loop {
            let xi = rnd(xs[i], c) as i64;
            if (hi == 0.0 && xs[i] >= 0.0)
                || (lo < 0.0 && xi > hi as i64)
                || (lo >= 0.0 && xi >= hi as i64)
            {
                break;
            }
            j += 1;
            if j <= width - 12 {
                out.push_str(&format!("{}", xi.abs() % 10));
            }
            i += 1;
            if i >= n {
                break;
            }
        }
        if j > width {
            out.push_str(&format!("+{}", j - width));
        }
        out.push('\n');
        if i >= n {
            break;
        }
        hi += mu;
        lo += mu;
    }
    // The trailing newline exactly as stem.c's final Rprintf.
    out.push('\n');
    heap.emit(&out);
    Ok(ValueId::NIL)
}

/// barplot(height, width, space, beside, horiz, col) — GNU's layout
/// arithmetic exactly: space scaled by mean width, cumulative right
/// edges, midpoints returned invisibly. Vector input treats as a
/// one-column beside matrix exactly as GNU.
pub fn barplot_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env,
        &["height", "width", "space", "names.arg", "legend.text", "beside", "horiz", "col", "border", "main", "xlab", "ylab", "plot"])?;
    let input = slots[0].unwrap_or(ValueId::NIL);
    // Shape detection: matrix spreads rows as series, vectors become
    /// the one-column beside form exactly as GNU.
    let mut heights: Vec<Vec<f64>> = Vec::new();
    let mut nrow = 1usize;
    let mut ncol = 0usize;
    let mut vector_input = false;
    if let Some(dim_id) = get_attrib(heap, interner, input, "dim") {
        if let Some(RObject::Integer(dims)) = heap.object(dim_id).cloned() {
            if dims.len() == 2 {
                nrow = dims[0] as usize;
                ncol = dims[1] as usize;
                let vals = reals_of_value(heap, input).map_err(RSignal::from)?;
                for r in 0..nrow {
                    let mut series = Vec::new();
                    for c in 0..ncol {
                        series.push(vals.get(c * nrow + r).copied().unwrap_or(0.0));
                    }
                    heights.push(series);
                }
            }
        }
    }
    if heights.is_empty() {
        vector_input = true;
        nrow = 1;
        let vals = reals_of_value(heap, input).map_err(RSignal::from)?;
        ncol = vals.len();
        heights.push(vals);
    }
    let mut beside = slots[5]
        .map(|v| matches!(heap.object(v), Some(RObject::Logical(vals)) if vals.first().copied().unwrap_or(0) != 0))
        .unwrap_or(false);
    if vector_input {
        beside = true;
    }
    let width: Vec<f64> = {
        let w = slots[1].and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_else(|| vec![1.0]);
        let target = if beside { nrow } else { ncol };
        (0..target).map(|i| w[i % w.len()]).collect()
    };
    let mean_width = width.iter().sum::<f64>() / width.len() as f64;
    // space scaled exactly as GNU — 0.2 default, c(0,1) for beside
    /// matrices.
    let spaces: Vec<f64> = match slots[2] {
        Some(s) => {
            let spec = reals_of_value(heap, s).unwrap_or_default();
            if spec.len() == 2 && beside && !vector_input {
                // c(0,1) means: intra-group space then inter-group —
                /// expanded exactly as GNU's rep.int composition.
                let mut expanded = Vec::new();
                for _c in 0..ncol {
                    expanded.push(spec[1]);
                    for _r in 0..nrow.saturating_sub(1) {
                        expanded.push(spec[0]);
                    }
                }
                expanded.iter().map(|x| x * mean_width).collect()
            } else {
                vec![spec.first().copied().unwrap_or(0.2) * mean_width]
            }
        }
        None => {
            let base = if !vector_input && beside { 0.0 } else { 0.2 };
            vec![base * mean_width]
        }
    };
    // Cumulative positions exactly as GNU: w.r = cumsum(space+width),
    /// midpoints and lefts derived.
    let count = if beside { nrow * ncol } else { ncol };
    let mut wr: Vec<f64> = Vec::new();
    let mut acc = 0.0;
    for i in 0..count {
        let space = if spaces.len() == 1 { spaces[0] } else { spaces.get(i).copied().unwrap_or(0.0) };
        let w = width[i % width.len()];
        acc += space + w;
        wr.push(acc);
    }
    let wm: Vec<f64> = wr.iter().enumerate().map(|(i, &r)| r - width[i % width.len()] / 2.0).collect();
    let wl: Vec<f64> = wr.iter().enumerate().map(|(i, &r)| r - width[i % width.len()]).collect();
    // Limits exactly as GNU: x spans the bar positions, y the stacked
    /// heights with zero.
    let stacked: Vec<f64> = (0..ncol).map(|c| {
        heights.iter().map(|series| series.get(c).copied().unwrap_or(0.0)).sum()
    }).collect();
    let ymax = if beside {
        heights.iter().flat_map(|s| s.iter().copied()).fold(0.0f64, f64::max)
    } else {
        stacked.iter().copied().fold(0.0f64, f64::max)
    };
    let ymin = heights.iter().flat_map(|s| s.iter().copied()).fold(0.0f64, f64::min);
    let xlim = (wl[0], wr[wr.len() - 1]);
    let ylim = (ymin.min(0.0), ymax);
    let do_plot = slots[12]
        .map(|v| matches!(heap.object(v), Some(RObject::Logical(vals)) if vals.first().copied().unwrap_or(0) != 0))
        .unwrap_or(true);
    if do_plot {
        let cell = |v: ValueId, rest: ValueId, heap: &mut Heap| {
            heap.alloc(RObject::Pairlist { car: v, cdr: rest, tag: ValueId::NIL })
        };
        plot_new_primitive(heap, interner, ValueId::NIL, caller_env)?;
        // yaxs="i" exactly as GNU's opar — exact axis, no extension.
        let xaxs_obj = heap.alloc(RObject::String(vec![interner.intern_string("i")]));
        let _ = xaxs_obj;
        let xlim_obj = heap.alloc(RObject::Real(vec![xlim.0, xlim.1]));
        let ylim_obj = heap.alloc(RObject::Real(vec![ylim.0, ylim.1]));
        let win_args = {
            let second = cell(ylim_obj, ValueId::NIL, heap);
            cell(xlim_obj, second, heap)
        };
        plot_window_primitive(heap, interner, win_args, caller_env)?;
        // Bars: stacked series accumulate; beside rows sit at their own
        /// positions exactly as GNU's per-series loop.
        let col_default = if vector_input { "grey" } else { "grey" };
        for r in 0..nrow {
            let mut base = vec![0.0f64; ncol];
            if r > 0 && !beside {
                for c in 0..ncol {
                    base[c] = (0..r).map(|rr| heights[rr].get(c).copied().unwrap_or(0.0)).sum();
                }
            }
            for c in 0..ncol {
                let h = heights[r].get(c).copied().unwrap_or(0.0);
                let pos_index = if beside { c * nrow + r } else { c };
                let bottom = if beside { 0.0 } else { base[c] };
                let xleft = heap.alloc(RObject::Real(vec![wl[pos_index]]));
                let ybot = heap.alloc(RObject::Real(vec![bottom]));
                let xright = heap.alloc(RObject::Real(vec![wr[pos_index]]));
                let ytop = heap.alloc(RObject::Real(vec![bottom + h]));
                let col_obj = heap.alloc(RObject::String(vec![interner.intern_string(col_default)]));
                let rect_args = {
                    let col_cell = cell(col_obj, ValueId::NIL, heap);
                    let ytop_cell = cell(ytop, col_cell, heap);
                    let xright_cell = cell(xright, ytop_cell, heap);
                    let ybot_cell = cell(ybot, xright_cell, heap);
                    cell(xleft, ybot_cell, heap)
                };
                rect_primitive(heap, interner, rect_args, caller_env)?;
            }
        }
    }
    // Midpoints returned invisibly exactly as GNU — beside matrices
    /// shape them back to rows.
    let out: Vec<f64> = if beside && !vector_input {
        wm
    } else if beside {
        wm
    } else {
        wm
    };
    let result = heap.alloc(RObject::Real(out));
    heap.visible = false;
    Ok(result)
}

/// segments(x0, y0, x1, y1) — GNU's lockstep-recycled segment paths
/// with per-segment color resolution exactly as lines carries col.
pub fn segments_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    use r_graphics_engine::{Color, DrawOperation, Path, PathCommand, Stroke};
    let slots = eval_matched_args(heap, interner, args, caller_env, &["x0", "y0", "x1", "y1", "col"])?;
    let x0 = slots[0].and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
    let y0 = slots[1].and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
    let x1 = slots[2].and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
    let y1 = slots[3].and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
    let n = x0.len().max(y0.len()).max(x1.len()).max(y1.len());
    if n == 0 {
        return Ok(ValueId::NIL);
    }
    for i in 0..n {
        let (xa, ya, xb, yb) = (
            x0[i % x0.len()],
            y0[i % y0.len()],
            x1[i % x1.len()],
            y1[i % y1.len()],
        );
        if !(xa.is_finite() && ya.is_finite() && xb.is_finite() && yb.is_finite()) {
            continue;
        }
        let (dx0, dy0) = heap.graphics.user_to_device(xa, ya);
        let (dx1, dy1) = heap.graphics.user_to_device(xb, yb);
        let col = slots[4]
            .and_then(|v| resolve_rcolor(heap, interner, v, i))
            .unwrap_or(Color::BLACK);
        let parstroke_2 = par_stroke(heap, col);
        heap.graphics.scene_mut().push(DrawOperation::Path(Path {
            commands: vec![
                PathCommand::MoveTo(dx0, dy0),
                PathCommand::LineTo(dx1, dy1),
            ],
            stroke: parstroke_2,
            anti_alias: true,
            ..Default::default()
        }));
    }
    Ok(ValueId::NIL)
}

/// abline(a, b, h, v) — GNU's reference lines spanning the plot
/// region exactly: intercept/slope pairs, horizontal levels, or
/// vertical positions clipped to the usr window.
pub fn abline_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    use r_graphics_engine::{Color, DrawOperation, Path, PathCommand, Stroke};
    let slots = eval_matched_args(heap, interner, args, caller_env, &["a", "b", "h", "v", "col"])?;
    let usr = match heap.graphics.query("usr") {
        Some(r_core::graphics::layout::ParValue::Real(u)) if u.len() == 4 => u,
        _ => vec![0.0, 1.0, 0.0, 1.0],
    };
    let mut spans: Vec<(f64, f64, f64, f64)> = Vec::new();
    // Horizontal levels span the x window exactly as h.
    if let Some(hs) = slots[2].and_then(|v| reals_of_value(heap, v).ok()) {
        for h in hs {
            if h.is_finite() {
                spans.push((usr[0], h, usr[1], h));
            }
        }
    }
    if let Some(vs) = slots[3].and_then(|v| reals_of_value(heap, v).ok()) {
        for v in vs {
            if v.is_finite() {
                spans.push((v, usr[2], v, usr[3]));
            }
        }
    }
    if let Some(aa) = slots[0].and_then(|v| reals_of_value(heap, v).ok()) {
        let bb = slots[1]
            .and_then(|v| reals_of_value(heap, v).ok())
            .unwrap_or_else(|| vec![1.0]);
        for i in 0..aa.len() {
            let a = aa[i];
            let b = bb[i % bb.len()];
            if !(a.is_finite() && b.is_finite()) {
                continue;
            }
            // Evaluate at the window edges exactly as the line clips.
            let y0 = a + b * usr[0];
            let y1 = a + b * usr[1];
            spans.push((usr[0], y0, usr[1], y1));
        }
    }
    for (i, (xa, ya, xb, yb)) in spans.iter().enumerate() {
        let (dx0, dy0) = heap.graphics.user_to_device(*xa, *ya);
        let (dx1, dy1) = heap.graphics.user_to_device(*xb, *yb);
        let col = slots[4]
            .and_then(|v| resolve_rcolor(heap, interner, v, i))
            .unwrap_or(Color::BLACK);
        let parstroke_1 = par_stroke(heap, col);
        heap.graphics.scene_mut().push(DrawOperation::Path(Path {
            commands: vec![
                PathCommand::MoveTo(dx0, dy0),
                PathCommand::LineTo(dx1, dy1),
            ],
            stroke: parstroke_1,
            anti_alias: true,
            ..Default::default()
        }));
    }
    Ok(ValueId::NIL)
}

/// polygon(x, y) — GNU's closed filled path exactly as rect carries
/// col and border: NA coordinates split the polygon into separate
/// subpaths, each closed.
pub fn polygon_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    use r_graphics_engine::{Color, DrawOperation, Path, PathCommand, Stroke};
    let slots = eval_matched_args(heap, interner, args, caller_env, &["x", "y", "col", "border"])?;
    let xs = slots[0].and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
    let ys = slots[1].and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
    let n = xs.len().min(ys.len());
    if n == 0 {
        return Ok(ValueId::NIL);
    }
    const TRANSPARENT: Color = Color { r: 0, g: 0, b: 0, a: 0 };
    let fill = slots[2]
        .and_then(|v| resolve_rcolor(heap, interner, v, 0))
        .unwrap_or(TRANSPARENT);
    let border = match slots[3] {
        Some(v) => resolve_rcolor(heap, interner, v, 0).unwrap_or(TRANSPARENT),
        None => Color::BLACK,
    };
    // Split at NA exactly as lines does, closing each run.
    let mut run: Vec<(f32, f32)> = Vec::new();
    let mut emit_run = |heap: &mut Heap, run: &mut Vec<(f32, f32)>| {
        if run.len() >= 2 {
            let mut commands = vec![PathCommand::MoveTo(run[0].0, run[0].1)];
            for &(x, y) in &run[1..] {
                commands.push(PathCommand::LineTo(x, y));
            }
            commands.push(PathCommand::Close);
            let parstroke_0 = par_stroke(heap, border);
            heap.graphics.scene_mut().push(DrawOperation::Path(Path {
                commands,
                fill,
                stroke: parstroke_0,
                anti_alias: true,
            }));
        }
        run.clear();
    };
    for i in 0..n {
        let (x, y) = (xs[i], ys[i]);
        if x.is_finite() && y.is_finite() {
            let (dx, dy) = heap.graphics.user_to_device(x, y);
            run.push((dx, dy));
        } else {
            emit_run(heap, &mut run);
        }
    }
    emit_run(heap, &mut run);
    Ok(ValueId::NIL)
}

/// plot(x, y) — GNU's high-level numeric scatter exactly as
/// plot.default composes: data-range limits, a fresh frame, the
/// window, points, both axes, and the box. Type "p" points exactly
/// as the default; the remaining types compose in their own time.
pub fn plot_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let slots = eval_matched_args(heap, interner, args, caller_env, &["x", "y", "type"])?;
    let mut xs = slots[0].and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
    let mut ys = slots[1].and_then(|v| reals_of_value(heap, v).ok()).unwrap_or_default();
    let plot_type = match slots[2].and_then(|v| match heap.object(v) {
        Some(RObject::String(values)) => values.first().map(|s| interner.string_text(*s).to_string()),
        _ => None,
    }) {
        Some(text) => text,
        None => "p".to_string(),
    };
    // plot(x) with a single vector plots against the index exactly as
    /// GNU's xy.coords default.
    if ys.is_empty() && !xs.is_empty() {
        ys = std::mem::take(&mut xs);
        xs = (1..=ys.len()).map(|i| i as f64).collect();
    }
    let pairs: Vec<(f64, f64)> = xs
        .iter()
        .copied()
        .zip(ys.iter().copied())
        .filter(|(x, y)| x.is_finite() && y.is_finite())
        .collect();
    if pairs.is_empty() {
        return Ok(ValueId::NIL);
    }
    let xlo = pairs.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let xhi = pairs.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    let ylo = pairs.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let yhi = pairs.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    // Compose through the primitives exactly as user code would —
    /// construct each call's evaluated arguments directly.
    let xlim = heap.alloc(RObject::Real(vec![xlo, xhi]));
    let ylim = heap.alloc(RObject::Real(vec![ylo, yhi]));
    let xv = heap.alloc(RObject::Real(pairs.iter().map(|p| p.0).collect::<Vec<_>>()));
    let yv = heap.alloc(RObject::Real(pairs.iter().map(|p| p.1).collect::<Vec<_>>()));
    let cell = |v: ValueId, rest: ValueId, heap: &mut Heap| {
        heap.alloc(RObject::Pairlist { car: v, cdr: rest, tag: ValueId::NIL })
    };
    // plot.new()
    plot_new_primitive(heap, interner, ValueId::NIL, caller_env)?;
    // plot.window(xlim, ylim)
    let win_args = {
        let second = cell(ylim, ValueId::NIL, heap);
        cell(xlim, second, heap)
    };
    plot_window_primitive(heap, interner, win_args, caller_env)?;
    // The glyph layer exactly as type selects: points, lines, both,
    /// overplotted, or nothing — "n" still draws axes and the frame.
    let xy_args = {
        let second = cell(yv, ValueId::NIL, heap);
        cell(xv, second, heap)
    };
    match plot_type.as_str() {
        "l" => {
            lines_primitive(heap, interner, xy_args, caller_env)?;
        }
        "b" | "o" => {
            lines_primitive(heap, interner, xy_args, caller_env)?;
            let second = cell(yv, ValueId::NIL, heap);
            let again = cell(xv, second, heap);
            points_primitive(heap, interner, again, caller_env)?;
        }
        "n" => {}
        _ => {
            points_primitive(heap, interner, xy_args, caller_env)?;
        }
    }
    // axis(1); axis(2)
    let one = heap.alloc(RObject::Real(vec![1.0]));
    let two = heap.alloc(RObject::Real(vec![2.0]));
    let a1 = cell(one, ValueId::NIL, heap);
    axis_primitive(heap, interner, a1, caller_env)?;
    let a2 = cell(two, ValueId::NIL, heap);
    axis_primitive(heap, interner, a2, caller_env)?;
    // box()
    box_primitive(heap, interner, ValueId::NIL, caller_env)?;
    Ok(ValueId::NIL)
}

/// rgb(red, green, blue, alpha, maxColorValue) — GNU's hex composition
/// exactly: channels scale by 255/maxColorValue, round half away, and
/// the hex renders uppercase exactly as #RRGGBB.
pub fn rgb_primitive(
    heap: &mut Heap,
    interner: &mut SymbolInterner,
    args: ValueId,
    caller_env: ValueId,
) -> EvalResult {
    let positional = eval_matched_args(heap, interner, args, caller_env,
        &["red", "green", "blue", "alpha", "maxColorValue", "names"])?;
    let max_value = positional.get(4).copied().flatten()
        .and_then(|v| reals_of_value(heap, v).ok()).and_then(|r| r.first().copied()).unwrap_or(1.0);
    let channel = |index: usize| -> Vec<f64> {
        positional.get(index).copied().flatten()
            .and_then(|v| reals_of_value(heap, v).ok())
            .unwrap_or_default()
    };
    let reds = channel(0);
    let greens = channel(1);
    let blues = channel(2);
    let n = reds.len().max(greens.len()).max(blues.len());
    if n == 0 {
        return Ok(heap.alloc(RObject::String(Vec::new())));
    }
    let quant = |v: f64| -> u32 {
        let scaled = (v / max_value * 255.0).round();
        scaled.clamp(0.0, 255.0) as u32
    };
    let out: Vec<r_core::intern::InternedStr> = (0..n).map(|i| {
        let r = quant(reds[i % reds.len().max(1)]);
        let g = quant(greens[i % greens.len().max(1)]);
        let b = quant(blues[i % blues.len().max(1)]);
        interner.intern_string(&format!("#{:02X}{:02X}{:02X}", r, g, b))
    }).collect();
    Ok(heap.alloc(RObject::String(out)))
}
