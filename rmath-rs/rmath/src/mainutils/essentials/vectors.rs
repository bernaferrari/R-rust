//! Essentials domain module `vectors` — extracted verbatim from essentials.rs.

use super::*;
use std::ffi::CString;
use std::os::raw::c_int;

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
use crate::sexp::context::RError;
use crate::sexp::ffi::{
    FALSE, NA_INTEGER, NA_REAL, R_NA_BIT_PATTERN, R_xlen_t, Rbyte, Rcomplex, SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

// ---------------------------------------------------------------------------
// do_c — combine vectors
// ---------------------------------------------------------------------------

/// bind.c AnswerType()/ListAnswer() classify a plain pairlist cell-wise.
unsafe fn is_bind_pairlist(t: c_int) -> bool {
    t == SEXPTYPE::LISTSXP
}

/// bind.c AnswerType() `default:` — every other non-vector entry
/// (language objects, symbols, closures, builtins, promises, ...) binds
/// as exactly one element and forces a list result.  Using the raw
/// XLENGTH() on these is undefined (their length field overlaps the
/// pairlist pointers, which used to yield astronomic allocations).
unsafe fn is_bind_single_object(t: c_int) -> bool {
    t == SEXPTYPE::LANGSXP
        || t == SEXPTYPE::DOTSXP
        || t == SEXPTYPE::SYMSXP
        || t == SEXPTYPE::CLOSXP
        || t == SEXPTYPE::SPECIALSXP
        || t == SEXPTYPE::BUILTINSXP
        || t == SEXPTYPE::PROMSXP
        || t == SEXPTYPE::EXTPTRSXP
        || t == SEXPTYPE::BCODESXP
        || t == SEXPTYPE::WEAKREFSXP
}

/// Number of list slots `x` occupies in a c() result.
unsafe fn bind_length(x: SEXP, t: c_int) -> R_xlen_t {
    unsafe {
        if is_bind_pairlist(t) {
            let mut cell = x;
            let mut n: R_xlen_t = 0;
            while !cell.is_null() && cell != R_NilValue() {
                n += 1;
                cell = CDR(cell);
            }
            n
        } else if is_bind_single_object(t) {
            1
        } else {
            XLENGTH(x)
        }
    }
}

/// The i-th list slot of `x` under c() binding semantics.
unsafe fn bind_element(x: SEXP, t: c_int, i: R_xlen_t) -> SEXP {
    unsafe {
        if is_bind_pairlist(t) {
            let mut cell = x;
            let mut k: R_xlen_t = 0;
            while k < i && !cell.is_null() && cell != R_NilValue() {
                cell = CDR(cell);
                k += 1;
            }
            CAR(cell)
        } else if is_bind_single_object(t) {
            x
        } else if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP {
            VECTOR_ELT(x, i)
        } else {
            extract_element(x, i)
        }
    }
}

/// R's `c(...)` — concatenates vectors into a single vector.
///
/// Coercion rules: STRSXP > CPLXSXP > REALSXP > INTSXP > LGLSXP.
/// If any arg is STRSXP, result is STRSXP.
pub unsafe fn do_c(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let datetime_class = leading_datetime_class(args);
        // First pass: determine result type and total length
        let mut result_type = SEXPTYPE::NILSXP.as_c_int();
        let mut total_len: R_xlen_t = 0;
        let mut has_names = false;
        let names_symbol = crate::sexp::attrib_core::R_NamesSymbol();
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                let tag = TAG(current);
                if !tag.is_null() && tag != R_NilValue() {
                    has_names = true;
                }
                if crate::mainutils::objects::isS4(arg) != 0 {
                    result_type = SEXPTYPE::VECSXP.as_c_int();
                    total_len += 1;
                    current = CDR(current);
                    continue;
                }
                let t = TYPEOF(arg);
                let arg_names = crate::sexp::attrib_core::getAttrib(arg, names_symbol);
                if !arg_names.is_null()
                    && arg_names != R_NilValue()
                    && TYPEOF(arg_names) == SEXPTYPE::STRSXP
                    && XLENGTH(arg_names) > 0
                {
                    has_names = true;
                }
                if t == SEXPTYPE::EXPRSXP {
                    // bind.c AnswerType(): expression args force an
                    // expression result (flag 512) and win over the list
                    // flag (256).
                    result_type = SEXPTYPE::EXPRSXP.as_c_int();
                } else if is_bind_pairlist(t) || is_bind_single_object(t) {
                    // Non-vector entries force a list result; expressions
                    // keep precedence.
                    if result_type != SEXPTYPE::EXPRSXP.as_c_int() {
                        result_type = SEXPTYPE::VECSXP.as_c_int();
                    }
                } else if t == SEXPTYPE::VECSXP {
                    if result_type != SEXPTYPE::EXPRSXP.as_c_int() {
                        result_type = SEXPTYPE::VECSXP.as_c_int();
                    }
                } else if datetime_class.is_some()
                    && result_type != SEXPTYPE::VECSXP
                    && result_type != SEXPTYPE::EXPRSXP
                {
                    result_type = SEXPTYPE::REALSXP.as_c_int();
                } else if t == SEXPTYPE::STRSXP
                    && result_type != SEXPTYPE::VECSXP
                    && result_type != SEXPTYPE::EXPRSXP
                {
                    result_type = SEXPTYPE::STRSXP.as_c_int();
                } else if t == SEXPTYPE::CPLXSXP
                    && result_type != SEXPTYPE::VECSXP
                    && result_type != SEXPTYPE::EXPRSXP
                    && result_type != SEXPTYPE::STRSXP
                {
                    result_type = SEXPTYPE::CPLXSXP.as_c_int();
                } else if t == SEXPTYPE::REALSXP
                    && result_type != SEXPTYPE::VECSXP
                    && result_type != SEXPTYPE::EXPRSXP
                    && result_type != SEXPTYPE::STRSXP
                    && result_type != SEXPTYPE::CPLXSXP
                {
                    result_type = SEXPTYPE::REALSXP.as_c_int();
                } else if t == SEXPTYPE::INTSXP
                    && result_type != SEXPTYPE::VECSXP
                    && result_type != SEXPTYPE::EXPRSXP
                    && result_type != SEXPTYPE::STRSXP
                    && result_type != SEXPTYPE::CPLXSXP
                    && result_type != SEXPTYPE::REALSXP
                {
                    result_type = SEXPTYPE::INTSXP.as_c_int();
                } else if t == SEXPTYPE::LGLSXP
                    && result_type != SEXPTYPE::VECSXP
                    && result_type != SEXPTYPE::EXPRSXP
                    && result_type != SEXPTYPE::STRSXP
                    && result_type != SEXPTYPE::CPLXSXP
                    && result_type != SEXPTYPE::REALSXP
                    && result_type != SEXPTYPE::INTSXP
                {
                    result_type = SEXPTYPE::LGLSXP.as_c_int();
                } else if t == SEXPTYPE::RAWSXP && result_type == SEXPTYPE::NILSXP.as_c_int() {
                    result_type = SEXPTYPE::RAWSXP.as_c_int();
                }
                total_len += bind_length(arg, t);
            }
            current = CDR(current);
        }

        if total_len == 0 {
            return if result_type == SEXPTYPE::NILSXP.as_c_int() {
                R_NilValue()
            } else {
                Rf_allocVector3(result_type, 0)
            };
        }

        // Second pass: copy data
        let result = Rf_allocVector3(result_type, total_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let mut offset: R_xlen_t = 0;
        let names = if has_names {
            let names = Rf_allocVector3(SEXPTYPE::STRSXP, total_len);
            if names.is_null() {
                return R_NilValue();
            }
            let empty = Rf_mkChar(CString::new("").unwrap_or_default().as_ptr());
            for i in 0..total_len {
                SET_STRING_ELT(names, i, empty);
            }
            names
        } else {
            R_NilValue()
        };
        let _names_guard = if has_names {
            Some(protect(names))
        } else {
            None
        };

        if result_type == SEXPTYPE::VECSXP || result_type == SEXPTYPE::EXPRSXP {
            current = args;
            while !current.is_null() && current != R_NilValue() {
                let arg = CAR(current);
                if !arg.is_null() && arg != R_NilValue() {
                    if crate::mainutils::objects::isS4(arg) != 0 {
                        SET_VECTOR_ELT(
                            result,
                            offset,
                            crate::mainutils::duplicate::lazy_duplicate(arg),
                        );
                        if has_names {
                            let tag = TAG(current);
                            if !tag.is_null() && tag != R_NilValue() {
                                SET_STRING_ELT(names, offset, PRINTNAME(tag));
                            }
                        }
                        offset += 1;
                        current = CDR(current);
                        continue;
                    }
                    let t = TYPEOF(arg);
                    let n = bind_length(arg, t);
                    let arg_names = crate::sexp::attrib_core::getAttrib(arg, names_symbol);
                    for i in 0..n {
                        let value = bind_element(arg, t, i);
                        SET_VECTOR_ELT(result, offset + i, value);

                        if has_names {
                            if !arg_names.is_null()
                                && arg_names != R_NilValue()
                                && TYPEOF(arg_names) == SEXPTYPE::STRSXP
                                && i < XLENGTH(arg_names)
                            {
                                SET_STRING_ELT(names, offset + i, STRING_ELT(arg_names, i));
                            } else {
                                let tag = TAG(current);
                                if !tag.is_null() && tag != R_NilValue() && i == 0 {
                                    SET_STRING_ELT(names, offset + i, PRINTNAME(tag));
                                }
                            }
                        }
                    }
                    offset += n;
                }
                current = CDR(current);
            }

            if has_names {
                crate::sexp::attrib_core::setAttrib(result, names_symbol, names);
            }
            return result;
        }

        current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                let t = TYPEOF(arg);
                let n = XLENGTH(arg);

                if let Some((class, _source)) = datetime_class {
                    let dst = REAL(result);
                    for i in 0..n {
                        *dst.add((offset + i) as usize) = datetime_c_value(arg, i, class);
                    }
                } else if result_type == SEXPTYPE::REALSXP {
                    let dst = REAL(result);
                    for i in 0..n {
                        let val = if t == SEXPTYPE::REALSXP {
                            REAL_ELT(arg, i as c_int)
                        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                            let v = integer_or_logical_elt(arg, i as c_int);
                            if v == NA_INTEGER { NA_REAL } else { v as f64 }
                        } else {
                            NA_REAL
                        };
                        *dst.add((offset + i) as usize) = val;
                    }
                } else if result_type == SEXPTYPE::INTSXP {
                    let dst = INTEGER(result);
                    for i in 0..n {
                        let val = if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                            integer_or_logical_elt(arg, i as c_int)
                        } else {
                            NA_INTEGER
                        };
                        *dst.add((offset + i) as usize) = val;
                    }
                } else if result_type == SEXPTYPE::LGLSXP {
                    let dst = LOGICAL(result);
                    for i in 0..n {
                        let val = if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP {
                            integer_or_logical_elt(arg, i as c_int)
                        } else {
                            NA_INTEGER
                        };
                        *dst.add((offset + i) as usize) = val;
                    }
                } else if result_type == SEXPTYPE::RAWSXP {
                    let dst = RAW(result);
                    for i in 0..n {
                        let val = if t == SEXPTYPE::RAWSXP {
                            *RAW(arg).add(i as usize)
                        } else {
                            0 as Rbyte
                        };
                        *dst.add((offset + i) as usize) = val;
                    }
                } else if result_type == SEXPTYPE::CPLXSXP {
                    let dst = COMPLEX(result);
                    for i in 0..n {
                        let val = if t == SEXPTYPE::CPLXSXP {
                            *COMPLEX(arg).add(i as usize)
                        } else if t == SEXPTYPE::REALSXP {
                            Rcomplex {
                                r: REAL_ELT(arg, i as c_int),
                                i: 0.0,
                            }
                        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                            let v = integer_or_logical_elt(arg, i as c_int);
                            if v == NA_INTEGER {
                                Rcomplex { r: NA_REAL, i: 0.0 }
                            } else {
                                Rcomplex {
                                    r: v as f64,
                                    i: 0.0,
                                }
                            }
                        } else {
                            Rcomplex {
                                r: NA_REAL,
                                i: NA_REAL,
                            }
                        };
                        *dst.add((offset + i) as usize) = val;
                    }
                } else if result_type == SEXPTYPE::STRSXP {
                    for i in 0..n {
                        if t == SEXPTYPE::STRSXP {
                            SET_STRING_ELT(result, offset + i, STRING_ELT(arg, i));
                        } else if element_coerces_to_character_na(arg, i) {
                            SET_STRING_ELT(result, offset + i, crate::sexp::globals::R_NaString());
                        } else {
                            let value = elt_to_string(arg, i);
                            let cstr = CString::new(value).unwrap_or_default();
                            SET_STRING_ELT(result, offset + i, Rf_mkChar(cstr.as_ptr()));
                        }
                    }
                }
                if has_names {
                    let tag = TAG(current);
                    if !tag.is_null() && tag != R_NilValue() {
                        let printname = PRINTNAME(tag);
                        if !printname.is_null() {
                            SET_STRING_ELT(names, offset, printname);
                        }
                    }
                }
                offset += n;
            }
            current = CDR(current);
        }

        if has_names {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_NamesSymbol(),
                names,
            );
        }
        if let Some((class, source)) = datetime_class {
            set_datetime_class_from(result, source, class);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// do_seq — generate sequences
// ---------------------------------------------------------------------------

/// R's `tabulate(bin, nbins)` — count positive integer bin occurrences.
pub unsafe fn do_tabulate(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let bins = arg_by_name_or_position(args, &["bin"], 0);
        if bins.is_null() || bins == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }

        let nbins_arg = arg_by_name_or_position(args, &["nbins"], 1);
        let nbins = if nbins_arg.is_null() || nbins_arg == R_NilValue() {
            default_tabulate_bins(bins)
        } else {
            (real_or_default(nbins_arg, 0.0) as i64).max(0) as usize
        };

        let result = Rf_allocVector3(SEXPTYPE::INTSXP, nbins as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..nbins {
            *INTEGER(result).add(i) = 0;
        }

        for i in 0..XLENGTH(bins) {
            let Some(bin) = tabulate_bin_value(bins, i) else {
                continue;
            };
            if bin > 0 && bin <= nbins {
                let slot = INTEGER(result).add(bin - 1);
                *slot = slot.read().saturating_add(1);
            }
        }
        result
    }
}

