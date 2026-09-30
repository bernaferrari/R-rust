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
        || t == SEXPTYPE::ENVSXP
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

/// bind.c HasNames() for a pairlist: any non-NULL cell TAG.
unsafe fn bind_pairlist_has_tags(x: SEXP) -> bool {
    unsafe {
        let mut cell = x;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            if !tag.is_null() && tag != R_NilValue() {
                return true;
            }
            cell = CDR(cell);
        }
        false
    }
}

/// bind.c ListAnswer / NewExtractNames: printname of the i-th pairlist TAG.
unsafe fn bind_pairlist_cell_name(x: SEXP, i: R_xlen_t) -> SEXP {
    unsafe {
        let mut cell = x;
        let mut k: R_xlen_t = 0;
        while k < i && !cell.is_null() && cell != R_NilValue() {
            cell = CDR(cell);
            k += 1;
        }
        if cell.is_null() || cell == R_NilValue() {
            return R_NilValue();
        }
        let tag = TAG(cell);
        if tag.is_null() || tag == R_NilValue() || TYPEOF(tag) != SEXPTYPE::SYMSXP {
            R_NilValue()
        } else {
            PRINTNAME(tag)
        }
    }
}

/// R's `c(...)` — concatenates vectors into a single vector.
///
/// Coercion rules: STRSXP > CPLXSXP > REALSXP > INTSXP > LGLSXP.
/// If any arg is STRSXP, result is STRSXP.
pub unsafe fn do_c(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let args = crate::mainutils::bind::R_listCompact(args, true);
        let mut ans = R_NilValue();
        if crate::eval::missing::DispatchAnyOrEval(
            call,
            op,
            c"c".as_ptr(),
            args,
            rho,
            &mut ans,
            1,
            1,
        ) != 0
        {
            return ans;
        }
        let first = if args.is_null() || args == R_NilValue() {
            R_NilValue()
        } else {
            CAR(args)
        };
        if crate::mainutils::objects::inherits2(first, c"POSIXlt".as_ptr()) != 0 {
            return do_c_POSIXlt(call, op, args, rho);
        }
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
                } else if is_bind_pairlist(t) && bind_pairlist_has_tags(arg) {
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
            let empty = Rf_mkChar(c"".as_ptr());
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
                            let mut named = false;
                            if is_bind_pairlist(t) {
                                let cell_name = bind_pairlist_cell_name(arg, i);
                                if !cell_name.is_null() && cell_name != R_NilValue() {
                                    SET_STRING_ELT(names, offset + i, cell_name);
                                    named = true;
                                }
                            } else if !arg_names.is_null()
                                && arg_names != R_NilValue()
                                && TYPEOF(arg_names) == SEXPTYPE::STRSXP
                                && i < XLENGTH(arg_names)
                            {
                                SET_STRING_ELT(names, offset + i, STRING_ELT(arg_names, i));
                                named = true;
                            }
                            if !named {
                                let filled = c_arg_elem_name(
                                    TAG(current),
                                    arg_names,
                                    i,
                                    n,
                                );
                                if !filled.is_null() && filled != R_NilValue() {
                                    SET_STRING_ELT(names, offset + i, filled);
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
                        } else if t == SEXPTYPE::RAWSXP {
                            *RAW(arg).add(i as usize) as f64
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
                        } else if t == SEXPTYPE::RAWSXP {
                            *RAW(arg).add(i as usize) as c_int
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
                        } else if t == SEXPTYPE::RAWSXP {
                            if *RAW(arg).add(i as usize) != 0 { 1 } else { 0 }
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
                    let arg_names = crate::sexp::attrib_core::getAttrib(arg, names_symbol);
                    for i in 0..n {
                        let filled = c_arg_elem_name(TAG(current), arg_names, i, n);
                        if !filled.is_null() && filled != R_NilValue() {
                            SET_STRING_ELT(names, offset + i, filled);
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

/// GNU bind.c `NewName`: tag, tag.elem, or tag1/tag2 when a tagged
/// argument expands to more than one element.
unsafe fn c_arg_elem_name(tag: SEXP, arg_names: SEXP, i: R_xlen_t, n: R_xlen_t) -> SEXP {
    unsafe {
        let own = if !arg_names.is_null()
            && arg_names != R_NilValue()
            && TYPEOF(arg_names) == SEXPTYPE::STRSXP
            && i < XLENGTH(arg_names)
        {
            let s = STRING_ELT(arg_names, i);
            if !s.is_null()
                && s != R_NilValue()
                && (s == crate::sexp::globals::R_NaString() || *CHAR(s) != 0)
            {
                Some(s)
            } else {
                None
            }
        } else {
            None
        };
        let tag_chars = if !tag.is_null() && tag != R_NilValue() {
            let p = PRINTNAME(tag);
            if !p.is_null() && *CHAR(p) != 0 {
                Some(p)
            } else {
                None
            }
        } else {
            None
        };
        match (tag_chars, own) {
            (Some(t), Some(o)) => {
                let tb = std::ffi::CStr::from_ptr(CHAR(t)).to_string_lossy();
                let ob = std::ffi::CStr::from_ptr(CHAR(o)).to_string_lossy();
                let combined = format!("{tb}.{ob}");
                let cstr = CString::new(combined).unwrap_or_default();
                Rf_mkChar(cstr.as_ptr())
            }
            (None, Some(o)) => o,
            (Some(t), None) if n == 1 => t,
            (Some(t), None) => {
                let tb = std::ffi::CStr::from_ptr(CHAR(t)).to_string_lossy();
                let combined = format!("{}{}", tb, i + 1);
                let cstr = CString::new(combined).unwrap_or_default();
                Rf_mkChar(cstr.as_ptr())
            }
            (None, None) => Rf_mkChar(c"".as_ptr()),
        }
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
        if let Some(&first) = arg_vecs.first() {
            if XLENGTH(first) == 0 && crate::sexp::accessors::OBJECT(first) != 0 {
                return crate::mainutils::duplicate::Rf_duplicate(first);
            }
        }
        if let Some(frame) = pminmax_data_frame(&arg_vecs, is_min) {
            return frame;
        }
        if let Some(factor) = pminmax_factor_result(&arg_vecs, max_len, is_min, na_rm) {
            return factor;
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
                let mut v = elt_real_safe(arg, idx);
                if v.to_bits() == R_NA_BIT_PATTERN || v.is_nan() {
                    seen_missing = true;
                    continue;
                }
                if crate::mainutils::essentials::sexp_has_class(arg_vecs[0], "difftime")
                    && crate::mainutils::essentials::sexp_has_class(arg, "difftime")
                {
                    v *= difftime_unit_seconds(arg) / difftime_unit_seconds(arg_vecs[0]);
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
        copy_pminmax_shape(arg_vecs[0], result, max_len);
        if crate::mainutils::essentials::sexp_has_class(arg_vecs[0], "difftime") {
            let class = crate::sexp::attrib_core::getAttrib(arg_vecs[0], crate::sexp::attrib_core::R_ClassSymbol());
            crate::sexp::attrib_core::setAttrib(result, crate::sexp::attrib_core::R_ClassSymbol(), class);
            let units_sym = crate::sexp::symbol::Rf_install(c"units".as_ptr());
            crate::sexp::attrib_core::setAttrib(result, units_sym, crate::sexp::attrib_core::getAttrib(arg_vecs[0], units_sym));
        }
        result
    }
}
fn difftime_unit_seconds(x: SEXP) -> f64 {
    unsafe {
        let units = crate::sexp::attrib_core::getAttrib(x, crate::sexp::symbol::Rf_install(c"units".as_ptr()));
        let name = if units.is_null() || units == R_NilValue() || TYPEOF(units) != SEXPTYPE::STRSXP {
            "secs"
        } else {
            let p = crate::sexp::accessors::CHAR(crate::sexp::accessors::STRING_ELT(units, 0));
            std::ffi::CStr::from_ptr(p).to_str().unwrap_or("secs")
        };
        match name {
            "mins" => 60.0,
            "hours" => 3600.0,
            "days" => 86400.0,
            "weeks" => 604800.0,
            _ => 1.0,
        }
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
        copy_pminmax_shape(arg_vecs[0], result, max_len);
        result
    }
}
unsafe fn pminmax_factor_result(
    arg_vecs: &[SEXP],
    max_len: R_xlen_t,
    is_min: bool,
    na_rm: bool,
) -> Option<SEXP> {
    unsafe {
        let owner = arg_vecs.iter().copied().find(|arg| {
            crate::mainutils::apply::isFactor(*arg) != 0
        })?;
        let levels = crate::sexp::attrib_core::getAttrib(owner, Rf_install(c"levels".as_ptr()));
        if levels.is_null() || TYPEOF(levels) != SEXPTYPE::STRSXP {
            return None;
        }
        let nlev = XLENGTH(levels);
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, max_len);
        let _g = protect(result);
        for i in 0..max_len {
            let mut best = 0;
            let mut seen = false;
            let mut missing = false;
            for &arg in arg_vecs {
                let n = XLENGTH(arg);
                if n == 0 {
                    continue;
                }
                let idx = i % n;
                let code = if crate::mainutils::apply::isFactor(arg) != 0 {
                    let c = *INTEGER(arg).add(idx as usize);
                    if c == NA_INTEGER { None } else { Some(c) }
                } else {
                    let v = elt_real_safe(arg, idx);
                    if v.to_bits() == R_NA_BIT_PATTERN || v.is_nan() {
                        None
                    } else {
                        let label = if v.fract() == 0.0 {
                            format!("{}", v as i64)
                        } else {
                            format!("{v}")
                        };
                        let mut found = None;
                        for j in 0..nlev {
                            let s = elt_to_string(levels, j);
                            if s == label {
                                found = Some((j as c_int) + 1);
                                break;
                            }
                        }
                        if found.is_none() {
                            continue;
                        }
                        found
                    }
                };
                match code {
                    None => missing = true,
                    Some(c) => {
                        if !seen {
                            best = c;
                            seen = true;
                        } else if (is_min && c < best) || (!is_min && c > best) {
                            best = c;
                        }
                    }
                }
            }
            *INTEGER(result).add(i as usize) = if (missing && !na_rm) || !seen {
                NA_INTEGER
            } else {
                best
            };
        }
        let class = crate::sexp::attrib_core::getAttrib(
            owner,
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        crate::sexp::attrib_core::setAttrib(result, crate::sexp::attrib_core::R_ClassSymbol(), class);
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"levels".as_ptr()), levels);
        Some(result)
    }
}
/// GNU `pmin`/`pmax` copy the first argument's dimensions onto a result of
/// the same length (`mostattributes<-`).
unsafe fn copy_pminmax_shape(first: SEXP, result: SEXP, max_len: R_xlen_t) {
    unsafe {
        if XLENGTH(first) != max_len {
            return;
        }
        let names = crate::sexp::attrib_core::getAttrib(
            first,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        if !names.is_null()
            && names != R_NilValue()
            && XLENGTH(names) == max_len
        {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_NamesSymbol(),
                names,
            );
        }
        let dim = crate::sexp::attrib_core::getAttrib(
            first,
            crate::sexp::attrib_core::R_DimSymbol(),
        );
        if dim.is_null() || dim == R_NilValue() {
            return;
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimSymbol(),
            dim,
        );
        let dimnames = crate::sexp::attrib_core::getAttrib(
            first,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
        );
        if !dimnames.is_null() && dimnames != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
                dimnames,
            );
        }
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
        if tx == SEXPTYPE::VECSXP && tv == SEXPTYPE::VECSXP {
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, total);
            let _g = protect(result);
            for i in 0..after {
                SET_VECTOR_ELT(result, i, VECTOR_ELT(x, i));
            }
            for i in 0..vlen {
                SET_VECTOR_ELT(result, after + i, VECTOR_ELT(values, i));
            }
            for i in after..n {
                SET_VECTOR_ELT(result, i + vlen, VECTOR_ELT(x, i));
            }
            let xn = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
            let vn = crate::sexp::attrib_core::getAttrib(values, crate::sexp::attrib_core::R_NamesSymbol());
            if (!xn.is_null() && xn != R_NilValue()) || (!vn.is_null() && vn != R_NilValue()) {
                let names = Rf_allocVector3(SEXPTYPE::STRSXP, total);
                for i in 0..total {
                    let src = if i < after {
                        (xn, i)
                    } else if i < after + vlen {
                        (vn, i - after)
                    } else {
                        (xn, i - vlen)
                    };
                    let ch = if src.0.is_null() || src.0 == R_NilValue() || src.1 >= XLENGTH(src.0) {
                        crate::sexp::constructors::Rf_mkChar(c"".as_ptr())
                    } else {
                        STRING_ELT(src.0, src.1)
                    };
                    SET_STRING_ELT(names, i, ch);
                }
                crate::sexp::attrib_core::setAttrib(result, crate::sexp::attrib_core::R_NamesSymbol(), names);
            }
            return result;
        }
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

/// Scalar `n` for the vector `head`/`tail` builtins, following `utils:::.checkHT`.
///
/// A missing argument arrives as Nil, same as an explicit NULL, so Nil keeps
/// the default 6. The closure path rejects NULL.
unsafe fn head_tail_n(n_arg: SEXP) -> i64 {
    unsafe {
        if n_arg.is_null() || n_arg == R_NilValue() {
            return 6;
        }
        let kind = TYPEOF(n_arg);
        let nlen = XLENGTH(n_arg);
        if nlen == 0 || head_tail_n_all_missing(n_arg, kind, nlen) {
            base_error("invalid 'n' - must contain at least one non-missing element, got none.");
        }
        // is.numeric() || is.logical(). is.logical does not dispatch.
        // is.numeric rejects factors and the Date/POSIXt/difftime methods.
        let numeric_or_logical = if kind == SEXPTYPE::LGLSXP {
            true
        } else if kind == SEXPTYPE::INTSXP {
            crate::mainutils::objects::inherits2(n_arg, c"factor".as_ptr()) == 0
                && crate::mainutils::objects::inherits2(n_arg, c"Date".as_ptr()) == 0
                && crate::mainutils::objects::inherits2(n_arg, c"POSIXt".as_ptr()) == 0
                && crate::mainutils::objects::inherits2(n_arg, c"difftime".as_ptr()) == 0
        } else if kind == SEXPTYPE::REALSXP {
            crate::mainutils::objects::inherits2(n_arg, c"Date".as_ptr()) == 0
                && crate::mainutils::objects::inherits2(n_arg, c"POSIXt".as_ptr()) == 0
                && crate::mainutils::objects::inherits2(n_arg, c"difftime".as_ptr()) == 0
        } else {
            false
        };
        if !numeric_or_logical {
            base_error("invalid 'n' - must be numeric, possibly NA.");
        }
        real_or_default(n_arg, 6.0) as i64
    }
}

unsafe fn head_tail_n_all_missing(n_arg: SEXP, kind: c_int, nlen: R_xlen_t) -> bool {
    unsafe {
        if kind == SEXPTYPE::LGLSXP || kind == SEXPTYPE::INTSXP {
            let data = INTEGER(n_arg);
            for i in 0..nlen as usize {
                if *data.add(i) != NA_INTEGER {
                    return false;
                }
            }
            true
        } else if kind == SEXPTYPE::REALSXP {
            let data = REAL(n_arg);
            for i in 0..nlen as usize {
                if !(*data.add(i)).is_nan() {
                    return false;
                }
            }
            true
        } else if kind == SEXPTYPE::STRSXP {
            let na = crate::sexp::globals::R_NaString();
            for i in 0..nlen {
                let elt = STRING_ELT(n_arg, i);
                if !elt.is_null() && elt != na {
                    return false;
                }
            }
            true
        } else if kind == SEXPTYPE::CPLXSXP {
            let data = COMPLEX(n_arg);
            for i in 0..nlen as usize {
                let z = *data.add(i);
                if !z.r.is_nan() && !z.i.is_nan() {
                    return false;
                }
            }
            true
        } else if kind == SEXPTYPE::VECSXP {
            // all(is.na(list)): an element is missing only as a length-1 atomic NA.
            for i in 0..nlen {
                let elt = VECTOR_ELT(n_arg, i);
                let missing = if elt.is_null()
                    || elt == R_NilValue()
                    || !SEXPTYPE(TYPEOF(elt)).is_vector_type()
                    || XLENGTH(elt) != 1
                {
                    false
                } else {
                    let ek = TYPEOF(elt);
                    if ek == SEXPTYPE::LGLSXP || ek == SEXPTYPE::INTSXP {
                        *INTEGER(elt) == NA_INTEGER
                    } else if ek == SEXPTYPE::REALSXP {
                        (*REAL(elt)).is_nan()
                    } else if ek == SEXPTYPE::STRSXP {
                        let s = STRING_ELT(elt, 0);
                        !s.is_null() && s == crate::sexp::globals::R_NaString()
                    } else if ek == SEXPTYPE::CPLXSXP {
                        let z = *COMPLEX(elt);
                        z.r.is_nan() || z.i.is_nan()
                    } else {
                        false
                    }
                };
                if !missing {
                    return false;
                }
            }
            true
        } else {
            false
        }
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
        let n = head_tail_n(n_arg);
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
        let n = head_tail_n(n_arg);
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

unsafe fn dispatch_is(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
    generic: &[u8],
) -> Option<SEXP> {
    unsafe {
        let mut ans = R_NilValue();
        if crate::eval::dispatch::DispatchOrEval(
            call,
            op,
            generic.as_ptr() as *const std::os::raw::c_char,
            args,
            rho,
            &mut ans,
            0,
            1,
        ) != 0
        {
            Some(ans)
        } else {
            None
        }
    }
}

/// GNU `copyDimAndNames`: dim+dimnames if array, else names.
unsafe fn copy_dim_and_names(src: SEXP, dst: SEXP) {
    unsafe {
        let dims = crate::sexp::attrib_core::getAttrib(src, crate::sexp::attrib_core::R_DimSymbol());
        if !dims.is_null() && dims != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(dst, crate::sexp::attrib_core::R_DimSymbol(), dims);
            let dimnames =
                crate::sexp::attrib_core::getAttrib(src, crate::sexp::attrib_core::R_DimNamesSymbol());
            if !dimnames.is_null() && dimnames != R_NilValue() {
                crate::sexp::attrib_core::setAttrib(
                    dst,
                    crate::sexp::attrib_core::R_DimNamesSymbol(),
                    dimnames,
                );
            }
        } else {
            let names =
                crate::sexp::attrib_core::getAttrib(src, crate::sexp::attrib_core::R_NamesSymbol());
            if !names.is_null() && names != R_NilValue() {
                crate::sexp::attrib_core::setAttrib(
                    dst,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    names,
                );
            }
        }
    }
}


/// R's `is.finite(x)` — check for finite values.
pub unsafe fn do_is_finite(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if let Some(ans) = dispatch_is(call, op, args, rho, b"is.finite\0") {
            return ans;
        }
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
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
            } else if t == SEXPTYPE::CPLXSXP {
                let z = *COMPLEX(x).add(i as usize);
                z.r.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN
                    && z.i.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN
                    && z.r.is_finite()
                    && z.i.is_finite()
            } else {
                false
            };
            *dst.add(i as usize) = if is_fin { TRUE } else { FALSE };
        }
        copy_dim_and_names(x, result);
        result


    }
}

/// R's `is.infinite(x)` — check for infinite values.
pub unsafe fn do_is_infinite(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if let Some(ans) = dispatch_is(call, op, args, rho, b"is.infinite\0") {
            return ans;
        }
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
            } else if t == SEXPTYPE::CPLXSXP {
                let z = *COMPLEX(x).add(i as usize);
                z.r.is_infinite() || z.i.is_infinite()
            } else {
                false
            };
            *dst.add(i as usize) = if is_infinite { TRUE } else { FALSE };
        }
        copy_dim_and_names(x, result);
        result


    }
}

