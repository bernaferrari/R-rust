//! Essentials domain module `print` — extracted verbatim from essentials.rs.

use super::*;
use std::collections::BTreeSet;
use std::ffi::{CStr, CString};

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_REAL, R_NA_BIT_PATTERN, R_xlen_t, SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// Print / summary methods
// ---------------------------------------------------------------------------

/// R's `print.matrix(x)` — print a matrix with proper row/col formatting.
pub unsafe fn do_print_matrix(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("NULL");
            return R_NilValue();
        }
        let dim_attr = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"dim".as_ptr()));
        let (nrow, ncol) =
            if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2
            {
                (
                    *INTEGER(dim_attr) as R_xlen_t,
                    *INTEGER(dim_attr).add(1) as R_xlen_t,
                )
            } else {
                let n = XLENGTH(x).max(1);
                (n, 1)
            };

        // Get colnames
        let colnames = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"dimnames".as_ptr()));
        let col_names_vec: Vec<String> =
            if !colnames.is_null() && TYPEOF(colnames) == SEXPTYPE::VECSXP && LENGTH(colnames) >= 2
            {
                let cn = VECTOR_ELT(colnames, 1);
                if !cn.is_null() && TYPEOF(cn) == SEXPTYPE::STRSXP {
                    let m = XLENGTH(cn).min(ncol);
                    (0..m).map(|i| elt_to_string(cn, i)).collect()
                } else {
                    (0..ncol).map(|i| format!("[,{}]", i + 1)).collect()
                }
            } else {
                (0..ncol).map(|i| format!("[,{}]", i + 1)).collect()
            };

        // Print column headers
        let mut header = String::from("     ");
        for name in &col_names_vec {
            let _ = std::fmt::Write::write_fmt(&mut header, format_args!("{:>12}", name));
        }
        println!("{}", header);

        // Print rows
        for r in 0..nrow {
            let row_label = format!("[{},]", r + 1);
            print!("{:>4} ", row_label);
            for c in 0..ncol {
                let idx = c * nrow + r;
                let s = elt_to_string(x, idx as R_xlen_t);
                print!("{:>12}", s);
            }
            println!();
        }

        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `print.list(x)` — print a list with element names.
pub unsafe fn do_print_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("NULL");
            return R_NilValue();
        }
        let n = XLENGTH(x);
        // Get names
        let names = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"names".as_ptr()));
        let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP;

        for i in 0..n {
            let name = if has_names && i < XLENGTH(names) {
                let s = elt_to_string(names, i);
                if s.is_empty() {
                    format!("${}", i + 1)
                } else {
                    format!("${}", s)
                }
            } else {
                format!("${}", i + 1)
            };
            let elem = VECTOR_ELT(x, i as i64);
            let type_str = if elem.is_null() {
                "NULL".to_string()
            } else {
                let t = TYPEOF(elem);
                match t {
                    t if t == SEXPTYPE::REALSXP => "num".to_string(),
                    t if t == SEXPTYPE::INTSXP => "int".to_string(),
                    t if t == SEXPTYPE::LGLSXP => "logi".to_string(),
                    t if t == SEXPTYPE::STRSXP => "chr".to_string(),
                    t if t == SEXPTYPE::VECSXP => "list".to_string(),
                    _ => "obj".to_string(),
                }
            };
            let preview = if elem.is_null() {
                "NULL".to_string()
            } else {
                let m = XLENGTH(elem).min(3);
                let parts: Vec<String> = (0..m).map(|j| elt_to_string(elem, j)).collect();
                format!("{}: {}", type_str, parts.join(" "))
            };
            println!("{}\n{}", name, preview);
        }

        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

pub(crate) fn quantile_type7(sorted: &[f64], prob: f64) -> f64 {
    if sorted.is_empty() {
        return NA_REAL;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let h = 1.0 + (sorted.len() as f64 - 1.0) * prob;
    let lo = h.floor() as usize;
    let hi = h.ceil() as usize;
    let frac = h - lo as f64;
    let lower = sorted[lo.saturating_sub(1)];
    let upper = sorted[hi.saturating_sub(1)];
    lower + frac * (upper - lower)
}

unsafe fn named_summary_result(ty: SEXPTYPE, names: &[&str]) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(ty, names.len() as R_xlen_t);
        if !result.is_null() {
            let _result_guard = protect(result);
            let owned_names = names
                .iter()
                .map(|name| (*name).to_string())
                .collect::<Vec<_>>();
            set_string_names(result, &owned_names);
            set_summary_default_class(result);
        }
        result
    }
}

unsafe fn summary_tagged_int(args: SEXP, name: &str, default: i32) -> i32 {
    unsafe {
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            if tag_name(cell).as_deref() == Some(name) {
                let v = crate::main::coerce::asInteger(CAR(cell));
                if v != NA_INTEGER {
                    return v;
                }
            }
            cell = CDR(cell);
        }
        default
    }
}


unsafe fn summary_factor_result(x: SEXP, levels: Vec<String>, maxsum: i32) -> SEXP {
    unsafe {
        let mut counts = vec![0_i32; levels.len()];
        let mut na_count = 0_i32;

        for i in 0..XLENGTH(x) {
            let code = *INTEGER(x).add(i as usize);
            if code == NA_INTEGER || code <= 0 || code as usize > levels.len() {
                na_count += 1;
            } else {
                counts[(code - 1) as usize] += 1;
            }
        }

        let mut maxsum = maxsum.max(1);
        if na_count > 0 {
            maxsum -= 1;
        }
        let mut pairs: Vec<(String, i32)> = levels.into_iter().zip(counts).collect();
        if pairs.len() as i32 > maxsum && maxsum >= 1 {
            pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            let keep = (maxsum as usize).saturating_sub(1);
            let other: i32 = pairs.iter().skip(keep).map(|p| p.1).sum();
            pairs.truncate(keep);
            pairs.push(("(Other)".to_string(), other));
        }
        if na_count > 0 {
            pairs.push(("NAs".to_string(), na_count));
        }

        let result = Rf_allocVector3(SEXPTYPE::INTSXP, pairs.len() as R_xlen_t);
        if result.is_null() {
            return result;
        }
        let _result_guard = protect(result);
        let mut names = Vec::with_capacity(pairs.len());
        for (i, (name, count)) in pairs.iter().enumerate() {
            *INTEGER(result).add(i) = *count;
            names.push(name.clone());
        }
        set_string_names(result, &names);
        result
    }
}


pub unsafe fn format_summary_warnings(x: SEXP) -> String {
    unsafe {
        let n = if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::VECSXP {
            0
        } else {
            XLENGTH(x)
        };
        if n == 0 {
            return "No warnings\n".to_string();
        }
        let names = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        let counts = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"counts".as_ptr()));
        let count_of = |i: R_xlen_t| -> i64 {
            if !counts.is_null()
                && counts != R_NilValue()
                && TYPEOF(counts) == SEXPTYPE::INTSXP
                && i < XLENGTH(counts)
            {
                *INTEGER(counts).add(i as usize) as i64
            } else {
                1
            }
        };
        let mut total = 0i64;
        let mut lines: Vec<(i64, String, String)> = Vec::new();
        for i in 0..n {
            let msg = if !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == SEXPTYPE::STRSXP
                && i < XLENGTH(names)
            {
                let s = STRING_ELT(names, i);
                if s.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(CHAR(s))
                        .to_string_lossy()
                        .into_owned()
                }
            } else {
                String::new()
            };
            let call = VECTOR_ELT(x, i);
            let dcall = if call.is_null() || call == R_NilValue() {
                String::new()
            } else {
                crate::mainutils::errors::warning_dcall(call)
            };
            let count = count_of(i);
            total += count;
            lines.push((count, dcall, msg));
        }
        let mut out = String::new();
        if lines.len() == 1 {
            let (_, dcall, msg) = &lines[0];
            out.push_str(&format!("{total} identical warnings:\n"));
            if dcall.is_empty() {
                out.push_str(msg);
                out.push('\n');
            } else {
                out.push_str(&format!("In {dcall} : {msg}\n"));
            }
        } else {
            out.push_str(&format!(
                "Summary of (a total of {total}) warning messages:\n"
            ));
            for (count, dcall, msg) in &lines {
                if dcall.is_empty() {
                    out.push_str(&format!("{count}x : {msg}\n"));
                } else {
                    out.push_str(&format!("{count}x : In {dcall} : {msg}\n"));
                }
            }
        }
        out
    }
}

pub unsafe fn emit_summary_warnings(x: SEXP) {
    unsafe {
        let text = format_summary_warnings(x);
        for line in text.lines() {
            str_emit_line(line);
        }
    }
}


