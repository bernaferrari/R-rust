/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1997--2022  The R Core Team
 *  Copyright (C) 1995, 1996  Robert Gentleman and Ross Ihaka
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program; if not, a copy is available at
 *  https://www.R-project.org/Licenses/
 *
 *  Ported from r-source/src/library/graphics/src/par.c
 *
 *  GRZ-like state information.
 *  Provides the functionality of the "par" function in S.
 */

use super::layout::{LayoutState, Regions};
use std::collections::{BTreeMap, HashMap};
use std::ffi::c_void;
use std::os::raw::{c_char, c_double, c_int, c_uchar, c_uint, c_ushort};

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;
use crate::sexp::symbol::Rf_install;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ParValue {
    Logical(Vec<c_int>),
    Integer(Vec<c_int>),
    Real(Vec<c_double>),
    String(String),
}

/// Base-graphics parameter block used by `gpptr` / `dpptr`.
///
/// The layout matches the subset of R's `GPar` consumed by `plot.rs`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct GPar {
    pub adj: c_double,
    pub ann: c_int,
    pub bg: c_uint,
    pub bty: c_char,
    pub cex: c_double,
    pub lheight: c_double,
    pub col: c_uint,
    pub crt: c_double,
    pub din: [c_double; 2],
    pub err: c_int,
    pub fg: c_uint,
    pub family: [c_char; 201],
    pub font: c_int,
    pub gamma: c_double,
    pub lab: [c_int; 3],
    pub las: c_int,
    pub lty: c_int,
    pub lwd: c_double,
    pub mgp: [c_double; 3],
    pub mkh: c_double,
    pub pch: c_int,
    pub ps: c_double,
    pub smo: c_int,
    pub srt: c_double,
    pub tck: c_double,
    pub tcl: c_double,
    pub xaxp: [c_double; 3],
    pub xaxs: c_char,
    pub xaxt: c_char,
    pub xlog: c_int,
    pub xpd: c_int,
    pub oldxpd: c_int,
    pub yaxp: [c_double; 3],
    pub yaxs: c_char,
    pub yaxt: c_char,
    pub ylog: c_int,
    pub cexbase: c_double,
    pub cexmain: c_double,
    pub cexlab: c_double,
    pub cexsub: c_double,
    pub cexaxis: c_double,
    pub fontmain: c_int,
    pub fontlab: c_int,
    pub fontsub: c_int,
    pub fontaxis: c_int,
    pub colmain: c_uint,
    pub collab: c_uint,
    pub colsub: c_uint,
    pub colaxis: c_uint,
    pub mar: [c_double; 4],
    pub oma: [c_double; 4],
    pub pin: [c_double; 2],
    pub plt: [c_double; 4],
    pub fig: [c_double; 4],
    pub usr: [c_double; 4],
    pub logusr: [c_double; 4],
    pub new: c_int,
    pub state: c_int,
    pub valid: c_int,
}

#[derive(Clone)]
struct DeviceGParEntry {
    gp: GPar,
    dp: GPar,
    dp_saved: GPar,
}

#[derive(Default)]
pub(crate) struct GraphicsParState {
    overrides: BTreeMap<String, ParValue>,
    pub(crate) layout: LayoutState,
    /// GNU keeps `par(mfg=...)`'s implicit `new=TRUE` on an unused device
    /// until GNewPlot resets the first figure to cell 1.
    pending_mfg_before_first_plot: bool,
    base_register_index: Option<c_int>,
    device_gpars: HashMap<usize, DeviceGParEntry>,
    active_device: Option<c_int>,
    device_parameters: HashMap<c_int, DeviceParameters>,
}

#[derive(Default)]
struct DeviceParameters {
    overrides: BTreeMap<String, ParValue>,
    layout: LayoutState,
    pending_mfg_before_first_plot: bool,
}

impl GraphicsParState {
    fn select_device(&mut self, device: c_int) {
        if self.active_device == Some(device) {
            return;
        }
        let old = DeviceParameters {
            overrides: std::mem::take(&mut self.overrides),
            layout: std::mem::take(&mut self.layout),
            pending_mfg_before_first_plot: std::mem::take(&mut self.pending_mfg_before_first_plot),
        };
        if let Some(previous) = self.active_device {
            self.device_parameters.insert(previous, old);
        }
        let next = self.device_parameters.remove(&device).unwrap_or_default();
        self.overrides = next.overrides;
        self.layout = next.layout;
        self.pending_mfg_before_first_plot = next.pending_mfg_before_first_plot;
        self.active_device = Some(device);
    }
}

fn default_gpar() -> GPar {
    GPar {
        adj: 0.5,
        ann: 1,
        bg: 0xffffff,
        bty: b'o' as c_char,
        cex: 1.0,
        lheight: 1.0,
        col: 0x000000,
        crt: 0.0,
        din: [7.0, 7.0],
        err: -1,
        fg: 0x000000,
        family: [0; 201],
        font: 1,
        gamma: 1.0,
        lab: [5, 6, 4],
        las: 0,
        lty: 1,
        lwd: 1.0,
        mgp: [3.0, 1.0, 0.0],
        mkh: 0.25,
        pch: 1,
        ps: 12.0,
        smo: 1,
        srt: 0.0,
        tck: -1.0,
        tcl: -0.5,
        xaxp: [0.0, 1.0, 5.0],
        xaxs: b'r' as c_char,
        xaxt: b's' as c_char,
        xlog: 0,
        xpd: 0,
        oldxpd: 0,
        yaxp: [0.0, 1.0, 5.0],
        yaxs: b'r' as c_char,
        yaxt: b's' as c_char,
        ylog: 0,
        cexbase: 1.0,
        cexmain: 1.2,
        cexlab: 1.0,
        cexsub: 1.0,
        cexaxis: 1.0,
        fontmain: 2,
        fontlab: 1,
        fontsub: 1,
        fontaxis: 1,
        colmain: 0x000000,
        collab: 0x000000,
        colsub: 0x000000,
        colaxis: 0x000000,
        mar: [5.1, 4.1, 4.1, 2.1],
        oma: [0.0, 0.0, 0.0, 0.0],
        pin: [4.5, 4.5],
        plt: [0.1171429, 0.8838095, 0.1171429, 0.8838095],
        fig: [0.0, 1.0, 0.0, 1.0],
        usr: [0.0, 1.0, 0.0, 1.0],
        logusr: [0.0, 0.0, 0.0, 0.0],
        new: 1,
        state: 1,
        valid: 1,
    }
}

fn device_key(dd: pGEDevDesc) -> usize {
    dd as usize
}

fn with_device_gpar_entry<T>(
    dd: pGEDevDesc,
    f: impl FnOnce(&mut DeviceGParEntry) -> T,
) -> Option<T> {
    if dd.is_null() {
        return None;
    }
    Some(with_par_state(|state| {
        let entry = state.device_gpars.entry(device_key(dd)).or_insert_with(|| {
            let gp = default_gpar();
            DeviceGParEntry {
                dp: gp,
                dp_saved: gp,
                gp,
            }
        });
        f(entry)
    }))
}

const PAR_ORDER: &[&str] = &[
    "xlog",
    "ylog",
    "adj",
    "ann",
    "ask",
    "bg",
    "bty",
    "cex",
    "cex.axis",
    "cex.lab",
    "cex.main",
    "cex.sub",
    "cin",
    "col",
    "col.axis",
    "col.lab",
    "col.main",
    "col.sub",
    "cra",
    "crt",
    "csi",
    "cxy",
    "din",
    "err",
    "family",
    "fg",
    "fig",
    "fin",
    "font",
    "font.axis",
    "font.lab",
    "font.main",
    "font.sub",
    "lab",
    "las",
    "lend",
    "lheight",
    "ljoin",
    "lmitre",
    "lty",
    "lwd",
    "mai",
    "mar",
    "mex",
    "mfcol",
    "mfg",
    "mfrow",
    "mgp",
    "mkh",
    "new",
    "oma",
    "omd",
    "omi",
    "page",
    "pch",
    "pin",
    "plt",
    "ps",
    "pty",
    "smo",
    "srt",
    "tck",
    "tcl",
    "usr",
    "xaxp",
    "xaxs",
    "xaxt",
    "xpd",
    "yaxp",
    "yaxs",
    "yaxt",
    "ylbias",
];

