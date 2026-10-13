/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1999-2025   The R Core Team
 *  Copyright (C) 1995--1997  Robert Gentleman and Ross Ihaka
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
 *  Ported from r-source/src/library/stats/src/filter.c
 */

use std::os::raw::{c_double, c_int, c_longlong};

use crate::attrib_core::{R_DimSymbol, getAttrib, setAttrib};
#[allow(unused_imports)]
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;

// Shared helpers used across the child modules.

unsafe fn coerceVector(x: SEXP, type_: c_int) -> SEXP {
    unsafe { crate::main::coerce::coerceVector(x, type_) }
}

unsafe fn asInteger(x: SEXP) -> c_int {
    unsafe { crate::main::coerce::asInteger(x) }
}

unsafe fn asLogical(x: SEXP) -> c_int {
    unsafe { crate::main::coerce::asLogical(x) }
}

unsafe fn asBool(x: SEXP) -> bool {
    unsafe {
        let v = asLogical(x);
        v != 0 && v != NA_INTEGER
    }
}

unsafe fn as_integer(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return NA_INTEGER;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::INTSXP {
            return *INTEGER(x);
        }
        if t == SEXPTYPE::REALSXP {
            let v = *REAL(x);
            if v.is_nan() || v < c_int::MIN as f64 || v > c_int::MAX as f64 {
                return NA_INTEGER;
            }
            return v as c_int;
        }
        if t == SEXPTYPE::LGLSXP {
            return *INTEGER(x);
        }
        NA_INTEGER
    }
}

unsafe fn as_logical(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return NA_INTEGER;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::LGLSXP {
            return *INTEGER(x);
        }
        if t == SEXPTYPE::INTSXP {
            return *INTEGER(x);
        }
        NA_INTEGER
    }
}

fn my_isok(x: c_double) -> bool {
    !ISNAN(x)
}

unsafe fn nrows(x: SEXP) -> c_int {
    unsafe {
        let d = getAttrib(x, R_DimSymbol());
        if d.is_null() || d == R_NilValue() {
            return LENGTH(x);
        }
        *INTEGER(d)
    }
}

unsafe fn ncols(x: SEXP) -> c_int {
    unsafe {
        let d = getAttrib(x, R_DimSymbol());
        if d.is_null() || d == R_NilValue() {
            return 1;
        }
        if LENGTH(d) >= 2 {
            return *INTEGER(d).add(1);
        }
        1
    }
}

fn elt_real_or(x: SEXP, default: f64) -> f64 {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return default;
        }
        if TYPEOF(x) == SEXPTYPE::REALSXP && XLENGTH(x) > 0 {
            return *REAL(x);
        }
        if TYPEOF(x) == SEXPTYPE::INTSXP && XLENGTH(x) > 0 {
            return *INTEGER(x) as f64;
        }
        default
    }
}

/// GNU `KalmanLike` for state dim `p`. Matrices are column-major.
/// `s_up` gates the P prediction (`l > UP`); likelihood still includes
/// every non-NA observation, matching `arima.c`.
fn kalman_like_nd(
    y: &[f64],
    z: &[f64],
    a0: &[f64],
    p0: &[f64],
    t: &[f64],
    v: &[f64],
    h: f64,
    s_up: i32,
) -> f64 {
    let n = y.len();
    let p = a0.len();
    if n == 0 || p == 0 || z.len() != p || p0.len() != p * p || t.len() != p * p || v.len() != p * p
    {
        return f64::INFINITY;
    }
    let mut a = a0.to_vec();
    let mut pmat = p0.to_vec();
    let mut pnew = p0.to_vec();
    let mut anew = vec![0.0; p];
    let mut mm = vec![0.0; p * p];
    let mut m = vec![0.0; p];
    let mut ssq = 0.0;
    let mut sumlog = 0.0;
    let mut nu = 0.0;
    for l in 0..n {
        for i in 0..p {
            let mut tmp = 0.0;
            for k in 0..p {
                tmp += t[i + p * k] * a[k];
            }
            anew[i] = tmp;
        }
        if (l as i32) > s_up {
            for i in 0..p {
                for j in 0..p {
                    let mut tmp = 0.0;
                    for k in 0..p {
                        tmp += t[i + p * k] * pmat[k + p * j];
                    }
                    mm[i + p * j] = tmp;
                }
            }
            for i in 0..p {
                for j in 0..p {
                    let mut tmp = v[i + p * j];
                    for k in 0..p {
                        tmp += mm[i + p * k] * t[j + p * k];
                    }
                    pnew[i + p * j] = tmp;
                }
            }
        }
        if y[l].is_nan() {
            a.copy_from_slice(&anew);
            pmat.copy_from_slice(&pnew);
            continue;
        }
        let mut resid = y[l];
        for i in 0..p {
            resid -= z[i] * anew[i];
        }
        let mut gain = h;
        for i in 0..p {
            let mut tmp = 0.0;
            for j in 0..p {
                tmp += pnew[i + j * p] * z[j];
            }
            m[i] = tmp;
            gain += z[i] * tmp;
        }
        if !(gain > 0.0) {
            return f64::INFINITY;
        }
        ssq += resid * resid / gain;
        sumlog += gain.ln();
        nu += 1.0;
        for i in 0..p {
            a[i] = anew[i] + m[i] * resid / gain;
        }
        for i in 0..p {
            for j in 0..p {
                pmat[i + j * p] = pnew[i + j * p] - m[i] * m[j] / gain;
            }
        }
    }
    if nu < 1.0 {
        return f64::INFINITY;
    }
    0.5 * (ssq / nu + sumlog / nu)
}