unsafe fn summary_warnings(x: SEXP) -> SEXP {
    unsafe {
        let n = if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::VECSXP {
            0
        } else {
            XLENGTH(x)
        };
        let names = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        let mut keys: Vec<String> = Vec::new();
        let mut idxs: Vec<R_xlen_t> = Vec::new();
        let mut counts: Vec<i32> = Vec::new();
        for i in 0..n {
            let msg = if !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == SEXPTYPE::STRSXP
                && i < XLENGTH(names)
            {
                let s = STRING_ELT(names, i);
                if s.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(CHAR(s))
                        .to_string_lossy()
                        .into_owned()
                }
            } else {
                String::new()
            };
            let call = VECTOR_ELT(x, i);
            let dcall = if call.is_null() || call == R_NilValue() {
                String::new()
            } else {
                crate::mainutils::errors::warning_dcall(call)
            };
            let key = format!("{dcall} |<:>| {msg}");
            if let Some(pos) = keys.iter().position(|k| k == &key) {
                counts[pos] += 1;
            } else {
                keys.push(key);
                idxs.push(i);
                counts.push(1);
            }
        }
        let out_n = idxs.len() as i64;
        let out = Rf_allocVector3(SEXPTYPE::VECSXP, out_n);
        let _out = protect(out);
        let out_names = Rf_allocVector3(SEXPTYPE::STRSXP, out_n);
        let _out_names = protect(out_names);
        let out_counts = Rf_allocVector3(SEXPTYPE::INTSXP, out_n);
        let _out_counts = protect(out_counts);
        for (j, &i) in idxs.iter().enumerate() {
            SET_VECTOR_ELT(out, j as R_xlen_t, VECTOR_ELT(x, i));
            if !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == SEXPTYPE::STRSXP
                && i < XLENGTH(names)
            {
                SET_STRING_ELT(out_names, j as R_xlen_t, STRING_ELT(names, i));
            }
            *INTEGER(out_counts).add(j) = counts[j];
        }
        crate::sexp::attrib_core::setAttrib(
            out,
            crate::sexp::attrib_core::R_NamesSymbol(),
            out_names,
        );
        crate::sexp::attrib_core::setAttrib(out, Rf_install(c"counts".as_ptr()), out_counts);
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _class = protect(class);
        SET_STRING_ELT(
            class,
            0,
            crate::sexp::constructors::Rf_mkChar(c"summary.warnings".as_ptr()),
        );
        crate::sexp::attrib_core::setAttrib(out, crate::sexp::attrib_core::R_ClassSymbol(), class);
        out
    }
}




/// R's `summary.default(x)`: return GNU R-shaped summaryDefault/table vectors.
pub unsafe fn do_summary_default(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let class = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        if !class.is_null()
            && class != R_NilValue()
            && TYPEOF(class) == SEXPTYPE::STRSXP
            && XLENGTH(class) > 0
        {
            let ncl = XLENGTH(class);
            for i in 0..ncl {
                let s = STRING_ELT(class, i);
                if s.is_null() {
                    continue;
                }
                let name = std::ffi::CStr::from_ptr(CHAR(s)).to_string_lossy();
                if name == "warnings" || name == "summary.warnings" {
                    return summary_warnings(x);
                }

                if name == "manova" || name == "maov" {
                    return crate::mainutils::essentials::do_summary_manova(_call, _op, args, _rho);
                }
                if name == "lm" || name == "aov" {
                    return crate::mainutils::essentials::do_summary_lm(_call, _op, args, _rho);
                }
                if name == "data.frame" {
                    return do_summary_data_frame(_call, _op, args, _rho);
                }
            }
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        let maxsum = summary_tagged_int(args, "maxsum", 100);

        if let Some(levels) = aggregate_factor_levels(x) {
            return summary_factor_result(x, levels, maxsum);
        }


        if t == SEXPTYPE::REALSXP || t == SEXPTYPE::INTSXP {
            let mut vals: Vec<f64> = Vec::new();
            let mut na_count = 0_i32;
            for i in 0..n {
                let v = if t == SEXPTYPE::REALSXP {
                    *REAL(x).add(i as usize)
                } else {
                    let iv = *INTEGER(x).add(i as usize);
                    if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
                };
                if v.to_bits() == R_NA_BIT_PATTERN || v.is_nan() {
                    na_count += 1;
                } else {
                    vals.push(v);
                }
            }
            let names: Vec<&str> = if na_count > 0 {
                vec![
                    "Min.", "1st Qu.", "Median", "Mean", "3rd Qu.", "Max.", "NAs",
                ]
            } else {
                vec!["Min.", "1st Qu.", "Median", "Mean", "3rd Qu.", "Max."]
            };
            let result = named_summary_result(SEXPTYPE::REALSXP, &names);
            if result.is_null() {
                return result;
            }
            let _result_guard = protect(result);
            vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let values = if vals.is_empty() {
                vec![NA_REAL, NA_REAL, NA_REAL, f64::NAN, NA_REAL, NA_REAL]
            } else {
                vec![
                    vals[0],
                    quantile_type7(&vals, 0.25),
                    quantile_type7(&vals, 0.5),
                    vals.iter().sum::<f64>() / vals.len() as f64,
                    quantile_type7(&vals, 0.75),
                    vals[vals.len() - 1],
                ]
            };
            for (i, value) in values.iter().enumerate() {
                *REAL(result).add(i) = *value;
            }
            if na_count > 0 {
                *REAL(result).add(6) = na_count as f64;
            }
            return result;
        }

        if t == SEXPTYPE::LGLSXP {
            let mut false_count = 0_i32;
            let mut true_count = 0_i32;
            let mut na_count = 0_i32;
            for i in 0..n {
                match *LOGICAL(x).add(i as usize) {
                    TRUE => true_count += 1,
                    FALSE => false_count += 1,
                    _ => na_count += 1,
                }
            }
            let mut names = vec!["Mode"];
            if false_count > 0 {
                names.push("FALSE");
            }
            if true_count > 0 {
                names.push("TRUE");
            }
            if na_count > 0 {
                names.push("NAs");
            }
            let result = named_summary_result(SEXPTYPE::STRSXP, &names);
            if result.is_null() {
                return result;
            }
            let _result_guard = protect(result);
            let mut i = 0;
            SET_STRING_ELT(result, i, Rf_mkChar(c"logical".as_ptr()));
            i += 1;
            if false_count > 0 {
                let false_text = CString::new(false_count.to_string()).unwrap_or_default();
                SET_STRING_ELT(result, i, Rf_mkChar(false_text.as_ptr()));
                i += 1;
            }
            if true_count > 0 {
                let true_text = CString::new(true_count.to_string()).unwrap_or_default();
                SET_STRING_ELT(result, i, Rf_mkChar(true_text.as_ptr()));
                i += 1;
            }
            if na_count > 0 {
                let na_text = CString::new(na_count.to_string()).unwrap_or_default();
                SET_STRING_ELT(result, i, Rf_mkChar(na_text.as_ptr()));
            }
            return result;
        }

        if t == SEXPTYPE::STRSXP {
            let mut unique = BTreeSet::new();
            let mut blank_count = 0_i32;
            let mut min_chars: Option<usize> = None;
            let mut max_chars: Option<usize> = None;
            let mut na_count = 0_i32;
            for i in 0..n {
                let value = STRING_ELT(x, i);
                if value.is_null() || value == crate::sexp::globals::R_NaString() {
                    na_count += 1;
                    continue;
                }
                let text = CStr::from_ptr(CHAR(value)).to_string_lossy().into_owned();
                if text.is_empty() {
                    blank_count += 1;
                }
                let chars = text.chars().count();
                min_chars = Some(min_chars.map_or(chars, |current| current.min(chars)));
                max_chars = Some(max_chars.map_or(chars, |current| current.max(chars)));
                unique.insert(text);
            }
            let names: Vec<&str> = if na_count > 0 {
                vec![
                    "Length",
                    "N.unique",
                    "N.blank",
                    "Min.nchar",
                    "Max.nchar",
                    "NAs",
                ]
            } else {
                vec!["Length", "N.unique", "N.blank", "Min.nchar", "Max.nchar"]
            };
            let result = named_summary_result(SEXPTYPE::INTSXP, &names);
            if result.is_null() {
                return result;
            }
            let _result_guard = protect(result);
            *INTEGER(result) = n as i32;
            *INTEGER(result).add(1) = unique.len() as i32;
            *INTEGER(result).add(2) = blank_count;
            *INTEGER(result).add(3) = min_chars.map(|v| v as i32).unwrap_or(NA_INTEGER);
            *INTEGER(result).add(4) = max_chars.map(|v| v as i32).unwrap_or(NA_INTEGER);
            if na_count > 0 {
                *INTEGER(result).add(5) = na_count;
            }
            return result;
        }

        do_typeof(_call, _op, args, _rho)
    }
}

unsafe fn sexp_has_class_name(x: SEXP, class_name: &str) -> bool {
    unsafe {
        let class = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"class".as_ptr()));
        if class.is_null() || TYPEOF(class) != SEXPTYPE::STRSXP {
            return false;
        }
        (0..XLENGTH(class)).any(|i| elt_to_string(class, i) == class_name)
    }
}

/// GNU `strOptions()$digits.d` / `$vec.len` defaults.
const STR_DIGITS_D: i32 = 3;
const STR_VEC_LEN: f64 = 4.0;
const STR_OUT_DEC: *const std::os::raw::c_char = b".\0".as_ptr() as *const std::os::raw::c_char;

fn str_signif(x: f64, digits: i32) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    let exp = x.abs().log10().floor();
    let pow = 10f64.powf(f64::from(digits) - 1.0 - exp);
    (x * pow).round() / pow
}

fn str_drop0trailing(s: &str) -> String {
    if s.contains('e') || s.contains('E') {
        return s.to_string();
    }
    if let Some(_dot) = s.find('.') {
        let mut t = s.to_string();
        while t.ends_with('0') {
            t.pop();
        }
        if t.ends_with('.') {
            t.pop();
        }
        return t;
    }
    s.to_string()
}