const READONLY_PARS: &[&str] = &["cin", "cra", "csi", "cxy", "din", "page"];

fn default_par_value(name: &str) -> Option<ParValue> {
    let value = match name {
        "xlog" | "ylog" | "ask" | "new" | "xpd" => ParValue::Logical(vec![FALSE]),
        "ann" | "page" => ParValue::Logical(vec![TRUE]),
        "adj" => ParValue::Real(vec![0.5]),
        "bg" => ParValue::String("transparent".into()),
        "bty" => ParValue::String("o".into()),
        "cex" | "cex.axis" | "cex.lab" | "cex.sub" | "lheight" | "lwd" | "mex" | "smo" => {
            ParValue::Real(vec![1.0])
        }
        "cex.main" => ParValue::Real(vec![1.2]),
        // mai/mar/oma/omi/omd/fig/fin/plt/pin/din/csi/cin/cra/cxy/mfg/
        // mfrow/mfcol are owned by LayoutState (see layout.rs).
        "col" | "col.axis" | "col.lab" | "col.main" | "col.sub" | "fg" => {
            ParValue::String("black".into())
        }
        "crt" | "srt" => ParValue::Real(vec![0.0]),
        "err" | "las" => ParValue::Integer(vec![0]),
        "family" => ParValue::String(String::new()),
        "usr" => ParValue::Real(vec![0.0, 1.0, 0.0, 1.0]),
        "font" | "font.axis" | "font.lab" | "font.sub" | "pch" => ParValue::Integer(vec![1]),
        "font.main" => ParValue::Integer(vec![2]),
        "lab" => ParValue::Integer(vec![5, 5, 7]),
        "lend" | "ljoin" => ParValue::String("round".into()),
        "lmitre" => ParValue::Real(vec![10.0]),
        "lty" => ParValue::String("solid".into()),
        "mgp" => ParValue::Real(vec![3.0, 1.0, 0.0]),
        "mkh" => ParValue::Real(vec![0.001]),
        "ps" => ParValue::Integer(vec![12]),
        "pty" => ParValue::String("m".into()),
        "tck" => ParValue::Real(vec![NA_REAL]),
        "tcl" => ParValue::Real(vec![-0.5]),
        "xaxp" | "yaxp" => ParValue::Real(vec![0.0, 1.0, 5.0]),
        "xaxs" | "yaxs" => ParValue::String("r".into()),
        "xaxt" | "yaxt" => ParValue::String("s".into()),
        "ylbias" => ParValue::Real(vec![0.2]),
        _ => return None,
    };
    Some(value)
}

fn is_readonly_par(name: &str) -> bool {
    READONLY_PARS.contains(&name)
}

fn is_known_par(name: &str) -> bool {
    PAR_ORDER.contains(&name)
}

pub(crate) fn par_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

/* ---- ParTable: pure data, no graphics engine dependency ---- */

/// ParTab entry: maps a parameter name to a code.
/// code: 0 = normal, 1 = not inline, 2 = read-only,
///       -1 = unknown, -2 = obsolete, -3 = graphical args
#[derive(Clone, Copy)]
struct ParTab {
    name: &'static str,
    code: c_int,
}

/// The complete ParTable array from par.c.
/// This is pure data used by ParCode() to look up parameter codes.
static PAR_TABLE: &[ParTab] = &[
    ParTab {
        name: "adj",
        code: 0,
    },
    ParTab {
        name: "ann",
        code: 0,
    },
    ParTab {
        name: "ask",
        code: 1,
    },
    ParTab {
        name: "bg",
        code: 0,
    },
    ParTab {
        name: "bty",
        code: 0,
    },
    ParTab {
        name: "cex",
        code: 0,
    },
    ParTab {
        name: "cex.axis",
        code: 0,
    },
    ParTab {
        name: "cex.lab",
        code: 0,
    },
    ParTab {
        name: "cex.main",
        code: 0,
    },
    ParTab {
        name: "cex.sub",
        code: 0,
    },
    ParTab {
        name: "cin",
        code: 2,
    },
    ParTab {
        name: "col",
        code: 0,
    },
    ParTab {
        name: "col.axis",
        code: 0,
    },
    ParTab {
        name: "col.lab",
        code: 0,
    },
    ParTab {
        name: "col.main",
        code: 0,
    },
    ParTab {
        name: "col.sub",
        code: 0,
    },
    ParTab {
        name: "cra",
        code: 2,
    },
    ParTab {
        name: "crt",
        code: 0,
    },
    ParTab {
        name: "csi",
        code: 2,
    },
    ParTab {
        name: "csy",
        code: 0,
    },
    ParTab {
        name: "cxy",
        code: 2,
    },
    ParTab {
        name: "din",
        code: 2,
    },
    ParTab {
        name: "err",
        code: 0,
    },
    ParTab {
        name: "family",
        code: 0,
    },
    ParTab {
        name: "fg",
        code: 0,
    },
    ParTab {
        name: "fig",
        code: 1,
    },
    ParTab {
        name: "fin",
        code: 1,
    },
    ParTab {
        name: "font",
        code: 0,
    },
    ParTab {
        name: "font.axis",
        code: 0,
    },
    ParTab {
        name: "font.lab",
        code: 0,
    },
    ParTab {
        name: "font.main",
        code: 0,
    },
    ParTab {
        name: "font.sub",
        code: 0,
    },
    ParTab {
        name: "lab",
        code: 0,
    },
    ParTab {
        name: "las",
        code: 0,
    },
    ParTab {
        name: "lend",
        code: 0,
    },
    ParTab {
        name: "lheight",
        code: 1,
    },
    ParTab {
        name: "ljoin",
        code: 0,
    },
    ParTab {
        name: "lmitre",
        code: 0,
    },
    ParTab {
        name: "lty",
        code: 0,
    },
    ParTab {
        name: "lwd",
        code: 0,
    },
    ParTab {
        name: "mai",
        code: 1,
    },
    ParTab {
        name: "mar",
        code: 1,
    },
    ParTab {
        name: "mex",
        code: 1,
    },
    ParTab {
        name: "mfcol",
        code: 1,
    },
    ParTab {
        name: "mfg",
        code: 1,
    },
    ParTab {
        name: "mfrow",
        code: 1,
    },
    ParTab {
        name: "mgp",
        code: 0,
    },
    ParTab {
        name: "mkh",
        code: 0,
    },
    ParTab {
        name: "new",
        code: 1,
    },
    ParTab {
        name: "oma",
        code: 1,
    },
    ParTab {
        name: "omd",
        code: 1,
    },
    ParTab {
        name: "omi",
        code: 1,
    },
    ParTab {
        name: "page",
        code: 2,
    },
    ParTab {
        name: "pch",
        code: 0,
    },
    ParTab {
        name: "pin",
        code: 1,
    },
    ParTab {
        name: "plt",
        code: 1,
    },
    ParTab {
        name: "ps",
        code: 1,
    },
    ParTab {
        name: "pty",
        code: 1,
    },
    ParTab {
        name: "smo",
        code: 0,
    },
    ParTab {
        name: "srt",
        code: 0,
    },
    ParTab {
        name: "tck",
        code: 0,
    },
    ParTab {
        name: "tcl",
        code: 0,
    },
    ParTab {
        name: "usr",
        code: 1,
    },
    ParTab {
        name: "xaxp",
        code: 0,
    },
    ParTab {
        name: "xaxs",
        code: 0,
    },
    ParTab {
        name: "xaxt",
        code: 0,
    },
    ParTab {
        name: "xlog",
        code: 1,
    },
    ParTab {
        name: "xpd",
        code: 0,
    },
    ParTab {
        name: "yaxp",
        code: 0,
    },
    ParTab {
        name: "yaxs",
        code: 0,
    },
    ParTab {
        name: "yaxt",
        code: 0,
    },
    ParTab {
        name: "ylbias",
        code: 1,
    },
    ParTab {
        name: "ylog",
        code: 1,
    },
    /* Obsolete pars */
    ParTab {
        name: "gamma",
        code: -2,
    },
    ParTab {
        name: "type",
        code: -2,
    },
    ParTab {
        name: "tmag",
        code: -2,
    },
    /* Non-pars that might get passed to Specify2 */
    ParTab {
        name: "asp",
        code: -3,
    },
    ParTab {
        name: "main",
        code: -3,
    },
    ParTab {
        name: "sub",
        code: -3,
    },
    ParTab {
        name: "xlab",
        code: -3,
    },
    ParTab {
        name: "ylab",
        code: -3,
    },
    ParTab {
        name: "xlim",
        code: -3,
    },
    ParTab {
        name: "ylim",
        code: -3,
    },
];