fn default_tabulate_bins(bins: SEXP) -> usize {
    unsafe {
        let mut max_bin = 1_usize;
        for i in 0..XLENGTH(bins) {
            if let Some(bin) = tabulate_bin_value(bins, i)
                && bin > max_bin
            {
                max_bin = bin;
            }
        }
        max_bin
    }
}

fn tabulate_bin_value(bins: SEXP, index: R_xlen_t) -> Option<usize> {
    unsafe {
        match TYPEOF(bins) {
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                let value = *INTEGER(bins).add(index as usize);
                (value != NA_INTEGER).then_some(value.max(0) as usize)
            }
            t if t == SEXPTYPE::REALSXP => {
                let value = *REAL(bins).add(index as usize);
                if value.to_bits() == R_NA_BIT_PATTERN || value.is_nan() || !value.is_finite() {
                    None
                } else {
                    Some((value as i64).max(0) as usize)
                }
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Parallel min/max and which.min/which.max
// ---------------------------------------------------------------------------

/// R's `pmin(...)` — parallel minimum across vectors (element-wise min with recycling).
pub unsafe fn do_pmin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_pminmax(args, true) }
}

/// R's `pmax(...)` — parallel maximum across vectors (element-wise max with recycling).
pub unsafe fn do_pmax(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_pminmax(args, false) }
}