/// GNU `format(x, trim=TRUE, drop0trailing=TRUE)` under `options(digits=digits.d)`.
unsafe fn str_format_real(x: f64) -> String {
    unsafe {
        if x.is_nan() {
            return if x.to_bits() == R_NA_BIT_PATTERN {
                "NA".to_string()
            } else {
                "NaN".to_string()
            };
        }
        let tmp = Rf_allocVector3(SEXPTYPE::REALSXP, 1);
        if tmp.is_null() {
            return x.to_string();
        }
        let _tmp = protect(tmp);
        *REAL(tmp) = x;
        let old = crate::mainutils::format::format_get_R_print();
        crate::mainutils::format::format_set_R_print(crate::mainutils::format::RPrint {
            digits: STR_DIGITS_D,
            scipen: old.scipen,
            na_width: old.na_width,
            na_width_noquote: old.na_width_noquote,
        });
        let mut w = 0;
        let mut d = 0;
        let mut e = 0;
        crate::mainutils::format::formatRealS(tmp, 1, &mut w, &mut d, &mut e, 0);
        let encoded = crate::mainutils::printutils::EncodeReal0(x, w, d, e, STR_OUT_DEC);
        crate::mainutils::format::format_set_R_print(old);
        let raw = if encoded.is_null() {
            String::new()
        } else {
            CStr::from_ptr(encoded).to_string_lossy().into_owned()
        };
        str_drop0trailing(raw.trim())
    }
}

unsafe fn str_numeric_integer_like(x: SEXP, n_check: usize) -> bool {
    unsafe {
        if TYPEOF(x) == SEXPTYPE::INTSXP {
            return true;
        }
        if TYPEOF(x) != SEXPTYPE::REALSXP {
            return false;
        }
        let n = (XLENGTH(x) as usize).min(n_check);
        for i in 0..n {
            let v = REAL_ELT(x, i as std::os::raw::c_int);
            if v.is_nan() {
                continue;
            }
            let ao = v.abs();
            if !(ao > 1e-10 || v == 0.0) || !(ao < 1e10 || v == 0.0) {
                return false;
            }
            if (v - str_signif(v, STR_DIGITS_D)).abs() > 9e-16 * ao {
                return false;
            }
        }
        true
    }
}


unsafe fn str_atomic_summary(x: SEXP) -> String {
    unsafe { str_atomic_summary_opts(x, true) }
}

unsafe fn str_atomic_summary_opts(x: SEXP, give_length: bool) -> String {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return "NULL".to_string();
        }
        if sexp_has_class_name(x, "factor") {
            return str_factor_summary(x);
        }
        if TYPEOF(x) == SEXPTYPE::LANGSXP || sexp_has_class_name(x, "formula") {
            return str_language_summary(x);
        }

        if sexp_has_class_name(x, "ts") {
            let n = XLENGTH(x);
            let tsp = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"tsp".as_ptr()));
            let (start, end) = if !tsp.is_null() && TYPEOF(tsp) == SEXPTYPE::REALSXP && XLENGTH(tsp) >= 2
            {
                (
                    str_format_real(*REAL(tsp)),
                    str_format_real(*REAL(tsp).add(1)),
                )
            } else {
                ("1".to_string(), n.to_string())
            };
            let preview = str_preview_reals_or_ints(x, 0);
            return format!("Time-Series [1:{n}] from {start} to {end}: {preview}");
        }

        let t = TYPEOF(x);
        let n = XLENGTH(x);
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let names = crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        let named = !names.is_null()
            && names != R_NilValue()
            && TYPEOF(names) == SEXPTYPE::STRSXP
            && XLENGTH(names) > 0;
        let named_prefix = if named { "Named " } else { "" };
        let type_name = match t {
            t if t == SEXPTYPE::REALSXP => "num",
            t if t == SEXPTYPE::INTSXP => "int",
            t if t == SEXPTYPE::LGLSXP => "logi",
            t if t == SEXPTYPE::STRSXP => "chr",
            t if t == SEXPTYPE::CPLXSXP => "cplx",
            t if t == SEXPTYPE::RAWSXP => "raw",
            t if t == SEXPTYPE::VECSXP => "List",
            _ => "?",
        };
        if !dim.is_null() && TYPEOF(dim) == SEXPTYPE::INTSXP && XLENGTH(dim) >= 1 {
            let dims: Vec<String> = (0..XLENGTH(dim))
                .map(|i| format!("1:{}", *INTEGER(dim).add(i as usize)))
                .collect();
            let preview = str_preview_reals_or_ints(x, 6);
            return format!("{named_prefix}{type_name} [{}] {preview}", dims.join(", "));
        }
        if n == 0 {
            return format!("{named_prefix}{type_name}(0)");
        }
        let preview = str_preview_reals_or_ints(x, 10);
        if preview.is_empty() {
            format!("{named_prefix}{type_name} [1:{n}]")
        } else if !give_length && n != 1 {
            format!("{named_prefix}{type_name}  {preview}")
        } else if n == 1 {
            format!("{named_prefix}{type_name} {preview}")
        } else {
            format!("{named_prefix}{type_name} [1:{n}] {preview}")
        }

    }
}

/// GNU str.default factor header: `Factor`/`Ord.factor`, quoted levels,
/// integer codes after the colon.
unsafe fn str_factor_summary(x: SEXP) -> String {
    unsafe {
        let ordered = sexp_has_class_name(x, "ordered");
        let kind = if ordered { "Ord.factor" } else { "Factor" };
        let sep = if ordered { "<" } else { "," };
        let levels = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"levels".as_ptr()));
        let nlev = if !levels.is_null() && TYPEOF(levels) == SEXPTYPE::STRSXP {
            XLENGTH(levels)
        } else {
            0
        };
        let quoted: Vec<String> = (0..nlev)
            .map(|i| format!("\"{}\"", elt_to_string(levels, i)))
            .collect();
        // GNU: include levels until cumulative `3 + nchar(quoted)-2` exceeds 13.
        let mut shown = 0usize;
        let mut acc = 0i32;
        for q in &quoted {
            acc += 3 + (q.len() as i32 - 2);
            shown += 1;
            if acc > 13 {
                break;
            }
        }
        if shown == 0 && nlev > 0 {
            shown = 1;
        }
        let mut level_text = quoted[..shown.min(quoted.len())].join(sep);
        if nlev > shown as i64 {
            level_text.push_str(sep);
            level_text.push_str("..");
        }



        let n = XLENGTH(x) as usize;
        let show = n.min(10);
        let mut parts = Vec::with_capacity(show);
        for i in 0..show {
            let v = INTEGER_ELT(x, i as std::os::raw::c_int);
            if v == NA_INTEGER {
                parts.push("NA".to_string());
            } else {
                parts.push(v.to_string());
            }
        }
        let mut preview = parts.join(" ");
        if n > show {
            preview.push_str(" ...");
        }
        if nlev > 0 {
            format!("{kind} w/ {nlev} levels {level_text}: {preview}")
        } else {
            format!("{kind} w/ {nlev} levels: {preview}")
        }
    }
}

unsafe fn str_language_summary(x: SEXP) -> String {
    unsafe {
        let deparsed = crate::mainutils::deparse::deparse_symbolic(x, true);
        let _deparsed = protect(deparsed);
        let mut text = String::new();
        if !deparsed.is_null() && TYPEOF(deparsed) == SEXPTYPE::STRSXP {
            for i in 0..XLENGTH(deparsed) {
                if i > 0 {
                    text.push(' ');
                }
                text.push_str(&elt_to_string(deparsed, i));
            }
        }
        let mut out = if sexp_has_class_name(x, "formula") {
            format!("Class 'formula'  language {text}")
        } else {
            format!("language {text}")
        };
        let env = crate::sexp::attrib_core::getAttrib(x, Rf_install(c".Environment".as_ptr()));
        if !env.is_null() && env != R_NilValue() {
            out.push_str("\n  .. ..- attr(*, \".Environment\")=");
            out.push_str(&str_environment_brief(env));
        }
        out
    }
}

unsafe fn str_environment_brief(env: SEXP) -> String {
    unsafe {
        if env == crate::sexp::globals::R_EmptyEnv() {
            "<environment: R_EmptyEnv>".to_string()
        } else if env == crate::sexp::globals::R_BaseEnv() {
            "<environment: base>".to_string()
        } else if env == crate::sexp::globals::R_GlobalEnv() {
            "<environment: R_GlobalEnv>".to_string()
        } else {
            "<environment: 0x0>".to_string()
        }
    }
}



unsafe fn str_preview_ints(x: SEXP, max: usize) -> String {
    unsafe {
        let n = XLENGTH(x) as usize;
        let show = n.min(max);
        let quote = TYPEOF(x) == SEXPTYPE::STRSXP;
        let mut parts = Vec::with_capacity(show);
        for i in 0..show {
            let s = elt_to_string(x, i as R_xlen_t);
            if quote {
                parts.push(format!("\"{s}\""));
            } else {
                parts.push(s);
            }
        }
        let mut text = parts.join(" ");
        if n > show {
            text.push_str(" ...");
        }
        text
    }
}

