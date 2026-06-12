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
    base_register_index: Option<c_int>,
    device_gpars: HashMap<usize, DeviceGParEntry>,
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
        "cin" => ParValue::Real(vec![0.15, 0.2]),
        "col" | "col.axis" | "col.lab" | "col.main" | "col.sub" | "fg" => {
            ParValue::String("black".into())
        }
        "cra" => ParValue::Real(vec![10.8, 14.4]),
        "crt" | "srt" => ParValue::Real(vec![0.0]),
        "csi" => ParValue::Real(vec![0.2]),
        "cxy" => ParValue::Real(vec![0.0260416666666667, 0.0387596899224806]),
        "din" | "fin" => ParValue::Real(vec![7.0, 7.0]),
        "err" | "las" => ParValue::Integer(vec![0]),
        "family" => ParValue::String(String::new()),
        "fig" | "omd" | "usr" => ParValue::Real(vec![0.0, 1.0, 0.0, 1.0]),
        "font" | "font.axis" | "font.lab" | "font.sub" | "pch" => ParValue::Integer(vec![1]),
        "font.main" => ParValue::Integer(vec![2]),
        "lab" => ParValue::Integer(vec![5, 5, 7]),
        "lend" | "ljoin" => ParValue::String("round".into()),
        "lmitre" => ParValue::Real(vec![10.0]),
        "lty" => ParValue::String("solid".into()),
        "mai" => ParValue::Real(vec![1.02, 0.82, 0.82, 0.42]),
        "mar" => ParValue::Real(vec![5.1, 4.1, 4.1, 2.1]),
        "mfcol" | "mfrow" => ParValue::Integer(vec![1, 1]),
        "mfg" => ParValue::Integer(vec![1, 1, 1, 1]),
        "mgp" => ParValue::Real(vec![3.0, 1.0, 0.0]),
        "mkh" => ParValue::Real(vec![0.001]),
        "oma" | "omi" => ParValue::Real(vec![0.0, 0.0, 0.0, 0.0]),
        "pin" => ParValue::Real(vec![5.76, 5.16]),
        "plt" => ParValue::Real(vec![
            0.117142857142857,
            0.94,
            0.145714285714286,
            0.882857142857143,
        ]),
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

fn par_error(message: impl Into<String>) -> ! {
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
    name: *const c_char,
    code: c_int,
}

// Safety: PAR_TABLE only contains pointers to static string literals,
// which live for the entire program duration.
unsafe impl Sync for ParTab {}

