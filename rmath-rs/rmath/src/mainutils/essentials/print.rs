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
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
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
        let colnames = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr()),
        );
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
        let names = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
        );
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

unsafe fn summary_factor_result(x: SEXP, levels: Vec<String>) -> SEXP {
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

        let include_na = na_count > 0;
        let result_len = counts.len() + usize::from(include_na);
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, result_len as R_xlen_t);
        if result.is_null() {
            return result;
        }
        let _result_guard = protect(result);

        for (i, count) in counts.iter().enumerate() {
            *INTEGER(result).add(i) = *count;
        }

        let mut names = levels;
        if include_na {
            *INTEGER(result).add(counts.len()) = na_count;
            names.push("NAs".to_string());
        }
        set_string_names(result, &names);
        result
    }
}

/// R's `summary.default(x)`: return GNU R-shaped summaryDefault/table vectors.
pub unsafe fn do_summary_default(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);

        if let Some(levels) = aggregate_factor_levels(x) {
            return summary_factor_result(x, levels);
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
            let names: Vec<&str> = if na_count > 0 {
                vec!["Mode", "FALSE", "TRUE", "NAs"]
            } else {
                vec!["Mode", "FALSE", "TRUE"]
            };
            let result = named_summary_result(SEXPTYPE::STRSXP, &names);
            if result.is_null() {
                return result;
            }
            let _result_guard = protect(result);
            SET_STRING_ELT(result, 0, Rf_mkChar(c"logical".as_ptr()));
            let false_text = CString::new(false_count.to_string()).unwrap_or_default();
            let true_text = CString::new(true_count.to_string()).unwrap_or_default();
            SET_STRING_ELT(result, 1, Rf_mkChar(false_text.as_ptr()));
            SET_STRING_ELT(result, 2, Rf_mkChar(true_text.as_ptr()));
            if na_count > 0 {
                let na_text = CString::new(na_count.to_string()).unwrap_or_default();
                SET_STRING_ELT(result, 3, Rf_mkChar(na_text.as_ptr()));
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

/// R's `str(x)` — compact structure display.
pub unsafe fn do_str(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!(" NULL");
            return R_NilValue();
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);

        if t == SEXPTYPE::VECSXP {
            // List
            let names = crate::sexp::attrib_core::getAttrib(
                x,
                Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
            );
            let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP;

            // Check for data.frame class
            let class = crate::sexp::attrib_core::getAttrib(
                x,
                Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
            );
            let is_df = if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
                elt_to_string(class, 0) == "data.frame"
            } else {
                false
            };

            if is_df {
                let ncol = n;
                let nrow = if ncol > 0 {
                    let first = VECTOR_ELT(x, 0);
                    if first.is_null() { 0 } else { XLENGTH(first) }
                } else {
                    0
                };
                println!("'data.frame':\t{} obs. of  {} variables:", nrow, ncol);
                for i in 0..ncol.min(6) {
                    let name = if has_names && i < XLENGTH(names) {
                        elt_to_string(names, i)
                    } else {
                        format!("$ {}", i + 1)
                    };
                    let elem = VECTOR_ELT(x, i as i64);
                    let elem_type = if elem.is_null() {
                        "NULL".to_string()
                    } else {
                        let et = TYPEOF(elem);
                        let m = XLENGTH(elem);
                        match et {
                            t if t == SEXPTYPE::REALSXP => format!("num [1:{}]", m),
                            t if t == SEXPTYPE::INTSXP => format!("int [1:{}]", m),
                            t if t == SEXPTYPE::LGLSXP => format!("logi [1:{}]", m),
                            t if t == SEXPTYPE::STRSXP => format!("chr [1:{}]", m),
                            _ => format!("? [1:{}]", m),
                        }
                    };
                    println!(" ${:<12}: {}", name, elem_type);
                }
            } else {
                println!("List of {}", n);
                for i in 0..n.min(6) {
                    let name = if has_names && i < XLENGTH(names) {
                        elt_to_string(names, i)
                    } else {
                        format!("[[{}]]", i + 1)
                    };
                    let elem = VECTOR_ELT(x, i as i64);
                    let elem_type = if elem.is_null() {
                        "NULL".to_string()
                    } else {
                        let et = TYPEOF(elem);
                        let m = XLENGTH(elem);
                        match et {
                            t if t == SEXPTYPE::REALSXP => format!("num [1:{}]", m),
                            t if t == SEXPTYPE::INTSXP => format!("int [1:{}]", m),
                            t if t == SEXPTYPE::LGLSXP => format!("logi [1:{}]", m),
                            t if t == SEXPTYPE::STRSXP => format!("chr [1:{}]", m),
                            t if t == SEXPTYPE::VECSXP => format!("list [1:{}]", m),
                            _ => format!("? [1:{}]", m),
                        }
                    };
                    println!(" $ {}: {}", name, elem_type);
                }
            }
        } else {
            // Atomic vector or other
            let type_name = match t {
                t if t == SEXPTYPE::REALSXP => "num",
                t if t == SEXPTYPE::INTSXP => "int",
                t if t == SEXPTYPE::LGLSXP => "logi",
                t if t == SEXPTYPE::STRSXP => "chr",
                t if t == SEXPTYPE::CPLXSXP => "cplx",
                t if t == SEXPTYPE::RAWSXP => "raw",
                _ => "?",
            };
            let preview_n = n.min(6);
            let parts: Vec<String> = (0..preview_n).map(|i| elt_to_string(x, i)).collect();
            print!(" {} [1:{}]", type_name, n);
            if !parts.is_empty() {
                print!(": {}", parts.join(" "));
            }
            if n > preview_n {
                print!(" ...");
            }
            println!();
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
        let names = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
        );
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
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );

        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) == 2 {
            // 2D table: print as matrix
            let nrow = *INTEGER(dim_attr) as usize;
            let ncol = *INTEGER(dim_attr).add(1) as usize;

            // Get dimnames
            let dn = crate::sexp::attrib_core::getAttrib(
                x,
                Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr()),
            );
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
            let names =
                crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
            let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP;
            if has_names {
                let labels = (0..n)
                    .map(|i| elt_to_string(names, i))
                    .collect::<Vec<_>>()
                    .join(" ");
                println!("{}", labels);
                let values = (0..n)
                    .map(|i| {
                        if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                            (*INTEGER(x).add(i as usize)).to_string()
                        } else if t == SEXPTYPE::REALSXP {
                            format!("{}", *REAL(x).add(i as usize))
                        } else {
                            elt_to_string(x, i)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                println!("{}", values);
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

/// R's `print.factor(x)` — print factor with levels and counts.
///
/// Prints the factor values and a levels summary like:
///   [1] a b c a
///   Levels: a b c
pub unsafe fn do_print_factor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("NULL");
            return R_NilValue();
        }

        let n = XLENGTH(x);

        // Get levels attribute
        let levels = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("levels").unwrap_or_default().as_ptr()),
        );
        let has_levels = !levels.is_null() && TYPEOF(levels) == SEXPTYPE::STRSXP;

        // Print the factor values
        if n == 0 {
            println!("factor(0)");
        } else {
            let t = TYPEOF(x);
            let mut counts: Vec<i32> = Vec::new();
            if has_levels {
                let nl = XLENGTH(levels);
                counts.resize(nl as usize, 0);
            }

            for i in 0..n {
                let val = if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                    let v = *INTEGER(x).add(i as usize);
                    if v == NA_INTEGER {
                        "<NA>".to_string()
                    } else if has_levels && (v as R_xlen_t) <= XLENGTH(levels) && v > 0 {
                        let idx = (v - 1) as R_xlen_t;
                        if (idx as usize) < counts.len() {
                            counts[idx as usize] += 1;
                        }
                        elt_to_string(levels, idx)
                    } else {
                        format!("{}", v)
                    }
                } else {
                    elt_to_string(x, i)
                };
                if i == 0 {
                    print!("[1] {}", val);
                } else {
                    print!(" {}", val);
                }
            }
            println!();

            // Print levels summary
            if has_levels {
                let nl = XLENGTH(levels);
                print!("Levels:");
                for i in 0..nl {
                    let lvl = elt_to_string(levels, i);
                    print!(" {}", lvl);
                }
                println!();
            }
        }

        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `summary.data.frame(x)` — summary for data.frame (prints column summaries).