fn lyapunov_nd(t: &[f64], v: &[f64], r: usize) -> Vec<f64> {
    let mut p = vec![0.0; r * r];
    let mut tp = vec![0.0; r * r];
    for _ in 0..400 {
        for i in 0..r {
            for j in 0..r {
                let mut s = 0.0;
                for k in 0..r {
                    s += t[i + k * r] * p[k + j * r];
                }
                tp[i + j * r] = s;
            }
        }
        for i in 0..r {
            for j in 0..r {
                let mut s = v[i + j * r];
                for m in 0..r {
                    s += tp[i + m * r] * t[j + m * r];
                }
                p[i + j * r] = s;
            }
        }
    }
    p
}

fn kalman_s2_nd(
    y: &[f64],
    z: &[f64],
    a0: &[f64],
    p0: &[f64],
    t: &[f64],
    v: &[f64],
    h: f64,
    s_up: i32,
) -> f64 {
    let n = y.len();
    let p = a0.len();
    if n == 0 || p == 0 || z.len() != p || p0.len() != p * p || t.len() != p * p || v.len() != p * p
    {
        return f64::NAN;
    }
    let mut a = a0.to_vec();
    let mut pmat = p0.to_vec();
    let mut pnew = p0.to_vec();
    let mut anew = vec![0.0; p];
    let mut mm = vec![0.0; p * p];
    let mut m = vec![0.0; p];
    let mut ssq = 0.0;
    let mut nu = 0.0;
    for l in 0..n {
        for i in 0..p {
            let mut tmp = 0.0;
            for k in 0..p {
                tmp += t[i + p * k] * a[k];
            }
            anew[i] = tmp;
        }
        if (l as i32) > s_up {
            for i in 0..p {
                for j in 0..p {
                    let mut tmp = 0.0;
                    for k in 0..p {
                        tmp += t[i + p * k] * pmat[k + p * j];
                    }
                    mm[i + p * j] = tmp;
                }
            }
            for i in 0..p {
                for j in 0..p {
                    let mut tmp = v[i + p * j];
                    for k in 0..p {
                        tmp += mm[i + p * k] * t[j + p * k];
                    }
                    pnew[i + p * j] = tmp;
                }
            }
        }
        if y[l].is_nan() {
            a.copy_from_slice(&anew);
            pmat.copy_from_slice(&pnew);
            continue;
        }
        let mut resid = y[l];
        for i in 0..p {
            resid -= z[i] * anew[i];
        }
        let mut gain = h;
        for i in 0..p {
            let mut tmp = 0.0;
            for j in 0..p {
                tmp += pnew[i + j * p] * z[j];
            }
            m[i] = tmp;
            gain += z[i] * tmp;
        }
        if !(gain > 0.0) {
            return f64::NAN;
        }
        ssq += resid * resid / gain;
        nu += 1.0;
        for i in 0..p {
            a[i] = anew[i] + m[i] * resid / gain;
        }
        for i in 0..p {
            for j in 0..p {
                pmat[i + j * p] = pnew[i + j * p] - m[i] * m[j] / gain;
            }
        }
    }
    if nu < 1.0 { f64::NAN } else { ssq / nu }
}