unsafe fn str_preview_reals_or_ints(x: SEXP, max: usize) -> String {
    unsafe {
        if TYPEOF(x) != SEXPTYPE::REALSXP && TYPEOF(x) != SEXPTYPE::INTSXP {
            return str_preview_ints(x, if max == 0 { 10 } else { max });
        }
        let n = XLENGTH(x) as usize;
        if n == 0 {
            return String::new();
        }
        let integer_like = str_numeric_integer_like(x, n.min(round_2_5()));
        let v_len = if integer_like {
            round_2_5()
        } else {
            (1.25 * STR_VEC_LEN).round() as usize
        };
        let show = n.min(v_len);
        let parts = if TYPEOF(x) == SEXPTYPE::REALSXP {
            str_format_real_slice(x, show)
        } else {
            (0..show)
                .map(|i| {
                    let v = INTEGER_ELT(x, i as std::os::raw::c_int);
                    if v == NA_INTEGER {
                        "NA".to_string()
                    } else {
                        v.to_string()
                    }
                })
                .collect()
        };
        let mut text = parts.join(" ");
        if n > show {
            text.push_str(" ...");
        }
        text
    }
}

/// GNU `format(object[seq_len(ile)], trim=TRUE, drop0trailing=TRUE)` under digits.d.
unsafe fn str_format_real_slice(x: SEXP, show: usize) -> Vec<String> {
    unsafe {
        let tmp = Rf_allocVector3(SEXPTYPE::REALSXP, show as R_xlen_t);
        if tmp.is_null() {
            return (0..show)
                .map(|i| str_format_real(REAL_ELT(x, i as std::os::raw::c_int)))
                .collect();
        }
        let _tmp = protect(tmp);
        for i in 0..show {
            *REAL(tmp).add(i) = REAL_ELT(x, i as std::os::raw::c_int);
        }
        let old = crate::mainutils::format::format_get_R_print();
        crate::mainutils::format::format_set_R_print(crate::mainutils::format::RPrint {
            digits: STR_DIGITS_D,
            scipen: old.scipen,
            na_width: old.na_width,
            na_width_noquote: old.na_width_noquote,
        });
        let mut w = 0;
        let mut d = 0;
        let mut e = 0;


        crate::mainutils::format::formatRealS(tmp, show as R_xlen_t, &mut w, &mut d, &mut e, 0);
        let mut parts = Vec::with_capacity(show);
        for i in 0..show {
            let encoded = crate::mainutils::printutils::EncodeReal0(
                REAL_ELT(tmp, i as std::os::raw::c_int),
                w,
                d,
                e,
                STR_OUT_DEC,
            );
            let raw = if encoded.is_null() {
                String::new()
            } else {
                CStr::from_ptr(encoded).to_string_lossy().into_owned()
            };
            parts.push(str_drop0trailing(raw.trim()));
        }
        crate::mainutils::format::format_set_R_print(old);
        parts
    }
}

fn round_2_5() -> usize {
    (2.5 * STR_VEC_LEN).round() as usize
}


/// Emit a str() line through the session output capture when one is active,
/// so interleaving with captured print output stays in order.
fn str_emit_line(line: &str) {
    if crate::sexp::output::is_capturing() {
        crate::sexp::output::capture_stdout(&format!("{line}\n"));
    } else {
        println!("{line}");
    }
}

unsafe fn str_emit_nonstandard_attrs(x: SEXP, skip: &[&str]) {
    unsafe {
        let mut attrs = crate::sexp::accessors::ATTRIB(x);
        while !attrs.is_null() && attrs != R_NilValue() {
            let tag = TAG(attrs);
            let name = if tag.is_null() || tag == R_NilValue() {
                String::new()
            } else {
                CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            };
            if !name.is_empty() && !skip.iter().any(|s| *s == name) {
                let summary = str_atomic_summary(CAR(attrs));
                let sep = if summary.starts_with("Class") || summary.starts_with("language") {
                    ""
                } else {
                    " "
                };
                str_emit_line(&format!(" - attr(*, \"{name}\")={sep}{summary}"));
            }

            attrs = CDR(attrs);
        }
    }
}


/// R's `str(x)` — compact structure display.
pub unsafe fn do_str(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut x = CAR(args);
        if TYPEOF(x) == SEXPTYPE::PROMSXP {
            x = crate::sexp::envir::forcePromise(x);
        }
        if x.is_null() || x == R_NilValue() {
            str_emit_line(" NULL");
            return R_NilValue();
        }
        let t = TYPEOF(x);
        let n = crate::sexp::constructors::Rf_length(x) as R_xlen_t;

        // str.default for is.language && !is.expression: prints
        // " language "/" symbol " followed by the deparsed object
        // (braced blocks collapsed to "{ ... }" on one line). The generic
        // vector path below must not see these: XLENGTH of a pairlist node
        // is garbage.
        if sexp_has_class_name(x, "formula") {
            str_emit_line(&str_language_summary(x));
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }
        if t == SEXPTYPE::LANGSXP || t == SEXPTYPE::SYMSXP || t == SEXPTYPE::EXPRSXP {
            // str.default for expressions: "  expression(...)" with at most
            // three elements shown (round(0.75 * vec.len)) and " ..." when
            // truncated.
            if t == SEXPTYPE::EXPRSXP {
                let n_expr = XLENGTH(x);
                let show = n_expr.min(3);
                let _sub_guard;
                let deparse_target = if show == n_expr {
                    x
                } else {
                    let sub = Rf_allocVector3(SEXPTYPE::EXPRSXP, show);
                    _sub_guard = protect(sub);
                    for i in 0..show {
                        SET_VECTOR_ELT(sub, i, VECTOR_ELT(x, i));
                    }
                    sub
                };
                let deparsed = crate::mainutils::deparse::deparse_symbolic(deparse_target, true);
                let _deparsed_guard = protect(deparsed);
                let mut text = String::new();
                for i in 0..XLENGTH(deparsed) {
                    if i > 0 {
                        text.push(' ');
                    }
                    text.push_str(&elt_to_string(deparsed, i));
                }
                if show < n_expr {
                    text.push_str(" ...");
                }
                str_emit_line(&format!("  {text}"));
                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
                return x;
            }
            let deparsed = crate::mainutils::deparse::deparse_symbolic(x, true);
            let _deparsed_guard = protect(deparsed);
            let mut lines: Vec<String> = (0..XLENGTH(deparsed))
                .map(|i| elt_to_string(deparsed, i))
                .collect();
            let last = lines.len().saturating_sub(1);
            if t == SEXPTYPE::LANGSXP
                && lines.len() > 1
                && lines[0].trim() == "{"
                && lines[last].trim() == "}"
                && lines.len() >= 3
            {
                // str.default: trimEnds each middle line (leading space run
                // becomes a single space, trailing spaces stripped) and join
                // with ";".
                let middles: Vec<String> = lines[1..last]
                    .iter()
                    .map(|l| {
                        let body = l.trim_start_matches(' ');
                        let lead = l.len() - body.len();
                        let trimmed = if lead > 0 {
                            format!(" {body}")
                        } else {
                            body.to_string()
                        };
                        trimmed.trim_end_matches(' ').to_string()
                    })
                    .collect();
                lines = vec![lines[0].clone(), middles.join(";"), lines[last].clone()];
            }
            let prefix = if t == SEXPTYPE::LANGSXP {
                let mode_chars =
                    crate::eval::attrib_core::language_implicit_class_chars(x);
                let mode = std::ffi::CStr::from_ptr(CHAR(mode_chars)).to_string_lossy();
                if mode.as_ref() == "(" {
                    " language, mode \"(\":".to_string()
                } else {
                    " language".to_string()
                }
            } else {
                " symbol".to_string()
            };
            str_emit_line(&format!("{} {}", prefix, lines.join(" ")));

            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }

        if t == SEXPTYPE::VECSXP {
            let names = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"names".as_ptr()));
            let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP;
            let class = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"class".as_ptr()));
            let is_df = if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
                (0..XLENGTH(class)).any(|i| elt_to_string(class, i) == "data.frame")
            } else {
                false
            };

            if is_df {
                let ncol = n;
                let nrow = if ncol > 0 {
                    let first = VECTOR_ELT(x, 0);
                    if first.is_null() {
                        0
                    } else {
                        XLENGTH(first)
                    }
                } else {
                    0
                };
                let extra_classes: Vec<String> = if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
                    (0..XLENGTH(class))
                        .map(|i| elt_to_string(class, i))
                        .filter(|c| c != "data.frame")
                        .collect()
                } else {
                    Vec::new()
                };
                let var_word = if ncol == 1 { "variable" } else { "variables" };
                let colon = if ncol > 0 { ":" } else { "" };
                if extra_classes.is_empty() {
                    str_emit_line(&format!(
                        "'data.frame':\t{nrow} obs. of  {ncol} {var_word}{colon}"
                    ));
                } else {
                    let quoted: Vec<String> =
                        extra_classes.iter().map(|c| format!("'{c}'")).collect();
                    str_emit_line(&format!(
                        "Classes {} and 'data.frame':\t{nrow} obs. of  {ncol} {var_word}{colon}",
                        quoted.join(", ")
                    ));
                }
                let raw_names: Vec<String> = (0..ncol)
                    .map(|i| {
                        if has_names && i < XLENGTH(names) {
                            elt_to_string(names, i)
                        } else {
                            format!("{}", i + 1)
                        }
                    })
                    .collect();
                let name_width = raw_names.iter().map(String::len).max().unwrap_or(0);
                for i in 0..ncol {
                    let name = format!("{:<name_width$}", raw_names[i as usize]);
                    let elem = VECTOR_ELT(x, i as i64);
                    str_emit_line(&format!(
                        " $ {name}: {}",
                        str_atomic_summary_opts(elem, false)
                    ));
                }
                str_emit_nonstandard_attrs(x, &["names", "class", "row.names"]);
            } else {
                str_emit_line(&format!("List of {n}"));
                for i in 0..n.min(6) {
                    let name = if has_names && i < XLENGTH(names) {
                        elt_to_string(names, i)
                    } else {
                        format!("[[{}]]", i + 1)
                    };
                    let elem = VECTOR_ELT(x, i as i64);
                    str_emit_line(&format!(" $ {name}: {}", str_atomic_summary(elem)));
                }
            }
        } else {
            str_emit_line(&format!(" {}", str_atomic_summary(x)));
            let names = crate::sexp::attrib_core::getAttrib(
                x,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            if !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == SEXPTYPE::STRSXP
                && XLENGTH(names) > 0
            {
                str_emit_line(&format!(
                    " - attr(*, \"names\")= {}",
                    str_atomic_summary(names)
                ));
            }
        }



        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

// ---------------------------------------------------------------------------
// S3 print/summary dispatch
// ---------------------------------------------------------------------------

/// R's `print.default(x, ...)` — default print method.
/// Equivalent to the existing do_print but named for S3 dispatch.
pub unsafe fn do_print_default(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_print(_call, _op, args, _rho) }
}