/// R's `is.nan(x)` — check for NaN values (not NA).
pub unsafe fn do_is_nan(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if let Some(ans) = dispatch_is(call, op, args, rho, b"is.nan\0") {
            return ans;
        }
        let x = CAR(args);
        let t = TYPEOF(x);
        let supported = t == SEXPTYPE::NILSXP
            || t == SEXPTYPE::STRSXP
            || t == SEXPTYPE::RAWSXP
            || t == SEXPTYPE::LGLSXP
            || t == SEXPTYPE::INTSXP
            || t == SEXPTYPE::REALSXP
            || t == SEXPTYPE::CPLXSXP;
        if !supported {
            let name = match t {
                t if t == SEXPTYPE::VECSXP => "list",
                t if t == SEXPTYPE::EXPRSXP => "expression",
                t if t == SEXPTYPE::LANGSXP => "language",
                t if t == SEXPTYPE::ENVSXP => "environment",
                t if t == SEXPTYPE::CLOSXP => "closure",
                t if t == SEXPTYPE::SYMSXP => "symbol",
                _ => "unknown",
            };
            crate::mainutils::errors::errorcall_str(
                call,
                &format!("default method not implemented for type '{name}'"),
            );
        }
        let n = if x.is_null() || x == R_NilValue() {
            0
        } else {
            XLENGTH(x)
        };
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
            } else if t == SEXPTYPE::CPLXSXP {
                let z = *COMPLEX(x).add(i as usize);
                (z.r.is_nan() && z.r.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN)
                    || (z.i.is_nan() && z.i.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN)
            } else {
                false
            };
            *dst.add(i as usize) = if is_nan { TRUE } else { FALSE };
        }
        copy_dim_and_names(x, result);
        result


    }
}



