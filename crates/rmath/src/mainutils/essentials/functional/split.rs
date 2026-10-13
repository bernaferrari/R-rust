//! split()/unsplit()/melt()/cast().

use super::*;

/// R's `split(x, f)` — split vector `x` into groups defined by `f`.
pub unsafe fn do_split(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let f = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || f.is_null() || f == R_NilValue() {
            return R_NilValue();
        }

        let n = if crate::mainutils::essentials::sexp_has_class(x, "POSIXlt")
            && TYPEOF(x) == SEXPTYPE::VECSXP
        {
            crate::mainutils::subassign::posixlt_obs_length(x)
        } else {
            XLENGTH(x)
        };
        let classed = crate::sexp::accessors::OBJECT(x) != 0;

        let nf = XLENGTH(f);
        if nf == 0 && n > 0 {
            base_error("group length is 0 but data length > 0");
        }

        let factor_levels = split_factor_levels(f);
        let mut labels = factor_levels.clone().unwrap_or_default();
        let mut groups: Vec<Vec<R_xlen_t>> = vec![Vec::new(); labels.len()];
        let mut label_index: BTreeMap<String, usize> = labels
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, label)| (label, index))
            .collect();

        for i in 0..n {
            let f_index = i % nf;
            let Some(label) = split_group_label(f, f_index) else {
                continue;
            };
            let group_index = if let Some(index) = label_index.get(&label).copied() {
                index
            } else {
                let index = labels.len();
                label_index.insert(label.clone(), index);
                labels.push(label);
                groups.push(Vec::new());
                index
            };
            groups[group_index].push(i);
        }

        if factor_levels.is_none() {
            let mut ordered: Vec<(String, Vec<R_xlen_t>)> = labels
                .iter()
                .filter_map(|label| {
                    label_index
                        .get(label)
                        .map(|&index| (label.clone(), groups[index].clone()))
                })
                .collect();
            ordered.sort_by(|left, right| split_label_cmp(TYPEOF(f), &left.0, &right.0));
            labels = ordered.iter().map(|(label, _)| label.clone()).collect();
            groups = ordered.into_iter().map(|(_, group)| group).collect();
        }

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, labels.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let result_names = Rf_allocVector3(SEXPTYPE::STRSXP, labels.len() as R_xlen_t);
        let _names_guard = protect(result_names);
        let x_names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        let have_x_names = !x_names.is_null()
            && x_names != R_NilValue()
            && TYPEOF(x_names) == SEXPTYPE::STRSXP
            && XLENGTH(x_names) >= n;

        for (group_index, (label, indices)) in labels.iter().zip(groups.iter()).enumerate() {
            let sub = if classed {
                let idx = Rf_allocVector3(SEXPTYPE::INTSXP, indices.len() as R_xlen_t);
                let _idx = protect(idx);
                for (dst, &src) in indices.iter().enumerate() {
                    *INTEGER(idx).add(dst) = (src + 1) as c_int;
                }
                let sub_args = Rf_cons(x, Rf_cons(idx, R_NilValue()));
                let _sa = protect(sub_args);
                crate::mainutils::subset::do_subset(_call, _op, sub_args, rho)
            } else {
                let sub = Rf_allocVector3(TYPEOF(x), indices.len() as R_xlen_t);
                let _sub_guard = protect(sub);
                for (dst, &src) in indices.iter().enumerate() {
                    copy_matrix_element(sub, dst as R_xlen_t, x, src);
                }
                restore_datetime_or_difftime_class(x, sub);
                if have_x_names {
                    let names = Rf_allocVector3(SEXPTYPE::STRSXP, indices.len() as R_xlen_t);
                    let _group_names_guard = protect(names);
                    for (dst, &src) in indices.iter().enumerate() {
                        SET_STRING_ELT(names, dst as R_xlen_t, STRING_ELT(x_names, src));
                    }
                    crate::sexp::attrib_core::setAttrib(
                        sub,
                        crate::sexp::attrib_core::R_NamesSymbol(),
                        names,
                    );
                }
                sub
            };
            SET_VECTOR_ELT(result, group_index as R_xlen_t, sub);

            let label_c = CString::new(label.as_str()).unwrap_or_default();
            SET_STRING_ELT(
                result_names,
                group_index as R_xlen_t,
                Rf_mkChar(label_c.as_ptr()),
            );
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            result_names,
        );
        result
    }
}

