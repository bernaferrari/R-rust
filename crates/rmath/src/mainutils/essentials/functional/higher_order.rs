//! Compact/keep/discard/append/flatten higher-order helpers.

use super::*;

/// R's `is.atomic(x)` — TRUE for non-recursive types (not list, pairlist, etc.).
pub unsafe fn do_is_atomic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        let is_atomic = t == SEXPTYPE::LGLSXP
            || t == SEXPTYPE::INTSXP
            || t == SEXPTYPE::REALSXP
            || t == SEXPTYPE::CPLXSXP
            || t == SEXPTYPE::STRSXP
            || t == SEXPTYPE::RAWSXP
            || t == SEXPTYPE::CHARSXP;
        Rf_ScalarLogical(if is_atomic { TRUE } else { FALSE })
    }
}

/// R's `is.recursive(x)` — TRUE for recursive types (list, pairlist, language, etc.).
pub unsafe fn do_is_recursive(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        let is_rec = t == SEXPTYPE::VECSXP
            || t == SEXPTYPE::LISTSXP
            || t == SEXPTYPE::LANGSXP
            || t == SEXPTYPE::CLOSXP
            || t == SEXPTYPE::BUILTINSXP
            || t == SEXPTYPE::SPECIALSXP
            || t == SEXPTYPE::ENVSXP
            || t == SEXPTYPE::EXPRSXP;
        Rf_ScalarLogical(if is_rec { TRUE } else { FALSE })
    }
}

/// R's `is.object(x)` — TRUE if x has a "class" attribute.
pub unsafe fn do_is_object(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let class = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"class".as_ptr()));
        Rf_ScalarLogical(if !class.is_null() && class != R_NilValue() {
            TRUE
        } else {
            FALSE
        })
    }
}

// ---------------------------------------------------------------------------
// List operations
// ---------------------------------------------------------------------------

/// R-like `list.append(x, ...)` — append elements to a list.
pub unsafe fn do_list_append(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let rest = CDR(args);
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let mut extra_count: R_xlen_t = 0;
        let mut cur = rest;
        while !cur.is_null() && cur != R_NilValue() {
            extra_count += 1;
            cur = CDR(cur);
        }

        let total = n + extra_count;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        // Copy original elements
        for i in 0..n {
            SET_VECTOR_ELT(result, i as i64, VECTOR_ELT(x, i));
        }

        // Append new elements
        let mut offset = n;
        cur = rest;
        while !cur.is_null() && cur != R_NilValue() {
            let elem = CAR(cur);
            SET_VECTOR_ELT(result, offset as i64, elem);
            offset += 1;
            cur = CDR(cur);
        }
        result
    }
}

/// R-like `list.prepend(x, ...)` — prepend elements to a list.
pub unsafe fn do_list_prepend(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let rest = CDR(args);
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let mut extra_count: R_xlen_t = 0;
        let mut cur = rest;
        while !cur.is_null() && cur != R_NilValue() {
            extra_count += 1;
            cur = CDR(cur);
        }

        let total = n + extra_count;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        // Prepend new elements
        let mut offset: R_xlen_t = 0;
        cur = rest;
        while !cur.is_null() && cur != R_NilValue() {
            let elem = CAR(cur);
            SET_VECTOR_ELT(result, offset as i64, elem);
            offset += 1;
            cur = CDR(cur);
        }

        // Copy original elements
        for i in 0..n {
            SET_VECTOR_ELT(result, (offset + i) as i64, VECTOR_ELT(x, i));
        }
        result
    }
}

/// R-like `compact(x)` — remove NULL elements from a list.
pub unsafe fn do_compact(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::VECSXP {
            return x;
        }

        let n = XLENGTH(x);
        let mut kept: Vec<R_xlen_t> = Vec::new();
        for i in 0..n {
            let elem = VECTOR_ELT(x, i);
            if !elem.is_null() && elem != R_NilValue() {
                kept.push(i);
            }
        }

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, kept.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        for (j, &i) in kept.iter().enumerate() {
            SET_VECTOR_ELT(result, j as i64, VECTOR_ELT(x, i));
        }
        result
    }
}

/// R-like `keep(x, i)` — keep elements at 1-based indices from a list/vector.
pub unsafe fn do_keep(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let i_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || i_arg.is_null() || i_arg == R_NilValue() {
            return x;
        }

        let t = TYPEOF(x);
        let n_i = XLENGTH(i_arg);
        let result = Rf_allocVector3(t, n_i);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        if t == SEXPTYPE::VECSXP {
            for j in 0..n_i {
                let idx = (*INTEGER(i_arg).add(j as usize) - 1) as R_xlen_t; // 1-based to 0-based
                if idx >= 0 {
                    let elem = VECTOR_ELT(x, idx);
                    SET_VECTOR_ELT(result, j as i64, elem);
                }
            }
        } else if t == SEXPTYPE::REALSXP {
            let dst = REAL(result);
            for j in 0..n_i {
                let idx = (*INTEGER(i_arg).add(j as usize) - 1) as R_xlen_t;
                if idx >= 0 {
                    *dst.add(j as usize) = *REAL(x).add(idx as usize);
                } else {
                    *dst.add(j as usize) = NA_REAL;
                }
            }
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let dst = INTEGER(result);
            for j in 0..n_i {
                let idx = (*INTEGER(i_arg).add(j as usize) - 1) as R_xlen_t;
                if idx >= 0 {
                    *dst.add(j as usize) = *INTEGER(x).add(idx as usize);
                } else {
                    *dst.add(j as usize) = NA_INTEGER;
                }
            }
        }
        result
    }
}