pub unsafe fn do_summary_data_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        if TYPEOF(x) != SEXPTYPE::VECSXP {
            return do_summary_default(_call, _op, args, _rho);
        }
        let ncol = XLENGTH(x);
        let names = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
        );
        let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP;

        for j in 0..ncol {
            let name = if has_names && j < XLENGTH(names) {
                elt_to_string(names, j)
            } else {
                format!("[,{}]", j + 1)
            };
            let col = VECTOR_ELT(x, j as R_xlen_t);
            println!("      {} ", name);
            if col.is_null() {
                println!(" Mode:NULL ");
            } else {
                let t = TYPEOF(col);
                if t == SEXPTYPE::REALSXP || t == SEXPTYPE::INTSXP {
                    let n = XLENGTH(col);
                    let mut vals: Vec<f64> = Vec::new();
                    for i in 0..n {
                        let v = if t == SEXPTYPE::REALSXP {
                            *REAL(col).add(i as usize)
                        } else {
                            let iv = *INTEGER(col).add(i as usize);
                            if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
                        };
                        if v.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN && !v.is_nan() {
                            vals.push(v);
                        }
                    }
                    let na_count = n as usize - vals.len();
                    if vals.is_empty() {
                        println!(
                            " Min. : NA   1st Qu.: NA   Median : NA   Mean : NA   3rd Qu.: NA   Max. : NA   NA's: {}",
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
                        print!(
                            " Min. :{:.1}   1st Qu.:{:.1}   Median :{:.1}   Mean :{:.1}   3rd Qu.:{:.1}   Max. :{:.1}",
                            min_v, vals[q1_idx], median_v, mean_v, vals[q3_idx], max_v
                        );
                        if na_count > 0 {
                            print!("   NA's: {}", na_count);
                        }
                        println!();
                    }
                } else if t == SEXPTYPE::LGLSXP {
                    println!(" Mode :logical ");
                } else if t == SEXPTYPE::STRSXP {
                    println!(" Mode :character ");
                } else if t == SEXPTYPE::VECSXP {
                    println!(" Length:{} ", XLENGTH(col));
                } else {
                    println!(
                        " Mode :{} ",
                        elt_to_string(do_typeof(_call, _op, args, _rho), 0)
                    );
                }
            }
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `format.data.frame(x)` — format data.frame as character matrix.
pub unsafe fn do_format_data_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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

        let ncol = XLENGTH(x);
        let nrow = if ncol > 0 {
            let first = VECTOR_ELT(x, 0);
            if first.is_null() { 0 } else { XLENGTH(first) }
        } else {
            0
        };

        // Build a character matrix with ncol columns
        let total = ncol * nrow;
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..nrow {
            for j in 0..ncol {
                let col = VECTOR_ELT(x, j as R_xlen_t);
                let val = if col.is_null() {
                    "NULL".to_string()
                } else {
                    elt_to_string(col, i)
                };
                let cstr = CString::new(val).unwrap_or_default();
                let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                if !charsxp.is_null() {
                    let data = (*result).gengc_next_node as *mut SEXP;
                    *data.add((j as R_xlen_t * nrow + i) as usize) = charsxp;
                }
            }
        }

        // Set dim attribute
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            let _dim_guard = protect(dim);
            *INTEGER(dim) = nrow as i32;
            *INTEGER(dim).add(1) = ncol as i32;
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                dim,
            );
        }

        result
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