/// R's `is.matrix(x)` — check if x has a dim attribute with exactly 2 dimensions.
pub unsafe fn do_is_matrix(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut ans = R_NilValue();
        if crate::eval::dispatch::DispatchOrEval(
            call,
            op,
            c"is.matrix".as_ptr(),
            args,
            rho,
            &mut ans,
            0,
            1,
        ) != 0
        {
            return ans;
        }
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let dim_attr = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"dim".as_ptr()));
        let is_mat =
            !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) == 2;
        Rf_ScalarLogical(if is_mat { TRUE } else { FALSE })
    }
}

/// R's `is.array(x)` — check if x has a dim attribute.
pub unsafe fn do_is_array(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut ans = R_NilValue();
        if crate::eval::dispatch::DispatchOrEval(
            call,
            op,
            c"is.array".as_ptr(),
            args,
            rho,
            &mut ans,
            0,
            1,
        ) != 0
        {
            return ans;
        }
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let dim_attr = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"dim".as_ptr()));
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
        Rf_ScalarLogical(
            if TYPEOF(x) == SEXPTYPE::VECSXP || TYPEOF(x) == SEXPTYPE::LISTSXP {
                TRUE
            } else {
                FALSE
            },
        )
    }
}

unsafe fn is_data_frame(x: SEXP) -> bool {
    unsafe {
        if x.is_null() || TYPEOF(x) != SEXPTYPE::VECSXP {
            return false;
        }
        let class = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
        if class.is_null() || TYPEOF(class) != SEXPTYPE::STRSXP {
            return false;
        }
        for i in 0..XLENGTH(class) {
            if elt_to_string(class, i) == "data.frame" {
                return true;
            }
        }
        false
    }
}

