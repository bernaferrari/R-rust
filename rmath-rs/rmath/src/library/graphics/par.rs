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

use std::collections::BTreeMap;
use std::ffi::c_void;
use std::os::raw::{c_char, c_double, c_int, c_uchar, c_ushort};

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

#[derive(Default)]
pub(crate) struct GraphicsParState {
    overrides: BTreeMap<String, ParValue>,
    base_register_index: Option<c_int>,
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
    graphics_error("graphics::layout is not implemented without a graphics device layout backend")
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

/* ---- Stub: gpptr / dpptr / dpSavedptr / Rf_setBaseDevice ---- */

/// gpptr -- get the current GPar pointer (graphics parameters).
/// Stub: returns null.
pub unsafe fn gpptr(_dd: pGEDevDesc) -> *mut c_void {
    std::ptr::null_mut()
}

/// dpptr -- get the display GPar pointer (display parameters).
/// Stub: returns null.
pub unsafe fn dpptr(_dd: pGEDevDesc) -> *mut c_void {
    std::ptr::null_mut()
}

/// dpSavedptr -- get the saved display GPar pointer.
/// Stub: returns null.
pub unsafe fn dpSavedptr(_dd: pGEDevDesc) -> *mut c_void {
    std::ptr::null_mut()
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
    fn layout_reports_explicit_backend_gap() {
        let mut instance = RInstance::new();

        unsafe {
            let previous = replace_current_instance(Some(&mut instance as *mut RInstance));
            let _restore = CurrentInstanceRestore(previous);

            let payload = std::panic::catch_unwind(|| {
                C_layout(R_NilValue());
            })
            .expect_err("layout without a device backend should error");
            let err = payload
                .downcast_ref::<RError>()
                .expect("expected RError payload");
            assert!(err.message.contains("graphics::layout"));
        }
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