fn symbol_chars(expr: SEXP) -> Option<String> {
    unsafe {
        if expr.is_null()
            || expr == R_NilValue()
            || expr == crate::sexp::globals::R_MissingArg()
            || TYPEOF(expr) != SEXPTYPE::SYMSXP
        {
            return None;
        }
        let print_name = PRINTNAME(expr);
        if print_name.is_null() || print_name == R_NilValue() {
            return None;
        }
        let raw = CHAR(print_name);
        if raw.is_null() {
            return None;
        }
        Some(std::ffi::CStr::from_ptr(raw).to_string_lossy().into_owned())
    }
}

fn collect_i_calls(expr: SEXP, out: &mut Vec<SEXP>) {
    unsafe {
        if expr.is_null() || expr == R_NilValue() || TYPEOF(expr) != SEXPTYPE::LANGSXP {
            return;
        }
        if symbol_chars(CAR(expr)).as_deref() == Some("I") {
            out.push(expr);
            return;
        }
        let mut cell = CDR(expr);
        while !cell.is_null() && cell != R_NilValue() {
            collect_i_calls(CAR(cell), out);
            cell = CDR(cell);
        }
    }
}

fn collect_formula_symbols(expr: SEXP, out: &mut Vec<String>) {
    unsafe {
        if expr.is_null() || expr == R_NilValue() || expr == crate::sexp::globals::R_MissingArg() {
            return;
        }
        if TYPEOF(expr) == SEXPTYPE::SYMSXP {
            let Some(name) = symbol_chars(expr) else {
                return;
            };
            if !matches!(
                name.as_str(),
                "~" | "+" | "-" | "*" | ":" | "/" | "^" | "I" | "("
            ) && !out.iter().any(|s| s == &name)
            {
                out.push(name);
            }
            return;
        }
        if TYPEOF(expr) == SEXPTYPE::LANGSXP {
            let mut cell = CDR(expr);
            while !cell.is_null() && cell != R_NilValue() {
                collect_formula_symbols(CAR(cell), out);
                cell = CDR(cell);
            }
        }
    }
}

fn deparse_call(expr: SEXP) -> String {
    unsafe {
        if TYPEOF(expr) == SEXPTYPE::SYMSXP {
            let name = symbol_chars(expr).unwrap_or_default();
            if name.is_empty() {
                return String::new();
            }
            if crate::mainutils::deparse::local_parse_data::is_valid_r_name_bytes(name.as_bytes()) {
                return name;
            }
            return format!("`{name}`");
        }
        if TYPEOF(expr) == SEXPTYPE::LGLSXP {
            let v = *INTEGER(expr);
            return if v == NA_INTEGER {
                "NA".to_string()
            } else if v == 0 {
                "FALSE".to_string()
            } else {
                "TRUE".to_string()
            };
        }
        if TYPEOF(expr) == SEXPTYPE::INTSXP {
            let v = *INTEGER(expr);
            return if v == NA_INTEGER {
                "NA".to_string()
            } else {
                v.to_string()
            };
        }
        if TYPEOF(expr) == SEXPTYPE::REALSXP {
            return (*REAL(expr)).to_string();
        }
        if TYPEOF(expr) != SEXPTYPE::LANGSXP {
            return String::new();
        }
        let op = symbol_chars(CAR(expr)).unwrap_or_default();
        let mut args = Vec::new();
        let mut cell = CDR(expr);
        while !cell.is_null() && cell != R_NilValue() {
            let mut piece = deparse_call(CAR(cell));
            let tag = TAG(cell);
            if let Some(name) = symbol_chars(tag) {
                piece = format!("{name} = {piece}");
            }
            args.push(piece);
            cell = CDR(cell);
        }
        if (op == "[" || op == "[[") && !args.is_empty() {
            let idx = args[1..]
                .iter()
                .map(|a| a.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let close = if op == "[[" { "]]" } else { "]" };
            return format!("{}{}{}{}", args[0], op, idx, close);
        }
        if op == "(" && args.len() == 1 {
            return format!("({})", args[0]);
        }
        if op == ":" && args.len() == 2 {
            return format!("{}:{}", args[0], args[1]);
        }
        if op == "^" && args.len() == 2 {
            return format!("{}^{}", args[0], args[1]);
        }
        if matches!(op.as_str(), "+" | "-" | "*" | "/") && args.len() == 2 {
            return format!("{} {} {}", args[0], op, args[1]);
        }
        if op == "-" && args.len() == 1 {
            return format!("-{}", args[0]);
        }
        format!("{op}({})", args.join(", "))
    }
}

unsafe fn named_list_elt(x: SEXP, name: &str) -> SEXP {
    unsafe {
        if TYPEOF(x) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if names.is_null() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return R_NilValue();
        }
        for i in 0..XLENGTH(x) {
            let s = std::ffi::CStr::from_ptr(CHAR(STRING_ELT(names, i)));
            if s.to_string_lossy() == name {
                return VECTOR_ELT(x, i);
            }
        }
        R_NilValue()
    }
}