unsafe fn pminmax_data_frame(arg_vecs: &[SEXP], is_min: bool) -> Option<SEXP> {
    unsafe {
        let frame = arg_vecs.iter().copied().find(|arg| is_data_frame(*arg))?;
        let ncol = XLENGTH(frame);
        let out = Rf_allocVector3(SEXPTYPE::VECSXP, ncol);
        let _g = protect(out);
        for i in 0..ncol {
            let column = VECTOR_ELT(frame, i);
            let nrow = XLENGTH(column);
            let short = arg_vecs.iter().any(|arg| {
                !is_data_frame(*arg) && XLENGTH(*arg) != nrow && XLENGTH(*arg) > 0
            });
            let value = if short {
                pminmax_frame_column(column, arg_vecs, i, is_min)
            } else {
                let mut cell = R_NilValue();
                for &arg in arg_vecs.iter().rev() {
                    let value = if is_data_frame(arg) { VECTOR_ELT(arg, i) } else { arg };
                    cell = Rf_cons(value, cell);
                }
                do_pminmax(cell, is_min)
            };
            SET_VECTOR_ELT(out, i, value);
        }
        let names = crate::sexp::attrib_core::getAttrib(frame, crate::sexp::attrib_core::R_NamesSymbol());
        if !names.is_null() && names != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(out, crate::sexp::attrib_core::R_NamesSymbol(), names);
        }
        let rows = crate::sexp::attrib_core::getAttrib(frame, crate::sexp::attrib_core::R_RowNamesSymbol());
        if !rows.is_null() && rows != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(out, crate::sexp::attrib_core::R_RowNamesSymbol(), rows);
        }
        crate::sexp::attrib_core::setAttrib(
            out,
            crate::sexp::attrib_core::R_ClassSymbol(),
            Rf_mkString(c"data.frame".as_ptr()),
        );
        Some(out)
    }
}