/// GNU `print.Date(x, max = NULL, ...)`.
pub unsafe fn do_print_Date(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return R_NilValue();
        }
        let mut max = None;
        let max_sym = Rf_install(c"max".as_ptr());
        let mut p = CDR(args);
        while !p.is_null() && p != R_NilValue() {
            if TAG(p) == max_sym {
                let v = CAR(p);
                if !v.is_null() && v != R_NilValue() {
                    if TYPEOF(v) == SEXPTYPE::INTSXP && XLENGTH(v) > 0 {
                        max = Some(*INTEGER(v) as i64);
                    } else if TYPEOF(v) == SEXPTYPE::REALSXP && XLENGTH(v) > 0 {
                        max = Some(*REAL(v) as i64);
                    }
                }
                break;
            }
            p = CDR(p);
        }
        if let Some(sexp) = crate::sexp::object::Sexp::from_raw(x) {
            let text = crate::sexp::output::format_date_vector_max(sexp, max);
            if crate::sexp::output::is_capturing() {
                crate::sexp::output::capture_stdout(&format!("{text}\n"));
            } else {
                println!("{text}");
            }
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// GNU `print.POSIXct(x, ..., usetz = TRUE, digits = getOption("digits.secs"))`.
pub unsafe fn do_print_POSIXct(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || XLENGTH(x) == 0 {
            let text = "POSIXct of length 0";
            if crate::sexp::output::is_capturing() {
                crate::sexp::output::capture_stdout(&format!("{text}\n"));
            } else {
                println!("{text}");
            }
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return if x.is_null() || x == R_NilValue() {
                R_NilValue()
            } else {
                x
            };
        }

        let mut max = None;
        let mut width = None;
        let max_sym = Rf_install(c"max".as_ptr());
        let digits_sym = Rf_install(c"digits".as_ptr());
        let usetz_sym = Rf_install(c"usetz".as_ptr());
        let width_sym = Rf_install(c"width".as_ptr());
        let mut digits = R_NilValue();
        let mut usetz_val = TRUE;
        let mut p = CDR(args);
        while !p.is_null() && p != R_NilValue() {
            if TAG(p) == max_sym {
                let v = CAR(p);
                if !v.is_null() && v != R_NilValue() {
                    if TYPEOF(v) == SEXPTYPE::INTSXP && XLENGTH(v) > 0 {
                        max = Some(*INTEGER(v) as i64);
                    } else if TYPEOF(v) == SEXPTYPE::REALSXP && XLENGTH(v) > 0 {
                        max = Some(*REAL(v) as i64);
                    }
                }
            } else if TAG(p) == digits_sym {
                digits = CAR(p);
            } else if TAG(p) == usetz_sym {
                let v = CAR(p);
                if TYPEOF(v) == SEXPTYPE::LGLSXP && XLENGTH(v) > 0 {
                    usetz_val = *INTEGER(v);
                }
            } else if TAG(p) == width_sym {
                let v = CAR(p);
                if TYPEOF(v) == SEXPTYPE::INTSXP && XLENGTH(v) > 0 {
                    width = Some(*INTEGER(v));
                } else if TYPEOF(v) == SEXPTYPE::REALSXP && XLENGTH(v) > 0 {
                    width = Some(*REAL(v) as i32);
                }
            }
            p = CDR(p);
        }

        let usetz = Rf_ScalarLogical(usetz_val);
        let _u = protect(usetz);
        let mut rest = Rf_cons(usetz, R_NilValue());
        SETTAG(rest, usetz_sym);
        let _r0 = protect(rest);
        if !digits.is_null() && digits != R_NilValue() {
            let cell = Rf_cons(digits, rest);
            SETTAG(cell, digits_sym);
            rest = cell;
            let _rd = protect(rest);
        }

        let fmt_args = Rf_cons(x, rest);
        let _fa = protect(fmt_args);
        let formatted = crate::mainutils::datetime::do_format_POSIXct(call, op, fmt_args, rho);
        let _f = protect(formatted);
        if let Some(sexp) = crate::sexp::object::Sexp::from_raw(formatted) {
            let print_max = max.unwrap_or_else(|| {
                crate::mainutils::options::GetOptionMaxPrint() as i64
            });
            let old_width = width.map(|w| crate::mainutils::options::R_SetOptionWidth(w));
            let text = crate::sexp::output::format_vector_stock_n(sexp, true, Some(print_max));
            if let Some(old) = old_width {
                crate::mainutils::options::R_SetOptionWidth(old);
            }
            if crate::sexp::output::is_capturing() {
                crate::sexp::output::capture_stdout(&format!("{text}\n"));
            } else {
                println!("{text}");
            }
        }

        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}




fn print_data_frame_show_row_names(args: SEXP) -> bool {
    unsafe {
        let row_names_sym = Rf_install(c"row.names".as_ptr());
        let mut arg = CDR(args);
        while !arg.is_null() && arg != R_NilValue() {
            if TAG(arg) == row_names_sym {
                let value = CAR(arg);
                if TYPEOF(value) == SEXPTYPE::LGLSXP {
                    let data = LOGICAL(value);
                    if !data.is_null() {
                        return *data != FALSE;
                    }
                }
            }
            arg = CDR(arg);
        }
        true
    }
}

fn print_data_frame_column_texts(
    x: SEXP,
    ncol: R_xlen_t,
    nrow: R_xlen_t,
) -> (Vec<String>, Vec<Vec<String>>) {
    unsafe {
        let names = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"names".as_ptr()));
        let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP;
        let mut headers = Vec::with_capacity(ncol as usize);
        let mut columns = Vec::with_capacity(ncol as usize);
        for j in 0..ncol {
            let header = if has_names && j < XLENGTH(names) {
                elt_to_string(names, j)
            } else {
                format!("[,{}]", j + 1)
            };
            headers.push(header);
            let col = VECTOR_ELT(x, j as R_xlen_t);
            let mut values = Vec::with_capacity(nrow as usize);
            for i in 0..nrow {
                let val = if col.is_null() {
                    "NULL".to_string()
                } else {
                    elt_to_string(col, i)
                };
                values.push(val);
            }
            columns.push(values);
        }
        (headers, columns)
    }
}

fn emit_print_data_frame_line(line: &str) {
    if crate::sexp::output::is_capturing() {
        crate::sexp::output::capture_stdout(&format!("{line}\n"));
    } else {
        println!("{line}");
    }
}

/// Derive the row labels of a data.frame for printing.
///
/// Mirrors stock `print.data.frame`: automatic compact row names (`c(NA, n)`
/// stored as a length-2 integer vector) expand to `1..n`; explicit integer or
/// character `row.names` are used verbatim.
fn data_frame_row_labels(x: SEXP, nrow: R_xlen_t) -> Vec<String> {
    let row_names =
        unsafe { crate::sexp::attrib_core::getAttrib(x, Rf_install(c"row.names".as_ptr())) };
    unsafe {
        if !row_names.is_null() {
            let t = TYPEOF(row_names);
            if t == SEXPTYPE::STRSXP && XLENGTH(row_names) == nrow {
                return (0..nrow).map(|i| elt_to_string(row_names, i)).collect();
            }
            if t == SEXPTYPE::INTSXP && XLENGTH(row_names) == nrow {
                // Compact automatic row names are stored as c(NA_integer_, n);
                // only a full-length vector of real integers names the rows.
                let first = *INTEGER(row_names);
                let is_compact = XLENGTH(row_names) == 2 && first == crate::sexp::ffi::NA_INTEGER;
                if !is_compact {
                    return (0..nrow)
                        .map(|i| INTEGER(row_names).add(i as usize).read().to_string())
                        .collect();
                }
            }
        }
    }
    (1..=nrow).map(|i| i.to_string()).collect()
}