/// pGEDevDesc is an opaque pointer to the graphics device descriptor.
/// The full type is defined in the Graphics Engine, which is not yet ported.
type pGEDevDesc = *mut c_void;

/// Look up a graphical parameter name in ParTable and return its code.
/// Returns -1 if not found.
///
/// This is the Rust equivalent of `static int ParCode(const char *what)`.
pub fn ParCode(what: &str) -> c_int {
    PAR_TABLE
        .iter()
        .find(|entry| entry.name == what)
        .map_or(-1, |entry| entry.code)
}

/// Specify -- set a graphical parameter via par().
fn Specify(name: &str, value: SEXP, _dd: pGEDevDesc) {
    {
        if !is_known_par(&name) {
            par_error(format!(
                "invalid value specified for graphical parameter \"{name}\""
            ));
        }
        if is_readonly_par(&name) {
            par_error(format!("graphical parameter \"{name}\" is read-only"));
        }
        let value = sexp_to_par_value(value);
        let frame_started = graphics_frame_started();
        let regions_changed =
            with_par_state(|state| apply_parameter(state, &name, &value, frame_started));
        if regions_changed {
            #[cfg(feature = "renderplot-device")]
            crate::mainutils::portable_plot::refresh_frame_regions();
        }
    }
}

/// Specify2 -- set a graphical parameter from a high-level graphics function.
fn Specify2(name: &str, value: SEXP, dd: pGEDevDesc) {
    if is_known_par(name) && !is_readonly_par(name) {
        Specify(name, value, dd);
    }
}

/// Query -- return the current value of a graphical parameter.
fn Query(name: &str, _dd: pGEDevDesc) -> SEXP {
    {
        if !is_known_par(&name) {
            par_error(format!(
                "invalid value specified for graphical parameter \"{name}\""
            ));
        }
        let value = parameter(&name);
        par_value_to_sexp(&value)
    }
}

pub(crate) fn parameter(name: &str) -> ParValue {
    if name == "ask" {
        return ParValue::Logical(vec![
            crate::library::grdevices::device_registry::page_question(None) as c_int,
        ]);
    }
    with_par_state(|state| {
        let cexbase = real_override(state, "cex", 1.0);
        let mex = real_override(state, "mex", 1.0);
        state
            .layout
            .refresh_if_device_geometry_changed(cexbase, mex, 1.0);
        match layout_par_value(state, name) {
            Some(value) => value,
            None => current_par_value(state, name),
        }
    })
}

/// Queries owned by the device-layout state (see layout.rs).
fn layout_par_value(state: &GraphicsParState, name: &str) -> Option<ParValue> {
    match name {
        "cxy" => {
            let usr = match state.overrides.get("usr") {
                Some(ParValue::Real(values)) if values.len() == 4 => {
                    [values[0], values[1], values[2], values[3]]
                }
                _ => [0.0, 1.0, 0.0, 1.0],
            };
            Some(state.layout.query_cxy(usr))
        }
        "page" => {
            // Mimics the GNewPlot decision logic, like GNU's par.c.
            let new = logical_override(state, "new");
            let starts_page = if new {
                !graphics_frame_started()
            } else {
                state.layout.starts_new_page()
            };
            Some(ParValue::Logical(vec![i32::from(starts_page)]))
        }
        _ => state.layout.query(name),
    }
}

fn logical_override(state: &GraphicsParState, name: &str) -> bool {
    matches!(
        state.overrides.get(name),
        Some(ParValue::Logical(values)) if values.first().is_some_and(|value| *value != 0)
    )
}

fn real_override(state: &GraphicsParState, name: &str, fallback: f64) -> f64 {
    match state.overrides.get(name) {
        Some(ParValue::Real(values)) if values.len() == 1 && values[0].is_finite() => values[0],
        Some(ParValue::Integer(values)) if values.len() == 1 && values[0] != NA_INTEGER => {
            f64::from(values[0])
        }
        _ => fallback,
    }
}

pub(crate) fn set_plot_parameter(name: &str, value: ParValue) {
    if name == "ask" {
        let ask = match value {
            ParValue::Logical(values) | ParValue::Integer(values) => {
                if values.len() != 1 {
                    par_error("graphical parameter \"ask\" has the wrong length");
                }
                values[0] != NA_INTEGER && values[0] != 0
            }
            ParValue::Real(values) => {
                if values.len() != 1 {
                    par_error("graphical parameter \"ask\" has the wrong length");
                }
                !values[0].is_nan() && values[0] != 0.0
            }
            ParValue::String(value) => matches!(value.as_str(), "TRUE" | "True" | "true" | "T"),
        };
        crate::library::grdevices::device_registry::page_question(Some(ask));
        return;
    }
    let frame_started = graphics_frame_started();
    let regions_changed =
        with_par_state(|state| apply_parameter(state, name, &value, frame_started));
    if regions_changed {
        #[cfg(feature = "renderplot-device")]
        crate::mainutils::portable_plot::refresh_frame_regions();
    }
}