unsafe fn pminmax_frame_column(column: SEXP, arg_vecs: &[SEXP], _index: R_xlen_t, is_min: bool) -> SEXP {
    unsafe {
        let current = crate::mainutils::duplicate::Rf_duplicate(column);
        let _g = protect(current);
        let nrow = XLENGTH(current);
        for &arg in arg_vecs {
            if is_data_frame(arg) {
                continue;
            }
            let olen = XLENGTH(arg);
            if olen == 0 {
                continue;
            }
            let mut change = vec![false; nrow as usize];
            for i in 0..nrow {
                let left = elt_real_safe(current, i);
                let right = elt_real_safe(arg, i % olen);
                if left.is_nan() || right.is_nan() {
                    continue;
                }
                change[i as usize] = if is_min { left > right } else { left < right };
            }
            for i in 0..nrow {
                if !change[i as usize] {
                    continue;
                }
                let missing = i >= olen;
                let value = if missing { NA_REAL } else { elt_real_safe(arg, i) };
                if TYPEOF(current) == SEXPTYPE::INTSXP && !missing {
                    *INTEGER(current).add(i as usize) = value as c_int;
                } else if TYPEOF(current) == SEXPTYPE::INTSXP {
                    *INTEGER(current).add(i as usize) = NA_INTEGER;
                } else {
                    *REAL(current).add(i as usize) = value;
                }
            }
        }
        current
    }
}