/// R-like `discard(x, i)` — discard elements at 1-based indices from a list/vector.
pub unsafe fn do_discard(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let i_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || i_arg.is_null() || i_arg == R_NilValue() {
            return x;
        }

        let n = XLENGTH(x);
        let n_i = XLENGTH(i_arg);

        // Collect which indices to discard (0-based)
        let mut discard_set: std::collections::HashSet<R_xlen_t> = std::collections::HashSet::new();
        for j in 0..n_i {
            let idx = (*INTEGER(i_arg).add(j as usize) - 1) as R_xlen_t;
            if idx >= 0 && idx < n {
                discard_set.insert(idx);
            }
        }

        let t = TYPEOF(x);
        let new_len = n - discard_set.len() as R_xlen_t;
        let result = Rf_allocVector3(t, new_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let mut out_idx: R_xlen_t = 0;
        if t == SEXPTYPE::VECSXP {
            for i in 0..n {
                if !discard_set.contains(&i) {
                    SET_VECTOR_ELT(result, out_idx as i64, VECTOR_ELT(x, i));
                    out_idx += 1;
                }
            }
        } else if t == SEXPTYPE::REALSXP {
            let dst = REAL(result);
            for i in 0..n {
                if !discard_set.contains(&i) {
                    *dst.add(out_idx as usize) = *REAL(x).add(i as usize);
                    out_idx += 1;
                }
            }
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let dst = INTEGER(result);
            for i in 0..n {
                if !discard_set.contains(&i) {
                    *dst.add(out_idx as usize) = *INTEGER(x).add(i as usize);
                    out_idx += 1;
                }
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Complete list/data.frame — checking
// ---------------------------------------------------------------------------

/// R's `is.data.frame(x)` — check if x has "data.frame" class.
pub unsafe fn do_is_data_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let class = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"class".as_ptr()));
        if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP && XLENGTH(class) > 0 {
            let cls = elt_to_string(class, 0);
            return Rf_ScalarLogical(if cls == "data.frame" { TRUE } else { FALSE });
        }
        Rf_ScalarLogical(FALSE)
    }
}

// ---------------------------------------------------------------------------
// Complete list operations — modifyList, splice, flatten, split, melt, cast
// ---------------------------------------------------------------------------

/// R's `modifyList(old, new)` — merge new into old (simplified: shallow merge).
pub unsafe fn do_modify_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let old = CAR(args);
        let new_list = CAR(CDR(args));
        if old.is_null() || old == R_NilValue() {
            return new_list;
        }
        if new_list.is_null() || new_list == R_NilValue() {
            return old;
        }
        // Simplified: if both are VECSXP, return new_list (shallow overlay)
        let t_old = TYPEOF(old);
        let t_new = TYPEOF(new_list);
        if t_old == SEXPTYPE::VECSXP && t_new == SEXPTYPE::VECSXP {
            // Return a copy of old with elements from new overlaid
            let n_old = XLENGTH(old);
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, n_old);
            if result.is_null() {
                return new_list;
            }
            let _p = protect(result);
            for i in 0..n_old {
                let elem = VECTOR_ELT(old, i);
                crate::sexp::accessors::SET_VECTOR_ELT(result, i, elem);
            }
            // Overlay elements from new (simplified: by index)
            let n_new = XLENGTH(new_list);
            for i in 0..n_new.min(n_old) {
                let elem = VECTOR_ELT(new_list, i);
                crate::sexp::accessors::SET_VECTOR_ELT(result, i, elem);
            }
            return result;
        }
        new_list
    }
}

/// R's `splice(x, i, value)` — splice value into list at position i (simplified).
pub unsafe fn do_splice(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let i_arg = CAR(CDR(args));
        let value = CAR(CDR(CDR(args)));
        if x.is_null() || x == R_NilValue() {
            return x;
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::VECSXP {
            return x;
        }
        let n = XLENGTH(x);
        let pos = real_or_default(i_arg, 1.0) as i64;
        // Insert value at position pos (1-indexed)
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n + 1);
        if result.is_null() {
            return x;
        }
        let _p = protect(result);
        let pos = ((pos - 1).max(0).min(n as i64)) as usize;
        for i in 0..pos {
            crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, VECTOR_ELT(x, i as i64));
        }
        crate::sexp::accessors::SET_VECTOR_ELT(result, pos as i64, value);
        for i in pos..(n as usize) {
            crate::sexp::accessors::SET_VECTOR_ELT(result, (i + 1) as i64, VECTOR_ELT(x, i as i64));
        }
        result
    }
}

/// R's `flatten(x)` — flatten a nested list (simplified: one level deep).
pub unsafe fn do_flatten(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return x;
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::VECSXP {
            return x;
        }
        // Count total elements after flattening
        let n = XLENGTH(x);
        let mut total: R_xlen_t = 0;
        for i in 0..n {
            let elem = VECTOR_ELT(x, i);
            if !elem.is_null() && TYPEOF(elem) == SEXPTYPE::VECSXP {
                let sub_n = XLENGTH(elem);
                total += sub_n;
            } else {
                total += 1;
            }
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, total);
        if result.is_null() {
            return x;
        }
        let _p = protect(result);
        let mut idx: R_xlen_t = 0;
        for i in 0..n {
            let elem = VECTOR_ELT(x, i);
            if !elem.is_null() && TYPEOF(elem) == SEXPTYPE::VECSXP {
                let sub_n = XLENGTH(elem);
                for j in 0..sub_n {
                    crate::sexp::accessors::SET_VECTOR_ELT(result, idx, VECTOR_ELT(elem, j));
                    idx += 1;
                }
            } else {
                crate::sexp::accessors::SET_VECTOR_ELT(result, idx, elem);
                idx += 1;
            }
        }
        result
    }
}