unsafe fn do_pminmax(args: SEXP, is_min: bool) -> SEXP {
    unsafe {
        let na_rm = named_logical_arg(args, "na.rm").unwrap_or(false);
        let mut arg_vecs: Vec<SEXP> = Vec::new();
        let mut max_len: R_xlen_t = 0;
        let mut result_type = SEXPTYPE::INTSXP;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if tag_name(current).as_deref() != Some("na.rm")
                && !arg.is_null()
                && arg != R_NilValue()
            {
                arg_vecs.push(arg);
                if TYPEOF(arg) == SEXPTYPE::STRSXP {
                    result_type = SEXPTYPE::STRSXP;
                } else if TYPEOF(arg) == SEXPTYPE::REALSXP && result_type != SEXPTYPE::STRSXP {
                    result_type = SEXPTYPE::REALSXP;
                }
                let n = XLENGTH(arg);
                if n > max_len {
                    max_len = n;
                }
            }
            current = CDR(current);
        }
        if arg_vecs.is_empty() || max_len == 0 {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }
        if result_type == SEXPTYPE::STRSXP {
            return pminmax_character(&arg_vecs, max_len, is_min, na_rm);
        }
        let result = Rf_allocVector3(result_type, max_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..max_len {
            let mut best = 0.0;
            let mut seen_value = false;
            let mut seen_missing = false;
            for &arg in &arg_vecs {
                let n = XLENGTH(arg);
                if n == 0 {
                    continue;
                }
                let idx = i % n;
                let v = elt_real_safe(arg, idx);
                if v.to_bits() == R_NA_BIT_PATTERN || v.is_nan() {
                    seen_missing = true;
                    continue;
                }
                if !seen_value {
                    best = v;
                    seen_value = true;
                } else if is_min {
                    if v < best {
                        best = v;
                    }
                } else {
                    if v > best {
                        best = v;
                    }
                }
            }
            if result_type == SEXPTYPE::REALSXP {
                *REAL(result).add(i as usize) = if seen_missing && !na_rm || !seen_value {
                    NA_REAL
                } else {
                    best
                };
            } else {
                *INTEGER(result).add(i as usize) = if seen_missing && !na_rm || !seen_value {
                    NA_INTEGER
                } else {
                    best as c_int
                };
            }
        }
        result
    }
}