#[cfg(test)]
mod head_tail_n_tests {
    use std::ffi::{CStr, CString};
    use std::panic::AssertUnwindSafe;

    use crate::sexp::accessors::{
        CHAR, COMPLEX, SET_STRING_ELT, SET_VECTOR_ELT, STRING_ELT, TYPEOF, XLENGTH,
    };
    use crate::sexp::constructors::{
        Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
        Rf_mkString,
    };
    use crate::sexp::context::RError;
    use crate::sexp::ffi::{NA_INTEGER, NA_REAL, SEXP, SEXPTYPE, TRUE};
    use crate::sexp::globals::{R_NaString, R_NilValue};
    use crate::sexp::protect::protect;

    use super::{do_head, do_tail};

    fn describe_sexp(value: SEXP) -> String {
        unsafe {
            if value.is_null() || value == R_NilValue() {
                return "NULL".to_string();
            }
            let kind = TYPEOF(value);
            let n = XLENGTH(value);
            if kind != SEXPTYPE::STRSXP {
                return format!("type={kind:?} len={n}");
            }
            let mut parts = Vec::with_capacity(n as usize);
            for i in 0..n {
                let elt = STRING_ELT(value, i);
                if elt.is_null() || elt == R_NaString() {
                    parts.push("NA".to_string());
                } else {
                    parts.push(CStr::from_ptr(CHAR(elt)).to_string_lossy().into_owned());
                }
            }
            format!("len={n}:{}", parts.join(","))
        }
    }

    fn call_ht(which: &str, x: SEXP, n: SEXP) -> String {
        let caught = std::panic::catch_unwind(AssertUnwindSafe(|| unsafe {
            let args = Rf_cons(x, Rf_cons(n, R_NilValue()));
            let _args = protect(args);
            if which == "head" {
                do_head(
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    args,
                    std::ptr::null_mut(),
                )
            } else {
                do_tail(
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    args,
                    std::ptr::null_mut(),
                )
            }
        }));
        match caught {
            Ok(value) => format!("OK:{}", describe_sexp(value)),
            Err(payload) => {
                if let Some(err) = payload.downcast_ref::<RError>() {
                    format!("ERR:{}", err.message)
                } else if let Some(err) = payload.downcast_ref::<String>() {
                    format!("PANIC:{err}")
                } else {
                    "PANIC:unknown".to_string()
                }
            }
        }
    }

