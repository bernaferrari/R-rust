use super::*;

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

pub(crate) unsafe fn summary_tagged_int(args: SEXP, name: &str, default: i32) -> i32 {
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

pub(crate) unsafe fn summary_factor_result(x: SEXP, levels: Vec<String>, maxsum: i32) -> SEXP {
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
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
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
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
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
        let class =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
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
            let digits = {
                let mut cell = CDR(args);
                let mut found = None;
                while !cell.is_null() && cell != R_NilValue() {
                    if tag_name(cell).as_deref() == Some("digits") {
                        let v = crate::main::coerce::asInteger(CAR(cell));
                        if v != NA_INTEGER && v > 0 {
                            found = Some(v);
                        }
                    }
                    cell = CDR(cell);
                }
                found
            };
            let values = if let Some(digits) = digits {
                values
                    .into_iter()
                    .map(|v| {
                        if !v.is_finite() || v == 0.0 {
                            v
                        } else {
                            let exp = v.abs().log10().floor();
                            let scale = 10f64.powf(digits as f64 - 1.0 - exp);
                            (v * scale).round() / scale
                        }
                    })
                    .collect::<Vec<_>>()
            } else {
                values
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

pub(crate) unsafe fn sexp_has_class_name(x: SEXP, class_name: &str) -> bool {
    unsafe {
        let class = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"class".as_ptr()));
        if class.is_null() || TYPEOF(class) != SEXPTYPE::STRSXP {
            return false;
        }
        (0..XLENGTH(class)).any(|i| elt_to_string(class, i) == class_name)
    }
}

pub(crate) unsafe fn sexp_first_class_name(x: SEXP) -> Option<String> {
    unsafe {
        let class = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"class".as_ptr()));
        if class.is_null() || TYPEOF(class) != SEXPTYPE::STRSXP || XLENGTH(class) < 1 {
            return None;
        }
        Some(elt_to_string(class, 0))
    }
}

/// GNU: `if(cl != mod && substr(cl, 1, nchar(mod)) != mod) paste0("'",cl,"' ", mod)`.
pub(crate) fn str_class_mod_prefix(class: Option<&str>, type_name: &str) -> String {
    let Some(cl) = class else {
        return String::new();
    };
    if cl == type_name {
        return String::new();
    }
    if cl.len() >= type_name.len() && &cl[..type_name.len()] == type_name {
        return String::new();
    }
    format!("'{cl}' ")
}