unsafe fn pminmax_character(
    arg_vecs: &[SEXP],
    max_len: R_xlen_t,
    is_min: bool,
    na_rm: bool,
) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, max_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..max_len {
            let mut best = String::new();
            let mut seen_value = false;
            let mut seen_missing = false;
            for &arg in arg_vecs {
                let n = XLENGTH(arg);
                if n == 0 {
                    continue;
                }
                let idx = i % n;
                let missing = if TYPEOF(arg) == SEXPTYPE::STRSXP {
                    let charsxp = crate::sexp::accessors::STRING_ELT(arg, idx);
                    charsxp.is_null() || charsxp == crate::sexp::globals::R_NaString()
                } else {
                    let v = elt_real_safe(arg, idx);
                    v.to_bits() == R_NA_BIT_PATTERN || v.is_nan()
                };
                if missing {
                    seen_missing = true;
                    continue;
                }
                let value = elt_to_string(arg, idx);
                if !seen_value {
                    best = value;
                    seen_value = true;
                } else if (is_min && value < best) || (!is_min && value > best) {
                    best = value;
                }
            }
            if (seen_missing && !na_rm) || !seen_value {
                SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
            } else {
                let cstr = CString::new(best).unwrap_or_default();
                let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                SET_STRING_ELT(result, i, charsxp);
            }
        }
        result
    }
}