    unsafe fn letters_vector() -> SEXP {
        unsafe {
            let out = Rf_allocVector3(SEXPTYPE::STRSXP, 26);
            let _guard = protect(out);
            for i in 0..26u8 {
                let bytes = [b'a' + i];
                let cs = CString::new(bytes.as_slice()).unwrap();
                SET_STRING_ELT(out, i as i64, Rf_mkChar(cs.as_ptr()));
            }
            out
        }
    }

    unsafe fn direct_report() -> String {
        unsafe {
            let letters = letters_vector();
            let _letters = protect(letters);
            let chr = CString::new("3").unwrap();
            let n_chr = Rf_mkString(chr.as_ptr());
            let n_true = Rf_ScalarLogical(TRUE);
            let n_three = Rf_ScalarInteger(3);
            let n_na = Rf_ScalarLogical(NA_INTEGER);
            format!(
                "head_chr={}\ntail_chr={}\nhead_true={}\ntail_true={}\nhead_3={}\nhead_na={}\ntail_na={}",
                call_ht("head", letters, n_chr),
                call_ht("tail", letters, n_chr),
                call_ht("head", letters, n_true),
                call_ht("tail", letters, n_true),
                call_ht("head", letters, n_three),
                call_ht("head", letters, n_na),
                call_ht("tail", letters, n_na),
            )
        }
    }

    unsafe fn scalar_complex(re: f64, im: f64) -> SEXP {
        let z = Rf_allocVector3(SEXPTYPE::CPLXSXP, 1);
        let data = COMPLEX(z);
        (*data).r = re;
        (*data).i = im;
        z
    }