/// The complete ParTable array from par.c.
/// This is pure data used by ParCode() to look up parameter codes.
static PAR_TABLE: &[ParTab] = &[
    ParTab {
        name: b"adj\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"ann\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"ask\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"bg\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"bty\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"cex\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"cex.axis\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"cex.lab\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"cex.main\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"cex.sub\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"cin\0".as_ptr() as *const c_char,
        code: 2,
    },
    ParTab {
        name: b"col\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"col.axis\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"col.lab\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"col.main\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"col.sub\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"cra\0".as_ptr() as *const c_char,
        code: 2,
    },
    ParTab {
        name: b"crt\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"csi\0".as_ptr() as *const c_char,
        code: 2,
    },
    ParTab {
        name: b"csy\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"cxy\0".as_ptr() as *const c_char,
        code: 2,
    },
    ParTab {
        name: b"din\0".as_ptr() as *const c_char,
        code: 2,
    },
    ParTab {
        name: b"err\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"family\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"fg\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"fig\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"fin\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"font\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"font.axis\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"font.lab\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"font.main\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"font.sub\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"lab\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"las\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"lend\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"lheight\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"ljoin\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"lmitre\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"lty\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"lwd\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"mai\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"mar\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"mex\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"mfcol\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"mfg\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"mfrow\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"mgp\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"mkh\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"new\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"oma\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"omd\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"omi\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"page\0".as_ptr() as *const c_char,
        code: 2,
    },
    ParTab {
        name: b"pch\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"pin\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"plt\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"ps\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"pty\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"smo\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"srt\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"tck\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"tcl\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"usr\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"xaxp\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"xaxs\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"xaxt\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"xlog\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"xpd\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"yaxp\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"yaxs\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"yaxt\0".as_ptr() as *const c_char,
        code: 0,
    },
    ParTab {
        name: b"ylbias\0".as_ptr() as *const c_char,
        code: 1,
    },
    ParTab {
        name: b"ylog\0".as_ptr() as *const c_char,
        code: 1,
    },
    /* Obsolete pars */
    ParTab {
        name: b"gamma\0".as_ptr() as *const c_char,
        code: -2,
    },
    ParTab {
        name: b"type\0".as_ptr() as *const c_char,
        code: -2,
    },
    ParTab {
        name: b"tmag\0".as_ptr() as *const c_char,
        code: -2,
    },
    /* Non-pars that might get passed to Specify2 */
    ParTab {
        name: b"asp\0".as_ptr() as *const c_char,
        code: -3,
    },
    ParTab {
        name: b"main\0".as_ptr() as *const c_char,
        code: -3,
    },
    ParTab {
        name: b"sub\0".as_ptr() as *const c_char,
        code: -3,
    },
    ParTab {
        name: b"xlab\0".as_ptr() as *const c_char,
        code: -3,
    },
    ParTab {
        name: b"ylab\0".as_ptr() as *const c_char,
        code: -3,
    },
    ParTab {
        name: b"xlim\0".as_ptr() as *const c_char,
        code: -3,
    },
    ParTab {
        name: b"ylim\0".as_ptr() as *const c_char,
        code: -3,
    },
    ParTab {
        name: std::ptr::null(),
        code: -1,
    },
];

/// pGEDevDesc is an opaque pointer to the graphics device descriptor.
/// The full type is defined in the Graphics Engine, which is not yet ported.
type pGEDevDesc = *mut c_void;

/// Look up a graphical parameter name in ParTable and return its code.
/// Returns -1 if not found.
///
/// This is the Rust equivalent of `static int ParCode(const char *what)`.
pub unsafe fn ParCode(what: *const c_char) -> c_int {
    unsafe {
        if what.is_null() {
            return -1;
        }
        let what_str = std::ffi::CStr::from_ptr(what);
        let what_bytes = what_str.to_bytes();
        for entry in PAR_TABLE.iter() {
            if entry.name.is_null() {
                break;
            }
            let name_str = std::ffi::CStr::from_ptr(entry.name);
            if name_str.to_bytes() == what_bytes {
                return entry.code;
            }
        }
        -1
    }
}

/* ---- Stub helper functions ---- */

/// Helper: compare two C strings for equality.
unsafe fn streql(a: *const c_char, b: *const c_char) -> bool {
    unsafe {
        if a.is_null() || b.is_null() {
            return false;
        }
        libc::strcmp(a, b) == 0
    }
}

unsafe fn par_name_from_c(what: *const c_char) -> String {
    unsafe {
        if what.is_null() {
            par_error("invalid graphical parameter");
        }
        std::ffi::CStr::from_ptr(what)
            .to_string_lossy()
            .into_owned()
    }
}

/// Specify -- set a graphical parameter via par().
unsafe fn Specify(what: *const c_char, value: SEXP, _dd: pGEDevDesc) {
    unsafe {
        let name = par_name_from_c(what);
        if !is_known_par(&name) {
            par_error(format!(
                "invalid value specified for graphical parameter \"{name}\""
            ));
        }
        if is_readonly_par(&name) {
            par_error(format!("graphical parameter \"{name}\" is read-only"));
        }
        let value = sexp_to_par_value(value);
        with_par_state(|state| {
            state.overrides.insert(name, value);
        });
    }
}