/// R's `print.data.frame(x)` — print a data.frame nicely with aligned columns.
pub unsafe fn do_print_data_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            emit_print_data_frame_line("NULL");
            return R_NilValue();
        }
        if TYPEOF(x) != SEXPTYPE::VECSXP {
            return do_print(_call, _op, args, _rho);
        }
        let ncol = XLENGTH(x);
        let nrow = if ncol > 0 {
            let first = VECTOR_ELT(x, 0);
            if first.is_null() { 0 } else { XLENGTH(first) }
        } else {
            0
        };

        let show_row_names = print_data_frame_show_row_names(args);
        let (headers, columns) = print_data_frame_column_texts(x, ncol, nrow);
        let row_labels = data_frame_row_labels(x, nrow);
        let row_width = row_labels
            .iter()
            .map(|label| label.len())
            .max()
            .unwrap_or(0)
            .max(1);
        let widths: Vec<usize> = headers
            .iter()
            .zip(&columns)
            .map(|(header, values)| {
                values
                    .iter()
                    .fold(header.len(), |max, value| max.max(value.len()))
            })
            .collect();

        if !headers.is_empty() {
            let header = headers
                .iter()
                .enumerate()
                .map(|(idx, name)| format!("{:>width$}", name, width = widths[idx]))
                .collect::<Vec<_>>()
                .join(" ");
            // With row labels shown, stock pads the header by the label
            // column width plus one separator; with row.names = FALSE the
            // label column is empty strings, leaving exactly one separator.
            let label_pad = if show_row_names {
                " ".repeat(row_width)
            } else {
                String::new()
            };
            emit_print_data_frame_line(&format!("{label_pad} {header}"));
        }

        let print_rows = nrow.min(100) as usize; // increased for better visibility/polish (was 20 hard cap per review feedback on df print); R uses max.print option
        for row in 0..print_rows {
            let mut cells = Vec::with_capacity(headers.len() + usize::from(show_row_names));
            // Stock left-justifies row labels (auto 1..n, explicit numeric,
            // and character row names alike) inside the label column; a hidden
            // label column still contributes its separator space.
            if show_row_names {
                cells.push(format!(
                    "{:<row_width$}",
                    row_labels.get(row).map(String::as_str).unwrap_or("")
                ));
            } else {
                cells.push(String::new());
            }
            for (idx, values) in columns.iter().enumerate() {
                let value = values.get(row).map(String::as_str).unwrap_or("");
                cells.push(format!("{:>width$}", value, width = widths[idx]));
            }
            emit_print_data_frame_line(&cells.join(" "));
        }
        if nrow > 20 {
            emit_print_data_frame_line(&format!(
                "  [ reached 'max' / getOption(\"max.print\") -- omitted {} rows ]",
                nrow - 20
            ));
        }

        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `print.table(x)` — print a table object.
pub unsafe fn do_print_table(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("NULL");
            return R_NilValue();
        }
        // Table objects are typically arrays (REALSXP/INTSXP with dim attribute)
        let t = TYPEOF(x);
        let dim_attr = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"dim".as_ptr()));

        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) == 2 {
            // 2D table: print as matrix
            let nrow = *INTEGER(dim_attr) as usize;
            let ncol = *INTEGER(dim_attr).add(1) as usize;

            // Get dimnames
            let dn = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"dimnames".as_ptr()));
            let has_dn = !dn.is_null() && TYPEOF(dn) == SEXPTYPE::VECSXP;

            // Print row names and values
            for i in 0..nrow {
                let rname = if has_dn && !VECTOR_ELT(dn, 0).is_null() {
                    elt_to_string(VECTOR_ELT(dn, 0), i as R_xlen_t)
                } else {
                    format!("{}", i + 1)
                };
                print!("{:>12} ", rname);
                for j in 0..ncol {
                    let idx = i * ncol + j;
                    let val = if t == SEXPTYPE::REALSXP {
                        format!("{:>6}", *REAL(x).add(idx))
                    } else if t == SEXPTYPE::INTSXP {
                        format!("{:>6}", *INTEGER(x).add(idx))
                    } else {
                        format!("{:>6}", elt_to_string(x, idx as R_xlen_t))
                    };
                    print!("{}", val);
                }
                println!();
            }
            // Print column names
            if has_dn && !VECTOR_ELT(dn, 1).is_null() {
                print!("{:>12} ", "");
                for j in 0..ncol {
                    print!("{:>6}", elt_to_string(VECTOR_ELT(dn, 1), j as R_xlen_t));
                }
                println!();
            }
        } else {
            let n = XLENGTH(x);
            let mut names =
                crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
            if names.is_null() || TYPEOF(names) != SEXPTYPE::STRSXP {
                let dn = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"dimnames".as_ptr()));
                if !dn.is_null() && TYPEOF(dn) == SEXPTYPE::VECSXP && XLENGTH(dn) >= 1 {
                    names = VECTOR_ELT(dn, 0);
                }
            }
            let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP;
            if has_names {
                let labels: Vec<String> = (0..n).map(|i| elt_to_string(names, i)).collect();
                let values: Vec<String> = (0..n)
                    .map(|i| {
                        if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                            (*INTEGER(x).add(i as usize)).to_string()
                        } else if t == SEXPTYPE::REALSXP {
                            format!("{}", *REAL(x).add(i as usize))
                        } else {
                            elt_to_string(x, i)
                        }
                    })
                    .collect();
                let width = labels
                    .iter()
                    .zip(&values)
                    .map(|(name, value)| name.len().max(value.len()))
                    .max()
                    .unwrap_or(1);
                let name_line = labels
                    .iter()
                    .map(|name| format!("{name:>width$}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let value_line = values
                    .iter()
                    .map(|value| format!("{value:>width$}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                println!();
                // GNU print.array of a 1-d table: a space after the last
                // column, so both header and count lines gain a trailing space.
                println!("{name_line} ");
                println!("{value_line} ");

                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
                return x;
            }
            for i in 0..n {
                let val = elt_to_string(x, i);
                println!("  {}", val);
            }
        }

        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// GNU `print.factor`: `as.character` then `print(..., quote=FALSE)`, which
/// `format()`s labels to the widest field (`<NA>` is 4), then `Levels:`.
pub unsafe fn do_print_factor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("NULL");
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let levels = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"levels".as_ptr()));
        let has_levels = !levels.is_null() && TYPEOF(levels) == SEXPTYPE::STRSXP;

        if n == 0 {
            println!("factor(0)");
        } else {
            let t = TYPEOF(x);
            let mut labels: Vec<String> = Vec::with_capacity(n as usize);
            for i in 0..n {
                let val = if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                    let v = *INTEGER(x).add(i as usize);
                    if v == NA_INTEGER {
                        "<NA>".to_string()
                    } else if has_levels && (v as R_xlen_t) <= XLENGTH(levels) && v > 0 {
                        let idx = (v - 1) as R_xlen_t;
                        let charsxp = STRING_ELT(levels, idx);
                        if charsxp == crate::sexp::globals::R_NaString() {
                            "<NA>".to_string()
                        } else {
                            elt_to_string(levels, idx)
                        }
                    } else {
                        format!("{v}")
                    }
                } else {
                    elt_to_string(x, i)
                };
                labels.push(val);
            }
            let width = labels
                .iter()
                .map(|s| s.chars().count())
                .max()
                .unwrap_or(0);
            for (i, val) in labels.iter().enumerate() {
                let padded = format!("{val:<width$}");
                if i == 0 {
                    print!("[1] {padded}");
                } else {
                    print!(" {padded}");
                }
            }
            println!();

            if has_levels {
                let nl = XLENGTH(levels);
                print!("Levels:");
                for i in 0..nl {
                    let charsxp = STRING_ELT(levels, i);
                    let lvl = if charsxp == crate::sexp::globals::R_NaString() {
                        "<NA>".to_string()
                    } else {
                        elt_to_string(levels, i)
                    };
                    print!(" {lvl}");
                }
                println!();
            }
        }

        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// GNU `summary.data.frame` — character table of per-column summaries.
pub unsafe fn do_summary_data_frame(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        if TYPEOF(x) != SEXPTYPE::VECSXP {
            return do_summary_default(call, op, args, rho);
        }
        let ncol = XLENGTH(x);
        let maxsum = summary_tagged_int(args, "maxsum", 7);
        let opt_digits = crate::mainutils::options::GetOptionDigits();
        let digits = summary_tagged_int(args, "digits", (opt_digits - 3).max(3));
        let col_names_attr =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());

        let mut columns: Vec<Vec<String>> = Vec::with_capacity(ncol as usize);
        let mut label_widths: Vec<usize> = Vec::with_capacity(ncol as usize);
        let mut headers: Vec<String> = Vec::with_capacity(ncol as usize);
        for j in 0..ncol {
            let header = if !col_names_attr.is_null()
                && TYPEOF(col_names_attr) == SEXPTYPE::STRSXP
                && j < XLENGTH(col_names_attr)
            {
                elt_to_string(col_names_attr, j)
            } else {
                format!("[,{}]", j + 1)
            };
            headers.push(header);
            let col = VECTOR_ELT(x, j);
            let sms = summarize_frame_column(col, maxsum, rho);
            let _sms = protect(sms);
            let (cells, lw) = format_summary_column(sms, digits, rho);

            label_widths.push(lw);
            columns.push(cells);
        }
        let nrow = columns.iter().map(Vec::len).max().unwrap_or(0);
        for col in &mut columns {
            col.resize(nrow, String::new());
        }
        let blanks = " ".repeat(label_widths.iter().copied().max().unwrap_or(0) + 2);
        for (i, header) in headers.iter_mut().enumerate() {
            let pad = ((label_widths[i] as f64) - (header.chars().count() as f64) / 2.0)
                .floor()
                .max(0.0) as usize;
            let pad = pad.min(blanks.len());
            *header = format!("{}{header}", &blanks[..pad]);
        }


        let result = Rf_allocVector3(SEXPTYPE::STRSXP, (nrow * ncol as usize) as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _r = protect(result);
        for (j, col) in columns.iter().enumerate() {
            for (i, cell) in col.iter().enumerate() {
                let cstr = CString::new(cell.as_str()).unwrap_or_default();
                SET_STRING_ELT(
                    result,
                    (j * nrow + i) as R_xlen_t,
                    crate::sexp::constructors::Rf_mkChar(cstr.as_ptr()),
                );
            }
        }
        crate::mainutils::essentials::set_two_dim_attr(result, nrow as R_xlen_t, ncol);
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _dn = protect(dn);
        let rn = Rf_allocVector3(SEXPTYPE::STRSXP, nrow as R_xlen_t);
        let _rn = protect(rn);
        for i in 0..nrow {
            SET_STRING_ELT(rn, i as R_xlen_t, crate::sexp::constructors::Rf_mkChar(c"".as_ptr()));
        }
        let cn = string_vector(&headers);
        SET_VECTOR_ELT(dn, 0, rn);
        SET_VECTOR_ELT(dn, 1, cn);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dn,
        );
        let class = Rf_mkString(c"table".as_ptr());
        let _cl = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::TRUE);
        result
    }
}