/// R's `which.min(x)` — 1-based index of minimum element.
pub unsafe fn do_which_min(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_which_minmax(args, true) }
}

/// R's `which.max(x)` — 1-based index of maximum element.
pub unsafe fn do_which_max(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_which_minmax(args, false) }
}

unsafe fn do_which_minmax(args: SEXP, is_min: bool) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || XLENGTH(x) == 0 {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let n = XLENGTH(x);
        let mut best: Option<(R_xlen_t, f64)> = None;
        for i in 0..n {
            let v = elt_real_safe(x, i);
            if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || v.is_nan() {
                continue;
            }
            match best {
                None => best = Some((i, v)),
                Some((_, best_val)) if is_min && v < best_val => {
                    best = Some((i, v));
                }
                Some((_, best_val)) if !is_min && v > best_val => {
                    best = Some((i, v));
                }
                _ => {}
            }
        }
        if let Some((best_idx, _)) = best {
            Rf_ScalarInteger((best_idx + 1) as c_int)
        } else {
            Rf_allocVector3(SEXPTYPE::INTSXP, 0)
        }
    }
}

// ---------------------------------------------------------------------------
// Data manipulation: append, head, tail, subset
// ---------------------------------------------------------------------------

/// R's `append(x, values, after)` — insert values into vector at position.
pub unsafe fn do_append(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let values = CAR(CDR(args));
        let after_arg = CAR(CDR(CDR(args)));
        if (x.is_null() || x == R_NilValue()) && (values.is_null() || values == R_NilValue()) {
            return R_NilValue();
        }
        if values.is_null() || values == R_NilValue() {
            return x;
        }
        if x.is_null() || x == R_NilValue() {
            return values;
        }
        let n = XLENGTH(x);
        let vlen = XLENGTH(values);
        let after = if after_arg.is_null() || after_arg == R_NilValue() {
            n as i64
        } else {
            real_or_default(after_arg, n as f64) as i64
        };
        let after = (after.max(0) as R_xlen_t).min(n);
        let total = n + vlen;
        let tx = TYPEOF(x);
        let tv = TYPEOF(values);
        let t = if tx == SEXPTYPE::STRSXP || tv == SEXPTYPE::STRSXP {
            SEXPTYPE::STRSXP
        } else if tx == SEXPTYPE::CPLXSXP || tv == SEXPTYPE::CPLXSXP {
            SEXPTYPE::CPLXSXP
        } else if tx == SEXPTYPE::REALSXP || tv == SEXPTYPE::REALSXP {
            SEXPTYPE::REALSXP
        } else if (tx == SEXPTYPE::LGLSXP || tx == SEXPTYPE::INTSXP || tx == SEXPTYPE::RAWSXP)
            && (tv == SEXPTYPE::LGLSXP || tv == SEXPTYPE::INTSXP || tv == SEXPTYPE::RAWSXP)
        {
            SEXPTYPE::INTSXP
        } else {
            std::panic::panic_any(RError {
                message: format!(
                    "cannot handle type {:?} in 'append'",
                    if tx == SEXPTYPE::NILSXP.as_c_int() {
                        tv
                    } else {
                        tx
                    }
                ),
            });
        };
        let result = Rf_allocVector3(t, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        if t == SEXPTYPE::STRSXP {
            for i in 0..after {
                SET_STRING_ELT(result, i, str_elt_or_na(x, i));
            }
            for i in 0..vlen {
                SET_STRING_ELT(result, after + i, str_elt_or_na(values, i));
            }
            for i in after..n {
                SET_STRING_ELT(result, i + vlen, str_elt_or_na(x, i));
            }
        } else if t == SEXPTYPE::REALSXP {
            let dst = REAL(result);
            for i in 0..after {
                *dst.add(i as usize) = elt_real_safe(x, i);
            }
            for i in 0..vlen {
                *dst.add((after + i) as usize) = elt_real_safe(values, i);
            }
            for i in after..n {
                *dst.add((i + vlen) as usize) = elt_real_safe(x, i);
            }
        } else if t == SEXPTYPE::CPLXSXP {
            let dst = COMPLEX(result);
            for i in 0..after {
                *dst.add(i as usize) = cplx_elt_or_na(x, i);
            }
            for i in 0..vlen {
                *dst.add((after + i) as usize) = cplx_elt_or_na(values, i);
            }
            for i in after..n {
                *dst.add((i + vlen) as usize) = cplx_elt_or_na(x, i);
            }
        } else if t == SEXPTYPE::INTSXP {
            let dst = INTEGER(result);
            for i in 0..after {
                *dst.add(i as usize) = int_elt_or_na(x, i);
            }
            for i in 0..vlen {
                *dst.add((after + i) as usize) = int_elt_or_na(values, i);
            }
            for i in after..n {
                *dst.add((i + vlen) as usize) = int_elt_or_na(x, i);
            }
        } else if t == SEXPTYPE::RAWSXP {
            let dst = RAW(result);
            for i in 0..after {
                *dst.add(i as usize) = raw_elt_or_zero(x, i);
            }
            for i in 0..vlen {
                *dst.add((after + i) as usize) = raw_elt_or_zero(values, i);
            }
            for i in after..n {
                *dst.add((i + vlen) as usize) = raw_elt_or_zero(x, i);
            }
        }
        result
    }
}