/// Specify2 -- set a graphical parameter from a high-level graphics function.
unsafe fn Specify2(what: *const c_char, value: SEXP, dd: pGEDevDesc) {
    unsafe {
        let name = par_name_from_c(what);
        if is_known_par(&name) && !is_readonly_par(&name) {
            Specify(what, value, dd);
        }
    }
}

/// Query -- return the current value of a graphical parameter.
unsafe fn Query(what: *const c_char, _dd: pGEDevDesc) -> SEXP {
    unsafe {
        let name = par_name_from_c(what);
        if !is_known_par(&name) {
            par_error(format!(
                "invalid value specified for graphical parameter \"{name}\""
            ));
        }
        with_par_state(|state| {
            let value = current_par_value(state, &name);
            par_value_to_sexp(&value)
        })
    }
}

fn with_par_state<T>(f: impl FnOnce(&mut GraphicsParState) -> T) -> T {
    crate::sexp::instance::with_required_current_instance(|instance| {
        f(&mut instance.graphics_par_state)
    })
}

fn graphics_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

unsafe fn layout_pop_arg(args: &mut SEXP) -> SEXP {
    unsafe {
        if args.is_null() || *args == R_NilValue() {
            graphics_error("invalid graphics layout");
        }
        let value = CAR(*args);
        *args = CDR(*args);
        value
    }
}

unsafe fn layout_int_scalar(args: &mut SEXP, name: &str) -> c_int {
    unsafe {
        let value = layout_pop_arg(args);
        if value.is_null() || value == R_NilValue() || XLENGTH(value) < 1 {
            graphics_error(format!("invalid '{name}' in graphics layout"));
        }
        match TYPEOF(value) {
            t if t == SEXPTYPE::INTSXP => *INTEGER(value),
            t if t == SEXPTYPE::REALSXP => *REAL(value) as c_int,
            _ => graphics_error(format!("invalid '{name}' in graphics layout")),
        }
    }
}

unsafe fn layout_int_values(args: &mut SEXP, name: &str, min_len: usize) -> Vec<c_int> {
    unsafe {
        let value = layout_pop_arg(args);
        if value.is_null() || value == R_NilValue() || XLENGTH(value) < min_len as R_xlen_t {
            graphics_error(format!("invalid '{name}' in graphics layout"));
        }
        let n = XLENGTH(value) as usize;
        match TYPEOF(value) {
            t if t == SEXPTYPE::INTSXP => {
                let src = INTEGER(value);
                (0..n).map(|i| *src.add(i)).collect()
            }
            t if t == SEXPTYPE::REALSXP => {
                let src = REAL(value);
                (0..n).map(|i| *src.add(i) as c_int).collect()
            }
            _ => graphics_error(format!("invalid '{name}' in graphics layout")),
        }
    }
}

unsafe fn layout_real_values(args: &mut SEXP, name: &str, min_len: usize) -> Vec<c_double> {
    unsafe {
        let value = layout_pop_arg(args);
        if value.is_null() || value == R_NilValue() || XLENGTH(value) < min_len as R_xlen_t {
            graphics_error(format!("invalid '{name}' in graphics layout"));
        }
        let n = XLENGTH(value) as usize;
        match TYPEOF(value) {
            t if t == SEXPTYPE::REALSXP => {
                let src = REAL(value);
                (0..n).map(|i| *src.add(i)).collect()
            }
            t if t == SEXPTYPE::INTSXP => {
                let src = INTEGER(value);
                (0..n).map(|i| *src.add(i) as c_double).collect()
            }
            _ => graphics_error(format!("invalid '{name}' in graphics layout")),
        }
    }
}