unsafe fn summarize_frame_column(col: SEXP, maxsum: i32, rho: SEXP) -> SEXP {
    unsafe {
        if col.is_null() || col == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        if let Some(levels) = aggregate_factor_levels(col) {
            return summary_factor_result(col, levels, maxsum);
        }
        let args = Rf_cons(col, R_NilValue());
        let _a = protect(args);
        do_summary_default(std::ptr::null_mut(), std::ptr::null_mut(), args, rho)
    }
}

unsafe fn format_summary_column(sms: SEXP, digits: i32, rho: SEXP) -> (Vec<String>, usize) {
    unsafe {
        if sms.is_null() || sms == R_NilValue() {
            return (Vec::new(), 0);
        }
        let names_attr =
            crate::sexp::attrib_core::getAttrib(sms, crate::sexp::attrib_core::R_NamesSymbol());
        let n = XLENGTH(sms);
        let mut labels = Vec::with_capacity(n as usize);
        for i in 0..n {
            labels.push(if !names_attr.is_null()
                && TYPEOF(names_attr) == SEXPTYPE::STRSXP
                && i < XLENGTH(names_attr)
            {
                elt_to_string(names_attr, i)
            } else {
                String::new()
            });
        }
        let lw = labels.iter().map(|s| s.chars().count()).max().unwrap_or(0);
        let labels: Vec<String> = labels
            .into_iter()
            .map(|s| format!("{s:<lw$}"))
            .collect();

        let digits_s = Rf_ScalarInteger(digits);
        let _ds = protect(digits_s);
        let rest = Rf_cons(digits_s, R_NilValue());
        SETTAG(rest, Rf_install(c"digits".as_ptr()));
        let _rest = protect(rest);
        let fmt_args = Rf_cons(sms, rest);
        let _fa = protect(fmt_args);
        let formatted = crate::mainutils::essentials::do_format(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            fmt_args,
            rho,
        );

        let _f = protect(formatted);
        let mut cells = Vec::with_capacity(n as usize);
        for i in 0..n {
            let value = if !formatted.is_null()
                && TYPEOF(formatted) == SEXPTYPE::STRSXP
                && i < XLENGTH(formatted)
            {
                elt_to_string(formatted, i)
            } else {
                String::new()
            };
            cells.push(format!("{}:{value}  ", labels.get(i as usize).cloned().unwrap_or_default()));
        }
        (cells, lw)
    }
}


/// R's `format.data.frame(x, ..., justify = "none")`.
pub unsafe fn do_format_data_frame(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        if TYPEOF(x) != SEXPTYPE::VECSXP {
            if XLENGTH(x) == 0 {
                return x;
            }
            // Return a single-column STRSXP of formatted values
            let n = XLENGTH(x);
            let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            for i in 0..n {
                let s = elt_to_string(x, i);
                let cstr = CString::new(s).unwrap_or_default();
                let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                if !charsxp.is_null() {
                    let data = (*result).gengc_next_node as *mut SEXP;
                    *data.add(i as usize) = charsxp;
                }
            }
            return result;
        }

        let rest = format_data_frame_rest_args(args);
        let _rest_guard = protect(rest);
        let out = crate::mainutils::duplicate::shallow_duplicate(x);
        if out.is_null() {
            return R_NilValue();
        }
        let _out_guard = protect(out);
        let ncol = XLENGTH(out);
        for i in 0..ncol {
            let col = VECTOR_ELT(out, i);
            let col_args = Rf_cons(col, rest);
            let _col_args_guard = protect(col_args);
            let formatted = crate::mainutils::essentials::do_format(call, op, col_args, rho);
            let formatted = mark_asis_if_character(formatted);
            SET_VECTOR_ELT(out, i, formatted);
        }
        out
    }
}

unsafe fn format_data_frame_rest_args(args: SEXP) -> SEXP {
    unsafe {
        let rest = CDR(args);
        let mut cell = rest;
        while !cell.is_null() && cell != R_NilValue() {
            if tag_name(cell).as_deref() == Some("justify") {
                return rest;
            }
            cell = CDR(cell);
        }
        let none = crate::sexp::constructors::Rf_mkString(c"none".as_ptr());
        let _none = protect(none);
        let cell = Rf_cons(none, rest);
        SETTAG(cell, Rf_install(c"justify".as_ptr()));
        cell
    }
}

unsafe fn mark_asis_if_character(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() || TYPEOF(x) != SEXPTYPE::STRSXP {
            return x;
        }
        if crate::mainutils::objects::inherits2(x, c"AsIs".as_ptr()) != FALSE {
            return x;
        }
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if class_vec.is_null() {
            return x;
        }
        let _c = protect(class_vec);
        SET_STRING_ELT(class_vec, 0, crate::sexp::constructors::Rf_mkChar(c"AsIs".as_ptr()));
        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class_vec,
        );
        x
    }
}

// ---------------------------------------------------------------------------
// S3 print dispatch — type-specific print methods
// ---------------------------------------------------------------------------