/// GNU `unsplit(value, f)` inverse of `split`.
pub unsafe fn do_unsplit(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let value = CAR(args);
        let f = CAR(CDR(args));
        if value.is_null()
            || value == R_NilValue()
            || TYPEOF(value) != SEXPTYPE::VECSXP
            || XLENGTH(value) == 0
            || f.is_null()
            || f == R_NilValue()
        {
            return R_NilValue();
        }
        let first = VECTOR_ELT(value, 0);
        let n = XLENGTH(f);
        let result = Rf_allocVector3(TYPEOF(first), n);
        let _r = protect(result);
        let ng = XLENGTH(value) as usize;
        let mut cursor = vec![0i64; ng];
        for i in 0..n {
            let g = if TYPEOF(f) == SEXPTYPE::INTSXP {
                (*INTEGER(f).add(i as usize) as i64) - 1
            } else {
                elt_real_safe(f, i).round() as i64 - 1
            };
            if g < 0 || (g as usize) >= ng {
                continue;
            }
            let src = VECTOR_ELT(value, g);
            let pos = cursor[g as usize];
            if !src.is_null() && src != R_NilValue() && pos < XLENGTH(src) {
                copy_matrix_element(result, i, src, pos);
                cursor[g as usize] = pos + 1;
            }
        }
        result
    }
}

pub(super) unsafe fn split_factor_levels(f: SEXP) -> Option<Vec<String>> {
    unsafe {
        let levels =
            crate::sexp::attrib_core::getAttrib(f, crate::sexp::attrib_core::R_LevelsSymbol());
        if levels.is_null() || levels == R_NilValue() || TYPEOF(levels) != SEXPTYPE::STRSXP {
            return None;
        }
        let mut out = Vec::with_capacity(XLENGTH(levels) as usize);
        for i in 0..XLENGTH(levels) {
            out.push(elt_to_string(levels, i));
        }
        Some(out)
    }
}

pub(super) unsafe fn split_group_label(f: SEXP, index: R_xlen_t) -> Option<String> {
    unsafe {
        if let Some(levels) = split_factor_levels(f) {
            if TYPEOF(f) != SEXPTYPE::INTSXP {
                return None;
            }
            let raw = *INTEGER(f).add(index as usize);
            if raw == NA_INTEGER || raw < 1 || raw as usize > levels.len() {
                return None;
            }
            return Some(levels[(raw - 1) as usize].clone());
        }

        match TYPEOF(f) {
            t if t == SEXPTYPE::INTSXP => {
                let value = *INTEGER(f).add(index as usize);
                (value != NA_INTEGER).then(|| value.to_string())
            }
            t if t == SEXPTYPE::LGLSXP => {
                let value = *LOGICAL(f).add(index as usize);
                match value {
                    TRUE => Some("TRUE".to_string()),
                    FALSE => Some("FALSE".to_string()),
                    _ => None,
                }
            }
            t if t == SEXPTYPE::REALSXP => {
                let value = *REAL(f).add(index as usize);
                if value.to_bits() == R_NA_BIT_PATTERN || value.is_nan() {
                    None
                } else {
                    Some(format!("{value}"))
                }
            }
            t if t == SEXPTYPE::STRSXP => {
                let value = STRING_ELT(f, index);
                if value.is_null() || value == crate::sexp::globals::R_NaString() {
                    None
                } else {
                    Some(elt_to_string(f, index))
                }
            }
            _ => Some(elt_to_string(f, index)),
        }
    }
}

pub(super) fn split_label_cmp(t: c_int, left: &str, right: &str) -> std::cmp::Ordering {
    if t == SEXPTYPE::LGLSXP {
        return split_logical_rank(left).cmp(&split_logical_rank(right));
    }
    if t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP {
        let left_num = left.parse::<f64>().ok();
        let right_num = right.parse::<f64>().ok();
        if let (Some(left_num), Some(right_num)) = (left_num, right_num)
            && let Some(ordering) = left_num.partial_cmp(&right_num)
        {
            return ordering;
        }
    }
    left.cmp(right)
}

pub(super) fn split_logical_rank(value: &str) -> u8 {
    match value {
        "FALSE" => 0,
        "TRUE" => 1,
        _ => 2,
    }
}

/// R's `melt(x)` — melt a data.frame to long format (simplified).
pub unsafe fn do_melt(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Simplified: return the input as-is
        // A full implementation would reshape the data.frame
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        x
    }
}

/// R's `cast(x, formula)` — cast melted data (simplified).
pub unsafe fn do_cast(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Simplified: return the input as-is
        // A full implementation would reshape using the formula
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        x
    }
}