/// Specify(): route layout-coupled parameters through the GNU-faithful
/// layout state (validation + coupled recomputation); everything else
/// lands in the plain override map. Returns true when the device
/// regions moved.
fn apply_parameter(
    state: &mut GraphicsParState,
    name: &str,
    value: &ParValue,
    frame_started: bool,
) -> bool {
    let cexbase = real_override(state, "cex", 1.0);
    let mex = real_override(state, "mex", 1.0);
    match name {
        "mai" => {
            state.layout.set_mai(value, cexbase, mex, 1.0);
            true
        }
        "mar" => {
            state.layout.set_mar(value, cexbase, mex, 1.0);
            true
        }
        "mex" => {
            let mex = state.layout.check_mex(value);
            state
                .overrides
                .insert("mex".into(), ParValue::Real(vec![mex]));
            state.layout.refresh(cexbase, mex, 1.0);
            true
        }
        "oma" => {
            state.layout.set_oma(value, cexbase, mex, 1.0);
            true
        }
        "omi" => {
            state.layout.set_omi(value, cexbase, mex, 1.0);
            true
        }
        "omd" => {
            state.layout.set_omd(value, cexbase, mex, 1.0);
            true
        }
        "fig" => {
            state.layout.set_fig(value, cexbase, mex, 1.0);
            true
        }
        "fin" => {
            state.layout.set_fin(value, cexbase, mex, 1.0);
            true
        }
        "pin" => {
            state.layout.set_pin(value, cexbase, mex, 1.0);
            true
        }
        "plt" => {
            state.layout.set_plt(value, cexbase, mex, 1.0);
            true
        }
        "pty" => {
            if let ParValue::String(text) = value {
                state.layout.set_pty(text);
            } else {
                par_error("invalid value specified for graphical parameter \"pty\"");
            }
            false
        }
        "mfrow" | "mfcol" => {
            let (nrow, ncol) = state.layout.set_mfrow(value, name == "mfcol");
            let cex = LayoutState::split_cex(nrow, ncol);
            state
                .overrides
                .insert("cex".into(), ParValue::Real(vec![cex]));
            state
                .overrides
                .insert("mex".into(), ParValue::Real(vec![1.0]));
            true
        }
        "mfg" => {
            for warning in state.layout.set_mfg(value, cexbase, mex, 1.0) {
                let message = std::ffi::CString::new(warning).unwrap_or_default();

                crate::mainutils::errors::warningcall_cstr(
                    crate::sexp::globals::R_NilValue(),
                    &message,
                );
            }
            state
                .overrides
                .insert("new".into(), ParValue::Logical(vec![TRUE]));
            state.pending_mfg_before_first_plot = !frame_started;
            true
        }
        "new"
            if !frame_started
                && state.pending_mfg_before_first_plot
                && logical_false_value(value) =>
        {
            // GNU keeps the implicit `new=TRUE` established by par(mfg=)
            // on an unused device; the first GNewPlot still resets to cell 1.
            state
                .overrides
                .insert("new".into(), ParValue::Logical(vec![TRUE]));
            false
        }
        "ps" => {
            let ps = match value {
                ParValue::Integer(values) | ParValue::Logical(values) => {
                    if values.len() != 1 {
                        par_error("graphical parameter \"ps\" has the wrong length");
                    }
                    values[0]
                }
                ParValue::Real(values) => {
                    if values.len() != 1 {
                        par_error("graphical parameter \"ps\" has the wrong length");
                    }
                    values[0] as c_int
                }
                _ => par_error("invalid value specified for graphical parameter \"ps\""),
            };
            if ps == NA_INTEGER || ps < 0 {
                par_error("invalid value specified for graphical parameter \"ps\"");
            }
            state
                .overrides
                .insert("ps".into(), ParValue::Integer(vec![ps]));
            false
        }
        _ => {
            state.overrides.insert(name.to_owned(), value.clone());
            false
        }
    }
}

/// The text a `CString::new(text).unwrap_or_default()` round trip keeps:
/// strings with an embedded NUL become empty.
fn nul_free_or_empty(text: &str) -> &str {
    if text.contains('\0') { "" } else { text }
}

fn with_par_state<T>(f: impl FnOnce(&mut GraphicsParState) -> T) -> T {
    // Capture the device without opening one or holding a registry borrow while
    // accessing parameters. Device labels are unique within the owning session.
    let device = crate::library::grdevices::device_registry::current_device_label();
    crate::sexp::instance::with_required_current_instance(|instance| unsafe {
        let state = &mut (*instance).graphics_par_state;
        state.select_device(device);
        f(state)
    })
}

/// Drop closed-device state before its descriptor can be reused. A headless
/// render can retain its final parameters on the null device for host queries.
pub(crate) fn forget_device_parameters(device: c_int, descriptor: usize, retain_on_null: bool) {
    crate::sexp::instance::with_required_current_instance(|instance| unsafe {
        let state = &mut (*instance).graphics_par_state;
        let removed = if state.active_device == Some(device) {
            state.active_device = None;
            Some(DeviceParameters {
                overrides: std::mem::take(&mut state.overrides),
                layout: std::mem::take(&mut state.layout),
                pending_mfg_before_first_plot: std::mem::take(
                    &mut state.pending_mfg_before_first_plot,
                ),
            })
        } else {
            state.device_parameters.remove(&device)
        };
        state.device_gpars.remove(&descriptor);
        if retain_on_null {
            if let Some(parameters) = removed {
                if state.active_device == Some(1) {
                    state.overrides = parameters.overrides;
                    state.layout = parameters.layout;
                    state.pending_mfg_before_first_plot = parameters.pending_mfg_before_first_plot;
                } else {
                    state.device_parameters.insert(1, parameters);
                }
            }
        }
    });
}

/// Reset registry-owned parameter state when test code recreates the registry.
pub(crate) fn reset_device_parameters() {
    crate::sexp::instance::with_required_current_instance(|instance| unsafe {
        let state = &mut (*instance).graphics_par_state;
        state.overrides.clear();
        state.layout = LayoutState::default();
        state.pending_mfg_before_first_plot = false;
        state.active_device = None;
        state.device_parameters.clear();
        state.device_gpars.clear();
    });
}

/// A staged `plot.new` layout transition. The candidate is committed only
/// after its derived regions pass the GNewPlot validity checks.
pub(crate) struct PlotFigureTransition {
    layout: Option<LayoutState>,
    pending_mfg_before_first_plot: bool,
}

/// Prepare the layout and regions for plot.new without changing live par
/// state. A failed region check must not consume an mfrow/mfcol cell or page.
pub(crate) fn prepare_plot_figure(
    cexbase: f64,
    mex: f64,
    font_scale: f64,
) -> (Regions, bool, PlotFigureTransition) {
    let frame_started = graphics_frame_started();
    with_par_state(|state| {
        if logical_override(state, "new") && frame_started {
            return (
                state.layout.regions(),
                false,
                PlotFigureTransition {
                    layout: None,
                    pending_mfg_before_first_plot: state.pending_mfg_before_first_plot,
                },
            );
        }
        let mut layout = state.layout.clone();
        let mut pending_mfg_before_first_plot = state.pending_mfg_before_first_plot;
        let clear = if !frame_started {
            pending_mfg_before_first_plot = false;
            layout.begin_first_figure(cexbase, mex, font_scale)
        } else {
            layout.begin_figure(cexbase, mex, font_scale)
        };
        let regions = layout.regions();
        (
            regions,
            clear,
            PlotFigureTransition {
                layout: Some(layout),
                pending_mfg_before_first_plot,
            },
        )
    })
}

/// Commit a prepared plot.new transition after the candidate regions validate.
pub(crate) fn commit_plot_figure(transition: PlotFigureTransition) {
    with_par_state(|state| {
        if let Some(layout) = transition.layout {
            state.layout = layout;
            state.pending_mfg_before_first_plot = transition.pending_mfg_before_first_plot;
        }
    });
}

fn graphics_frame_started() -> bool {
    #[cfg(feature = "renderplot-device")]
    if let Some(started) = crate::mainutils::portable_plot::renderplot_frame_started() {
        return started;
    }
    super::graphics::plot_new_called()
}

fn logical_false_value(value: &ParValue) -> bool {
    match value {
        ParValue::Logical(values) | ParValue::Integer(values) => {
            values.first().is_some_and(|value| *value == 0)
        }
        ParValue::Real(values) => values.first().is_some_and(|value| *value == 0.0),
        _ => false,
    }
}

/// Detached conversion inputs captured without calling into the device while
/// the live graphics-parameter state is borrowed.
#[cfg(feature = "renderplot-device")]
#[derive(PartialEq)]
pub(crate) struct ConversionLayoutSnapshot {
    pub regions: Regions,
    pub csi: f64,
    pub mex: f64,
    pub cex: f64,
    pub oma: [f64; 4],
}

#[cfg(feature = "renderplot-device")]
pub(crate) fn conversion_layout_snapshot(width: f64, height: f64) -> ConversionLayoutSnapshot {
    with_par_state(|state| {
        let cex = real_override(state, "cex", 1.0);
        let mex = real_override(state, "mex", 1.0);
        let mut layout = state.layout.clone();
        layout.refresh_to_size(width, height, cex, mex, 1.0);
        let (csi, oma) = layout.conversion_yardsticks();
        ConversionLayoutSnapshot {
            regions: layout.regions(),
            csi,
            mex,
            cex,
            oma,
        }
    })
}

/// Current device/inner/figure/plot regions in raster units.
pub(crate) fn current_regions() -> Regions {
    with_par_state(|state| state.layout.regions())
}

fn graphics_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