fn mark_formula(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return x;
        }
        let class = Rf_mkString(c"formula".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_ClassSymbol(), class);
        x
    }
}

fn inherits_formula(x: SEXP) -> bool {
    unsafe {
        let class =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
        if class.is_null() || class == R_NilValue() || TYPEOF(class) != SEXPTYPE::STRSXP {
            return false;
        }
        for i in 0..XLENGTH(class) {
            let raw = CHAR(STRING_ELT(class, i));
            if !raw.is_null() && std::ffi::CStr::from_ptr(raw).to_bytes() == b"formula" {
                return true;
            }
        }
        false
    }
}

unsafe fn character_row_names(rn: SEXP, n: i64) -> SEXP {
    unsafe {
        if rn.is_null() || rn == R_NilValue() {
            return R_NilValue();
        }
        if TYPEOF(rn) == SEXPTYPE::STRSXP && XLENGTH(rn) == n {
            return rn;
        }
        if TYPEOF(rn) == SEXPTYPE::INTSXP && XLENGTH(rn) == 2 && *INTEGER(rn) == NA_INTEGER {
            let count = (*INTEGER(rn).add(1)).unsigned_abs() as i64;
            if count != n {
                return R_NilValue();
            }
            let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            for i in 0..n {
                let label = std::ffi::CString::new((i + 1).to_string()).unwrap_or_default();
                SET_STRING_ELT(out, i, Rf_mkChar(label.as_ptr()));
            }
            return out;
        }
        if TYPEOF(rn) == SEXPTYPE::INTSXP && XLENGTH(rn) == n {
            let out = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            for i in 0..n {
                let v = *INTEGER(rn).add(i as usize);
                let label = if v == NA_INTEGER {
                    "NA".to_string()
                } else {
                    v.to_string()
                };
                let c = std::ffi::CString::new(label).unwrap_or_default();
                SET_STRING_ELT(out, i, Rf_mkChar(c.as_ptr()));
            }
            return out;
        }
        R_NilValue()
    }
}

fn split_top_level_colon(lab: &str) -> Vec<&str> {
    if !is_formula_interaction(lab) {
        return vec![lab];
    }
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, ch) in lab.char_indices() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => {
                parts.push(&lab[start..i]);
                start = i + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&lab[start..]);
    parts
}

fn is_formula_interaction(lab: &str) -> bool {
    let mut depth = 0i32;
    for ch in lab.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            ':' | '*' if depth == 0 => return true,
            _ => {}
        }
    }
    false
}

fn matrix_colnames(x: SEXP) -> Vec<String> {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Vec::new();
        }
        if TYPEOF(x) == SEXPTYPE::STRSXP {
            let mut out = Vec::new();
            for i in 0..XLENGTH(x) {
                out.push(
                    std::ffi::CStr::from_ptr(CHAR(STRING_ELT(x, i)))
                        .to_string_lossy()
                        .into_owned(),
                );
            }
            return out;
        }
        let labs = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::symbol::Rf_install(c"term.labels".as_ptr()),
        );
        if !labs.is_null() && labs != R_NilValue() && TYPEOF(labs) == SEXPTYPE::STRSXP {
            return matrix_colnames(labs);
        }
        let dn =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimNamesSymbol());
        if !dn.is_null() && dn != R_NilValue() && TYPEOF(dn) == SEXPTYPE::VECSXP && XLENGTH(dn) >= 2
        {
            return matrix_colnames(VECTOR_ELT(dn, 1));
        }
        Vec::new()
    }
}

mod accessors;
mod acf;
mod arima;
mod filtering;
mod formula_update;
mod kalman;
mod model_frame;
mod model_helpers;
mod model_matrix;
mod terms;
mod ts;
mod ts_misc;

pub use accessors::*;
pub use acf::*;
pub use arima::*;
pub use filtering::*;
pub use formula_update::*;
pub use kalman::*;
pub use model_frame::*;
pub use model_helpers::*;
pub use model_matrix::*;
pub use terms::*;
pub use ts::*;
pub use ts_misc::*;