/// R's `head(x, n=6)` — first n elements.
pub unsafe fn do_head(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let len = XLENGTH(x);
        let n = if n_arg.is_null() || n_arg == R_NilValue() {
            6i64
        } else {
            real_or_default(n_arg, 6.0) as i64
        };
        let n = if n < 0 {
            (len as i64 + n).max(0) as R_xlen_t
        } else {
            n.min(len as i64) as R_xlen_t
        };
        let n = n.min(len);
        if n == 0 {
            return Rf_allocVector3(TYPEOF(x), 0);
        }
        let result = Rf_allocVector3(TYPEOF(x), n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let t = TYPEOF(x);
        for i in 0..n {
            copy_vector_element(result, i, x, i, SEXPTYPE(t));
        }
        slice_names_attribute(x, result, 0, n);
        result
    }
}

/// R's `tail(x, n=6)` — last n elements.
pub unsafe fn do_tail(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let len = XLENGTH(x);
        let n = if n_arg.is_null() || n_arg == R_NilValue() {
            6i64
        } else {
            real_or_default(n_arg, 6.0) as i64
        };
        let n = if n < 0 {
            (len as i64 + n).max(0) as R_xlen_t
        } else {
            n.min(len as i64) as R_xlen_t
        };
        let n = n.min(len);
        if n == 0 {
            return Rf_allocVector3(TYPEOF(x), 0);
        }
        let start = len - n;
        let result = Rf_allocVector3(TYPEOF(x), n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let t = TYPEOF(x);
        for i in 0..n {
            copy_vector_element(result, i, x, start + i, SEXPTYPE(t));
        }
        slice_names_attribute(x, result, start, n);
        result
    }
}

