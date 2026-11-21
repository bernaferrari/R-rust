//! Essentials domain module `vectors` — extracted verbatim from essentials.rs.

use super::*;
use crate::sexp::object::SexpMut;
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

// ---------------------------------------------------------------------------
// do_c — combine vectors
// ---------------------------------------------------------------------------

/// bind.c AnswerType()/ListAnswer() classify a plain pairlist cell-wise.
fn is_bind_pairlist(t: c_int) -> bool {
    t == SEXPTYPE::LISTSXP
}

/// bind.c AnswerType() `default:` — every other non-vector entry
/// (language objects, symbols, closures, builtins, promises, ...) binds
/// as exactly one element and forces a list result.  Using the raw
/// XLENGTH() on these is undefined (their length field overlaps the
/// pairlist pointers, which used to yield astronomic allocations).
fn is_bind_single_object(t: c_int) -> bool {
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
fn bind_length(x: SEXP, t: c_int) -> R_xlen_t {
    {
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
fn bind_element(x: SEXP, t: c_int, i: R_xlen_t) -> SEXP {
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
        owner_sexp(active_owner(), x)
            .try_vector_elt(i)
            .map(|elt| elt.as_raw())
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
    } else {
        extract_element(x, i)
    }
}

/// bind.c HasNames() for a pairlist: any non-NULL cell TAG.
fn bind_pairlist_has_tags(x: SEXP) -> bool {
    {
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
fn bind_pairlist_cell_name(x: SEXP, i: R_xlen_t) -> SEXP {
    {
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
pub fn do_c(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
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
        if crate::mainutils::objects::inherits2(first, c"POSIXlt") != 0 {
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
        let owner = active_owner();
        let mut offset: R_xlen_t = 0;
        let names = if has_names {
            let names = Rf_allocVector3(SEXPTYPE::STRSXP, total_len);
            if names.is_null() {
                return R_NilValue();
            }
            if let (Ok(mut names_mut), Ok(empty)) = (
                owner.sexp(names).and_then(SexpMut::try_from_checked),
                owner.sexp(mk_char_cstr(c"")),
            ) {
                for i in 0..total_len {
                    let _ = names_mut.try_set_string_elt(i, empty.clone());
                }
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
            let mut result_mut = owner
                .sexp(result)
                .and_then(SexpMut::try_from_checked)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let mut names_mut = if has_names {
                Some(
                    owner
                        .sexp(names)
                        .and_then(SexpMut::try_from_checked)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
                )
            } else {
                None
            };
            current = args;
            while !current.is_null() && current != R_NilValue() {
                let arg = CAR(current);
                if !arg.is_null() && arg != R_NilValue() {
                    if crate::mainutils::objects::isS4(arg) != 0 {
                        let dup = crate::mainutils::duplicate::lazy_duplicate(arg);
                        let dup_owned = owner.sexp(dup).unwrap_or_else(|error| {
                            crate::sexp::context::r_error(error.to_string())
                        });
                        result_mut
                            .try_set_vector_elt(offset, dup_owned)
                            .unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });
                        if has_names {
                            let tag = TAG(current);
                            if !tag.is_null() && tag != R_NilValue() {
                                let printname =
                                    owner.sexp(PRINTNAME(tag)).unwrap_or_else(|error| {
                                        crate::sexp::context::r_error(error.to_string())
                                    });
                                if let Some(names_mut) = names_mut.as_mut() {
                                    let _ = names_mut.try_set_string_elt(offset, printname);
                                }
                            }
                        }
                        offset += 1;
                        current = CDR(current);
                        continue;
                    }
                    let t = TYPEOF(arg);
                    let n = bind_length(arg, t);
                    let arg_names = crate::sexp::attrib_core::getAttrib(arg, names_symbol);
                    let arg_names_owned = owner_sexp(owner, arg_names);
                    for i in 0..n {
                        let value = bind_element(arg, t, i);
                        let value_owned = owner.sexp(value).unwrap_or_else(|error| {
                            crate::sexp::context::r_error(error.to_string())
                        });
                        result_mut
                            .try_set_vector_elt(offset + i, value_owned)
                            .unwrap_or_else(|error| {
                                crate::sexp::context::r_error(error.to_string())
                            });

                        if has_names {
                            let mut named = false;
                            if is_bind_pairlist(t) {
                                let cell_name = bind_pairlist_cell_name(arg, i);
                                if !cell_name.is_null() && cell_name != R_NilValue() {
                                    let cell_name = owner.sexp(cell_name).unwrap_or_else(|error| {
                                        crate::sexp::context::r_error(error.to_string())
                                    });
                                    if let Some(names_mut) = names_mut.as_mut() {
                                        let _ = names_mut.try_set_string_elt(offset + i, cell_name);
                                    }
                                    named = true;
                                }
                            } else if !arg_names.is_null()
                                && arg_names != R_NilValue()
                                && TYPEOF(arg_names) == SEXPTYPE::STRSXP
                                && i < XLENGTH(arg_names)
                            {
                                if let Ok(elt) = arg_names_owned.try_string_elt(i) {
                                    if let Some(names_mut) = names_mut.as_mut() {
                                        let _ = names_mut.try_set_string_elt(offset + i, elt);
                                    }
                                    named = true;
                                }
                            }
                            if !named {
                                let filled = c_arg_elem_name(TAG(current), arg_names, i, n);
                                if !filled.is_null() && filled != R_NilValue() {
                                    let filled = owner.sexp(filled).unwrap_or_else(|error| {
                                        crate::sexp::context::r_error(error.to_string())
                                    });
                                    if let Some(names_mut) = names_mut.as_mut() {
                                        let _ = names_mut.try_set_string_elt(offset + i, filled);
                                    }
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
        let mut result_mut = owner
            .sexp(result)
            .and_then(SexpMut::try_from_checked)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let mut names_mut = if has_names {
            Some(
                owner
                    .sexp(names)
                    .and_then(SexpMut::try_from_checked)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string())),
            )
        } else {
            None
        };
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                let t = TYPEOF(arg);
                let n = XLENGTH(arg);
                let arg_owned = owner_sexp(owner, arg);
                fn fail<T>(error: crate::sexp::object::SexpError) -> T {
                    crate::sexp::context::r_error(error.to_string())
                }

                if let Some((class, _source)) = datetime_class {
                    for i in 0..n {
                        result_mut
                            .try_set_real_elt(offset + i, datetime_c_value(arg, i, class))
                            .unwrap_or_else(fail);
                    }
                } else if result_type == SEXPTYPE::REALSXP {
                    for i in 0..n {
                        let val = if t == SEXPTYPE::REALSXP {
                            arg_owned.try_real_elt(i).unwrap_or_else(fail)
                        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                            let v = integer_like_elt(&arg_owned, i);
                            if v == NA_INTEGER { NA_REAL } else { v as f64 }
                        } else if t == SEXPTYPE::RAWSXP {
                            arg_owned.try_raw_elt(i).unwrap_or_else(fail) as f64
                        } else {
                            NA_REAL
                        };
                        result_mut
                            .try_set_real_elt(offset + i, val)
                            .unwrap_or_else(fail);
                    }
                } else if result_type == SEXPTYPE::INTSXP {
                    for i in 0..n {
                        let val = if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                            integer_like_elt(&arg_owned, i)
                        } else if t == SEXPTYPE::RAWSXP {
                            arg_owned.try_raw_elt(i).unwrap_or_else(fail) as c_int
                        } else {
                            NA_INTEGER
                        };
                        result_mut
                            .try_set_integer_elt(offset + i, val)
                            .unwrap_or_else(fail);
                    }
                } else if result_type == SEXPTYPE::LGLSXP {
                    for i in 0..n {
                        let val = if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP {
                            integer_like_elt(&arg_owned, i)
                        } else if t == SEXPTYPE::RAWSXP {
                            if arg_owned.try_raw_elt(i).unwrap_or_else(fail) != 0 {
                                1
                            } else {
                                0
                            }
                        } else {
                            NA_INTEGER
                        };
                        result_mut
                            .try_set_logical_elt(offset + i, val)
                            .unwrap_or_else(fail);
                    }
                } else if result_type == SEXPTYPE::RAWSXP {
                    for i in 0..n {
                        let val = if t == SEXPTYPE::RAWSXP {
                            arg_owned.try_raw_elt(i).unwrap_or_else(fail)
                        } else {
                            0 as Rbyte
                        };
                        result_mut
                            .try_set_raw_elt(offset + i, val)
                            .unwrap_or_else(fail);
                    }
                } else if result_type == SEXPTYPE::CPLXSXP {
                    for i in 0..n {
                        let val = if t == SEXPTYPE::CPLXSXP {
                            arg_owned.try_complex_elt(i).unwrap_or_else(fail)
                        } else if t == SEXPTYPE::REALSXP {
                            Rcomplex {
                                r: arg_owned.try_real_elt(i).unwrap_or_else(fail),
                                i: 0.0,
                            }
                        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                            let v = integer_like_elt(&arg_owned, i);
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
                        result_mut
                            .try_set_complex_elt(offset + i, val)
                            .unwrap_or_else(fail);
                    }
                } else if result_type == SEXPTYPE::STRSXP {
                    for i in 0..n {
                        if t == SEXPTYPE::STRSXP {
                            let elt = arg_owned.try_string_elt(i).unwrap_or_else(fail);
                            result_mut
                                .try_set_string_elt(offset + i, elt)
                                .unwrap_or_else(fail);
                        } else if element_coerces_to_character_na(arg, i) {
                            let na = owner
                                .sexp(crate::sexp::globals::R_NaString())
                                .unwrap_or_else(fail);
                            result_mut
                                .try_set_string_elt(offset + i, na)
                                .unwrap_or_else(fail);
                        } else {
                            let value = elt_to_string(arg, i);
                            let cstr = CString::new(value).unwrap_or_default();
                            let chars = owner.sexp(mk_char_cstr(&cstr)).unwrap_or_else(fail);
                            result_mut
                                .try_set_string_elt(offset + i, chars)
                                .unwrap_or_else(fail);
                        }
                    }
                }
                if has_names {
                    let arg_names = crate::sexp::attrib_core::getAttrib(arg, names_symbol);
                    for i in 0..n {
                        let filled = c_arg_elem_name(TAG(current), arg_names, i, n);
                        if !filled.is_null() && filled != R_NilValue() {
                            let filled = owner.sexp(filled).unwrap_or_else(fail);
                            if let Some(names_mut) = names_mut.as_mut() {
                                let _ = names_mut.try_set_string_elt(offset + i, filled);
                            }
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
fn c_arg_elem_name(tag: SEXP, arg_names: SEXP, i: R_xlen_t, n: R_xlen_t) -> SEXP {
    let own = if !arg_names.is_null()
        && arg_names != R_NilValue()
        && TYPEOF(arg_names) == SEXPTYPE::STRSXP
        && i < XLENGTH(arg_names)
    {
        let s = owner_sexp(active_owner(), arg_names)
            .try_string_elt(i)
            .map(|elt| elt.as_raw())
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        if !s.is_null()
            && s != R_NilValue()
            && (s == crate::sexp::globals::R_NaString() || !charsxp_text_of(s).as_cstr().is_empty())
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
        if !p.is_null() && !charsxp_text_of(p).as_cstr().is_empty() {
            Some(p)
        } else {
            None
        }
    } else {
        None
    };
    match (tag_chars, own) {
        (Some(t), Some(o)) => {
            let tb = charsxp_text(t).unwrap_or_default();
            let ob_cstr = charsxp_cstring(o);
            let ob = ob_cstr.to_string_lossy();
            let combined = format!("{tb}.{ob}");
            let cstr = CString::new(combined).unwrap_or_default();
            mk_char_cstr(&cstr)
        }
        (None, Some(o)) => o,
        (Some(t), None) if n == 1 => t,
        (Some(t), None) => {
            let tb_cstr = charsxp_cstring(t);
            let tb = tb_cstr.to_string_lossy();
            let combined = format!("{}{}", tb, i + 1);
            let cstr = CString::new(combined).unwrap_or_default();
            mk_char_cstr(&cstr)
        }
        (None, None) => mk_char_cstr(c""),
    }
}

// ---------------------------------------------------------------------------
// do_seq — generate sequences
// ---------------------------------------------------------------------------

/// GNU's internal integer-counting kernel; public validation stays in R.
pub fn do_tabulate(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    super::tabulate::evaluate(call, op, args, rho)
}

// ---------------------------------------------------------------------------
// Parallel min/max and which.min/which.max
// ---------------------------------------------------------------------------

/// R's `pmin(...)` — parallel minimum across vectors (element-wise min with recycling).
pub fn do_pmin(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
        super::parallel_extrema::numeric(call, op, args, rho, true, false)
            .unwrap_or_else(|| do_pminmax(args, true))
    }
}

/// R's `pmax(...)` — parallel maximum across vectors (element-wise max with recycling).
pub fn do_pmax(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
        super::parallel_extrema::numeric(call, op, args, rho, false, false)
            .unwrap_or_else(|| do_pminmax(args, false))
    }
}

/// `.Internal(pmax(na.rm, ...))` wrapper: the GNU internal contract passes
/// the `na.rm` flag as the first, untagged argument. The numeric kernel
/// reads it directly; legacy character/classed paths still use retagging.
pub fn do_pmax_internal(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
        super::parallel_extrema::numeric(call, op, args, rho, false, true)
            .unwrap_or_else(|| do_pminmax(retag_internal_narm(args), false))
    }
}

/// `.Internal(pmin(na.rm, ...))` wrapper; see `do_pmax_internal`.
pub fn do_pmin_internal(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
        super::parallel_extrema::numeric(call, op, args, rho, true, true)
            .unwrap_or_else(|| do_pminmax(retag_internal_narm(args), true))
    }
}

/// Tag the leading `na.rm` flag of a GNU internal pmin/pmax argument list.
fn retag_internal_narm(args: SEXP) -> SEXP {
    if !args.is_null() && args != R_NilValue() {
        let cstring = std::ffi::CString::new("na.rm").unwrap_or_default();
        crate::sexp::accessors::SETTAG(args, install_cstr(&cstring));
    }
    args
}

fn do_pminmax(args: SEXP, is_min: bool) -> SEXP {
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
    let owner = active_owner();
    let mut result_mut = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
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
            result_mut
                .try_set_real_elt(
                    i,
                    if seen_missing && !na_rm || !seen_value {
                        NA_REAL
                    } else {
                        best
                    },
                )
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        } else {
            result_mut
                .try_set_integer_elt(
                    i,
                    if seen_missing && !na_rm || !seen_value {
                        NA_INTEGER
                    } else {
                        best as c_int
                    },
                )
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
    }
    copy_pminmax_shape(arg_vecs[0], result, max_len);
    if crate::mainutils::essentials::sexp_has_class(arg_vecs[0], "difftime") {
        let class = crate::sexp::attrib_core::getAttrib(
            arg_vecs[0],
            crate::sexp::attrib_core::R_ClassSymbol(),
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        let units_sym = install_str("units");
        crate::sexp::attrib_core::setAttrib(
            result,
            units_sym,
            crate::sexp::attrib_core::getAttrib(arg_vecs[0], units_sym),
        );
    }
    result
}
fn difftime_unit_seconds(x: SEXP) -> f64 {
    let units = crate::sexp::attrib_core::getAttrib(
        x,
        install_str("units"),
    );
    let name_owned;
    let name = if units.is_null() || units == R_NilValue() || TYPEOF(units) != SEXPTYPE::STRSXP
    {
        "secs"
    } else {
        name_owned = owner_sexp(active_owner(), units)
            .try_string_value_elt(0)
            .ok()
            .flatten()
            .unwrap_or_else(|| "secs".to_string());
        name_owned.as_str()
    };
    match name {
        "mins" => 60.0,
        "hours" => 3600.0,
        "days" => 86400.0,
        "weeks" => 604800.0,
        _ => 1.0,
    }
}

fn pminmax_character(arg_vecs: &[SEXP], max_len: R_xlen_t, is_min: bool, na_rm: bool) -> SEXP {
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
                owner_sexp(active_owner(), arg)
                    .try_string_value_elt(idx)
                    .map(|v| v.is_none())
                    .unwrap_or(true)
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
        let owner = active_owner();
        if let Ok(mut result_mut) = owner.sexp(result).and_then(SexpMut::try_from_checked) {
            if (seen_missing && !na_rm) || !seen_value {
                if let Ok(na) = owner.sexp(crate::sexp::globals::R_NaString()) {
                    let _ = result_mut.try_set_string_elt(i, na);
                }
            } else {
                let cstr = CString::new(best.clone()).unwrap_or_default();
                if let Ok(charsxp) =
                    owner.sexp(mk_char_cstr(&cstr))
                {
                    let _ = result_mut.try_set_string_elt(i, charsxp);
                }
            }
        }
    }
    copy_pminmax_shape(arg_vecs[0], result, max_len);
    result
}
fn pminmax_factor_result(
    arg_vecs: &[SEXP],
    max_len: R_xlen_t,
    is_min: bool,
    na_rm: bool,
) -> Option<SEXP> {
    let owner = arg_vecs
        .iter()
        .copied()
        .find(|arg| is_factor(*arg) != 0)?;
    let levels = crate::sexp::attrib_core::getAttrib(owner, install_str("levels"));
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
            let code = if is_factor(arg) != 0 {
                let c = owner_sexp(active_owner(), arg)
                    .try_integer_elt(idx)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
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
        if let Ok(mut result_mut) = active_owner()
            .sexp(result)
            .and_then(SexpMut::try_from_checked)
        {
            let _ = result_mut.try_set_integer_elt(
                i,
                if (missing && !na_rm) || !seen {
                    NA_INTEGER
                } else {
                    best
                },
            );
        }
    }
    let class =
        crate::sexp::attrib_core::getAttrib(owner, crate::sexp::attrib_core::R_ClassSymbol());
    crate::sexp::attrib_core::setAttrib(
        result,
        crate::sexp::attrib_core::R_ClassSymbol(),
        class,
    );
    crate::sexp::attrib_core::setAttrib(result, install_str("levels"), levels);
    Some(result)
}
/// GNU `pmin`/`pmax` copy the first argument's dimensions onto a result of
/// the same length (`mostattributes<-`).
fn copy_pminmax_shape(first: SEXP, result: SEXP, max_len: R_xlen_t) {
    if XLENGTH(first) != max_len {
        return;
    }
    let names =
        crate::sexp::attrib_core::getAttrib(first, crate::sexp::attrib_core::R_NamesSymbol());
    if !names.is_null() && names != R_NilValue() && XLENGTH(names) == max_len {
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names,
        );
    }
    let dim = crate::sexp::attrib_core::getAttrib(first, crate::sexp::attrib_core::R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        return;
    }
    crate::sexp::attrib_core::setAttrib(result, crate::sexp::attrib_core::R_DimSymbol(), dim);
    let dimnames =
        crate::sexp::attrib_core::getAttrib(first, crate::sexp::attrib_core::R_DimNamesSymbol());
    if !dimnames.is_null() && dimnames != R_NilValue() {
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dimnames,
        );
    }
}

/// GNU's first minimum index, including selected names and object transforms.
pub fn do_which_min(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    super::which_extrema::evaluate(call, op, args, rho, true)
}

/// GNU's first maximum index, including selected names and object transforms.
pub fn do_which_max(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    super::which_extrema::evaluate(call, op, args, rho, false)
}

// ---------------------------------------------------------------------------
// Data manipulation: append, head, tail, subset
// ---------------------------------------------------------------------------

/// R's `append(x, values, after)` — insert values into vector at position.
pub fn do_append(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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
        let owner = active_owner();
        let x_owned = owner_sexp(owner, x);
        let values_owned = owner_sexp(owner, values);
        let mut result_mut = owner
            .sexp(result)
            .and_then(SexpMut::try_from_checked)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        for i in 0..after {
            let elt = x_owned
                .try_vector_elt(i)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            result_mut
                .try_set_vector_elt(i, elt)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        for i in 0..vlen {
            let elt = values_owned
                .try_vector_elt(i)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            result_mut
                .try_set_vector_elt(after + i, elt)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        for i in after..n {
            let elt = x_owned
                .try_vector_elt(i)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            result_mut
                .try_set_vector_elt(i + vlen, elt)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        let xn =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        let vn = crate::sexp::attrib_core::getAttrib(
            values,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );
        if (!xn.is_null() && xn != R_NilValue()) || (!vn.is_null() && vn != R_NilValue()) {
            let names = Rf_allocVector3(SEXPTYPE::STRSXP, total);
            let xn_owned = owner_sexp(owner, xn);
            let vn_owned = owner_sexp(owner, vn);
            if let (Ok(mut names_mut), Ok(empty)) = (
                owner.sexp(names).and_then(SexpMut::try_from_checked),
                owner.sexp(mk_char_cstr(c"")),
            ) {
                for i in 0..total {
                    let (src_owned, src_raw, idx) = if i < after {
                        (&xn_owned, xn, i)
                    } else if i < after + vlen {
                        (&vn_owned, vn, i - after)
                    } else {
                        (&xn_owned, xn, i - vlen)
                    };
                    let ch = if src_raw.is_null()
                        || src_raw == R_NilValue()
                        || idx >= XLENGTH(src_raw)
                    {
                        empty.clone()
                    } else {
                        src_owned.try_string_elt(idx).unwrap_or_else(|error| {
                            crate::sexp::context::r_error(error.to_string())
                        })
                    };
                    let _ = names_mut.try_set_string_elt(i, ch);
                }
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_NamesSymbol(),
                names,
            );
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
    let owner = active_owner();
    let mut result_mut = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    fn fail<T>(error: crate::sexp::object::SexpError) -> T {
        crate::sexp::context::r_error(error.to_string())
    }
    if t == SEXPTYPE::STRSXP {
        for i in 0..after {
            let elt = owner.sexp(str_elt_or_na(x, i)).unwrap_or_else(fail);
            result_mut.try_set_string_elt(i, elt).unwrap_or_else(fail);
        }
        for i in 0..vlen {
            let elt = owner.sexp(str_elt_or_na(values, i)).unwrap_or_else(fail);
            result_mut
                .try_set_string_elt(after + i, elt)
                .unwrap_or_else(fail);
        }
        for i in after..n {
            let elt = owner.sexp(str_elt_or_na(x, i)).unwrap_or_else(fail);
            result_mut
                .try_set_string_elt(i + vlen, elt)
                .unwrap_or_else(fail);
        }
    } else if t == SEXPTYPE::REALSXP {
        for i in 0..after {
            result_mut
                .try_set_real_elt(i, elt_real_safe(x, i))
                .unwrap_or_else(fail);
        }
        for i in 0..vlen {
            result_mut
                .try_set_real_elt(after + i, elt_real_safe(values, i))
                .unwrap_or_else(fail);
        }
        for i in after..n {
            result_mut
                .try_set_real_elt(i + vlen, elt_real_safe(x, i))
                .unwrap_or_else(fail);
        }
    } else if t == SEXPTYPE::CPLXSXP {
        for i in 0..after {
            result_mut
                .try_set_complex_elt(i, cplx_elt_or_na(x, i))
                .unwrap_or_else(fail);
        }
        for i in 0..vlen {
            result_mut
                .try_set_complex_elt(after + i, cplx_elt_or_na(values, i))
                .unwrap_or_else(fail);
        }
        for i in after..n {
            result_mut
                .try_set_complex_elt(i + vlen, cplx_elt_or_na(x, i))
                .unwrap_or_else(fail);
        }
    } else if t == SEXPTYPE::INTSXP {
        for i in 0..after {
            result_mut
                .try_set_integer_elt(i, int_elt_or_na(x, i))
                .unwrap_or_else(fail);
        }
        for i in 0..vlen {
            result_mut
                .try_set_integer_elt(after + i, int_elt_or_na(values, i))
                .unwrap_or_else(fail);
        }
        for i in after..n {
            result_mut
                .try_set_integer_elt(i + vlen, int_elt_or_na(x, i))
                .unwrap_or_else(fail);
        }
    } else if t == SEXPTYPE::RAWSXP {
        for i in 0..after {
            result_mut
                .try_set_raw_elt(i, raw_elt_or_zero(x, i))
                .unwrap_or_else(fail);
        }
        for i in 0..vlen {
            result_mut
                .try_set_raw_elt(after + i, raw_elt_or_zero(values, i))
                .unwrap_or_else(fail);
        }
        for i in after..n {
            result_mut
                .try_set_raw_elt(i + vlen, raw_elt_or_zero(x, i))
                .unwrap_or_else(fail);
        }
    }
    result
}

/// Scalar `n` for the vector `head`/`tail` builtins, following `utils:::.checkHT`.
///
/// A missing argument arrives as Nil, same as an explicit NULL, so Nil keeps
/// the default 6. The closure path rejects NULL.
fn head_tail_n(n_arg: SEXP) -> i64 {
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
        crate::mainutils::objects::inherits2(n_arg, c"factor") == 0
            && crate::mainutils::objects::inherits2(n_arg, c"Date") == 0
            && crate::mainutils::objects::inherits2(n_arg, c"POSIXt") == 0
            && crate::mainutils::objects::inherits2(n_arg, c"difftime") == 0
    } else if kind == SEXPTYPE::REALSXP {
        crate::mainutils::objects::inherits2(n_arg, c"Date") == 0
            && crate::mainutils::objects::inherits2(n_arg, c"POSIXt") == 0
            && crate::mainutils::objects::inherits2(n_arg, c"difftime") == 0
    } else {
        false
    };
    if !numeric_or_logical {
        base_error("invalid 'n' - must be numeric, possibly NA.");
    }
    real_or_default(n_arg, 6.0) as i64
}

fn head_tail_n_all_missing(n_arg: SEXP, kind: c_int, nlen: R_xlen_t) -> bool {
    let owner = active_owner();
    let n_arg_owned = owner_sexp(owner, n_arg);
    if kind == SEXPTYPE::LGLSXP || kind == SEXPTYPE::INTSXP {
        for i in 0..nlen {
            if integer_like_elt(&n_arg_owned, i) != NA_INTEGER {
                return false;
            }
        }
        true
    } else if kind == SEXPTYPE::REALSXP {
        for i in 0..nlen {
            let v = n_arg_owned
                .try_real_elt(i)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            if !v.is_nan() {
                return false;
            }
        }
        true
    } else if kind == SEXPTYPE::STRSXP {
        for i in 0..nlen {
            if let Ok(Some(_)) = n_arg_owned.try_string_value_elt(i) {
                return false;
            }
        }
        true
    } else if kind == SEXPTYPE::CPLXSXP {
        for i in 0..nlen {
            let z = n_arg_owned
                .try_complex_elt(i)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            if !z.r.is_nan() && !z.i.is_nan() {
                return false;
            }
        }
        true
    } else if kind == SEXPTYPE::VECSXP {
        // all(is.na(list)): an element is missing only as a length-1 atomic NA.
        for i in 0..nlen {
            let elt = n_arg_owned
                .try_vector_elt(i)
                .map(|elt| elt.as_raw())
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            let missing = if elt.is_null()
                || elt == R_NilValue()
                || !SEXPTYPE(TYPEOF(elt)).is_vector_type()
                || XLENGTH(elt) != 1
            {
                false
            } else {
                let ek = TYPEOF(elt);
                let elt_owned = owner_sexp(owner, elt);
                if ek == SEXPTYPE::LGLSXP || ek == SEXPTYPE::INTSXP {
                    integer_like_elt(&elt_owned, 0) == NA_INTEGER
                } else if ek == SEXPTYPE::REALSXP {
                    elt_owned
                        .try_real_elt(0)
                        .map(|v| v.is_nan())
                        .unwrap_or(false)
                } else if ek == SEXPTYPE::STRSXP {
                    matches!(elt_owned.try_string_value_elt(0), Ok(None))
                } else if ek == SEXPTYPE::CPLXSXP {
                    elt_owned
                        .try_complex_elt(0)
                        .map(|z| z.r.is_nan() || z.i.is_nan())
                        .unwrap_or(false)
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

/// R's `head(x, n=6)` — first n elements.
pub fn do_head(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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

/// R's `tail(x, n=6)` — last n elements.
pub fn do_tail(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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

pub(crate) fn copy_vector_element(
    dst: SEXP,
    dst_index: R_xlen_t,
    src: SEXP,
    src_index: R_xlen_t,
    target_type: SEXPTYPE,
) {
    let owner = active_owner();
    let dst_owned = owner
        .sexp(dst)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    let src_owned = owner_sexp(owner, src);
    let mut dst_mut = dst_owned;
    fn fail<T>(error: crate::sexp::object::SexpError) -> T {
        crate::sexp::context::r_error(error.to_string())
    }
    match target_type {
        t if t == SEXPTYPE::STRSXP => {
            let elt = src_owned.try_string_elt(src_index).unwrap_or_else(fail);
            dst_mut
                .try_set_string_elt(dst_index, elt)
                .unwrap_or_else(fail);
        }
        t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
            let elt = src_owned.try_vector_elt(src_index).unwrap_or_else(fail);
            dst_mut
                .try_set_vector_elt(dst_index, elt)
                .unwrap_or_else(fail);
        }
        t if t == SEXPTYPE::REALSXP => {
            let v = src_owned.try_real_elt(src_index).unwrap_or_else(fail);
            dst_mut.try_set_real_elt(dst_index, v).unwrap_or_else(fail);
        }
        t if t == SEXPTYPE::INTSXP => {
            let v = integer_like_elt(&src_owned, src_index);
            dst_mut
                .try_set_integer_elt(dst_index, v)
                .unwrap_or_else(fail);
        }
        t if t == SEXPTYPE::LGLSXP => {
            let v = integer_like_elt(&src_owned, src_index);
            dst_mut
                .try_set_logical_elt(dst_index, v)
                .unwrap_or_else(fail);
        }
        t if t == SEXPTYPE::RAWSXP => {
            let v = src_owned.try_raw_elt(src_index).unwrap_or_else(fail);
            dst_mut.try_set_raw_elt(dst_index, v).unwrap_or_else(fail);
        }
        _ => {}
    }
}

fn slice_names_attribute(x: SEXP, result: SEXP, start: R_xlen_t, len: R_xlen_t) {
    let names = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
    if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
        return;
    }
    let sliced = Rf_allocVector3(SEXPTYPE::STRSXP, len);
    if sliced.is_null() {
        return;
    }
    let _sliced_guard = protect(sliced);
    let owner = active_owner();
    let names_owned = owner_sexp(owner, names);
    if let Ok(mut sliced_mut) = owner.sexp(sliced).and_then(SexpMut::try_from_checked) {
        for i in 0..len {
            if let Ok(elt) = names_owned.try_string_elt(start + i) {
                let _ = sliced_mut.try_set_string_elt(i, elt);
            }
        }
    }
    crate::sexp::attrib_core::setAttrib(result, crate::sexp::attrib_core::R_NamesSymbol(), sliced);
}

/// R's `x[i]` — subset extraction (simplified: integer index vector).
pub fn do_subset(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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
    let owner = active_owner();
    let x_owned = owner_sexp(owner, x);
    let mut result_mut = owner
        .sexp(result)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for j in 0..n {
        let idx = elt_real_safe(i, j) as i64;
        if idx < 1 {
            continue;
        }
        let src = (idx - 1) as R_xlen_t;
        if t == SEXPTYPE::REALSXP {
            let v = x_owned
                .try_real_elt(src)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            result_mut
                .try_set_real_elt(j, v)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        } else if t == SEXPTYPE::INTSXP {
            let v = x_owned
                .try_integer_elt(src)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            result_mut
                .try_set_integer_elt(j, v)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        } else if t == SEXPTYPE::LGLSXP {
            let v = x_owned
                .try_logical_elt(src)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            result_mut
                .try_set_logical_elt(j, v)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Type checking: is.finite, is.infinite, is.nan, is.matrix, is.array, is.list
// ---------------------------------------------------------------------------

pub(super) fn dispatch_is(
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

/// GNU `is.finite` with checked element reads and original-owner roots.
pub fn do_is_finite(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
        super::finite_predicates::evaluate(
            call,
            op,
            args,
            rho,
            super::finite_predicates::Predicate::Finite,
        )
    }
}

/// GNU `is.infinite` with checked element reads and original-owner roots.
pub fn do_is_infinite(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
        super::finite_predicates::evaluate(
            call,
            op,
            args,
            rho,
            super::finite_predicates::Predicate::Infinite,
        )
    }
}

/// GNU `is.nan` with checked element reads and original-owner roots.
pub fn do_is_nan(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    {
        super::finite_predicates::evaluate(
            call,
            op,
            args,
            rho,
            super::finite_predicates::Predicate::Nan,
        )
    }
}

/// R's `is.matrix(x)` — check if x has a dim attribute with exactly 2 dimensions.
pub fn do_is_matrix(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
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
        let dim_attr = crate::sexp::attrib_core::getAttrib(x, install_str("dim"));
        let is_mat =
            !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) == 2;
        Rf_ScalarLogical(if is_mat { TRUE } else { FALSE })
    }
}

/// R's `is.array(x)` — check if x has a dim attribute.
pub fn do_is_array(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
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
        let dim_attr = crate::sexp::attrib_core::getAttrib(x, install_str("dim"));
        let is_array = !dim_attr.is_null()
            && dim_attr != R_NilValue()
            && TYPEOF(dim_attr) == SEXPTYPE::INTSXP
            && LENGTH(dim_attr) > 0;
        Rf_ScalarLogical(if is_array { TRUE } else { FALSE })
    }
}

/// R's `is.list(x)` — check if x is a VECSXP (list).
pub fn do_is_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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

fn is_data_frame(x: SEXP) -> bool {
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

fn pminmax_data_frame(arg_vecs: &[SEXP], is_min: bool) -> Option<SEXP> {
    let frame = arg_vecs.iter().copied().find(|arg| is_data_frame(*arg))?;
    let ncol = XLENGTH(frame);
    let out = Rf_allocVector3(SEXPTYPE::VECSXP, ncol);
    let _g = protect(out);
    let owner = active_owner();
    let frame_owned = owner_sexp(owner, frame);
    let mut out_mut = owner
        .sexp(out)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for i in 0..ncol {
        let column = frame_owned
            .try_vector_elt(i)
            .map(|elt| elt.as_raw())
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let nrow = XLENGTH(column);
        let short = arg_vecs
            .iter()
            .any(|arg| !is_data_frame(*arg) && XLENGTH(*arg) != nrow && XLENGTH(*arg) > 0);
        let value = if short {
            pminmax_frame_column(column, arg_vecs, i, is_min)
        } else {
            let mut cell = R_NilValue();
            for &arg in arg_vecs.iter().rev() {
                let value = if is_data_frame(arg) {
                    owner_sexp(owner, arg)
                        .try_vector_elt(i)
                        .map(|elt| elt.as_raw())
                        .unwrap_or_else(|error| {
                            crate::sexp::context::r_error(error.to_string())
                        })
                } else {
                    arg
                };
                cell = Rf_cons(value, cell);
            }
            do_pminmax(cell, is_min)
        };
        let value_owned = owner
            .sexp(value)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        out_mut
            .try_set_vector_elt(i, value_owned)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
    let names =
        crate::sexp::attrib_core::getAttrib(frame, crate::sexp::attrib_core::R_NamesSymbol());
    if !names.is_null() && names != R_NilValue() {
        crate::sexp::attrib_core::setAttrib(
            out,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names,
        );
    }
    let rows = crate::sexp::attrib_core::getAttrib(
        frame,
        crate::sexp::attrib_core::R_RowNamesSymbol(),
    );
    if !rows.is_null() && rows != R_NilValue() {
        crate::sexp::attrib_core::setAttrib(
            out,
            crate::sexp::attrib_core::R_RowNamesSymbol(),
            rows,
        );
    }
    crate::sexp::attrib_core::setAttrib(
        out,
        crate::sexp::attrib_core::R_ClassSymbol(),
        mk_string_cstr(c"data.frame"),
    );
    Some(out)
}

fn pminmax_frame_column(column: SEXP, arg_vecs: &[SEXP], _index: R_xlen_t, is_min: bool) -> SEXP {
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
        let owner = active_owner();
        let mut current_mut = owner
            .sexp(current)
            .and_then(SexpMut::try_from_checked)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        for i in 0..nrow {
            if !change[i as usize] {
                continue;
            }
            let missing = i >= olen;
            let value = if missing {
                NA_REAL
            } else {
                elt_real_safe(arg, i)
            };
            if TYPEOF(current) == SEXPTYPE::INTSXP && !missing {
                current_mut
                    .try_set_integer_elt(i, value as c_int)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            } else if TYPEOF(current) == SEXPTYPE::INTSXP {
                current_mut
                    .try_set_integer_elt(i, NA_INTEGER)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            } else {
                current_mut
                    .try_set_real_elt(i, value)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }
        }
    }
    current
}

#[cfg(test)]
mod head_tail_n_tests {
    use std::ffi::{CStr, CString};
    use std::panic::AssertUnwindSafe;

    use crate::sexp::accessors::{
        CHAR, COMPLEX, SET_STRING_ELT, SET_VECTOR_ELT, STRING_ELT, TYPEOF, XLENGTH,
    };
    use crate::sexp::constructors::{
        Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons,
    };
    use crate::sexp::context::RError;
    use crate::sexp::ffi::{NA_INTEGER, NA_REAL, SEXP, SEXPTYPE, TRUE};
    use crate::sexp::globals::{R_NaString, R_NilValue};
    use crate::sexp::protect::protect;

    use super::{charsxp_text, do_head, do_tail, mk_char_cstr, mk_string_cstr};

    fn describe_sexp(value: SEXP) -> String {
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
                parts.push(charsxp_text(elt).unwrap_or_default());
            }
        }
        format!("len={n}:{}", parts.join(","))
    }

    fn call_ht(which: &str, x: SEXP, n: SEXP) -> String {
        let caught = std::panic::catch_unwind(AssertUnwindSafe(|| {
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

    fn letters_vector() -> SEXP {
        let out = Rf_allocVector3(SEXPTYPE::STRSXP, 26);
        let _guard = protect(out);
        for i in 0..26u8 {
            let bytes = [b'a' + i];
            let cs = CString::new(bytes.as_slice()).unwrap();
            SET_STRING_ELT(out, i as i64, mk_char_cstr(&cs));
        }
        out
    }

    fn direct_report() -> String {
        let letters = letters_vector();
        let _letters = protect(letters);
        let chr = CString::new("3").unwrap();
        let n_chr = mk_string_cstr(&chr);
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

    // SAFETY: callers activate the original managed fixture and keep returned
    // raw values live before any subsequent allocating fixture operation.
    fn scalar_complex(re: f64, im: f64) -> SEXP {
        unsafe {
            let z = Rf_allocVector3(SEXPTYPE::CPLXSXP, 1);
            let data = COMPLEX(z);
            (*data).r = re;
            (*data).i = im;
            z
        }
    }

    fn classed(x: SEXP, classes: &[&str]) -> SEXP {
        let input = crate::sexp::context::own_control_value(x);
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, classes.len() as i64);
        let _class = protect(class);
        for (i, name) in classes.iter().enumerate() {
            let cs = CString::new(*name).unwrap();
            SET_STRING_ELT(class, i as i64, mk_char_cstr(&cs));
        }
        crate::sexp::attrib_core::setAttrib(
            input.as_raw(),
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        input.as_raw()
    }

    fn list_of(elts: &[SEXP]) -> SEXP {
        let inputs: Vec<_> = elts
            .iter()
            .map(|value| crate::sexp::context::own_control_value(*value))
            .collect();
        let v = Rf_allocVector3(SEXPTYPE::VECSXP, elts.len() as i64);
        for (i, elt) in inputs.iter().enumerate() {
            SET_VECTOR_ELT(v, i as i64, elt.as_raw());
        }
        v
    }

    /// GNU R 4.6.1 `utils:::.checkHT` on the head/tail builtins.
    fn builtin_missing_and_class_report() -> String {
        unsafe {
            let letters = letters_vector();
            let _letters = protect(letters);
            let mut lines = Vec::new();
            let mut pair = |label: &str, n: SEXP| {
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
        let direct = direct_report();
        let extra = builtin_missing_and_class_report();
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