    unsafe fn classed(x: SEXP, classes: &[&str]) -> SEXP {
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, classes.len() as i64);
        let _class = protect(class);
        for (i, name) in classes.iter().enumerate() {
            let cs = CString::new(*name).unwrap();
            SET_STRING_ELT(class, i as i64, Rf_mkChar(cs.as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_ClassSymbol(), class);
        x
    }

    unsafe fn list_of(elts: &[SEXP]) -> SEXP {
        let v = Rf_allocVector3(SEXPTYPE::VECSXP, elts.len() as i64);
        for (i, elt) in elts.iter().enumerate() {
            SET_VECTOR_ELT(v, i as i64, *elt);
        }
        v
    }

    /// GNU R 4.6.1 `utils:::.checkHT` on the head/tail builtins.
    unsafe fn builtin_missing_and_class_report() -> String {
        unsafe {
            let letters = letters_vector();
            let _letters = protect(letters);
            let mut lines = Vec::new();
            let mut pair = |label: &str, n: SEXP| unsafe {
                let _n = protect(n);
                lines.push(format!("head_{label}={}", call_ht("head", letters, n)));
                lines.push(format!("tail_{label}={}", call_ht("tail", letters, n)));
            };

            pair("na_cplx", scalar_complex(NA_REAL, NA_REAL));
            pair("cplx_re_na", scalar_complex(NA_REAL, 1.0));
            pair("cplx_im_na", scalar_complex(1.0, NA_REAL));
            pair("cplx_re_nan", scalar_complex(f64::NAN, 0.0));
            pair("cplx_im_nan", scalar_complex(0.0, f64::NAN));
            let both = Rf_allocVector3(SEXPTYPE::CPLXSXP, 2);
            let _both = protect(both);
            let data = COMPLEX(both);
            (*data).r = NA_REAL;
            (*data).i = NA_REAL;
            (*data.add(1)).r = NA_REAL;
            (*data.add(1)).i = NA_REAL;
            pair("cplx_pair", both);
            pair("cplx_one", scalar_complex(1.0, 0.0));

            pair("list0", Rf_allocVector3(SEXPTYPE::VECSXP, 0));
            let na_lgl = Rf_ScalarLogical(NA_INTEGER);
            let _na_lgl = protect(na_lgl);
            pair("list_na", list_of(&[na_lgl]));
            let na_chr = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
            let _na_chr = protect(na_chr);
            SET_STRING_ELT(na_chr, 0, R_NaString());
            pair("list_na_chr", list_of(&[na_chr]));
            pair("list_na_pair", list_of(&[na_lgl, na_chr]));
            let one = Rf_ScalarReal(1.0);
            let _one = protect(one);
            pair("list_one", list_of(&[one]));

            pair("expr0", Rf_allocVector3(SEXPTYPE::EXPRSXP, 0));
            let expr_na = Rf_allocVector3(SEXPTYPE::EXPRSXP, 1);
            let _expr_na = protect(expr_na);
            SET_VECTOR_ELT(expr_na, 0, na_lgl);
            pair("expr_na", expr_na);

            pair("factor3", classed(Rf_ScalarInteger(3), &["factor"]));
            pair(
                "ordered3",
                classed(Rf_ScalarInteger(3), &["ordered", "factor"]),
            );
            pair("date", classed(Rf_ScalarReal(3.0), &["Date"]));
            pair(
                "posixct",
                classed(Rf_ScalarReal(3.0), &["POSIXct", "POSIXt"]),
            );
            pair("int_date", classed(Rf_ScalarInteger(3), &["Date"]));
            pair("foo", classed(Rf_ScalarInteger(3), &["foo"]));
            pair(
                "factor_na",
                classed(Rf_ScalarInteger(NA_INTEGER), &["factor"]),
            );
            pair("date_na", classed(Rf_ScalarReal(NA_REAL), &["Date"]));
            pair("lgl_date", classed(Rf_ScalarLogical(TRUE), &["Date"]));
            pair("difftime", classed(Rf_ScalarReal(3.0), &["difftime"]));

            lines.join("\n")
        }
    }

    fn expected_builtin_missing_and_class_report() -> String {
        let miss = "ERR:invalid 'n' - must contain at least one non-missing element, got none.";
        let num = "ERR:invalid 'n' - must be numeric, possibly NA.";
        let rows = [
            ("na_cplx", miss),
            ("cplx_re_na", miss),
            ("cplx_im_na", miss),
            ("cplx_re_nan", miss),
            ("cplx_im_nan", miss),
            ("cplx_pair", miss),
            ("cplx_one", num),
            ("list0", miss),
            ("list_na", miss),
            ("list_na_chr", miss),
            ("list_na_pair", miss),
            ("list_one", num),
            ("expr0", miss),
            ("expr_na", num),
            ("factor3", num),
            ("ordered3", num),
            ("date", num),
            ("posixct", num),
            ("int_date", num),
            ("foo", "OK"),
            ("factor_na", miss),
            ("date_na", miss),
            ("lgl_date", "OK"),
            ("difftime", num),
        ];
        let mut lines = Vec::new();
        for (label, msg) in rows {
            let (head, tail) = match msg {
                "OK" if label == "foo" => ("OK:len=3:a,b,c", "OK:len=3:x,y,z"),
                "OK" => ("OK:len=1:a", "OK:len=1:z"),
                _ => (msg, msg),
            };
            lines.push(format!("head_{label}={head}"));
            lines.push(format!("tail_{label}={tail}"));
        }
        lines.join("\n")
    }

    #[test]
    fn head_tail_rejects_non_numeric_n() {
        // GNU R 4.6.1 utils:::.checkHT (reg-tests-1e.R PR#18357).
        let mut session = crate::sexp::session::RSession::new();
        let direct = unsafe { direct_report() };
        let extra = unsafe { builtin_missing_and_class_report() };
        let r_level = session.eval_script_with_output_capture_then(
            r#"paste(c(
                tryCatch(paste(head(letters, "3"), collapse=","), error=function(e) conditionMessage(e)),
                tryCatch(paste(tail(letters, "3"), collapse=","), error=function(e) conditionMessage(e)),
                tryCatch(paste(head(letters, TRUE), collapse=","), error=function(e) conditionMessage(e)),
                tryCatch(paste(tail(letters, TRUE), collapse=","), error=function(e) conditionMessage(e)),
                tryCatch(paste(head(letters, 3), collapse=","), error=function(e) conditionMessage(e)),
                tryCatch(paste(head(letters, NA), collapse=","), error=function(e) conditionMessage(e)),
                tryCatch(paste(tail(letters, NA), collapse=","), error=function(e) conditionMessage(e))
            ), collapse="|")"#,
            |result, output, _| match result {
                Ok(value) => describe_sexp(value.as_raw()),
                Err(err) => format!("EVAL_ERR:{} stderr:{}", err.message, output.stderr),
            },
        );
        let report = format!("r={r_level}\n{direct}\n{extra}");
        let expected = format!(
            "{}\n{}",
            "\
r=len=1:invalid 'n' - must be numeric, possibly NA.|invalid 'n' - must be numeric, possibly NA.|a|z|a,b,c|invalid 'n' - must contain at least one non-missing element, got none.|invalid 'n' - must contain at least one non-missing element, got none.
head_chr=ERR:invalid 'n' - must be numeric, possibly NA.
tail_chr=ERR:invalid 'n' - must be numeric, possibly NA.
head_true=OK:len=1:a
tail_true=OK:len=1:z
head_3=OK:len=3:a,b,c
head_na=ERR:invalid 'n' - must contain at least one non-missing element, got none.
tail_na=ERR:invalid 'n' - must contain at least one non-missing element, got none.",
            expected_builtin_missing_and_class_report()
        );
        assert_eq!(report, expected);
    }
}