pub(crate) fn copy_vector_element(
    dst: SEXP,
    dst_index: R_xlen_t,
    src: SEXP,
    src_index: R_xlen_t,
    target_type: SEXPTYPE,
) {
    unsafe {
        match target_type {
            t if t == SEXPTYPE::STRSXP => {
                SET_STRING_ELT(dst, dst_index, STRING_ELT(src, src_index));
            }
            t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                SET_VECTOR_ELT(dst, dst_index, VECTOR_ELT(src, src_index));
            }
            t if t == SEXPTYPE::REALSXP => {
                *REAL(dst).add(dst_index as usize) = *REAL(src).add(src_index as usize);
            }
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                *INTEGER(dst).add(dst_index as usize) = *INTEGER(src).add(src_index as usize);
            }
            t if t == SEXPTYPE::RAWSXP => {
                *RAW(dst).add(dst_index as usize) = *RAW(src).add(src_index as usize);
            }
            _ => {}
        }
    }
}

unsafe fn slice_names_attribute(x: SEXP, result: SEXP, start: R_xlen_t, len: R_xlen_t) {
    unsafe {
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return;
        }
        let sliced = Rf_allocVector3(SEXPTYPE::STRSXP, len);
        if sliced.is_null() {
            return;
        }
        let _sliced_guard = protect(sliced);
        for i in 0..len {
            SET_STRING_ELT(sliced, i, STRING_ELT(names, start + i));
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            sliced,
        );
    }
}