fn layout_pop_arg(args: &mut SEXP) -> SEXP {
    {
        if args.is_null() || *args == R_NilValue() {
            graphics_error("invalid graphics layout");
        }
        let value = CAR(*args);
        *args = CDR(*args);
        value
    }
}

fn layout_int_scalar(args: &mut SEXP, name: &str) -> c_int {
    {
        let value = layout_pop_arg(args);
        if value.is_null() || value == R_NilValue() || XLENGTH(value) < 1 {
            graphics_error(format!("invalid '{name}' in graphics layout"));
        }
        match TYPEOF(value) {
            t if t == SEXPTYPE::INTSXP => INTEGER_ELT(value, 0),
            t if t == SEXPTYPE::REALSXP => REAL_ELT(value, 0) as c_int,
            t if t == SEXPTYPE::LGLSXP => LOGICAL_ELT(value, 0),
            _ => graphics_error(format!("invalid '{name}' in graphics layout")),
        }
    }
}

fn layout_int_values(args: &mut SEXP, name: &str, min_len: usize) -> Vec<c_int> {
    {
        let value = layout_pop_arg(args);
        if value.is_null() || value == R_NilValue() {
            if min_len == 0 {
                return Vec::new();
            }
            graphics_error(format!("invalid '{name}' in graphics layout"));
        }
        if XLENGTH(value) < min_len as R_xlen_t {
            graphics_error(format!("invalid '{name}' in graphics layout"));
        }
        let n = XLENGTH(value) as usize;
        match TYPEOF(value) {
            t if t == SEXPTYPE::INTSXP => (0..n).map(|i| INTEGER_ELT(value, i as c_int)).collect(),
            t if t == SEXPTYPE::REALSXP => (0..n)
                .map(|i| REAL_ELT(value, i as c_int) as c_int)
                .collect(),
            t if t == SEXPTYPE::LGLSXP => (0..n).map(|i| LOGICAL_ELT(value, i as c_int)).collect(),
            _ => graphics_error(format!("invalid '{name}' in graphics layout")),
        }
    }
}

fn layout_real_values(args: &mut SEXP, name: &str, min_len: usize) -> Vec<c_double> {
    {
        let value = layout_pop_arg(args);
        if value.is_null() || value == R_NilValue() || XLENGTH(value) < min_len as R_xlen_t {
            graphics_error(format!("invalid '{name}' in graphics layout"));
        }
        let n = XLENGTH(value) as usize;
        match TYPEOF(value) {
            t if t == SEXPTYPE::REALSXP => (0..n).map(|i| REAL_ELT(value, i as c_int)).collect(),
            t if t == SEXPTYPE::INTSXP => (0..n)
                .map(|i| INTEGER_ELT(value, i as c_int) as c_double)
                .collect(),
            _ => graphics_error(format!("invalid '{name}' in graphics layout")),
        }
    }
}

fn apply_layout_state(nrow: c_int, ncol: c_int, order: &[c_int], num_figures: c_int) {
    let cex = LayoutState::split_cex(nrow, ncol);
    let regions_moved = with_par_state(|state| {
        state
            .layout
            .set_layout(nrow, ncol, order, num_figures, cex, 1.0, 1.0);
        state
            .overrides
            .insert("cex".into(), ParValue::Real(vec![cex]));
        state
            .overrides
            .insert("mex".into(), ParValue::Real(vec![1.0]));
        state
            .overrides
            .insert("new".into(), ParValue::Logical(vec![FALSE]));
        true
    });
    if regions_moved {
        #[cfg(feature = "renderplot-device")]
        crate::mainutils::portable_plot::refresh_frame_regions();
    }
}

fn matrix_shape(value: SEXP) -> Option<(c_int, c_int)> {
    {
        let dim =
            crate::sexp::attrib_core::getAttrib(value, crate::sexp::attrib_core::R_DimSymbol());
        if dim.is_null()
            || dim == R_NilValue()
            || TYPEOF(dim) != SEXPTYPE::INTSXP
            || XLENGTH(dim) < 2
        {
            return None;
        }
        Some((INTEGER_ELT(dim, 0), INTEGER_ELT(dim, 1)))
    }
}

fn layout_matrix_order(value: SEXP, len: usize) -> Vec<c_int> {
    {
        if value.is_null() || value == R_NilValue() || XLENGTH(value) < len as R_xlen_t {
            graphics_error("invalid layout matrix");
        }
        match TYPEOF(value) {
            t if t == SEXPTYPE::INTSXP => {
                (0..len).map(|i| INTEGER_ELT(value, i as c_int)).collect()
            }
            t if t == SEXPTYPE::REALSXP => (0..len)
                .map(|i| REAL_ELT(value, i as c_int) as c_int)
                .collect(),
            _ => graphics_error("invalid layout matrix"),
        }
    }
}

fn validate_layout_order(order: &[c_int], num_figures: c_int) {
    for figure in 1..=num_figures {
        if !order.contains(&figure) {
            graphics_error(format!(
                "layout matrix must contain at least one reference\nto each of the values {{1 ... {num_figures}}}\n"
            ));
        }
    }
}

fn current_par_value(state: &GraphicsParState, name: &str) -> ParValue {
    state
        .overrides
        .get(name)
        .cloned()
        .or_else(|| default_par_value(name))
        .unwrap_or_else(|| {
            par_error(format!(
                "invalid value specified for graphical parameter \"{name}\""
            ))
        })
}

fn par_value_to_sexp(value: &ParValue) -> SEXP {
    {
        match value {
            ParValue::Logical(values) => {
                let result = Rf_allocVector3(SEXPTYPE::LGLSXP, values.len() as R_xlen_t);
                if result.is_null() {
                    return R_NilValue();
                }
                for (i, value) in values.iter().enumerate() {
                    SET_LOGICAL_ELT(result, i as c_int, *value);
                }
                result
            }
            ParValue::Integer(values) => {
                let result = Rf_allocVector3(SEXPTYPE::INTSXP, values.len() as R_xlen_t);
                if result.is_null() {
                    return R_NilValue();
                }
                for (i, value) in values.iter().enumerate() {
                    SET_INTEGER_ELT(result, i as c_int, *value);
                }
                result
            }
            ParValue::Real(values) => {
                let result = Rf_allocVector3(SEXPTYPE::REALSXP, values.len() as R_xlen_t);
                if result.is_null() {
                    return R_NilValue();
                }
                for (i, value) in values.iter().enumerate() {
                    SET_REAL_ELT(result, i as c_int, *value);
                }
                result
            }
            ParValue::String(value) => {
                crate::sexp::constructors::mk_string_str(nul_free_or_empty(value))
            }
        }
    }
}

fn mixed_par_list(slots: &[(String, bool)]) -> SEXP {
    {
        let n = slots.len() as R_xlen_t;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let name_vec = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if name_vec.is_null() {
            return R_NilValue();
        }
        let _name_guard = protect(name_vec);
        for (i, (name, known)) in slots.iter().enumerate() {
            let value = if *known {
                let value = parameter(name);
                par_value_to_sexp(&value)
            } else {
                R_NilValue()
            };
            SET_VECTOR_ELT(result, i as R_xlen_t, value);
            SET_STRING_ELT(
                name_vec,
                i as R_xlen_t,
                crate::sexp::constructors::mk_char_str(nul_free_or_empty(name)),
            );
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            name_vec,
        );
        result
    }
}

fn null_named_list(names: &[String]) -> SEXP {
    {
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, names.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let name_vec = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as R_xlen_t);
        if name_vec.is_null() {
            return R_NilValue();
        }
        let _name_guard = protect(name_vec);
        for (i, name) in names.iter().enumerate() {
            SET_VECTOR_ELT(result, i as R_xlen_t, R_NilValue());
            SET_STRING_ELT(
                name_vec,
                i as R_xlen_t,
                crate::sexp::constructors::mk_char_str(nul_free_or_empty(name)),
            );
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            name_vec,
        );
        result
    }
}