fn apply_layout_state(nrow: c_int, ncol: c_int, order: &[c_int], num_figures: c_int) {
    let (mut current_row, mut current_col) = (1, 1);
    if num_figures > 0
        && let Some(index) = order.iter().position(|value| *value == num_figures)
    {
        current_row = (index as c_int % nrow) + 1;
        current_col = (index as c_int / nrow) + 1;
    }
    let cex = if nrow > 2 || ncol > 2 {
        0.66
    } else if nrow == 2 && ncol == 2 {
        0.83
    } else {
        1.0
    };

    with_par_state(|state| {
        state
            .overrides
            .insert("mfrow".into(), ParValue::Integer(vec![nrow, ncol]));
        state
            .overrides
            .insert("mfcol".into(), ParValue::Integer(vec![nrow, ncol]));
        state.overrides.insert(
            "mfg".into(),
            ParValue::Integer(vec![current_row, current_col, nrow, ncol]),
        );
        state
            .overrides
            .insert("cex".into(), ParValue::Real(vec![cex]));
        state
            .overrides
            .insert("mex".into(), ParValue::Real(vec![1.0]));
        state
            .overrides
            .insert("new".into(), ParValue::Logical(vec![FALSE]));
    });
}

unsafe fn matrix_shape(value: SEXP) -> Option<(c_int, c_int)> {
    unsafe {
        let dim =
            crate::sexp::attrib_core::getAttrib(value, crate::sexp::attrib_core::R_DimSymbol());
        if dim.is_null()
            || dim == R_NilValue()
            || TYPEOF(dim) != SEXPTYPE::INTSXP
            || XLENGTH(dim) < 2
        {
            return None;
        }
        Some((*INTEGER(dim), *INTEGER(dim).add(1)))
    }
}