/// R's `print.integer(x)` — print integer vector with index labels.
pub unsafe fn do_print_integer(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("integer(0)");
            return R_NilValue();
        }
        let rendered = if let Some(sexp) = crate::sexp::object::Sexp::from_raw(x) {
            crate::sexp::output::format_vector_stock(sexp, true)
        } else {
            String::new()
        };
        if crate::sexp::output::is_capturing() {
            crate::sexp::output::capture_stdout(&rendered);
        } else {
            print!("{rendered}");
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `print.numeric(x)` — print numeric (double) vector with index labels.
pub unsafe fn do_print_numeric(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("numeric(0)");
            return R_NilValue();
        }
        let rendered = if let Some(sexp) = crate::sexp::object::Sexp::from_raw(x) {
            crate::sexp::output::format_vector_stock(sexp, true)
        } else {
            String::new()
        };
        if crate::sexp::output::is_capturing() {
            crate::sexp::output::capture_stdout(&rendered);
        } else {
            print!("{rendered}");
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `print.logical(x)` — print logical vector with index labels.
pub unsafe fn do_print_logical(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("logical(0)");
            return R_NilValue();
        }
        let rendered = if let Some(sexp) = crate::sexp::object::Sexp::from_raw(x) {
            crate::sexp::output::format_vector_stock(sexp, true)
        } else {
            String::new()
        };
        if crate::sexp::output::is_capturing() {
            crate::sexp::output::capture_stdout(&rendered);
        } else {
            print!("{rendered}");
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `print.character(x)` — print character vector with index labels.
pub unsafe fn do_print_character(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("character(0)");
            return R_NilValue();
        }
        let n = XLENGTH(x);
        if n == 0 {
            println!("character(0)");
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }
        let rendered = if let Some(sexp) = crate::sexp::object::Sexp::from_raw(x) {
            crate::sexp::output::format_vector_stock(sexp, true)
        } else {
            String::new()
        };
        if crate::sexp::output::is_capturing() {
            crate::sexp::output::capture_stdout(&rendered);
        } else {
            print!("{rendered}");
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `print.complex(x)` — print complex vector with index labels.
pub unsafe fn do_print_complex(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("complex(0)");
            return R_NilValue();
        }
        let rendered = if let Some(sexp) = crate::sexp::object::Sexp::from_raw(x) {
            crate::sexp::output::format_vector_stock(sexp, true)
        } else {
            String::new()
        };
        if crate::sexp::output::is_capturing() {
            crate::sexp::output::capture_stdout(&rendered);
        } else {
            print!("{rendered}");
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `print.function(x)` — print function definition.
pub unsafe fn do_print_function(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("NULL");
            return R_NilValue();
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::CLOSXP && t != SEXPTYPE::BUILTINSXP && t != SEXPTYPE::SPECIALSXP {
            return do_print(_call, _op, args, _rho);
        }
        // Print function signature
        let formals = if t == SEXPTYPE::CLOSXP {
            crate::sexp::accessors::FORMALS(x)
        } else {
            R_NilValue()
        };
        print!("function(");
        let mut first = true;
        let mut cur = formals;
        while !cur.is_null() && cur != R_NilValue() {
            if !first {
                print!(", ");
            }
            first = false;
            let tag = crate::sexp::accessors::TAG(cur);
            if !tag.is_null() {
                let pname = crate::sexp::accessors::PRINTNAME(tag);
                if !pname.is_null() {
                    let s = crate::sexp::accessors::CHAR(pname);
                    if !s.is_null() {
                        let name = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("?");
                        print!("{}", name);
                    }
                }
            }
            cur = CDR(cur);
        }
        println!(")");
        // Print body (simplified: just show it's a body)
        if t == SEXPTYPE::CLOSXP {
            let body = crate::sexp::accessors::BODY(x);
            if !body.is_null() {
                println!("{{ ... }}");
            }
        } else {
            println!("<primitive>");
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `print.environment(x)` — print environment summary.
pub unsafe fn do_print_environment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("NULL");
            return R_NilValue();
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::ENVSXP {
            return do_print(_call, _op, args, _rho);
        }
        // Print environment name
        let name = if x == crate::sexp::globals::R_GlobalEnv() {
            "R_GlobalEnv".to_string()
        } else if x == crate::sexp::globals::R_EmptyEnv() {
            "R_EmptyEnv".to_string()
        } else if x == crate::sexp::globals::R_BaseEnv() {
            "base".to_string()
        } else {
            "<environment>".to_string()
        };
        println!("<environment: {}>", name);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `enc2native(x)` — normalize character encodings to the native runtime encoding.
pub unsafe fn do_enc2native(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_enc2(args) }
}

/// R's `enc2utf8(x)` — normalize character encodings to UTF-8.
pub unsafe fn do_enc2utf8(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_enc2(args) }
}

unsafe fn do_enc2(args: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || TYPEOF(x) != SEXPTYPE::STRSXP {
            base_error("argument is not a character vector");
        }
        crate::mainutils::duplicate::duplicate(x)
    }
}

/// R's `print.formula(x)` — print formula.
pub unsafe fn do_print_formula(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("NULL");
            return R_NilValue();
        }
        // Formulas are typically LANGSXP with ~ operator
        let t = TYPEOF(x);
        if t == SEXPTYPE::LANGSXP {
            let op = CAR(x);
            if !op.is_null() {
                let pname = crate::sexp::accessors::PRINTNAME(op);
                if !pname.is_null() {
                    let s = crate::sexp::accessors::CHAR(pname);
                    if !s.is_null() {
                        let op_str = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("?");
                        if op_str == "~" {
                            // Formula: print left ~ right
                            let lhs = CAR(CDR(x));
                            let rhs = CDR(CDR(x));
                            let lhs_str = if lhs.is_null() {
                                String::new()
                            } else {
                                elt_to_string(lhs, 0)
                            };
                            let rhs_str = if rhs.is_null() {
                                String::new()
                            } else {
                                elt_to_string(CAR(rhs), 0)
                            };
                            println!("{} ~ {}", lhs_str, rhs_str);
                            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
                            return x;
                        }
                    }
                }
            }
        }
        do_print(_call, _op, args, _rho)
    }
}

/// R's `print.call(x)` — print call/language object.
pub unsafe fn do_print_call(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("NULL");
            return R_NilValue();
        }
        // Print as deparse-like output
        let s = do_deparse(_call, _op, args, _rho);
        if !s.is_null() && TYPEOF(s) == SEXPTYPE::STRSXP {
            let n = XLENGTH(s);
            for i in 0..n {
                println!("{}", elt_to_string(s, i));
            }
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `print.pairlist(x)` — print pairlist.
pub unsafe fn do_print_pairlist(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("NULL");
            return R_NilValue();
        }
        let mut cur = x;
        let mut i = 0;
        while !cur.is_null() && cur != R_NilValue() && TYPEOF(cur) == SEXPTYPE::LISTSXP {
            let tag = crate::sexp::accessors::TAG(cur);
            let val = CAR(cur);
            let name = if !tag.is_null() {
                let pname = crate::sexp::accessors::PRINTNAME(tag);
                if !pname.is_null() {
                    let s = crate::sexp::accessors::CHAR(pname);
                    if !s.is_null() {
                        std::ffi::CStr::from_ptr(s)
                            .to_str()
                            .unwrap_or("")
                            .to_string()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            let val_str = elt_to_string(val, 0);
            if name.is_empty() {
                println!("[[{}]]\n{}", i + 1, val_str);
            } else {
                println!("${}\n{}", name, val_str);
            }
            cur = CDR(cur);
            i += 1;
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `print.raw(x)` — print raw (byte) vector.
pub unsafe fn do_print_raw(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("NULL");
            return R_NilValue();
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::RAWSXP {
            // Not a raw vector, fall back to default print
            return do_print(_call, _op, args, _rho);
        }
        let n = XLENGTH(x);
        if n == 0 {
            println!("raw(0)");
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }
        let raw_ptr = RAW(x);
        let mut parts: Vec<String> = Vec::new();
        let display_n = n.min(999);
        for i in 0..display_n {
            let byte = *raw_ptr.add(i as usize);
            parts.push(format!("{:02x}", byte));
        }
        if n > 999 {
            parts.push("...".to_string());
        }
        // Print in R's raw vector style: [1] "00" "ff" "ab" ...
        let mut line = String::from("[1] ");
        for (i, p) in parts.iter().enumerate() {
            if i > 0 {
                line.push(' ');
            }
            let _ = std::fmt::Write::write_fmt(&mut line, format_args!("\"{}\"", p));
            // Wrap lines roughly every 16 entries for readability
            if (i + 1) % 16 == 0 && i + 1 < parts.len() {
                println!("{}", line);
                line = format!("[{}] ", i + 2);
            }
        }
        if !line.is_empty() {
            println!("{}", line);
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

// ---------------------------------------------------------------------------
// S3 summary dispatch — type-specific summary methods
// ---------------------------------------------------------------------------

/// R's `summary.numeric(x)` — summary for numeric (double) vector.
pub unsafe fn do_summary_numeric(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        let mut vals: Vec<f64> = Vec::new();
        for i in 0..n {
            let v = *REAL(x).add(i as usize);
            if v.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN && !v.is_nan() {
                vals.push(v);
            }
        }
        let na_count = n as usize - vals.len();
        if vals.is_empty() {
            println!("   Min. 1st Qu.  Median    Mean 3rd Qu.    Max.    NA's");
            println!(
                "     NA      NA      NA      NA      NA      NA       {}",
                n
            );
        } else {
            vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let min_v = vals[0];
            let max_v = vals[vals.len() - 1];
            let mean_v: f64 = vals.iter().sum::<f64>() / vals.len() as f64;
            let median_idx = vals.len() / 2;
            let median_v = if vals.len() % 2 == 1 {
                vals[median_idx]
            } else {
                (vals[median_idx - 1] + vals[median_idx]) / 2.0
            };
            let q1_idx = vals.len() / 4;
            let q3_idx = 3 * vals.len() / 4;
            println!("   Min. 1st Qu.  Median    Mean 3rd Qu.    Max.    NA's");
            println!(
                "{:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8}",
                min_v,
                vals[q1_idx],
                median_v,
                mean_v,
                vals[q3_idx],
                max_v,
                if na_count > 0 {
                    na_count.to_string()
                } else {
                    String::new()
                }
            );
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `summary.integer(x)` — summary for integer vector.
pub unsafe fn do_summary_integer(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        let mut vals: Vec<f64> = Vec::new();
        for i in 0..n {
            let v = *INTEGER(x).add(i as usize);
            if v != NA_INTEGER {
                vals.push(v as f64);
            }
        }
        let na_count = n as usize - vals.len();
        if vals.is_empty() {
            println!("   Min. 1st Qu.  Median    Mean 3rd Qu.    Max.    NA's");
            println!(
                "     NA      NA      NA      NA      NA      NA       {}",
                n
            );
        } else {
            vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let min_v = vals[0];
            let max_v = vals[vals.len() - 1];
            let mean_v: f64 = vals.iter().sum::<f64>() / vals.len() as f64;
            let median_idx = vals.len() / 2;
            let median_v = if vals.len() % 2 == 1 {
                vals[median_idx]
            } else {
                (vals[median_idx - 1] + vals[median_idx]) / 2.0
            };
            let q1_idx = vals.len() / 4;
            let q3_idx = 3 * vals.len() / 4;
            println!("   Min. 1st Qu.  Median    Mean 3rd Qu.    Max.    NA's");
            println!(
                "{:>8.0} {:>8.0} {:>8.0} {:>8.2} {:>8.0} {:>8.0} {:>8}",
                min_v,
                vals[q1_idx],
                median_v,
                mean_v,
                vals[q3_idx],
                max_v,
                if na_count > 0 {
                    na_count.to_string()
                } else {
                    String::new()
                }
            );
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `summary.logical(x)` — summary for logical vector (TRUE/FALSE/NA counts).
pub unsafe fn do_summary_logical(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        let mut true_count = 0;
        let mut false_count = 0;
        let mut na_count = 0;
        for i in 0..n {
            let v = *LOGICAL(x).add(i as usize);
            if v == NA_INTEGER {
                na_count += 1;
            } else if v == TRUE {
                true_count += 1;
            } else {
                false_count += 1;
            }
        }
        println!("   Mode   FALSE    TRUE    NA's");
        println!(
            "logical {:>7} {:>7} {:>7}",
            false_count, true_count, na_count
        );
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `summary.character(x)` — summary for character vector (class/length/NA).
pub unsafe fn do_summary_character(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        let mut na_count = 0;
        for i in 0..n {
            let charsxp = STRING_ELT(x, i);
            if charsxp.is_null() {
                na_count += 1;
            } else {
                let s = CHAR(charsxp);
                if s.is_null() {
                    na_count += 1;
                }
            }
        }
        println!("   Length     Class      Mode");
        println!("{:>9} character character", n);
        if na_count > 0 {
            println!("   NA's: {}", na_count);
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}