fn named_par_list(names: &[String]) -> SEXP {
    let values: Vec<_> = names.iter().map(|name| parameter(name)).collect();
    {
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, names.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let name_vec = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as R_xlen_t);
        if name_vec.is_null() {
            return R_NilValue();
        }
        let _name_guard = protect(name_vec);

        for (i, name) in names.iter().enumerate() {
            let value = &values[i];
            SET_VECTOR_ELT(result, i as R_xlen_t, par_value_to_sexp(&value));
            SET_STRING_ELT(
                name_vec,
                i as R_xlen_t,
                crate::sexp::constructors::mk_char_str(nul_free_or_empty(name)),
            );
        }

        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            name_vec,
        );
        result
    }
}

fn sexp_to_par_value(value: SEXP) -> ParValue {
    {
        match TYPEOF(value) {
            t if t == SEXPTYPE::LGLSXP => {
                let n = XLENGTH(value);
                let mut out = Vec::with_capacity(n as usize);
                for i in 0..n {
                    out.push(LOGICAL_ELT(value, i as c_int));
                }
                ParValue::Logical(out)
            }
            t if t == SEXPTYPE::INTSXP => {
                let n = XLENGTH(value);
                let mut out = Vec::with_capacity(n as usize);
                for i in 0..n {
                    out.push(INTEGER_ELT(value, i as c_int));
                }
                ParValue::Integer(out)
            }
            t if t == SEXPTYPE::REALSXP => {
                let n = XLENGTH(value);
                let mut out = Vec::with_capacity(n as usize);
                for i in 0..n {
                    out.push(REAL_ELT(value, i as c_int));
                }
                ParValue::Real(out)
            }
            t if t == SEXPTYPE::STRSXP && XLENGTH(value) == 1 => {
                let elt = STRING_ELT(value, 0);
                if elt == R_NaString() {
                    ParValue::String(String::new())
                } else {
                    let text = String::from_utf8_lossy(&charsxp_as_utf8(elt)).into_owned();
                    ParValue::String(text)
                }
            }
            _ => par_error("invalid value specified for graphical parameter"),
        }
    }
}

fn string_vector_values(value: SEXP) -> Vec<String> {
    {
        if TYPEOF(value) != SEXPTYPE::STRSXP {
            par_error("invalid argument passed to par()");
        }
        let n = XLENGTH(value);
        let mut out = Vec::with_capacity(n as usize);
        for i in 0..n {
            let elt = STRING_ELT(value, i);
            if elt == R_NaString() {
                par_error("invalid argument passed to par()");
            }
            out.push(String::from_utf8_lossy(&charsxp_as_utf8(elt)).into_owned());
        }
        out
    }
}

fn tag_name(tag: SEXP) -> Option<String> {
    {
        if tag.is_null() || tag == R_NilValue() {
            None
        } else {
            Some(String::from_utf8_lossy(&charsxp_as_utf8(PRINTNAME(tag))).into_owned())
        }
    }
}

fn logical_scalar(value: SEXP) -> Option<bool> {
    {
        if TYPEOF(value) != SEXPTYPE::LGLSXP || XLENGTH(value) < 1 {
            return None;
        }
        match LOGICAL_ELT(value, 0) {
            TRUE => Some(true),
            FALSE => Some(false),
            _ => None,
        }
    }
}

fn all_query_names(no_readonly: bool) -> Vec<String> {
    PAR_ORDER
        .iter()
        .filter(|name| !no_readonly || !is_readonly_par(name))
        .map(|name| (*name).to_string())
        .collect()
}

/// Rust-shaped `par()` implementation backed by per-session defaults and
/// overrides. It intentionally covers the query/update surface used by base
/// plotting and Android embedding without relying on process-global GE state.
pub fn do_par(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        let mut current = args;
        let mut query_names = Vec::new();
        let mut set_names = Vec::new();
        let mut set_values = Vec::new();
        let mut warned_names = Vec::new();
        let mut list_slots: Option<Vec<(String, bool)>> = None;
        let mut no_readonly = false;
        let mut arg_n = 0_i32;

        while !current.is_null() && current != R_NilValue() {
            arg_n += 1;
            let value = CAR(current);
            match tag_name(TAG(current)).as_deref() {
                Some("no.readonly") => {
                    no_readonly = logical_scalar(value).unwrap_or(false);
                }
                Some(name) => {
                    if !is_known_par(name) {
                        par_error(format!(
                            "invalid value specified for graphical parameter \"{name}\""
                        ));
                    }
                    if is_readonly_par(name) {
                        par_error(format!("graphical parameter \"{name}\" is read-only"));
                    }
                    set_names.push(name.to_string());
                    set_values.push(sexp_to_par_value(value));
                }
                None => {
                    if TYPEOF(value) == SEXPTYPE::STRSXP && XLENGTH(value) > 0 {
                        for name in string_vector_values(value) {
                            if !is_known_par(&name) {
                                par_error(format!(
                                    "invalid value specified for graphical parameter \"{name}\""
                                ));
                            }
                            query_names.push(name);
                        }
                    } else if arg_n == 1
                        && CDR(current) == R_NilValue()
                        && TYPEOF(value) == SEXPTYPE::VECSXP
                    {
                        let mut value = value;
                        let n0 = XLENGTH(value);
                        if n0 == 1 {
                            let inner = VECTOR_ELT(value, 0);
                            if TYPEOF(inner) == SEXPTYPE::VECSXP || inner == R_NilValue() {
                                value = inner;
                            }
                        }
                        let names = crate::sexp::attrib_core::getAttrib(
                            value,
                            crate::sexp::attrib_core::R_NamesSymbol(),
                        );
                        let n = XLENGTH(value);
                        let names_ok = TYPEOF(names) == SEXPTYPE::STRSXP && XLENGTH(names) == n;
                        if names_ok {
                            for i in 0..n {
                                let elt = STRING_ELT(names, i);
                                let name =
                                    String::from_utf8_lossy(&charsxp_as_utf8(elt)).into_owned();
                                if name.is_empty() {
                                    let inner = VECTOR_ELT(value, i);
                                    if TYPEOF(inner) == SEXPTYPE::STRSXP && XLENGTH(inner) > 0 {
                                        let ch = STRING_ELT(inner, 0);
                                        let qname = String::from_utf8_lossy(&charsxp_as_utf8(ch))
                                            .into_owned();
                                        if is_known_par(&qname) {
                                            list_slots
                                                .get_or_insert_with(Vec::new)
                                                .push((qname, true));
                                        } else {
                                            let msg = std::ffi::CString::new(format!(
                                                "\"{qname}\" is not a graphical parameter"
                                            ))
                                            .unwrap_or_default();
                                            crate::mainutils::errors::warningcall_cstr(call, &msg);
                                            list_slots
                                                .get_or_insert_with(Vec::new)
                                                .push((qname, false));
                                        }
                                    } else {
                                        let msg = std::ffi::CString::new(format!(
                                            "argument {} does not name a graphical parameter",
                                            i + 1
                                        ))
                                        .unwrap_or_default();
                                        crate::mainutils::errors::warningcall_cstr(call, &msg);
                                        list_slots
                                            .get_or_insert_with(Vec::new)
                                            .push((String::new(), false));
                                    }
                                    continue;
                                }
                                if !is_known_par(&name) {
                                    let msg = std::ffi::CString::new(format!(
                                        "\"{name}\" is not a graphical parameter"
                                    ))
                                    .unwrap_or_default();
                                    crate::mainutils::errors::warningcall_cstr(call, &msg);
                                    list_slots.get_or_insert_with(Vec::new).push((name, false));
                                    continue;
                                }
                                if is_readonly_par(&name) {
                                    let msg = std::ffi::CString::new(format!(
                                        "graphical parameter \"{name}\" cannot be set"
                                    ))
                                    .unwrap_or_default();
                                    crate::mainutils::errors::warningcall_cstr(call, &msg);
                                    list_slots.get_or_insert_with(Vec::new).push((name, true));
                                    continue;
                                }
                                list_slots
                                    .get_or_insert_with(Vec::new)
                                    .push((name.clone(), true));
                                let raw = VECTOR_ELT(value, i);
                                if name == "usr" && XLENGTH(raw) != 4 {
                                    par_error(
                                        "graphical parameter \"usr\" has the wrong length"
                                            .to_string(),
                                    );
                                }
                                set_names.push(name);
                                set_values.push(sexp_to_par_value(raw));
                            }
                        } else if n > 0
                            && (0..n).all(|i| TYPEOF(VECTOR_ELT(value, i)) == SEXPTYPE::STRSXP)
                        {
                            for i in 0..n {
                                let elt = VECTOR_ELT(value, i);
                                for j in 0..XLENGTH(elt) {
                                    let ch = STRING_ELT(elt, j);
                                    let name =
                                        String::from_utf8_lossy(&charsxp_as_utf8(ch)).into_owned();
                                    if is_known_par(&name) {
                                        query_names.push(name);
                                    }
                                }
                            }
                        } else if value != crate::sexp::globals::R_NilValue() {
                            let msg = std::ffi::CString::new(
                                "argument 1 does not name a graphical parameter",
                            )
                            .unwrap_or_default();
                            crate::mainutils::errors::warningcall_cstr(call, &msg);
                        }
                    } else {
                        let msg = std::ffi::CString::new(format!(
                            "argument {arg_n} does not name a graphical parameter"
                        ))
                        .unwrap_or_default();
                        crate::mainutils::errors::warningcall_cstr(call, &msg);
                    }
                }
            }
            current = CDR(current);
        }

        if let Some(slots) = list_slots.as_ref() {
            let result = mixed_par_list(slots);
            if !set_names.is_empty() {
                let _kept = protect(result);
                for (name, value) in set_names.iter().zip(set_values) {
                    set_plot_parameter(name, value);
                }
                crate::sexp::globals::set_R_Visible(FALSE);
            }
            return result;
        }
        if set_names.is_empty() && query_names.is_empty() {
            if !warned_names.is_empty() {
                return null_named_list(&warned_names);
            }
            let names = all_query_names(no_readonly);
            return named_par_list(&names);
        }

        let result = if !set_names.is_empty() {
            let old = named_par_list(&set_names);
            let _old = protect(old);
            for (name, value) in set_names.iter().zip(set_values) {
                set_plot_parameter(name, value);
            }
            old
        } else {
            // Graphics' R wrapper does `value[[1L]]` for one unnamed query.
            // Returning the list here keeps that `[[` on the whole parameter
            // (lab is c(5,5,7), not its first element). The base builtin
            // unwraps itself.
            named_par_list(&query_names)
        };

        if !set_names.is_empty() {
            crate::sexp::globals::set_R_Visible(FALSE);
        }
        result
    }
}