unsafe fn layout_matrix_order(value: SEXP, len: usize) -> Vec<c_int> {
    unsafe {
        if value.is_null() || value == R_NilValue() || XLENGTH(value) < len as R_xlen_t {
            graphics_error("invalid layout matrix");
        }
        match TYPEOF(value) {
            t if t == SEXPTYPE::INTSXP => {
                let src = INTEGER(value);
                (0..len).map(|i| *src.add(i)).collect()
            }
            t if t == SEXPTYPE::REALSXP => {
                let src = REAL(value);
                (0..len).map(|i| *src.add(i) as c_int).collect()
            }
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

unsafe fn par_value_to_sexp(value: &ParValue) -> SEXP {
    unsafe {
        match value {
            ParValue::Logical(values) => {
                let result = Rf_allocVector3(SEXPTYPE::LGLSXP, values.len() as R_xlen_t);
                if result.is_null() {
                    return R_NilValue();
                }
                let dst = LOGICAL(result);
                for (i, value) in values.iter().enumerate() {
                    *dst.add(i) = *value;
                }
                result
            }
            ParValue::Integer(values) => {
                let result = Rf_allocVector3(SEXPTYPE::INTSXP, values.len() as R_xlen_t);
                if result.is_null() {
                    return R_NilValue();
                }
                let dst = INTEGER(result);
                for (i, value) in values.iter().enumerate() {
                    *dst.add(i) = *value;
                }
                result
            }
            ParValue::Real(values) => {
                let result = Rf_allocVector3(SEXPTYPE::REALSXP, values.len() as R_xlen_t);
                if result.is_null() {
                    return R_NilValue();
                }
                let dst = REAL(result);
                for (i, value) in values.iter().enumerate() {
                    *dst.add(i) = *value;
                }
                result
            }
            ParValue::String(value) => {
                let cstr = std::ffi::CString::new(value.as_str()).unwrap_or_default();
                Rf_mkString(cstr.as_ptr())
            }
        }
    }
}

unsafe fn named_par_list(names: &[String], state: &GraphicsParState) -> SEXP {
    unsafe {
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
            let value = current_par_value(state, name);
            SET_VECTOR_ELT(result, i as R_xlen_t, par_value_to_sexp(&value));
            let cstr = std::ffi::CString::new(name.as_str()).unwrap_or_default();
            SET_STRING_ELT(name_vec, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
        }

        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            name_vec,
        );
        result
    }
}

unsafe fn sexp_to_par_value(value: SEXP) -> ParValue {
    unsafe {
        match TYPEOF(value) {
            t if t == SEXPTYPE::LGLSXP => {
                let n = XLENGTH(value);
                let src = LOGICAL(value);
                let mut out = Vec::with_capacity(n as usize);
                for i in 0..n {
                    out.push(*src.add(i as usize));
                }
                ParValue::Logical(out)
            }
            t if t == SEXPTYPE::INTSXP => {
                let n = XLENGTH(value);
                let src = INTEGER(value);
                let mut out = Vec::with_capacity(n as usize);
                for i in 0..n {
                    out.push(*src.add(i as usize));
                }
                ParValue::Integer(out)
            }
            t if t == SEXPTYPE::REALSXP => {
                let n = XLENGTH(value);
                let src = REAL(value);
                let mut out = Vec::with_capacity(n as usize);
                for i in 0..n {
                    out.push(*src.add(i as usize));
                }
                ParValue::Real(out)
            }
            t if t == SEXPTYPE::STRSXP && XLENGTH(value) == 1 => {
                let elt = STRING_ELT(value, 0);
                if elt == R_NaString() {
                    ParValue::String(String::new())
                } else {
                    let text = std::ffi::CStr::from_ptr(CHAR(elt))
                        .to_string_lossy()
                        .into_owned();
                    ParValue::String(text)
                }
            }
            _ => par_error("invalid value specified for graphical parameter"),
        }
    }
}

unsafe fn string_vector_values(value: SEXP) -> Vec<String> {
    unsafe {
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
            out.push(
                std::ffi::CStr::from_ptr(CHAR(elt))
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        out
    }
}

unsafe fn tag_name(tag: SEXP) -> Option<String> {
    unsafe {
        if tag.is_null() || tag == R_NilValue() {
            None
        } else {
            Some(
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned(),
            )
        }
    }
}

unsafe fn logical_scalar(value: SEXP) -> Option<bool> {
    unsafe {
        if TYPEOF(value) != SEXPTYPE::LGLSXP || XLENGTH(value) < 1 {
            return None;
        }
        match *LOGICAL(value) {
            TRUE => Some(true),
            FALSE => Some(false),
            _ => None,
        }
    }
}

unsafe fn all_query_names(no_readonly: bool) -> Vec<String> {
    PAR_ORDER
        .iter()
        .filter(|name| !no_readonly || !is_readonly_par(name))
        .map(|name| (*name).to_string())
        .collect()
}

/// Rust-shaped `par()` implementation backed by per-session defaults and
/// overrides. It intentionally covers the query/update surface used by base
/// plotting and Android embedding without relying on process-global GE state.
pub unsafe fn do_par(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut current = args;
        let mut query_names = Vec::new();
        let mut set_names = Vec::new();
        let mut set_values = Vec::new();
        let mut no_readonly = false;

        while !current.is_null() && current != R_NilValue() {
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
                    for name in string_vector_values(value) {
                        if !is_known_par(&name) {
                            par_error(format!(
                                "invalid value specified for graphical parameter \"{name}\""
                            ));
                        }
                        query_names.push(name);
                    }
                }
            }
            current = CDR(current);
        }

        if set_names.is_empty() && query_names.is_empty() {
            let names = all_query_names(no_readonly);
            return with_par_state(|state| named_par_list(&names, state));
        }

        let result = with_par_state(|state| {
            if !set_names.is_empty() {
                let old = named_par_list(&set_names, state);
                for (name, value) in set_names.iter().zip(set_values.into_iter()) {
                    state.overrides.insert(name.clone(), value);
                }
                old
            } else if query_names.len() == 1 {
                let value = current_par_value(state, &query_names[0]);
                par_value_to_sexp(&value)
            } else {
                named_par_list(&query_names, state)
            }
        });

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
pub unsafe fn C_par(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { do_par(call, op, args, rho) }
}

/* ---- C_layout (the layout() .Internal) ---- */

/// C_layout -- implementation of R's layout() function.
/// This is the .Internal(layout(...)) entry point.
///
/// Original C signature:
///   SEXP C_layout(SEXP args)
///
pub unsafe fn C_layout(_args: SEXP) -> SEXP {
    unsafe {
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
pub unsafe fn do_layout(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
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
pub unsafe fn ProcessInlinePars(_s: SEXP, _dd: pGEDevDesc) {
    /* Stub: full implementation walks a list and calls Specify2 for each tagged pair */
}

/* ---- baseCallback (GE event handler) ---- */

/// baseCallback -- event handler for the base graphics system, registered
/// with the Graphics Engine via GEregisterSystem.
pub unsafe extern "C" fn baseCallback(_task: c_int, _dd: pGEDevDesc, _data: SEXP) -> SEXP {
    unsafe { R_NilValue() }
}

/* ---- registerBase / unregisterBase / RunregisterBase ---- */

/// registerBase -- register the base graphics system with the Graphics Engine.
pub fn registerBase() {
    with_par_state(|state| {
        if state.base_register_index.is_some() {
            return;
        }
        let mut index = -1;
        unsafe {
            crate::mainutils::engine::GEregisterSystem(Some(baseCallback), &mut index);
        }
        if index >= 0 {
            state.base_register_index = Some(index);
        }
    });
}

/// unregisterBase -- unregister the base graphics system.
pub fn unregisterBase() {
    let index = with_par_state(|state| state.base_register_index.take());
    if let Some(index) = index {
        unsafe {
            crate::mainutils::engine::GEunregisterSystem(index);
        }
    }
}

/// RunregisterBase -- R-callable wrapper for unregisterBase.
/// Returns R_NilValue.
pub unsafe fn RunregisterBase() -> SEXP {
    unregisterBase();
    unsafe { R_NilValue() }
}

/* ---- gpptr / dpptr / dpSavedptr / Rf_setBaseDevice ---- */

/// gpptr -- get the current GPar pointer (graphics parameters).
pub unsafe fn gpptr(dd: pGEDevDesc) -> *mut c_void {
    with_device_gpar_entry(dd, |entry| &mut entry.gp as *mut GPar as *mut c_void)
        .unwrap_or(std::ptr::null_mut())
}

/// dpptr -- get the display GPar pointer (display parameters).
pub unsafe fn dpptr(dd: pGEDevDesc) -> *mut c_void {
    with_device_gpar_entry(dd, |entry| &mut entry.dp as *mut GPar as *mut c_void)
        .unwrap_or(std::ptr::null_mut())
}

/// dpSavedptr -- get the saved display GPar pointer.
pub unsafe fn dpSavedptr(dd: pGEDevDesc) -> *mut c_void {
    with_device_gpar_entry(dd, |entry| &mut entry.dp_saved as *mut GPar as *mut c_void)
        .unwrap_or(std::ptr::null_mut())
}

/// Rf_setBaseDevice -- mark the device as "dirty" (has received base output).
/// Stub: does nothing.
pub unsafe fn Rf_setBaseDevice(_val: c_int, _dd: pGEDevDesc) {
    /* Stub: sets bss->baseDevice = val */
}

/* ---- Stub: currentFigureLocation ---- */

/// currentFigureLocation -- get the current figure's row and column.
/// Stub: sets both to 0.
pub unsafe fn currentFigureLocation(row: *mut c_int, col: *mut c_int, _dd: pGEDevDesc) {
    unsafe {
        if !row.is_null() {
            *row = 0;
        }
        if !col.is_null() {
            *col = 0;
        }
    }
}

/* ---- Stub: restoredpSaved ---- */

/// restoredpSaved -- restore display parameters from saved state.
/// Stub: does nothing.
pub unsafe fn restoredpSaved(_dd: pGEDevDesc) {
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
        let mut instance = RInstance::new();

        unsafe {
            let previous = replace_current_instance(Some(&mut instance as *mut RInstance));
            let _restore = CurrentInstanceRestore(previous);

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
        }
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