/// R's `x[i]` — subset extraction (simplified: integer index vector).
pub unsafe fn do_subset(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let i = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || i.is_null() || i == R_NilValue() {
            return Rf_allocVector3(TYPEOF(x), 0);
        }
        let n = XLENGTH(i);
        let result = Rf_allocVector3(TYPEOF(x), n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let t = TYPEOF(x);
        for j in 0..n {
            let idx = elt_real_safe(i, j) as i64;
            if idx < 1 {
                continue;
            }
            let src = (idx - 1) as usize;
            if t == SEXPTYPE::REALSXP {
                *REAL(result).add(j as usize) = *REAL(x).add(src);
            } else if t == SEXPTYPE::INTSXP {
                *INTEGER(result).add(j as usize) = *INTEGER(x).add(src);
            } else if t == SEXPTYPE::LGLSXP {
                *LOGICAL(result).add(j as usize) = *LOGICAL(x).add(src);
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Type checking: is.finite, is.infinite, is.nan, is.matrix, is.array, is.list
// ---------------------------------------------------------------------------

/// R's `is.finite(x)` — check for finite values.
pub unsafe fn do_is_finite(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let is_fin = if t == SEXPTYPE::REALSXP {
                let v = *REAL(x).add(i as usize);
                v.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN && v.is_finite()
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                *INTEGER(x).add(i as usize) != NA_INTEGER
            } else {
                false
            };
            *dst.add(i as usize) = if is_fin { TRUE } else { FALSE };
        }
        result
    }
}

/// R's `is.infinite(x)` — check for infinite values.
pub unsafe fn do_is_infinite(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let is_infinite = if t == SEXPTYPE::REALSXP {
                (*REAL(x).add(i as usize)).is_infinite()
            } else {
                false
            };
            *dst.add(i as usize) = if is_infinite { TRUE } else { FALSE };
        }
        result
    }
}

/// R's `is.nan(x)` — check for NaN values (not NA).
pub unsafe fn do_is_nan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let is_nan = if t == SEXPTYPE::REALSXP {
                let v = *REAL(x).add(i as usize);
                v.is_nan() && v.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN
            } else {
                false
            };
            *dst.add(i as usize) = if is_nan { TRUE } else { FALSE };
        }
        result
    }
}

/// R's `is.matrix(x)` — check if x has a dim attribute with exactly 2 dimensions.
pub unsafe fn do_is_matrix(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        let is_mat =
            !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) == 2;
        Rf_ScalarLogical(if is_mat { TRUE } else { FALSE })
    }
}

/// R's `is.array(x)` — check if x has a dim attribute.
pub unsafe fn do_is_array(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        let is_array = !dim_attr.is_null()
            && dim_attr != R_NilValue()
            && TYPEOF(dim_attr) == SEXPTYPE::INTSXP
            && LENGTH(dim_attr) > 0;
        Rf_ScalarLogical(if is_array { TRUE } else { FALSE })
    }
}

/// R's `is.list(x)` — check if x is a VECSXP (list).
pub unsafe fn do_is_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        Rf_ScalarLogical(if TYPEOF(x) == SEXPTYPE::VECSXP {
            TRUE
        } else {
            FALSE
        })
    }
}