/// C_par -- implementation of R's par() function.
/// This is the .Internal(par(...)) entry point.
///
/// Original C signature:
///   SEXP C_par(SEXP call, SEXP op, SEXP args, SEXP rho)
pub fn C_par(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    do_par(call, op, args, rho)
}

/* ---- C_layout (the layout() .Internal) ---- */

/// C_layout -- implementation of R's layout() function.
/// This is the .Internal(layout(...)) entry point.
///
/// Original C signature:
///   SEXP C_layout(SEXP args)
pub fn C_layout(_args: SEXP) -> SEXP {
    {
        const MAX_LAYOUT_ROWS: c_int = 200;
        const MAX_LAYOUT_COLS: c_int = 200;
        const MAX_LAYOUT_CELLS: c_int = 10007;

        let mut args = CDR(_args);
        let nrow = layout_int_scalar(&mut args, "num.rows");
        if nrow > MAX_LAYOUT_ROWS {
            graphics_error(format!("too many rows in layout, limit {MAX_LAYOUT_ROWS}"));
        }
        let ncol = layout_int_scalar(&mut args, "num.cols");
        if ncol > MAX_LAYOUT_COLS {
            graphics_error(format!(
                "too many columns in layout, limit {MAX_LAYOUT_COLS}"
            ));
        }
        if nrow.saturating_mul(ncol) > MAX_LAYOUT_CELLS {
            graphics_error(format!(
                "too many cells in layout, limit {MAX_LAYOUT_CELLS}"
            ));
        }
        let cell_count = nrow.saturating_mul(ncol).max(0) as usize;
        let order = layout_int_values(&mut args, "mat", cell_count);
        let num_figures = layout_int_scalar(&mut args, "num.figures");
        let _widths = layout_real_values(&mut args, "col.widths", ncol.max(0) as usize);
        let _heights = layout_real_values(&mut args, "row.heights", nrow.max(0) as usize);
        let _cm_widths = layout_int_values(&mut args, "cm.widths", 0);
        let _cm_heights = layout_int_values(&mut args, "cm.heights", 0);
        let _respect = layout_int_scalar(&mut args, "respect");
        let _respect_mat = layout_int_values(&mut args, "respect.mat", cell_count);

        validate_layout_order(&order, num_figures);
        apply_layout_state(nrow, ncol, &order, num_figures);

        R_NilValue()
    }
}

/// Rust-level `layout()` entry point used by the headless Android runtime.
/// It mirrors the observable state changes from graphics' R wrapper and
/// `C_layout`; drawing backends consume the same `par()` state later.
pub fn do_layout(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    {
        if args.is_null() || args == R_NilValue() {
            graphics_error("argument 'mat' is missing, with no default");
        }
        let mat = CAR(args);
        let Some((nrow, ncol)) = matrix_shape(mat) else {
            graphics_error("'mat' must be a matrix");
        };
        let cell_count = nrow.saturating_mul(ncol).max(0) as usize;
        let order = layout_matrix_order(mat, cell_count);
        let num_figures = order.iter().copied().max().unwrap_or(0);
        validate_layout_order(&order, num_figures);
        apply_layout_state(nrow, ncol, &order, num_figures);

        let result = Rf_ScalarInteger(num_figures);
        crate::sexp::globals::set_R_Visible(FALSE);
        result
    }
}

/* ---- Stub: ProcessInlinePars ---- */

/// ProcessInlinePars -- handles inline par specifications in graphics functions.
/// Stub implementation: does nothing.
pub fn ProcessInlinePars(_s: SEXP, _dd: pGEDevDesc) {
    /* Stub: full implementation walks a list and calls Specify2 for each tagged pair */
}

/* ---- baseCallback (GE event handler) ---- */

/// baseCallback -- event handler for the base graphics system, registered
/// with the Graphics Engine via GEregisterSystem.
pub fn baseCallback(_task: c_int, _dd: pGEDevDesc, _data: SEXP) -> SEXP {
    R_NilValue()
}

/* ---- registerBase / unregisterBase / RunregisterBase ---- */

/// registerBase -- register the base graphics system with the Graphics Engine.
pub fn registerBase() {
    with_par_state(|state| {
        if state.base_register_index.is_some() {
            return;
        }
        let mut index = -1;

        crate::mainutils::engine::GEregisterSystem(Some(baseCallback), &mut index);

        if index >= 0 {
            state.base_register_index = Some(index);
        }
    });
}

/// unregisterBase -- unregister the base graphics system.
pub fn unregisterBase() {
    let index = with_par_state(|state| state.base_register_index.take());
    if let Some(index) = index {
        crate::mainutils::engine::GEunregisterSystem(index);
    }
}

/// RunregisterBase -- R-callable wrapper for unregisterBase.
/// Returns R_NilValue.
pub fn RunregisterBase() -> SEXP {
    unregisterBase();
    R_NilValue()
}

/* ---- GPar access / Rf_setBaseDevice ---- */

/// Copy of the device's current GPar (`*gpptr(dd)`); `None` for a null key.
pub(crate) fn gpar_get(dd: pGEDevDesc) -> Option<GPar> {
    with_device_gpar_entry(dd, |entry| entry.gp)
}

/// Copy of the device's display GPar (`*dpptr(dd)`); `None` for a null key.
pub(crate) fn dpar_get(dd: pGEDevDesc) -> Option<GPar> {
    with_device_gpar_entry(dd, |entry| entry.dp)
}

/// Update the device's current GPar in place. `f` runs inside the parameter
/// state borrow and must not draw, evaluate R or query parameters.
pub(crate) fn with_gpar_mut<T>(dd: pGEDevDesc, f: impl FnOnce(&mut GPar) -> T) -> Option<T> {
    with_device_gpar_entry(dd, |entry| f(&mut entry.gp))
}

/// Update the device's display GPar in place; same contract as
/// [`with_gpar_mut`].
pub(crate) fn with_dpar_mut<T>(dd: pGEDevDesc, f: impl FnOnce(&mut GPar) -> T) -> Option<T> {
    with_device_gpar_entry(dd, |entry| f(&mut entry.dp))
}

/// Rf_setBaseDevice -- mark the device as "dirty" (has received base output).
/// Stub: does nothing.
pub fn Rf_setBaseDevice(_val: c_int, _dd: pGEDevDesc) {
    /* Stub: sets bss->baseDevice = val */
}

/* ---- Stub: currentFigureLocation ---- */

/// currentFigureLocation -- get the current figure's row and column.
/// Stub: sets both to 0.
pub fn currentFigureLocation(row: &mut c_int, col: &mut c_int, _dd: pGEDevDesc) {
    *row = 0;
    *col = 0;
}

/* ---- Stub: restoredpSaved ---- */

/// restoredpSaved -- restore display parameters from saved state.
/// Stub: does nothing.
pub fn restoredpSaved(_dd: pGEDevDesc) {
    /* Stub: full implementation copies all fields from dpSaved to dp */
}

#[cfg(test)]
mod tests {
    use std::ffi::CString;

    use super::*;
    use crate::sexp::instance::{RInstance, replace_current_instance};

    struct CurrentInstanceRestore(Option<*mut RInstance>);

    impl Drop for CurrentInstanceRestore {
        fn drop(&mut self) {
            unsafe {
                replace_current_instance(self.0);
            }
        }
    }

    fn switch_device_from_callback(_event: c_int, _device: *mut c_void, _data: SEXP) -> SEXP {
        let nested = crate::sexp::session::RSession::construct_detached(
            crate::sexp::session::RSession::new_for_gc_tests,
        );
        nested.with_active(|| {
            set_plot_parameter("xlog", ParValue::Logical(vec![1]));
        });
        crate::library::grdevices::device_registry::selectDevice(2);
        set_plot_parameter("ylog", ParValue::Logical(vec![1]));
        R_NilValue()
    }

    #[test]
    fn graphics_callback_keeps_parameters_in_original_session_and_device() {
        let outer = crate::sexp::session::RSession::new_for_gc_tests();
        outer.with_active(|| {
            use crate::library::grdevices::device_registry::{curDevice, selectDevice};
            use crate::mainutils::engine::{GEhandleEvent, GEregisterSystem, GEunregisterSystem};
            selectDevice(0);
            assert_eq!(curDevice(), 1);
            set_plot_parameter("xlog", ParValue::Logical(vec![1]));
            selectDevice(0);
            assert_eq!(curDevice(), 2);
            assert_eq!(parameter("xlog"), ParValue::Logical(vec![0]));
            selectDevice(1);

            let mut registration = -1;
            GEregisterSystem(Some(switch_device_from_callback), &mut registration);
            assert!(registration >= 0);
            GEhandleEvent(0, std::ptr::null_mut(), R_NilValue());
            GEunregisterSystem(registration);

            assert_eq!(curDevice(), 2);
            assert_eq!(parameter("xlog"), ParValue::Logical(vec![0]));
            assert_eq!(parameter("ylog"), ParValue::Logical(vec![1]));
            selectDevice(1);
            assert_eq!(parameter("xlog"), ParValue::Logical(vec![1]));
            assert_eq!(parameter("ylog"), ParValue::Logical(vec![0]));
        });
    }

    #[test]
    fn par_updates_are_session_local() {
        let mut left = crate::android::RSession::new();
        let mut right = crate::android::RSession::new();

        let left_result =
            left.eval("par(mar = c(1, 2, 3, 4)); cat(paste(par('mar'), collapse=','))");
        let right_result = right.eval("cat(paste(par('mar'), collapse=','))");

        assert_eq!(left_result.output.trim(), "1,2,3,4");
        assert_eq!(right_result.output.trim(), "5.1,4.1,4.1,2.1");
    }

    #[test]
    fn par_reports_full_and_writable_parameter_sets() {
        let mut session = crate::android::RSession::new();
        let result = session.eval("cat(length(par()), length(par(no.readonly = TRUE)))");
        assert_eq!(result.output.trim(), "72 66");
    }

    #[test]
    fn c_par_delegates_to_session_local_par_state() {
        // C_par allocates and attaches attributes, which requires a managed
        // runtime owner; raw RInstance::new() only creates unowned storage.
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let mar = Rf_allocVector3(SEXPTYPE::REALSXP, 4);
            let _mar_guard = protect(mar);
            let mar_values = REAL(mar);
            for (idx, value) in [1.0, 2.0, 3.0, 4.0].iter().enumerate() {
                *mar_values.add(idx) = *value;
            }

            let args = Rf_cons(mar, R_NilValue());
            let _args_guard = protect(args);
            let mar_name = CString::new("mar").unwrap();
            SETTAG(args, Rf_install(mar_name.as_ptr()));

            let old = C_par(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );
            assert_eq!(LENGTH(old), 1);

            let query_text = Rf_mkString(mar_name.as_ptr());
            let _query_text_guard = protect(query_text);
            let query = Rf_cons(query_text, R_NilValue());
            let _query_guard = protect(query);

            let current = C_par(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                query,
                std::ptr::null_mut(),
            );
            assert_eq!(TYPEOF(current), SEXPTYPE::VECSXP);
            assert_eq!(XLENGTH(current), 1);
            let current = VECTOR_ELT(current, 0);
            assert_eq!(TYPEOF(current), SEXPTYPE::REALSXP);
            assert_eq!(XLENGTH(current), 4);
            let current_values = REAL(current);
            assert_eq!(
                [
                    *current_values.add(0),
                    *current_values.add(1),
                    *current_values.add(2),
                    *current_values.add(3),
                ],
                [1.0, 2.0, 3.0, 4.0]
            );
        });
    }

    #[test]
    fn layout_updates_session_local_par_state() {
        let mut session = crate::android::RSession::new();
        let result = session.eval(
            "layout(matrix(1:4, 2, 2)); cat(layout(matrix(1:9, 3, 3)), paste(par('mfg'), collapse=','), par('cex'), par('mex'))",
        );
        assert_eq!(result.output.trim(), "9 3,3,3,3 0.66 1");
    }

    #[test]
    fn base_registration_is_session_local_and_idempotent() {
        let mut left = RInstance::new();
        let mut right = RInstance::new();

        unsafe {
            let previous = replace_current_instance(Some(&mut left as *mut RInstance));
            let _restore = CurrentInstanceRestore(previous);

            registerBase();
            registerBase();
            with_par_state(|state| {
                assert_eq!(state.base_register_index, Some(0));
            });
        }

        unsafe {
            let previous = replace_current_instance(Some(&mut right as *mut RInstance));
            let _restore = CurrentInstanceRestore(previous);
            with_par_state(|state| {
                assert_eq!(state.base_register_index, None);
            });
        }

        unsafe {
            let previous = replace_current_instance(Some(&mut left as *mut RInstance));
            let _restore = CurrentInstanceRestore(previous);
            unregisterBase();
            unregisterBase();
            with_par_state(|state| {
                assert_eq!(state.base_register_index, None);
            });
        }
    }
}
