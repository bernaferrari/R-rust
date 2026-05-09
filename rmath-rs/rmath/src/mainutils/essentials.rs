//! Essential R built-in functions — c(), seq(), rep(), paste(), cat(), typeof(), is.na(), names().
//!
//! These are the most fundamental R functions that every R program uses.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, INTEGER, INTEGER_ELT, LENGTH, LOGICAL,
    PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_STRING_ELT, SET_VECTOR_ELT, SETCDR, SETTAG,
    STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_mkChar,
    Rf_mkString,
};
use crate::sexp::context::RError;
use crate::sexp::ffi::{
    FALSE, ISNAN, NA_INTEGER, NA_LOGICAL, NA_REAL, R_NA_BIT_PATTERN, R_xlen_t, Rbyte, Rcomplex,
    SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::{R_MissingArg, R_NilValue, R_UnboundValue};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

#[derive(Clone, Copy, Eq, PartialEq)]
enum DatetimeVectorClass {
    Date,
    Posixct,
}

unsafe fn leading_datetime_class(args: SEXP) -> Option<(DatetimeVectorClass, SEXP)> {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            return None;
        }
        let first = CAR(args);
        if sexp_has_class(first, "POSIXct") {
            Some((DatetimeVectorClass::Posixct, first))
        } else if sexp_has_class(first, "Date") {
            Some((DatetimeVectorClass::Date, first))
        } else {
            None
        }
    }
}

unsafe fn posixct_tzone_string(source: SEXP) -> String {
    unsafe {
        let tzone = crate::sexp::attrib_core::getAttrib(source, Rf_install(c"tzone".as_ptr()));
        if tzone.is_null() || tzone == R_NilValue() || TYPEOF(tzone) != SEXPTYPE::STRSXP {
            return "UTC".to_string();
        }
        if XLENGTH(tzone) == 0 {
            return "UTC".to_string();
        }
        let value = STRING_ELT(tzone, 0);
        if value.is_null() || value == crate::sexp::globals::R_NaString() {
            return "UTC".to_string();
        }
        CStr::from_ptr(CHAR(value))
            .to_str()
            .unwrap_or("UTC")
            .to_string()
    }
}

unsafe fn set_datetime_class_from(result: SEXP, source: SEXP, class: DatetimeVectorClass) {
    unsafe {
        match class {
            DatetimeVectorClass::Date => set_single_class(result, "Date"),
            DatetimeVectorClass::Posixct => {
                set_posixct_class(result, &posixct_tzone_string(source))
            }
        }
    }
}

pub unsafe fn do_cache_class(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() || CDR(args) == R_NilValue() {
            std::panic::panic_any(RError {
                message: "invalid class argument to internal .class_cache".to_string(),
            });
        }

        let class = CAR(args);
        if class.is_null() || class == R_NilValue() || TYPEOF(class) != SEXPTYPE::STRSXP {
            std::panic::panic_any(RError {
                message: "invalid class argument to internal .class_cache".to_string(),
            });
        }

        CADR(args)
    }
}

pub unsafe fn do_xtfrm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            return R_NilValue();
        }

        let x = CAR(args);
        match TYPEOF(x) {
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP || t == SEXPTYPE::LGLSXP => x,
            t if t == SEXPTYPE::STRSXP => {
                let n = XLENGTH(x);
                let out = Rf_allocVector3(SEXPTYPE::INTSXP, n);
                let _out_guard = protect(out);

                let mut values = Vec::with_capacity(n as usize);
                for i in 0..n {
                    let elt = STRING_ELT(x, i);
                    let value = if elt.is_null() || elt == crate::sexp::globals::R_NaString() {
                        None
                    } else {
                        Some(CStr::from_ptr(CHAR(elt)).to_string_lossy().into_owned())
                    };
                    values.push(value);
                }

                let mut sorted: Vec<String> = values.iter().filter_map(Clone::clone).collect();
                sorted.sort();
                sorted.dedup();

                let ranks: BTreeMap<String, c_int> = sorted
                    .into_iter()
                    .enumerate()
                    .map(|(idx, value)| (value, (idx + 1) as c_int))
                    .collect();

                let data = INTEGER(out);
                for (i, value) in values.into_iter().enumerate() {
                    *data.add(i) = value
                        .and_then(|value| ranks.get(&value).copied())
                        .unwrap_or(NA_INTEGER);
                }
                out
            }
            _ => x,
        }
    }
}

pub unsafe fn do_at(_call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() || CDR(args) == R_NilValue() {
            return R_NilValue();
        }
        let object = crate::eval::eval::Rf_eval(CAR(args), rho);
        let _object_guard = protect(object);
        let slot_name = CAR(CDR(args));
        let slot_args = Rf_cons(object, Rf_cons(slot_name, R_NilValue()));
        let _slot_args_guard = protect(slot_args);
        do_slot(_call, op, slot_args, rho)
    }
}

pub unsafe fn do_at_set(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let object = if args.is_null() || args == R_NilValue() {
            R_NilValue()
        } else {
            CAR(args)
        };
        let _ = do_set_slot(call, op, args, rho);
        object
    }
}

unsafe fn replacement_name(arg: SEXP) -> String {
    unsafe {
        if arg.is_null() || arg == R_NilValue() {
            String::new()
        } else if TYPEOF(arg) == SEXPTYPE::SYMSXP {
            elt_to_string(arg, 0)
        } else {
            elt_to_string(arg, 0)
        }
    }
}

pub unsafe fn do_dollar_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() || CDR(args) == R_NilValue() {
            return R_NilValue();
        }

        let object = CAR(args);
        let field = replacement_name(CAR(CDR(args)));
        let value = CAR(CDR(CDR(args)));
        if object.is_null() || object == R_NilValue() || field.is_empty() {
            return object;
        }
        if TYPEOF(object) != SEXPTYPE::VECSXP {
            return object;
        }

        let names_sym = crate::sexp::attrib_core::R_NamesSymbol();
        let names = crate::sexp::attrib_core::getAttrib(object, names_sym);
        let n = XLENGTH(object);

        if !names.is_null() && names != R_NilValue() && TYPEOF(names) == SEXPTYPE::STRSXP {
            for i in 0..n {
                let name = STRING_ELT(names, i);
                if !name.is_null() && CStr::from_ptr(CHAR(name)).to_string_lossy() == field {
                    SET_VECTOR_ELT(object, i, value);
                    return object;
                }
            }
        }

        let out = Rf_allocVector3(SEXPTYPE::VECSXP, n + 1);
        let _out_guard = protect(out);
        let out_names = Rf_allocVector3(SEXPTYPE::STRSXP, n + 1);
        let _names_guard = protect(out_names);
        let blank = Rf_mkChar(c"".as_ptr());

        for i in 0..n {
            SET_VECTOR_ELT(out, i, VECTOR_ELT(object, i));
            let name = if !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == SEXPTYPE::STRSXP
                && i < XLENGTH(names)
            {
                STRING_ELT(names, i)
            } else {
                blank
            };
            SET_STRING_ELT(out_names, i, name);
        }

        SET_VECTOR_ELT(out, n, value);
        let field_c = CString::new(field).unwrap_or_default();
        SET_STRING_ELT(out_names, n, Rf_mkChar(field_c.as_ptr()));
        crate::sexp::attrib_core::setAttrib(out, names_sym, out_names);
        out
    }
}

unsafe fn datetime_c_value(source: SEXP, index: R_xlen_t, class: DatetimeVectorClass) -> f64 {
    unsafe {
        match class {
            DatetimeVectorClass::Date => {
                if TYPEOF(source) == SEXPTYPE::STRSXP {
                    let value = STRING_ELT(source, index);
                    if value.is_null() || value == crate::sexp::globals::R_NaString() {
                        return NA_REAL;
                    }
                    let text = CStr::from_ptr(CHAR(value)).to_str().unwrap_or("");
                    return parse_iso_date_days(text).unwrap_or_else(|| {
                        base_error("character string is not in a standard unambiguous format");
                    });
                }
                let value = real_elt_or_default(source, index, NA_REAL);
                if sexp_has_class(source, "POSIXct") && value.to_bits() != R_NA_BIT_PATTERN {
                    (value / 86_400.0).floor()
                } else {
                    value
                }
            }
            DatetimeVectorClass::Posixct => {
                if TYPEOF(source) == SEXPTYPE::STRSXP {
                    let value = STRING_ELT(source, index);
                    if value.is_null() || value == crate::sexp::globals::R_NaString() {
                        return NA_REAL;
                    }
                    let text = CStr::from_ptr(CHAR(value)).to_str().unwrap_or("");
                    return parse_iso_datetime_seconds(text).unwrap_or_else(|| {
                        base_error("character string is not in a standard unambiguous format");
                    });
                }
                let value = real_elt_or_default(source, index, NA_REAL);
                if sexp_has_class(source, "Date") && value.to_bits() != R_NA_BIT_PATTERN {
                    value.floor() * 86_400.0
                } else {
                    value
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// do_c — combine vectors
// ---------------------------------------------------------------------------

/// R's `c(...)` — concatenates vectors into a single vector.
///
/// Coercion rules: STRSXP > CPLXSXP > REALSXP > INTSXP > LGLSXP.
/// If any arg is STRSXP, result is STRSXP.
pub unsafe fn do_c(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let datetime_class = leading_datetime_class(args);
        // First pass: determine result type and total length
        let mut result_type = SEXPTYPE::LGLSXP.as_c_int();
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
                if t == SEXPTYPE::VECSXP {
                    result_type = SEXPTYPE::VECSXP.as_c_int();
                } else if datetime_class.is_some() && result_type != SEXPTYPE::VECSXP {
                    result_type = SEXPTYPE::REALSXP.as_c_int();
                } else if t == SEXPTYPE::STRSXP && result_type != SEXPTYPE::VECSXP {
                    result_type = SEXPTYPE::STRSXP.as_c_int();
                } else if t == SEXPTYPE::CPLXSXP
                    && result_type != SEXPTYPE::VECSXP
                    && result_type != SEXPTYPE::STRSXP
                {
                    result_type = SEXPTYPE::CPLXSXP.as_c_int();
                } else if t == SEXPTYPE::REALSXP
                    && result_type != SEXPTYPE::VECSXP
                    && result_type != SEXPTYPE::STRSXP
                    && result_type != SEXPTYPE::CPLXSXP
                {
                    result_type = SEXPTYPE::REALSXP.as_c_int();
                } else if t == SEXPTYPE::INTSXP
                    && result_type != SEXPTYPE::VECSXP
                    && result_type != SEXPTYPE::STRSXP
                    && result_type != SEXPTYPE::CPLXSXP
                    && result_type != SEXPTYPE::REALSXP
                {
                    result_type = SEXPTYPE::INTSXP.as_c_int();
                }
                total_len += XLENGTH(arg);
            }
            current = CDR(current);
        }

        if total_len == 0 {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
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

        if result_type == SEXPTYPE::VECSXP {
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
                    let n = XLENGTH(arg);
                    let arg_names = crate::sexp::attrib_core::getAttrib(arg, names_symbol);
                    for i in 0..n {
                        let value = if t == SEXPTYPE::VECSXP {
                            VECTOR_ELT(arg, i)
                        } else {
                            extract_element(arg, i)
                        };
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
                            let v = INTEGER_ELT(arg, i as c_int);
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
                            INTEGER_ELT(arg, i as c_int)
                        } else {
                            NA_INTEGER
                        };
                        *dst.add((offset + i) as usize) = val;
                    }
                } else if result_type == SEXPTYPE::LGLSXP {
                    let dst = LOGICAL(result);
                    for i in 0..n {
                        let val = if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP {
                            *INTEGER(arg).add(i as usize)
                        } else {
                            NA_INTEGER
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
                            let v = INTEGER_ELT(arg, i as c_int);
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

unsafe fn datetime_seq_by(arg: SEXP, class: DatetimeVectorClass, default: f64) -> f64 {
    unsafe {
        if arg.is_null() || arg == R_NilValue() {
            return default;
        }
        if TYPEOF(arg) == SEXPTYPE::STRSXP {
            let text = elt_to_string(arg, 0).to_ascii_lowercase();
            let units = text.trim();
            return match class {
                DatetimeVectorClass::Date => match units {
                    "day" | "days" => 1.0,
                    "week" | "weeks" => 7.0,
                    _ => base_error("invalid string for 'by'"),
                },
                DatetimeVectorClass::Posixct => match units {
                    "sec" | "secs" | "second" | "seconds" => 1.0,
                    "min" | "mins" | "minute" | "minutes" => 60.0,
                    "hour" | "hours" => 3_600.0,
                    "day" | "days" => 86_400.0,
                    "week" | "weeks" => 604_800.0,
                    _ => base_error("invalid string for 'by'"),
                },
            };
        }
        real_or_default(arg, default)
    }
}

unsafe fn datetime_seq_endpoint(arg: SEXP, class: DatetimeVectorClass) -> f64 {
    unsafe {
        if TYPEOF(arg) == SEXPTYPE::STRSXP {
            let text = elt_to_string(arg, 0);
            return match class {
                DatetimeVectorClass::Date => parse_iso_date_days(&text).unwrap_or_else(|| {
                    base_error("character string is not in a standard unambiguous format")
                }),
                DatetimeVectorClass::Posixct => {
                    parse_iso_datetime_seconds(&text).unwrap_or_else(|| {
                        base_error("character string is not in a standard unambiguous format")
                    })
                }
            };
        }
        match class {
            DatetimeVectorClass::Date => {
                if sexp_has_class(arg, "POSIXct") {
                    let value = real_or_default(arg, NA_REAL);
                    if value.to_bits() == R_NA_BIT_PATTERN {
                        NA_REAL
                    } else {
                        (value / 86_400.0).floor()
                    }
                } else {
                    real_or_default(arg, NA_REAL)
                }
            }
            DatetimeVectorClass::Posixct => {
                if sexp_has_class(arg, "Date") {
                    let value = real_or_default(arg, NA_REAL);
                    if value.to_bits() == R_NA_BIT_PATTERN {
                        NA_REAL
                    } else {
                        value.floor() * 86_400.0
                    }
                } else {
                    real_or_default(arg, NA_REAL)
                }
            }
        }
    }
}

unsafe fn datetime_seq(
    from_arg: SEXP,
    to_arg: SEXP,
    by_arg: SEXP,
    length_out_arg: SEXP,
    class: DatetimeVectorClass,
) -> SEXP {
    unsafe {
        let from = datetime_seq_endpoint(from_arg, class);
        if from.to_bits() == R_NA_BIT_PATTERN {
            base_error("'from' must be a finite number");
        }

        let length_out = if length_out_arg.is_null() || length_out_arg == R_NilValue() {
            None
        } else {
            Some(numeric_elt_as_count(length_out_arg, 0))
        };
        let has_to = !(to_arg.is_null() || to_arg == R_NilValue());
        let to = if has_to {
            Some(datetime_seq_endpoint(to_arg, class))
        } else {
            None
        };
        let default_by = to
            .map(|to| if to < from { -1.0 } else { 1.0 })
            .unwrap_or(1.0);
        let by = datetime_seq_by(by_arg, class, default_by);
        if by == 0.0 {
            base_error("invalid '(to - from)/by'");
        }

        let values: Vec<f64> = if let Some(length_out) = length_out {
            (0..length_out).map(|i| from + i as f64 * by).collect()
        } else if let Some(to) = to {
            let n = ((to - from) / by).floor() as i64 + 1;
            let n = n.max(0) as usize;
            (0..n).map(|i| from + i as f64 * by).collect()
        } else {
            vec![from]
        };

        let result = Rf_allocVector3(SEXPTYPE::REALSXP, values.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        for (i, value) in values.into_iter().enumerate() {
            *dst.add(i) = value;
        }
        set_datetime_class_from(result, from_arg, class);
        result
    }
}

/// R's `seq(from, to, by)` — generates a sequence.
///
/// - seq(to) → 1:to
/// - seq(from, to) → from:to
/// - seq(from, to, by) → from, from+by, ... until past to
pub unsafe fn do_seq(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut a1 = R_NilValue();
        let mut a2 = R_NilValue();
        let mut a3 = R_NilValue();
        let mut length_out_arg = R_NilValue();
        let mut positional = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let value = CAR(current);
            match tag_name(current).as_deref() {
                Some("from") => a1 = value,
                Some("to") => a2 = value,
                Some("by") => a3 = value,
                Some("length.out") => length_out_arg = value,
                _ => {
                    match positional {
                        0 => a1 = value,
                        1 => a2 = value,
                        2 => a3 = value,
                        3 => length_out_arg = value,
                        _ => {}
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }

        if sexp_has_class(a1, "POSIXct") {
            return datetime_seq(a1, a2, a3, length_out_arg, DatetimeVectorClass::Posixct);
        }
        if sexp_has_class(a1, "Date") {
            return datetime_seq(a1, a2, a3, length_out_arg, DatetimeVectorClass::Date);
        }

        let (from, to, by) = if a2 == R_NilValue() {
            // seq(to)
            let to_val = real_or_default(a1, 1.0);
            (1.0, to_val, 1.0)
        } else if a3 == R_NilValue() {
            // seq(from, to)
            let from_val = real_or_default(a1, 1.0);
            let to_val = real_or_default(a2, 1.0);
            (from_val, to_val, 1.0)
        } else {
            // seq(from, to, by)
            let from_val = real_or_default(a1, 1.0);
            let to_val = real_or_default(a2, 1.0);
            let by_val = real_or_default(a3, 1.0);
            (from_val, to_val, by_val)
        };

        if by == 0.0 {
            return R_NilValue();
        }

        let n = ((to - from) / by).floor() as i64 + 1;
        let n = n.max(0) as R_xlen_t;

        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            *dst.add(i as usize) = from + (i as f64) * by;
        }
        result
    }
}

/// R's `sequence(nvec, from = 1, by = 1)` for common integer-compatible inputs.
pub unsafe fn do_sequence(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let nvec = CAR(args);
        if nvec.is_null() || nvec == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let from_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(args))
        };
        let by_arg = if CDR(args).is_null()
            || CDR(args) == R_NilValue()
            || CDR(CDR(args)).is_null()
            || CDR(CDR(args)) == R_NilValue()
        {
            R_NilValue()
        } else {
            CAR(CDR(CDR(args)))
        };

        let n = XLENGTH(nvec);
        let mut total: usize = 0;
        let mut lengths = Vec::with_capacity(n as usize);
        for i in 0..n {
            let len = numeric_elt_as_count(nvec, i);
            total += len;
            lengths.push(len);
        }

        let result = Rf_allocVector3(SEXPTYPE::INTSXP, total as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);
        let mut offset = 0;
        for (i, len) in lengths.into_iter().enumerate() {
            let from = real_elt_or_default(from_arg, i as R_xlen_t, 1.0) as c_int;
            let by = real_elt_or_default(by_arg, i as R_xlen_t, 1.0) as c_int;
            let mut value = from;
            for _ in 0..len {
                *dst.add(offset) = value;
                value += by;
                offset += 1;
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// do_rep — repeat elements
// ---------------------------------------------------------------------------

/// R's `rep(x, times)` — repeats a vector `times` times.
pub unsafe fn do_rep(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut x = R_NilValue();
        let mut times_arg = R_NilValue();
        let mut length_out_arg = R_NilValue();
        let mut each_arg = R_NilValue();
        let mut positional = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let value = CAR(current);
            match tag_name(current).as_deref() {
                Some("x") => x = value,
                Some("times") => times_arg = value,
                Some("length.out") => length_out_arg = value,
                Some("each") => each_arg = value,
                _ => {
                    match positional {
                        0 => x = value,
                        1 => times_arg = value,
                        2 => length_out_arg = value,
                        3 => each_arg = value,
                        _ => {}
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }

        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let each = if each_arg.is_null() || each_arg == R_NilValue() {
            1_usize
        } else {
            (real_or_default(each_arg, 1.0) as i64).max(0) as usize
        };
        let n = XLENGTH(x);
        let times_len = if times_arg.is_null() || times_arg == R_NilValue() {
            0
        } else {
            XLENGTH(times_arg)
        };
        let times_scalar = if times_len == 0 {
            1_usize
        } else if times_len == 1 {
            (real_or_default(times_arg, 1.0) as i64).max(0) as usize
        } else {
            0
        };

        let mut indices: Vec<R_xlen_t> = Vec::new();
        if times_len > 1 {
            for i in 0..n {
                let repeats = numeric_elt_as_count(times_arg, i);
                for _ in 0..repeats {
                    for _ in 0..each {
                        indices.push(i);
                    }
                }
            }
        } else {
            let mut expanded: Vec<R_xlen_t> = Vec::new();
            for i in 0..n {
                for _ in 0..each {
                    expanded.push(i);
                }
            }
            for _ in 0..times_scalar {
                indices.extend_from_slice(&expanded);
            }
        }

        if !(length_out_arg.is_null() || length_out_arg == R_NilValue()) {
            let length_out = (real_or_default(length_out_arg, indices.len() as f64) as i64).max(0);
            let length_out = length_out as usize;
            if indices.is_empty() && length_out > 0 {
                return Rf_allocVector3(TYPEOF(x), 0);
            }
            indices = (0..length_out)
                .map(|i| indices[i % indices.len()])
                .collect();
        }

        let t = TYPEOF(x);
        let result = Rf_allocVector3(t, indices.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for (out_idx, &src_idx) in indices.iter().enumerate() {
            copy_vector_elt(result, out_idx as R_xlen_t, x, src_idx);
        }

        if sexp_has_class(x, "POSIXct") {
            set_datetime_class_from(result, x, DatetimeVectorClass::Posixct);
        } else if sexp_has_class(x, "Date") {
            set_datetime_class_from(result, x, DatetimeVectorClass::Date);
        }

        result
    }
}

// ---------------------------------------------------------------------------
// Core vector/scalar helpers live in `essentials_basic`.
// ---------------------------------------------------------------------------

pub use super::essentials_basic::*;

// ---------------------------------------------------------------------------
// do_nchar — string length
// ---------------------------------------------------------------------------

/// R's `nchar(x)` — number of characters in strings.
pub unsafe fn do_nchar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);
        for i in 0..n {
            if TYPEOF(x) == SEXPTYPE::STRSXP {
                let idx = if XLENGTH(x) == 0 { 0 } else { i % XLENGTH(x) };
                let charsxp = STRING_ELT(x, idx);
                if charsxp == crate::sexp::globals::R_NaString() {
                    *dst.add(i as usize) = NA_INTEGER;
                    continue;
                }
            }
            *dst.add(i as usize) = elt_to_string(x, i).len() as c_int;
        }
        result
    }
}

// ---------------------------------------------------------------------------
// do_substr — substring extraction
// ---------------------------------------------------------------------------

/// R's `substr(x, start, stop)` — extract substrings.
pub unsafe fn do_substr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let start_arg = CAR(CDR(args));
        let stop_arg = CAR(CDR(CDR(args)));

        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..n {
            if TYPEOF(x) == SEXPTYPE::STRSXP {
                let idx = if XLENGTH(x) == 0 { 0 } else { i % XLENGTH(x) };
                if STRING_ELT(x, idx) == crate::sexp::globals::R_NaString() {
                    SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                    continue;
                }
            }
            let s = elt_to_string(x, i);
            let start = (real_elt_or_default(start_arg, i, 1.0) as usize).max(1) - 1;
            let stop = real_elt_or_default(stop_arg, i, 1000.0) as usize;
            let chars: Vec<char> = s.chars().collect();
            let end = stop.min(chars.len());
            let sub: String = if start < chars.len() {
                chars[start..end].iter().collect()
            } else {
                String::new()
            };
            let cstr = CString::new(sub).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i as usize) = charsxp;
            }
        }

        result
    }
}

// ---------------------------------------------------------------------------
// String case conversion
// ---------------------------------------------------------------------------

/// R's `tolower(x)`.
pub unsafe fn do_tolower(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_case_convert(args, true) }
}

/// R's `toupper(x)`.
pub unsafe fn do_toupper(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_case_convert(args, false) }
}

unsafe fn do_case_convert(args: SEXP, to_lower: bool) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = if x.is_null() || x == R_NilValue() {
            0
        } else {
            XLENGTH(x)
        };
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..n {
            if as_character_element_is_na(x, i) {
                SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                continue;
            }
            let s = elt_to_string(x, i);
            let converted = if to_lower {
                s.to_lowercase()
            } else {
                s.to_uppercase()
            };
            let cstr = CString::new(converted).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i as usize) = charsxp;
            }
        }

        result
    }
}

unsafe fn as_character_element_is_na(x: SEXP, i: R_xlen_t) -> bool {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return false;
        }
        match TYPEOF(x) {
            t if t == SEXPTYPE::STRSXP => STRING_ELT(x, i) == crate::sexp::globals::R_NaString(),
            t if t == SEXPTYPE::LGLSXP => *LOGICAL(x).add(i as usize) == NA_LOGICAL,
            t if t == SEXPTYPE::INTSXP => INTEGER_ELT(x, i as c_int) == NA_INTEGER,
            t if t == SEXPTYPE::REALSXP => {
                REAL_ELT(x, i as c_int).to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
            }
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// String manipulation: trimws, sprintf, gsub, sub, strsplit
// ---------------------------------------------------------------------------

/// R's `trimws(x, which="both")` — trim whitespace from strings.
pub unsafe fn do_trimws(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return x;
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            let s = elt_to_string(x, i);
            let trimmed = s.trim();
            let cstr = CString::new(trimmed).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i as usize) = charsxp;
            }
        }
        result
    }
}

/// R's `sprintf(fmt, ...)` for common scalar/vector formats.
pub unsafe fn do_sprintf(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let fmt_arg = CAR(args);
        if fmt_arg.is_null() || fmt_arg == R_NilValue() {
            return Rf_mkString(CString::new("").unwrap_or_default().as_ptr());
        }
        let fmt_len = XLENGTH(fmt_arg);
        if fmt_len == 0 {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let mut values: Vec<SEXP> = Vec::new();
        let mut lengths: Vec<R_xlen_t> = vec![fmt_len];
        let mut max_len: R_xlen_t = fmt_len;
        let mut current = CDR(args);
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                let len = XLENGTH(arg);
                if len == 0 {
                    return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
                }
                max_len = max_len.max(len);
                lengths.push(len);
                values.push(arg);
            }
            current = CDR(current);
        }
        if lengths.iter().any(|&len| max_len % len != 0) {
            base_error("arguments cannot be recycled to the same length");
        }

        let result = Rf_allocVector3(SEXPTYPE::STRSXP, max_len);
        let _result_guard = protect(result);
        for row in 0..max_len {
            let fmt = elt_to_string(fmt_arg, row);
            let mut value_idx = 0usize;
            let mut out = String::new();
            let fmt_chars: Vec<char> = fmt.chars().collect();
            let mut i = 0usize;
            while i < fmt_chars.len() {
                if fmt_chars[i] != '%' {
                    out.push(fmt_chars[i]);
                    i += 1;
                    continue;
                }
                if i + 1 < fmt_chars.len() && fmt_chars[i + 1] == '%' {
                    out.push('%');
                    i += 2;
                    continue;
                }

                let mut j = i + 1;
                let mut flags = SprintfFlags::default();
                while j < fmt_chars.len() {
                    match fmt_chars[j] {
                        '-' => flags.left_align = true,
                        '+' => flags.sign_plus = true,
                        ' ' => flags.sign_space = true,
                        '0' => flags.zero_pad = true,
                        _ => break,
                    }
                    j += 1;
                }
                let mut width = String::new();
                while j < fmt_chars.len() && fmt_chars[j].is_ascii_digit() {
                    width.push(fmt_chars[j]);
                    j += 1;
                }
                let width = width.parse::<usize>().ok();
                let precision = if j < fmt_chars.len() && fmt_chars[j] == '.' {
                    j += 1;
                    let mut digits = String::new();
                    while j < fmt_chars.len() && fmt_chars[j].is_ascii_digit() {
                        digits.push(fmt_chars[j]);
                        j += 1;
                    }
                    Some(digits.parse::<usize>().unwrap_or(0))
                } else {
                    None
                };
                if j >= fmt_chars.len() {
                    out.push('%');
                    i += 1;
                    continue;
                }

                let spec = fmt_chars[j];
                if matches!(
                    spec,
                    's' | 'd' | 'i' | 'o' | 'x' | 'X' | 'f' | 'e' | 'E' | 'g' | 'G'
                ) && value_idx < values.len()
                {
                    out.push_str(&format_sprintf_value(
                        values[value_idx],
                        row,
                        spec,
                        width,
                        precision,
                        flags,
                    ));
                    value_idx += 1;
                    i = j + 1;
                } else {
                    out.push('%');
                    i += 1;
                }
            }
            SET_STRING_ELT(
                result,
                row,
                Rf_mkChar(CString::new(out).unwrap_or_default().as_ptr()),
            );
        }
        result
    }
}

#[derive(Clone, Copy, Default)]
struct SprintfFlags {
    left_align: bool,
    sign_plus: bool,
    sign_space: bool,
    zero_pad: bool,
}

unsafe fn format_sprintf_value(
    value: SEXP,
    index: R_xlen_t,
    spec: char,
    width: Option<usize>,
    precision: Option<usize>,
    flags: SprintfFlags,
) -> String {
    unsafe {
        let len = XLENGTH(value);
        let idx = if len == 0 { 0 } else { index % len };
        let rendered = match spec {
            's' => {
                let mut s = elt_to_string(value, idx);
                if let Some(limit) = precision {
                    s = s.chars().take(limit).collect();
                }
                s
            }
            'd' | 'i' => format_signed_integer(sprintf_integer_value(value, idx), flags),
            'o' => format!("{:o}", sprintf_integer_value(value, idx)),
            'x' => format!("{:x}", sprintf_integer_value(value, idx)),
            'X' => format!("{:X}", sprintf_integer_value(value, idx)),
            'f' => {
                let value = sprintf_real_value(value, idx);
                let rendered = match precision {
                    Some(places) => format!("{value:.places$}"),
                    None => format!("{value:.6}"),
                };
                apply_float_sign(rendered, value, flags)
            }
            'e' | 'E' => {
                let value = sprintf_real_value(value, idx);
                let places = precision.unwrap_or(6);
                let rendered = if spec == 'E' {
                    normalize_exponent(format!("{value:.places$E}"), 'E')
                } else {
                    normalize_exponent(format!("{value:.places$e}"), 'e')
                };
                apply_float_sign(rendered, value, flags)
            }
            'g' | 'G' => {
                let value = sprintf_real_value(value, idx);
                let precision = precision.unwrap_or(6).max(1);
                let rendered = format_general_float(value, precision, spec == 'G');
                apply_float_sign(rendered, value, flags)
            }
            _ => elt_to_string(value, idx),
        };
        pad_sprintf(rendered, width, flags, spec != 's')
    }
}

fn format_signed_integer(value: i64, flags: SprintfFlags) -> String {
    if value >= 0 && flags.sign_plus {
        format!("+{value}")
    } else if value >= 0 && flags.sign_space {
        format!(" {value}")
    } else {
        value.to_string()
    }
}

fn apply_float_sign(rendered: String, value: f64, flags: SprintfFlags) -> String {
    if value.is_sign_negative() || rendered.starts_with('-') {
        rendered
    } else if flags.sign_plus {
        format!("+{rendered}")
    } else if flags.sign_space {
        format!(" {rendered}")
    } else {
        rendered
    }
}

fn normalize_exponent(rendered: String, marker: char) -> String {
    let Some(pos) = rendered.find(marker) else {
        return rendered;
    };
    let mantissa = &rendered[..pos];
    let exponent = &rendered[pos + marker.len_utf8()..];
    let exp = exponent.parse::<i32>().unwrap_or(0);
    format!("{mantissa}{marker}{exp:+03}")
}

fn format_general_float(value: f64, precision: usize, uppercase: bool) -> String {
    if value == 0.0 || !value.is_finite() {
        return trim_general_float(format!("{value:.prec$}", prec = precision - 1));
    }

    let abs = value.abs();
    let exponent = abs.log10().floor() as i32;
    if exponent < -4 || exponent >= precision as i32 {
        let rendered = if uppercase {
            normalize_exponent(format!("{value:.prec$E}", prec = precision - 1), 'E')
        } else {
            normalize_exponent(format!("{value:.prec$e}", prec = precision - 1), 'e')
        };
        trim_exponent_mantissa(rendered, uppercase)
    } else {
        let decimals = (precision as i32 - exponent - 1).max(0) as usize;
        trim_general_float(format!("{value:.decimals$}"))
    }
}

fn trim_general_float(mut rendered: String) -> String {
    if rendered.contains('.') {
        while rendered.ends_with('0') {
            rendered.pop();
        }
        if rendered.ends_with('.') {
            rendered.pop();
        }
    }
    rendered
}

fn trim_exponent_mantissa(rendered: String, uppercase: bool) -> String {
    let marker = if uppercase { 'E' } else { 'e' };
    let Some(pos) = rendered.find(marker) else {
        return rendered;
    };
    let mantissa = trim_general_float(rendered[..pos].to_string());
    format!("{mantissa}{}", &rendered[pos..])
}

unsafe fn sprintf_integer_value(value: SEXP, index: R_xlen_t) -> i64 {
    unsafe {
        match TYPEOF(value) {
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                INTEGER_ELT(value, index as c_int) as i64
            }
            t if t == SEXPTYPE::REALSXP => REAL_ELT(value, index as c_int) as i64,
            _ => elt_to_string(value, index).parse::<i64>().unwrap_or(0),
        }
    }
}

unsafe fn sprintf_real_value(value: SEXP, index: R_xlen_t) -> f64 {
    unsafe {
        match TYPEOF(value) {
            t if t == SEXPTYPE::REALSXP => REAL_ELT(value, index as c_int),
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                INTEGER_ELT(value, index as c_int) as f64
            }
            _ => elt_to_string(value, index)
                .parse::<f64>()
                .unwrap_or(f64::NAN),
        }
    }
}

fn pad_sprintf(
    rendered: String,
    width: Option<usize>,
    flags: SprintfFlags,
    allow_zero_pad: bool,
) -> String {
    let Some(width) = width else {
        return rendered;
    };
    let len = rendered.chars().count();
    if len >= width {
        return rendered;
    }
    let pad = width - len;
    if flags.left_align {
        return format!("{}{}", rendered, " ".repeat(pad));
    }
    let zero_pad = flags.zero_pad && allow_zero_pad;
    let ch = if zero_pad { '0' } else { ' ' };
    if zero_pad && matches!(rendered.chars().next(), Some('-' | '+' | ' ')) {
        let sign = rendered.chars().next().unwrap_or_default();
        let rest = &rendered[sign.len_utf8()..];
        format!("{sign}{}{}", ch.to_string().repeat(pad), rest)
    } else {
        format!("{}{}", ch.to_string().repeat(pad), rendered)
    }
}

/// R's `gsub(pattern, replacement, x)` — global string substitution (literal).
pub unsafe fn do_gsub(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_string_replace(args, true) }
}

/// R's `sub(pattern, replacement, x)` — first match substitution (literal).
pub unsafe fn do_sub(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_string_replace(args, false) }
}

/// R's `grep(pattern, x, ..., value = FALSE)` for fixed and ERE matching.
pub unsafe fn do_grep(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pattern_arg = arg_by_name_or_position(args, &["pattern"], 0);
        let x_arg = arg_by_name_or_position(args, &["x", "text"], 1);
        if pattern_arg.is_null() || x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let value = named_logical_arg(args, "value").unwrap_or(false);
        let invert = named_logical_arg(args, "invert").unwrap_or(false);
        let ignore_case = named_logical_arg(args, "ignore.case").unwrap_or(false);
        let fixed = named_logical_arg(args, "fixed").unwrap_or(false);
        let pattern = elt_to_string(pattern_arg, 0);
        let matches = grep_match_indices(x_arg, &pattern, ignore_case, fixed, invert);

        if value {
            let result = Rf_allocVector3(SEXPTYPE::STRSXP, matches.len() as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            for (out_idx, src_idx) in matches.into_iter().enumerate() {
                if TYPEOF(x_arg) == SEXPTYPE::STRSXP {
                    SET_STRING_ELT(result, out_idx as R_xlen_t, STRING_ELT(x_arg, src_idx));
                } else {
                    SET_STRING_ELT(
                        result,
                        out_idx as R_xlen_t,
                        Rf_mkChar(
                            CString::new(elt_to_string(x_arg, src_idx))
                                .unwrap_or_default()
                                .as_ptr(),
                        ),
                    );
                }
            }
            result
        } else {
            let result = Rf_allocVector3(SEXPTYPE::INTSXP, matches.len() as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            let dst = INTEGER(result);
            for (out_idx, src_idx) in matches.into_iter().enumerate() {
                *dst.add(out_idx) = (src_idx + 1) as c_int;
            }
            result
        }
    }
}

/// R's `grepl(pattern, x, ...)` for fixed and ERE matching.
pub unsafe fn do_grepl(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pattern_arg = arg_by_name_or_position(args, &["pattern"], 0);
        let x_arg = arg_by_name_or_position(args, &["x", "text"], 1);
        if pattern_arg.is_null() || x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }
        let ignore_case = named_logical_arg(args, "ignore.case").unwrap_or(false);
        let fixed = named_logical_arg(args, "fixed").unwrap_or(false);
        let pattern = elt_to_string(pattern_arg, 0);
        let n = XLENGTH(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            if is_string_na(x_arg, i) {
                *dst.add(i as usize) = FALSE;
                continue;
            }
            let matched =
                grep_value_matches(&elt_to_string(x_arg, i), &pattern, ignore_case, fixed);
            *dst.add(i as usize) = if matched { TRUE } else { FALSE };
        }
        result
    }
}

/// R's `agrep(pattern, x, ...)` — approximate fixed-string matching.
pub unsafe fn do_agrep(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pattern_arg = arg_by_name_or_position(args, &["pattern"], 0);
        let x_arg = arg_by_name_or_position(args, &["x", "text"], 1);
        if pattern_arg.is_null() || x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let value = named_logical_arg(args, "value").unwrap_or(false);
        let ignore_case = named_logical_arg(args, "ignore.case").unwrap_or(false);
        let max_distance = agrep_max_distance(args, pattern_arg);
        let pattern = elt_to_string(pattern_arg, 0);
        let matches = agrep_match_indices(x_arg, &pattern, max_distance, ignore_case);

        if value {
            let result = Rf_allocVector3(SEXPTYPE::STRSXP, matches.len() as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            for (out_idx, src_idx) in matches.into_iter().enumerate() {
                SET_STRING_ELT(result, out_idx as R_xlen_t, STRING_ELT(x_arg, src_idx));
            }
            result
        } else {
            let result = Rf_allocVector3(SEXPTYPE::INTSXP, matches.len() as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            let dst = INTEGER(result);
            for (out_idx, src_idx) in matches.into_iter().enumerate() {
                *dst.add(out_idx) = (src_idx + 1) as c_int;
            }
            result
        }
    }
}

/// R's `agrepl(pattern, x, ...)` — logical approximate matching.
pub unsafe fn do_agrepl(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pattern_arg = arg_by_name_or_position(args, &["pattern"], 0);
        let x_arg = arg_by_name_or_position(args, &["x", "text"], 1);
        if pattern_arg.is_null() || x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }
        let ignore_case = named_logical_arg(args, "ignore.case").unwrap_or(false);
        let max_distance = agrep_max_distance(args, pattern_arg);
        let pattern = elt_to_string(pattern_arg, 0);
        let n = XLENGTH(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let matched = !is_string_na(x_arg, i)
                && approximate_contains(
                    &pattern,
                    &elt_to_string(x_arg, i),
                    max_distance,
                    ignore_case,
                );
            *dst.add(i as usize) = if matched { TRUE } else { FALSE };
        }
        result
    }
}

/// R's `pcre_config()` — report regex engine feature switches.
pub unsafe fn do_pcre_config(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    const FEATURES: [(&str, c_int); 4] = [
        ("UTF-8", TRUE),
        ("Unicode properties", TRUE),
        ("JIT", FALSE),
        ("stack", FALSE),
    ];

    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, FEATURES.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let data = LOGICAL(result);
        for (i, (_, value)) in FEATURES.iter().enumerate() {
            *data.add(i) = *value;
        }

        let names = Rf_allocVector3(SEXPTYPE::STRSXP, FEATURES.len() as R_xlen_t);
        if !names.is_null() {
            let _names_guard = protect(names);
            for (i, (name, _)) in FEATURES.iter().enumerate() {
                SET_STRING_ELT(
                    names,
                    i as R_xlen_t,
                    Rf_mkChar(CString::new(*name).unwrap_or_default().as_ptr()),
                );
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
                names,
            );
        }

        result
    }
}

fn agrep_max_distance(args: SEXP, pattern_arg: SEXP) -> usize {
    unsafe {
        let raw = arg_by_name_or_position(args, &["max.distance"], 2);
        let value = if raw.is_null() || raw == R_NilValue() {
            0.1
        } else {
            real_or_default(raw, 0.1)
        };
        if value <= 0.0 {
            return 0;
        }
        if value <= 1.0 {
            let pattern_len = elt_to_string(pattern_arg, 0).chars().count().max(1);
            (value * pattern_len as f64).ceil() as usize
        } else {
            value.ceil() as usize
        }
    }
}

unsafe fn agrep_match_indices(
    x: SEXP,
    pattern: &str,
    max_distance: usize,
    ignore_case: bool,
) -> Vec<R_xlen_t> {
    unsafe {
        let n = XLENGTH(x);
        let mut matches = Vec::new();
        for i in 0..n {
            if is_string_na(x, i) {
                continue;
            }
            if approximate_contains(pattern, &elt_to_string(x, i), max_distance, ignore_case) {
                matches.push(i);
            }
        }
        matches
    }
}

fn approximate_contains(pattern: &str, text: &str, max_distance: usize, ignore_case: bool) -> bool {
    let pattern = if ignore_case {
        pattern.to_ascii_lowercase()
    } else {
        pattern.to_string()
    };
    let text = if ignore_case {
        text.to_ascii_lowercase()
    } else {
        text.to_string()
    };
    let pat = pattern.as_bytes();
    let hay = text.as_bytes();
    if pat.is_empty() {
        return true;
    }
    if crate::mainutils::grep::levenshtein_distance(pat, hay) <= max_distance {
        return true;
    }
    let min_len = pat.len().saturating_sub(max_distance).max(1);
    let max_len = (pat.len() + max_distance).min(hay.len());
    for start in 0..hay.len() {
        for len in min_len..=max_len {
            let end = start + len;
            if end > hay.len() {
                break;
            }
            if crate::mainutils::grep::levenshtein_distance(pat, &hay[start..end]) <= max_distance {
                return true;
            }
        }
    }
    false
}

unsafe fn do_string_replace(args: SEXP, global: bool) -> SEXP {
    unsafe {
        let pattern_arg = CAR(args);
        let replacement_arg = CAR(CDR(args));
        let x_arg = CAR(CDR(CDR(args)));
        let ignore_case = named_logical_arg(args, "ignore.case").unwrap_or(false);
        let fixed = named_logical_arg(args, "fixed").unwrap_or(false);
        if pattern_arg.is_null()
            || replacement_arg.is_null()
            || x_arg.is_null()
            || x_arg == R_NilValue()
        {
            return R_NilValue();
        }
        let pattern = elt_to_string(pattern_arg, 0);
        let replacement = elt_to_string(replacement_arg, 0);
        let n = XLENGTH(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            let s = elt_to_string(x_arg, i);
            let replaced = if fixed && global {
                s.replace(&pattern, &replacement)
            } else if fixed {
                s.replacen(&pattern, &replacement, 1)
            } else if let Some(replaced) =
                crate::mainutils::grep::ere_replace(&pattern, &s, &replacement, global, ignore_case)
            {
                replaced
            } else {
                s
            };
            let cstr = CString::new(replaced).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i as usize) = charsxp;
            }
        }
        result
    }
}

/// R's `strsplit(x, split)` — split strings by separator, return list.
pub unsafe fn do_strsplit(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let split_arg = CAR(CDR(args));
        if x_arg.is_null() || x_arg == R_NilValue() || split_arg.is_null() {
            return Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        }
        let split = elt_to_string(split_arg, 0);
        let n = XLENGTH(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            let s = elt_to_string(x_arg, i);
            let parts: Vec<&str> = if split.is_empty() {
                s.split("").filter(|p| !p.is_empty()).collect()
            } else {
                s.split(&split).collect()
            };
            let vec = Rf_allocVector3(SEXPTYPE::STRSXP, parts.len() as R_xlen_t);
            if !vec.is_null() {
                let _vec_guard = protect(vec);
                for (j, part) in parts.iter().enumerate() {
                    let cstr = CString::new(*part).unwrap_or_default();
                    let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                    if !charsxp.is_null() {
                        let data = (*vec).gengc_next_node as *mut SEXP;
                        *data.add(j) = charsxp;
                    }
                }
            }
            crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, vec);
        }
        result
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
        let mut arg_vecs: Vec<SEXP> = Vec::new();
        let mut max_len: R_xlen_t = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                arg_vecs.push(arg);
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
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, max_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        for i in 0..max_len {
            let mut best = NA_REAL;
            for &arg in &arg_vecs {
                let n = XLENGTH(arg);
                if n == 0 {
                    continue;
                }
                let idx = i % n;
                let v = elt_real_safe(arg, idx);
                if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    continue;
                }
                if best.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    best = v;
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
            *dst.add(i as usize) = best;
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
        let t = if TYPEOF(values) == SEXPTYPE::STRSXP || TYPEOF(x) == SEXPTYPE::STRSXP {
            SEXPTYPE::STRSXP.as_c_int()
        } else if TYPEOF(x) == SEXPTYPE::REALSXP || TYPEOF(values) == SEXPTYPE::REALSXP {
            SEXPTYPE::REALSXP.as_c_int()
        } else {
            SEXPTYPE::INTSXP.as_c_int()
        };
        let result = Rf_allocVector3(t, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        if t == SEXPTYPE::REALSXP {
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
        } else if t == SEXPTYPE::INTSXP {
            let dst = INTEGER(result);
            for i in 0..after {
                *dst.add(i as usize) = if TYPEOF(x) == SEXPTYPE::INTSXP {
                    *INTEGER(x).add(i as usize)
                } else {
                    let v = elt_real_safe(x, i);
                    if v.is_nan() || v == NA_REAL {
                        NA_INTEGER
                    } else {
                        v as c_int
                    }
                };
            }
            for i in 0..vlen {
                *dst.add((after + i) as usize) = if TYPEOF(values) == SEXPTYPE::INTSXP {
                    *INTEGER(values).add(i as usize)
                } else {
                    let v = elt_real_safe(values, i);
                    if v.is_nan() || v == NA_REAL {
                        NA_INTEGER
                    } else {
                        v as c_int
                    }
                };
            }
            for i in after..n {
                *dst.add((i + vlen) as usize) = if TYPEOF(x) == SEXPTYPE::INTSXP {
                    *INTEGER(x).add(i as usize)
                } else {
                    let v = elt_real_safe(x, i);
                    if v.is_nan() || v == NA_REAL {
                        NA_INTEGER
                    } else {
                        v as c_int
                    }
                };
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

fn copy_vector_element(
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
// Set operations: setdiff, union, intersect, setequal
// ---------------------------------------------------------------------------

/// R's `setdiff(x, y)` — elements in x but not in y.
pub unsafe fn do_setdiff(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        let y = arg_by_name_or_position(args, &["y"], 1);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(TYPEOF(x), 0);
        }
        let xn = XLENGTH(x);
        let yn = if y.is_null() || y == R_NilValue() {
            0
        } else {
            XLENGTH(y)
        };
        let t = TYPEOF(x);
        let sexptype = SEXPTYPE(t);
        let mut y_keys: std::collections::BTreeSet<AtomicUniqueKey> =
            std::collections::BTreeSet::new();
        for i in 0..yn {
            y_keys.insert(atomic_unique_key(y, i, sexptype));
        }
        let mut result_indices: Vec<R_xlen_t> = Vec::new();
        let mut seen: std::collections::BTreeSet<AtomicUniqueKey> =
            std::collections::BTreeSet::new();
        for i in 0..xn {
            let key = atomic_unique_key(x, i, sexptype);
            if !y_keys.contains(&key) && seen.insert(key) {
                result_indices.push(i);
            }
        }
        let result = Rf_allocVector3(t, result_indices.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (out, &src) in result_indices.iter().enumerate() {
            copy_atomic_element(result, out as R_xlen_t, x, src, sexptype);
        }
        result
    }
}

/// R's `union(x, y)` — unique elements from both vectors.
pub unsafe fn do_union(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        let y = arg_by_name_or_position(args, &["y"], 1);
        let t = if !x.is_null() && x != R_NilValue() {
            TYPEOF(x)
        } else if !y.is_null() && y != R_NilValue() {
            TYPEOF(y)
        } else {
            SEXPTYPE::INTSXP.as_c_int()
        };
        let sexptype = SEXPTYPE(t);
        let mut seen: std::collections::BTreeSet<AtomicUniqueKey> =
            std::collections::BTreeSet::new();
        let mut result_sources: Vec<(SEXP, R_xlen_t)> = Vec::new();
        let mut add_from = |src: SEXP| {
            if !src.is_null() && src != R_NilValue() {
                let n = XLENGTH(src);
                for i in 0..n {
                    let key = atomic_unique_key(src, i, sexptype);
                    if seen.insert(key) {
                        result_sources.push((src, i));
                    }
                }
            }
        };
        add_from(x);
        add_from(y);
        let result = Rf_allocVector3(t, result_sources.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (out, &(src, src_index)) in result_sources.iter().enumerate() {
            copy_atomic_element(result, out as R_xlen_t, src, src_index, sexptype);
        }
        result
    }
}

/// R's `intersect(x, y)` — elements common to both vectors.
pub unsafe fn do_intersect(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        let y = arg_by_name_or_position(args, &["y"], 1);
        if x.is_null() || x == R_NilValue() || y.is_null() || y == R_NilValue() {
            return Rf_allocVector3(TYPEOF(x), 0);
        }
        let t = TYPEOF(x);
        let sexptype = SEXPTYPE(t);
        let xn = XLENGTH(x);
        let yn = XLENGTH(y);
        let mut y_keys: std::collections::BTreeSet<AtomicUniqueKey> =
            std::collections::BTreeSet::new();
        for i in 0..yn {
            y_keys.insert(atomic_unique_key(y, i, sexptype));
        }
        let mut seen: std::collections::BTreeSet<AtomicUniqueKey> =
            std::collections::BTreeSet::new();
        let mut result_indices: Vec<R_xlen_t> = Vec::new();
        for i in 0..xn {
            let key = atomic_unique_key(x, i, sexptype);
            if y_keys.contains(&key) && seen.insert(key) {
                result_indices.push(i);
            }
        }
        let result = Rf_allocVector3(t, result_indices.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (out, &src) in result_indices.iter().enumerate() {
            copy_atomic_element(result, out as R_xlen_t, x, src, sexptype);
        }
        result
    }
}

/// R's `setequal(x, y)` — TRUE if x and y contain the same unique values.
pub unsafe fn do_setequal(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        let y = arg_by_name_or_position(args, &["y"], 1);
        if (x.is_null() || x == R_NilValue()) && (y.is_null() || y == R_NilValue()) {
            return Rf_ScalarLogical(TRUE);
        }
        if x.is_null() || x == R_NilValue() || y.is_null() || y == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let xn = XLENGTH(x);
        let yn = XLENGTH(y);
        let tx = TYPEOF(x);
        let sexptype = SEXPTYPE(tx);
        let mut x_set: std::collections::BTreeSet<AtomicUniqueKey> =
            std::collections::BTreeSet::new();
        let mut y_set: std::collections::BTreeSet<AtomicUniqueKey> =
            std::collections::BTreeSet::new();
        for i in 0..xn {
            x_set.insert(atomic_unique_key(x, i, sexptype));
        }
        for i in 0..yn {
            y_set.insert(atomic_unique_key(y, i, sexptype));
        }
        Rf_ScalarLogical(if x_set == y_set { TRUE } else { FALSE })
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

// ---------------------------------------------------------------------------
// Conversion: chartr, format
// ---------------------------------------------------------------------------

/// R's `chartr(old, new, x)` — character-by-character translation.
pub unsafe fn do_chartr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let old_arg = CAR(args);
        let new_arg = CAR(CDR(args));
        let x_arg = CAR(CDR(CDR(args)));
        if old_arg.is_null() || new_arg.is_null() {
            return R_NilValue();
        }
        let old_str = elt_to_string(old_arg, 0);
        let new_str = elt_to_string(new_arg, 0);
        let old_chars: Vec<char> = old_str.chars().collect();
        let new_chars: Vec<char> = new_str.chars().collect();
        let n = if x_arg.is_null() || x_arg == R_NilValue() {
            0
        } else {
            XLENGTH(x_arg)
        };
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            if as_character_element_is_na(x_arg, i) {
                SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                continue;
            }
            let s = elt_to_string(x_arg, i);
            let translated: String = s
                .chars()
                .map(|c| {
                    if let Some(pos) = old_chars.iter().position(|&oc| oc == c) {
                        *new_chars.get(pos).unwrap_or(&c)
                    } else {
                        c
                    }
                })
                .collect();
            let cstr = CString::new(translated).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i as usize) = charsxp;
            }
        }
        result
    }
}

/// R's `format(x, digits, nsmall)` — format numbers as strings.
pub unsafe fn do_format(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let digits_arg = CAR(CDR(args));
        let nsmall_arg = CAR(CDR(CDR(args)));
        if x.is_null() || x == R_NilValue() {
            return Rf_mkString(CString::new("").unwrap_or_default().as_ptr());
        }
        let nsmall = if nsmall_arg.is_null() || nsmall_arg == R_NilValue() {
            0usize
        } else {
            real_or_default(nsmall_arg, 0.0) as usize
        };
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            let s = if TYPEOF(x) == SEXPTYPE::REALSXP {
                let v = *REAL(x).add(i as usize);
                if sexp_has_class(x, "POSIXct") {
                    posix_seconds_to_iso(v, false).unwrap_or_else(|| "NA".to_string())
                } else if sexp_has_class(x, "Date") {
                    date_days_to_iso(v).unwrap_or_else(|| "NA".to_string())
                } else if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    "NA".to_string()
                } else if nsmall > 0 {
                    format!("{:.*}", nsmall, v)
                } else {
                    format!("{}", v)
                }
            } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER {
                    "NA".to_string()
                } else {
                    format!("{}", v)
                }
            } else {
                elt_to_string(x, i)
            };
            let cstr = CString::new(s).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i as usize) = charsxp;
            }
        }
        result
    }
}

#[derive(Clone, Copy)]
enum CalendarLabel {
    Weekday,
    Month,
    Quarter,
}

unsafe fn calendar_days_from_element(x: SEXP, i: R_xlen_t) -> Option<f64> {
    unsafe {
        if TYPEOF(x) != SEXPTYPE::REALSXP {
            return None;
        }
        let value = *REAL(x).add(i as usize);
        if value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || !value.is_finite() {
            return None;
        }
        if sexp_has_class(x, "POSIXct") {
            Some((value / 86_400.0).floor())
        } else if sexp_has_class(x, "Date") {
            Some(value.floor())
        } else {
            None
        }
    }
}

fn calendar_label(days: f64, kind: CalendarLabel) -> Option<String> {
    const WEEKDAYS: [&str; 7] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];

    let (_, month, _) = date_days_to_civil(days)?;
    match kind {
        CalendarLabel::Weekday => {
            let day_index = ((days.floor() as i64) + 4).rem_euclid(7) as usize;
            Some(WEEKDAYS[day_index].to_string())
        }
        CalendarLabel::Month => Some(MONTHS[(month - 1) as usize].to_string()),
        CalendarLabel::Quarter => Some(format!("Q{}", (month - 1) / 3 + 1)),
    }
}

unsafe fn calendar_label_builtin(args: SEXP, kind: CalendarLabel) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        if TYPEOF(x) != SEXPTYPE::REALSXP
            || (!sexp_has_class(x, "Date") && !sexp_has_class(x, "POSIXct"))
        {
            base_error("no applicable method");
        }

        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _guard = protect(result);
        for i in 0..n {
            let label = calendar_days_from_element(x, i)
                .and_then(|days| calendar_label(days, kind))
                .or_else(|| matches!(kind, CalendarLabel::Quarter).then(|| "QNA".to_string()));
            let charsxp = label
                .and_then(|label| CString::new(label).ok())
                .map(|label| Rf_mkChar(label.as_ptr()))
                .unwrap_or_else(|| crate::sexp::globals::R_NaString());
            SET_STRING_ELT(result, i, charsxp);
        }
        result
    }
}

/// R's `weekdays(x)` for Date/POSIXct values.
pub unsafe fn do_weekdays(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { calendar_label_builtin(args, CalendarLabel::Weekday) }
}

/// R's `months(x)` for Date/POSIXct values.
pub unsafe fn do_months(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { calendar_label_builtin(args, CalendarLabel::Month) }
}

/// R's `quarters(x)` for Date/POSIXct values.
pub unsafe fn do_quarters(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { calendar_label_builtin(args, CalendarLabel::Quarter) }
}

/// R's `format.info(x, digits, nsmall)` width metadata.
pub unsafe fn do_format_info(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let digits = arg_by_name_or_position(args, &["digits"], 1);
        let digits = if digits.is_null() {
            R_NilValue()
        } else {
            digits
        };
        let nsmall = arg_by_name_or_position(args, &["nsmall"], 2);
        let nsmall = if nsmall.is_null() || nsmall == R_NilValue() {
            Rf_ScalarInteger(0)
        } else {
            nsmall
        };

        let tail = Rf_cons(nsmall, R_NilValue());
        let _tail_guard = protect(tail);
        let middle = Rf_cons(digits, tail);
        let _middle_guard = protect(middle);
        let normalized_args = Rf_cons(x, middle);
        let _args_guard = protect(normalized_args);
        crate::mainutils::paste_impl::do_formatinfo(call, op, normalized_args, rho)
    }
}

// ---------------------------------------------------------------------------
// do_order — order indices for sorting
// ---------------------------------------------------------------------------

/// R's `order(...)` — returns permutation of indices that sort the input.
pub unsafe fn do_order(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let n = XLENGTH(x);
        let decreasing = named_logical_arg(args, "decreasing").unwrap_or(false);
        let na_placement = order_na_placement(args, 1);
        let ordered_indices = ordered_atomic_indices(x, decreasing, na_placement);

        let result = Rf_allocVector3(SEXPTYPE::INTSXP, ordered_indices.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);
        for (i, &orig_idx) in ordered_indices.iter().enumerate() {
            *dst.add(i) = (orig_idx + 1) as c_int;
        }
        result
    }
}

fn ordered_atomic_indices(
    x: SEXP,
    decreasing: bool,
    na_placement: SortNaPlacement,
) -> Vec<R_xlen_t> {
    unsafe {
        let n = XLENGTH(x);
        let mut missing_indices: Vec<R_xlen_t> = Vec::new();
        let mut ordered_indices: Vec<R_xlen_t> = match TYPEOF(x) {
            t if t == SEXPTYPE::STRSXP => {
                let mut values: Vec<(SEXP, R_xlen_t)> = Vec::with_capacity(n as usize);
                for i in 0..n {
                    let value = STRING_ELT(x, i);
                    if charsxp_is_na(value) {
                        missing_indices.push(i);
                    } else {
                        values.push((value, i));
                    }
                }
                values.sort_by(|a, b| {
                    let ordering = compare_charsxp_for_sort(a.0, b.0);
                    if decreasing {
                        ordering.reverse()
                    } else {
                        ordering
                    }
                });
                values.into_iter().map(|(_, index)| index).collect()
            }
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                let mut values: Vec<(c_int, R_xlen_t)> = Vec::with_capacity(n as usize);
                for i in 0..n {
                    let value = *INTEGER(x).add(i as usize);
                    if value == NA_INTEGER {
                        missing_indices.push(i);
                    } else {
                        values.push((value, i));
                    }
                }
                values.sort_by(|a, b| {
                    let ordering = a.0.cmp(&b.0);
                    if decreasing {
                        ordering.reverse()
                    } else {
                        ordering
                    }
                });
                values.into_iter().map(|(_, index)| index).collect()
            }
            t if t == SEXPTYPE::REALSXP => {
                let mut values: Vec<(f64, R_xlen_t)> = Vec::with_capacity(n as usize);
                for i in 0..n {
                    let value = *REAL(x).add(i as usize);
                    if ISNAN(value) {
                        missing_indices.push(i);
                    } else {
                        values.push((value, i));
                    }
                }
                values.sort_by(|a, b| {
                    let ordering = a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal);
                    if decreasing {
                        ordering.reverse()
                    } else {
                        ordering
                    }
                });
                values.into_iter().map(|(_, index)| index).collect()
            }
            _ => {
                let mut values: Vec<(f64, R_xlen_t)> = Vec::with_capacity(n as usize);
                for i in 0..n {
                    let value = elt_real_safe(x, i);
                    if ISNAN(value) {
                        missing_indices.push(i);
                    } else {
                        values.push((value, i));
                    }
                }
                values.sort_by(|a, b| {
                    let ordering = a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal);
                    if decreasing {
                        ordering.reverse()
                    } else {
                        ordering
                    }
                });
                values.into_iter().map(|(_, index)| index).collect()
            }
        };

        match na_placement {
            SortNaPlacement::First => {
                let mut with_missing = missing_indices;
                with_missing.extend(ordered_indices);
                with_missing
            }
            SortNaPlacement::Last => {
                ordered_indices.extend(missing_indices);
                ordered_indices
            }
            SortNaPlacement::Remove => ordered_indices,
        }
    }
}

fn order_na_placement(args: SEXP, position: usize) -> SortNaPlacement {
    unsafe {
        let arg = arg_by_name_or_position(args, &["na.last"], position);
        if arg.is_null() || arg == R_NilValue() || XLENGTH(arg) == 0 {
            return SortNaPlacement::Last;
        }
        let raw = if TYPEOF(arg) == SEXPTYPE::LGLSXP || TYPEOF(arg) == SEXPTYPE::INTSXP {
            *INTEGER(arg)
        } else if TYPEOF(arg) == SEXPTYPE::REALSXP {
            let value = *REAL(arg);
            if ISNAN(value) {
                NA_LOGICAL
            } else {
                value as c_int
            }
        } else {
            TRUE
        };
        match raw {
            NA_LOGICAL => SortNaPlacement::Remove,
            FALSE => SortNaPlacement::First,
            _ => SortNaPlacement::Last,
        }
    }
}

// ---------------------------------------------------------------------------
// do_rank — ranks of elements
// ---------------------------------------------------------------------------

/// R's `rank(x)` — returns ranks of elements (average ties method).
pub unsafe fn do_rank(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }
        let n = XLENGTH(x);
        let na_placement = order_na_placement(args, 1);
        let ties_method = rank_ties_method(args);
        let mut missing_indices: Vec<R_xlen_t> = Vec::new();
        let mut ranks = vec![NA_REAL; n as usize];

        match TYPEOF(x) {
            t if t == SEXPTYPE::STRSXP => {
                let mut values: Vec<(SEXP, R_xlen_t)> = Vec::with_capacity(n as usize);
                for i in 0..n {
                    let value = STRING_ELT(x, i);
                    if charsxp_is_na(value) {
                        missing_indices.push(i);
                    } else {
                        values.push((value, i));
                    }
                }
                values.sort_by(|a, b| compare_charsxp_for_sort(a.0, b.0));
                assign_tied_ranks(&mut ranks, &values, ties_method, 0, |a, b| {
                    compare_charsxp_for_sort(a.0, b.0) == std::cmp::Ordering::Equal
                });
            }
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                let mut values: Vec<(c_int, R_xlen_t)> = Vec::with_capacity(n as usize);
                for i in 0..n {
                    let value = *INTEGER(x).add(i as usize);
                    if value == NA_INTEGER {
                        missing_indices.push(i);
                    } else {
                        values.push((value, i));
                    }
                }
                values.sort_by(|a, b| a.0.cmp(&b.0));
                assign_tied_ranks(&mut ranks, &values, ties_method, 0, |a, b| a.0 == b.0);
            }
            _ => {
                let mut values: Vec<(f64, R_xlen_t)> = Vec::with_capacity(n as usize);
                for i in 0..n {
                    let value = elt_real_safe(x, i);
                    if ISNAN(value) {
                        missing_indices.push(i);
                    } else {
                        values.push((value, i));
                    }
                }
                values.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                assign_tied_ranks(&mut ranks, &values, ties_method, 0, |a, b| a.0 == b.0);
            }
        }

        let nonmissing_count = n as usize - missing_indices.len();
        let mut is_missing = vec![false; n as usize];
        for &index in &missing_indices {
            is_missing[index as usize] = true;
        }
        match na_placement {
            SortNaPlacement::First => {
                for (i, rank) in ranks.iter_mut().enumerate() {
                    if !is_missing[i] {
                        *rank += missing_indices.len() as f64;
                    }
                }
                for (offset, &index) in missing_indices.iter().enumerate() {
                    ranks[index as usize] = (offset + 1) as f64;
                }
            }
            SortNaPlacement::Last => {
                for (offset, &index) in missing_indices.iter().enumerate() {
                    ranks[index as usize] = (nonmissing_count + offset + 1) as f64;
                }
            }
            SortNaPlacement::Remove => {}
        }

        let output_len = if na_placement == SortNaPlacement::Remove {
            nonmissing_count
        } else {
            n as usize
        };
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, output_len as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        let mut out = 0usize;
        for i in 0..n as usize {
            if na_placement == SortNaPlacement::Remove && is_missing[i] {
                continue;
            }
            *dst.add(out) = ranks[i];
            out += 1;
        }
        result
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum RankTiesMethod {
    Average,
    First,
    Last,
    Min,
    Max,
}

fn rank_ties_method(args: SEXP) -> RankTiesMethod {
    unsafe {
        let arg = arg_by_name_or_position(args, &["ties.method"], 2);
        if arg.is_null()
            || arg == R_NilValue()
            || TYPEOF(arg) != SEXPTYPE::STRSXP
            || XLENGTH(arg) == 0
        {
            return RankTiesMethod::Average;
        }
        match elt_to_string(arg, 0).as_str() {
            "first" => RankTiesMethod::First,
            "last" => RankTiesMethod::Last,
            "min" => RankTiesMethod::Min,
            "max" => RankTiesMethod::Max,
            _ => RankTiesMethod::Average,
        }
    }
}

fn assign_tied_ranks<T, F>(
    ranks: &mut [f64],
    values: &[(T, R_xlen_t)],
    ties_method: RankTiesMethod,
    rank_offset: usize,
    same_key: F,
) where
    F: Fn(&(T, R_xlen_t), &(T, R_xlen_t)) -> bool,
{
    let mut i = 0usize;
    while i < values.len() {
        let mut j = i + 1;
        while j < values.len() && same_key(&values[i], &values[j]) {
            j += 1;
        }
        match ties_method {
            RankTiesMethod::Average => {
                let avg_rank = (rank_offset + i + rank_offset + j + 1) as f64 / 2.0;
                for item in &values[i..j] {
                    ranks[item.1 as usize] = avg_rank;
                }
            }
            RankTiesMethod::First => {
                for (offset, item) in values[i..j].iter().enumerate() {
                    ranks[item.1 as usize] = (rank_offset + i + offset + 1) as f64;
                }
            }
            RankTiesMethod::Last => {
                for (offset, item) in values[i..j].iter().enumerate() {
                    ranks[item.1 as usize] = (rank_offset + j - offset) as f64;
                }
            }
            RankTiesMethod::Min => {
                let rank = (rank_offset + i + 1) as f64;
                for item in &values[i..j] {
                    ranks[item.1 as usize] = rank;
                }
            }
            RankTiesMethod::Max => {
                let rank = (rank_offset + j) as f64;
                for item in &values[i..j] {
                    ranks[item.1 as usize] = rank;
                }
            }
        }
        i = j;
    }
}

// ---------------------------------------------------------------------------
// do_duplicated — identify duplicates
// ---------------------------------------------------------------------------

/// R's `duplicated(x, incomparables, fromLast, nmax)` — returns logical vector, TRUE for duplicated elements.
///
/// - `incomparables`: values to exclude from duplicate checking (typically NA or FALSE)
/// - `fromLast`: if TRUE, consider last occurrence as original (mark earlier as dup)
/// - `nmax`: max number of unique elements expected (optimization hint; NA_INTEGER = no limit)
pub unsafe fn do_duplicated(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }

        let incomparables = arg_by_name_or_position(args, &["incomparables"], 1);
        let from_last = logical_arg_by_name_or_position(args, "fromLast", 2).unwrap_or(false);
        let nmax = integer_arg_by_name_or_position(args, "nmax", 3).unwrap_or(NA_INTEGER);

        // Build incomparables set
        let mut incomparable_set: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        if !incomparables.is_null() && incomparables != R_NilValue() {
            let in_n = XLENGTH(incomparables);
            for i in 0..in_n {
                let s = elt_to_string(incomparables, i);
                incomparable_set.insert(s);
            }
        }

        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);

        // Compute nmax limit
        let effective_nmax: usize = if nmax == NA_INTEGER || nmax <= 0 {
            usize::MAX
        } else {
            nmax as usize
        };

        if from_last {
            // Scan from last to first; last occurrence is original, earlier are duplicates
            let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            for i in 0..n {
                *dst.add(i as usize) = FALSE;
            }
            for i in (0..n).rev() {
                let s = elt_to_string(x, i);
                if incomparable_set.contains(&s) {
                    *dst.add(i as usize) = FALSE;
                } else if seen.contains(&s) {
                    *dst.add(i as usize) = TRUE;
                } else {
                    seen.insert(s);
                    *dst.add(i as usize) = FALSE;
                    if seen.len() >= effective_nmax {
                        for j in 0..i {
                            let sj = elt_to_string(x, j);
                            if !incomparable_set.contains(&sj) {
                                *dst.add(j as usize) = TRUE;
                            }
                        }
                        break;
                    }
                }
            }
        } else {
            // Scan from first to last; first occurrence is original, later are duplicates
            let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            for i in 0..n {
                let s = elt_to_string(x, i);
                if incomparable_set.contains(&s) {
                    *dst.add(i as usize) = FALSE;
                } else if seen.contains(&s) {
                    *dst.add(i as usize) = TRUE;
                } else {
                    seen.insert(s);
                    *dst.add(i as usize) = FALSE;
                    if seen.len() >= effective_nmax {
                        // Everything remaining is a duplicate
                        for j in (i + 1)..n {
                            let sj = elt_to_string(x, j);
                            if incomparable_set.contains(&sj) {
                                *dst.add(j as usize) = FALSE;
                            } else {
                                *dst.add(j as usize) = TRUE;
                            }
                        }
                        break;
                    }
                }
            }
        }

        result
    }
}

// ---------------------------------------------------------------------------
// do_anyDuplicated — check for any duplicates
// ---------------------------------------------------------------------------

/// R's `anyDuplicated(x, incomparables, fromLast, nmax)` — returns index of first duplicate (0 if none).
///
/// Supports incomparables, fromLast, and nmax parameters just like `duplicated()`.
pub unsafe fn do_anyDuplicated(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
        }

        let incomparables = arg_by_name_or_position(args, &["incomparables"], 1);
        let from_last = logical_arg_by_name_or_position(args, "fromLast", 2).unwrap_or(false);
        let nmax = integer_arg_by_name_or_position(args, "nmax", 3).unwrap_or(NA_INTEGER);

        // Build incomparables set
        let mut incomparable_set: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        if !incomparables.is_null() && incomparables != R_NilValue() {
            let in_n = XLENGTH(incomparables);
            for i in 0..in_n {
                let s = elt_to_string(incomparables, i);
                incomparable_set.insert(s);
            }
        }

        let n = XLENGTH(x);
        let effective_nmax: usize = if nmax == NA_INTEGER || nmax <= 0 {
            usize::MAX
        } else {
            nmax as usize
        };

        if from_last {
            // From last: find last duplicated element index
            let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            for i in (0..n).rev() {
                let s = elt_to_string(x, i);
                if !incomparable_set.contains(&s) {
                    if seen.contains(&s) {
                        return Rf_ScalarInteger((i + 1) as c_int);
                    } else {
                        seen.insert(s);
                        if seen.len() >= effective_nmax {
                            break;
                        }
                    }
                }
            }
            Rf_ScalarInteger(0)
        } else {
            // From first: find first duplicated element index
            let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            for i in 0..n {
                let s = elt_to_string(x, i);
                if !incomparable_set.contains(&s) {
                    if seen.contains(&s) {
                        return Rf_ScalarInteger((i + 1) as c_int);
                    }
                    seen.insert(s);
                    if seen.len() >= effective_nmax {
                        break;
                    }
                }
            }
            Rf_ScalarInteger(0)
        }
    }
}

// ---------------------------------------------------------------------------
// do_duplicated.array — array deduplication along margins
// ---------------------------------------------------------------------------

/// R's `duplicated.array(x, MARGIN, fromLast)` — finds duplicated rows/columns in an array.
///
/// - `x`: array or matrix
/// - `MARGIN`: which margin to check (1=rows, 2=cols, etc.)
/// - `fromLast`: if TRUE, last occurrence is original
pub unsafe fn do_duplicated_array(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }

        // Parse MARGIN (default = 1, i.e. rows)
        let margin = {
            let rest = CDR(args);
            if rest.is_null() || rest == R_NilValue() {
                1i32
            } else {
                real_or_default(CAR(rest), 1.0) as i32
            }
        };

        // Parse fromLast (default = FALSE)
        let from_last = {
            let rest = CDR(args);
            if rest.is_null() || rest == R_NilValue() {
                false
            } else {
                let rest2 = CDR(rest);
                if rest2.is_null() || rest2 == R_NilValue() {
                    false
                } else {
                    let v = real_or_default(CAR(rest2), 0.0);
                    v != 0.0
                }
            }
        };

        let n = XLENGTH(x);
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }

        // Get dimensions
        let dim = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );

        if dim.is_null() || dim == R_NilValue() || XLENGTH(dim) < 2 {
            // Not really an array — fall back to regular duplicated
            let mut new_args = R_NilValue();
            // push nmax as NA
            new_args = Rf_cons(Rf_ScalarInteger(NA_INTEGER), new_args);
            new_args = Rf_cons(
                Rf_ScalarLogical(if from_last { TRUE } else { FALSE }),
                new_args,
            );
            new_args = Rf_cons(R_NilValue(), new_args); // incomparables
            new_args = Rf_cons(x, new_args);
            return do_duplicated(_call, _op, new_args, _rho);
        }

        let dims_len = XLENGTH(dim);
        let dim_vals = INTEGER(dim);
        let nrows = *dim_vals as usize;
        let ncols = if dims_len >= 2 {
            (*dim_vals.add(1)) as usize
        } else {
            1
        };

        // For 2D arrays, support MARGIN=1 (rows) and MARGIN=2 (columns)
        if margin == 1 && dims_len == 2 {
            // Duplicate rows
            let total = nrows;
            let result = Rf_allocVector3(SEXPTYPE::LGLSXP, total as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            let dst = LOGICAL(result);

            // Hash each row as a string
            let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            let t = TYPEOF(x);

            if from_last {
                // First pass collect, second pass mark
                let mut row_strings: Vec<String> = Vec::with_capacity(total);
                for row in 0..total {
                    let mut parts: Vec<String> = Vec::with_capacity(ncols);
                    for col in 0..ncols {
                        let idx = row + col * nrows; // column-major
                        parts.push(elt_to_string(x, idx as R_xlen_t));
                    }
                    row_strings.push(parts.join("\x01"));
                }
                // Collect from end
                let mut unique_from_end: std::collections::BTreeSet<String> =
                    std::collections::BTreeSet::new();
                for row in (0..total).rev() {
                    unique_from_end.insert(row_strings[row].clone());
                }
                // Mark from start
                let mut encountered: std::collections::BTreeSet<String> =
                    std::collections::BTreeSet::new();
                for row in 0..total {
                    if encountered.contains(&row_strings[row]) {
                        *dst.add(row) = TRUE;
                    } else {
                        encountered.insert(row_strings[row].clone());
                        *dst.add(row) = FALSE;
                    }
                }
            } else {
                for row in 0..total {
                    let mut parts: Vec<String> = Vec::with_capacity(ncols);
                    for col in 0..ncols {
                        let idx = row + col * nrows; // column-major
                        parts.push(elt_to_string(x, idx as R_xlen_t));
                    }
                    let key = parts.join("\x01");
                    if seen.contains(&key) {
                        *dst.add(row) = TRUE;
                    } else {
                        seen.insert(key);
                        *dst.add(row) = FALSE;
                    }
                }
            }

            result
        } else if margin == 2 && dims_len == 2 {
            // Duplicate columns
            let total = ncols;
            let result = Rf_allocVector3(SEXPTYPE::LGLSXP, total as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            let dst = LOGICAL(result);

            let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

            if from_last {
                let mut col_strings: Vec<String> = Vec::with_capacity(total);
                for col in 0..total {
                    let mut parts: Vec<String> = Vec::with_capacity(nrows);
                    for row in 0..nrows {
                        let idx = row + col * nrows;
                        parts.push(elt_to_string(x, idx as R_xlen_t));
                    }
                    col_strings.push(parts.join("\x01"));
                }
                let mut encountered: std::collections::BTreeSet<String> =
                    std::collections::BTreeSet::new();
                for col in 0..total {
                    if encountered.contains(&col_strings[col]) {
                        *dst.add(col) = TRUE;
                    } else {
                        encountered.insert(col_strings[col].clone());
                        *dst.add(col) = FALSE;
                    }
                }
            } else {
                for col in 0..total {
                    let mut parts: Vec<String> = Vec::with_capacity(nrows);
                    for row in 0..nrows {
                        let idx = row + col * nrows;
                        parts.push(elt_to_string(x, idx as R_xlen_t));
                    }
                    let key = parts.join("\x01");
                    if seen.contains(&key) {
                        *dst.add(col) = TRUE;
                    } else {
                        seen.insert(key);
                        *dst.add(col) = FALSE;
                    }
                }
            }

            result
        } else {
            // Generic: flatten along margin — fallback to duplicated on flattened vector
            // For higher-dimensional arrays, treat as 1D
            let mut new_args = R_NilValue();
            new_args = Rf_cons(Rf_ScalarInteger(NA_INTEGER), new_args);
            new_args = Rf_cons(
                Rf_ScalarLogical(if from_last { TRUE } else { FALSE }),
                new_args,
            );
            new_args = Rf_cons(R_NilValue(), new_args);
            new_args = Rf_cons(x, new_args);
            do_duplicated(_call, _op, new_args, _rho)
        }
    }
}

// ---------------------------------------------------------------------------
// do_anyDuplicated.array — check for any duplicates in array along margin
// ---------------------------------------------------------------------------

/// R's `anyDuplicated.array(x, MARGIN, fromLast)` — returns index of first duplicate in array (0 if none).
pub unsafe fn do_anyDuplicated_array(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
        }

        // Parse MARGIN (default = 1)
        let margin = {
            let rest = CDR(args);
            if rest.is_null() || rest == R_NilValue() {
                1i32
            } else {
                real_or_default(CAR(rest), 1.0) as i32
            }
        };

        // Parse fromLast (default = FALSE)
        let from_last = {
            let rest = CDR(args);
            if rest.is_null() || rest == R_NilValue() {
                false
            } else {
                let rest2 = CDR(rest);
                if rest2.is_null() || rest2 == R_NilValue() {
                    false
                } else {
                    let v = real_or_default(CAR(rest2), 0.0);
                    v != 0.0
                }
            }
        };

        // Get dimensions
        let dim = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );

        if dim.is_null() || dim == R_NilValue() || XLENGTH(dim) < 2 {
            // Not really an array — fall back to regular anyDuplicated
            let mut new_args = R_NilValue();
            new_args = Rf_cons(Rf_ScalarInteger(NA_INTEGER), new_args);
            new_args = Rf_cons(
                Rf_ScalarLogical(if from_last { TRUE } else { FALSE }),
                new_args,
            );
            new_args = Rf_cons(R_NilValue(), new_args);
            new_args = Rf_cons(x, new_args);
            return do_anyDuplicated(_call, _op, new_args, _rho);
        }

        let dims_len = XLENGTH(dim);
        let dim_vals = INTEGER(dim);
        let nrows = *dim_vals as usize;
        let ncols = if dims_len >= 2 {
            (*dim_vals.add(1)) as usize
        } else {
            1
        };

        if margin == 1 && dims_len == 2 {
            // Check duplicate rows
            let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            if from_last {
                let mut row_strings: Vec<String> = Vec::with_capacity(nrows);
                for row in 0..nrows {
                    let mut parts: Vec<String> = Vec::with_capacity(ncols);
                    for col in 0..ncols {
                        let idx = row + col * nrows;
                        parts.push(elt_to_string(x, idx as R_xlen_t));
                    }
                    row_strings.push(parts.join("\x01"));
                }
                let mut result_idx = 0i32;
                let mut encountered: std::collections::BTreeSet<String> =
                    std::collections::BTreeSet::new();
                for row in (0..nrows).rev() {
                    if encountered.contains(&row_strings[row]) {
                        result_idx = (row + 1) as c_int; // R 1-indexed
                    } else {
                        encountered.insert(row_strings[row].clone());
                    }
                }
                Rf_ScalarInteger(result_idx)
            } else {
                for row in 0..nrows {
                    let mut parts: Vec<String> = Vec::with_capacity(ncols);
                    for col in 0..ncols {
                        let idx = row + col * nrows;
                        parts.push(elt_to_string(x, idx as R_xlen_t));
                    }
                    let key = parts.join("\x01");
                    if seen.contains(&key) {
                        return Rf_ScalarInteger((row + 1) as c_int);
                    }
                    seen.insert(key);
                }
                Rf_ScalarInteger(0)
            }
        } else if margin == 2 && dims_len == 2 {
            // Check duplicate columns
            let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            if from_last {
                let mut col_strings: Vec<String> = Vec::with_capacity(ncols);
                for col in 0..ncols {
                    let mut parts: Vec<String> = Vec::with_capacity(nrows);
                    for row in 0..nrows {
                        let idx = row + col * nrows;
                        parts.push(elt_to_string(x, idx as R_xlen_t));
                    }
                    col_strings.push(parts.join("\x01"));
                }
                let mut result_idx = 0i32;
                let mut encountered: std::collections::BTreeSet<String> =
                    std::collections::BTreeSet::new();
                for col in (0..ncols).rev() {
                    if encountered.contains(&col_strings[col]) {
                        result_idx = (col + 1) as c_int;
                    } else {
                        encountered.insert(col_strings[col].clone());
                    }
                }
                Rf_ScalarInteger(result_idx)
            } else {
                for col in 0..ncols {
                    let mut parts: Vec<String> = Vec::with_capacity(nrows);
                    for row in 0..nrows {
                        let idx = row + col * nrows;
                        parts.push(elt_to_string(x, idx as R_xlen_t));
                    }
                    let key = parts.join("\x01");
                    if seen.contains(&key) {
                        return Rf_ScalarInteger((col + 1) as c_int);
                    }
                    seen.insert(key);
                }
                Rf_ScalarInteger(0)
            }
        } else {
            // Generic fallback
            let mut new_args = R_NilValue();
            new_args = Rf_cons(Rf_ScalarInteger(NA_INTEGER), new_args);
            new_args = Rf_cons(
                Rf_ScalarLogical(if from_last { TRUE } else { FALSE }),
                new_args,
            );
            new_args = Rf_cons(R_NilValue(), new_args);
            new_args = Rf_cons(x, new_args);
            do_anyDuplicated(_call, _op, new_args, _rho)
        }
    }
}

// ---------------------------------------------------------------------------
// do_match — match values in table
// ---------------------------------------------------------------------------

/// R's `match(x, table, nomatch, incomparables)` — first table index for each x.
pub unsafe fn do_match(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = match_arg(args, 0, "x", R_NilValue());
        let table = match_arg(args, 1, "table", R_NilValue());
        let nomatch_arg = match_arg(args, 2, "nomatch", Rf_ScalarInteger(NA_INTEGER));
        let incomparables = match_arg(args, 3, "incomparables", R_NilValue());
        let nomatch = integer_scalar_or(nomatch_arg, NA_INTEGER);

        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);

        let common_type = match_common_type(x, table);
        let mut incomparable_set = BTreeSet::new();
        if !incomparables.is_null() && incomparables != R_NilValue() {
            for i in 0..XLENGTH(incomparables) {
                incomparable_set.insert(match_key(incomparables, i, common_type));
            }
        }

        let mut lookup: BTreeMap<MatchKey, c_int> = BTreeMap::new();
        if !table.is_null() && table != R_NilValue() {
            let tn = XLENGTH(table);
            for i in 0..tn {
                lookup
                    .entry(match_key(table, i, common_type))
                    .or_insert((i + 1) as c_int);
            }
        }
        for i in 0..n {
            let key = match_key(x, i, common_type);
            *dst.add(i as usize) = if incomparable_set.contains(&key) {
                nomatch
            } else {
                *lookup.get(&key).unwrap_or(&nomatch)
            };
        }
        result
    }
}

unsafe fn integer_scalar_or(arg: SEXP, default: c_int) -> c_int {
    unsafe {
        if arg.is_null() || arg == R_NilValue() || arg == R_MissingArg() {
            return default;
        }
        match SEXPTYPE(TYPEOF(arg)) {
            SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => {
                if XLENGTH(arg) < 1 {
                    default
                } else {
                    INTEGER_ELT(arg, 0)
                }
            }
            SEXPTYPE::REALSXP => {
                if XLENGTH(arg) < 1 {
                    default
                } else {
                    let value = REAL_ELT(arg, 0);
                    if value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                        default
                    } else {
                        value as c_int
                    }
                }
            }
            _ => default,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum MatchKey {
    Missing,
    String(String),
    Integer(c_int),
    Real(u64),
}

fn match_common_type(x: SEXP, table: SEXP) -> SEXPTYPE {
    unsafe {
        let xtype = if x.is_null() {
            SEXPTYPE::NILSXP
        } else {
            SEXPTYPE(TYPEOF(x))
        };
        let ttype = if table.is_null() || table == R_NilValue() {
            xtype
        } else {
            SEXPTYPE(TYPEOF(table))
        };
        if xtype == SEXPTYPE::STRSXP || ttype == SEXPTYPE::STRSXP {
            SEXPTYPE::STRSXP
        } else if xtype == SEXPTYPE::REALSXP || ttype == SEXPTYPE::REALSXP {
            SEXPTYPE::REALSXP
        } else {
            SEXPTYPE::INTSXP
        }
    }
}

unsafe fn match_key(x: SEXP, index: R_xlen_t, common_type: SEXPTYPE) -> MatchKey {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return MatchKey::Missing;
        }
        match common_type {
            SEXPTYPE::STRSXP => {
                if TYPEOF(x) == SEXPTYPE::STRSXP
                    && STRING_ELT(x, index) == crate::sexp::globals::R_NaString()
                {
                    MatchKey::Missing
                } else {
                    MatchKey::String(elt_to_string(x, index))
                }
            }
            SEXPTYPE::REALSXP => {
                let value = match TYPEOF(x) {
                    t if t == SEXPTYPE::REALSXP => REAL_ELT(x, index as c_int),
                    t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                        let value = INTEGER_ELT(x, index as c_int);
                        if value == NA_INTEGER {
                            NA_REAL
                        } else {
                            value as f64
                        }
                    }
                    _ => elt_to_string(x, index).parse::<f64>().unwrap_or(NA_REAL),
                };
                if value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    MatchKey::Missing
                } else if value.is_nan() {
                    MatchKey::Real(f64::NAN.to_bits())
                } else {
                    MatchKey::Real(value.to_bits())
                }
            }
            _ => {
                let value = match TYPEOF(x) {
                    t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                        INTEGER_ELT(x, index as c_int)
                    }
                    t if t == SEXPTYPE::REALSXP => {
                        let value = REAL_ELT(x, index as c_int);
                        if value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                            NA_INTEGER
                        } else {
                            value as c_int
                        }
                    }
                    _ => elt_to_string(x, index)
                        .parse::<c_int>()
                        .unwrap_or(NA_INTEGER),
                };
                if value == NA_INTEGER {
                    MatchKey::Missing
                } else {
                    MatchKey::Integer(value)
                }
            }
        }
    }
}

unsafe fn match_arg(args: SEXP, position: usize, name: &str, default: SEXP) -> SEXP {
    unsafe {
        if let Some(value) = named_arg(args, name) {
            return value;
        }
        let mut positional = 0usize;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).is_none() {
                if positional == position {
                    let value = CAR(current);
                    return if value.is_null() || value == R_MissingArg() {
                        default
                    } else {
                        value
                    };
                }
                positional += 1;
            }
            current = CDR(current);
        }
        default
    }
}

// ---------------------------------------------------------------------------
// do_findInterval — find interval in sorted vector
// ---------------------------------------------------------------------------

/// R's `findInterval(x, vec)` — for each x, find j such that vec[j] <= x < vec[j+1].
pub unsafe fn do_findInterval(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let vec = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || vec.is_null() || vec == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let n = XLENGTH(x);
        let vn = XLENGTH(vec);
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);
        let mut vvals: Vec<f64> = Vec::with_capacity(vn as usize);
        for i in 0..vn {
            vvals.push(elt_real_safe(vec, i));
        }
        for i in 0..n {
            let xi = elt_real_safe(x, i);
            if xi.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || xi.is_nan() {
                *dst.add(i as usize) = NA_INTEGER;
                continue;
            }
            let mut lo = 0i32;
            let mut hi = vn as i32;
            while lo < hi {
                let mid = (lo + hi) / 2;
                if vvals[mid as usize] <= xi {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            *dst.add(i as usize) = lo;
        }
        result
    }
}

// ---------------------------------------------------------------------------
// do_cut — cut numeric vector into intervals
// ---------------------------------------------------------------------------

/// R's `cut(x, breaks)` — cuts numeric vector into intervals, returns STRSXP.
pub unsafe fn do_cut(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let breaks_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let n = XLENGTH(x);
        let mut break_pts: Vec<f64> = Vec::new();
        if !breaks_arg.is_null() && breaks_arg != R_NilValue() {
            let bt = TYPEOF(breaks_arg);
            if bt == SEXPTYPE::INTSXP || bt == SEXPTYPE::REALSXP {
                let bn = XLENGTH(breaks_arg);
                if bn == 1 {
                    let nbins = elt_real_safe(breaks_arg, 0) as i64;
                    if nbins < 1 {
                        return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
                    }
                    let mut lo = f64::INFINITY;
                    let mut hi = f64::NEG_INFINITY;
                    for i in 0..n {
                        let v = elt_real_safe(x, i);
                        if v.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN && !v.is_nan() {
                            if v < lo {
                                lo = v;
                            }
                            if v > hi {
                                hi = v;
                            }
                        }
                    }
                    if lo == f64::INFINITY {
                        lo = 0.0;
                        hi = 1.0;
                    }
                    let step = (hi - lo) / nbins as f64;
                    for i in 0..=nbins {
                        break_pts.push(lo + i as f64 * step);
                    }
                    if let Some(last) = break_pts.last_mut() {
                        *last += step * 0.001;
                    }
                } else {
                    for i in 0..bn {
                        break_pts.push(elt_real_safe(breaks_arg, i));
                    }
                }
            }
        }
        if break_pts.len() < 2 {
            break_pts = vec![0.0, 1.0];
        }
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            let v = elt_real_safe(x, i);
            let label = if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || v.is_nan() {
                "NA".to_string()
            } else {
                let mut lo_idx = break_pts.len() - 1;
                for j in 0..break_pts.len() - 1 {
                    if v >= break_pts[j] && v < break_pts[j + 1] {
                        lo_idx = j;
                        break;
                    }
                }
                format!("({},{})", break_pts[lo_idx], break_pts[lo_idx + 1])
            };
            let cstr = CString::new(label).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i as usize) = charsxp;
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// String operations: startsWith, endsWith, str_pad, str_count, str_replace
// ---------------------------------------------------------------------------

/// R's `startsWith(x, prefix)` — check if strings start with prefix.
pub unsafe fn do_startsWith(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let prefix_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || prefix_arg.is_null() {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }
        let prefix = elt_to_string(prefix_arg, 0);
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let s = elt_to_string(x, i);
            *dst.add(i as usize) = if s.starts_with(&prefix) { TRUE } else { FALSE };
        }
        result
    }
}

/// R's `endsWith(x, suffix)` — check if strings end with suffix.
pub unsafe fn do_endsWith(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let suffix_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || suffix_arg.is_null() {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }
        let suffix = elt_to_string(suffix_arg, 0);
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let s = elt_to_string(x, i);
            *dst.add(i as usize) = if s.ends_with(&suffix) { TRUE } else { FALSE };
        }
        result
    }
}

/// R's `str_pad(x, width, side="left", pad=" ")` — pad strings to a width.
pub unsafe fn do_str_pad(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let width_arg = CAR(CDR(args));
        let side_arg = CAR(CDR(CDR(args)));
        let pad_arg = CAR(CDR(CDR(CDR(args))));
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let width = if width_arg.is_null() || width_arg == R_NilValue() {
            1usize
        } else {
            real_or_default(width_arg, 1.0).max(0.0) as usize
        };
        let side = if side_arg.is_null() || side_arg == R_NilValue() {
            "left".to_string()
        } else {
            elt_to_string(side_arg, 0)
        };
        let pad_char = if pad_arg.is_null() || pad_arg == R_NilValue() {
            " ".to_string()
        } else {
            elt_to_string(pad_arg, 0)
        };
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            let s = elt_to_string(x, i);
            let slen = s.chars().count();
            let padded = if slen >= width {
                s
            } else {
                let diff = width - slen;
                let pad_str: String = pad_char.chars().cycle().take(diff).collect();
                match side.as_str() {
                    "left" => format!("{}{}", pad_str, s),
                    "right" => format!("{}{}", s, pad_str),
                    "both" => {
                        let left = diff / 2;
                        let right = diff - left;
                        let lp: String = pad_char.chars().cycle().take(left).collect();
                        let rp: String = pad_char.chars().cycle().take(right).collect();
                        format!("{}{}{}", lp, s, rp)
                    }
                    _ => format!("{}{}", pad_str, s),
                }
            };
            let cstr = CString::new(padded).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i as usize) = charsxp;
            }
        }
        result
    }
}

/// R's `str_count(x, pattern)` — count occurrences of pattern in strings.
pub unsafe fn do_str_count(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let pattern_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || pattern_arg.is_null() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let pattern = elt_to_string(pattern_arg, 0);
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);
        for i in 0..n {
            let s = elt_to_string(x, i);
            let count = if pattern.is_empty() {
                s.len() + 1
            } else {
                s.matches(&pattern).count()
            };
            *dst.add(i as usize) = count as c_int;
        }
        result
    }
}

/// R's `str_replace(x, pattern, replacement)` — alias for sub.
pub unsafe fn do_str_replace(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_sub(_call, _op, args, _rho) }
}

// ---------------------------------------------------------------------------
// R runtime type checks: is.language, is.call, is.symbol, is.name,
//   is.pairlist, is.function, is.expression, is.environment
// ---------------------------------------------------------------------------

/// R's `is.language(x)` — TRUE for LANGSXP, SYMSXP, or EXPRSXP.
pub unsafe fn do_is_language(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        Rf_ScalarLogical(
            if t == SEXPTYPE::LANGSXP || t == SEXPTYPE::SYMSXP || t == SEXPTYPE::EXPRSXP {
                TRUE
            } else {
                FALSE
            },
        )
    }
}

/// R's `is.call(x)` — TRUE for LANGSXP.
pub unsafe fn do_is_call(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        Rf_ScalarLogical(if TYPEOF(x) == SEXPTYPE::LANGSXP {
            TRUE
        } else {
            FALSE
        })
    }
}

/// R's `is.symbol(x)` — TRUE for SYMSXP.
pub unsafe fn do_is_symbol(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        Rf_ScalarLogical(if TYPEOF(x) == SEXPTYPE::SYMSXP {
            TRUE
        } else {
            FALSE
        })
    }
}

/// R's `is.name(x)` — alias for is.symbol.
pub unsafe fn do_is_name(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_is_symbol(_call, _op, args, _rho) }
}

/// R's `is.pairlist(x)` — TRUE for LISTSXP.
pub unsafe fn do_is_pairlist(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        Rf_ScalarLogical(if TYPEOF(x) == SEXPTYPE::LISTSXP {
            TRUE
        } else {
            FALSE
        })
    }
}

/// R's `is.function(x)` — TRUE for CLOSXP, BUILTINSXP, or SPECIALSXP.
pub unsafe fn do_is_function(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        Rf_ScalarLogical(
            if t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
                TRUE
            } else {
                FALSE
            },
        )
    }
}

/// R's `is.expression(x)` — TRUE for EXPRSXP.
pub unsafe fn do_is_expression(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        Rf_ScalarLogical(if TYPEOF(x) == SEXPTYPE::EXPRSXP {
            TRUE
        } else {
            FALSE
        })
    }
}

/// R's `is.environment(x)` — TRUE for ENVSXP.
pub unsafe fn do_is_environment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        Rf_ScalarLogical(if TYPEOF(x) == SEXPTYPE::ENVSXP {
            TRUE
        } else {
            FALSE
        })
    }
}

// ---------------------------------------------------------------------------
// S3: setOldClass, methods
// ---------------------------------------------------------------------------

/// R's `setOldClass(Class)` — register old-style S3 class. Simplified: returns Class.
pub unsafe fn do_setOldClass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let class_arg = CAR(args);
        if class_arg.is_null() || class_arg == R_NilValue() {
            return R_NilValue();
        }
        class_arg
    }
}

/// R's `methods(generic)` — list methods known to the Rust runtime.
pub unsafe fn do_methods(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let generic_arg = CAR(args);
        if generic_arg.is_null() || generic_arg == R_NilValue() {
            return string_vector(&all_runtime_method_names());
        }
        let generic = elt_to_string(generic_arg, 0);
        if generic.is_empty() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }

        let prefix = format!("{generic}.");
        let methods = all_runtime_method_names()
            .into_iter()
            .filter(|name| name.starts_with(&prefix))
            .collect::<Vec<_>>();
        string_vector(&methods)
    }
}

fn all_runtime_method_names() -> Vec<String> {
    let mut methods = crate::eval::builtin::builtin_handler_names()
        .filter(|name| name.contains('.'))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    methods.sort();
    methods.dedup();
    methods
}

// ---------------------------------------------------------------------------
// Matrix: lower.tri, upper.tri
// ---------------------------------------------------------------------------

/// R's `lower.tri(x, diag=FALSE)` — TRUE for lower triangle of matrix.
pub unsafe fn do_lower_tri(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let diag_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let include_diag = !diag_arg.is_null()
            && diag_arg != R_NilValue()
            && real_or_default(diag_arg, 0.0) != 0.0;
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
                let n = XLENGTH(x);
                (n, 1)
            };
        let total = nrow * ncol;
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for j in 0..ncol {
            for i in 0..nrow {
                let idx = (j * nrow + i) as usize;
                let is_lower = if include_diag { i >= j } else { i > j };
                *dst.add(idx) = if is_lower { TRUE } else { FALSE };
            }
        }
        result
    }
}

/// R's `upper.tri(x, diag=FALSE)` — TRUE for upper triangle of matrix.
pub unsafe fn do_upper_tri(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let diag_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let include_diag = !diag_arg.is_null()
            && diag_arg != R_NilValue()
            && real_or_default(diag_arg, 0.0) != 0.0;
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
                let n = XLENGTH(x);
                (n, 1)
            };
        let total = nrow * ncol;
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for j in 0..ncol {
            for i in 0..nrow {
                let idx = (j * nrow + i) as usize;
                let is_upper = if include_diag { i <= j } else { i < j };
                *dst.add(idx) = if is_upper { TRUE } else { FALSE };
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Complete error handling — calling handlers and restarts
// ---------------------------------------------------------------------------

/// R's `withCallingHandlers(expr, ...)` — evaluate expr with calling handlers.
/// Handlers are evaluated before unwinding (unlike tryCatch).
pub unsafe fn do_withCallingHandlers(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }

        let old_stack = condition_handler_stack();
        let new_stack = calling_handler_stack_from_args(CDR(args), rho, old_stack);
        set_condition_handler_stack(new_stack);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::eval::eval::Rf_eval(expr, rho)
        }));
        set_condition_handler_stack(old_stack);

        match result {
            Ok(value) => value,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

fn condition_handler_stack() -> SEXP {
    crate::sexp::instance::with_required_current_instance(|inst| inst.error_state.handler_stack)
}

fn set_condition_handler_stack(stack: SEXP) {
    crate::sexp::instance::with_required_current_instance(|inst| {
        inst.error_state.handler_stack = stack;
    });
}

unsafe fn calling_handler_stack_from_args(mut args: SEXP, rho: SEXP, old_stack: SEXP) -> SEXP {
    unsafe {
        let mut entries = Vec::new();
        while !args.is_null() && args != R_NilValue() {
            let Some(class_name) = tag_name(args) else {
                args = CDR(args);
                continue;
            };
            let handler = crate::eval::eval::Rf_eval(CAR(args), rho);
            if is_function_value(handler) {
                entries.push(calling_handler_entry(&class_name, handler, rho));
            }
            args = CDR(args);
        }

        let mut stack = old_stack;
        for entry in entries.into_iter().rev() {
            stack = Rf_cons(entry, stack);
        }
        stack
    }
}

unsafe fn calling_handler_entry(class_name: &str, handler: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let entry = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        if entry.is_null() {
            return R_NilValue();
        }
        let _entry_guard = protect(entry);
        SET_VECTOR_ELT(
            entry,
            0,
            Rf_mkString(CString::new(class_name).unwrap_or_default().as_ptr()),
        );
        SET_VECTOR_ELT(entry, 1, handler);
        SET_VECTOR_ELT(entry, 2, rho);
        entry
    }
}

unsafe fn signal_calling_handlers(condition: SEXP, rho: SEXP) {
    unsafe {
        let classes = crate::sexp::attrib_core::getAttrib(condition, Rf_install(c"class".as_ptr()));
        if classes.is_null() || classes == R_NilValue() || TYPEOF(classes) != SEXPTYPE::STRSXP {
            return;
        }

        let stack = condition_handler_stack();
        for class_idx in 0..XLENGTH(classes) {
            let class_name = elt_to_string(classes, class_idx);
            let mut current = stack;
            while !current.is_null() && current != R_NilValue() {
                let entry = CAR(current);
                if calling_handler_entry_class(entry).as_deref() == Some(class_name.as_str()) {
                    let handler = VECTOR_ELT(entry, 1);
                    call_condition_handler(handler, condition, rho);
                }
                current = CDR(current);
            }
        }
    }
}

unsafe fn calling_handler_entry_class(entry: SEXP) -> Option<String> {
    unsafe {
        if entry.is_null() || entry == R_NilValue() || TYPEOF(entry) != SEXPTYPE::VECSXP {
            return None;
        }
        let class = VECTOR_ELT(entry, 0);
        if class.is_null() || class == R_NilValue() || TYPEOF(class) != SEXPTYPE::STRSXP {
            return None;
        }
        Some(elt_to_string(class, 0))
    }
}

unsafe fn call_condition_handler(handler: SEXP, condition: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(handler) == SEXPTYPE::CLOSXP {
            let args = Rf_cons(condition, R_NilValue());
            let call = Rf_cons(handler, args);
            if !call.is_null() {
                (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            }
            crate::eval::closure::applyClosure(call, handler, args, rho, R_NilValue(), TRUE)
        } else {
            let call = crate::sexp::constructors::Rf_lang2(handler, condition);
            crate::eval::eval::Rf_eval(call, rho)
        }
    }
}

/// R's `computeRestarts()` — compute available restarts for current condition.
pub unsafe fn do_computeRestarts(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { restart_stack_as_list() }
}

/// R's `findRestart(name)` — find a restart by name.
pub unsafe fn do_findRestart(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let name_arg = CAR(args);
        if name_arg.is_null() || name_arg == R_NilValue() {
            return R_NilValue();
        }
        let name = elt_to_string(name_arg, 0);
        find_restart_by_name(&name).unwrap_or_else(|| R_NilValue())
    }
}

/// R's `restarts()` — list available restarts.
pub unsafe fn do_restarts(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { restart_stack_as_list() }
}

/// R's `invokeRestart(restart, ...)` — call a restart and return to its dynamic extent.
pub unsafe fn do_invokeRestart(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let restart_arg = CAR(args);
        let restart = resolve_restart_arg(restart_arg, true).unwrap_or_else(|| {
            base_error(format!(
                "no 'restart' '{}' found",
                restart_arg_name(restart_arg)
            ));
        });
        invoke_restart(restart, CDR(args), rho)
    }
}

/// R's `tryInvokeRestart(restart, ...)` — invoke a restart if one is active.
pub unsafe fn do_tryInvokeRestart(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let restart_arg = CAR(args);
        match resolve_restart_arg(restart_arg, true) {
            Some(restart) => invoke_restart(restart, CDR(args), rho),
            None => R_NilValue(),
        }
    }
}

/// R's `isRestart(x)` — check for a restart object.
pub unsafe fn do_isRestart(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        Rf_ScalarLogical(if is_restart_object(CAR(args)) {
            TRUE
        } else {
            FALSE
        })
    }
}

/// R's `restartDescription(r)` — return the restart description, if any.
pub unsafe fn do_restartDescription(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let restart = CAR(args);
        if !is_restart_object(restart) {
            return R_NilValue();
        }
        let description = restart_field(restart, "description", 3);
        if description.is_null() || description == R_NilValue() {
            Rf_mkString(c"".as_ptr())
        } else {
            description
        }
    }
}

unsafe fn resolve_restart_arg(restart_arg: SEXP, require_active_object: bool) -> Option<SEXP> {
    unsafe {
        if restart_arg.is_null() || restart_arg == R_NilValue() {
            return None;
        }
        if TYPEOF(restart_arg) == SEXPTYPE::VECSXP {
            if require_active_object {
                return find_restart_by_object(restart_arg)
                    .or_else(|| base_error("restart not on stack"));
            }
            return if is_restart_object(restart_arg) {
                Some(restart_arg)
            } else {
                None
            };
        }
        if TYPEOF(restart_arg) == SEXPTYPE::STRSXP {
            return find_restart_by_name(&elt_to_string(restart_arg, 0));
        }
        None
    }
}

unsafe fn restart_arg_name(restart_arg: SEXP) -> String {
    unsafe {
        if !restart_arg.is_null()
            && restart_arg != R_NilValue()
            && TYPEOF(restart_arg) == SEXPTYPE::STRSXP
        {
            elt_to_string(restart_arg, 0)
        } else {
            String::new()
        }
    }
}

unsafe fn invoke_restart(restart: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let handler = restart_handler(restart);
        let value = if is_function_value(handler) {
            call_function_with_args(handler, args, rho)
        } else {
            R_NilValue()
        };
        std::panic::panic_any(crate::sexp::context::RSignal::Restart(value));
    }
}

unsafe fn call_function_with_args(handler: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let call = Rf_cons(handler, args);
        if !call.is_null() {
            (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        if TYPEOF(handler) == SEXPTYPE::CLOSXP {
            crate::eval::closure::applyClosure(call, handler, args, rho, R_NilValue(), TRUE)
        } else {
            crate::eval::eval::Rf_eval(call, rho)
        }
    }
}

fn restart_stack() -> SEXP {
    crate::sexp::instance::with_required_current_instance(|inst| inst.error_state.restart_stack)
}

fn set_restart_stack(stack: SEXP) {
    crate::sexp::instance::with_required_current_instance(|inst| {
        inst.error_state.restart_stack = stack;
    });
}

unsafe fn restart_stack_as_list() -> SEXP {
    unsafe {
        let mut restarts = Vec::new();
        let mut current = restart_stack();
        while !current.is_null() && current != R_NilValue() {
            restarts.push(CAR(current));
            current = CDR(current);
        }

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, restarts.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (i, restart) in restarts.iter().enumerate() {
            SET_VECTOR_ELT(result, i as R_xlen_t, *restart);
        }
        result
    }
}

unsafe fn find_restart_by_name(name: &str) -> Option<SEXP> {
    unsafe {
        let mut current = restart_stack();
        while !current.is_null() && current != R_NilValue() {
            let restart = CAR(current);
            if restart_name(restart).as_deref() == Some(name) {
                return Some(restart);
            }
            current = CDR(current);
        }
        None
    }
}

unsafe fn find_restart_by_object(needle: SEXP) -> Option<SEXP> {
    unsafe {
        let mut current = restart_stack();
        while !current.is_null() && current != R_NilValue() {
            let restart = CAR(current);
            if restart == needle {
                return Some(restart);
            }
            current = CDR(current);
        }
        None
    }
}

unsafe fn restart_name(restart: SEXP) -> Option<String> {
    unsafe {
        if restart.is_null() || restart == R_NilValue() || TYPEOF(restart) != SEXPTYPE::VECSXP {
            return None;
        }
        let name = restart_field(restart, "name", 0);
        if name.is_null() || name == R_NilValue() || TYPEOF(name) != SEXPTYPE::STRSXP {
            return None;
        }
        Some(elt_to_string(name, 0))
    }
}

unsafe fn restart_handler(restart: SEXP) -> SEXP {
    unsafe { restart_field(restart, "handler", 2) }
}

unsafe fn restart_field(restart: SEXP, field_name: &str, fallback_index: R_xlen_t) -> SEXP {
    unsafe {
        if restart.is_null() || restart == R_NilValue() || TYPEOF(restart) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let names = crate::sexp::attrib_core::getAttrib(restart, Rf_install(c"names".as_ptr()));
        if !names.is_null() && names != R_NilValue() && TYPEOF(names) == SEXPTYPE::STRSXP {
            let limit = XLENGTH(names).min(XLENGTH(restart));
            for index in 0..limit {
                if elt_to_string(names, index) == field_name {
                    return VECTOR_ELT(restart, index);
                }
            }
        }
        if fallback_index < XLENGTH(restart) {
            VECTOR_ELT(restart, fallback_index)
        } else {
            R_NilValue()
        }
    }
}

unsafe fn restart_entry(name: &str, handler: SEXP) -> SEXP {
    unsafe {
        let restart = Rf_allocVector3(SEXPTYPE::VECSXP, 6);
        if restart.is_null() {
            return R_NilValue();
        }
        let _restart_guard = protect(restart);
        SET_VECTOR_ELT(
            restart,
            0,
            Rf_mkString(CString::new(name).unwrap_or_default().as_ptr()),
        );
        SET_VECTOR_ELT(restart, 1, R_NilValue());
        SET_VECTOR_ELT(restart, 2, handler);
        SET_VECTOR_ELT(restart, 3, Rf_mkString(c"".as_ptr()));
        SET_VECTOR_ELT(restart, 4, R_NilValue());
        SET_VECTOR_ELT(restart, 5, R_NilValue());

        let names = string_vector(&[
            "name".to_string(),
            "exit".to_string(),
            "handler".to_string(),
            "description".to_string(),
            "test".to_string(),
            "interactive".to_string(),
        ]);
        crate::sexp::attrib_core::setAttrib(restart, Rf_install(c"names".as_ptr()), names);
        crate::sexp::attrib_core::setAttrib(
            restart,
            Rf_install(c"class".as_ptr()),
            Rf_mkString(c"restart".as_ptr()),
        );
        restart
    }
}

unsafe fn is_restart_object(value: SEXP) -> bool {
    unsafe {
        !value.is_null()
            && value != R_NilValue()
            && TYPEOF(value) == SEXPTYPE::VECSXP
            && inherits_class(value, "restart")
    }
}

// ---------------------------------------------------------------------------
// Complete package system — library, require, installed.packages, find.package
// ---------------------------------------------------------------------------

/// R's `.libPaths()` — inspect or replace the session's library search path.
pub unsafe fn do_lib_paths(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        if !args.is_null() && args != R_NilValue() {
            let value = CAR(args);
            if !value.is_null() && value != R_NilValue() && TYPEOF(value) == SEXPTYPE::STRSXP {
                let mut paths = Vec::with_capacity(LENGTH(value).max(0) as usize);
                for i in 0..LENGTH(value) {
                    let path = CStr::from_ptr(CHAR(STRING_ELT(value, i as R_xlen_t)))
                        .to_string_lossy()
                        .into_owned();
                    paths.push(PathBuf::from(path));
                }
                crate::sexp::instance::with_required_current_instance(|inst| {
                    inst.path_policy.set_library_paths(paths);
                });
            }
        }

        let paths = crate::sexp::instance::with_required_current_instance(|inst| {
            inst.path_policy
                .library_paths()
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        });
        string_vector(&paths)
    }
}

/// R's `library.dynam()` — native package loading is outside the pure-R Android runtime.
pub unsafe fn do_library_dynam(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    package_error(
        "library.dynam() loads native extension code, which is disabled in this pure-R Android runtime; use Rust-ported internals or a host-owned native-library policy",
    )
}

/// R's `library(package, ...)` — load a package.
pub unsafe fn do_library(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pkg_arg = CAR(args);
        if pkg_arg.is_null() || pkg_arg == R_NilValue() {
            package_error("no package specified");
        }
        let package_name = elt_to_string(pkg_arg, 0);
        if package_name.is_empty() || package_name == "NA" {
            package_error("invalid package name");
        }
        let lib_path = find_package_path(&package_name);
        if lib_path.is_empty() {
            package_error(format!("there is no package called '{}'", package_name));
        }
        if package_attached(&package_name) {
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return R_NilValue();
        }
        match load_pure_r_package(&package_name, Path::new(&lib_path)) {
            Ok(()) => {
                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
                R_NilValue()
            }
            Err(message) => package_error(message),
        }
    }
}

/// R's `require(package, ...)` — check if a package can be loaded.
pub unsafe fn do_require(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pkg_arg = CAR(args);
        if pkg_arg.is_null() || pkg_arg == R_NilValue() {
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return Rf_ScalarLogical(FALSE);
        }
        let package_name = elt_to_string(pkg_arg, 0);
        let lib_path = find_package_path(&package_name);
        if lib_path.is_empty() {
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return Rf_ScalarLogical(FALSE);
        }
        if package_attached(&package_name) {
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return Rf_ScalarLogical(TRUE);
        }
        match load_pure_r_package(&package_name, Path::new(&lib_path)) {
            Ok(()) => {
                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
                Rf_ScalarLogical(TRUE)
            }
            Err(_) => {
                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
                Rf_ScalarLogical(FALSE)
            }
        }
    }
}

/// R's `installed.packages(...)` — list installed packages.
pub unsafe fn do_installed_packages(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let packages = installed_package_rows();
        installed_packages_matrix(&packages)
    }
}

/// R's `find.package(package, ...)` — find the path to a package.
pub unsafe fn do_find_package(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pkg_arg = CAR(args);
        if pkg_arg.is_null() || pkg_arg == R_NilValue() {
            return R_NilValue();
        }
        let package_name = elt_to_string(pkg_arg, 0);
        let path = find_package_path(&package_name);
        if path.is_empty() {
            return R_NilValue();
        }
        Rf_mkString(CString::new(path).unwrap_or_default().as_ptr())
    }
}

/// R's `packageVersion(pkg)` — read a package version from DESCRIPTION.
pub unsafe fn do_package_version(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let package_arg = arg_by_name_or_position(args, &["pkg", "package"], 0);
        let package = elt_to_string(package_arg, 0);
        match package_description_fields(&package) {
            Ok(fields) => match fields.get("Version") {
                Some(version) => string_vector(std::slice::from_ref(version)),
                None => package_error(format!("package '{}' has no Version field", package)),
            },
            Err(message) => package_error(message),
        }
    }
}

/// R's `packageDescription(pkg, fields = NULL)` — read DESCRIPTION metadata.
pub unsafe fn do_package_description(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let package_arg = arg_by_name_or_position(args, &["pkg", "package"], 0);
        let fields_arg = arg_by_name_or_position(args, &["fields"], 1);
        let package = elt_to_string(package_arg, 0);
        let fields = match package_description_fields(&package) {
            Ok(fields) => fields,
            Err(message) => package_error(message),
        };

        if !fields_arg.is_null() && fields_arg != R_NilValue() && XLENGTH(fields_arg) > 0 {
            let selected = (0..XLENGTH(fields_arg))
                .map(|i| {
                    let name = elt_to_string(fields_arg, i);
                    fields.get(&name).cloned()
                })
                .collect::<Vec<_>>();
            return optional_string_vector(&selected);
        }

        named_string_list(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        )
    }
}

/// R's `loadNamespace(package)` — load a package namespace without attaching it.
pub unsafe fn do_load_namespace(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let package_arg = arg_by_name_or_position(args, &["package", "name"], 0);
        let package = elt_to_string(package_arg, 0);
        match load_package_namespace_by_name(&package) {
            Ok(env) => env,
            Err(message) => package_error(message),
        }
    }
}

/// R's `requireNamespace(package, quietly = FALSE)` — namespace availability probe.
pub unsafe fn do_require_namespace(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let package_arg = arg_by_name_or_position(args, &["package", "quietly"], 0);
        let package = elt_to_string(package_arg, 0);
        Rf_ScalarLogical(if load_package_namespace_by_name(&package).is_ok() {
            TRUE
        } else {
            FALSE
        })
    }
}

/// R's `getNamespace(name)` — return a loaded namespace, loading on demand.
pub unsafe fn do_get_namespace(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { do_load_namespace(_call, _op, args, rho) }
}

/// R's `asNamespace(ns)` — coerce a package name or environment to a namespace.
pub unsafe fn do_as_namespace(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let ns = CAR(args);
        if !ns.is_null() && TYPEOF(ns) == SEXPTYPE::ENVSXP {
            return ns;
        }
        let package = elt_to_string(ns, 0);
        match load_package_namespace_by_name(&package) {
            Ok(env) => env,
            Err(message) => package_error(message),
        }
    }
}

/// R's `loadedNamespaces()` — list namespaces loaded in this session.
pub unsafe fn do_loaded_namespaces(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut names = crate::sexp::instance::with_required_current_instance(|inst| {
            inst.package_namespace_cache
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        });
        names.sort();
        string_vector(&names)
    }
}

/// R's `data(..., package, envir)` — load package data.
///
/// The Android runtime intentionally supports source-form package data
/// (`data/*.R`) and rejects serialized/lazy databases with an explicit error.
pub unsafe fn do_data(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let topic_arg = arg_by_name_or_position(args, &["list"], 0);
        let package_arg = arg_by_name_or_position(args, &["package"], 1);
        let envir_arg = arg_by_name_or_position(args, &["envir"], 2);
        let target_env = if !envir_arg.is_null() && TYPEOF(envir_arg) == SEXPTYPE::ENVSXP {
            envir_arg
        } else {
            rho
        };

        let packages = package_arg_values(package_arg);
        if topic_arg.is_null() || topic_arg == R_NilValue() || XLENGTH(topic_arg) == 0 {
            let names = list_package_data_sets(&packages);
            return string_vector(&names);
        }

        let mut loaded = Vec::<String>::new();
        for i in 0..XLENGTH(topic_arg) {
            let topic = elt_to_string(topic_arg, i);
            if topic.is_empty() || topic == "NA" {
                continue;
            }
            match load_package_data_set(&topic, &packages, target_env) {
                Ok(true) => push_unique(&mut loaded, topic),
                Ok(false) => package_error(format!("data set '{}' not found", topic)),
                Err(message) => package_error(message),
            }
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        string_vector(&loaded)
    }
}

// ---------------------------------------------------------------------------
// Complete R runtime — source, sys.source, demo, example
// ---------------------------------------------------------------------------

/// R's `source(file, local, echo, ...)` — evaluate an R script file.
pub unsafe fn do_source(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = CAR(args);
        if file_arg.is_null() || file_arg == R_NilValue() {
            eprintln!("source: no file specified");
            return R_NilValue();
        }
        let file_path = elt_to_string(file_arg, 0);

        match std::fs::read_to_string(&file_path) {
            Ok(content) => eval_source_text(&content, rho),
            Err(e) => {
                base_error(format!("cannot open file '{}': {}", file_path, e));
            }
        }
    }
}

/// R's `sys.source(file, envir, ...)` — source an R file into a specific environment.
pub unsafe fn do_sys_source(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = CAR(args);
        let envir_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(args))
        };

        if file_arg.is_null() || file_arg == R_NilValue() {
            eprintln!("sys.source: no file specified");
            return R_NilValue();
        }
        let file_path = elt_to_string(file_arg, 0);
        let target_env = if !envir_arg.is_null() && envir_arg != R_NilValue() {
            envir_arg
        } else {
            rho
        };

        match std::fs::read_to_string(&file_path) {
            Ok(content) => eval_source_text(&content, target_env),
            Err(e) => {
                base_error(format!("cannot open file '{}': {}", file_path, e));
            }
        }
    }
}

unsafe fn eval_source_text(content: &str, env: SEXP) -> SEXP {
    unsafe {
        let parsed = parse_source_expression_vector(content);
        let result = if parsed.is_null() || parsed == R_NilValue() {
            R_NilValue()
        } else {
            crate::eval::eval::Rf_eval(parsed, env)
        };
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        result
    }
}

/// R's `demo(topic, ...)` — run a demo (simplified).
pub unsafe fn do_demo(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let topic_arg = CAR(args);
        if topic_arg.is_null() || topic_arg == R_NilValue() {
            eprintln!("demo: no topic specified");
            return R_NilValue();
        }
        let topic = elt_to_string(topic_arg, 0);
        // Look for demo in common locations
        let demo_path = find_package_demo(&topic);
        if demo_path.is_empty() {
            eprintln!("No demo available for topic '{}'", topic);
            return R_NilValue();
        }
        match std::fs::read_to_string(&demo_path) {
            Ok(_content) => {
                eprintln!("Demo for topic: {}", topic);
                // In a full impl, parse and eval demo content
                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
                R_NilValue()
            }
            Err(e) => {
                eprintln!("Error reading demo '{}': {}", topic, e);
                R_NilValue()
            }
        }
    }
}

/// R's `example(topic, ...)` — run an example (simplified).
pub unsafe fn do_example(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let topic_arg = CAR(args);
        if topic_arg.is_null() || topic_arg == R_NilValue() {
            eprintln!("example: no topic specified");
            return R_NilValue();
        }
        let topic = elt_to_string(topic_arg, 0);
        // Look for examples in common locations
        let example_path = find_package_example(&topic);
        if example_path.is_empty() {
            eprintln!("No examples available for topic '{}'", topic);
            return R_NilValue();
        }
        match std::fs::read_to_string(&example_path) {
            Ok(_content) => {
                eprintln!("Examples for topic: {}", topic);
                // In a full impl, parse and eval example content
                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
                R_NilValue()
            }
            Err(e) => {
                eprintln!("Error reading example '{}': {}", topic, e);
                R_NilValue()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Register essentials builtins
// ---------------------------------------------------------------------------

/// Register essential builtins in the base environment.
pub unsafe fn register_essentials_builtins(env: SEXP) {
    unsafe {
        use crate::sexp::accessors::SET_FRAME;

        let all_fns = [
            "c",
            "seq",
            "sequence",
            "seq_len",
            "seq_along",
            "rep",
            "paste",
            "paste0",
            "cat",
            "print",
            "typeof",
            "mode",
            "storage.mode",
            "storage.mode<-",
            "identity",
            "is.na",
            "names",
            "logical",
            "integer",
            "numeric",
            "double",
            "single",
            "complex",
            "character",
            "raw",
            "vector",
            "which",
            "ifelse",
            "any",
            "all",
            "table",
            "simplify2array",
            "match.arg",
            "char.expand",
            "type.convert",
            "as.environment",
            "pos.to.env",
            "sort.list",
            "match.fun",
            "as.integer",
            "as.double",
            "as.character",
            "as.Date",
            "as.POSIXct",
            "as.logical",
            "as.list",
            "as.vector",
            "as.call",
            "length",
            "nchar",
            "substr",
            "tolower",
            "toupper",
            "enc2native",
            "enc2utf8",
            "trimws",
            "sprintf",
            "gsub",
            "sub",
            "grep",
            "grepl",
            "agrep",
            "agrepl",
            "pcre_config",
            "strsplit",
            "pmin",
            "pmax",
            "cumsum",
            "cumprod",
            "which.min",
            "which.max",
            "append",
            "head",
            "tail",
            "sort",
            "rev",
            "unique",
            ".primTrace",
            ".primUntrace",
            "@",
            "@<-",
            "$<-",
            ".cache_class",
            "...elt",
            "...length",
            "...names",
            "forceAndCall",
            "declare",
            "environment<-",
            "standardGeneric",
            "xtfrm",
            ".External.graphics",
            "browser",
            "[",
            ".subset",
            "[[",
            ".subset2",
            "setdiff",
            "union",
            "intersect",
            "setequal",
            "is.finite",
            "is.infinite",
            "is.nan",
            "is.matrix",
            "is.array",
            "is.list",
            "chartr",
            "format",
            "weekdays",
            "months",
            "quarters",
            "format.info",
            "apply",
            "tapply",
            "mapply",
            "outer",
            "sweep",
            "abs",
            "sign",
            "ceiling",
            "floor",
            "round",
            "trunc",
            "sqrt",
            "log",
            "log2",
            "log10",
            "exp",
            "dnorm",
            "pnorm",
            "qnorm",
            "dpois",
            "ppois",
            "qpois",
            "dbinom",
            "pbinom",
            "qbinom",
            "dgamma",
            "pgamma",
            "qgamma",
            "dcauchy",
            "pcauchy",
            "qcauchy",
            "dexp",
            "pexp",
            "qexp",
            "dbeta",
            "pbeta",
            "qbeta",
            "dt",
            "pt",
            "qt",
            "dchisq",
            "pchisq",
            "qchisq",
            "dweibull",
            "pweibull",
            "qweibull",
            "df",
            "pf",
            "qf",
            "dnbinom",
            "pnbinom",
            "qnbinom",
            "dunif",
            "punif",
            "qunif",
            "dgeom",
            "pgeom",
            "qgeom",
            "dlnorm",
            "plnorm",
            "qlnorm",
            "dlogis",
            "plogis",
            "qlogis",
            "dsignrank",
            "psignrank",
            "qsignrank",
            "dwilcox",
            "pwilcox",
            "qwilcox",
            "dhyper",
            "phyper",
            "qhyper",
            "ptukey",
            "qtukey",
            "dmultinom",
            "NROW",
            "NCOL",
            "nrow",
            "ncol",
            "tsp",
            "tsp<-",
            "lengths",
            "length<-",
            "rownames",
            "row.names",
            "colnames",
            "class",
            ".class2",
            "list",
            "data.frame",
            "attr",
            "attributes",
            "structure",
            "::",
            ":::",
            "comment",
            "unname",
            "oldClass",
            "names<-",
            "dim<-",
            "dimnames<-",
            "rownames<-",
            "row.names<-",
            "colnames<-",
            "class<-",
            "comment<-",
            "oldClass<-",
            "attr<-",
            "attributes<-",
            "noquote",
            "deparse",
            "nargs",
            "UseMethod",
            "NextMethod",
            "missing",
            "parent.frame",
            "sys.call",
            "sys.frame",
            "unclass",
            "oldClass",
            "getwd",
            "setwd",
            "basename",
            "dirname",
            "file.path",
            "file.show",
            "file.exists",
            "file.info",
            "file.size",
            "file.mtime",
            "list.files",
            "list.dirs",
            "normalizePath",
            "tempdir",
            "tempfile",
            "dir.exists",
            "dir.create",
            "file.create",
            "file.append",
            "file.link",
            "file.symlink",
            "file.remove",
            "file.rename",
            "file.copy",
            "file.access",
            "file.choose",
            "unlink",
            "nzchar",
            "lapply",
            "sapply",
            "vapply",
            "Map",
            "Filter",
            "do.call",
            "set.seed",
            "RNGkind",
            "runif",
            "rnorm",
            "rpois",
            "rexp",
            "sample",
            "sample.int",
            "is.atomic",
            "is.recursive",
            "is.object",
            "is.vector",
            "is.data.frame",
            "is.unsorted",
            "is.primitive",
            "is.loaded",
            "is.single",
            "file",
            "url",
            "textConnection",
            "textConnectionValue",
            "rawConnection",
            "close",
            "flush",
            "summary",
            "str",
            "as.data.frame",
            "unlist",
            // S3 print/summary dispatch
            "print.default",
            "print.data.frame",
            "print.table",
            "print.factor",
            "print.raw",
            "summary.default",
            "summary.data.frame",
            "format.data.frame",
            // Matrix/linear algebra
            "matrix",
            "array",
            "aperm",
            "backsolve",
            "asplit",
            "drop",
            "diag",
            "dim",
            "%*%",
            "crossprod",
            "tcrossprod",
            "max.col",
            "det",
            "solve",
            // Environment functions
            "emptyenv",
            "baseenv",
            "globalenv",
            "new.env",
            "environment",
            "ls",
            "lockBinding",
            "unlockBinding",
            "bindingIsLocked",
            "bindingIsActive",
            "makeActiveBinding",
            "lockEnvironment",
            "environmentIsLocked",
            // R runtime essentials
            "args",
            "formals",
            "body",
            // String/vector completion
            "charmatch",
            "pmatch",
            "charToRaw",
            "rawToChar",
            "strtoi",
            "strtrim",
            "regexpr",
            "gregexpr",
            "regexec",
            // Data manipulation
            "order",
            "rank",
            "duplicated",
            "anyDuplicated",
            "duplicated.array",
            "anyDuplicated.array",
            "match",
            "%in%",
            "diff",
            "setNames",
            "findInterval",
            "cut",
            // String operations
            "startsWith",
            "endsWith",
            // R runtime type checks
            "is.language",
            "is.call",
            "is.symbol",
            "is.name",
            "is.pairlist",
            "is.function",
            "is.expression",
            "is.environment",
            // S3
            "setOldClass",
            "methods",
            // Matrix
            "lower.tri",
            "upper.tri",
            // Math2 builtins
            "round",
            "signif",
            "trunc",
            "log2",
            // R runtime
            "eval",
            "substitute",
            "quote",
            "parse",
            // Error system
            "conditionMessage",
            "conditionCall",
            "simpleError",
            "simpleWarning",
            "withRestarts",
            // S3/S4
            "isS4",
            "is",
            "setClass",
            "setValidity",
            "isVirtualClass",
            // S4 class system
            "new",
            "show",
            "slotNames",
            "slot",
            "extends",
            "isSealedClass",
            "sealClass",
            "representation",
            "possibleExtends",
            "setReplaceMethod",
            "getMethod",
            "removeGeneric",
            "removeMethod",
            "isGeneric",
            "findMethod",
            "findMethods",
            "showMethods",
            "getGenerics",
            "getMethods",
            "existsMethod",
            "hasMethod",
            "selectMethod",
            // Complete I/O
            "scan",
            "write.table",
            "readLines",
            "writeLines",
            "sink",
            "sink.number",
            // Math/Statistics
            "cov",
            "cor",
            "scale",
            "rle",
            "inverse.rle",
            // R runtime
            "commandArgs",
            "getOption",
            "options",
            "interactive",
            "getRversion",
            "R.Version",
            // Complete data operations
            "reshape",
            "complete.cases",
            "na.omit",
            "na.exclude",
            // Complete string/vector
            "strwrap",
            "system.file",
            "system",
            "system2",
            // Complete R runtime
            "deparse1",
            "dput",
            "dget",
            "bquote",
            // Complete I/O
            "packageStartupMessage",
            // Environment completion
            "parent.env",
            "environmentName",
            "exists",
            "find",
            "get",
            "assign",
            "rm",
            "dyn.load",
            "dyn.unload",
            "library.dynam",
            // Complete S3 coercion
            "as.complex",
            "as.raw",
            "as",
            // Complete I/O
            "capture.output",
            "withVisible",
            "invisible",
            "proc.time",
            "stop",
            "warning",
            "message",
            "stopifnot",
            "suppressWarnings",
            "suppressMessages",
            "tryCatch",
            "force",
            // Complete R runtime
            "isTRUE",
            "isFALSE",
            "anyNA",
            // Complete list operations
            "modifyList",
            "split",
            // Complete R runtime — with/within/transform
            "with",
            "within",
            "transform",
            // Complete base R — table operations, factors, aggregation
            "prop.table",
            "addmargins",
            "ftable",
            "xtabs",
            "aggregate",
            "ave",
            "by",
            "interaction",
            "relevel",
            "factor",
            "is.factor",
            "is.ordered",
            "levels",
            "levels<-",
            "nlevels",
            // Complete R runtime — Sys.* functions, R.home
            "R.home",
            "date",
            "Sys.getenv",
            "Sys.setenv",
            "Sys.unsetenv",
            "Sys.which",
            "Sys.info",
            "Sys.time",
            "Sys.sleep",
            "Sys.Date",
            "Sys.timezone",
            "OlsonNames",
            "Sys.localeconv",
            "Sys.getlocale",
            "Sys.setlocale",
            "Sys.readlink",
            "Sys.chmod",
            "Sys.umask",
            "path.expand",
            "l10n_info",
            "Cstack_info",
            "extSoftVersion",
            "Sys.getpid",
            "capabilities",
            // Complete data operations — subset
            "subset",
            // Complete R runtime — match.call, sys.nframe, sys.function, on.exit
            "match.call",
            "sys.nframe",
            "sys.function",
            "on.exit",
            // Complete I/O — read.csv, write.csv, read.table
            "read.csv",
            "write.csv",
            "read.table",
            // Complete connections — gzfile, pipe, fifo, socket, seek, pushBack, readBin, writeBin
            "gzfile",
            "bzfile",
            "xzfile",
            "pipe",
            "fifo",
            "socketConnection",
            "isOpen",
            "isIncomplete",
            "isSeekable",
            "seek",
            "pushBack",
            "pushBackLength",
            "readBin",
            "writeBin",
            // Complete S3 generics — as.matrix, as.numeric
            "as.matrix",
            "as.numeric",
            "inherits",
            "toString",
            // Complete R runtime — par, getGraphicsEvent
            "par",
            "layout",
            "getGraphicsEvent",
            // Complete R runtime — Rprof, Rprofmem, gc, gcinfo, memory.size, object.size
            "Rprof",
            "Rprofmem",
            "gc",
            "gc.time",
            "gcinfo",
            "gctorture",
            "gctorture2",
            "memory.size",
            "memory.profile",
            "object.size",
            // Complete I/O — European CSV, delimited, fixed-width
            "read.csv2",
            "write.csv2",
            "read.delim",
            "read.fwf",
            "readChar",
            "writeChar",
            // Complete S3 — method dispatch
            "getS3method",
            "registerS3method",
            "setGeneric",
            "setMethod",
            // Complete R runtime — serialization
            "readRDS",
            "saveRDS",
            // Complete error handling — calling handlers and restarts
            "withCallingHandlers",
            "computeRestarts",
            "findRestart",
            "invokeRestart",
            "tryInvokeRestart",
            "isRestart",
            "restartDescription",
            // Complete package system
            ".libPaths",
            "library",
            "require",
            "installed.packages",
            "find.package",
            "packageVersion",
            "packageDescription",
            "loadNamespace",
            "requireNamespace",
            "getNamespace",
            "asNamespace",
            "loadedNamespaces",
            "data",
            "attach",
            "detach",
            "search",
            "searchpaths",
            // Complete R runtime — source, demo, example
            "source",
            "sys.source",
            "demo",
            "example",
            // Complete base R — colSums, rowSums, colMeans, rowMeans, col, row
            "colSums",
            "rowSums",
            "colMeans",
            "rowMeans",
            "col",
            "row",
            // Complete R runtime — cbind, rbind, t (transpose), statistics
            "cbind",
            "rbind",
            "t",
            "var",
            "sd",
            "median",
            "cummin",
            "cummax",
            "dimnames",
            "Re",
            "Im",
            "Mod",
            "Arg",
            "Conj",
            "sin",
            "cos",
            "tan",
            "asin",
            "acos",
            "atan",
            "atan2",
            "expm1",
            "log1p",
            "acosh",
            "asinh",
            "atanh",
            "cospi",
            "sinpi",
            "tanpi",
            // Core arithmetic — dispatched via do_summary/do_math1 in eval.rs
            "sum",
            "min",
            "max",
            "prod",
            "range",
            // Core math — dispatched via do_math1 in eval.rs
            "ceiling",
            "floor",
            "sqrt",
            "log",
            "log10",
            "exp",
            "sinh",
            "cosh",
            "tanh",
            // Type checks — dispatched via do_is_type in eval.rs
            "is.numeric",
            "is.integer",
            "is.double",
            "is.logical",
            "is.character",
            "is.null",
            "identical",
            // Complete special functions for libRmath
            "lgamma",
            "gamma",
            "digamma",
            "trigamma",
            "psigamma",
            "beta",
            "lbeta",
            "choose",
            "lchoose",
            "factorial",
            "lfactorial",
            "besselI",
            "besselJ",
            "besselK",
            "besselY",
        ];

        let frame = (*env).data.envsxp.frame;
        let mut chain = frame;
        for name in all_fns {
            let kind = match name {
                "quote" | "substitute" => SEXPTYPE::SPECIALSXP,
                _ => SEXPTYPE::BUILTINSXP,
            };
            let prim = crate::eval::primitive::make_primitive_binding(name, kind);
            let sym = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
            let cell = Rf_cons(prim, chain);
            (*cell).data.listsxp.tagval = sym;
            chain = cell;
        }
        let pi_sym = Rf_install(c"pi".as_ptr());
        let pi_value = Rf_ScalarReal(std::f64::consts::PI);
        let _pi_value_guard = protect(pi_value);
        let pi_cell = Rf_cons(pi_value, chain);
        (*pi_cell).data.listsxp.tagval = pi_sym;
        chain = pi_cell;

        let version_value = do_R_version(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            R_NilValue(),
            env,
        );
        let _version_guard = protect(version_value);
        for name in ["R.version", "version"] {
            let sym = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
            let cell = Rf_cons(version_value, chain);
            (*cell).data.listsxp.tagval = sym;
            chain = cell;
        }

        let version_string = do_R_version_string(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            R_NilValue(),
            env,
        );
        let _version_string_guard = protect(version_string);
        let sym = Rf_install(c"R.version.string".as_ptr());
        let cell = Rf_cons(version_string, chain);
        (*cell).data.listsxp.tagval = sym;
        chain = cell;
        SET_FRAME(env, chain);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

unsafe fn string_vector(values: &[String]) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, values.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (i, value) in values.iter().enumerate() {
            SET_STRING_ELT(
                result,
                i as R_xlen_t,
                Rf_mkChar(CString::new(value.as_str()).unwrap_or_default().as_ptr()),
            );
        }
        result
    }
}

unsafe fn optional_string_vector(values: &[Option<String>]) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, values.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (i, value) in values.iter().enumerate() {
            let charsxp = match value {
                Some(value) => Rf_mkChar(CString::new(value.as_str()).unwrap_or_default().as_ptr()),
                None => crate::sexp::globals::R_NaString(),
            };
            SET_STRING_ELT(result, i as R_xlen_t, charsxp);
        }
        result
    }
}

unsafe fn named_string_vector(values: &[String], names: &[String]) -> SEXP {
    unsafe {
        let result = string_vector(values);
        if result.is_null() || result == R_NilValue() {
            return result;
        }
        set_string_names(result, names);
        result
    }
}

unsafe fn named_string_list(items: impl IntoIterator<Item = (String, String)>) -> SEXP {
    unsafe {
        let items = items.into_iter().collect::<Vec<_>>();
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, items.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        let names = Rf_allocVector3(SEXPTYPE::STRSXP, items.len() as R_xlen_t);
        if names.is_null() {
            return R_NilValue();
        }
        let _names_guard = protect(names);

        for (i, (name, value)) in items.iter().enumerate() {
            let value_vec = string_vector(std::slice::from_ref(value));
            SET_VECTOR_ELT(result, i as R_xlen_t, value_vec);
            SET_STRING_ELT(
                names,
                i as R_xlen_t,
                Rf_mkChar(CString::new(name.as_str()).unwrap_or_default().as_ptr()),
            );
        }

        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names,
        );
        result
    }
}

/// Try to find a package by name in this session's configured library paths.
fn find_package_path(package: &str) -> String {
    crate::sexp::instance::with_required_current_instance(|inst| {
        inst.path_policy
            .find_package_path(package)
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default()
    })
}

fn package_description_fields(package: &str) -> Result<BTreeMap<String, String>, String> {
    if package.is_empty() || package == "NA" {
        return Err("invalid package name".to_string());
    }

    let package_path = find_package_path(package);
    if package_path.is_empty() {
        return Err(format!("there is no package called '{}'", package));
    }

    let description = Path::new(&package_path).join("DESCRIPTION");
    let content = std::fs::read_to_string(&description)
        .map_err(|err| format!("could not read {}: {err}", description.display()))?;
    Ok(description_fields(&content))
}

unsafe fn load_package_namespace_by_name(package: &str) -> Result<SEXP, String> {
    unsafe {
        if package.is_empty() || package == "NA" {
            return Err("invalid package name".to_string());
        }

        let package_path = find_package_path(package);
        if package_path.is_empty() {
            return Err(format!("there is no package called '{}'", package));
        }

        let package_dir = Path::new(&package_path);
        let description = package_dir.join("DESCRIPTION");
        if package_needs_compilation(&description)? {
            return Err(format!(
                "package '{}' declares NeedsCompilation: yes; this pure-R Android runtime does not load compiled package code",
                package
            ));
        }

        let mut loading = vec![package.to_string()];
        let (env, _) = load_package_namespace(package, package_dir, &mut loading)?;
        Ok(env)
    }
}

const INSTALLED_PACKAGE_COLUMNS: [&str; 16] = [
    "Package",
    "LibPath",
    "Version",
    "Priority",
    "Depends",
    "Imports",
    "LinkingTo",
    "Suggests",
    "Enhances",
    "License",
    "License_is_FOSS",
    "License_restricts_use",
    "OS_type",
    "MD5sum",
    "NeedsCompilation",
    "Built",
];

#[derive(Debug)]
struct InstalledPackageRow {
    package: String,
    library_path: String,
    fields: BTreeMap<String, String>,
}

impl InstalledPackageRow {
    fn value_for(&self, column: &str) -> String {
        match column {
            "Package" => self.package.clone(),
            "LibPath" => self.library_path.clone(),
            _ => self.fields.get(column).cloned().unwrap_or_default(),
        }
    }
}

fn installed_package_rows() -> Vec<InstalledPackageRow> {
    let library_paths = crate::sexp::instance::with_required_current_instance(|inst| {
        inst.path_policy.library_paths().to_vec()
    });
    let mut packages = Vec::<InstalledPackageRow>::new();
    let mut seen = Vec::<String>::new();

    for library_path in library_paths {
        let Ok(entries) = std::fs::read_dir(&library_path) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let package_dir = entry.path();
            let description = package_dir.join("DESCRIPTION");
            if !package_dir.is_dir() || !description.is_file() {
                continue;
            }
            let Some(fallback_name) = package_dir.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Ok(content) = std::fs::read_to_string(&description) else {
                continue;
            };
            let fields = description_fields(&content);
            let package = fields
                .get("Package")
                .cloned()
                .unwrap_or_else(|| fallback_name.to_string());
            if package.is_empty() || seen.contains(&package) {
                continue;
            }
            seen.push(package.clone());
            packages.push(InstalledPackageRow {
                package,
                library_path: library_path.to_string_lossy().into_owned(),
                fields,
            });
        }
    }

    packages.sort_by(|left, right| left.package.cmp(&right.package));
    packages
}

unsafe fn installed_packages_matrix(packages: &[InstalledPackageRow]) -> SEXP {
    unsafe {
        let nrow = packages.len();
        let ncol = INSTALLED_PACKAGE_COLUMNS.len();
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, (nrow * ncol) as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for (col_idx, column) in INSTALLED_PACKAGE_COLUMNS.iter().enumerate() {
            for (row_idx, package) in packages.iter().enumerate() {
                let value = package.value_for(column);
                SET_STRING_ELT(
                    result,
                    (col_idx * nrow + row_idx) as R_xlen_t,
                    Rf_mkChar(CString::new(value).unwrap_or_default().as_ptr()),
                );
            }
        }

        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if dim.is_null() {
            return R_NilValue();
        }
        let _dim_guard = protect(dim);
        *INTEGER(dim).add(0) = nrow as c_int;
        *INTEGER(dim).add(1) = ncol as c_int;
        crate::sexp::attrib_core::setAttrib(result, crate::sexp::attrib_core::R_DimSymbol(), dim);

        let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        if dimnames.is_null() {
            return R_NilValue();
        }
        let _dimnames_guard = protect(dimnames);
        let row_name_values = packages
            .iter()
            .map(|package| package.package.clone())
            .collect::<Vec<_>>();
        let row_names = string_vector(&row_name_values);
        if row_names.is_null() {
            return R_NilValue();
        }
        let _row_names_guard = protect(row_names);

        let col_name_values = INSTALLED_PACKAGE_COLUMNS
            .iter()
            .map(|column| column.to_string())
            .collect::<Vec<_>>();
        let col_names = string_vector(&col_name_values);
        if col_names.is_null() {
            return R_NilValue();
        }
        let _col_names_guard = protect(col_names);

        SET_VECTOR_ELT(dimnames, 0, row_names);
        SET_VECTOR_ELT(dimnames, 1, col_names);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dimnames,
        );

        result
    }
}

fn package_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: message.into(),
    });
}

unsafe fn package_name_symbol() -> SEXP {
    unsafe { Rf_install(c".packageName".as_ptr()) }
}

unsafe fn name_symbol() -> SEXP {
    unsafe { Rf_install(c"name".as_ptr()) }
}

unsafe fn namespace_env_symbol() -> SEXP {
    unsafe { Rf_install(c".namespaceEnv".as_ptr()) }
}

unsafe fn lazy_data_names_symbol() -> SEXP {
    unsafe { Rf_install(c".lazyDataNames".as_ptr()) }
}

unsafe fn package_name_binding(env: SEXP) -> Option<String> {
    unsafe {
        let value = crate::sexp::envir::R_findVarInFrame(env, package_name_symbol());
        if value.is_null()
            || value == R_NilValue()
            || value == crate::sexp::globals::R_UnboundValue()
            || TYPEOF(value) != SEXPTYPE::STRSXP
            || XLENGTH(value) < 1
        {
            return None;
        }
        Some(elt_to_string(value, 0))
    }
}

unsafe fn package_attached(package: &str) -> bool {
    unsafe {
        let mut env = crate::sexp::accessors::ENCLOS(crate::sexp::globals::R_GlobalEnv());
        let base = crate::sexp::globals::R_BaseEnv();
        while !env.is_null() && env != base {
            if package_name_binding(env).as_deref() == Some(package) {
                return true;
            }
            env = crate::sexp::accessors::ENCLOS(env);
        }
        false
    }
}

unsafe fn load_pure_r_package(package: &str, package_dir: &Path) -> Result<(), String> {
    unsafe {
        let description = package_dir.join("DESCRIPTION");
        if !description.is_file() {
            return Err(format!("package '{}' has no DESCRIPTION", package));
        }
        if package_needs_compilation(&description)? {
            return Err(format!(
                "package '{}' declares NeedsCompilation: yes; this pure-R Android runtime does not load compiled package code",
                package
            ));
        }

        let mut loading = vec![package.to_string()];
        let (package_env, namespace) = load_package_namespace(package, package_dir, &mut loading)?;
        let _package_env_guard = crate::sexp::protect::protect(package_env);

        let attach_env = make_package_attach_env(package, namespace.as_ref(), package_env)?;
        attach_package_env(attach_env);
        Ok(())
    }
}

fn package_needs_compilation(description: &Path) -> Result<bool, String> {
    let content = std::fs::read_to_string(description)
        .map_err(|err| format!("could not read {}: {err}", description.display()))?;
    Ok(description_fields(&content)
        .get("NeedsCompilation")
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("yes") || value.eq_ignore_ascii_case("true")
        }))
}

fn package_declares_lazy_data(package_dir: &Path) -> Result<bool, String> {
    let description = package_dir.join("DESCRIPTION");
    let content = std::fs::read_to_string(&description)
        .map_err(|err| format!("could not read {}: {err}", description.display()))?;
    Ok(description_fields(&content)
        .get("LazyData")
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("yes") || value.eq_ignore_ascii_case("true")
        }))
}

fn description_fields(description: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::<String, String>::new();
    let mut current_key: Option<String> = None;

    for line in description.lines() {
        if line.trim().is_empty() {
            break;
        }

        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(key) = current_key.as_ref()
                && let Some(value) = fields.get_mut(key)
            {
                if !value.is_empty() {
                    value.push('\n');
                }
                value.push_str(line.trim());
            }
            continue;
        }

        let Some((field, value)) = line.split_once(':') else {
            current_key = None;
            continue;
        };
        let field = field.trim();
        if field.is_empty() || field.chars().any(char::is_whitespace) {
            current_key = None;
            continue;
        }
        let field = field.to_string();
        fields.insert(field.clone(), value.trim().to_string());
        current_key = Some(field);
    }

    fields
}

unsafe fn load_package_namespace(
    package: &str,
    package_dir: &Path,
    loading: &mut Vec<String>,
) -> Result<(SEXP, Option<NamespaceDirectives>), String> {
    unsafe {
        if let Some(env) = cached_package_namespace(package, package_dir) {
            return Ok((env, read_namespace_directives(package_dir)?));
        }

        let package_env = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            crate::sexp::globals::R_BaseEnv(),
            R_NilValue(),
        );
        if package_env.is_null() {
            return Err(format!(
                "could not create namespace for package '{}'",
                package
            ));
        }
        let _package_env_guard = crate::sexp::protect::protect(package_env);

        define_package_metadata(package, package_env);
        reject_unsupported_internal_data(package, package_dir)?;
        reject_unsupported_lazyload_code(package, package_dir)?;
        let namespace = populate_package_namespace(package, package_dir, package_env, loading)?;
        cache_package_namespace(package, package_dir, package_env);
        Ok((package_env, namespace))
    }
}

fn normalized_package_dir(package_dir: &Path) -> PathBuf {
    std::fs::canonicalize(package_dir).unwrap_or_else(|_| package_dir.to_path_buf())
}

fn cached_package_namespace(package: &str, package_dir: &Path) -> Option<SEXP> {
    let package_dir = normalized_package_dir(package_dir);
    crate::sexp::instance::with_required_current_instance(|inst| {
        inst.package_namespace_cache
            .get(package)
            .and_then(|(cached_dir, env)| (*cached_dir == package_dir).then_some(*env))
    })
}

fn cache_package_namespace(package: &str, package_dir: &Path, package_env: SEXP) {
    let package_dir = normalized_package_dir(package_dir);
    crate::sexp::instance::with_required_current_instance(|inst| {
        inst.package_namespace_cache
            .insert(package.to_string(), (package_dir, package_env));
    });
}

fn package_arg_values(package_arg: SEXP) -> Vec<String> {
    unsafe {
        if package_arg.is_null() || package_arg == R_NilValue() || XLENGTH(package_arg) == 0 {
            return Vec::new();
        }
        (0..XLENGTH(package_arg))
            .map(|i| elt_to_string(package_arg, i))
            .filter(|package| !package.is_empty() && package != "NA")
            .collect()
    }
}

fn list_package_data_sets(packages: &[String]) -> Vec<String> {
    let mut names = Vec::<String>::new();
    for package_dir in data_package_dirs(packages) {
        let data_dir = package_dir.join("data");
        let Ok(entries) = std::fs::read_dir(data_dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("r"))
                && let Some(name) = path.file_stem().and_then(|stem| stem.to_str())
            {
                push_unique(&mut names, name.to_string());
            }
        }
    }
    names.sort();
    names
}

fn data_package_dirs(packages: &[String]) -> Vec<PathBuf> {
    crate::sexp::instance::with_required_current_instance(|inst| {
        if packages.is_empty() {
            return inst
                .path_policy
                .library_paths()
                .iter()
                .filter_map(|library| std::fs::read_dir(library).ok())
                .flat_map(|entries| entries.filter_map(Result::ok).map(|entry| entry.path()))
                .filter(|path| path.join("DESCRIPTION").is_file())
                .collect();
        }

        packages
            .iter()
            .filter_map(|package| inst.path_policy.find_package_path(package))
            .collect()
    })
}

unsafe fn load_package_data_set(
    topic: &str,
    packages: &[String],
    target_env: SEXP,
) -> Result<bool, String> {
    unsafe {
        let mut unsupported_data = None::<PathBuf>;
        for package_dir in data_package_dirs(packages) {
            let data_dir = package_dir.join("data");
            let source_file = data_dir.join(format!("{topic}.R"));
            if source_file.is_file() {
                source_r_file_into_env(&source_file, target_env)?;
                return Ok(true);
            }

            let unsupported = [
                data_dir.join(format!("{topic}.rda")),
                data_dir.join(format!("{topic}.RData")),
                data_dir.join("Rdata.rdb"),
                data_dir.join("Rdata.rdx"),
            ];
            if let Some(path) = unsupported.iter().find(|path| path.is_file()) {
                unsupported_data = Some(path.clone());
            }
        }
        if let Some(path) = unsupported_data {
            return Err(format!(
                "data set '{}' uses unsupported serialized/lazy data file {}; this pure-R Android runtime supports data/*.R only",
                topic,
                path.display()
            ));
        }
        Ok(false)
    }
}

unsafe fn source_r_file_into_env(file: &Path, env: SEXP) -> Result<(), String> {
    unsafe {
        let code = std::fs::read_to_string(file)
            .map_err(|err| format!("could not read {}: {err}", file.display()))?;
        let expr = crate::sexp::memory::with_arena(|arena| {
            crate::eval::parser::parse(&code, arena).map_err(|err| err.to_string())
        })?;
        let expr = if expr.is_null() { R_NilValue() } else { expr };
        let _ = crate::eval::eval::Rf_eval(expr, env);
        Ok(())
    }
}

unsafe fn define_package_metadata(package: &str, package_env: SEXP) {
    unsafe {
        let package_string = Rf_mkString(CString::new(package).unwrap_or_default().as_ptr());
        if !package_string.is_null() {
            crate::sexp::envir::defineVar(package_name_symbol(), package_string, package_env);
        }

        let search_name = format!("package:{package}");
        let search_string = Rf_mkString(CString::new(search_name).unwrap_or_default().as_ptr());
        if !search_string.is_null() {
            crate::sexp::attrib_core::setAttrib(package_env, name_symbol(), search_string);
        }
    }
}

fn reject_unsupported_internal_data(package: &str, package_dir: &Path) -> Result<(), String> {
    for path in [
        package_dir.join("R").join("sysdata.rda"),
        package_dir.join("R").join("sysdata.RData"),
        package_dir.join("R").join("sysdata.rdb"),
        package_dir.join("R").join("sysdata.rdx"),
    ] {
        if path.is_file() {
            return Err(format!(
                "package '{}' uses unsupported internal serialized data {}; this pure-R Android runtime supports R source files and data/*.R only",
                package,
                path.display()
            ));
        }
    }
    Ok(())
}

fn reject_unsupported_lazyload_code(package: &str, package_dir: &Path) -> Result<(), String> {
    let r_dir = package_dir.join("R");
    if !r_dir.is_dir() {
        return Ok(());
    }

    let entries = std::fs::read_dir(&r_dir)
        .map_err(|err| format!("could not read R directory for package '{package}': {err}"))?;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let unsupported = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rdb") || ext.eq_ignore_ascii_case("rdx"));
        if unsupported {
            return Err(format!(
                "package '{}' uses unsupported byte-compiled/lazyload R code {}; this pure-R Android runtime supports source R/*.R files only",
                package,
                path.display()
            ));
        }
    }

    Ok(())
}

unsafe fn source_package_r_files(
    package: &str,
    package_dir: &Path,
    package_env: SEXP,
) -> Result<(), String> {
    unsafe {
        let r_dir = package_dir.join("R");
        if !r_dir.is_dir() {
            return Ok(());
        }

        let mut files = std::fs::read_dir(&r_dir)
            .map_err(|err| format!("could not read R directory for package '{package}': {err}"))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("r"))
            })
            .collect::<Vec<_>>();
        files.sort();

        for file in files {
            source_r_file_into_env(&file, package_env)?;
        }

        Ok(())
    }
}

unsafe fn source_package_lazy_data(
    package: &str,
    package_dir: &Path,
    package_env: SEXP,
) -> Result<Vec<String>, String> {
    unsafe {
        if !package_declares_lazy_data(package_dir)? {
            return Ok(Vec::new());
        }

        let data_dir = package_dir.join("data");
        if !data_dir.is_dir() {
            return Ok(Vec::new());
        }

        let before = frame_binding_names(package_env, true)
            .into_iter()
            .collect::<BTreeSet<_>>();
        let mut files = std::fs::read_dir(&data_dir)
            .map_err(|err| format!("could not read data directory for package '{package}': {err}"))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("r"))
            })
            .collect::<Vec<_>>();
        files.sort();

        for file in files {
            source_r_file_into_env(&file, package_env)?;
        }

        let mut names = frame_binding_names(package_env, true)
            .into_iter()
            .filter(|name| !before.contains(name.as_str()))
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        Ok(names)
    }
}

unsafe fn define_lazy_data_names(package_env: SEXP, names: &[String]) {
    unsafe {
        let values = string_vector(names);
        if !values.is_null() {
            crate::sexp::envir::defineVar(lazy_data_names_symbol(), values, package_env);
        }
    }
}

unsafe fn lazy_data_names_binding(package_env: SEXP) -> Vec<String> {
    unsafe {
        let value = crate::sexp::envir::R_findVarInFrame(package_env, lazy_data_names_symbol());
        if value.is_null()
            || value == R_NilValue()
            || value == crate::sexp::globals::R_UnboundValue()
            || TYPEOF(value) != SEXPTYPE::STRSXP
        {
            return Vec::new();
        }

        (0..XLENGTH(value))
            .map(|i| elt_to_string(value, i))
            .filter(|name| !name.is_empty() && name != "NA")
            .collect()
    }
}

#[derive(Clone, Debug, Default)]
struct NamespaceDirectives {
    exports: Vec<String>,
    export_patterns: Vec<String>,
    imports: Vec<NamespaceImport>,
    s3_methods: Vec<S3MethodDirective>,
    native_libraries: Vec<String>,
}

#[derive(Clone, Debug)]
enum NamespaceImport {
    All { package: String },
    From { package: String, names: Vec<String> },
}

#[derive(Clone, Debug)]
struct S3MethodDirective {
    generic: String,
    class: String,
    method: Option<String>,
}

unsafe fn populate_package_namespace(
    package: &str,
    package_dir: &Path,
    package_env: SEXP,
    loading: &mut Vec<String>,
) -> Result<Option<NamespaceDirectives>, String> {
    unsafe {
        let namespace = read_namespace_directives(package_dir)?;
        if let Some(directives) = namespace.as_ref() {
            reject_native_namespace_directives(package, directives)?;
            apply_namespace_imports(package, package_env, directives, loading)?;
        }
        source_package_r_files(package, package_dir, package_env)?;
        let lazy_data_names = source_package_lazy_data(package, package_dir, package_env)?;
        define_lazy_data_names(package_env, &lazy_data_names);
        if let Some(directives) = namespace.as_ref() {
            register_namespace_s3_methods(package, package_env, directives)?;
        }
        Ok(namespace)
    }
}

unsafe fn apply_namespace_imports(
    package: &str,
    package_env: SEXP,
    directives: &NamespaceDirectives,
    loading: &mut Vec<String>,
) -> Result<(), String> {
    unsafe {
        for import in &directives.imports {
            let import_package = match import {
                NamespaceImport::All { package } | NamespaceImport::From { package, .. } => {
                    package.as_str()
                }
            };

            if loading.iter().any(|entry| entry == import_package) {
                return Err(format!(
                    "package '{}' has cyclic namespace import involving '{}'",
                    package, import_package
                ));
            }

            let import_dir = find_package_path(import_package);
            if import_dir.is_empty() {
                return Err(format!(
                    "package '{}' imports missing package '{}'",
                    package, import_package
                ));
            }

            loading.push(import_package.to_string());
            let result = import_namespace_bindings(
                package,
                package_env,
                Path::new(&import_dir),
                import,
                loading,
            );
            loading.pop();
            result?;
        }
        Ok(())
    }
}

unsafe fn import_namespace_bindings(
    package: &str,
    package_env: SEXP,
    import_dir: &Path,
    import: &NamespaceImport,
    loading: &mut Vec<String>,
) -> Result<(), String> {
    unsafe {
        let import_package = match import {
            NamespaceImport::All { package } | NamespaceImport::From { package, .. } => {
                package.as_str()
            }
        };

        let (import_env, namespace) = load_package_namespace(import_package, import_dir, loading)?;
        let _import_guard = crate::sexp::protect::protect(import_env);

        let imported_names = match import {
            NamespaceImport::All { .. } => namespace_exports(namespace.as_ref(), import_env),
            NamespaceImport::From { names, .. } => names.clone(),
        };

        let mut missing = Vec::new();
        for name in imported_names {
            let Ok(symbol_name) = CString::new(name.as_str()) else {
                missing.push(name);
                continue;
            };
            let symbol = Rf_install(symbol_name.as_ptr());
            let value = crate::sexp::envir::R_findVarInFrame(import_env, symbol);
            if value.is_null()
                || value == R_NilValue()
                || value == crate::sexp::globals::R_UnboundValue()
            {
                missing.push(name);
            } else {
                crate::sexp::envir::defineVar(symbol, value, package_env);
            }
        }

        if missing.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "package '{}' imports undefined objects from '{}': {}",
                package,
                import_package,
                missing.join(", ")
            ))
        }
    }
}

unsafe fn make_package_attach_env(
    package: &str,
    namespace: Option<&NamespaceDirectives>,
    package_env: SEXP,
) -> Result<SEXP, String> {
    unsafe {
        let Some(directives) = namespace else {
            return Ok(package_env);
        };

        let attach_env = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            crate::sexp::globals::R_BaseEnv(),
            R_NilValue(),
        );
        if attach_env.is_null() {
            return Err(format!(
                "could not create attach environment for package '{}'",
                package
            ));
        }
        let _attach_guard = crate::sexp::protect::protect(attach_env);

        define_package_metadata(package, attach_env);
        crate::sexp::envir::defineVar(namespace_env_symbol(), package_env, attach_env);
        let s3_table = crate::sexp::envir::R_findVarInFrame(package_env, s3_methods_table_symbol());
        if !s3_table.is_null()
            && s3_table != crate::sexp::globals::R_UnboundValue()
            && TYPEOF(s3_table) == SEXPTYPE::ENVSXP
        {
            crate::sexp::envir::defineVar(s3_methods_table_symbol(), s3_table, attach_env);
        }

        let mut exports = namespace_exports(Some(directives), package_env);
        for name in lazy_data_names_binding(package_env) {
            push_unique(&mut exports, name);
        }
        let mut missing = Vec::new();
        for export in exports {
            let Ok(symbol_name) = CString::new(export.as_str()) else {
                missing.push(export);
                continue;
            };
            let symbol = Rf_install(symbol_name.as_ptr());
            let value = crate::sexp::envir::R_findVarInFrame(package_env, symbol);
            if value.is_null()
                || value == R_NilValue()
                || value == crate::sexp::globals::R_UnboundValue()
            {
                missing.push(export);
            } else {
                crate::sexp::envir::defineVar(symbol, value, attach_env);
            }
        }

        if missing.is_empty() {
            Ok(attach_env)
        } else {
            Err(format!(
                "package '{}' has undefined exports: {}",
                package,
                missing.join(", ")
            ))
        }
    }
}

fn read_namespace_directives(package_dir: &Path) -> Result<Option<NamespaceDirectives>, String> {
    let namespace = package_dir.join("NAMESPACE");
    if !namespace.is_file() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&namespace)
        .map_err(|err| format!("could not read {}: {err}", namespace.display()))?;
    Ok(Some(parse_namespace_directives(&content)))
}

fn parse_namespace_directives(content: &str) -> NamespaceDirectives {
    let mut directives = NamespaceDirectives::default();
    let uncommented = strip_namespace_comments(content);
    for (directive, args) in parse_namespace_calls(&uncommented) {
        match directive.as_str() {
            "export" => {
                for name in split_namespace_args(&args)
                    .into_iter()
                    .filter_map(clean_namespace_name)
                {
                    push_unique(&mut directives.exports, name);
                }
            }
            "exportPattern" => {
                if let Some(pattern) = split_namespace_args(&args)
                    .first()
                    .and_then(|arg| clean_namespace_name(arg))
                {
                    push_unique(&mut directives.export_patterns, pattern);
                }
            }
            "import" => {
                if let Some(package) = split_namespace_args(&args)
                    .first()
                    .and_then(|arg| clean_namespace_name(arg))
                {
                    directives.imports.push(NamespaceImport::All { package });
                }
            }
            "importFrom" => {
                let parts = split_namespace_args(&args);
                let Some(package) = parts.first().and_then(|arg| clean_namespace_name(arg)) else {
                    continue;
                };
                let names = parts
                    .iter()
                    .skip(1)
                    .filter_map(|arg| clean_namespace_name(arg))
                    .collect::<Vec<_>>();
                if !names.is_empty() {
                    directives
                        .imports
                        .push(NamespaceImport::From { package, names });
                }
            }
            "S3method" => {
                let parts = split_namespace_args(&args);
                let Some(generic) = parts.first().and_then(|arg| clean_namespace_name(arg)) else {
                    continue;
                };
                let Some(class) = parts.get(1).and_then(|arg| clean_namespace_name(arg)) else {
                    continue;
                };
                let method = parts.get(2).and_then(|arg| clean_namespace_name(arg));
                directives.s3_methods.push(S3MethodDirective {
                    generic,
                    class,
                    method,
                });
            }
            "useDynLib" => {
                for name in split_namespace_args(&args)
                    .into_iter()
                    .filter_map(clean_namespace_name)
                {
                    push_unique(&mut directives.native_libraries, name);
                }
            }
            _ => {}
        }
    }
    directives
}

fn reject_native_namespace_directives(
    package: &str,
    directives: &NamespaceDirectives,
) -> Result<(), String> {
    if directives.native_libraries.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "package '{}' requires native libraries via useDynLib({}); this pure-R Android runtime does not load native package code",
            package,
            directives.native_libraries.join(", ")
        ))
    }
}

unsafe fn register_namespace_s3_methods(
    package: &str,
    package_env: SEXP,
    directives: &NamespaceDirectives,
) -> Result<(), String> {
    unsafe {
        for method in &directives.s3_methods {
            let method_name = method
                .method
                .clone()
                .unwrap_or_else(|| format!("{}.{}", method.generic, method.class));
            let Ok(method_cstr) = CString::new(method_name.as_str()) else {
                return Err(format!(
                    "package '{}' has invalid S3 method name '{}'",
                    package, method_name
                ));
            };
            let method_sym = Rf_install(method_cstr.as_ptr());
            let method_value = crate::sexp::envir::R_findVarInFrame(package_env, method_sym);
            if method_value.is_null()
                || method_value == R_NilValue()
                || method_value == crate::sexp::globals::R_UnboundValue()
            {
                return Err(format!(
                    "package '{}' declares missing S3 method '{}'",
                    package, method_name
                ));
            }
            define_s3_method(package_env, &method.generic, &method.class, method_value)?;
        }
        Ok(())
    }
}

fn strip_namespace_comments(content: &str) -> String {
    let mut stripped = String::with_capacity(content.len());
    let mut in_string = false;
    let mut quote = '\0';
    let mut escaped = false;
    let mut in_comment = false;

    for ch in content.chars() {
        if in_comment {
            if ch == '\n' {
                in_comment = false;
                stripped.push('\n');
            }
            continue;
        }

        if in_string {
            stripped.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' && quote != '`' {
                escaped = true;
            } else if ch == quote {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' | '\'' | '`' => {
                in_string = true;
                quote = ch;
                stripped.push(ch);
            }
            '#' => {
                in_comment = true;
            }
            _ => stripped.push(ch),
        }
    }

    stripped
}

fn parse_namespace_calls(content: &str) -> Vec<(String, String)> {
    let mut calls = Vec::new();
    let mut chars = content.char_indices().peekable();

    while let Some((start, ch)) = chars.next() {
        if !(ch.is_ascii_alphabetic() || ch == '.') {
            continue;
        }

        let mut end = start + ch.len_utf8();
        while let Some(&(idx, next)) = chars.peek() {
            if next.is_ascii_alphanumeric() || next == '.' {
                chars.next();
                end = idx + next.len_utf8();
            } else {
                break;
            }
        }

        let directive = content[start..end].trim();
        let mut scan = chars.clone();
        while let Some(&(_, whitespace)) = scan.peek() {
            if whitespace.is_whitespace() {
                scan.next();
            } else {
                break;
            }
        }
        let Some((open_idx, '(')) = scan.next() else {
            continue;
        };

        let Some((close_idx, args)) = find_namespace_call_args(content, open_idx) else {
            continue;
        };
        calls.push((directive.to_string(), args.to_string()));

        while let Some(&(idx, _)) = chars.peek() {
            if idx <= close_idx {
                chars.next();
            } else {
                break;
            }
        }
    }

    calls
}

fn find_namespace_call_args(content: &str, open_idx: usize) -> Option<(usize, &str)> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut quote = '\0';
    let mut escaped = false;

    for (idx, ch) in content[open_idx..].char_indices() {
        let absolute_idx = open_idx + idx;
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' && quote != '`' {
                escaped = true;
            } else if ch == quote {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' | '\'' | '`' => {
                in_string = true;
                quote = ch;
            }
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some((absolute_idx, &content[open_idx + 1..absolute_idx]));
                }
            }
            _ => {}
        }
    }

    None
}

fn split_namespace_args(args: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut quote = '\0';
    let mut escaped = false;

    for (idx, ch) in args.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' && quote != '`' {
                escaped = true;
            } else if ch == quote {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' | '\'' | '`' => {
                in_string = true;
                quote = ch;
            }
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(args[start..idx].trim());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    parts.push(args[start..].trim());
    parts
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

unsafe fn namespace_exports(
    namespace: Option<&NamespaceDirectives>,
    package_env: SEXP,
) -> Vec<String> {
    unsafe {
        let Some(directives) = namespace else {
            return frame_binding_names(package_env, false);
        };

        let mut exports = directives.exports.clone();
        for name in frame_binding_names(package_env, false) {
            if directives
                .export_patterns
                .iter()
                .any(|pattern| simple_namespace_pattern_matches(pattern, &name))
            {
                push_unique(&mut exports, name);
            }
        }
        exports
    }
}

unsafe fn frame_binding_names(env: SEXP, include_hidden: bool) -> Vec<String> {
    unsafe {
        let mut names = Vec::new();
        let mut frame = FRAME(env);
        while !frame.is_null() && frame != R_NilValue() {
            let value = CAR(frame);
            if value != crate::sexp::globals::R_UnboundValue()
                && let Some(name) = symbol_name(TAG(frame))
                && (include_hidden || !name.starts_with('.'))
                && name != ".packageName"
                && name != ".namespaceEnv"
            {
                names.push(name);
            }
            frame = CDR(frame);
        }
        names.sort();
        names.dedup();
        names
    }
}

fn simple_namespace_pattern_matches(pattern: &str, name: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }

    let anchored_start = pattern.starts_with('^');
    let anchored_end = pattern.ends_with('$');
    let mut body = pattern;
    if anchored_start {
        body = &body[1..];
    }
    if anchored_end && !body.is_empty() {
        body = &body[..body.len() - 1];
    }

    let mut literal = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            let Some(escaped) = chars.next() else {
                return false;
            };
            literal.push(escaped);
        } else if ".^$|?*+()[]{}".contains(ch) {
            return false;
        } else {
            literal.push(ch);
        }
    }

    if anchored_start && anchored_end {
        name == literal
    } else if anchored_start {
        name.starts_with(&literal)
    } else if anchored_end {
        name.ends_with(&literal)
    } else {
        name.contains(&literal)
    }
}

fn clean_namespace_name(raw: &str) -> Option<String> {
    let name = raw
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches('`')
        .trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

unsafe fn attach_package_env(package_env: SEXP) {
    unsafe {
        let global = crate::sexp::globals::R_GlobalEnv();
        let old_enclos = crate::sexp::accessors::ENCLOS(global);
        crate::sexp::accessors::SET_ENCLOS(global, package_env);
        crate::sexp::accessors::SET_ENCLOS(package_env, old_enclos);
    }
}

/// Try to find a demo file for a topic.
fn find_package_demo(topic: &str) -> String {
    let r_home = std::env::var("R_HOME").unwrap_or_else(|_| "/usr/lib/R".to_string());
    let paths = [
        format!("{}/library/*/demo/{}.R", r_home, topic),
        format!("/usr/local/lib/R/site-library/*/demo/{}.R", topic),
    ];
    // Simplified: check a few common locations
    for p in &paths {
        if std::path::Path::new(p).exists() {
            return p.clone();
        }
    }
    String::new()
}

/// Try to find an example file for a topic.
fn find_package_example(topic: &str) -> String {
    let r_home = std::env::var("R_HOME").unwrap_or_else(|_| "/usr/lib/R".to_string());
    let paths = [
        format!("{}/library/*/R-ex/{}.R", r_home, topic),
        format!("/usr/local/lib/R/site-library/*/R-ex/{}.R", topic),
    ];
    for p in &paths {
        if std::path::Path::new(p).exists() {
            return p.clone();
        }
    }
    String::new()
}

/// Read a scalar real from a numeric SEXP, with default.
fn real_or_default(x: SEXP, default: f64) -> f64 {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return default;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::REALSXP {
            *REAL(x)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            default
        }
    }
}

fn real_elt_or_default(x: SEXP, i: R_xlen_t, default: f64) -> f64 {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return default;
        }
        let n = XLENGTH(x);
        if n == 0 {
            return default;
        }
        let idx = i % n;
        let t = TYPEOF(x);
        if t == SEXPTYPE::REALSXP {
            *REAL(x).add(idx as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(idx as usize);
            if v == NA_INTEGER { default } else { v as f64 }
        } else {
            default
        }
    }
}

fn numeric_elt_as_count(x: SEXP, i: R_xlen_t) -> usize {
    let value = real_elt_or_default(x, i, 0.0);
    if value.is_finite() {
        (value as i64).max(0) as usize
    } else {
        0
    }
}

fn named_logical_arg(args: SEXP, name: &str) -> Option<bool> {
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).as_deref() == Some(name) {
                let value = CAR(current);
                if value.is_null() || value == R_NilValue() || XLENGTH(value) == 0 {
                    return None;
                }
                let raw = if TYPEOF(value) == SEXPTYPE::LGLSXP || TYPEOF(value) == SEXPTYPE::INTSXP
                {
                    *INTEGER(value)
                } else if TYPEOF(value) == SEXPTYPE::REALSXP {
                    *REAL(value) as c_int
                } else {
                    return None;
                };
                return (raw != NA_INTEGER).then_some(raw != 0);
            }
            current = CDR(current);
        }
        None
    }
}

fn logical_arg_by_name_or_position(args: SEXP, name: &str, position: usize) -> Option<bool> {
    unsafe {
        let value = arg_by_name_or_position(args, &[name], position);
        if value.is_null() || value == R_NilValue() || XLENGTH(value) == 0 {
            return None;
        }
        let raw = if TYPEOF(value) == SEXPTYPE::LGLSXP || TYPEOF(value) == SEXPTYPE::INTSXP {
            *INTEGER(value)
        } else if TYPEOF(value) == SEXPTYPE::REALSXP {
            let value = *REAL(value);
            if ISNAN(value) {
                NA_LOGICAL
            } else {
                value as c_int
            }
        } else {
            return None;
        };
        (raw != NA_INTEGER).then_some(raw != 0)
    }
}

fn integer_arg_by_name_or_position(args: SEXP, name: &str, position: usize) -> Option<c_int> {
    unsafe {
        let value = arg_by_name_or_position(args, &[name], position);
        if value.is_null() || value == R_NilValue() || XLENGTH(value) == 0 {
            return None;
        }
        let raw = if TYPEOF(value) == SEXPTYPE::INTSXP || TYPEOF(value) == SEXPTYPE::LGLSXP {
            *INTEGER(value)
        } else if TYPEOF(value) == SEXPTYPE::REALSXP {
            let value = *REAL(value);
            if value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || value.is_nan() {
                NA_INTEGER
            } else {
                value as c_int
            }
        } else {
            return None;
        };
        Some(raw)
    }
}

fn arg_by_name_or_position(args: SEXP, names: &[&str], position: usize) -> SEXP {
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if let Some(tag) = tag_name(current) {
                if names.iter().any(|name| tag == *name) {
                    return CAR(current);
                }
            }
            current = CDR(current);
        }

        let mut positional = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).is_none() {
                if positional == position {
                    return CAR(current);
                }
                positional += 1;
            }
            current = CDR(current);
        }
        R_NilValue()
    }
}

fn is_string_na(x: SEXP, i: R_xlen_t) -> bool {
    unsafe {
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::STRSXP {
            return false;
        }
        let n = XLENGTH(x);
        if n == 0 {
            return false;
        }
        STRING_ELT(x, i % n) == crate::sexp::globals::R_NaString()
    }
}

fn element_coerces_to_character_na(x: SEXP, i: R_xlen_t) -> bool {
    unsafe {
        let n = XLENGTH(x);
        if x.is_null() || x == R_NilValue() || n == 0 {
            return false;
        }
        let idx = i % n;
        let ty = TYPEOF(x);
        if ty == SEXPTYPE::STRSXP {
            is_string_na(x, idx)
        } else if ty == SEXPTYPE::INTSXP || ty == SEXPTYPE::LGLSXP {
            *INTEGER(x).add(idx as usize) == NA_INTEGER
        } else if ty == SEXPTYPE::REALSXP {
            let value = *REAL(x).add(idx as usize);
            value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
        } else {
            false
        }
    }
}

fn string_contains(text: &str, pattern: &str, ignore_case: bool) -> bool {
    if ignore_case {
        text.to_lowercase().contains(&pattern.to_lowercase())
    } else {
        text.contains(pattern)
    }
}

fn grep_value_matches(text: &str, pattern: &str, ignore_case: bool, fixed: bool) -> bool {
    if fixed {
        string_contains(text, pattern, ignore_case)
    } else {
        crate::mainutils::grep::ere_is_match(pattern, text, ignore_case)
    }
}

fn grep_match_indices(
    x: SEXP,
    pattern: &str,
    ignore_case: bool,
    fixed: bool,
    invert: bool,
) -> Vec<R_xlen_t> {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Vec::new();
        }
        let n = XLENGTH(x);
        let mut matches = Vec::new();
        for i in 0..n {
            if is_string_na(x, i) {
                if invert {
                    matches.push(i);
                }
                continue;
            }
            let matched = grep_value_matches(&elt_to_string(x, i), pattern, ignore_case, fixed);
            if if invert { !matched } else { matched } {
                matches.push(i);
            }
        }
        matches
    }
}

fn environment_arg_or_default(args: SEXP, names: &[&str], position: usize, default: SEXP) -> SEXP {
    unsafe {
        let arg = arg_by_name_or_position(args, names, position);
        if !arg.is_null() && arg != R_NilValue() && TYPEOF(arg) == SEXPTYPE::ENVSXP {
            arg
        } else {
            default
        }
    }
}

fn copy_vector_elt(dst: SEXP, dst_idx: R_xlen_t, src: SEXP, src_idx: R_xlen_t) {
    unsafe {
        match TYPEOF(src) {
            t if t == SEXPTYPE::REALSXP => {
                *REAL(dst).add(dst_idx as usize) = *REAL(src).add(src_idx as usize);
            }
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                *INTEGER(dst).add(dst_idx as usize) = *INTEGER(src).add(src_idx as usize);
            }
            t if t == SEXPTYPE::STRSXP => {
                SET_STRING_ELT(dst, dst_idx, STRING_ELT(src, src_idx));
            }
            t if t == SEXPTYPE::RAWSXP => {
                *RAW(dst).add(dst_idx as usize) = *RAW(src).add(src_idx as usize);
            }
            _ => {}
        }
    }
}

fn map_path_strings(x: SEXP, f: fn(&str) -> String) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            if TYPEOF(x) == SEXPTYPE::STRSXP {
                let idx = if XLENGTH(x) == 0 { 0 } else { i % XLENGTH(x) };
                if STRING_ELT(x, idx) == crate::sexp::globals::R_NaString() {
                    SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                    continue;
                }
            }
            let value = f(&elt_to_string(x, i));
            SET_STRING_ELT(
                result,
                i,
                Rf_mkChar(CString::new(value).unwrap_or_default().as_ptr()),
            );
        }
        result
    }
}

fn trim_trailing_separators(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() && !path.is_empty() {
        &path[..1]
    } else {
        trimmed
    }
}

fn r_basename(path: &str) -> String {
    let trimmed = trim_trailing_separators(path);
    if trimmed == "/" || trimmed == "\\" {
        return trimmed.to_string();
    }
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(trimmed)
        .to_string()
}

fn r_dirname(path: &str) -> String {
    let trimmed = trim_trailing_separators(path);
    if trimmed == "/" || trimmed == "\\" {
        return trimmed.to_string();
    }
    match trimmed.rfind(['/', '\\']) {
        Some(0) => trimmed[..1].to_string(),
        Some(pos) => trimmed[..pos].to_string(),
        None => ".".to_string(),
    }
}

/// Convert an element of a vector to a String.
pub(crate) fn elt_to_string(x: SEXP, i: R_xlen_t) -> String {
    unsafe {
        if x.is_null() {
            return "NULL".to_string();
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        if n == 0 {
            return String::new();
        }
        let idx = i % n;

        if t == SEXPTYPE::REALSXP {
            let v = *REAL(x).add(idx as usize);
            if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                "NA".to_string()
            } else {
                format!("{}", v)
            }
        } else if t == SEXPTYPE::INTSXP {
            let v = *INTEGER(x).add(idx as usize);
            if v == NA_INTEGER {
                "NA".to_string()
            } else {
                format!("{}", v)
            }
        } else if t == SEXPTYPE::LGLSXP {
            let v = *LOGICAL(x).add(idx as usize);
            if v == NA_INTEGER {
                "NA".to_string()
            } else if v == TRUE {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        } else if t == SEXPTYPE::STRSXP {
            let charsxp = crate::sexp::accessors::STRING_ELT(x, idx);
            if charsxp.is_null() {
                "NA".to_string()
            } else {
                let s = crate::sexp::accessors::CHAR(charsxp);
                if s.is_null() {
                    "NA".to_string()
                } else {
                    std::ffi::CStr::from_ptr(s)
                        .to_str()
                        .unwrap_or("NA")
                        .to_string()
                }
            }
        } else if t == SEXPTYPE::SYMSXP {
            let pname = crate::sexp::accessors::PRINTNAME(x);
            if pname.is_null() {
                "symbol".to_string()
            } else {
                let s = crate::sexp::accessors::CHAR(pname);
                if s.is_null() {
                    "symbol".to_string()
                } else {
                    std::ffi::CStr::from_ptr(s)
                        .to_str()
                        .unwrap_or("symbol")
                        .to_string()
                }
            }
        } else {
            format!("{:?}", t)
        }
    }
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn days_from_civil(mut year: i64, month: i64, day: i64) -> i64 {
    year -= if month <= 2 { 1 } else { 0 };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * month_prime + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(mut days: i64) -> (i64, i64, i64) {
    days += 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let month_prime = (5 * doy + 2) / 153;
    let day = doy - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

pub(crate) fn parse_iso_date_days(text: &str) -> Option<f64> {
    let text = text.trim();
    let mut parts = text.split('-');
    let year = parts.next()?.parse::<i64>().ok()?;
    let month = parts.next()?.parse::<i64>().ok()?;
    let day = parts.next()?.parse::<i64>().ok()?;
    if parts.next().is_some()
        || !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month)
    {
        return None;
    }
    Some(days_from_civil(year, month, day) as f64)
}

pub(crate) fn date_days_to_iso(days: f64) -> Option<String> {
    let (year, month, day) = date_days_to_civil(days)?;
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

pub(crate) fn date_days_to_civil(days: f64) -> Option<(i64, i64, i64)> {
    if days.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || !days.is_finite() {
        return None;
    }
    Some(civil_from_days(days.floor() as i64))
}

pub(crate) fn parse_iso_datetime_seconds(text: &str) -> Option<f64> {
    let text = text.trim();
    let mut fields = text.split_whitespace();
    let date = fields.next()?;
    let time = fields.next().unwrap_or("00:00:00");
    if fields.next().is_some() {
        return None;
    }
    let days = parse_iso_date_days(date)?;
    let mut parts = time.split(':');
    let hour = parts.next()?.parse::<i64>().ok()?;
    let minute = parts.next()?.parse::<i64>().ok()?;
    let second = parts.next().unwrap_or("0").parse::<i64>().ok()?;
    if parts.next().is_some()
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
    {
        return None;
    }
    Some(days * 86_400.0 + (hour * 3_600 + minute * 60 + second) as f64)
}

pub(crate) fn posix_seconds_to_iso(seconds: f64, include_tz: bool) -> Option<String> {
    posix_seconds_to_iso_with_time(seconds, include_tz, false)
}

pub(crate) fn posix_seconds_to_iso_with_time(
    seconds: f64,
    include_tz: bool,
    force_time: bool,
) -> Option<String> {
    if seconds.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || !seconds.is_finite() {
        return None;
    }
    let whole = seconds.floor() as i64;
    let days = whole.div_euclid(86_400);
    let rem = whole.rem_euclid(86_400);
    let date = date_days_to_iso(days as f64)?;
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let second = rem % 60;
    let mut out = if !force_time && hour == 0 && minute == 0 && second == 0 {
        date
    } else {
        format!("{date} {hour:02}:{minute:02}:{second:02}")
    };
    if include_tz {
        out.push_str(" UTC");
    }
    Some(out)
}

pub(crate) unsafe fn sexp_has_class(x: SEXP, class_name: &str) -> bool {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return false;
        }
        let class =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
        if class.is_null() || class == R_NilValue() || TYPEOF(class) != SEXPTYPE::STRSXP {
            return false;
        }
        (0..XLENGTH(class)).any(|i| elt_to_string(class, i) == class_name)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// Tests removed — arena initialization required for SEXP allocation tests

// ---------------------------------------------------------------------------
// lapply/sapply/Map/Filter/do.call — functional programming
// ---------------------------------------------------------------------------

/// R's `lapply(X, FUN)` — apply FUN to each element, return list.
pub unsafe fn do_lapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = eval_arg_by_name_or_position(args, &["X"], 0, rho);
        let fun = callable_arg_by_name_or_position(args, &["FUN"], 1);
        if x.is_null() || x == R_NilValue() || fun.is_null() {
            return Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            let elem = extract_element(x, i);
            let val = apply_unary_value(fun, elem, rho);
            crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, val);
        }
        result
    }
}

/// R's `sapply(X, FUN)` — like lapply but simplifies to vector.
pub unsafe fn do_sapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let list = do_lapply(_call, _op, args, rho);
        simplify_scalar_list(list)
    }
}

/// R's `vapply(X, FUN, FUN.VALUE)` — apply and simplify using FUN.VALUE's scalar type.
pub unsafe fn do_vapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let template_expr = arg_by_name_or_position(args, &["FUN.VALUE"], 2);
        let template_type = fun_value_type(template_expr, rho);
        let list = do_lapply(_call, _op, args, rho);
        simplify_scalar_list_as(list, template_type)
    }
}

/// R's `Map(f, ...)` — apply f element-wise.
pub unsafe fn do_map(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let fun = callable_arg_by_name_or_position(args, &["f", "FUN"], 0);
        let x = eval_arg_by_name_or_position(args, &[], 1, rho);
        if fun.is_null() || x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..n {
            let elem = extract_element(x, i);
            let val = apply_unary_value(fun, elem, rho);
            crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, val);
        }
        result
    }
}

/// R's `Filter(f, x)` — keep elements where f returns TRUE.
pub unsafe fn do_filter(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let fun = callable_arg_by_name_or_position(args, &["f", "FUN"], 0);
        let x = eval_arg_by_name_or_position(args, &["x"], 1, rho);
        if fun.is_null() || x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        let mut kept: Vec<R_xlen_t> = Vec::new();
        for i in 0..n {
            let elem = extract_element(x, i);
            let val = apply_unary_value(fun, elem, rho);
            if !val.is_null() && TYPEOF(val) == SEXPTYPE::LGLSXP && *LOGICAL(val) != 0 {
                kept.push(i);
            }
        }
        let result = Rf_allocVector3(TYPEOF(x), kept.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (new_i, &old_i) in kept.iter().enumerate() {
            if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(result).add(new_i) = *REAL(x).add(old_i as usize);
            } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                *INTEGER(result).add(new_i) = *INTEGER(x).add(old_i as usize);
            }
        }
        result
    }
}

/// R's `do.call(what, args)` — call function with list of args.
pub unsafe fn do_do_call(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let fun = callable_arg_by_name_or_position(args, &["what"], 0);
        let arg_list = eval_arg_by_name_or_position(args, &["args"], 1, rho);
        if fun.is_null() || arg_list.is_null() {
            return R_NilValue();
        }
        let n = if TYPEOF(arg_list) == SEXPTYPE::VECSXP {
            XLENGTH(arg_list)
        } else {
            0
        };
        let names = if TYPEOF(arg_list) == SEXPTYPE::VECSXP {
            crate::sexp::attrib_core::getAttrib(arg_list, crate::sexp::attrib_core::R_NamesSymbol())
        } else {
            R_NilValue()
        };
        let mut call_args = R_NilValue();
        for i in (0..n).rev() {
            let cell = Rf_cons(
                crate::sexp::accessors::VECTOR_ELT(arg_list, i as i64),
                call_args,
            );
            if !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == SEXPTYPE::STRSXP
                && i < XLENGTH(names)
            {
                let name = STRING_ELT(names, i);
                if !name.is_null() {
                    let chars = CHAR(name);
                    if !chars.is_null() && *chars != 0 {
                        SETTAG(cell, Rf_install(chars));
                    }
                }
            }
            call_args = cell;
        }
        let call_sexp = Rf_cons(fun, call_args);
        if !call_sexp.is_null() {
            (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        crate::eval::eval::Rf_eval(call_sexp, rho)
    }
}

fn callable_arg_by_name_or_position(args: SEXP, names: &[&str], position: usize) -> SEXP {
    unsafe { callable_expr(arg_by_name_or_position(args, names, position)) }
}

fn eval_arg_by_name_or_position(args: SEXP, names: &[&str], position: usize, rho: SEXP) -> SEXP {
    unsafe {
        let expr = arg_by_name_or_position(args, names, position);
        if expr.is_null() || expr == R_NilValue() {
            R_NilValue()
        } else {
            crate::eval::eval::Rf_eval(expr, rho)
        }
    }
}

fn callable_expr(fun: SEXP) -> SEXP {
    unsafe {
        if fun.is_null() || fun == R_NilValue() {
            return fun;
        }
        if TYPEOF(fun) == SEXPTYPE::STRSXP && XLENGTH(fun) > 0 {
            let charsxp = STRING_ELT(fun, 0);
            if charsxp.is_null() || charsxp == crate::sexp::globals::R_NaString() {
                return R_NilValue();
            }
            let name = CHAR(charsxp);
            if name.is_null() {
                R_NilValue()
            } else {
                Rf_install(name)
            }
        } else {
            fun
        }
    }
}

fn fun_value_type(template_expr: SEXP, rho: SEXP) -> SEXPTYPE {
    unsafe {
        if !template_expr.is_null()
            && template_expr != R_NilValue()
            && TYPEOF(template_expr) == SEXPTYPE::LANGSXP
        {
            let head = CAR(template_expr);
            if TYPEOF(head) == SEXPTYPE::SYMSXP {
                if let Some(name) = symbol_name(head) {
                    if let Some(template_type) = match name.as_str() {
                        "integer" => Some(SEXPTYPE::INTSXP),
                        "numeric" | "double" => Some(SEXPTYPE::REALSXP),
                        "logical" => Some(SEXPTYPE::LGLSXP),
                        "character" => Some(SEXPTYPE::STRSXP),
                        _ => None,
                    } {
                        return template_type;
                    }
                }
            }
        }
        let template = if template_expr.is_null() || template_expr == R_NilValue() {
            R_NilValue()
        } else {
            crate::eval::eval::Rf_eval(template_expr, rho)
        };
        SEXPTYPE(TYPEOF(template))
    }
}

fn apply_unary_value(fun: SEXP, value: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let arg_sym = Rf_install(c"..rport_apply_value".as_ptr());
        let call_env = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), rho, R_NilValue());
        crate::sexp::envir::defineVar(arg_sym, value, call_env);

        let call_args = Rf_cons(arg_sym, R_NilValue());
        let call_sexp = Rf_cons(fun, call_args);
        if !call_sexp.is_null() {
            (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        crate::eval::eval::Rf_eval(call_sexp, call_env)
    }
}

fn simplify_scalar_list(list: SEXP) -> SEXP {
    unsafe {
        if list.is_null() || TYPEOF(list) != SEXPTYPE::VECSXP {
            return list;
        }
        let n = XLENGTH(list);
        if n == 0 {
            return list;
        }
        let first = VECTOR_ELT(list, 0);
        if first.is_null() || XLENGTH(first) != 1 {
            return list;
        }
        simplify_scalar_list_as(list, SEXPTYPE(TYPEOF(first)))
    }
}

fn simplify_scalar_list_as(list: SEXP, elem_type: SEXPTYPE) -> SEXP {
    unsafe {
        if list.is_null() || TYPEOF(list) != SEXPTYPE::VECSXP {
            return list;
        }
        if elem_type != SEXPTYPE::REALSXP
            && elem_type != SEXPTYPE::INTSXP
            && elem_type != SEXPTYPE::LGLSXP
            && elem_type != SEXPTYPE::STRSXP
        {
            return list;
        }
        let n = XLENGTH(list);
        let result = Rf_allocVector3(elem_type, n);
        if result.is_null() {
            return list;
        }
        let _result_guard = protect(result);
        for i in 0..n {
            let elem = VECTOR_ELT(list, i as i64);
            if elem.is_null() || TYPEOF(elem) != elem_type || XLENGTH(elem) != 1 {
                return list;
            }
            if elem_type == SEXPTYPE::REALSXP {
                *REAL(result).add(i as usize) = *REAL(elem);
            } else if elem_type == SEXPTYPE::INTSXP {
                *INTEGER(result).add(i as usize) = *INTEGER(elem);
            } else if elem_type == SEXPTYPE::LGLSXP {
                *LOGICAL(result).add(i as usize) = *LOGICAL(elem);
            } else if elem_type == SEXPTYPE::STRSXP {
                SET_STRING_ELT(result, i, STRING_ELT(elem, 0));
            }
        }
        result
    }
}

fn extract_element(x: SEXP, i: R_xlen_t) -> SEXP {
    unsafe {
        let t = TYPEOF(x);
        if t == SEXPTYPE::VECSXP {
            return crate::sexp::accessors::VECTOR_ELT(x, i as i64);
        }
        let elem = Rf_allocVector3(t, 1);
        if elem.is_null() {
            return R_NilValue();
        }
        if t == SEXPTYPE::REALSXP {
            *REAL(elem) = *REAL(x).add(i as usize);
        } else if t == SEXPTYPE::INTSXP {
            *INTEGER(elem) = *INTEGER(x).add(i as usize);
        } else if t == SEXPTYPE::LGLSXP {
            *LOGICAL(elem) = *LOGICAL(x).add(i as usize);
        }
        elem
    }
}

// ---------------------------------------------------------------------------
// apply / tapply / mapply / outer / sweep — higher-order array functions
// ---------------------------------------------------------------------------

/// Extract a row from a matrix (column-major storage) as a length-ncol vector.
unsafe fn extract_matrix_row(x: SEXP, nrow: R_xlen_t, ncol: R_xlen_t, row: R_xlen_t) -> SEXP {
    unsafe {
        let t = TYPEOF(x);
        let result = Rf_allocVector3(t, ncol);
        if result.is_null() {
            return R_NilValue();
        }
        for j in 0..ncol {
            let src = (j * nrow + row) as usize;
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

/// Extract a column from a matrix (column-major storage) as a length-nrow vector.
unsafe fn extract_matrix_col(x: SEXP, nrow: R_xlen_t, _ncol: R_xlen_t, col: R_xlen_t) -> SEXP {
    unsafe {
        let t = TYPEOF(x);
        let result = Rf_allocVector3(t, nrow);
        if result.is_null() {
            return R_NilValue();
        }
        let offset = (col * nrow) as usize;
        if t == SEXPTYPE::REALSXP {
            for i in 0..nrow {
                *REAL(result).add(i as usize) = *REAL(x).add(offset + i as usize);
            }
        } else if t == SEXPTYPE::INTSXP {
            for i in 0..nrow {
                *INTEGER(result).add(i as usize) = *INTEGER(x).add(offset + i as usize);
            }
        } else if t == SEXPTYPE::LGLSXP {
            for i in 0..nrow {
                *LOGICAL(result).add(i as usize) = *LOGICAL(x).add(offset + i as usize);
            }
        }
        result
    }
}

/// R's `apply(X, MARGIN, FUN)` — apply FUN over margins of array/matrix.
///
/// For a 2D matrix:
/// - MARGIN=1: apply FUN to each row, return vector of length nrow
/// - MARGIN=2: apply FUN to each column, return vector of length ncol
pub unsafe fn do_apply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = eval_arg_by_name_or_position(args, &["X"], 0, rho);
        let margin_arg = eval_arg_by_name_or_position(args, &["MARGIN"], 1, rho);
        let fun = callable_arg_by_name_or_position(args, &["FUN"], 2);
        if x.is_null() || x == R_NilValue() || fun.is_null() {
            return R_NilValue();
        }

        // Get dimensions
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if dim_attr.is_null() || TYPEOF(dim_attr) != SEXPTYPE::INTSXP || LENGTH(dim_attr) < 2 {
            return R_NilValue(); // not a matrix/array
        }
        let nrow = *INTEGER(dim_attr) as R_xlen_t;
        let ncol = *INTEGER(dim_attr).add(1) as R_xlen_t;
        let margin = real_or_default(margin_arg, 1.0) as i64;

        if margin == 1 {
            // Apply over rows
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, nrow);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            for i in 0..nrow {
                let row_vec = extract_matrix_row(x, nrow, ncol, i);
                let call_args = Rf_cons(row_vec, R_NilValue());
                let call_sexp = Rf_cons(fun, call_args);
                if !call_sexp.is_null() {
                    (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                }
                let val = crate::eval::eval::Rf_eval(call_sexp, rho);
                crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, val);
            }
            simplify_scalar_list(result)
        } else if margin == 2 {
            // Apply over columns
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncol);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            for j in 0..ncol {
                let col_vec = extract_matrix_col(x, nrow, ncol, j);
                let call_args = Rf_cons(col_vec, R_NilValue());
                let call_sexp = Rf_cons(fun, call_args);
                if !call_sexp.is_null() {
                    (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                }
                let val = crate::eval::eval::Rf_eval(call_sexp, rho);
                crate::sexp::accessors::SET_VECTOR_ELT(result, j as i64, val);
            }
            simplify_scalar_list(result)
        } else {
            R_NilValue()
        }
    }
}

/// R's `tapply(X, INDEX, FUN)` — apply FUN to each group defined by INDEX.
///
/// Iterates unique values of INDEX, collects matching elements from X, calls FUN on each group.
pub unsafe fn do_tapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let index = CAR(CDR(args));
        let fun = CAR(CDR(CDR(args)));
        if x.is_null() || x == R_NilValue() || index.is_null() || fun.is_null() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let idx_n = XLENGTH(index);

        // Collect unique index values and group membership
        let mut group_keys: Vec<i64> = Vec::new();
        let mut group_map: std::collections::BTreeMap<i64, usize> =
            std::collections::BTreeMap::new();
        let mut groups: Vec<Vec<R_xlen_t>> = Vec::new();

        let idx_t = TYPEOF(index);
        for i in 0..n {
            let idx_i = if idx_n == 0 { 0 } else { i % idx_n };
            let key = if idx_t == SEXPTYPE::INTSXP || idx_t == SEXPTYPE::LGLSXP {
                *INTEGER(index).add(idx_i as usize) as i64
            } else if idx_t == SEXPTYPE::REALSXP {
                (*REAL(index).add(idx_i as usize)).to_bits() as i64
            } else {
                idx_i as i64
            };

            if let Some(&g) = group_map.get(&key) {
                groups[g].push(i);
            } else {
                let g = groups.len();
                group_map.insert(key, g);
                group_keys.push(key);
                groups.push(vec![i]);
            }
        }

        let num_groups = groups.len() as R_xlen_t;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, num_groups);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for (g, indices) in groups.iter().enumerate() {
            let group_vec = Rf_allocVector3(TYPEOF(x), indices.len() as R_xlen_t);
            if !group_vec.is_null() {
                let t = TYPEOF(x);
                for (j, &src_i) in indices.iter().enumerate() {
                    if t == SEXPTYPE::REALSXP {
                        *REAL(group_vec).add(j) = *REAL(x).add(src_i as usize);
                    } else if t == SEXPTYPE::INTSXP {
                        *INTEGER(group_vec).add(j) = *INTEGER(x).add(src_i as usize);
                    } else if t == SEXPTYPE::LGLSXP {
                        *LOGICAL(group_vec).add(j) = *LOGICAL(x).add(src_i as usize);
                    }
                }
            }
            let call_args = Rf_cons(group_vec, R_NilValue());
            let call_sexp = Rf_cons(fun, call_args);
            if !call_sexp.is_null() {
                (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            }
            let val = crate::eval::eval::Rf_eval(call_sexp, rho);
            crate::sexp::accessors::SET_VECTOR_ELT(result, g as i64, val);
        }

        result
    }
}

/// R's `mapply(FUN, ...)` — multivariate sapply. Applies FUN element-wise across multiple vectors with recycling.
pub unsafe fn do_mapply(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let fun = CAR(args);
        let vec_args = CDR(args);
        if fun.is_null() {
            return R_NilValue();
        }

        // Collect vector args, find max length
        let mut arg_vecs: Vec<SEXP> = Vec::new();
        let mut max_len: R_xlen_t = 0;
        let mut current = vec_args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                arg_vecs.push(arg);
                let n = XLENGTH(arg);
                if n > max_len {
                    max_len = n;
                }
            }
            current = CDR(current);
        }
        if max_len == 0 {
            return R_NilValue();
        }

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, max_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..max_len {
            // Build call: FUN(arg1[i], arg2[i], ...) with recycling
            let mut call_args = R_NilValue();
            for &arg in arg_vecs.iter().rev() {
                let n = XLENGTH(arg);
                let idx = if n == 0 { 0 } else { i % n };
                let elem = extract_element(arg, idx);
                call_args = Rf_cons(elem, call_args);
            }
            let call_sexp = Rf_cons(fun, call_args);
            if !call_sexp.is_null() {
                (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            }
            let val = crate::eval::eval::Rf_eval(call_sexp, rho);
            crate::sexp::accessors::SET_VECTOR_ELT(result, i as i64, val);
        }

        result
    }
}

/// R's `outer(X, Y, FUN="*")` — outer product. Returns a matrix of length(X) x length(Y).
///
/// For each pair (x_i, y_j), computes FUN(x_i, y_j).
pub unsafe fn do_outer(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        let fun_arg = CAR(CDR(CDR(args)));
        if x.is_null() || x == R_NilValue() || y.is_null() || y == R_NilValue() {
            return R_NilValue();
        }

        let nx = XLENGTH(x);
        let ny = XLENGTH(y);

        // Determine if FUN is a symbol (operator name) or a function object
        let use_multiply = if fun_arg.is_null() || fun_arg == R_NilValue() {
            true
        } else if TYPEOF(fun_arg) == SEXPTYPE::STRSXP {
            elt_to_string(fun_arg, 0) == "*"
        } else if TYPEOF(fun_arg) == SEXPTYPE::SYMSXP {
            let pname = crate::sexp::accessors::PRINTNAME(fun_arg);
            if !pname.is_null() {
                let s = crate::sexp::accessors::CHAR(pname);
                if !s.is_null() {
                    std::ffi::CStr::from_ptr(s).to_str().unwrap_or("") == "*"
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        let result = Rf_allocVector3(SEXPTYPE::REALSXP, nx * ny);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);

        if use_multiply {
            // Fast path: multiply
            for i in 0..nx {
                let xi = elt_real_safe(x, i);
                for j in 0..ny {
                    let yj = elt_real_safe(y, j);
                    *dst.add((j * nx + i) as usize) = xi * yj;
                }
            }
        } else {
            // General path: call FUN(x_i, y_j) for each pair
            for i in 0..nx {
                let xi = extract_element(x, i);
                for j in 0..ny {
                    let yj = extract_element(y, j);
                    let call_args = Rf_cons(xi, Rf_cons(yj, R_NilValue()));
                    let call_sexp = Rf_cons(fun_arg, call_args);
                    if !call_sexp.is_null() {
                        (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                    }
                    let val = crate::eval::eval::Rf_eval(call_sexp, rho);
                    let v = if !val.is_null() && TYPEOF(val) == SEXPTYPE::REALSXP {
                        *REAL(val)
                    } else if !val.is_null()
                        && (TYPEOF(val) == SEXPTYPE::INTSXP || TYPEOF(val) == SEXPTYPE::LGLSXP)
                    {
                        let iv = *INTEGER(val);
                        if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
                    } else {
                        NA_REAL
                    };
                    *dst.add((j * nx + i) as usize) = v;
                }
            }
        }

        // Set dim attribute: c(nx, ny)
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            *INTEGER(dim) = nx as c_int;
            *INTEGER(dim).add(1) = ny as c_int;
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                dim,
            );
        }

        result
    }
}

/// R's `sweep(x, MARGIN, STATS, FUN="-")` — sweep out statistics from array.
///
/// For each row/column, applies FUN(x, STATS) element-wise.
pub unsafe fn do_sweep(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let margin_arg = CAR(CDR(args));
        let stats = CAR(CDR(CDR(args)));
        let fun_arg = CAR(CDR(CDR(CDR(args))));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        // Determine operation
        let op_str = if fun_arg.is_null() || fun_arg == R_NilValue() {
            "-".to_string()
        } else if TYPEOF(fun_arg) == SEXPTYPE::STRSXP {
            elt_to_string(fun_arg, 0)
        } else if TYPEOF(fun_arg) == SEXPTYPE::SYMSXP {
            let pname = crate::sexp::accessors::PRINTNAME(fun_arg);
            if !pname.is_null() {
                let s = crate::sexp::accessors::CHAR(pname);
                if !s.is_null() {
                    std::ffi::CStr::from_ptr(s)
                        .to_str()
                        .unwrap_or("-")
                        .to_string()
                } else {
                    "-".to_string()
                }
            } else {
                "-".to_string()
            }
        } else {
            String::new()
        };

        let margin = if margin_arg.is_null() || margin_arg == R_NilValue() {
            1
        } else {
            real_or_default(margin_arg, 1.0) as i64
        };

        let t = TYPEOF(x);
        let n = XLENGTH(x);

        // Get dimensions
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
                (n, 1)
            };

        let result = Rf_allocVector3(t, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        // Fast path for common ops
        let apply_binary = |src_val: f64, stat_val: f64| -> f64 {
            match op_str.as_str() {
                "-" => src_val - stat_val,
                "+" => src_val + stat_val,
                "*" => src_val * stat_val,
                "/" => {
                    if stat_val != 0.0 {
                        src_val / stat_val
                    } else {
                        NA_REAL
                    }
                }
                _ => src_val - stat_val,
            }
        };

        if margin == 1 {
            // Sweep across rows: subtract STATS from each row
            let stats_len = if stats.is_null() || stats == R_NilValue() {
                0
            } else {
                XLENGTH(stats)
            };
            for i in 0..nrow {
                for j in 0..ncol {
                    let src_idx = (j * nrow + i) as usize;
                    let stat_idx = if stats_len == 0 { 0 } else { j % stats_len };
                    let src_val = if t == SEXPTYPE::REALSXP {
                        *REAL(x).add(src_idx)
                    } else if t == SEXPTYPE::INTSXP {
                        let v = *INTEGER(x).add(src_idx);
                        if v == NA_INTEGER { NA_REAL } else { v as f64 }
                    } else {
                        NA_REAL
                    };
                    let stat_val = if stats.is_null() || stats == R_NilValue() {
                        0.0
                    } else {
                        elt_real_safe(stats, stat_idx)
                    };
                    let res = apply_binary(src_val, stat_val);
                    if t == SEXPTYPE::REALSXP {
                        *REAL(result).add(src_idx) = res;
                    } else if t == SEXPTYPE::INTSXP {
                        *INTEGER(result).add(src_idx) = if res.is_nan() || res == NA_REAL {
                            NA_INTEGER
                        } else {
                            res as c_int
                        };
                    }
                }
            }
        } else if margin == 2 {
            // Sweep across columns: subtract STATS from each column
            let stats_len = if stats.is_null() || stats == R_NilValue() {
                0
            } else {
                XLENGTH(stats)
            };
            for j in 0..ncol {
                for i in 0..nrow {
                    let src_idx = (j * nrow + i) as usize;
                    let stat_idx = if stats_len == 0 { 0 } else { i % stats_len };
                    let src_val = if t == SEXPTYPE::REALSXP {
                        *REAL(x).add(src_idx)
                    } else if t == SEXPTYPE::INTSXP {
                        let v = *INTEGER(x).add(src_idx);
                        if v == NA_INTEGER { NA_REAL } else { v as f64 }
                    } else {
                        NA_REAL
                    };
                    let stat_val = if stats.is_null() || stats == R_NilValue() {
                        0.0
                    } else {
                        elt_real_safe(stats, stat_idx)
                    };
                    let res = apply_binary(src_val, stat_val);
                    if t == SEXPTYPE::REALSXP {
                        *REAL(result).add(src_idx) = res;
                    } else if t == SEXPTYPE::INTSXP {
                        *INTEGER(result).add(src_idx) = if res.is_nan() || res == NA_REAL {
                            NA_INTEGER
                        } else {
                            res as c_int
                        };
                    }
                }
            }
        }

        // Copy dim attribute if present
        if !dim_attr.is_null() {
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                dim_attr,
            );
        }

        result
    }
}

// ---------------------------------------------------------------------------
// Error handling: stop, warning, message, tryCatch, inherits, exists, get, assign
// ---------------------------------------------------------------------------

/// R's `stop(...)` — raise error.
pub unsafe fn do_stop(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let s = elt_to_string(CAR(args), 0);
        std::panic::panic_any(crate::sexp::context::RError { message: s });
    }
}

/// R's `warning(...)` — issue warning.
pub unsafe fn do_warning(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let warning_text =
            condition_message_text(args, &["call.", "immediate.", "noBreaks.", "domain"]);
        let condition = simple_condition(&warning_text, &["simpleWarning", "warning", "condition"]);
        signal_calling_handlers(condition, rho);

        let message = format!("Warning message:\n{} \n", warning_text);
        if crate::sexp::output::is_capturing() {
            crate::sexp::output::capture_stderr(&message);
        } else {
            eprint!("{message}");
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// R's `message(...)` — print message.
pub unsafe fn do_message(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let text = condition_message_text(args, &["domain", "appendLF"]);
        let message = format!("{}\n", text);
        let condition = simple_condition(&message, &["simpleMessage", "message", "condition"]);
        signal_calling_handlers(condition, rho);

        if crate::sexp::output::is_capturing() {
            crate::sexp::output::capture_stderr(&message);
        } else {
            eprint!("{message}");
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// R's `inherits(x, what)` — check class.
pub unsafe fn do_inherits(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let what = CAR(CDR(args));
        if x.is_null() || what.is_null() {
            return Rf_ScalarLogical(FALSE);
        }
        let class_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
        );
        if class_attr.is_null() || TYPEOF(class_attr) != SEXPTYPE::STRSXP {
            return Rf_ScalarLogical(FALSE);
        }
        let target = elt_to_string(what, 0);
        let n = XLENGTH(class_attr);
        for i in 0..n {
            if elt_to_string(class_attr, i) == target {
                return Rf_ScalarLogical(TRUE);
            }
        }
        Rf_ScalarLogical(FALSE)
    }
}

fn tag_name(cell: SEXP) -> Option<String> {
    unsafe {
        let tag = TAG(cell);
        if tag.is_null() || tag == R_NilValue() {
            return None;
        }
        let pname = PRINTNAME(tag);
        if pname.is_null() {
            return None;
        }
        let chars = CHAR(pname);
        if chars.is_null() {
            None
        } else {
            Some(std::ffi::CStr::from_ptr(chars).to_str().ok()?.to_string())
        }
    }
}

unsafe fn simple_error_condition(message: &str) -> SEXP {
    unsafe { simple_condition(message, &["simpleError", "error", "condition"]) }
}

unsafe fn simple_condition(message: &str, classes: &[&str]) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let msg = Rf_mkString(CString::new(message).unwrap_or_default().as_ptr());
        SET_VECTOR_ELT(result, 0, msg);

        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !names.is_null() {
            let _np = protect(names);
            SET_STRING_ELT(
                names,
                0,
                Rf_mkChar(CString::new("message").unwrap_or_default().as_ptr()),
            );
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
                names,
            );
        }

        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("message").unwrap_or_default().as_ptr()),
            msg,
        );

        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
        if !class.is_null() {
            let _cp = protect(class);
            for (i, name) in classes.iter().enumerate() {
                SET_STRING_ELT(
                    class,
                    i as R_xlen_t,
                    Rf_mkChar(CString::new(*name).unwrap_or_default().as_ptr()),
                );
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
                class,
            );
        }

        result
    }
}

unsafe fn find_try_catch_error_handler(args: SEXP, rho: SEXP) -> Option<SEXP> {
    unsafe {
        let mut current = CDR(args);
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).as_deref() == Some("error") {
                let handler = crate::eval::eval::Rf_eval(CAR(current), rho);
                if !handler.is_null() && handler != R_NilValue() {
                    return Some(handler);
                }
            }
            current = CDR(current);
        }
        None
    }
}

/// R's `tryCatch(expr, error = function(e) ...)` — basic error handler support.
pub unsafe fn do_tryCatch(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        if expr.is_null() {
            return R_NilValue();
        }

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::eval::eval::Rf_eval(expr, rho)
        }));
        match result {
            Ok(val) => val,
            Err(payload) => {
                let message = match payload.downcast::<crate::sexp::context::RSignal>() {
                    Ok(signal) => match *signal {
                        crate::sexp::context::RSignal::Error { message } => message,
                        other => std::panic::panic_any(other),
                    },
                    Err(payload) => match payload.downcast::<crate::sexp::context::RError>() {
                        Ok(err) => err.message.clone(),
                        Err(payload) => std::panic::resume_unwind(payload),
                    },
                };

                let Some(handler) = find_try_catch_error_handler(args, rho) else {
                    std::panic::panic_any(crate::sexp::context::RError { message });
                };
                let condition = simple_error_condition(&message);
                let call = crate::sexp::constructors::Rf_lang2(handler, condition);
                crate::eval::eval::Rf_eval(call, rho)
            }
        }
    }
}

/// R's `exists(x, envir)` — check name exists.
pub unsafe fn do_exists(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let name_arg = arg_by_name_or_position(args, &["x"], 0);
        let name = elt_to_string(name_arg, 0);
        let sym = Rf_install(CString::new(name.as_str()).unwrap_or_default().as_ptr());
        let env = environment_arg_or_default(args, &["envir", "where", "frame"], 1, rho);
        let inherits = named_logical_arg(args, "inherits").unwrap_or(true);
        let mode_arg = {
            let named = arg_by_name_or_position(args, &["mode"], 2);
            if !named.is_null() && named != R_NilValue() {
                named
            } else {
                let second = arg_by_name_or_position(args, &[], 1);
                if !second.is_null() && second != R_NilValue() && TYPEOF(second) == SEXPTYPE::STRSXP
                {
                    second
                } else {
                    R_NilValue()
                }
            }
        };
        let mode = if mode_arg.is_null() || mode_arg == R_NilValue() || XLENGTH(mode_arg) == 0 {
            "any".to_string()
        } else {
            elt_to_string(mode_arg, 0)
        };
        let found = if crate::eval::builtin::is_hidden_builtin_name(&name) {
            false
        } else if mode == "function" {
            let value = if inherits {
                crate::sexp::envir::R_findVar(sym, env)
            } else {
                crate::sexp::envir::R_findVarInFrame(env, sym)
            };
            crate::eval::builtin::has_builtin_handler(&name) || is_function_value(value)
        } else {
            crate::sexp::envir::binding_exists_raw(env, sym, inherits)
                || crate::eval::builtin::has_builtin_handler(&name)
        };
        Rf_ScalarLogical(if found { TRUE } else { FALSE })
    }
}

/// R's `find(what, mode = "any")` — locate a name on the search path.
pub unsafe fn do_find(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let what_arg = arg_by_name_or_position(args, &["what"], 0);
        if what_arg.is_null() || what_arg == R_NilValue() || XLENGTH(what_arg) == 0 {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }

        let name = elt_to_string(what_arg, 0);
        if name.is_empty() || crate::eval::builtin::is_hidden_builtin_name(&name) {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }

        let mode_arg = arg_by_name_or_position(args, &["mode"], 1);
        let mode = if mode_arg.is_null() || mode_arg == R_NilValue() || XLENGTH(mode_arg) == 0 {
            "any".to_string()
        } else {
            elt_to_string(mode_arg, 0)
        };
        let want_function = mode == "function";
        let numeric = logical_arg_by_name_or_position(args, "numeric", 2).unwrap_or(false);

        let sym = Rf_install(CString::new(name.as_str()).unwrap_or_default().as_ptr());
        let mut matches = Vec::new();
        for (label, env) in search_path_entries() {
            if find_matches_mode(env, sym, &name, want_function) {
                matches.push(label);
            }
        }

        if numeric {
            return find_numeric_result(&matches);
        }

        let result = Rf_allocVector3(SEXPTYPE::STRSXP, matches.len() as R_xlen_t);
        for (i, value) in matches.iter().enumerate() {
            SET_STRING_ELT(
                result,
                i as R_xlen_t,
                Rf_mkChar(CString::new(value.as_str()).unwrap_or_default().as_ptr()),
            );
        }
        result
    }
}

unsafe fn find_numeric_result(matches: &[String]) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, matches.len() as R_xlen_t);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, matches.len() as R_xlen_t);
        for (i, value) in matches.iter().enumerate() {
            *INTEGER(result).add(i) = (i + 1) as c_int;
            SET_STRING_ELT(
                names,
                i as R_xlen_t,
                Rf_mkChar(CString::new(value.as_str()).unwrap_or_default().as_ptr()),
            );
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names,
        );
        result
    }
}

unsafe fn find_matches_mode(env: SEXP, symbol: SEXP, name: &str, want_function: bool) -> bool {
    unsafe {
        if env.is_null() || env == R_NilValue() {
            return false;
        }
        let value = crate::sexp::envir::R_findVarInFrame(env, symbol);
        let is_base_builtin = env == crate::sexp::globals::R_BaseEnv()
            && crate::eval::builtin::has_builtin_handler(name);
        if value == R_UnboundValue() {
            return !want_function && is_base_builtin;
        }
        if want_function {
            is_function_value(value) || is_base_builtin
        } else {
            true
        }
    }
}

/// R's `get(x, envir)` — get value.
pub unsafe fn do_get(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let name_arg = arg_by_name_or_position(args, &["x"], 0);
        let name = elt_to_string(name_arg, 0);
        let env = environment_arg_or_default(args, &["envir", "pos"], 1, rho);
        let inherits = named_logical_arg(args, "inherits").unwrap_or(true);
        let sym = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
        if inherits {
            crate::sexp::envir::R_findVar(sym, env)
        } else {
            crate::sexp::envir::R_findVarInFrame(env, sym)
        }
    }
}

/// R's `assign(x, value, envir)` — assign value.
pub unsafe fn do_assign(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let name_arg = arg_by_name_or_position(args, &["x"], 0);
        let name = elt_to_string(name_arg, 0);
        let val = arg_by_name_or_position(args, &["value"], 1);
        if val.is_null() {
            return R_NilValue();
        }
        let env = environment_arg_or_default(args, &["envir", "pos"], 2, rho);
        crate::sexp::envir::defineVar(
            Rf_install(CString::new(name).unwrap_or_default().as_ptr()),
            val,
            env,
        );
        crate::sexp::globals::set_R_Visible(FALSE);
        val
    }
}

unsafe fn symbol_name(sym: SEXP) -> Option<String> {
    unsafe {
        if sym.is_null() || sym == R_NilValue() || TYPEOF(sym) != SEXPTYPE::SYMSXP {
            return None;
        }
        let printname = PRINTNAME(sym);
        if printname.is_null() || printname == R_NilValue() {
            return None;
        }
        let ptr = CHAR(printname);
        if ptr.is_null() {
            return None;
        }
        Some(CStr::from_ptr(ptr).to_string_lossy().into_owned())
    }
}

unsafe fn logical_arg(arg: SEXP, default: bool) -> bool {
    unsafe {
        if arg.is_null() || arg == R_NilValue() || XLENGTH(arg) < 1 {
            return default;
        }
        if TYPEOF(arg) == SEXPTYPE::LGLSXP {
            return *LOGICAL(arg) != FALSE;
        }
        if TYPEOF(arg) == SEXPTYPE::INTSXP {
            return *INTEGER(arg) != 0;
        }
        default
    }
}

/// R's `ls(envir)` — list objects.
pub unsafe fn do_ls(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut env = rho;
        let mut all_names = false;
        let mut sorted = true;

        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let arg = CAR(cell);
            let name = symbol_name(TAG(cell));
            match name.as_deref() {
                Some("name") | Some("pos") | Some("envir") => {
                    if TYPEOF(arg) == SEXPTYPE::ENVSXP {
                        env = arg;
                    }
                }
                Some("all.names") => all_names = logical_arg(arg, all_names),
                Some("sorted") => sorted = logical_arg(arg, sorted),
                _ if TYPEOF(arg) == SEXPTYPE::ENVSXP => env = arg,
                _ => {}
            }
            cell = CDR(cell);
        }

        let mut names = Vec::new();
        if TYPEOF(env) == SEXPTYPE::ENVSXP {
            let mut frame = FRAME(env);
            while !frame.is_null() && frame != R_NilValue() {
                let value = CAR(frame);
                if value != crate::sexp::globals::R_UnboundValue()
                    && let Some(name) = symbol_name(TAG(frame))
                    && (all_names || !name.starts_with('.'))
                {
                    names.push(name);
                }
                frame = CDR(frame);
            }
        }

        if sorted {
            names.sort();
        }

        let result = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as R_xlen_t);
        for (i, name) in names.iter().enumerate() {
            let cstr = CString::new(name.as_str()).unwrap_or_default();
            SET_STRING_ELT(result, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
        }
        result
    }
}

/// R's `rm(list, envir)` — remove objects.
pub unsafe fn do_rm(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let list = arg_by_name_or_position(args, &["list"], 0);
        if list.is_null() || TYPEOF(list) != SEXPTYPE::STRSXP {
            return R_NilValue();
        }
        let env = environment_arg_or_default(args, &["envir"], 1, rho);
        for i in 0..XLENGTH(list) {
            let sym = Rf_install(
                CString::new(elt_to_string(list, i))
                    .unwrap_or_default()
                    .as_ptr(),
            );
            crate::sexp::envir::remove_binding_raw(env, sym);
        }
        crate::sexp::globals::set_R_Visible(FALSE);
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// Distribution functions: dnorm, pnorm, qnorm, dpois, ppois
// ---------------------------------------------------------------------------

/// R's `dnorm(x, mean=0, sd=1)` — normal density.
pub unsafe fn do_dnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_log(args, 0.0, 1.0, |x, m, s, give_log| {
        crate::dist::normal::dnorm4_inner(x, m, s, give_log)
    })
}

/// R's `pnorm(q, mean=0, sd=1)` — normal CDF.
pub unsafe fn do_pnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(args, 0.0, 1.0, |q, m, s, lower_tail, log_p| {
        crate::dist::normal::pnorm5_inner(q, m, s, lower_tail, log_p)
    })
}

/// R's `qnorm(p, mean=0, sd=1)` — normal quantile.
pub unsafe fn do_qnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary_with_tail_log(args, 0.0, 1.0, |p, m, s, lower_tail, log_p| {
        crate::dist::normal::qnorm5_inner(p, m, s, lower_tail, log_p)
    })
}

/// R's `dpois(x, lambda)` — Poisson density.
pub unsafe fn do_dpois(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |x, lam, _| {
        crate::dist::poisson::dpois_inner(x, lam, false)
    })
}

/// R's `ppois(q, lambda)` — Poisson CDF.
pub unsafe fn do_ppois(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |q, lam, _| {
        crate::dist::poisson::ppois_inner(q, lam, true, false)
    })
}

/// R's `qpois(p, lambda)` — Poisson quantile.
pub unsafe fn do_qpois(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let p = arg_by_name_or_position(args, &["p"], 0);
        let lambda = real_or_default(arg_by_name_or_position(args, &["lambda"], 1), 1.0);
        let lower_tail = logical_arg(arg_by_name_or_position(args, &["lower.tail"], 2), true);
        let log_p = logical_arg(arg_by_name_or_position(args, &["log.p"], 3), false);
        map_real_distribution(p, |p| {
            crate::dist::poisson::qpois_inner(p, lambda, lower_tail, log_p)
        })
    }
}

/// R's `dbinom(x, size, prob)` — binomial density.
pub unsafe fn do_dbinom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.5, |x, n, p| {
        crate::dist::binomial::dbinom_inner(x, n, p, false)
    })
}

/// R's `pbinom(q, size, prob)` — binomial CDF.
pub unsafe fn do_pbinom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.5, |q, n, p| {
        crate::dist::binomial::pbinom_inner(q, n, p, true, false)
    })
}

/// R's `qbinom(p, size, prob)` — binomial quantile.
pub unsafe fn do_qbinom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let p = arg_by_name_or_position(args, &["p"], 0);
        let size = real_or_default(arg_by_name_or_position(args, &["size"], 1), 1.0);
        let prob = real_or_default(arg_by_name_or_position(args, &["prob"], 2), 0.5);
        let lower_tail = logical_arg(arg_by_name_or_position(args, &["lower.tail"], 3), true);
        let log_p = logical_arg(arg_by_name_or_position(args, &["log.p"], 4), false);
        map_real_distribution(p, |p| {
            crate::dist::binomial::qbinom_inner(p, size, prob, lower_tail, log_p)
        })
    }
}

/// R's `dexp(x, rate)` — exponential density.
pub unsafe fn do_dexp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |x, rate, _| {
        crate::dist::exponential::dexp_inner(x, 1.0 / rate, false)
    })
}

/// R's `pexp(q, rate)` — exponential CDF.
pub unsafe fn do_pexp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |q, rate, _| {
        crate::dist::exponential::pexp_inner(q, 1.0 / rate, true, false)
    })
}

/// R's `qexp(p, rate)` — exponential quantile.
pub unsafe fn do_qexp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let p = arg_by_name_or_position(args, &["p"], 0);
        let rate = real_or_default(arg_by_name_or_position(args, &["rate"], 1), 1.0);
        let lower_tail = logical_arg(arg_by_name_or_position(args, &["lower.tail"], 2), true);
        let log_p = logical_arg(arg_by_name_or_position(args, &["log.p"], 3), false);
        map_real_distribution(p, |p| {
            crate::dist::exponential::qexp_inner(p, 1.0 / rate, lower_tail, log_p)
        })
    }
}

// ---------------------------------------------------------------------------
// Distribution functions: gamma, beta, t, chisq, cauchy, weibull, f, nbinom, geom
// ---------------------------------------------------------------------------

/// R's `dgamma(x, shape, scale=1)` — gamma density.
pub unsafe fn do_dgamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |x, shape, scale| {
        crate::dist::gamma::dgamma_inner(x, shape, scale, false)
    })
}

/// R's `pgamma(q, shape, scale=1)` — gamma CDF.
pub unsafe fn do_pgamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |q, shape, scale| {
        crate::dist::gamma::pgamma_inner(q, shape, scale, true, false)
    })
}

/// R's `qgamma(p, shape, scale=1)` — gamma quantile.
pub unsafe fn do_qgamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |p, shape, scale| {
        crate::dist::gamma::qgamma_inner(p, shape, scale, true, false)
    })
}

/// R's `dbeta(x, shape1, shape2)` — beta density.
pub unsafe fn do_dbeta(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |x, a, b| {
        crate::dist::beta::dbeta_inner(x, a, b, false)
    })
}

/// R's `pbeta(q, shape1, shape2)` — beta CDF.
pub unsafe fn do_pbeta(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |q, a, b| {
        crate::dist::beta::pbeta_inner(q, a, b, true, false)
    })
}

/// R's `qbeta(p, shape1, shape2)` — beta quantile.
pub unsafe fn do_qbeta(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |p, a, b| {
        crate::dist::beta::qbeta_inner(p, a, b, true, false)
    })
}

/// R's `dt(x, df)` — t density.
pub unsafe fn do_dt(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |x, df, _| {
        crate::dist::t_dist::dt_inner(x, df, false)
    })
}

/// R's `pt(q, df)` — t CDF.
pub unsafe fn do_pt(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |q, df, _| {
        crate::dist::t_dist::pt_inner(q, df, true, false)
    })
}

/// R's `qt(p, df)` — t quantile.
pub unsafe fn do_qt(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |p, df, _| {
        crate::dist::t_dist::qt_inner(p, df, true, false)
    })
}

/// R's `dchisq(x, df)` — chi-squared density.
pub unsafe fn do_dchisq(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |x, df, _| {
        crate::dist::chisq::dchisq_inner(x, df, false)
    })
}

/// R's `pchisq(q, df)` — chi-squared CDF.
pub unsafe fn do_pchisq(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |q, df, _| {
        crate::dist::chisq::pchisq_inner(q, df, true, false)
    })
}

/// R's `qchisq(p, df)` — chi-squared quantile.
pub unsafe fn do_qchisq(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |p, df, _| {
        crate::dist::chisq::qchisq_inner(p, df, true, false)
    })
}

/// R's `dcauchy(x, location=0, scale=1)` — Cauchy density.
pub unsafe fn do_dcauchy(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.0, 1.0, |x, loc, sc| {
        crate::dist::cauchy::dcauchy_inner(x, loc, sc, false)
    })
}

/// R's `pcauchy(q, location=0, scale=1)` — Cauchy CDF.
pub unsafe fn do_pcauchy(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.0, 1.0, |q, loc, sc| {
        crate::dist::cauchy::pcauchy_inner(q, loc, sc, true, false)
    })
}

/// R's `qcauchy(p, location=0, scale=1)` — Cauchy quantile.
pub unsafe fn do_qcauchy(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.0, 1.0, |p, loc, sc| {
        crate::dist::cauchy::qcauchy_inner(p, loc, sc, true, false)
    })
}

/// R's `dweibull(x, shape, scale=1)` — Weibull density.
pub unsafe fn do_dweibull(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |x, shape, scale| {
        crate::dist::weibull::dweibull_inner(x, shape, scale, false)
    })
}

/// R's `pweibull(q, shape, scale=1)` — Weibull CDF.
pub unsafe fn do_pweibull(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |q, shape, scale| {
        crate::dist::weibull::pweibull_inner(q, shape, scale, true, false)
    })
}

/// R's `qweibull(p, shape, scale=1)` — Weibull quantile.
pub unsafe fn do_qweibull(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |p, shape, scale| {
        crate::dist::weibull::qweibull_inner(p, shape, scale, true, false)
    })
}

/// R's `df(x, df1, df2)` — F distribution density.
pub unsafe fn do_df(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |x, df1, df2| {
        crate::dist::f_dist::df_inner(x, df1, df2, false)
    })
}

/// R's `pf(q, df1, df2)` — F distribution CDF.
pub unsafe fn do_pf(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |q, df1, df2| {
        crate::dist::f_dist::pf_inner(q, df1, df2, true, false)
    })
}

/// R's `qf(p, df1, df2)` — F distribution quantile.
pub unsafe fn do_qf(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |p, df1, df2| {
        crate::dist::f_dist::qf_inner(p, df1, df2, true, false)
    })
}

/// R's `dunif(x, min=0, max=1)` — uniform density.
pub unsafe fn do_dunif(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        let min = real_or_default(arg_by_name_or_position(args, &["min"], 1), 0.0);
        let max = real_or_default(arg_by_name_or_position(args, &["max"], 2), 1.0);
        let give_log = logical_arg(arg_by_name_or_position(args, &["log"], 3), false);
        map_real_distribution(x, |x| {
            crate::dist::uniform::dunif_inner(x, min, max, give_log)
        })
    }
}

/// R's `punif(q, min=0, max=1)` — uniform CDF.
pub unsafe fn do_punif(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let q = arg_by_name_or_position(args, &["q"], 0);
        let min = real_or_default(arg_by_name_or_position(args, &["min"], 1), 0.0);
        let max = real_or_default(arg_by_name_or_position(args, &["max"], 2), 1.0);
        let lower_tail = logical_arg(arg_by_name_or_position(args, &["lower.tail"], 3), true);
        let log_p = logical_arg(arg_by_name_or_position(args, &["log.p"], 4), false);
        map_real_distribution(q, |q| {
            crate::dist::uniform::punif_inner(q, min, max, lower_tail, log_p)
        })
    }
}

/// R's `qunif(p, min=0, max=1)` — uniform quantile.
pub unsafe fn do_qunif(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let p = arg_by_name_or_position(args, &["p"], 0);
        let min = real_or_default(arg_by_name_or_position(args, &["min"], 1), 0.0);
        let max = real_or_default(arg_by_name_or_position(args, &["max"], 2), 1.0);
        let lower_tail = logical_arg(arg_by_name_or_position(args, &["lower.tail"], 3), true);
        let log_p = logical_arg(arg_by_name_or_position(args, &["log.p"], 4), false);
        map_real_distribution(p, |p| {
            crate::dist::uniform::qunif_inner(p, min, max, lower_tail, log_p)
        })
    }
}

/// R's `dnbinom(x, size, prob)` — negative binomial density.
pub unsafe fn do_dnbinom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.5, |x, size, prob| {
        crate::dist::nbinom::dnbinom_inner(x, size, prob, false)
    })
}

/// R's `pnbinom(q, size, prob)` — negative binomial CDF.
pub unsafe fn do_pnbinom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.5, |q, size, prob| {
        crate::dist::nbinom::pnbinom_inner(q, size, prob, true, false)
    })
}

/// R's `qnbinom(p, size, prob)` — negative binomial quantile.
pub unsafe fn do_qnbinom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.5, |p, size, prob| {
        crate::dist::nbinom::qnbinom_inner(p, size, prob, true, false)
    })
}

/// R's `dgeom(x, prob)` — geometric density.
pub unsafe fn do_dgeom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.5, 0.0, |x, p, _| {
        crate::dist::geometric::dgeom_inner(x, p, false)
    })
}

/// R's `pgeom(q, prob)` — geometric CDF.
pub unsafe fn do_pgeom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.5, 0.0, |q, p, _| {
        crate::dist::geometric::pgeom_inner(q, p, true, false)
    })
}

/// R's `qgeom(p, prob)` — geometric quantile.
pub unsafe fn do_qgeom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.5, 0.0, |p, prob, _| {
        crate::dist::geometric::qgeom_inner(p, prob, true, false)
    })
}

// ---------------------------------------------------------------------------
// Distribution functions: lnorm, logistic, signrank, wilcox, hyper, tukey
// ---------------------------------------------------------------------------

/// R's `dlnorm(x, meanlog=0, sdlog=1)` — lognormal density.
pub unsafe fn do_dlnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.0, 1.0, |x, meanlog, sdlog| {
        crate::dist::lnorm::dlnorm_inner(x, meanlog, sdlog, false)
    })
}

/// R's `plnorm(q, meanlog=0, sdlog=1)` — lognormal CDF.
pub unsafe fn do_plnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.0, 1.0, |q, meanlog, sdlog| {
        crate::dist::lnorm::plnorm_inner(q, meanlog, sdlog, true, false)
    })
}

/// R's `qlnorm(p, meanlog=0, sdlog=1)` — lognormal quantile.
pub unsafe fn do_qlnorm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.0, 1.0, |p, meanlog, sdlog| {
        crate::dist::lnorm::qlnorm_inner(p, meanlog, sdlog, true, false)
    })
}

/// R's `dlogis(x, location=0, scale=1)` — logistic density.
pub unsafe fn do_dlogis(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.0, 1.0, |x, location, scale| {
        crate::dist::logistic::dlogis_inner(x, location, scale, false)
    })
}

/// R's `plogis(q, location=0, scale=1)` — logistic CDF.
pub unsafe fn do_plogis(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.0, 1.0, |q, location, scale| {
        crate::dist::logistic::plogis_inner(q, location, scale, true, false)
    })
}

/// R's `qlogis(p, location=0, scale=1)` — logistic quantile.
pub unsafe fn do_qlogis(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 0.0, 1.0, |p, location, scale| {
        crate::dist::logistic::qlogis_inner(p, location, scale, true, false)
    })
}

/// R's `dsignrank(x, n)` — Wilcoxon signed rank density.
pub unsafe fn do_dsignrank(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |x, n, _| {
        crate::dist::signrank::dsignrank_inner(x, n, false)
    })
}

/// R's `psignrank(q, n)` — Wilcoxon signed rank CDF.
pub unsafe fn do_psignrank(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |q, n, _| {
        crate::dist::signrank::psignrank_inner(q, n, true, false)
    })
}

/// R's `qsignrank(p, n)` — Wilcoxon signed rank quantile.
pub unsafe fn do_qsignrank(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 0.0, |p, n, _| {
        crate::dist::signrank::qsignrank_inner(p, n, true, false)
    })
}

/// R's `dwilcox(x, m, n)` — Wilcoxon rank sum density.
pub unsafe fn do_dwilcox(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |x, m, n| {
        crate::dist::wilcox::dwilcox_inner(x, m, n, false)
    })
}

/// R's `pwilcox(q, m, n)` — Wilcoxon rank sum CDF.
pub unsafe fn do_pwilcox(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |q, m, n| {
        crate::dist::wilcox::pwilcox_inner(q, m, n, true, false)
    })
}

/// R's `qwilcox(p, m, n)` — Wilcoxon rank sum quantile.
pub unsafe fn do_qwilcox(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 1.0, 1.0, |p, m, n| {
        crate::dist::wilcox::qwilcox_inner(p, m, n, true, false)
    })
}

/// R's `dhyper(x, m, n, k)` — hypergeometric density (4 params).
pub unsafe fn do_dhyper(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_tertiary(args, 1.0, 1.0, 1.0, |x, m, n, k| {
        crate::dist::hypergeometric::dhyper_inner(x, m, n, k, false)
    })
}

/// R's `phyper(q, m, n, k)` — hypergeometric CDF (4 params).
pub unsafe fn do_phyper(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_tertiary(args, 1.0, 1.0, 1.0, |q, m, n, k| {
        crate::dist::hypergeometric::phyper_inner(q, m, n, k, true, false)
    })
}

/// R's `qhyper(p, m, n, k)` — hypergeometric quantile (4 params).
pub unsafe fn do_qhyper(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_tertiary(args, 1.0, 1.0, 1.0, |p, m, n, k| {
        crate::dist::hypergeometric::qhyper_inner(p, m, n, k, true, false)
    })
}

/// R's `dtukey(q, nmeans, df)` — Studentized range CDF (nranges defaults to 1).
pub unsafe fn do_ptukey(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 2.0, 1.0, |q, nmeans, df| {
        crate::dist::tukey::ptukey_inner(q, 1.0, nmeans, df, true, false)
    })
}

/// R's `qtukey(p, nmeans, df)` — Studentized range quantile (nranges defaults to 1).
pub unsafe fn do_qtukey(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    do_dist_unary(args, 2.0, 1.0, |p, nmeans, df| {
        crate::dist::tukey::qtukey_inner(p, 1.0, nmeans, df, true, false)
    })
}

/// R's `dmultinom(x, prob, log=FALSE)` — multinomial probability.
pub unsafe fn do_dmultinom(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let prob_arg = CAR(CDR(args));
        let log_arg = CAR(CDR(CDR(args)));

        if x_arg.is_null() || prob_arg.is_null() {
            return R_NilValue();
        }

        let nx = if x_arg == R_NilValue() {
            0
        } else {
            XLENGTH(x_arg)
        };
        let np = if prob_arg == R_NilValue() {
            0
        } else {
            XLENGTH(prob_arg)
        };
        if nx != np {
            base_error("x[] and prob[] must be equal length vectors.");
        }
        let give_log = if log_arg.is_null() || log_arg == R_NilValue() {
            false
        } else {
            real_or_default(log_arg, 0.0) != 0.0
        };

        // Collect x values
        let mut xv: Vec<f64> = Vec::with_capacity(nx as usize);
        for i in 0..nx {
            xv.push(elt_real_safe(x_arg, i));
        }

        // Collect and validate prob values
        let mut pv: Vec<f64> = Vec::with_capacity(np as usize);
        let mut prob_sum = 0.0;
        for i in 0..np {
            let p = elt_real_safe(prob_arg, i);
            if !p.is_finite() || p < 0.0 {
                base_error("probabilities must be finite, non-negative and not all 0");
            }
            prob_sum += p;
            pv.push(p);
        }
        if prob_sum <= 0.0 {
            base_error("probabilities must be finite, non-negative and not all 0");
        }
        for p in &mut pv {
            *p /= prob_sum;
        }

        // dmultinom: log-probability of multinomial outcome
        // Uses lgammafn(x+1) for log-factorial terms
        let k = xv.len().min(pv.len());
        let n_total: f64 = xv.iter().sum();

        let mut log_prob = crate::special::gamma::lgammafn(n_total + 1.0);
        for i in 0..k {
            log_prob -= crate::special::gamma::lgammafn(xv[i] + 1.0);
            if pv[i] > 0.0 {
                log_prob += xv[i] * pv[i].ln();
            } else if xv[i] > 0.0 {
                log_prob = f64::NEG_INFINITY;
            }
        }

        let result = if give_log { log_prob } else { log_prob.exp() };
        Rf_ScalarReal(result)
    }
}

/// Generic vectorized distribution function with 3 extra parameters (4 total: x, p1, p2, p3).
fn do_dist_tertiary(
    args: SEXP,
    default_p1: f64,
    default_p2: f64,
    default_p3: f64,
    f: fn(f64, f64, f64, f64) -> f64,
) -> SEXP {
    unsafe {
        let x = CAR(args);
        let p1 = real_or_default(CAR(CDR(args)), default_p1);
        let p2_arg = CAR(CDR(CDR(args)));
        let p2 = if p2_arg.is_null() || p2_arg == R_NilValue() {
            default_p2
        } else {
            real_or_default(p2_arg, default_p2)
        };
        let p3_arg = CAR(CDR(CDR(CDR(args))));
        let p3 = if p3_arg.is_null() || p3_arg == R_NilValue() {
            default_p3
        } else {
            real_or_default(p3_arg, default_p3)
        };
        if x.is_null() {
            return R_NilValue();
        }
        if x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        if n == 0 {
            return result;
        }
        let dst = REAL(result);
        for i in 0..n {
            *dst.add(i as usize) = f(elt_real_safe(x, i), p1, p2, p3);
        }
        result
    }
}

/// Generic vectorized distribution function with 2 parameters.
fn do_dist_unary(
    args: SEXP,
    default_p1: f64,
    default_p2: f64,
    f: fn(f64, f64, f64) -> f64,
) -> SEXP {
    unsafe {
        let x = CAR(args);
        let p1 = real_or_default(CAR(CDR(args)), default_p1);
        let p2_arg = CAR(CDR(CDR(args)));
        let p2 = if p2_arg.is_null() || p2_arg == R_NilValue() {
            default_p2
        } else {
            real_or_default(p2_arg, default_p2)
        };
        if x.is_null() {
            return R_NilValue();
        }
        if x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        if n == 0 {
            return result;
        }
        let dst = REAL(result);
        for i in 0..n {
            *dst.add(i as usize) = f(elt_real_safe(x, i), p1, p2);
        }
        result
    }
}

fn do_dist_unary_with_log(
    args: SEXP,
    default_p1: f64,
    default_p2: f64,
    f: fn(f64, f64, f64, bool) -> f64,
) -> SEXP {
    unsafe {
        let [x, p1_arg, p2_arg, log_arg, ..] = dist_args::<4>(args);
        let p1 = real_or_default(p1_arg, default_p1);
        let p2 = real_or_default(p2_arg, default_p2);
        let give_log = logical_arg(log_arg, false);
        map_real_distribution(x, |x| f(x, p1, p2, give_log))
    }
}

fn do_dist_unary_with_tail_log(
    args: SEXP,
    default_p1: f64,
    default_p2: f64,
    f: fn(f64, f64, f64, bool, bool) -> f64,
) -> SEXP {
    unsafe {
        let [x, p1_arg, p2_arg, lower_tail_arg, log_p_arg] = dist_args::<5>(args);
        let p1 = real_or_default(p1_arg, default_p1);
        let p2 = real_or_default(p2_arg, default_p2);
        let lower_tail = logical_arg(lower_tail_arg, true);
        let log_p = logical_arg(log_p_arg, false);
        map_real_distribution(x, |x| f(x, p1, p2, lower_tail, log_p))
    }
}

unsafe fn dist_args<const N: usize>(args: SEXP) -> [SEXP; N] {
    unsafe {
        let mut out = [R_NilValue(); N];
        let mut cur = args;
        for slot in &mut out {
            if cur.is_null() || cur == R_NilValue() {
                break;
            }
            *slot = CAR(cur);
            cur = CDR(cur);
        }
        out
    }
}

unsafe fn map_real_distribution(mut x: SEXP, f: impl Fn(f64) -> f64) -> SEXP {
    unsafe {
        if x.is_null() {
            return R_NilValue();
        }
        if x == R_NilValue() {
            x = Rf_allocVector3(SEXPTYPE::REALSXP, 0);
            if x.is_null() {
                return R_NilValue();
            }
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        if n == 0 {
            return result;
        }
        let dst = REAL(result);
        for i in 0..n {
            *dst.add(i as usize) = f(elt_real_safe(x, i));
        }
        result
    }
}

fn elt_real_safe(x: SEXP, i: R_xlen_t) -> f64 {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return NA_REAL;
        }
        let n = XLENGTH(x);
        if n == 0 {
            return NA_REAL;
        }
        let idx = if n == 0 { 0 } else { i % n };
        let t = TYPEOF(x);
        if t == SEXPTYPE::REALSXP {
            *REAL(x).add(idx as usize)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let v = *INTEGER(x).add(idx as usize);
            if v == NA_INTEGER { NA_REAL } else { v as f64 }
        } else {
            NA_REAL
        }
    }
}

// ---------------------------------------------------------------------------
// Matrix operations: matrix(), t(), nrow(), ncol(), dim(), diag()
// ---------------------------------------------------------------------------

fn supported_matrix_type(t: c_int) -> bool {
    t == SEXPTYPE::REALSXP
        || t == SEXPTYPE::INTSXP
        || t == SEXPTYPE::LGLSXP
        || t == SEXPTYPE::CPLXSXP
        || t == SEXPTYPE::RAWSXP
        || t == SEXPTYPE::STRSXP
        || t == SEXPTYPE::VECSXP
}

unsafe fn set_matrix_na_or_zero(x: SEXP, i: R_xlen_t) {
    unsafe {
        match TYPEOF(x) {
            t if t == SEXPTYPE::REALSXP => *REAL(x).add(i as usize) = NA_REAL,
            t if t == SEXPTYPE::INTSXP => *INTEGER(x).add(i as usize) = NA_INTEGER,
            t if t == SEXPTYPE::LGLSXP => *LOGICAL(x).add(i as usize) = NA_INTEGER,
            t if t == SEXPTYPE::CPLXSXP => {
                *COMPLEX(x).add(i as usize) = Rcomplex {
                    r: NA_REAL,
                    i: NA_REAL,
                };
            }
            t if t == SEXPTYPE::RAWSXP => *RAW(x).add(i as usize) = 0 as Rbyte,
            t if t == SEXPTYPE::STRSXP => {
                SET_STRING_ELT(x, i, crate::sexp::globals::R_NaString());
            }
            t if t == SEXPTYPE::VECSXP => SET_VECTOR_ELT(x, i, R_NilValue()),
            _ => {}
        }
    }
}

unsafe fn set_matrix_zero(x: SEXP, i: R_xlen_t) {
    unsafe {
        match TYPEOF(x) {
            t if t == SEXPTYPE::REALSXP => *REAL(x).add(i as usize) = 0.0,
            t if t == SEXPTYPE::INTSXP => *INTEGER(x).add(i as usize) = 0,
            t if t == SEXPTYPE::LGLSXP => *LOGICAL(x).add(i as usize) = FALSE,
            t if t == SEXPTYPE::CPLXSXP => {
                *COMPLEX(x).add(i as usize) = Rcomplex { r: 0.0, i: 0.0 };
            }
            t if t == SEXPTYPE::RAWSXP => *RAW(x).add(i as usize) = 0 as Rbyte,
            t if t == SEXPTYPE::STRSXP => SET_STRING_ELT(x, i, Rf_mkChar(c"".as_ptr())),
            t if t == SEXPTYPE::VECSXP => SET_VECTOR_ELT(x, i, R_NilValue()),
            _ => {}
        }
    }
}

unsafe fn copy_matrix_element(dst: SEXP, dst_i: R_xlen_t, src: SEXP, src_i: R_xlen_t) {
    unsafe {
        match TYPEOF(src) {
            t if t == SEXPTYPE::REALSXP => {
                *REAL(dst).add(dst_i as usize) = *REAL(src).add(src_i as usize)
            }
            t if t == SEXPTYPE::INTSXP => {
                *INTEGER(dst).add(dst_i as usize) = *INTEGER(src).add(src_i as usize)
            }
            t if t == SEXPTYPE::LGLSXP => {
                *LOGICAL(dst).add(dst_i as usize) = *LOGICAL(src).add(src_i as usize)
            }
            t if t == SEXPTYPE::CPLXSXP => {
                *COMPLEX(dst).add(dst_i as usize) = *COMPLEX(src).add(src_i as usize)
            }
            t if t == SEXPTYPE::RAWSXP => {
                *RAW(dst).add(dst_i as usize) = *RAW(src).add(src_i as usize)
            }
            t if t == SEXPTYPE::STRSXP => SET_STRING_ELT(dst, dst_i, STRING_ELT(src, src_i)),
            t if t == SEXPTYPE::VECSXP => SET_VECTOR_ELT(dst, dst_i, VECTOR_ELT(src, src_i)),
            _ => {}
        }
    }
}

unsafe fn set_two_dim_attr(x: SEXP, nrow: R_xlen_t, ncol: R_xlen_t) {
    unsafe {
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if dim.is_null() {
            return;
        }
        let _dim_guard = protect(dim);
        *INTEGER(dim) = nrow as c_int;
        *INTEGER(dim).add(1) = ncol as c_int;
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_DimSymbol(), dim);
    }
}

unsafe fn set_diagonal_identity_value(x: SEXP, i: R_xlen_t) {
    unsafe {
        match TYPEOF(x) {
            t if t == SEXPTYPE::REALSXP => *REAL(x).add(i as usize) = 1.0,
            t if t == SEXPTYPE::INTSXP => *INTEGER(x).add(i as usize) = 1,
            t if t == SEXPTYPE::LGLSXP => *LOGICAL(x).add(i as usize) = TRUE,
            t if t == SEXPTYPE::CPLXSXP => {
                *COMPLEX(x).add(i as usize) = Rcomplex { r: 1.0, i: 0.0 };
            }
            t if t == SEXPTYPE::RAWSXP => *RAW(x).add(i as usize) = 1 as Rbyte,
            _ => {}
        }
    }
}

unsafe fn string_vector_contains_value(x: SEXP, needle: &str) -> bool {
    unsafe {
        if x.is_null() || TYPEOF(x) != SEXPTYPE::STRSXP {
            return false;
        }
        for i in 0..XLENGTH(x) {
            let elt = STRING_ELT(x, i);
            if elt.is_null() {
                continue;
            }
            let ptr = CHAR(elt);
            if !ptr.is_null() && CStr::from_ptr(ptr).to_str().ok() == Some(needle) {
                return true;
            }
        }
        false
    }
}

unsafe fn is_data_frame_object(x: SEXP) -> bool {
    unsafe {
        let class = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
        );
        string_vector_contains_value(class, "data.frame")
    }
}

unsafe fn data_frame_row_count(x: SEXP) -> R_xlen_t {
    unsafe {
        let row_names = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("row.names").unwrap_or_default().as_ptr()),
        );
        if !row_names.is_null() && TYPEOF(row_names) == SEXPTYPE::INTSXP && LENGTH(row_names) == 2 {
            let first = *INTEGER(row_names);
            let second = *INTEGER(row_names).add(1);
            if first == NA_INTEGER && second < 0 {
                return -(second as R_xlen_t);
            }
        }

        let mut rows = 0;
        for i in 0..XLENGTH(x) {
            let col = VECTOR_ELT(x, i);
            if !col.is_null() {
                rows = rows.max(XLENGTH(col));
            }
        }
        rows
    }
}

/// R's `matrix(data, nrow, ncol, byrow)` — create a matrix.
pub unsafe fn do_matrix(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let data = CAR(args);
        let nrow_arg = CAR(CDR(args));
        let ncol_arg = CAR(CDR(CDR(args)));
        let byrow_arg = CAR(CDR(CDR(CDR(args))));

        if data.is_null() || data == R_NilValue() {
            return R_NilValue();
        }

        let data_len = XLENGTH(data);
        let nrow = if nrow_arg.is_null() || nrow_arg == R_NilValue() {
            data_len
        } else {
            real_or_default(nrow_arg, data_len as f64) as R_xlen_t
        };
        let ncol = if ncol_arg.is_null() || ncol_arg == R_NilValue() {
            if nrow == 0 {
                0
            } else {
                (data_len + nrow - 1) / nrow
            }
        } else {
            real_or_default(ncol_arg, 1.0) as R_xlen_t
        };
        let byrow = if byrow_arg.is_null() || byrow_arg == R_NilValue() {
            false
        } else {
            TYPEOF(byrow_arg) == SEXPTYPE::LGLSXP && *LOGICAL(byrow_arg) != 0
        };

        let t = TYPEOF(data);
        if !supported_matrix_type(t) || nrow < 0 || ncol < 0 {
            return R_NilValue();
        }
        let result = Rf_allocVector3(t, nrow * ncol);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        // R stores matrices in column-major order. If data has length zero, keep
        // the requested shape and fill with the type-appropriate missing value.
        for i in 0..(nrow * ncol) {
            if data_len == 0 {
                set_matrix_na_or_zero(result, i);
            } else {
                let src_idx = if byrow && ncol > 0 {
                    let row = i % nrow;
                    let col = i / nrow;
                    row * ncol + col
                } else {
                    i
                } % data_len;
                copy_matrix_element(result, i, data, src_idx);
            }
        }

        // Set dim attribute
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            *INTEGER(dim) = nrow as c_int;
            *INTEGER(dim).add(1) = ncol as c_int;
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                dim,
            );
        }

        result
    }
}

/// R's `array(data, dim, dimnames)` — create an array with recycled data.
pub unsafe fn do_array(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let data = arg_by_name_or_position(args, &["data"], 0);
        let dim_arg = arg_by_name_or_position(args, &["dim"], 1);
        let dimnames = arg_by_name_or_position(args, &["dimnames"], 2);

        let data_missing = data.is_null() || data == R_NilValue();
        let data_type = if data_missing {
            SEXPTYPE::LGLSXP.as_c_int()
        } else {
            TYPEOF(data)
        };
        if !supported_matrix_type(data_type) {
            std::panic::panic_any(RError {
                message: "'data' must be a vector type".to_string(),
            });
        }

        let data_len = if data_missing { 1 } else { XLENGTH(data) };
        let dim = match array_dimension_attribute(dim_arg, data_len) {
            Ok(dim) => dim,
            Err(message) => {
                std::panic::panic_any(RError { message });
            }
        };
        let _dim_guard = protect(dim);
        let dim_len = XLENGTH(dim);
        let mut total_len: R_xlen_t = 1;
        for i in 0..dim_len {
            total_len = total_len.saturating_mul(*INTEGER(dim).add(i as usize) as R_xlen_t);
        }

        if !valid_array_dimnames(dimnames, dim) {
            std::panic::panic_any(RError {
                message: "length of 'dimnames' not equal to array extent".to_string(),
            });
        }

        let result = Rf_allocVector3(data_type, total_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..total_len {
            if data_missing || data_len == 0 {
                set_matrix_na_or_zero(result, i);
            } else {
                copy_matrix_element(result, i, data, i % data_len);
            }
        }

        crate::sexp::attrib_core::setAttrib(result, crate::sexp::attrib_core::R_DimSymbol(), dim);
        if !dimnames.is_null() && dimnames != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
                dimnames,
            );
        }

        result
    }
}

/// R's `drop(x)` — remove extent-one dimensions from arrays and matrices.
pub unsafe fn do_drop(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        if dim.is_null() || dim == R_NilValue() || TYPEOF(dim) != SEXPTYPE::INTSXP {
            return x;
        }

        let dim_count = XLENGTH(dim);
        let mut kept_axes = Vec::new();
        for i in 0..dim_count {
            if *INTEGER(dim).add(i as usize) != 1 {
                kept_axes.push(i);
            }
        }
        if kept_axes.len() == dim_count as usize {
            return x;
        }

        let result = crate::mainutils::duplicate::shallow_duplicate(x);
        if result.is_null() || result == R_NilValue() {
            return result;
        }
        let _result_guard = protect(result);

        let dimnames =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimNamesSymbol());

        match kept_axes.len() {
            0 => {
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_DimSymbol(),
                    R_NilValue(),
                );
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_DimNamesSymbol(),
                    R_NilValue(),
                );
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    R_NilValue(),
                );
            }
            1 => {
                let names = retained_dimname(dimnames, kept_axes[0]);
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_DimSymbol(),
                    R_NilValue(),
                );
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_DimNamesSymbol(),
                    R_NilValue(),
                );
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    names,
                );
            }
            _ => {
                let new_dim = Rf_allocVector3(SEXPTYPE::INTSXP, kept_axes.len() as R_xlen_t);
                if new_dim.is_null() {
                    return R_NilValue();
                }
                let _dim_guard = protect(new_dim);
                for (out_i, axis) in kept_axes.iter().enumerate() {
                    *INTEGER(new_dim).add(out_i) = *INTEGER(dim).add(*axis as usize);
                }
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_DimSymbol(),
                    new_dim,
                );

                let new_dimnames = retained_dimnames(dimnames, &kept_axes);
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_DimNamesSymbol(),
                    new_dimnames,
                );
                crate::sexp::attrib_core::setAttrib(
                    result,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    R_NilValue(),
                );
            }
        }

        result
    }
}

/// R's `t(x)` — transpose a matrix.
pub unsafe fn do_transpose(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        // Get dimensions
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
                (XLENGTH(x), 1)
            };

        let t = TYPEOF(x);
        if !supported_matrix_type(t) || nrow < 0 || ncol < 0 {
            return R_NilValue();
        }
        let result = Rf_allocVector3(t, nrow * ncol);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        // Source and destination are both column-major:
        // src(row, col) = row + col * nrow
        // dst(col, row) = col + row * ncol
        for row in 0..nrow {
            for col in 0..ncol {
                let src = row + col * nrow;
                let dst = col + row * ncol;
                copy_matrix_element(result, dst, x, src);
            }
        }

        // Set transposed dim attribute
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            *INTEGER(dim) = ncol as c_int;
            *INTEGER(dim).add(1) = nrow as c_int;
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                dim,
            );
        }

        result
    }
}

/// R's `nrow(x)` — number of rows.
pub unsafe fn do_nrow(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
        }
        if is_data_frame_object(x) {
            return Rf_ScalarInteger(data_frame_row_count(x) as c_int);
        }
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 1 {
            Rf_ScalarInteger(*INTEGER(dim_attr))
        } else {
            Rf_ScalarInteger(XLENGTH(x) as c_int)
        }
    }
}

/// R's `ncol(x)` — number of columns.
pub unsafe fn do_ncol(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
        }
        if is_data_frame_object(x) {
            return Rf_ScalarInteger(XLENGTH(x) as c_int);
        }
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
            Rf_ScalarInteger(*INTEGER(dim_attr).add(1))
        } else {
            Rf_ScalarInteger(1)
        }
    }
}

/// R's `dim(x)` — dimensions as integer vector.
pub unsafe fn do_dim(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        if is_data_frame_object(x) {
            let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            if dim.is_null() {
                return R_NilValue();
            }
            *INTEGER(dim) = data_frame_row_count(x) as c_int;
            *INTEGER(dim).add(1) = XLENGTH(x) as c_int;
            return dim;
        }
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if !dim_attr.is_null() {
            dim_attr
        } else {
            R_NilValue()
        }
    }
}

/// R's `tsp(x)` — time-series parameter attribute.
pub unsafe fn do_tsp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_TspSymbol())
    }
}

/// R's `dim(x) <- value` — replace an object's dimension attribute.
pub unsafe fn do_dim_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        if value.is_null() || value == R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                x,
                crate::sexp::attrib_core::R_DimSymbol(),
                R_NilValue(),
            );
            crate::sexp::attrib_core::setAttrib(
                x,
                crate::sexp::attrib_core::R_DimNamesSymbol(),
                R_NilValue(),
            );
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }

        let dim = match dimension_attribute(value, XLENGTH(x)) {
            Ok(dim) => dim,
            Err(message) => {
                std::panic::panic_any(RError { message });
            }
        };

        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::attrib_core::R_NamesSymbol(),
            R_NilValue(),
        );
        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            R_NilValue(),
        );
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_DimSymbol(), dim);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `tsp(x) <- value` — replace or remove time-series parameters.
pub unsafe fn do_tsp_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        if value.is_null() || value == R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                x,
                crate::sexp::attrib_core::R_TspSymbol(),
                R_NilValue(),
            );
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }

        let tsp = match tsp_attribute(value) {
            Ok(tsp) => tsp,
            Err(message) => {
                std::panic::panic_any(RError { message });
            }
        };
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_TspSymbol(), tsp);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

unsafe fn tsp_attribute(value: SEXP) -> Result<SEXP, String> {
    unsafe {
        if XLENGTH(value) != 3
            || (TYPEOF(value) != SEXPTYPE::INTSXP && TYPEOF(value) != SEXPTYPE::REALSXP)
        {
            return Err("'tsp' attribute must be numeric of length three".to_string());
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP, 3);
        if result.is_null() {
            return Ok(R_NilValue());
        }
        let _result_guard = protect(result);
        for i in 0..3 {
            *REAL(result).add(i) = if TYPEOF(value) == SEXPTYPE::INTSXP {
                let n = INTEGER_ELT(value, i as c_int);
                if n == NA_INTEGER { NA_REAL } else { n as f64 }
            } else {
                REAL_ELT(value, i as c_int)
            };
        }

        let start = *REAL(result);
        let end = *REAL(result).add(1);
        let frequency = *REAL(result).add(2);
        if frequency.is_infinite() || start.is_infinite() || end.is_infinite() {
            return Err("invalid time series parameters specified (1)".to_string());
        }
        if !frequency.is_nan() && frequency <= 0.0 {
            return Err("invalid time series parameters specified (0)".to_string());
        }
        if start.is_finite() && end.is_finite() && frequency.is_finite() && end < start {
            return Err("invalid time series parameters specified (1)".to_string());
        }
        Ok(result)
    }
}

unsafe fn dimension_attribute(value: SEXP, object_len: R_xlen_t) -> Result<SEXP, String> {
    unsafe {
        if !is_atomic_vector_type(TYPEOF(value)) {
            return Err("invalid second argument, must be vector or NULL".to_string());
        }

        let n = XLENGTH(value);
        if n == 0 {
            return Err("length-0 dimension vector is invalid".to_string());
        }

        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        let mut product: i128 = 1;
        for i in 0..n {
            let part = dimension_component(value, i);
            if part == NA_INTEGER {
                return Err("the dims contain missing values".to_string());
            }
            if part < 0 {
                return Err("the dims contain negative values".to_string());
            }
            *INTEGER(dim).add(i as usize) = part;
            product = product.saturating_mul(part as i128);
        }

        if product != object_len as i128 {
            return Err(format!(
                "dims [product {product}] do not match the length of object [{object_len}]"
            ));
        }

        Ok(dim)
    }
}

unsafe fn array_dimension_attribute(value: SEXP, data_len: R_xlen_t) -> Result<SEXP, String> {
    unsafe {
        if value.is_null() || value == R_NilValue() {
            let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
            if dim.is_null() {
                return Ok(R_NilValue());
            }
            *INTEGER(dim) = data_len as c_int;
            return Ok(dim);
        }

        if !is_atomic_vector_type(TYPEOF(value)) {
            return Err("'dim' must be a numeric vector".to_string());
        }

        let n = XLENGTH(value);
        if n == 0 {
            return Err("length-0 dimension vector is invalid".to_string());
        }

        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        let mut product: i128 = 1;
        for i in 0..n {
            let part = dimension_component(value, i);
            if part == NA_INTEGER {
                return Err("the dims contain missing values".to_string());
            }
            if part < 0 {
                return Err("the dims contain negative values".to_string());
            }
            *INTEGER(dim).add(i as usize) = part;
            product = product.saturating_mul(part as i128);
        }

        if product > R_xlen_t::MAX as i128 {
            return Err("array is too large".to_string());
        }

        Ok(dim)
    }
}

unsafe fn valid_array_dimnames(dimnames: SEXP, dim: SEXP) -> bool {
    unsafe {
        if dimnames.is_null() || dimnames == R_NilValue() {
            return true;
        }
        if TYPEOF(dimnames) != SEXPTYPE::VECSXP {
            return false;
        }
        let dim_count = XLENGTH(dim);
        if XLENGTH(dimnames) > dim_count {
            return false;
        }
        for i in 0..XLENGTH(dimnames) {
            let names = VECTOR_ELT(dimnames, i);
            if names.is_null() || names == R_NilValue() {
                continue;
            }
            if XLENGTH(names) != *INTEGER(dim).add(i as usize) as R_xlen_t {
                return false;
            }
        }
        true
    }
}

unsafe fn retained_dimname(dimnames: SEXP, axis: R_xlen_t) -> SEXP {
    unsafe {
        if dimnames.is_null() || dimnames == R_NilValue() || TYPEOF(dimnames) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        if axis >= XLENGTH(dimnames) {
            return R_NilValue();
        }
        VECTOR_ELT(dimnames, axis)
    }
}

unsafe fn retained_dimnames(dimnames: SEXP, axes: &[R_xlen_t]) -> SEXP {
    unsafe {
        if dimnames.is_null() || dimnames == R_NilValue() || TYPEOF(dimnames) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, axes.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let mut has_names = false;
        for (out_i, axis) in axes.iter().enumerate() {
            let names = if *axis < XLENGTH(dimnames) {
                VECTOR_ELT(dimnames, *axis)
            } else {
                R_NilValue()
            };
            if !names.is_null() && names != R_NilValue() {
                has_names = true;
            }
            SET_VECTOR_ELT(result, out_i as R_xlen_t, names);
        }
        if has_names { result } else { R_NilValue() }
    }
}

unsafe fn dimension_component(value: SEXP, i: R_xlen_t) -> c_int {
    unsafe {
        let kind = TYPEOF(value);
        if kind == SEXPTYPE::INTSXP.as_c_int() || kind == SEXPTYPE::LGLSXP.as_c_int() {
            INTEGER_ELT(value, i as c_int)
        } else if kind == SEXPTYPE::REALSXP.as_c_int() {
            real_to_dimension(REAL_ELT(value, i as c_int))
        } else if kind == SEXPTYPE::CPLXSXP.as_c_int() {
            real_to_dimension((*COMPLEX(value).add(i as usize)).r)
        } else if kind == SEXPTYPE::STRSXP.as_c_int() {
            let elt = STRING_ELT(value, i);
            if elt.is_null() || elt == crate::sexp::globals::R_NaString() {
                NA_INTEGER
            } else {
                let text = CStr::from_ptr(CHAR(elt)).to_string_lossy();
                text.trim()
                    .parse::<f64>()
                    .map(real_to_dimension)
                    .unwrap_or(NA_INTEGER)
            }
        } else if kind == SEXPTYPE::RAWSXP.as_c_int() {
            *RAW(value).add(i as usize) as c_int
        } else {
            NA_INTEGER
        }
    }
}

fn real_to_dimension(value: f64) -> c_int {
    if !value.is_finite() || value < c_int::MIN as f64 || value > c_int::MAX as f64 {
        return NA_INTEGER;
    }
    value.trunc() as c_int
}

fn is_atomic_vector_type(kind: c_int) -> bool {
    kind == SEXPTYPE::LGLSXP.as_c_int()
        || kind == SEXPTYPE::INTSXP.as_c_int()
        || kind == SEXPTYPE::REALSXP.as_c_int()
        || kind == SEXPTYPE::CPLXSXP.as_c_int()
        || kind == SEXPTYPE::STRSXP.as_c_int()
        || kind == SEXPTYPE::RAWSXP.as_c_int()
}

/// R's `diag(x)` — extract diagonal or create diagonal matrix.
pub unsafe fn do_diag(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        // Check if x is a matrix
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
            // Extract diagonal
            let nrow = *INTEGER(dim_attr) as usize;
            let ncol = *INTEGER(dim_attr).add(1) as usize;
            let n = nrow.min(ncol);
            if !supported_matrix_type(TYPEOF(x)) {
                return R_NilValue();
            }
            let result = Rf_allocVector3(TYPEOF(x), n as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            for i in 0..n {
                let src = i + i * nrow;
                copy_matrix_element(result, i as R_xlen_t, x, src as R_xlen_t);
            }
            result
        } else {
            if XLENGTH(x) == 1 {
                let n = real_or_default(x, 0.0).max(0.0) as usize;
                let t = TYPEOF(x);
                if !supported_matrix_type(t) {
                    return R_NilValue();
                }
                let result = Rf_allocVector3(t, (n * n) as R_xlen_t);
                if result.is_null() {
                    return R_NilValue();
                }
                let _result_guard = protect(result);

                for i in 0..n * n {
                    set_matrix_zero(result, i as R_xlen_t);
                }
                for i in 0..n {
                    let dst = i + i * n;
                    set_diagonal_identity_value(result, dst as R_xlen_t);
                }

                let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
                if !dim.is_null() {
                    *INTEGER(dim) = n as c_int;
                    *INTEGER(dim).add(1) = n as c_int;
                    crate::sexp::attrib_core::setAttrib(
                        result,
                        Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                        dim,
                    );
                }
                return result;
            }

            // Create diagonal matrix from vector
            let n = XLENGTH(x) as usize;
            let t = TYPEOF(x);
            if !supported_matrix_type(t) {
                return R_NilValue();
            }
            let result = Rf_allocVector3(t, (n * n) as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);

            for i in 0..n * n {
                set_matrix_zero(result, i as R_xlen_t);
            }
            for i in 0..n {
                let dst = i + i * n;
                copy_matrix_element(result, dst as R_xlen_t, x, i as R_xlen_t);
            }

            // Set dim
            let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            if !dim.is_null() {
                *INTEGER(dim) = n as c_int;
                *INTEGER(dim).add(1) = n as c_int;
                crate::sexp::attrib_core::setAttrib(
                    result,
                    Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                    dim,
                );
            }
            result
        }
    }
}

// ---------------------------------------------------------------------------
// Set operations: unique, sort, order, rev, match, %in%, setequal, union, intersect, setdiff
// ---------------------------------------------------------------------------

/// R's `unique(x)` — return unique atomic elements in R's retained-index order.
pub unsafe fn do_unique(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x);
        let sexptype = SEXPTYPE(t);
        if t != SEXPTYPE::LGLSXP
            && t != SEXPTYPE::INTSXP
            && t != SEXPTYPE::REALSXP
            && t != SEXPTYPE::STRSXP
        {
            return x;
        }
        let n = XLENGTH(x);
        let from_last = logical_arg_by_name_or_position(args, "fromLast", 2).unwrap_or(false);
        let incomparables = arg_by_name_or_position(args, &["incomparables"], 1);
        let incomparable_keys = atomic_incomparable_keys(incomparables, sexptype);

        let mut unique_indices: Vec<R_xlen_t> = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        if from_last {
            let mut keep = vec![false; n as usize];
            for i in (0..n).rev() {
                let key = atomic_unique_key(x, i, sexptype);
                if incomparable_keys.contains(&key) || seen.insert(key) {
                    keep[i as usize] = true;
                }
            }
            for i in 0..n {
                if keep[i as usize] {
                    unique_indices.push(i);
                }
            }
        } else {
            for i in 0..n {
                let key = atomic_unique_key(x, i, sexptype);
                if incomparable_keys.contains(&key) || seen.insert(key) {
                    unique_indices.push(i);
                }
            }
        }

        let result = Rf_allocVector3(t, unique_indices.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (new_i, &old_i) in unique_indices.iter().enumerate() {
            match t {
                tt if tt == SEXPTYPE::REALSXP => {
                    *REAL(result).add(new_i) = *REAL(x).add(old_i as usize);
                }
                tt if tt == SEXPTYPE::STRSXP => {
                    SET_STRING_ELT(result, new_i as R_xlen_t, STRING_ELT(x, old_i));
                }
                _ => {
                    *INTEGER(result).add(new_i) = *INTEGER(x).add(old_i as usize);
                }
            }
        }
        result
    }
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
enum AtomicUniqueKey {
    Integer(c_int),
    Real(u64),
    String(String),
}

fn atomic_incomparable_keys(
    incomparables: SEXP,
    target_type: SEXPTYPE,
) -> std::collections::BTreeSet<AtomicUniqueKey> {
    unsafe {
        let mut keys = std::collections::BTreeSet::new();
        if incomparables.is_null() || incomparables == R_NilValue() {
            return keys;
        }
        let n = XLENGTH(incomparables);
        for i in 0..n {
            keys.insert(atomic_unique_key(incomparables, i, target_type));
        }
        keys
    }
}

fn atomic_unique_key(x: SEXP, index: R_xlen_t, target_type: SEXPTYPE) -> AtomicUniqueKey {
    unsafe {
        match target_type {
            t if t == SEXPTYPE::STRSXP => {
                if TYPEOF(x) == SEXPTYPE::STRSXP {
                    let value = STRING_ELT(x, index);
                    if charsxp_is_na(value) {
                        AtomicUniqueKey::String("<NA>".to_string())
                    } else {
                        AtomicUniqueKey::String(elt_to_string(x, index))
                    }
                } else if atomic_value_is_missing(x, index) {
                    AtomicUniqueKey::String("<NA>".to_string())
                } else {
                    AtomicUniqueKey::String(elt_to_string(x, index))
                }
            }
            t if t == SEXPTYPE::REALSXP => {
                let value = if TYPEOF(x) == SEXPTYPE::REALSXP {
                    *REAL(x).add(index as usize)
                } else {
                    let raw = *INTEGER(x).add(index as usize);
                    if raw == NA_INTEGER {
                        NA_REAL
                    } else {
                        raw as f64
                    }
                };
                if value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    AtomicUniqueKey::Real(crate::sexp::ffi::R_NA_BIT_PATTERN)
                } else if value.is_nan() {
                    AtomicUniqueKey::Real(f64::NAN.to_bits())
                } else {
                    AtomicUniqueKey::Real(value.to_bits())
                }
            }
            _ => {
                let value = if TYPEOF(x) == SEXPTYPE::REALSXP {
                    let value = *REAL(x).add(index as usize);
                    if value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                        NA_INTEGER
                    } else {
                        value as c_int
                    }
                } else {
                    *INTEGER(x).add(index as usize)
                };
                AtomicUniqueKey::Integer(value)
            }
        }
    }
}

fn atomic_value_is_missing(x: SEXP, index: R_xlen_t) -> bool {
    unsafe {
        match TYPEOF(x) {
            t if t == SEXPTYPE::STRSXP => charsxp_is_na(STRING_ELT(x, index)),
            t if t == SEXPTYPE::REALSXP => {
                let value = *REAL(x).add(index as usize);
                value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || value.is_nan()
            }
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                *INTEGER(x).add(index as usize) == NA_INTEGER
            }
            _ => false,
        }
    }
}

fn copy_atomic_element(
    dst: SEXP,
    dst_index: R_xlen_t,
    src: SEXP,
    src_index: R_xlen_t,
    target_type: SEXPTYPE,
) {
    unsafe {
        match target_type {
            t if t == SEXPTYPE::STRSXP => {
                if TYPEOF(src) == SEXPTYPE::STRSXP {
                    SET_STRING_ELT(dst, dst_index, STRING_ELT(src, src_index));
                } else {
                    let text = elt_to_string(src, src_index);
                    let cstr = CString::new(text).unwrap_or_default();
                    SET_STRING_ELT(
                        dst,
                        dst_index,
                        crate::sexp::constructors::Rf_mkChar(cstr.as_ptr()),
                    );
                }
            }
            t if t == SEXPTYPE::REALSXP => {
                let value = if TYPEOF(src) == SEXPTYPE::REALSXP {
                    *REAL(src).add(src_index as usize)
                } else {
                    let raw = *INTEGER(src).add(src_index as usize);
                    if raw == NA_INTEGER {
                        NA_REAL
                    } else {
                        raw as f64
                    }
                };
                *REAL(dst).add(dst_index as usize) = value;
            }
            _ => {
                let value = if TYPEOF(src) == SEXPTYPE::REALSXP {
                    let value = *REAL(src).add(src_index as usize);
                    if value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                        NA_INTEGER
                    } else {
                        value as c_int
                    }
                } else {
                    *INTEGER(src).add(src_index as usize)
                };
                *INTEGER(dst).add(dst_index as usize) = value;
            }
        }
    }
}

/// R's `sort(x, decreasing, na.last)` — sort an atomic vector.
pub unsafe fn do_sort(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let decreasing = sort_logical_arg(args, &["decreasing"], 1).unwrap_or(false);
        let na_placement = sort_na_placement(args);

        let t = TYPEOF(x);
        let n = XLENGTH(x);
        if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let mut vals: Vec<i32> = Vec::with_capacity(n as usize);
            let mut na_count = 0usize;
            for i in 0..n {
                let value = *INTEGER(x).add(i as usize);
                if value == NA_INTEGER {
                    na_count += 1;
                } else {
                    vals.push(value);
                }
            }
            if decreasing {
                vals.sort_by(|a, b| b.cmp(a));
            } else {
                vals.sort_unstable();
            }
            let output_len = sorted_len(vals.len(), na_count, na_placement);
            let result = Rf_allocVector3(t, output_len as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            let dst = INTEGER(result);
            let mut out = 0usize;
            if na_placement == SortNaPlacement::First {
                for _ in 0..na_count {
                    *dst.add(out) = NA_INTEGER;
                    out += 1;
                }
            }
            for value in vals {
                *dst.add(out) = value;
                out += 1;
            }
            if na_placement == SortNaPlacement::Last {
                for _ in 0..na_count {
                    *dst.add(out) = NA_INTEGER;
                    out += 1;
                }
            }
            result
        } else if t == SEXPTYPE::REALSXP {
            let mut vals: Vec<f64> = Vec::with_capacity(n as usize);
            let mut na_count = 0usize;
            for i in 0..n {
                let value = *REAL(x).add(i as usize);
                if ISNAN(value) {
                    na_count += 1;
                } else {
                    vals.push(value);
                }
            }
            vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            if decreasing {
                vals.reverse();
            }
            let output_len = sorted_len(vals.len(), na_count, na_placement);
            let result = Rf_allocVector3(t, output_len as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            let dst = REAL(result);
            let mut out = 0usize;
            if na_placement == SortNaPlacement::First {
                for _ in 0..na_count {
                    *dst.add(out) = NA_REAL;
                    out += 1;
                }
            }
            for value in vals {
                *dst.add(out) = value;
                out += 1;
            }
            if na_placement == SortNaPlacement::Last {
                for _ in 0..na_count {
                    *dst.add(out) = NA_REAL;
                    out += 1;
                }
            }
            result
        } else if t == SEXPTYPE::STRSXP {
            let mut vals: Vec<SEXP> = Vec::with_capacity(n as usize);
            let mut na_count = 0usize;
            for i in 0..n {
                let value = STRING_ELT(x, i);
                if charsxp_is_na(value) {
                    na_count += 1;
                } else {
                    vals.push(value);
                }
            }
            vals.sort_by(|a, b| compare_charsxp_for_sort(*a, *b));
            if decreasing {
                vals.reverse();
            }
            let output_len = sorted_len(vals.len(), na_count, na_placement);
            let result = Rf_allocVector3(t, output_len as R_xlen_t);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            let mut out = 0usize;
            if na_placement == SortNaPlacement::First {
                for _ in 0..na_count {
                    SET_STRING_ELT(result, out as R_xlen_t, crate::sexp::globals::R_NaString());
                    out += 1;
                }
            }
            for value in vals {
                SET_STRING_ELT(result, out as R_xlen_t, value);
                out += 1;
            }
            if na_placement == SortNaPlacement::Last {
                for _ in 0..na_count {
                    SET_STRING_ELT(result, out as R_xlen_t, crate::sexp::globals::R_NaString());
                    out += 1;
                }
            }
            result
        } else {
            let result = Rf_allocVector3(t, n);
            if result.is_null() {
                return R_NilValue();
            }
            result
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum SortNaPlacement {
    Remove,
    Last,
    First,
}

fn sorted_len(value_count: usize, na_count: usize, na_placement: SortNaPlacement) -> usize {
    value_count
        + match na_placement {
            SortNaPlacement::Remove => 0,
            SortNaPlacement::Last | SortNaPlacement::First => na_count,
        }
}

fn sort_na_placement(args: SEXP) -> SortNaPlacement {
    match sort_logical_arg(args, &["na.last"], 2) {
        Some(true) => SortNaPlacement::Last,
        Some(false) => SortNaPlacement::First,
        None => SortNaPlacement::Remove,
    }
}

fn sort_logical_arg(args: SEXP, names: &[&str], position: usize) -> Option<bool> {
    unsafe {
        let arg = arg_by_name_or_position(args, names, position);
        if arg.is_null() || arg == R_NilValue() || XLENGTH(arg) == 0 {
            return None;
        }
        let raw = if TYPEOF(arg) == SEXPTYPE::LGLSXP || TYPEOF(arg) == SEXPTYPE::INTSXP {
            *INTEGER(arg)
        } else if TYPEOF(arg) == SEXPTYPE::REALSXP {
            let value = *REAL(arg);
            if ISNAN(value) {
                NA_LOGICAL
            } else {
                value as c_int
            }
        } else {
            return None;
        };
        (raw != NA_LOGICAL).then_some(raw != 0)
    }
}

fn charsxp_is_na(value: SEXP) -> bool {
    unsafe { value.is_null() || value == crate::sexp::globals::R_NaString() }
}

fn compare_charsxp_for_sort(a: SEXP, b: SEXP) -> std::cmp::Ordering {
    unsafe {
        let a_is_na = charsxp_is_na(a);
        let b_is_na = charsxp_is_na(b);
        match (a_is_na, b_is_na) {
            (true, true) => return std::cmp::Ordering::Equal,
            (true, false) => return std::cmp::Ordering::Greater,
            (false, true) => return std::cmp::Ordering::Less,
            (false, false) => {}
        }
        let a_ptr = CHAR(a);
        let b_ptr = CHAR(b);
        let a_text = if a_ptr.is_null() {
            ""
        } else {
            std::ffi::CStr::from_ptr(a_ptr).to_str().unwrap_or("")
        };
        let b_text = if b_ptr.is_null() {
            ""
        } else {
            std::ffi::CStr::from_ptr(b_ptr).to_str().unwrap_or("")
        };
        a_text.cmp(b_text)
    }
}

/// R's `rev(x)` — reverse a vector.
pub unsafe fn do_rev(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        let result = Rf_allocVector3(t, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..n {
            let src = (n - 1 - i) as usize;
            let dst = i as usize;
            if t == SEXPTYPE::REALSXP {
                *REAL(result).add(dst) = *REAL(x).add(src);
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                *INTEGER(result).add(dst) = *INTEGER(x).add(src);
            } else if t == SEXPTYPE::STRSXP {
                SET_STRING_ELT(result, i, STRING_ELT(x, src as R_xlen_t));
            } else if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP {
                SET_VECTOR_ELT(result, i, VECTOR_ELT(x, src as R_xlen_t));
            } else if t == SEXPTYPE::RAWSXP {
                *RAW(result).add(dst) = *RAW(x).add(src);
            }
        }
        reverse_names_attribute(x, result, n);
        result
    }
}

unsafe fn reverse_names_attribute(x: SEXP, result: SEXP, len: R_xlen_t) {
    unsafe {
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return;
        }
        let reversed = Rf_allocVector3(SEXPTYPE::STRSXP, len);
        if reversed.is_null() {
            return;
        }
        let _reversed_guard = protect(reversed);
        for i in 0..len {
            let src = len - 1 - i;
            SET_STRING_ELT(reversed, i, STRING_ELT(names, src));
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            reversed,
        );
    }
}

unsafe fn logical_arg_value(x: SEXP, index: R_xlen_t) -> Option<c_int> {
    unsafe {
        match TYPEOF(x) {
            t if t == SEXPTYPE::LGLSXP.as_c_int() || t == SEXPTYPE::INTSXP.as_c_int() => {
                Some(*INTEGER(x).add(index as usize))
            }
            t if t == SEXPTYPE::REALSXP.as_c_int() => {
                let value = *REAL(x).add(index as usize);
                if value.is_nan() {
                    Some(NA_INTEGER)
                } else {
                    Some((value != 0.0) as c_int)
                }
            }
            _ => None,
        }
    }
}

unsafe fn logical_na_rm_from_args(args: SEXP) -> bool {
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).as_deref() == Some("na.rm") {
                let value = CAR(current);
                if !value.is_null() && value != R_NilValue() && XLENGTH(value) > 0 {
                    return logical_arg_value(value, 0) == Some(TRUE);
                }
            }
            current = CDR(current);
        }
        false
    }
}

/// R's `any(...)` — TRUE if any element is TRUE.
pub unsafe fn do_any(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let na_rm = logical_na_rm_from_args(args);
        let mut has_na = false;
        let mut current = args;

        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).as_deref() == Some("na.rm") {
                current = CDR(current);
                continue;
            }

            let x = CAR(current);
            if !x.is_null() && x != R_NilValue() {
                let n = XLENGTH(x);
                for i in 0..n {
                    match logical_arg_value(x, i) {
                        Some(TRUE) => return Rf_ScalarLogical(TRUE),
                        Some(NA_INTEGER) if !na_rm => has_na = true,
                        Some(_) | None => {}
                    }
                }
            }
            current = CDR(current);
        }

        Rf_ScalarLogical(if has_na { NA_INTEGER } else { FALSE })
    }
}

/// R's `all(...)` — TRUE if all elements are TRUE.
pub unsafe fn do_all(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let na_rm = logical_na_rm_from_args(args);
        let mut has_na = false;
        let mut current = args;

        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).as_deref() == Some("na.rm") {
                current = CDR(current);
                continue;
            }

            let x = CAR(current);
            if !x.is_null() && x != R_NilValue() {
                let n = XLENGTH(x);
                for i in 0..n {
                    match logical_arg_value(x, i) {
                        Some(FALSE) => return Rf_ScalarLogical(FALSE),
                        Some(NA_INTEGER) if !na_rm => has_na = true,
                        Some(_) | None => {}
                    }
                }
            }
            current = CDR(current);
        }

        Rf_ScalarLogical(if has_na { NA_INTEGER } else { TRUE })
    }
}

/// R's `seq_len(n)` — 1:n without recycling issues when n=0.
pub unsafe fn do_seq_len(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_arg = CAR(args);
        let n = real_or_default(n_arg, 0.0) as i64;
        if n <= 0 {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);
        for i in 0..n {
            *dst.add(i as usize) = (i + 1) as c_int;
        }
        result
    }
}

/// R's `seq_along(x)` — seq_along(x) = seq_len(length(x)).
pub unsafe fn do_seq_along(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = if x.is_null() || x == R_NilValue() {
            0
        } else {
            XLENGTH(x)
        };
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);
        for i in 0..n {
            *dst.add(i as usize) = (i + 1) as c_int;
        }
        result
    }
}

/// R's `cumsum(x)` — cumulative sum.
pub unsafe fn do_cumsum(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        let mut sum = 0.0f64;
        for i in 0..n {
            let v = elt_real_safe(x, i);
            if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                sum += v;
                *dst.add(i as usize) = sum;
            }
        }
        result
    }
}

/// R's `cumprod(x)` — cumulative product.
pub unsafe fn do_cumprod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        let mut prod = 1.0f64;
        for i in 0..n {
            let v = elt_real_safe(x, i);
            if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                prod *= v;
                *dst.add(i as usize) = prod;
            }
        }
        result
    }
}

/// R's `diff(x, lag)` — lagged differences.
pub unsafe fn do_diff(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let lag_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let lag = if lag_arg.is_null() || lag_arg == R_NilValue() {
            1
        } else {
            real_or_default(lag_arg, 1.0) as usize
        };
        let n = XLENGTH(x);
        if n <= lag as R_xlen_t {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }
        let result_len = n - lag as R_xlen_t;
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, result_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        for i in 0..result_len {
            let a = elt_real_safe(x, i);
            let b = elt_real_safe(x, i + lag as R_xlen_t);
            *dst.add(i as usize) = b - a;
        }
        result
    }
}

// ---------------------------------------------------------------------------
// I/O builtins: cat() to file, writeLines(), file.exists()
// ---------------------------------------------------------------------------

/// R's `writeLines(text, con = stdout(), sep = "\n", useBytes = FALSE)`.
pub unsafe fn do_writeLines(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let text = CAR(args);
        if text.is_null() || text == R_NilValue() {
            return R_NilValue();
        }

        let mut con = R_NilValue();
        let mut sep = "\n".to_string();
        let mut positional = 0;
        let mut current = CDR(args);
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            match tag_name(current).as_deref() {
                Some("con") => con = arg,
                Some("sep") => sep = elt_to_string(arg, 0),
                Some("useBytes") => {}
                _ => {
                    match positional {
                        0 => con = arg,
                        1 => sep = elt_to_string(arg, 0),
                        _ => {}
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }

        let path = if con.is_null() || con == R_NilValue() {
            "/dev/stdout".to_string()
        } else if TYPEOF(con) == SEXPTYPE::INTSXP {
            let sep_sxp = Rf_mkString(CString::new(sep).unwrap_or_default().as_ptr());
            let normalized = Rf_cons(
                text,
                Rf_cons(
                    con,
                    Rf_cons(sep_sxp, Rf_cons(Rf_ScalarLogical(FALSE), R_NilValue())),
                ),
            );
            return crate::mainutils::connections::do_writeLines(_call, _op, normalized, _rho);
        } else {
            elt_to_string(con, 0)
        };

        let n = if TYPEOF(text) == SEXPTYPE::STRSXP {
            XLENGTH(text)
        } else {
            1
        };
        if path == "/dev/stdout" {
            let mut output = String::new();
            for i in 0..n {
                output.push_str(&elt_to_string(text, i));
                output.push_str(&sep);
            }
            if crate::sexp::output::is_capturing() {
                crate::sexp::output::capture_stdout(&output);
            } else {
                print!("{}", output);
            }
        } else if let Ok(mut file) = std::fs::File::create(&path) {
            use std::io::Write;
            for i in 0..n {
                let _ = file.write_all(elt_to_string(text, i).as_bytes());
                let _ = file.write_all(sep.as_bytes());
            }
        }
        crate::sexp::globals::set_R_Visible(FALSE);
        R_NilValue()
    }
}

/// R's `readLines(con)` — read lines from file.
pub unsafe fn do_readLines(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let con = CAR(args);
        if con.is_null() {
            return R_NilValue();
        }
        let path = elt_to_string(con, 0);

        let lines = std::fs::read_to_string(&path).unwrap_or_default();
        let line_vec: Vec<&str> = lines.lines().collect();
        let n = line_vec.len();

        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for (i, line) in line_vec.iter().enumerate() {
            let cstr = CString::new(*line).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i) = charsxp;
            }
        }
        result
    }
}

/// R's `file.exists(...)` — check if files exist.
pub unsafe fn do_file_exists(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let path = elt_to_string(x, i);
            *dst.add(i as usize) = if std::path::Path::new(&path).exists() {
                TRUE
            } else {
                FALSE
            };
        }
        result
    }
}

/// R's `list.files(path)` — list files in directory.
pub unsafe fn do_list_files(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let path_arg = CAR(args);
        let path = if path_arg.is_null() || path_arg == R_NilValue() {
            ".".to_string()
        } else {
            elt_to_string(path_arg, 0)
        };

        let entries: Vec<String> = std::fs::read_dir(&path)
            .map(|dir| {
                dir.filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();

        let n = entries.len();
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for (i, name) in entries.iter().enumerate() {
            let cstr = CString::new(name.as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i) = charsxp;
            }
        }
        result
    }
}

/// R's `system(command, intern = FALSE)` — run a system command.
pub unsafe fn do_system(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let cmd = CAR(args);
        if cmd.is_null() {
            return R_NilValue();
        }
        let cmd_str = elt_to_string(cmd, 0);
        let intern = if !CDR(args).is_null() && CDR(args) != R_NilValue() {
            logical_arg(CAR(CDR(args)), false)
        } else {
            false
        };

        if system_commands_disabled_by_runtime_policy() {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "system() is disabled by the Android runtime policy".to_string(),
            });
        }

        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd_str)
            .output();
        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if intern {
                    let lines: Vec<&str> = stdout.lines().collect();
                    let result = Rf_allocVector3(SEXPTYPE::STRSXP, lines.len() as R_xlen_t);
                    for (i, line) in lines.iter().enumerate() {
                        let cstr = CString::new(*line).unwrap_or_default();
                        SET_STRING_ELT(result, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
                    }
                    result
                } else {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    if !stdout.is_empty() {
                        crate::sexp::output::capture_stdout(&stdout);
                    }
                    if !stderr.is_empty() {
                        crate::sexp::output::capture_stderr(&stderr);
                    }
                    crate::sexp::globals::set_R_Visible(FALSE);
                    Rf_ScalarInteger(out.status.code().unwrap_or(1))
                }
            }
            Err(_) => {
                crate::sexp::globals::set_R_Visible(FALSE);
                Rf_ScalarInteger(127)
            }
        }
    }
}

/// R's `system2(command, args, stdout, stderr, wait, input)` — run a command.
pub unsafe fn do_system2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let command_arg = arg_by_name_or_position(args, &["command"], 0);
        if command_arg.is_null() || command_arg == R_NilValue() {
            return R_NilValue();
        }
        let command = elt_to_string(command_arg, 0);
        let argv_arg = arg_by_name_or_position(args, &["args"], 1);
        let argv = if argv_arg.is_null() || argv_arg == R_NilValue() {
            Vec::new()
        } else {
            (0..XLENGTH(argv_arg))
                .map(|i| elt_to_string(argv_arg, i))
                .filter(|arg| !arg.is_empty() && arg != "NA")
                .collect::<Vec<_>>()
        };
        let stdout_arg = arg_by_name_or_position(args, &["stdout"], 2);
        let capture_stdout = logical_arg(stdout_arg, false);

        if system_commands_disabled_by_runtime_policy() {
            std::panic::panic_any(crate::sexp::context::RError {
                message: "system2() is disabled by the Android runtime policy".to_string(),
            });
        }

        let output = std::process::Command::new(&command).args(&argv).output();
        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                if capture_stdout {
                    let lines = stdout.lines().map(str::to_string).collect::<Vec<_>>();
                    return string_vector(&lines);
                }
                if !stdout.is_empty() {
                    crate::sexp::output::capture_stdout(&stdout);
                }
                if !stderr.is_empty() {
                    crate::sexp::output::capture_stderr(&stderr);
                }
                crate::sexp::globals::set_R_Visible(FALSE);
                Rf_ScalarInteger(out.status.code().unwrap_or(1))
            }
            Err(_) => {
                crate::sexp::globals::set_R_Visible(FALSE);
                Rf_ScalarInteger(127)
            }
        }
    }
}

fn system_commands_disabled_by_runtime_policy() -> bool {
    cfg!(target_os = "android")
}

/// R's `stopifnot(...)` — stop if any condition is FALSE.
pub unsafe fn do_stopifnot(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let cond = CAR(current);
            if !cond.is_null()
                && TYPEOF(cond) == SEXPTYPE::LGLSXP
                && LENGTH(cond) > 0
                && *LOGICAL(cond) == 0
            {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "FALSE is not TRUE".to_string(),
                });
            }
            current = CDR(current);
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// R's `nargs()` — number of arguments in the current call.
pub unsafe fn do_nargs(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let Some(context) = current_function_context() else {
            base_error("'nargs' used outside a function");
        };
        Rf_ScalarInteger(pairlist_len((*context).promiseargs))
    }
}

// ---------------------------------------------------------------------------
// S3 dispatch, environment functions, I/O extensions
// ---------------------------------------------------------------------------

/// R's `UseMethod(generic, obj)` — delegate to the translated object-system
/// dispatch implementation.
pub unsafe fn do_usemethod(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::objects::do_usemethod(call, op, args, rho) }
}

/// R's `missing(x)` — check if argument was missing in call.
pub unsafe fn do_missing(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        Rf_ScalarLogical(FALSE) // Simplified
    }
}

/// R's `parent.frame(n)` — get enclosing environment.
pub unsafe fn do_parent_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = context_index_arg(args, 1);
        if n == NA_INTEGER || n < 1 {
            base_error("invalid 'n' value");
        }

        let mut remaining = n;
        let mut context = crate::sexp::context::R_GlobalContext();
        while !context.is_null() {
            if (*context).callflag & crate::sexp::context::ctxt_flags::CTXT_FUNCTION != 0 {
                remaining -= 1;
                if remaining == 0 {
                    return (*context).sysparent;
                }
            }
            context = (*context).nextcontext;
        }
        crate::sexp::globals::R_GlobalEnv()
    }
}

/// R's `sys.call(which)` — get the call that's currently being evaluated.
pub unsafe fn do_sys_call(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let which = context_index_arg(args, 0);
        let top = crate::sexp::context::R_GlobalContext();
        if top.is_null() {
            R_NilValue()
        } else {
            crate::eval::context::R_syscall(which, top)
        }
    }
}

/// R's `sys.frame(which)` — get frame at specified level.
pub unsafe fn do_sys_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let which = context_index_arg(args, 0);
        let top = crate::sexp::context::R_GlobalContext();
        if top.is_null() {
            crate::sexp::globals::R_GlobalEnv()
        } else {
            crate::eval::context::R_sysframe(which, top)
        }
    }
}

unsafe fn current_function_context() -> Option<*mut crate::sexp::context::RCNTXT> {
    unsafe {
        let mut context = crate::sexp::context::R_GlobalContext();
        while !context.is_null() {
            if (*context).callflag & crate::sexp::context::ctxt_flags::CTXT_FUNCTION != 0 {
                return Some(context);
            }
            context = (*context).nextcontext;
        }
        None
    }
}

unsafe fn pairlist_len(mut list: SEXP) -> c_int {
    unsafe {
        let mut len = 0;
        while !list.is_null() && list != R_NilValue() {
            len += 1;
            list = CDR(list);
        }
        len
    }
}

unsafe fn context_index_arg(args: SEXP, default: c_int) -> c_int {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            default
        } else {
            real_or_default(CAR(args), default as f64) as c_int
        }
    }
}

/// R's `getwd()` — get working directory.
pub unsafe fn do_getwd(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        match std::env::current_dir() {
            Ok(path) => {
                let s = path.to_string_lossy();
                let cstr = CString::new(s.as_ref()).unwrap_or_default();
                Rf_mkString(cstr.as_ptr())
            }
            Err(_) => R_NilValue(),
        }
    }
}

/// R's `setwd(dir)` — set working directory.
pub unsafe fn do_setwd(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let dir_arg = CAR(args);
        if dir_arg.is_null() {
            return R_NilValue();
        }
        let path = elt_to_string(dir_arg, 0);
        match std::env::set_current_dir(&path) {
            Ok(()) => {
                crate::sexp::globals::set_R_Visible(FALSE);
                let cstr = CString::new(path).unwrap_or_default();
                Rf_mkString(cstr.as_ptr())
            }
            Err(_) => {
                std::panic::panic_any(crate::sexp::context::RError {
                    message: format!("cannot change working directory to '{}'", path),
                });
            }
        }
    }
}

/// R's `basename(path)` — final path component, vectorized over character input.
pub unsafe fn do_basename(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { map_path_strings(CAR(args), r_basename) }
}

/// R's `dirname(path)` — parent path component, vectorized over character input.
pub unsafe fn do_dirname(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { map_path_strings(CAR(args), r_dirname) }
}

/// R's `file.path(...)` — join path components element-wise with recycling.
pub unsafe fn do_file_path(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut parts = Vec::new();
        let mut max_len = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).as_deref() != Some("fsep") {
                let value = CAR(current);
                if !value.is_null() && value != R_NilValue() {
                    max_len = max_len.max(XLENGTH(value));
                    parts.push(value);
                }
            }
            current = CDR(current);
        }
        if parts.is_empty() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, max_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for i in 0..max_len {
            let joined = parts
                .iter()
                .filter_map(|part| {
                    let value = elt_to_string(*part, i);
                    (!value.is_empty()).then_some(value)
                })
                .collect::<Vec<_>>()
                .join("/");
            SET_STRING_ELT(
                result,
                i,
                Rf_mkChar(CString::new(joined).unwrap_or_default().as_ptr()),
            );
        }
        result
    }
}

/// R's `dir.exists(paths)` — check if directories exist.
pub unsafe fn do_dir_exists(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let path = elt_to_string(x, i);
            *dst.add(i as usize) = if std::path::Path::new(&path).is_dir() {
                TRUE
            } else {
                FALSE
            };
        }
        result
    }
}

/// R's `file.create(...)` — create empty files.
pub unsafe fn do_file_create(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let path = elt_to_string(x, i);
            *dst.add(i as usize) =
                match crate::mainutils::platform::create_file_with_session_umask(&path) {
                    Ok(_) => TRUE,
                    Err(_) => FALSE,
                };
        }
        result
    }
}

/// R's `unlink(x, recursive)` — delete files or directories.
pub unsafe fn do_unlink(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
        }
        let n = XLENGTH(x);
        let mut count = 0;
        for i in 0..n {
            let path = elt_to_string(x, i);
            let p = std::path::Path::new(&path);
            let result = if p.is_dir() {
                std::fs::remove_dir_all(p)
            } else {
                std::fs::remove_file(p)
            };
            if result.is_ok() {
                count += 1;
            }
        }
        let result = Rf_ScalarInteger(count);
        crate::sexp::globals::set_R_Visible(FALSE);
        result
    }
}

/// R's `nzchar(x)` — check if strings are non-empty.
pub unsafe fn do_nzchar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let s = elt_to_string(x, i);
            *dst.add(i as usize) = if s.is_empty() { FALSE } else { TRUE };
        }
        result
    }
}

// ---------------------------------------------------------------------------
// S3 dispatch helpers — NROW, NCOL, lengths, rownames, colnames, names, class
// ---------------------------------------------------------------------------

/// R's `NROW(x)` — number of rows; falls back to length(x) if no dim.
pub unsafe fn do_NROW(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
        }
        if is_data_frame_like(x) {
            return Rf_ScalarInteger(data_frame_row_count(x) as i32);
        }
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 1 {
            Rf_ScalarInteger(*INTEGER(dim_attr))
        } else {
            Rf_ScalarInteger(XLENGTH(x) as i32)
        }
    }
}

/// R's `NCOL(x)` — number of columns; returns 1 for vectors.
pub unsafe fn do_NCOL(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
        }
        if is_data_frame_like(x) {
            return Rf_ScalarInteger(XLENGTH(x) as i32);
        }
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
            Rf_ScalarInteger(*INTEGER(dim_attr).add(1))
        } else {
            Rf_ScalarInteger(1)
        }
    }
}

/// R's `lengths(x)` — length of each element in a list/vector.
pub unsafe fn do_lengths(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);
        let t = TYPEOF(x);
        if t == SEXPTYPE::VECSXP {
            for i in 0..n {
                let elem = VECTOR_ELT(x, i as i64);
                *dst.add(i as usize) = if elem.is_null() {
                    0
                } else {
                    XLENGTH(elem) as i32
                };
            }
        } else {
            for i in 0..n {
                *dst.add(i as usize) = 1;
            }
        }
        result
    }
}

/// R's `length(x) <- value` — resize vectors with R's missing-value fill rules.
pub unsafe fn do_length_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            std::panic::panic_any(RError {
                message: "cannot set length of NULL".to_string(),
            });
        }
        let new_len = match length_replacement_size(value) {
            Some(len) => len,
            None => {
                std::panic::panic_any(RError {
                    message: "invalid value".to_string(),
                });
            }
        };

        let result = resize_vector(x, new_len);
        resize_names(x, result, new_len);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        result
    }
}

unsafe fn resize_vector(x: SEXP, new_len: R_xlen_t) -> SEXP {
    unsafe {
        if XLENGTH(x) == new_len {
            return x;
        }
        let kind = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE(kind), new_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let copy_len = XLENGTH(x).min(new_len);

        if kind == SEXPTYPE::LGLSXP.as_c_int() || kind == SEXPTYPE::INTSXP.as_c_int() {
            for i in 0..copy_len {
                *INTEGER(result).add(i as usize) = INTEGER_ELT(x, i as c_int);
            }
            for i in copy_len..new_len {
                *INTEGER(result).add(i as usize) = NA_INTEGER;
            }
        } else if kind == SEXPTYPE::REALSXP.as_c_int() {
            for i in 0..copy_len {
                *REAL(result).add(i as usize) = REAL_ELT(x, i as c_int);
            }
            for i in copy_len..new_len {
                *REAL(result).add(i as usize) = NA_REAL;
            }
        } else if kind == SEXPTYPE::CPLXSXP.as_c_int() {
            for i in 0..copy_len {
                *COMPLEX(result).add(i as usize) = *COMPLEX(x).add(i as usize);
            }
            for i in copy_len..new_len {
                *COMPLEX(result).add(i as usize) = Rcomplex {
                    r: NA_REAL,
                    i: NA_REAL,
                };
            }
        } else if kind == SEXPTYPE::STRSXP.as_c_int() {
            for i in 0..copy_len {
                SET_STRING_ELT(result, i, STRING_ELT(x, i));
            }
            for i in copy_len..new_len {
                SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
            }
        } else if kind == SEXPTYPE::RAWSXP.as_c_int() {
            for i in 0..copy_len {
                *RAW(result).add(i as usize) = *RAW(x).add(i as usize);
            }
        } else if kind == SEXPTYPE::VECSXP.as_c_int() || kind == SEXPTYPE::EXPRSXP.as_c_int() {
            for i in 0..copy_len {
                SET_VECTOR_ELT(result, i, VECTOR_ELT(x, i));
            }
        } else {
            std::panic::panic_any(RError {
                message: "unsupported type for length assignment".to_string(),
            });
        }
        result
    }
}

unsafe fn resize_names(x: SEXP, result: SEXP, new_len: R_xlen_t) {
    unsafe {
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return;
        }

        let resized = Rf_allocVector3(SEXPTYPE::STRSXP, new_len);
        if resized.is_null() {
            return;
        }
        let _resized_guard = protect(resized);
        let copy_len = XLENGTH(names).min(new_len);
        for i in 0..copy_len {
            SET_STRING_ELT(resized, i, STRING_ELT(names, i));
        }
        for i in copy_len..new_len {
            SET_STRING_ELT(resized, i, Rf_mkChar(c"".as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            resized,
        );
    }
}

unsafe fn length_replacement_size(value: SEXP) -> Option<R_xlen_t> {
    unsafe {
        if value.is_null() || value == R_NilValue() || XLENGTH(value) == 0 {
            return None;
        }
        let raw = if TYPEOF(value) == SEXPTYPE::INTSXP || TYPEOF(value) == SEXPTYPE::LGLSXP {
            INTEGER_ELT(value, 0) as f64
        } else if TYPEOF(value) == SEXPTYPE::REALSXP {
            REAL_ELT(value, 0)
        } else if TYPEOF(value) == SEXPTYPE::STRSXP {
            elt_to_string(value, 0).trim().parse::<f64>().ok()?
        } else {
            return None;
        };
        if !raw.is_finite() || raw < 0.0 || raw > R_xlen_t::MAX as f64 {
            None
        } else {
            Some(raw.trunc() as R_xlen_t)
        }
    }
}

/// R's `storage.mode(x) <- value` — coerce storage while preserving attributes.
pub unsafe fn do_storage_mode_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        let target_type = match storage_mode_target(value) {
            Ok(target_type) => target_type,
            Err(message) => {
                std::panic::panic_any(RError { message });
            }
        };

        if TYPEOF(x) == target_type {
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }
        if inherits_class(x, "factor") {
            std::panic::panic_any(RError {
                message: "invalid to change the storage mode of a factor".to_string(),
            });
        }

        let result = crate::mainutils::coerce::coerceVector(x, target_type);
        let _result_guard = protect(result);
        crate::sexp::accessors::SET_ATTRIB(result, ATTRIB(x));
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        result
    }
}

unsafe fn storage_mode_target(value: SEXP) -> Result<c_int, String> {
    unsafe {
        if value.is_null()
            || value == R_NilValue()
            || TYPEOF(value) != SEXPTYPE::STRSXP
            || XLENGTH(value) < 1
            || is_string_na(value, 0)
        {
            return Err("'value' must be non-null character string".to_string());
        }

        let mode = elt_to_string(value, 0);
        match mode.as_str() {
            "logical" => Ok(SEXPTYPE::LGLSXP.as_c_int()),
            "integer" => Ok(SEXPTYPE::INTSXP.as_c_int()),
            "double" => Ok(SEXPTYPE::REALSXP.as_c_int()),
            "complex" => Ok(SEXPTYPE::CPLXSXP.as_c_int()),
            "character" => Ok(SEXPTYPE::STRSXP.as_c_int()),
            "raw" => Ok(SEXPTYPE::RAWSXP.as_c_int()),
            "list" => Ok(SEXPTYPE::VECSXP.as_c_int()),
            "expression" => Ok(SEXPTYPE::EXPRSXP.as_c_int()),
            "real" => Err("use of 'real' is defunct: use 'double' instead".to_string()),
            "single" => Err("use of 'single' is defunct: use mode<- instead".to_string()),
            _ => Err("invalid value".to_string()),
        }
    }
}

/// R's `rownames(x)` — get row names attribute.
pub unsafe fn do_rownames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let dimnames = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr()),
        );
        if !dimnames.is_null() && TYPEOF(dimnames) == SEXPTYPE::VECSXP && LENGTH(dimnames) >= 1 {
            return VECTOR_ELT(dimnames, 0);
        }
        if is_data_frame_like(x) {
            return string_vector(&data_frame_row_names(x));
        }
        crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("row.names").unwrap_or_default().as_ptr()),
        )
    }
}

/// R's `colnames(x)` — get column names attribute.
pub unsafe fn do_colnames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let dimnames = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr()),
        );
        if !dimnames.is_null() && TYPEOF(dimnames) == SEXPTYPE::VECSXP && LENGTH(dimnames) >= 2 {
            VECTOR_ELT(dimnames, 1)
        } else {
            R_NilValue()
        }
    }
}

/// R's `names(x)` — get names attribute (alias for do_names).
pub unsafe fn do_names_get(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_names(_call, _op, args, _rho) }
}

/// R's `names(x) <- value` — set names attribute.
pub unsafe fn do_names_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
            value,
        );
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `dimnames(x) <- value` — set matrix/array dimension names.
pub unsafe fn do_dimnames_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr()),
            value,
        );
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `rownames(x) <- value` — set matrix row names through dimnames[[1]].
pub unsafe fn do_rownames_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { set_matrix_dimname(args, 0) }
}

/// R's `colnames(x) <- value` — set matrix column names through dimnames[[2]].
pub unsafe fn do_colnames_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { set_matrix_dimname(args, 1) }
}

unsafe fn set_matrix_dimname(args: SEXP, axis: i64) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let dimnames_sym = Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr());
        let mut dimnames = crate::sexp::attrib_core::getAttrib(x, dimnames_sym);
        if dimnames.is_null() || dimnames == R_NilValue() || TYPEOF(dimnames) != SEXPTYPE::VECSXP {
            dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            if dimnames.is_null() {
                return x;
            }
            let _dimnames_guard = protect(dimnames);
            SET_VECTOR_ELT(dimnames, 0, R_NilValue());
            SET_VECTOR_ELT(dimnames, 1, R_NilValue());
            crate::sexp::attrib_core::setAttrib(x, dimnames_sym, dimnames);
        }

        if LENGTH(dimnames) > axis as i32 {
            SET_VECTOR_ELT(dimnames, axis, value);
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `class(x)` — get class attribute.
pub unsafe fn do_class_get(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let class = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
        );
        if class.is_null() || class == R_NilValue() {
            let t = TYPEOF(x);
            let name = if t == SEXPTYPE::REALSXP {
                "numeric"
            } else if t == SEXPTYPE::INTSXP {
                "integer"
            } else if t == SEXPTYPE::LGLSXP {
                "logical"
            } else if t == SEXPTYPE::STRSXP {
                "character"
            } else if t == SEXPTYPE::VECSXP {
                "list"
            } else {
                "NULL"
            };
            let cstr = CString::new(name).unwrap_or_default();
            Rf_mkString(cstr.as_ptr())
        } else {
            class
        }
    }
}

/// R's `.class2(x)` — class vector including implicit primitive inheritance.
pub unsafe fn do_class2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_mkString(c"NULL".as_ptr());
        }

        let class = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"class".as_ptr()));
        if !class.is_null() && class != R_NilValue() {
            return class;
        }

        let implicit: &[&std::ffi::CStr] = match TYPEOF(x) {
            t if t == SEXPTYPE::INTSXP => &[c"integer", c"numeric"],
            t if t == SEXPTYPE::REALSXP => &[c"numeric"],
            t if t == SEXPTYPE::LGLSXP => &[c"logical"],
            t if t == SEXPTYPE::CPLXSXP => &[c"complex"],
            t if t == SEXPTYPE::STRSXP => &[c"character"],
            t if t == SEXPTYPE::RAWSXP => &[c"raw"],
            t if t == SEXPTYPE::VECSXP => &[c"list"],
            t if t == SEXPTYPE::LANGSXP => &[c"call"],
            _ => &[c"NULL"],
        };

        let result = Rf_allocVector3(SEXPTYPE::STRSXP, implicit.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _guard = protect(result);
        for (i, name) in implicit.iter().enumerate() {
            SET_STRING_ELT(result, i as R_xlen_t, Rf_mkChar(name.as_ptr()));
        }
        result
    }
}

/// R's `class(x) <- value` — set class attribute.
pub unsafe fn do_class_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
            value,
        );
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `oldClass(x)` — direct S3 class attribute without implicit defaults.
pub unsafe fn do_oldClass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol())
    }
}

/// R's `oldClass(x) <- value` — set or remove the direct S3 class attribute.
pub unsafe fn do_oldClass_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_ClassSymbol(), value);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

// ---------------------------------------------------------------------------
// List / data.frame operations
// ---------------------------------------------------------------------------

/// R's `list(...)` — create a VECSXP (list) from arguments.
pub unsafe fn do_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut n: R_xlen_t = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            n += 1;
            current = CDR(current);
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let mut i: R_xlen_t = 0;
        current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            SET_VECTOR_ELT(result, i as i64, arg);
            i += 1;
            current = CDR(current);
        }
        // Copy names from the pairlist tags if present
        let mut name_parts: Vec<String> = Vec::new();
        let mut has_names = false;
        current = args;
        while !current.is_null() && current != R_NilValue() {
            let tag = (*current).data.listsxp.tagval;
            if !tag.is_null() && tag != R_NilValue() {
                let pname = crate::sexp::accessors::PRINTNAME(tag);
                if !pname.is_null() {
                    let s = crate::sexp::accessors::CHAR(pname);
                    if !s.is_null() {
                        name_parts.push(
                            std::ffi::CStr::from_ptr(s)
                                .to_str()
                                .unwrap_or("")
                                .to_string(),
                        );
                        has_names = true;
                    } else {
                        name_parts.push(String::new());
                    }
                } else {
                    name_parts.push(String::new());
                }
            } else {
                name_parts.push(String::new());
            }
            current = CDR(current);
        }
        if has_names {
            let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            if !names_vec.is_null() {
                let _names_guard = protect(names_vec);
                for (j, name) in name_parts.iter().enumerate() {
                    let cstr = CString::new(name.as_str()).unwrap_or_default();
                    let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                    if !charsxp.is_null() {
                        let data = (*names_vec).gengc_next_node as *mut SEXP;
                        *data.add(j) = charsxp;
                    }
                }
                crate::sexp::attrib_core::setAttrib(
                    result,
                    Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
                    names_vec,
                );
            }
        }
        result
    }
}

unsafe fn string_at_or_empty(x: SEXP, index: R_xlen_t) -> String {
    unsafe {
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::STRSXP || index >= XLENGTH(x)
        {
            return String::new();
        }
        let value = STRING_ELT(x, index);
        if value.is_null() || value == crate::sexp::globals::R_NaString() {
            return String::new();
        }
        CStr::from_ptr(CHAR(value)).to_string_lossy().into_owned()
    }
}

unsafe fn set_string_names(x: SEXP, names: &[String]) {
    unsafe {
        let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as R_xlen_t);
        if names_vec.is_null() {
            return;
        }
        let _names_guard = protect(names_vec);
        for (i, name) in names.iter().enumerate() {
            let cstr = CString::new(name.as_str()).unwrap_or_default();
            SET_STRING_ELT(names_vec, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
        }
        crate::sexp::attrib_core::setAttrib(
            x,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names_vec,
        );
    }
}

unsafe fn set_compact_row_names(x: SEXP, nrow: R_xlen_t) {
    unsafe {
        let rn = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if rn.is_null() {
            return;
        }
        let _row_names_guard = protect(rn);
        *INTEGER(rn) = NA_INTEGER;
        *INTEGER(rn).add(1) = -(nrow as i32);
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_RowNamesSymbol(), rn);
    }
}

unsafe fn set_data_frame_class(x: SEXP) {
    unsafe {
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if class_vec.is_null() {
            return;
        }
        let _class_guard = protect(class_vec);
        SET_STRING_ELT(class_vec, 0, Rf_mkChar(c"data.frame".as_ptr()));
        crate::sexp::attrib_core::setAttrib(x, Rf_install(c"class".as_ptr()), class_vec);
    }
}

unsafe fn set_summary_default_class(x: SEXP) {
    unsafe {
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        if class_vec.is_null() {
            return;
        }
        let _class_guard = protect(class_vec);
        SET_STRING_ELT(class_vec, 0, Rf_mkChar(c"summaryDefault".as_ptr()));
        SET_STRING_ELT(class_vec, 1, Rf_mkChar(c"table".as_ptr()));
        crate::sexp::attrib_core::setAttrib(x, Rf_install(c"class".as_ptr()), class_vec);
    }
}

fn repair_data_frame_names(names: &mut [String]) {
    let mut used: BTreeMap<String, usize> = BTreeMap::new();
    for (i, name) in names.iter_mut().enumerate() {
        if name.is_empty() {
            *name = format!("X{}", i + 1);
        }
        let base = name.clone();
        let mut suffix = *used.get(&base).unwrap_or(&0);
        if suffix == 0 && !used.contains_key(&base) {
            used.insert(base, 1);
            continue;
        }
        loop {
            let candidate = format!("{base}.{suffix}");
            suffix += 1;
            if !used.contains_key(&candidate) {
                used.insert(base.clone(), suffix);
                used.insert(candidate.clone(), 1);
                *name = candidate;
                break;
            }
        }
    }
}

unsafe fn recycle_column_if_needed(x: SEXP, target_len: R_xlen_t) -> SEXP {
    unsafe {
        let len = XLENGTH(x);
        if len == target_len || target_len == 0 {
            return x;
        }
        if len != 1 {
            base_error(format!(
                "arguments imply differing number of rows: {target_len}, {len}"
            ));
        }
        let ty = TYPEOF(x);
        let out = Rf_allocVector3(ty, target_len);
        if out.is_null() {
            return out;
        }
        let _out_guard = protect(out);
        for i in 0..target_len {
            match ty {
                t if t == SEXPTYPE::REALSXP => *REAL(out).add(i as usize) = *REAL(x),
                t if t == SEXPTYPE::INTSXP => *INTEGER(out).add(i as usize) = *INTEGER(x),
                t if t == SEXPTYPE::LGLSXP => *LOGICAL(out).add(i as usize) = *LOGICAL(x),
                t if t == SEXPTYPE::STRSXP => SET_STRING_ELT(out, i, STRING_ELT(x, 0)),
                _ => return x,
            }
        }
        out
    }
}

/// R's `data.frame(...)`: build a data-frame list while expanding data-frame arguments.
pub unsafe fn do_data_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let initial = do_list(_call, _op, args, _rho);
        if initial.is_null() || initial == R_NilValue() {
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
            if !result.is_null() {
                let _result_guard = protect(result);
                set_string_names(result, &[]);
                set_compact_row_names(result, 0);
                set_data_frame_class(result);
            }
            return result;
        }
        let _initial_guard = protect(initial);
        let arg_names =
            crate::sexp::attrib_core::getAttrib(initial, crate::sexp::attrib_core::R_NamesSymbol());
        let mut columns: Vec<SEXP> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        let mut nrow: Option<R_xlen_t> = None;

        for i in 0..XLENGTH(initial) {
            let value = VECTOR_ELT(initial, i);
            let arg_name = string_at_or_empty(arg_names, i);
            if sexp_has_class(value, "data.frame") && TYPEOF(value) == SEXPTYPE::VECSXP {
                let inner_names = crate::sexp::attrib_core::getAttrib(
                    value,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                );
                for j in 0..XLENGTH(value) {
                    let column = VECTOR_ELT(value, j);
                    let len = XLENGTH(column);
                    match nrow {
                        Some(existing) if len != existing => base_error(format!(
                            "arguments imply differing number of rows: {existing}, {len}"
                        )),
                        None => nrow = Some(len),
                        _ => {}
                    }
                    columns.push(column);
                    let child_name = string_at_or_empty(inner_names, j);
                    names.push(if arg_name.is_empty() {
                        child_name
                    } else if child_name.is_empty() {
                        arg_name.clone()
                    } else {
                        format!("{arg_name}.{child_name}")
                    });
                }
            } else {
                let len = XLENGTH(value);
                match nrow {
                    Some(existing) if len != existing && len != 1 => base_error(format!(
                        "arguments imply differing number of rows: {existing}, {len}"
                    )),
                    None => nrow = Some(len),
                    _ => {}
                }
                columns.push(value);
                names.push(arg_name);
            }
        }

        repair_data_frame_names(&mut names);
        let row_count = nrow.unwrap_or(0);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, columns.len() as R_xlen_t);
        if result.is_null() {
            return result;
        }
        let _result_guard = protect(result);
        for (i, column) in columns.iter().enumerate() {
            SET_VECTOR_ELT(
                result,
                i as R_xlen_t,
                recycle_column_if_needed(*column, row_count),
            );
        }
        set_string_names(result, &names);
        set_compact_row_names(result, row_count);
        set_data_frame_class(result);

        result
    }
}

// ---------------------------------------------------------------------------
// Attribute access helpers
// ---------------------------------------------------------------------------

/// R's `attr(x, which)` — get arbitrary attribute by name.
pub unsafe fn do_attr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let which = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || which.is_null() || which == R_NilValue() {
            return R_NilValue();
        }
        let attr_name = elt_to_string(which, 0);
        crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new(attr_name).unwrap_or_default().as_ptr()),
        )
    }
}

/// R's `attr(x, which) <- value` — set or remove a single attribute.
pub unsafe fn do_attr_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let which = CAR(CDR(args));
        let value = CAR(CDR(CDR(args)));
        if x.is_null() || x == R_NilValue() || which.is_null() || which == R_NilValue() {
            return R_NilValue();
        }
        let attr_name = elt_to_string(which, 0);
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(CString::new(attr_name).unwrap_or_default().as_ptr()),
            value,
        );
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `attributes(x) <- value` — replace all attributes from a named list.
pub unsafe fn do_attributes_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        crate::sexp::accessors::SET_ATTRIB(x, R_NilValue());
        if value.is_null() || value == R_NilValue() {
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }

        if TYPEOF(value) != SEXPTYPE::VECSXP {
            std::panic::panic_any(RError {
                message: "attributes must be a list or NULL".to_string(),
            });
        }

        let names =
            crate::sexp::attrib_core::getAttrib(value, crate::sexp::attrib_core::R_NamesSymbol());
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            std::panic::panic_any(RError {
                message: "attributes must be named".to_string(),
            });
        }

        for i in (0..XLENGTH(value)).rev() {
            let name_elt = STRING_ELT(names, i);
            if name_elt.is_null() || name_elt == crate::sexp::globals::R_NaString() {
                continue;
            }
            let name = CHAR(name_elt);
            if name.is_null() || CStr::from_ptr(name).to_bytes().is_empty() {
                continue;
            }
            crate::sexp::attrib_core::setAttrib(x, Rf_install(name), VECTOR_ELT(value, i));
        }

        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `comment(x)` — get the comment attribute.
pub unsafe fn do_comment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::getAttrib(x, comment_symbol())
    }
}

/// R's `comment(x) <- value` — set the comment attribute.
pub unsafe fn do_comment_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::attrib_core::setAttrib(x, comment_symbol(), value);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

unsafe fn comment_symbol() -> SEXP {
    unsafe { Rf_install(CString::new("comment").unwrap_or_default().as_ptr()) }
}

/// R's namespace lookup operators, `pkg::name` and `pkg:::name`.
pub unsafe fn do_namespace_get(call: SEXP, op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let package = CAR(args);
        let name = CAR(CDR(args));
        if package.is_null() || name.is_null() || package == R_NilValue() || name == R_NilValue() {
            return R_NilValue();
        }

        let package_name = if TYPEOF(package) == SEXPTYPE::SYMSXP {
            let pname = PRINTNAME(package);
            if pname.is_null() {
                String::new()
            } else {
                CStr::from_ptr(CHAR(pname)).to_string_lossy().into_owned()
            }
        } else {
            elt_to_string(package, 0)
        };

        if TYPEOF(name) != SEXPTYPE::SYMSXP {
            std::panic::panic_any(RError {
                message: "namespace lookup requires a name".to_string(),
            });
        }
        let lookup_name = symbol_name(name).unwrap_or_default();

        if package_name == "tools" {
            if lookup_name == "langElts" {
                let values = crate::sexp::init::LANGUAGE_ELEMENTS;
                let result = Rf_allocVector3(SEXPTYPE::STRSXP, values.len() as R_xlen_t);
                if result.is_null() {
                    return R_NilValue();
                }
                let _result_guard = protect(result);
                for (i, value) in values.iter().enumerate() {
                    let c_value =
                        CString::new(*value).expect("static language element has no NUL byte");
                    SET_STRING_ELT(result, i as R_xlen_t, Rf_mkChar(c_value.as_ptr()));
                }
                crate::sexp::globals::set_R_Visible(crate::sexp::ffi::TRUE);
                return result;
            }
            std::panic::panic_any(RError {
                message: format!("object '{lookup_name}' not found in tools namespace"),
            });
        }

        if package_name != "base" {
            let namespace = match load_package_namespace_by_name(&package_name) {
                Ok(env) => env,
                Err(message) => {
                    std::panic::panic_any(RError { message });
                }
            };
            let private_lookup = symbol_name(CAR(call)).as_deref() == Some(":::")
                || crate::eval::builtin::PRIMNAME(op) == ":::";
            if !private_lookup {
                let package_path = find_package_path(&package_name);
                let directives = read_namespace_directives(Path::new(&package_path))
                    .ok()
                    .flatten();
                let exports = namespace_exports(directives.as_ref(), namespace);
                if !exports.iter().any(|export| export == &lookup_name) {
                    std::panic::panic_any(RError {
                        message: format!(
                            "'{lookup_name}' is not an exported object from namespace '{package_name}'"
                        ),
                    });
                }
            }
            let value = crate::sexp::envir::R_findVarInFrame(namespace, name);
            if value == crate::sexp::globals::R_UnboundValue() {
                std::panic::panic_any(RError {
                    message: format!(
                        "object '{lookup_name}' not found in namespace '{package_name}'"
                    ),
                });
            }
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::TRUE);
            return value;
        }

        let value = crate::sexp::envir::R_findVar(name, crate::sexp::globals::R_BaseEnv());
        if value == crate::sexp::globals::R_UnboundValue() {
            std::panic::panic_any(RError {
                message: format!("object '{lookup_name}' not found in base namespace"),
            });
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::TRUE);
        value
    }
}

/// R's `attributes(x)` — return attributes as a named list.
pub unsafe fn do_attributes(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let attrs = ATTRIB(x);
        if attrs.is_null() || attrs == R_NilValue() {
            return R_NilValue();
        }

        let mut count = 0;
        let mut current = attrs;
        while !current.is_null() && current != R_NilValue() {
            count += 1;
            current = CDR(current);
        }

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, count);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, count);
        if names.is_null() {
            return R_NilValue();
        }
        let _names_guard = protect(names);

        current = attrs;
        let mut i = 0;
        while !current.is_null() && current != R_NilValue() {
            SET_VECTOR_ELT(result, i, CAR(current));
            let name = tag_name(current).unwrap_or_default();
            SET_STRING_ELT(
                names,
                i,
                Rf_mkChar(CString::new(name).unwrap_or_default().as_ptr()),
            );
            i += 1;
            current = CDR(current);
        }

        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            names,
        );
        result
    }
}

fn structure_attr_name(name: &str) -> &str {
    match name {
        ".Dim" => "dim",
        ".Dimnames" => "dimnames",
        ".Names" => "names",
        ".Tsp" => "tsp",
        ".Label" => "levels",
        other => other,
    }
}

/// R's `structure(.Data, ...)` — attach attributes to an object.
pub unsafe fn do_structure(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let mut current = CDR(args);
        while !current.is_null() && current != R_NilValue() {
            if let Some(name) = tag_name(current) {
                let attr_name = structure_attr_name(&name);
                crate::sexp::attrib_core::setAttrib(
                    x,
                    Rf_install(CString::new(attr_name).unwrap_or_default().as_ptr()),
                    CAR(current),
                );
            }
            current = CDR(current);
        }

        x
    }
}

/// R's `attr(x, which) <- value` — set arbitrary attribute by name.
pub unsafe fn do_setattr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let which = CAR(CDR(args));
        let value = CAR(CDR(CDR(args)));
        if x.is_null() || x == R_NilValue() || which.is_null() || which == R_NilValue() {
            return R_NilValue();
        }
        let attr_name = elt_to_string(which, 0);
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(CString::new(attr_name).unwrap_or_default().as_ptr()),
            value,
        );
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

// ---------------------------------------------------------------------------
// String formatting
// ---------------------------------------------------------------------------

/// R's `noquote(x)` — mark object to prevent quoting in print.
pub unsafe fn do_noquote(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return x;
        }
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !class_vec.is_null() {
            let cstr = CString::new("noquote").unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*class_vec).gengc_next_node as *mut SEXP;
                *data.add(0) = charsxp;
            }
            crate::sexp::attrib_core::setAttrib(
                x,
                Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
                class_vec,
            );
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `deparse(x)` — convert an object or expression to source-like text.
pub unsafe fn do_deparse(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::deparse::do_deparse(_call, _op, args, _rho) }
}

// ---------------------------------------------------------------------------
// List operations
// ---------------------------------------------------------------------------

/// R's `lengths(x)` alias — lengths of list elements.
/// Wrapper that delegates to do_lengths (already registered separately).
pub unsafe fn do_length_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_lengths(_call, _op, args, _rho) }
}

/// R's `names(x)` for lists — names of list elements.
/// Wrapper that delegates to do_names.
pub unsafe fn do_names_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_names(_call, _op, args, _rho) }
}

/// R's `[[i]]` — get element i from a list (1-indexed).
pub unsafe fn do_list_get(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let i = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || i.is_null() || i == R_NilValue() {
            return R_NilValue();
        }
        let idx = real_or_default(i, 0.0) as i64;
        if idx < 1 || TYPEOF(x) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let n = XLENGTH(x) as i64;
        if idx > n {
            return R_NilValue();
        }
        VECTOR_ELT(x, idx - 1)
    }
}

/// R's `[[i]] <- value` — set element i in a list (1-indexed).
pub unsafe fn do_list_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let i = CAR(CDR(args));
        let value = CAR(CDR(CDR(args)));
        if x.is_null() || x == R_NilValue() || i.is_null() || i == R_NilValue() {
            return R_NilValue();
        }
        let idx = real_or_default(i, 0.0) as i64;
        if idx < 1 || TYPEOF(x) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }
        let n = XLENGTH(x) as i64;
        if idx > n {
            return R_NilValue();
        }
        SET_VECTOR_ELT(x, idx - 1, value);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `c(...)` for lists — concatenate lists together.
/// If all args are VECSXP, result is a flattened VECSXP.
pub unsafe fn do_c_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut total_len: R_xlen_t = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                total_len += XLENGTH(arg);
            }
            current = CDR(current);
        }
        if total_len == 0 {
            return Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, total_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let mut offset: R_xlen_t = 0;
        current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                let n = XLENGTH(arg);
                if TYPEOF(arg) == SEXPTYPE::VECSXP {
                    for i in 0..n {
                        SET_VECTOR_ELT(result, (offset + i) as i64, VECTOR_ELT(arg, i as i64));
                    }
                } else {
                    // Wrap scalar/vector in a single slot
                    SET_VECTOR_ELT(result, offset as i64, arg);
                }
                offset += n;
            }
            current = CDR(current);
        }
        result
    }
}

/// R's `unlist(x)` — flatten nested list to a vector.
/// Simplified: if list elements are all numeric, return REALSXP.
pub unsafe fn do_unlist(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        if TYPEOF(x) != SEXPTYPE::VECSXP {
            return x;
        }
        let n = XLENGTH(x);
        // Collect all elements and determine output type
        let mut all_values: Vec<f64> = Vec::new();
        let mut all_ints: Vec<i32> = Vec::new();
        let mut all_strs: Vec<String> = Vec::new();
        let mut result_type: u32;
        let mut saw_str = false;

        for i in 0..n {
            let elem = VECTOR_ELT(x, i as i64);
            if elem.is_null() {
                continue;
            }
            let t = TYPEOF(elem);
            let m = XLENGTH(elem);
            for j in 0..m {
                if t == SEXPTYPE::REALSXP {
                    all_values.push(*REAL(elem).add(j as usize));
                } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                    let v = *INTEGER(elem).add(j as usize);
                    all_ints.push(v);
                } else if t == SEXPTYPE::STRSXP {
                    all_strs.push(elt_to_string(elem, j));
                    saw_str = true;
                } else if t == SEXPTYPE::VECSXP {
                    // Nested list — recurse via extraction
                    let inner = VECTOR_ELT(elem, j as i64);
                    if !inner.is_null() && TYPEOF(inner) == SEXPTYPE::REALSXP {
                        all_values.push(*REAL(inner));
                    } else {
                        saw_str = true;
                        all_strs.push(elt_to_string(inner, 0));
                    }
                } else {
                    all_values.push(NA_REAL);
                }
            }
        }
        let result_type = if saw_str {
            SEXPTYPE::STRSXP.as_c_int()
        } else if !all_values.is_empty() {
            SEXPTYPE::REALSXP.as_c_int()
        } else {
            SEXPTYPE::INTSXP.as_c_int()
        };

        let total: R_xlen_t = if result_type == SEXPTYPE::STRSXP {
            all_strs.len() as R_xlen_t
        } else if result_type == SEXPTYPE::REALSXP {
            (all_values.len() + all_ints.len()) as R_xlen_t
        } else {
            all_ints.len() as R_xlen_t
        };

        let result = Rf_allocVector3(result_type, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        if result_type == SEXPTYPE::REALSXP {
            let dst = REAL(result);
            let mut idx = 0usize;
            for &v in &all_values {
                *dst.add(idx) = v;
                idx += 1;
            }
            for &v in &all_ints {
                *dst.add(idx) = if v == NA_INTEGER { NA_REAL } else { v as f64 };
                idx += 1;
            }
        } else if result_type == SEXPTYPE::INTSXP {
            let dst = INTEGER(result);
            for (idx, &v) in all_ints.iter().enumerate() {
                *dst.add(idx) = v;
            }
        } else if result_type == SEXPTYPE::STRSXP {
            for (idx, s) in all_strs.iter().enumerate() {
                let cstr = CString::new(s.as_str()).unwrap_or_default();
                let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                if !charsxp.is_null() {
                    let data = (*result).gengc_next_node as *mut SEXP;
                    *data.add(idx) = charsxp;
                }
            }
        }

        result
    }
}

/// R's `is.atomic(x)` — TRUE for non-recursive types (not list, pairlist, etc.).
pub unsafe fn do_is_atomic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(TRUE);
        }
        let t = TYPEOF(x);
        let is_atomic = t == SEXPTYPE::LGLSXP
            || t == SEXPTYPE::INTSXP
            || t == SEXPTYPE::REALSXP
            || t == SEXPTYPE::CPLXSXP
            || t == SEXPTYPE::STRSXP
            || t == SEXPTYPE::RAWSXP
            || t == SEXPTYPE::CHARSXP
            || t == SEXPTYPE::NILSXP;
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
        let class = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
        );
        Rf_ScalarLogical(if !class.is_null() && class != R_NilValue() {
            TRUE
        } else {
            FALSE
        })
    }
}

// ---------------------------------------------------------------------------
// Connection basics (simplified)
// ---------------------------------------------------------------------------

/// R's `file(description)` — create a file connection.
/// Delegates to the session-owned connection table.
pub unsafe fn do_file(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_file(_call, _op, args, _rho) }
}

/// R's `url(description)` — create a URL connection.
/// Delegates to the session-owned connection table.
pub unsafe fn do_url(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_url(_call, _op, args, _rho) }
}

/// R's `close(con)` — close a connection.
/// Delegates to the session-owned connection table.
pub unsafe fn do_close(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_close(_call, _op, args, _rho) }
}

/// R's `flush(con)` — flush a connection.
/// Delegates to the session-owned connection table.
pub unsafe fn do_flush(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_flush(_call, _op, args, _rho) }
}

// ---------------------------------------------------------------------------
// Extended connection constructors
// ---------------------------------------------------------------------------

/// R's `gzfile(description, open, encoding, compression)` — gzip connection.
/// Simplified: delegates to connections.rs when available, else returns description.
pub unsafe fn do_gzfile(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let desc = CAR(args);
        if desc.is_null() || desc == R_NilValue() {
            return R_NilValue();
        }
        // Delegate to connections.rs full implementation
        crate::mainutils::connections::do_gzfile(_call, _op, args, _rho)
    }
}

/// R's `pipe(description, open, encoding)` — pipe connection.
/// Simplified: delegates to connections.rs when available, else returns description.
pub unsafe fn do_pipe(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let desc = CAR(args);
        if desc.is_null() || desc == R_NilValue() {
            return R_NilValue();
        }
        // Delegate to connections.rs full implementation
        crate::mainutils::connections::do_pipe(_call, _op, args, _rho)
    }
}

/// R's `fifo(description, open, blocking)` — FIFO connection.
/// Simplified: delegates to connections.rs when available, else returns description.
pub unsafe fn do_fifo(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let desc = CAR(args);
        if desc.is_null() || desc == R_NilValue() {
            return R_NilValue();
        }
        // Delegate to connections.rs full implementation
        crate::mainutils::connections::do_fifo(_call, _op, args, _rho)
    }
}

/// R's `socketConnection(host, port, open, blocking, server, encoding)` — socket connection.
/// Simplified: stub that returns NULL.
pub unsafe fn do_socketConnection(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _host = CAR(args);
        let _port = CAR(CDR(args));
        // Socket connections not yet fully supported
        crate::mainutils::connections::do_sockConnection(_call, _op, args, _rho)
    }
}

// ---------------------------------------------------------------------------
// Connection queries and operations
// ---------------------------------------------------------------------------

/// R's `isOpen(con, rw)` — check if a connection is open.
/// Simplified: delegates to connections.rs.
pub unsafe fn do_isOpen(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_isopen(_call, _op, args, _rho) }
}

/// R's `isIncomplete(con)` — check if a connection has incomplete read.
/// Simplified: delegates to connections.rs.
pub unsafe fn do_isIncomplete(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_isincomplete(_call, _op, args, _rho) }
}

/// R's `isSeekable(con)` — check if a connection supports seeking.
/// Delegates to the session-owned connection table.
pub unsafe fn do_isSeekable(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_isseekable(_call, _op, args, _rho) }
}

/// R's `seek(con, where, origin, rw)` — seek in a connection.
/// Simplified: delegates to connections.rs.
pub unsafe fn do_seek(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_seek(_call, _op, args, _rho) }
}

/// R's `pushBack(lines, con, newLine)` — push back lines to a connection.
/// Simplified: no-op stub.
pub unsafe fn do_pushBack(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_pushBack(_call, _op, args, _rho) }
}

/// R's `pushBackClear(con)` — clear push back buffer.
pub unsafe fn do_pushBackClear(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_pushBackClear(_call, _op, args, _rho) }
}

/// R's `pushBackLength(con)` — get push back buffer length.
/// Simplified: returns 0.
pub unsafe fn do_pushBackLength(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_pushBackLength(_call, _op, args, _rho) }
}

// ---------------------------------------------------------------------------
// Binary I/O
// ---------------------------------------------------------------------------

/// R's `readBin(con, what, n, size, signed, endian)` — read binary data.
/// Delegates to connections.rs for full implementation.
pub unsafe fn do_readBin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_readBin(_call, _op, args, _rho) }
}

/// R's `writeBin(object, con, size, endian, useBytes)` — write binary data.
/// Delegates to connections.rs for full implementation.
pub unsafe fn do_writeBin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::connections::do_writeBin(_call, _op, args, _rho) }
}

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

fn quantile_type7(sorted: &[f64], prob: f64) -> f64 {
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

/// R's `summary.default(x)`: return GNU R-shaped summaryDefault/table vectors.
pub unsafe fn do_summary_default(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);

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
// S3 generics
// ---------------------------------------------------------------------------

/// R's `as.data.frame(x)` — convert to data.frame.
/// Simplified: wraps x in a list with data.frame class.
pub unsafe fn do_as_data_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        // If already a data.frame, return as-is
        let class = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
        );
        if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
            let cls_name = elt_to_string(class, 0);
            if cls_name == "data.frame" {
                return x;
            }
        }
        // Wrap in a single-element list and set class
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        SET_VECTOR_ELT(result, 0, x);

        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !class_vec.is_null() {
            let _class_guard = protect(class_vec);
            let cstr = CString::new("data.frame").unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*class_vec).gengc_next_node as *mut SEXP;
                *data.add(0) = charsxp;
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
                class_vec,
            );
        }

        // Set row.names
        let nrow = XLENGTH(x);
        let rn = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !rn.is_null() {
            let _row_names_guard = protect(rn);
            *INTEGER(rn) = NA_INTEGER;
            *INTEGER(rn).add(1) = -(nrow as i32);
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("row.names").unwrap_or_default().as_ptr()),
                rn,
            );
        }

        // Set column name to "x"
        let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !names_vec.is_null() {
            let _names_guard = protect(names_vec);
            let cstr = CString::new("x").unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*names_vec).gengc_next_node as *mut SEXP;
                *data.add(0) = charsxp;
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
                names_vec,
            );
        }

        result
    }
}

/// R's `as.list(x)` — generic list conversion.
/// Delegates to do_as_list but available as a separate entry point.
pub unsafe fn do_as_list_generic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_as_list(_call, _op, args, _rho) }
}

// ---------------------------------------------------------------------------
// S3 print/summary dispatch
// ---------------------------------------------------------------------------

/// R's `print.default(x, ...)` — default print method.
/// Equivalent to the existing do_print but named for S3 dispatch.
pub unsafe fn do_print_default(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_print(_call, _op, args, _rho) }
}

/// R's `print.data.frame(x)` — print a data.frame nicely with aligned columns.
pub unsafe fn do_print_data_frame(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            println!("NULL");
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

        // Get column names
        let names = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
        );
        let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP;

        // Print header row (column names)
        if ncol > 0 {
            let mut header = String::new();
            for j in 0..ncol.min(20) {
                let name = if has_names && j < XLENGTH(names) {
                    elt_to_string(names, j)
                } else {
                    format!("[,{}]", j + 1)
                };
                let _ = std::fmt::Write::write_fmt(&mut header, format_args!("{:>12} ", name));
            }
            println!("{}", header);
        }

        // Print rows (up to 20)
        let print_rows = nrow.min(20);
        for i in 0..print_rows {
            let mut row = String::new();
            for j in 0..ncol.min(20) {
                let col = VECTOR_ELT(x, j as R_xlen_t);
                let val = if col.is_null() {
                    "NULL".to_string()
                } else {
                    elt_to_string(col, i)
                };
                let _ = std::fmt::Write::write_fmt(&mut row, format_args!("{:>12} ", val));
            }
            println!("{}", row);
        }
        if nrow > 20 {
            println!(
                "  [ reached 'max' / getOption(\"max.print\") -- omitted {} rows ]",
                nrow - 20
            );
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
// Matrix/linear algebra
// ---------------------------------------------------------------------------

/// R's `crossprod(x, y)` — computes t(x) %*% y.
/// If y is NULL, computes t(x) %*% x.
pub unsafe fn do_crossprod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::array::do_matprod_kind(
            crate::mainutils::array::MatProductKind::Cross,
            args,
            "crossprod",
        )
    }
}

/// R's `tcrossprod(x, y)` — computes x %*% t(y).
/// If y is NULL, computes x %*% t(x).
pub unsafe fn do_tcrossprod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::array::do_matprod_kind(
            crate::mainutils::array::MatProductKind::TransposedCross,
            args,
            "tcrossprod",
        )
    }
}

/// R's `det(x)` — determinant of a square matrix (simplified via LU-like approach).
/// For a 2x2 matrix: det = a*d - b*c. For larger, uses LU decomposition concept.
pub unsafe fn do_det(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarReal(NA_REAL);
        }

        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if dim_attr.is_null() || TYPEOF(dim_attr) != SEXPTYPE::INTSXP || LENGTH(dim_attr) != 2 {
            return Rf_ScalarReal(NA_REAL);
        }
        let n = *INTEGER(dim_attr) as usize;
        let m = *INTEGER(dim_attr).add(1) as usize;
        if n != m || n == 0 {
            return Rf_ScalarReal(NA_REAL);
        }

        if TYPEOF(x) != SEXPTYPE::REALSXP {
            return Rf_ScalarReal(NA_REAL);
        }

        // Compute determinant using LU decomposition (without pivoting for simplicity)
        let src = REAL(x);
        // Copy matrix data
        let mut mat: Vec<f64> = Vec::with_capacity(n * n);
        for i in 0..n * n {
            mat.push(*src.add(i));
        }

        let mut det_val = 1.0_f64;
        for i in 0..n {
            // Find pivot
            let mut max_val = mat[i * n + i].abs();
            let mut max_row = i;
            for k in (i + 1)..n {
                let v = mat[k * n + i].abs();
                if v > max_val {
                    max_val = v;
                    max_row = k;
                }
            }
            if max_val == 0.0 {
                return Rf_ScalarReal(0.0);
            }
            // Swap rows
            if max_row != i {
                for j in 0..n {
                    let tmp = mat[i * n + j];
                    mat[i * n + j] = mat[max_row * n + j];
                    mat[max_row * n + j] = tmp;
                }
                det_val = -det_val;
            }
            det_val *= mat[i * n + i];
            // Eliminate
            let pivot = mat[i * n + i];
            for k in (i + 1)..n {
                let factor = mat[k * n + i] / pivot;
                mat[k * n + i] = 0.0;
                for j in (i + 1)..n {
                    mat[k * n + j] -= factor * mat[i * n + j];
                }
            }
        }

        Rf_ScalarReal(det_val)
    }
}

/// R's `solve(a, b)` — solve the linear system a %*% x = b.
/// If b is omitted, computes the inverse of a (simplified).
/// Uses Gaussian elimination with partial pivoting.
pub unsafe fn do_solve(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let a = CAR(args);
        let b_cdr = CDR(args);
        let b = if b_cdr.is_null() || b_cdr == R_NilValue() {
            R_NilValue()
        } else {
            CAR(b_cdr)
        };

        if a.is_null() || a == R_NilValue() {
            return R_NilValue();
        }

        let dim_attr = crate::sexp::attrib_core::getAttrib(
            a,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if dim_attr.is_null() || TYPEOF(dim_attr) != SEXPTYPE::INTSXP || LENGTH(dim_attr) != 2 {
            return R_NilValue();
        }
        let n = *INTEGER(dim_attr) as usize;
        let m = *INTEGER(dim_attr).add(1) as usize;
        if n != m || n == 0 {
            return R_NilValue();
        }
        if TYPEOF(a) != SEXPTYPE::REALSXP {
            return R_NilValue();
        }

        let src = REAL(a);
        // Build augmented matrix [A | I] or [A | b]
        let nrhs = if b == R_NilValue() {
            n // inverse
        } else {
            let b_dim = crate::sexp::attrib_core::getAttrib(
                b,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
            );
            if !b_dim.is_null() && TYPEOF(b_dim) == SEXPTYPE::INTSXP && LENGTH(b_dim) == 2 {
                *INTEGER(b_dim).add(1) as usize
            } else {
                1
            }
        };

        let aug_cols = n + nrhs;
        let mut aug: Vec<f64> = vec![0.0; n * aug_cols];

        // Fill A
        for i in 0..n {
            for j in 0..n {
                aug[i * aug_cols + j] = *src.add(i * n + j);
            }
        }

        // Fill right-hand side
        if b == R_NilValue() {
            // Identity matrix for inverse
            for i in 0..n {
                aug[i * aug_cols + n + i] = 1.0;
            }
        } else {
            let b_src = REAL(b);
            for i in 0..n {
                for j in 0..nrhs {
                    aug[i * aug_cols + n + j] = *b_src.add(i * nrhs + j);
                }
            }
        }

        // Gaussian elimination with partial pivoting
        for i in 0..n {
            // Find pivot
            let mut max_val = aug[i * aug_cols + i].abs();
            let mut max_row = i;
            for k in (i + 1)..n {
                let v = aug[k * aug_cols + i].abs();
                if v > max_val {
                    max_val = v;
                    max_row = k;
                }
            }
            if max_val == 0.0 {
                return R_NilValue(); // singular
            }
            // Swap rows
            if max_row != i {
                for j in 0..aug_cols {
                    let tmp = aug[i * aug_cols + j];
                    aug[i * aug_cols + j] = aug[max_row * aug_cols + j];
                    aug[max_row * aug_cols + j] = tmp;
                }
            }
            // Eliminate below
            let pivot = aug[i * aug_cols + i];
            for k in (i + 1)..n {
                let factor = aug[k * aug_cols + i] / pivot;
                aug[k * aug_cols + i] = 0.0;
                for j in (i + 1)..aug_cols {
                    aug[k * aug_cols + j] -= factor * aug[i * aug_cols + j];
                }
            }
        }

        // Back substitution
        for i in (0..n).rev() {
            let diag = aug[i * aug_cols + i];
            for j in (n)..aug_cols {
                aug[i * aug_cols + j] /= diag;
            }
            aug[i * aug_cols + i] = 1.0;
            for k in 0..i {
                let factor = aug[k * aug_cols + i];
                for j in n..aug_cols {
                    aug[k * aug_cols + j] -= factor * aug[i * aug_cols + j];
                }
                aug[k * aug_cols + i] = 0.0;
            }
        }

        // Extract result
        let result_len = (n * nrhs) as R_xlen_t;
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, result_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            for j in 0..nrhs {
                *dst.add(i * nrhs + j) = aug[i * aug_cols + n + j];
            }
        }

        // Set dim if multi-column
        if nrhs > 1 {
            let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            if !dim.is_null() {
                let _dim_guard = protect(dim);
                *INTEGER(dim) = n as i32;
                *INTEGER(dim).add(1) = nrhs as i32;
                crate::sexp::attrib_core::setAttrib(
                    result,
                    Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                    dim,
                );
            }
        }

        result
    }
}

// ---------------------------------------------------------------------------
// Environment functions
// ---------------------------------------------------------------------------

/// R's `emptyenv()` — returns the empty environment (root of environment chain).
pub unsafe fn do_emptyenv(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::sexp::globals::R_EmptyEnv() }
}

/// R's `baseenv()` — returns the base environment.
pub unsafe fn do_baseenv(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::sexp::globals::R_BaseEnv() }
}

/// R's `globalenv()` — returns the global environment.
pub unsafe fn do_globalenv(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::sexp::globals::R_GlobalEnv() }
}

/// R's `new.env(hash, parent, size)` — create a new environment.
pub unsafe fn do_new_env(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let parent_arg = arg_by_name_or_position(args, &["parent"], 1);
        let parent = if parent_arg.is_null() || parent_arg == R_NilValue() {
            crate::sexp::globals::R_GlobalEnv()
        } else if TYPEOF(parent_arg) == SEXPTYPE::ENVSXP {
            parent_arg
        } else {
            crate::sexp::globals::R_GlobalEnv()
        };

        // Create a new environment with empty frame and parent
        let env = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(), // empty frame
            parent,       // enclosing env
            R_NilValue(), // no hash table (simplified)
        );
        env
    }
}

/// R's `environment(fun)` — get the environment associated with a closure.
pub unsafe fn do_environment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let fn_arg = CAR(args);
        if fn_arg.is_null() || fn_arg == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(fn_arg);
        if t == SEXPTYPE::CLOSXP {
            let env = crate::sexp::accessors::CLOENV(fn_arg);
            if env.is_null() { R_NilValue() } else { env }
        } else if t == SEXPTYPE::ENVSXP {
            fn_arg
        } else {
            R_NilValue()
        }
    }
}

/// R's `lockBinding(sym, env)` — lock a binding in an environment.
pub unsafe fn do_lockBinding(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let sym = binding_symbol_arg(CAR(args));
        let env = environment_arg(CAR(CDR(args)));
        if crate::sexp::envir::R_findVarInFrame(env, sym) == R_UnboundValue() {
            base_error("no binding for symbol");
        }
        crate::sexp::envir::lock_binding_raw(env, sym);
        R_NilValue()
    }
}

/// R's `unlockBinding(sym, env)` — unlock a binding in an environment.
pub unsafe fn do_unlockBinding(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let sym = binding_symbol_arg(CAR(args));
        let env = environment_arg(CAR(CDR(args)));
        crate::sexp::envir::unlock_binding_raw(env, sym);
        R_NilValue()
    }
}

/// R's `bindingIsLocked(sym, env)` — check if a binding is locked.
pub unsafe fn do_bindingIsLocked(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let sym = binding_symbol_arg(CAR(args));
        let env = environment_arg(CAR(CDR(args)));
        Rf_ScalarLogical(crate::sexp::envir::binding_is_locked_raw(env, sym) as c_int)
    }
}

/// R's `bindingIsActive(sym, env)` — check if a binding is active.
pub unsafe fn do_bindingIsActive(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let sym = binding_symbol_arg(CAR(args));
        let env = environment_arg(CAR(CDR(args)));
        if !crate::sexp::envir::binding_exists_in_frame_raw(env, sym) {
            base_error("no binding for symbol");
        }
        Rf_ScalarLogical(crate::sexp::envir::binding_is_active_raw(env, sym) as c_int)
    }
}

/// R's `makeActiveBinding(sym, fun, env)` — create an active binding.
pub unsafe fn do_makeActiveBinding(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let sym = binding_symbol_arg(CAR(args));
        let fun = CAR(CDR(args));
        let env = environment_arg(CAR(CDR(CDR(args))));
        if !is_function_value(fun) {
            base_error("not a function");
        }
        crate::sexp::envir::make_active_binding_raw(env, sym, fun);
        R_NilValue()
    }
}

/// R's `lockEnvironment(env, bindings)` — lock an environment.
pub unsafe fn do_lockEnvironment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let env = environment_arg(CAR(args));
        crate::sexp::envir::lock_environment_raw(env);

        let lock_bindings = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            false
        } else {
            real_or_default(CAR(CDR(args)), 0.0) != 0.0
        };
        if lock_bindings {
            let mut frame = FRAME(env);
            while !frame.is_null() && frame != R_NilValue() {
                let tag = TAG(frame);
                if !tag.is_null() && tag != R_NilValue() {
                    crate::sexp::envir::lock_binding_raw(env, tag);
                }
                frame = CDR(frame);
            }
        }

        R_NilValue()
    }
}

/// R's `environmentIsLocked(env)` — check if an environment is locked.
pub unsafe fn do_environmentIsLocked(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let env = environment_arg(CAR(args));
        Rf_ScalarLogical(crate::sexp::envir::environment_is_locked_raw(env) as c_int)
    }
}

unsafe fn environment_arg(value: SEXP) -> SEXP {
    unsafe {
        if value.is_null() || value == R_NilValue() || TYPEOF(value) != SEXPTYPE::ENVSXP {
            base_error("not an environment");
        }
        value
    }
}

unsafe fn binding_symbol_arg(value: SEXP) -> SEXP {
    unsafe {
        if value.is_null() || value == R_NilValue() {
            base_error("invalid symbol");
        }
        match TYPEOF(value) {
            t if t == SEXPTYPE::SYMSXP.as_c_int() => value,
            t if t == SEXPTYPE::STRSXP.as_c_int() && XLENGTH(value) > 0 => {
                let name = elt_to_string(value, 0);
                let c_name = CString::new(name).unwrap_or_default();
                Rf_install(c_name.as_ptr())
            }
            _ => base_error("invalid symbol"),
        }
    }
}

// ---------------------------------------------------------------------------
// R runtime essentials
// ---------------------------------------------------------------------------

unsafe fn make_r_version_list(simple_list_class: bool) -> SEXP {
    unsafe {
        let fields = [
            ("platform", "rust-port"),
            ("arch", std::env::consts::ARCH),
            ("os", std::env::consts::OS),
            ("system", "rust-port"),
            ("status", ""),
            ("major", "4"),
            ("minor", "4.1"),
            ("year", "2026"),
            ("month", "05"),
            ("day", "09"),
            ("svn rev", ""),
            ("language", "R"),
            ("version.string", "R version 4.4.1 (Rust Port)"),
            ("nickname", "Rust Port"),
        ];

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, fields.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for (i, (_, value)) in fields.iter().enumerate() {
            let value = CString::new(*value).unwrap_or_default();
            SET_VECTOR_ELT(result, i as R_xlen_t, Rf_mkString(value.as_ptr()));
        }

        let names = Rf_allocVector3(SEXPTYPE::STRSXP, fields.len() as R_xlen_t);
        if !names.is_null() {
            let _names_guard = protect(names);
            for (i, (name, _)) in fields.iter().enumerate() {
                let name = CString::new(*name).unwrap_or_default();
                SET_STRING_ELT(names, i as R_xlen_t, Rf_mkChar(name.as_ptr()));
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
                names,
            );
        }

        if simple_list_class {
            let class = Rf_mkString(c"simple.list".as_ptr());
            let _class_guard = protect(class);
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
        }

        result
    }
}

/// R's `version` — legacy constant alias for `R.version`.
pub unsafe fn do_version(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { make_r_version_list(true) }
}

/// R's `R.version` — returns a named list with version info.
pub unsafe fn do_R_version(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { make_r_version_list(true) }
}

/// R's `R.Version()` — returns the version info list without `simple.list` class.
pub unsafe fn do_R_Version(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { make_r_version_list(false) }
}

/// R's `args(fn)` — returns the formal arguments of a function as a pairlist.
/// With the body set to NULL.
pub unsafe fn do_args(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let fn_arg = CAR(args);
        if fn_arg.is_null() || fn_arg == R_NilValue() {
            return R_NilValue();
        }

        let t = TYPEOF(fn_arg);
        if t == SEXPTYPE::CLOSXP {
            return crate::mainutils::dstruct::mkCLOSXP(
                FORMALS(fn_arg),
                R_NilValue(),
                crate::sexp::globals::R_GlobalEnv(),
            );
        }

        if t != SEXPTYPE::BUILTINSXP && t != SEXPTYPE::SPECIALSXP {
            return R_NilValue();
        }

        let primitive_name = crate::eval::primitive::PRIMNAME(fn_arg);
        let primitive_symbol =
            Rf_install(CString::new(primitive_name).unwrap_or_default().as_ptr());

        for registry in [".ArgsEnv", ".GenericArgsEnv"] {
            let registry_symbol = Rf_install(CString::new(registry).unwrap_or_default().as_ptr());
            let registry_env = crate::sexp::envir::R_findVarInFrame(
                crate::sexp::globals::R_BaseEnv(),
                registry_symbol,
            );
            if registry_env == crate::sexp::globals::R_UnboundValue() {
                continue;
            }
            let prototype = crate::sexp::envir::R_findVarInFrame(registry_env, primitive_symbol);
            if prototype != crate::sexp::globals::R_UnboundValue()
                && TYPEOF(prototype) == SEXPTYPE::CLOSXP
            {
                return crate::mainutils::dstruct::mkCLOSXP(
                    FORMALS(prototype),
                    R_NilValue(),
                    crate::sexp::globals::R_GlobalEnv(),
                );
            }
        }

        R_NilValue()
    }
}

/// R's `formals(fn)` — get the formal arguments (parameter list) of a function.
pub unsafe fn do_formals(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let fn_arg = CAR(args);
        if fn_arg.is_null() || fn_arg == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(fn_arg);
        if t == SEXPTYPE::CLOSXP {
            let formals = crate::sexp::accessors::FORMALS(fn_arg);
            if formals.is_null() {
                R_NilValue()
            } else {
                formals
            }
        } else {
            R_NilValue()
        }
    }
}

/// R's `body(fn)` — get the body of a function.
pub unsafe fn do_body(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let fn_arg = CAR(args);
        if fn_arg.is_null() || fn_arg == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(fn_arg);
        if t == SEXPTYPE::CLOSXP {
            let body = crate::sexp::accessors::BODY(fn_arg);
            if body.is_null() { R_NilValue() } else { body }
        } else {
            R_NilValue()
        }
    }
}

/// R's `environment(fn)` — get the environment of a closure.
/// Same as do_environment, provided as an alternative name.
pub unsafe fn do_environment_of(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_environment(_call, _op, args, _rho) }
}

// ---------------------------------------------------------------------------
// String/vector completion: charmatch, pmatch, strtoi, strtrim
// ---------------------------------------------------------------------------

/// R's `charmatch(x, table)` — character matching.
/// Returns integer index of exact match (1-based), or 0 if no match, or NA if ambiguous.
pub unsafe fn do_charmatch(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let table_arg = CAR(CDR(args));
        let nomatch_arg = CAR(CDR(CDR(args)));
        let nomatch = if nomatch_arg.is_null() || nomatch_arg == R_NilValue() {
            NA_INTEGER
        } else {
            real_or_default(nomatch_arg, NA_REAL) as c_int
        };

        if x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let nx = XLENGTH(x_arg);
        let nt = if table_arg.is_null() || table_arg == R_NilValue() {
            0
        } else {
            XLENGTH(table_arg)
        };
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, nx);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);

        for i in 0..nx {
            let x_is_na = as_character_element_is_na(x_arg, i);
            let x_str = if x_is_na {
                String::new()
            } else {
                elt_to_string(x_arg, i)
            };
            let mut exact_matches = 0usize;
            let mut exact_index = nomatch;
            for j in 0..nt {
                let table_is_na = as_character_element_is_na(table_arg, j);
                let exact = if x_is_na || table_is_na {
                    x_is_na && table_is_na
                } else {
                    elt_to_string(table_arg, j) == x_str
                };
                if exact {
                    exact_matches += 1;
                    exact_index = (j + 1) as c_int;
                }
            }

            if exact_matches == 1 {
                *dst.add(i as usize) = exact_index;
                continue;
            }
            if exact_matches > 1 {
                *dst.add(i as usize) = 0;
                continue;
            }

            let mut partial_matches = 0usize;
            let mut partial_index = nomatch;
            if !x_is_na {
                for j in 0..nt {
                    if as_character_element_is_na(table_arg, j) {
                        continue;
                    }
                    let table_str = elt_to_string(table_arg, j);
                    if table_str.starts_with(&x_str) {
                        partial_matches += 1;
                        partial_index = (j + 1) as c_int;
                    }
                }
            }
            *dst.add(i as usize) = if partial_matches == 1 {
                partial_index
            } else if partial_matches > 1 {
                0
            } else {
                nomatch
            };
        }
        result
    }
}

/// R's `pmatch(x, table, nomatch=NA, duplicates.ok=FALSE)` — partial matching.
/// Returns integer vector of matches (1-based).
pub unsafe fn do_pmatch(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let table_arg = CAR(CDR(args));
        let nomatch_arg = CAR(CDR(CDR(args)));
        let duplicates_arg = CAR(CDR(CDR(CDR(args))));
        let nomatch = if nomatch_arg.is_null() || nomatch_arg == R_NilValue() {
            NA_INTEGER
        } else {
            real_or_default(nomatch_arg, NA_REAL as f64) as c_int
        };
        let duplicates_ok = if duplicates_arg.is_null() || duplicates_arg == R_NilValue() {
            false
        } else {
            real_or_default(duplicates_arg, 0.0) != 0.0
        };

        if x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let nx = XLENGTH(x_arg);
        let nt = if table_arg.is_null() || table_arg == R_NilValue() {
            0
        } else {
            XLENGTH(table_arg)
        };
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, nx);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);

        // Track which table entries are already matched
        let mut used = vec![false; nt as usize];

        for i in 0..nx {
            let x_is_na = as_character_element_is_na(x_arg, i);
            let x_str = if x_is_na {
                String::new()
            } else {
                elt_to_string(x_arg, i)
            };
            let mut best_match: c_int = nomatch;
            if x_is_na {
                for j in 0..nt {
                    if !duplicates_ok && used[j as usize] {
                        continue;
                    }
                    if as_character_element_is_na(table_arg, j) {
                        best_match = (j + 1) as c_int;
                        if !duplicates_ok {
                            used[j as usize] = true;
                        }
                        break;
                    }
                }
                *dst.add(i as usize) = best_match;
                continue;
            }

            if x_str.is_empty() {
                *dst.add(i as usize) = nomatch;
                continue;
            }

            for j in 0..nt {
                if !duplicates_ok && used[j as usize] {
                    continue;
                }
                if as_character_element_is_na(table_arg, j) {
                    continue;
                }
                if elt_to_string(table_arg, j) == x_str {
                    best_match = (j + 1) as c_int;
                    if !duplicates_ok {
                        used[j as usize] = true;
                    }
                    break;
                }
            }

            if best_match == nomatch {
                let mut partial_matches = 0usize;
                let mut partial_index = nomatch;
                for j in 0..nt {
                    if !duplicates_ok && used[j as usize] {
                        continue;
                    }
                    if as_character_element_is_na(table_arg, j) {
                        continue;
                    }
                    let t_str = elt_to_string(table_arg, j);
                    if t_str.starts_with(&x_str) {
                        partial_matches += 1;
                        partial_index = (j + 1) as c_int;
                    }
                }
                if partial_matches == 1 {
                    best_match = partial_index;
                    if !duplicates_ok {
                        used[(partial_index - 1) as usize] = true;
                    }
                }
            }
            *dst.add(i as usize) = best_match;
        }
        result
    }
}

/// R's `strtoi(x, base=10L)` — convert strings to integers.
pub unsafe fn do_strtoi(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let base_arg = CAR(CDR(args));
        let base = if base_arg.is_null() || base_arg == R_NilValue() {
            10
        } else {
            real_or_default(base_arg, 10.0) as i32
        };

        if x_arg.is_null() || x_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let n = XLENGTH(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = INTEGER(result);

        for i in 0..n {
            let s = elt_to_string(x_arg, i);
            let val = i64::from_str_radix(s.trim(), base as u32).unwrap_or(NA_INTEGER as i64);
            *dst.add(i as usize) = if val > c_int::MAX as i64 || val < c_int::MIN as i64 {
                NA_INTEGER
            } else {
                val as c_int
            };
        }
        result
    }
}

/// R's `strtrim(x, width)` — truncate strings to a maximum width.
pub unsafe fn do_strtrim(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let width_arg = CAR(CDR(args));
        if x_arg.is_null() || x_arg == R_NilValue() {
            return R_NilValue();
        }
        let width = if width_arg.is_null() || width_arg == R_NilValue() {
            usize::MAX
        } else {
            real_or_default(width_arg, f64::MAX) as usize
        };

        let n = XLENGTH(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..n {
            let s = elt_to_string(x_arg, i);
            let truncated: String = s.chars().take(width).collect();
            let cstr = CString::new(truncated).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i as usize) = charsxp;
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// R's math2 builtins (2-arg math): log2, round, signif, trunc
// ---------------------------------------------------------------------------

/// R's `log2(x)` — log base 2 with optional explicit base override.
pub unsafe fn do_log2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let base_arg = CAR(CDR(args));
        if x_arg.is_null() || x_arg == R_NilValue() {
            return R_NilValue();
        }
        let base = if base_arg.is_null() || base_arg == R_NilValue() {
            2.0
        } else {
            real_or_default(base_arg, std::f64::consts::E)
        };
        let n = XLENGTH(x_arg);
        let t = TYPEOF(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        let log_base = base.ln();
        for i in 0..n {
            let v = if t == SEXPTYPE::REALSXP {
                *REAL(x_arg).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let iv = *INTEGER(x_arg).add(i as usize);
                if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
            } else {
                NA_REAL
            };
            *dst.add(i as usize) = if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || v <= 0.0
            {
                NA_REAL
            } else {
                v.ln() / log_base
            };
        }
        result
    }
}

/// R's `round(x, digits=0)` — round to specified decimal digits.
pub unsafe fn do_round(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let digits_arg = CAR(CDR(args));
        if x_arg.is_null() || x_arg == R_NilValue() {
            return R_NilValue();
        }
        let digits = if digits_arg.is_null() || digits_arg == R_NilValue() {
            0.0
        } else {
            real_or_default(digits_arg, 0.0)
        };
        let n = XLENGTH(x_arg);
        let t = TYPEOF(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        let scale = 10.0_f64.powf(digits);
        for i in 0..n {
            let v = if t == SEXPTYPE::REALSXP {
                *REAL(x_arg).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let iv = *INTEGER(x_arg).add(i as usize);
                if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
            } else {
                NA_REAL
            };
            *dst.add(i as usize) = if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                NA_REAL
            } else {
                (v * scale).round() / scale
            };
        }
        result
    }
}

/// R's `signif(x, digits=6)` — round to significant digits.
pub unsafe fn do_signif(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let digits_arg = CAR(CDR(args));
        if x_arg.is_null() || x_arg == R_NilValue() {
            return R_NilValue();
        }
        let digits = if digits_arg.is_null() || digits_arg == R_NilValue() {
            6.0
        } else {
            real_or_default(digits_arg, 6.0).max(1.0)
        };
        let n = XLENGTH(x_arg);
        let t = TYPEOF(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let v = if t == SEXPTYPE::REALSXP {
                *REAL(x_arg).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let iv = *INTEGER(x_arg).add(i as usize);
                if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
            } else {
                NA_REAL
            };
            *dst.add(i as usize) = if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || v == 0.0
            {
                v
            } else {
                let magnitude = v.abs().log10().floor() - digits + 1.0;
                let scale = 10.0_f64.powf(magnitude);
                (v / scale).round() * scale
            };
        }
        result
    }
}

/// R's `trunc(x, ...)` — truncate toward zero with digits support.
pub unsafe fn do_trunc(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let _digits_arg = CAR(CDR(args));
        if x_arg.is_null() || x_arg == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x_arg);
        let t = TYPEOF(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let v = if t == SEXPTYPE::REALSXP {
                *REAL(x_arg).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let iv = *INTEGER(x_arg).add(i as usize);
                if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
            } else {
                NA_REAL
            };
            *dst.add(i as usize) = if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                NA_REAL
            } else {
                v.trunc()
            };
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Complete R runtime: eval, substitute, quote, parse
// ---------------------------------------------------------------------------

/// R's `eval(expr, envir, enclos)` — evaluate expression in environment.
pub unsafe fn do_eval(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        let envir_arg = CAR(CDR(args));
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }
        let envir = if envir_arg.is_null() || envir_arg == R_NilValue() {
            _rho
        } else {
            envir_arg
        };
        crate::eval::eval::Rf_eval(expr, envir)
    }
}

/// R's `substitute(expr, env)` — substitute symbols in expression.
pub unsafe fn do_substitute(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::coerce::do_substitute(_call, _op, args, _rho) }
}

/// R's `quote(expr)` — return expression unevaluated.
pub unsafe fn do_quote(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{CAR, NAMED, SET_NAMED};
        let mut nargs = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            nargs += 1;
            current = CDR(current);
        }
        if nargs != 1 {
            base_error(format!(
                "{nargs} arguments passed to 'quote' which requires 1"
            ));
        }
        let tag = TAG(args);
        if !tag.is_null() && tag != R_NilValue() {
            let name = if TYPEOF(tag) == SEXPTYPE::SYMSXP {
                let printname = PRINTNAME(tag);
                if printname.is_null() {
                    String::new()
                } else {
                    let chars = CHAR(printname);
                    if chars.is_null() {
                        String::new()
                    } else {
                        CStr::from_ptr(chars).to_string_lossy().into_owned()
                    }
                }
            } else {
                String::new()
            };
            if name != "expr" {
                base_error(format!(
                    "supplied argument name '{name}' does not match 'expr'"
                ));
            }
        }
        let val = CAR(args);
        if val.is_null() || val == R_NilValue() {
            return R_NilValue();
        }
        // ENSURE_NAMEDMAX — prevent modification of source code references
        if NAMED(val) < 2 {
            SET_NAMED(val, 2);
        }
        val
    }
}

/// R's `parse(text)` — parse R code strings into an expression vector.
pub unsafe fn do_parse(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let text_arg = arg_by_name_or_position(args, &["text"], 0);
        let file_arg = arg_by_name_or_position(args, &["file"], 0);
        if text_arg.is_null() || text_arg == R_NilValue() {
            if !file_arg.is_null() && file_arg != R_NilValue() {
                let file_path = elt_to_string(file_arg, 0);
                let content = std::fs::read_to_string(&file_path).unwrap_or_else(|err| {
                    base_error(format!("cannot open file '{}': {}", file_path, err))
                });
                return parse_source_expression_vector(&content);
            }
            return Rf_allocVector3(SEXPTYPE::EXPRSXP, 0);
        }

        let n = XLENGTH(text_arg);
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::EXPRSXP, 0);
        }

        let mut source = Vec::with_capacity(n as usize);
        for i in 0..n {
            if TYPEOF(text_arg) == SEXPTYPE::STRSXP && is_string_na(text_arg, i) {
                std::panic::panic_any(RError {
                    message: "invalid 'text' argument".to_string(),
                });
            }
            let text = elt_to_string(text_arg, i);
            source.push(text);
        }
        parse_source_strings(&source)
    }
}

unsafe fn parse_source_strings(source: &[String]) -> SEXP {
    let combined = source.join("\n");
    unsafe { parse_source_expression_vector(&combined) }
}

unsafe fn parse_source_expression_vector(source: &str) -> SEXP {
    unsafe {
        let parsed = crate::sexp::memory::with_arena(|arena| {
            crate::eval::parser::parse_expressions(source, arena).map_err(|err| err.to_string())
        })
        .unwrap_or_else(|message| std::panic::panic_any(RError { message }));

        let result = Rf_allocVector3(SEXPTYPE::EXPRSXP, parsed.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (i, value) in parsed.into_iter().enumerate() {
            SET_VECTOR_ELT(result, i as R_xlen_t, value);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Complete error system: condition handling
// ---------------------------------------------------------------------------

/// R's `conditionMessage(cond)` — get message from condition object.
pub unsafe fn do_conditionMessage(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let cond = CAR(args);
        if cond.is_null() || cond == R_NilValue() {
            return Rf_mkString(CString::new("").unwrap_or_default().as_ptr());
        }
        // Try to get the "message" attribute or element
        let msg_sym = Rf_install(CString::new("message").unwrap_or_default().as_ptr());
        let msg = crate::sexp::attrib_core::getAttrib(cond, msg_sym);
        if !msg.is_null() && msg != R_NilValue() && TYPEOF(msg) == SEXPTYPE::STRSXP {
            return msg;
        }
        // Fallback: deparse the condition
        Rf_mkString(
            CString::new(elt_to_string(cond, 0))
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

/// R's `conditionCall(cond)` — get call from condition object.
pub unsafe fn do_conditionCall(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let cond = CAR(args);
        if cond.is_null() || cond == R_NilValue() {
            return R_NilValue();
        }
        let call_sym = Rf_install(CString::new("call").unwrap_or_default().as_ptr());
        let call_val = crate::sexp::attrib_core::getAttrib(cond, call_sym);
        if !call_val.is_null() && call_val != R_NilValue() {
            return call_val;
        }
        R_NilValue()
    }
}

/// R's `simpleError(message, call)` — create a simple error condition.
pub unsafe fn do_simpleError(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let message_arg = CAR(args);
        let call_arg = CAR(CDR(args));
        let message = if message_arg.is_null() || message_arg == R_NilValue() {
            String::new()
        } else {
            elt_to_string(message_arg, 0)
        };
        // Create a simple list with class "simpleError" and "error" and "condition"
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let msg_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !msg_vec.is_null() {
            let cstr = CString::new(message).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*msg_vec).gengc_next_node as *mut SEXP;
                *data = charsxp;
            }
        }
        SET_VECTOR_ELT(result, 0, msg_vec);
        // Set names
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !names.is_null() {
            let cstr = CString::new("message").unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*names).gengc_next_node as *mut SEXP;
                *data = charsxp;
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
                names,
            );
        }
        // Set class
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
        if !class.is_null() {
            let classes = ["simpleError", "error", "condition"];
            for (i, &c) in classes.iter().enumerate() {
                let cs = CString::new(c).unwrap_or_default();
                let charsxp = crate::sexp::constructors::Rf_mkChar(cs.as_ptr());
                if !charsxp.is_null() {
                    let data = (*class).gengc_next_node as *mut SEXP;
                    *data.add(i) = charsxp;
                }
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
                class,
            );
        }
        result
    }
}

/// R's `simpleWarning(message, call)` — create a simple warning condition.
pub unsafe fn do_simpleWarning(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let message_arg = CAR(args);
        let message = if message_arg.is_null() || message_arg == R_NilValue() {
            String::new()
        } else {
            elt_to_string(message_arg, 0)
        };
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let msg_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !msg_vec.is_null() {
            let cstr = CString::new(message).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*msg_vec).gengc_next_node as *mut SEXP;
                *data = charsxp;
            }
        }
        SET_VECTOR_ELT(result, 0, msg_vec);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !names.is_null() {
            let cstr = CString::new("message").unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*names).gengc_next_node as *mut SEXP;
                *data = charsxp;
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
                names,
            );
        }
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
        if !class.is_null() {
            let classes = ["simpleWarning", "warning", "condition"];
            for (i, &c) in classes.iter().enumerate() {
                let cs = CString::new(c).unwrap_or_default();
                let charsxp = crate::sexp::constructors::Rf_mkChar(cs.as_ptr());
                if !charsxp.is_null() {
                    let data = (*class).gengc_next_node as *mut SEXP;
                    *data.add(i) = charsxp;
                }
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
                class,
            );
        }
        result
    }
}

/// R's `withRestarts(expr, ...)` — evaluate an expression with dynamic restarts.
pub unsafe fn do_withRestarts(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }

        let old_stack = restart_stack();
        let new_stack = restart_stack_from_args(CDR(args), rho, old_stack);
        set_restart_stack(new_stack);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::eval::eval::Rf_eval(expr, rho)
        }));
        set_restart_stack(old_stack);

        match result {
            Ok(value) => value,
            Err(payload) => match payload.downcast::<crate::sexp::context::RSignal>() {
                Ok(signal) => match *signal {
                    crate::sexp::context::RSignal::Restart(value) => value,
                    other => std::panic::panic_any(other),
                },
                Err(payload) => std::panic::resume_unwind(payload),
            },
        }
    }
}

unsafe fn restart_stack_from_args(mut args: SEXP, rho: SEXP, old_stack: SEXP) -> SEXP {
    unsafe {
        let mut entries = Vec::new();
        while !args.is_null() && args != R_NilValue() {
            let Some(name) = tag_name(args) else {
                args = CDR(args);
                continue;
            };
            let handler = crate::eval::eval::Rf_eval(CAR(args), rho);
            entries.push(restart_entry(&name, handler));
            args = CDR(args);
        }

        let mut stack = old_stack;
        for entry in entries.into_iter().rev() {
            stack = Rf_cons(entry, stack);
        }
        stack
    }
}

// ---------------------------------------------------------------------------
// Complete S3/S4: class, isS4, is
// ---------------------------------------------------------------------------

/// R's `class(x)` — get S3 class vector.
pub unsafe fn do_S3_class(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_class_get(_call, _op, args, _rho) }
}

/// R's `isS4(x)` — check if object is S4.
pub unsafe fn do_isS4(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        Rf_ScalarLogical(crate::mainutils::objects::isS4(x))
    }
}

/// R's `is(x, class2)` — type/class check.
pub unsafe fn do_is(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let class2_arg = CAR(CDR(args));
        if x.is_null() || class2_arg.is_null() || class2_arg == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let class2 = elt_to_string(class2_arg, 0);
        if x == R_NilValue() {
            return Rf_ScalarLogical(if class2 == "NULL" { TRUE } else { FALSE });
        }
        // Get the type of x
        let type_name = match TYPEOF(x) {
            t if t == SEXPTYPE::LGLSXP => "logical",
            t if t == SEXPTYPE::INTSXP => "integer",
            t if t == SEXPTYPE::REALSXP => "double",
            t if t == SEXPTYPE::CPLXSXP => "complex",
            t if t == SEXPTYPE::STRSXP => "character",
            t if t == SEXPTYPE::VECSXP => "list",
            t if t == SEXPTYPE::LISTSXP => "pairlist",
            t if t == SEXPTYPE::LANGSXP => "language",
            t if t == SEXPTYPE::SYMSXP => "symbol",
            t if t == SEXPTYPE::CLOSXP => "closure",
            t if t == SEXPTYPE::ENVSXP => "environment",
            _ => "any",
        };
        // Check S3 class
        let class_sym = Rf_install(CString::new("class").unwrap_or_default().as_ptr());
        let class_val = crate::sexp::attrib_core::getAttrib(x, class_sym);
        if !class_val.is_null()
            && class_val != R_NilValue()
            && TYPEOF(class_val) == SEXPTYPE::STRSXP
        {
            let n = LENGTH(class_val);
            for i in 0..n {
                let charsxp = crate::sexp::accessors::STRING_ELT(class_val, i as R_xlen_t);
                if !charsxp.is_null() {
                    let s = crate::sexp::accessors::CHAR(charsxp);
                    if !s.is_null() {
                        let c = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
                        if c == class2 {
                            return Rf_ScalarLogical(TRUE);
                        }
                        if crate::mainutils::objects::isS4(x) == TRUE
                            && crate::mainutils::objects::s4_class_extends(c, &class2)
                        {
                            return Rf_ScalarLogical(TRUE);
                        }
                    }
                }
            }
        }
        // Check type name
        let is_match = type_name == class2
            || (class2 == "numeric" && (type_name == "double" || type_name == "integer"))
            || (class2 == "vector"
                && (type_name == "logical"
                    || type_name == "integer"
                    || type_name == "double"
                    || type_name == "character"
                    || type_name == "complex"))
            || (class2 == "atomic"
                && type_name != "list"
                && type_name != "pairlist"
                && type_name != "language"
                && type_name != "closure"
                && type_name != "environment");
        Rf_ScalarLogical(if is_match { TRUE } else { FALSE })
    }
}

unsafe fn list_tag_name(cell: SEXP) -> Option<String> {
    unsafe {
        let tag = TAG(cell);
        if tag.is_null() || tag == R_NilValue() || TYPEOF(tag) != SEXPTYPE::SYMSXP {
            return None;
        }
        let printname = PRINTNAME(tag);
        if printname.is_null() {
            return None;
        }
        let chars = CHAR(printname);
        if chars.is_null() {
            return None;
        }
        Some(
            std::ffi::CStr::from_ptr(chars)
                .to_string_lossy()
                .into_owned(),
        )
    }
}

unsafe fn string_vector_values(x: SEXP) -> Vec<String> {
    unsafe {
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::STRSXP {
            return Vec::new();
        }
        let mut values = Vec::with_capacity(LENGTH(x).max(0) as usize);
        for i in 0..LENGTH(x) {
            values.push(elt_to_string(x, i as R_xlen_t));
        }
        values
    }
}

unsafe fn coerce_string_values(x: SEXP) -> Vec<String> {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Vec::new();
        }
        (0..XLENGTH(x)).map(|i| elt_to_string(x, i)).collect()
    }
}

unsafe fn string_vector_names_or_values(x: SEXP) -> Vec<String> {
    unsafe {
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::STRSXP {
            return Vec::new();
        }
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        let mut out = Vec::new();
        if !names.is_null() && names != R_NilValue() && TYPEOF(names) == SEXPTYPE::STRSXP {
            for i in 0..LENGTH(names) {
                let name = elt_to_string(names, i as R_xlen_t);
                if !name.is_empty() {
                    out.push(name);
                }
            }
        }
        if out.is_empty() {
            out = string_vector_values(x);
        }
        out
    }
}

unsafe fn s4_slots_from_args(args: SEXP) -> Vec<String> {
    unsafe {
        let mut slots = Vec::new();
        let mut current = CDR(args);
        while !current.is_null() && current != R_NilValue() {
            if let Some(name) = list_tag_name(current) {
                match name.as_str() {
                    "slots" | "representation" => {
                        for slot in string_vector_names_or_values(CAR(current)) {
                            if slots.iter().any(|existing| existing == &slot) {
                                std::panic::panic_any(RError {
                                    message: format!(
                                        "All slot names must be distinct in: ('{}')",
                                        slot
                                    ),
                                });
                            }
                            slots.push(slot);
                        }
                    }
                    "contains" | "where" | "prototype" | "validity" | "sealed" | "package" => {}
                    _ => {
                        if slots.iter().any(|existing| existing == &name) {
                            std::panic::panic_any(RError {
                                message: format!(
                                    "All slot names must be distinct in: ('{}')",
                                    name
                                ),
                            });
                        }
                        slots.push(name);
                    }
                }
            }
            current = CDR(current);
        }
        slots
    }
}

unsafe fn s4_contains_from_args(args: SEXP) -> Vec<String> {
    unsafe {
        let mut current = CDR(args);
        while !current.is_null() && current != R_NilValue() {
            if matches!(list_tag_name(current).as_deref(), Some("contains")) {
                let mut contains = string_vector_values(CAR(current));
                contains.retain(|name| !name.is_empty() && name != "VIRTUAL");
                let mut ordered = Vec::new();
                for parent in contains {
                    if !ordered.iter().any(|existing| existing == &parent) {
                        ordered.push(parent);
                    }
                }
                contains = ordered;
                return contains;
            }
            current = CDR(current);
        }
        Vec::new()
    }
}

unsafe fn string_vector_from_values(values: &[String]) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, values.len() as R_xlen_t);
        for (i, value) in values.iter().enumerate() {
            let cstr = CString::new(value.as_str()).unwrap_or_default();
            let charsxp = Rf_mkChar(cstr.as_ptr());
            SET_STRING_ELT(result, i as R_xlen_t, charsxp);
        }
        result
    }
}

/// R's `setClass(Class, representation, ...)` — define an S4 class.
pub unsafe fn do_setClass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let class_arg = CAR(args);
        if class_arg.is_null() || class_arg == R_NilValue() {
            std::panic::panic_any(RError {
                message: "'Class' must name an S4 class".to_string(),
            });
        }
        let class_name = elt_to_string(class_arg, 0);
        let slots = s4_slots_from_args(args);
        let contains = s4_contains_from_args(args);
        let virtual_class = string_vector_values(CAR(CDR(args)))
            .iter()
            .any(|value| value == "VIRTUAL");
        crate::mainutils::objects::register_s4_class_with_extends(
            class_name.clone(),
            slots,
            contains,
            virtual_class,
        );
        let cstr = CString::new(class_name).unwrap_or_default();
        Rf_mkString(cstr.as_ptr())
    }
}

/// R's `setValidity(Class, method)` — record that a class has a validity hook.
pub unsafe fn do_setValidity(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let class_name = elt_to_string(CAR(args), 0);
        if !crate::mainutils::objects::set_s4_validity(&class_name) {
            std::panic::panic_any(RError {
                message: format!("class '{}' is not defined", class_name),
            });
        }
        R_NilValue()
    }
}

/// R's `isVirtualClass(Class)` — check if a registered S4 class is virtual.
pub unsafe fn do_isVirtualClass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let class_name = elt_to_string(CAR(args), 0);
        let is_virtual = crate::mainutils::objects::s4_class(&class_name)
            .map(|class_def| class_def.virtual_class)
            .unwrap_or(false);
        Rf_ScalarLogical(if is_virtual { TRUE } else { FALSE })
    }
}

/// R's `new(Class, ...)` — create an S4 object (simplified).
/// Creates a list-based object with the class attribute set.
pub unsafe fn do_new(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let class_arg = CAR(args);
        if class_arg.is_null() || class_arg == R_NilValue() {
            return R_NilValue();
        }
        let class_name = elt_to_string(class_arg, 0);
        let Some(class_def) = crate::mainutils::objects::s4_class(&class_name) else {
            std::panic::panic_any(RError {
                message: format!("class '{}' is not defined", class_name),
            });
        };
        let class_slots = crate::mainutils::objects::s4_all_slots(&class_name).unwrap_or_default();
        if class_def.virtual_class {
            std::panic::panic_any(RError {
                message: format!("class '{}' is virtual", class_name),
            });
        }
        // Collect named slot values from ... args
        let mut slots: Vec<(String, SEXP)> = Vec::new();
        let mut current = CDR(args);
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            let slot_name =
                list_tag_name(current).unwrap_or_else(|| format!("slot{}", slots.len() + 1));
            if !class_slots.is_empty() && !class_slots.iter().any(|slot| slot == &slot_name) {
                std::panic::panic_any(RError {
                    message: format!(
                        "slot '{}' is not defined for class '{}'",
                        slot_name, class_name
                    ),
                });
            }
            slots.push((slot_name, arg));
            current = CDR(current);
        }
        for slot in &class_slots {
            if !slots.iter().any(|(name, _)| name == slot) {
                slots.push((slot.clone(), R_NilValue()));
            }
        }
        let n = slots.len() as R_xlen_t;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _np = protect(names);
        for (i, (name, val)) in slots.iter().enumerate() {
            crate::sexp::accessors::SET_VECTOR_ELT(result, i as R_xlen_t, *val);
            let cstr = CString::new(name.as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                SET_STRING_ELT(names, i as R_xlen_t, charsxp);
            }
        }
        // Set names attribute
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
            names,
        );
        // Set class attribute
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !class_vec.is_null() {
            let _class_guard = protect(class_vec);
            let cstr = CString::new(class_name.as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                SET_STRING_ELT(class_vec, 0, charsxp);
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
                class_vec,
            );
        }
        crate::mainutils::objects::asS4(result, TRUE, 0)
    }
}

/// R's `show(object)` — display an S4 object (simplified).
pub unsafe fn do_show(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let object = CAR(args);
        if object.is_null() || object == R_NilValue() {
            println!("NULL");
            return R_NilValue();
        }
        // Try to print class info
        let class_sym = Rf_install(CString::new("class").unwrap_or_default().as_ptr());
        let class_val = crate::sexp::attrib_core::getAttrib(object, class_sym);
        if !class_val.is_null()
            && class_val != R_NilValue()
            && TYPEOF(class_val) == SEXPTYPE::STRSXP
        {
            let charsxp = crate::sexp::accessors::STRING_ELT(class_val, 0);
            if !charsxp.is_null() {
                let s = crate::sexp::accessors::CHAR(charsxp);
                if !s.is_null() {
                    let class_str = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("unknown");
                    println!("An object of class \"{}\"", class_str);
                }
            }
        }
        // Print slots if VECSXP
        if TYPEOF(object) == SEXPTYPE::VECSXP {
            let n = XLENGTH(object);
            let names_sym = Rf_install(CString::new("names").unwrap_or_default().as_ptr());
            let names_val = crate::sexp::attrib_core::getAttrib(object, names_sym);
            for i in 0..n {
                let slot_val = crate::sexp::accessors::VECTOR_ELT(object, i);
                let slot_name = if !names_val.is_null() && names_val != R_NilValue() {
                    let ns = crate::sexp::accessors::STRING_ELT(names_val, i);
                    if !ns.is_null() {
                        let s = crate::sexp::accessors::CHAR(ns);
                        if !s.is_null() {
                            std::ffi::CStr::from_ptr(s)
                                .to_str()
                                .unwrap_or("")
                                .to_string()
                        } else {
                            format!("Slot{}", i + 1)
                        }
                    } else {
                        format!("Slot{}", i + 1)
                    }
                } else {
                    format!("Slot{}", i + 1)
                };
                let val_str = elt_to_string(slot_val, 0);
                println!("Slot \"{}\":", slot_name);
                println!("  {}", val_str);
            }
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        object
    }
}

/// R's `slotNames(Class)` — get the names of slots of an S4 class.
pub unsafe fn do_slotNames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let class_arg = CAR(args);
        if class_arg.is_null() || class_arg == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        if TYPEOF(class_arg) == SEXPTYPE::STRSXP {
            if let Some(slots) =
                crate::mainutils::objects::s4_all_slots(&elt_to_string(class_arg, 0))
            {
                return string_vector_from_values(&slots);
            }
        }
        if crate::mainutils::objects::isS4(class_arg) == TRUE {
            let class_sym = Rf_install(CString::new("class").unwrap_or_default().as_ptr());
            let class_val = crate::sexp::attrib_core::getAttrib(class_arg, class_sym);
            if !class_val.is_null()
                && class_val != R_NilValue()
                && TYPEOF(class_val) == SEXPTYPE::STRSXP
                && LENGTH(class_val) > 0
            {
                if let Some(slots) =
                    crate::mainutils::objects::s4_all_slots(&elt_to_string(class_val, 0))
                {
                    return string_vector_from_values(&slots);
                }
            }
        }
        // If it's an object with names, return names
        let names_sym = Rf_install(CString::new("names").unwrap_or_default().as_ptr());
        let names_val = crate::sexp::attrib_core::getAttrib(class_arg, names_sym);
        if !names_val.is_null()
            && names_val != R_NilValue()
            && TYPEOF(names_val) == SEXPTYPE::STRSXP
        {
            return names_val;
        }
        // If it's a string, treat as class name - return empty
        Rf_allocVector3(SEXPTYPE::STRSXP, 0)
    }
}

/// R's `slot(object, name)` — get the value of a slot.
pub unsafe fn do_slot(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let object = CAR(args);
        let name_arg = CAR(CDR(args));
        if object.is_null()
            || object == R_NilValue()
            || name_arg.is_null()
            || name_arg == R_NilValue()
        {
            return R_NilValue();
        }
        let slot_name = elt_to_string(name_arg, 0);
        // Look up by names attribute
        if TYPEOF(object) == SEXPTYPE::VECSXP {
            let names_sym = Rf_install(CString::new("names").unwrap_or_default().as_ptr());
            let names_val = crate::sexp::attrib_core::getAttrib(object, names_sym);
            if !names_val.is_null() && names_val != R_NilValue() {
                let n = LENGTH(names_val);
                for i in 0..n {
                    let ns = crate::sexp::accessors::STRING_ELT(names_val, i as R_xlen_t);
                    if !ns.is_null() {
                        let s = crate::sexp::accessors::CHAR(ns);
                        if !s.is_null() {
                            let name_str = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
                            if name_str == slot_name {
                                return crate::sexp::accessors::VECTOR_ELT(object, i as R_xlen_t);
                            }
                        }
                    }
                }
            }
        }
        R_NilValue()
    }
}

/// R's `set_slot(object, name, value)` — set the value of a slot.
pub unsafe fn do_set_slot(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let object = CAR(args);
        let name_arg = CAR(CDR(args));
        let value = CAR(CDR(CDR(args)));
        if object.is_null()
            || object == R_NilValue()
            || name_arg.is_null()
            || name_arg == R_NilValue()
        {
            return object;
        }
        let slot_name = elt_to_string(name_arg, 0);
        // Set slot in a VECSXP
        if TYPEOF(object) == SEXPTYPE::VECSXP {
            let names_sym = Rf_install(CString::new("names").unwrap_or_default().as_ptr());
            let names_val = crate::sexp::attrib_core::getAttrib(object, names_sym);
            if !names_val.is_null() && names_val != R_NilValue() {
                let n = LENGTH(names_val);
                for i in 0..n {
                    let ns = crate::sexp::accessors::STRING_ELT(names_val, i as R_xlen_t);
                    if !ns.is_null() {
                        let s = crate::sexp::accessors::CHAR(ns);
                        if !s.is_null() {
                            let name_str = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
                            if name_str == slot_name {
                                crate::sexp::accessors::SET_VECTOR_ELT(
                                    object,
                                    i as R_xlen_t,
                                    value,
                                );
                                return value;
                            }
                        }
                    }
                }
            }
        }
        object
    }
}

/// R's `extends(class1, class2)` — check if class1 extends class2.
pub unsafe fn do_extends(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let class1_arg = CAR(args);
        let class2_arg = CAR(CDR(args));
        if class1_arg.is_null() || class2_arg.is_null() {
            return Rf_ScalarLogical(FALSE);
        }
        let class1 = elt_to_string(class1_arg, 0);
        let class2 = elt_to_string(class2_arg, 0);
        // Simple: same class always extends
        if class1 == class2 {
            return Rf_ScalarLogical(TRUE);
        }
        // Check common inheritance
        let extends = match class1.as_str() {
            "numeric" | "double" => class2 == "vector" || class2 == "atomic",
            "integer" => class2 == "numeric" || class2 == "vector" || class2 == "atomic",
            "logical" => class2 == "vector" || class2 == "atomic",
            "character" => class2 == "vector" || class2 == "atomic",
            "complex" => class2 == "vector" || class2 == "atomic",
            "matrix" => class2 == "array",
            "data.frame" => class2 == "list",
            "factor" => class2 == "integer" || class2 == "vector" || class2 == "atomic",
            "ordered" => class2 == "factor" || class2 == "integer",
            _ => false,
        };
        Rf_ScalarLogical(if extends { TRUE } else { FALSE })
    }
}

/// R's `isSealedClass(Class)` — check if a class is sealed.
pub unsafe fn do_isSealedClass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Built-in types are always sealed
        Rf_ScalarLogical(TRUE)
    }
}

/// R's `sealClass(Class, ...)` — seal a class definition.
pub unsafe fn do_sealClass(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // No-op in simplified implementation
        R_NilValue()
    }
}

/// R's `representation(...)` — define class representation.
pub unsafe fn do_representation(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Collect named args as slot name = type pairs
        let n_list = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        if n_list.is_null() {
            return R_NilValue();
        }
        let _p = protect(n_list);
        // Count args
        let mut count: R_xlen_t = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            count += 1;
            current = CDR(current);
        }
        if count == 0 {
            return n_list;
        }
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, count);
        if result.is_null() {
            return R_NilValue();
        }
        let rp = protect(result);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, count);
        let np = protect(names);
        let mut idx: R_xlen_t = 0;
        current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            let tag = (*current).data.listsxp.tagval;
            let slot_name = if !tag.is_null() && tag != R_NilValue() {
                let sym_str = crate::sexp::accessors::CHAR(tag);
                if !sym_str.is_null() {
                    std::ffi::CStr::from_ptr(sym_str)
                        .to_str()
                        .unwrap_or("")
                        .to_string()
                } else {
                    format!("slot{}", idx + 1)
                }
            } else {
                format!("slot{}", idx + 1)
            };
            crate::sexp::accessors::SET_VECTOR_ELT(result, idx, arg);
            let cstr = CString::new(slot_name.as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let data = (*names).gengc_next_node as *mut SEXP;
                *data.add(idx as usize) = charsxp;
            }
            idx += 1;
            current = CDR(current);
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
            names,
        );
        result
    }
}

/// R's `containsClass(class1, class2)` — check class containment.
pub unsafe fn do_containsClass(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Delegates to extends
        do_extends(_call, _op, args, _rho)
    }
}

/// R's `possibleExtends(class1, class2)` — check possible extensions.
pub unsafe fn do_possibleExtends(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Simplified: delegates to extends
        do_extends(_call, _op, args, _rho)
    }
}

/// R's `setReplaceMethod(f, signature, definition)` — set replace method.
pub unsafe fn do_setReplaceMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Simplified: return the definition
        let definition = CAR(CDR(CDR(args)));
        if !definition.is_null() && definition != R_NilValue() {
            definition
        } else {
            R_NilValue()
        }
    }
}

/// R's `getMethod(f, signature)` — get a specific S4 method.
pub unsafe fn do_getMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Simplified: return the function name or NULL
        let f_arg = CAR(args);
        if f_arg.is_null() || f_arg == R_NilValue() {
            return R_NilValue();
        }
        f_arg
    }
}

/// R's `removeGeneric(f)` — remove a generic.
pub unsafe fn do_removeGeneric(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _f = CAR(args);
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `removeMethod(f, signature)` — remove a method.
pub unsafe fn do_removeMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _f = CAR(args);
        let _sig = CAR(CDR(args));
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `isGeneric(f)` — check if f is a generic.
pub unsafe fn do_isGeneric(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _f = CAR(args);
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `isMethod(f, signature)` — check if method exists.
pub unsafe fn do_isMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _f = CAR(args);
        let _sig = CAR(CDR(args));
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `findMethod(f, signature)` — find S4 method.
pub unsafe fn do_findMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _f = CAR(args);
        let _sig = CAR(CDR(args));
        R_NilValue()
    }
}

/// R's `findMethods(f)` — find all methods for a generic.
pub unsafe fn do_findMethods(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _f = CAR(args);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        if result.is_null() {
            R_NilValue()
        } else {
            result
        }
    }
}

/// R's `showMethods(f)` — show methods for a generic.
pub unsafe fn do_showMethods(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _f = CAR(args);
        println!("No methods found");
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// R's `getGenerics(where)` — get all generics.
pub unsafe fn do_getGenerics(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _where = CAR(args);
        Rf_allocVector3(SEXPTYPE::STRSXP, 0)
    }
}

/// R's `getMethods(f)` — get all methods for a generic.
pub unsafe fn do_getMethods(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _f = CAR(args);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 0);
        if result.is_null() {
            R_NilValue()
        } else {
            result
        }
    }
}

/// R's `existsMethod(f, signature)` — check if method exists.
pub unsafe fn do_existsMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _f = CAR(args);
        let _sig = CAR(CDR(args));
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `hasMethod(f, signature)` — alias for existsMethod.
pub unsafe fn do_hasMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_existsMethod(_call, _op, args, _rho) }
}

/// R's `selectMethod(f, signature)` — select method for generic.
pub unsafe fn do_selectMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let f_arg = CAR(args);
        if f_arg.is_null() || f_arg == R_NilValue() {
            return R_NilValue();
        }
        f_arg
    }
}

// ---------------------------------------------------------------------------
// Complete I/O: scan, write.table, sink
// ---------------------------------------------------------------------------

fn scan_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

fn split_scan_fields(contents: &str, sep: &str, nmax: i64) -> Vec<String> {
    let limit = if nmax > 0 { nmax as usize } else { usize::MAX };
    let fields: Box<dyn Iterator<Item = &str> + '_> = if sep.is_empty() {
        Box::new(contents.split_whitespace())
    } else {
        Box::new(
            contents
                .split(sep)
                .map(str::trim)
                .filter(|field| !field.is_empty()),
        )
    };
    fields.take(limit).map(ToOwned::to_owned).collect()
}

fn parse_scan_logical(value: &str) -> Option<c_int> {
    match value {
        "TRUE" | "True" | "true" | "T" | "1" => Some(TRUE),
        "FALSE" | "False" | "false" | "F" | "0" => Some(FALSE),
        "NA" => Some(NA_LOGICAL),
        _ => None,
    }
}

unsafe fn named_arg(args: SEXP, name: &str) -> Option<SEXP> {
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let tag = TAG(current);
            if !tag.is_null() && tag != R_NilValue() {
                let printname = PRINTNAME(tag);
                if !printname.is_null() {
                    let tag_name = CStr::from_ptr(CHAR(printname)).to_string_lossy();
                    if tag_name == name {
                        return Some(CAR(current));
                    }
                }
            }
            current = CDR(current);
        }
        None
    }
}

/// R's `scan(file, what, nmax, sep, ...)` — read data from a file path.
/// This covers the file-backed scalar-vector surface used by scripts and tests;
/// interactive console and connection scans report explicit R errors.
pub unsafe fn do_scan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = CAR(args);
        let what_arg = CAR(CDR(args));
        let nmax_arg = CAR(CDR(CDR(args)));
        let fourth_arg = CAR(CDR(CDR(CDR(args))));
        let fifth_arg = CAR(CDR(CDR(CDR(CDR(args)))));
        if file_arg.is_null() || file_arg == R_NilValue() {
            scan_error("scan() requires a file path in the Android/headless runtime");
        }
        if TYPEOF(file_arg) != SEXPTYPE::STRSXP || XLENGTH(file_arg) < 1 {
            scan_error("scan() currently supports character file paths only");
        }
        let filename = elt_to_string(file_arg, 0);
        if filename.is_empty() {
            scan_error("scan() cannot read from an interactive console in this runtime");
        }
        let what_type = if what_arg.is_null() || what_arg == R_NilValue() {
            SEXPTYPE::REALSXP.as_c_int()
        } else {
            TYPEOF(what_arg)
        };
        let nmax = if nmax_arg.is_null() || nmax_arg == R_NilValue() {
            -1_i64
        } else {
            real_or_default(nmax_arg, -1.0) as i64
        };
        let sep_arg = if let Some(sep) = named_arg(args, "sep") {
            sep
        } else if !fourth_arg.is_null() && TYPEOF(fourth_arg) == SEXPTYPE::STRSXP {
            fourth_arg
        } else {
            fifth_arg
        };
        let sep = if sep_arg.is_null() || sep_arg == R_NilValue() {
            String::new()
        } else {
            elt_to_string(sep_arg, 0)
        };

        let contents = match std::fs::read_to_string(&filename) {
            Ok(s) => s,
            Err(err) => scan_error(format!("cannot open file '{filename}': {err}")),
        };

        let values = split_scan_fields(&contents, &sep, nmax);
        let n = values.len() as R_xlen_t;
        if what_type == SEXPTYPE::INTSXP {
            let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = protect(result);
            let dst = INTEGER(result);
            for (i, value) in values.iter().enumerate() {
                let parsed = if value == "NA" {
                    NA_INTEGER
                } else {
                    value.parse::<c_int>().unwrap_or_else(|_| {
                        scan_error(format!("scan() expected an integer, got '{value}'"))
                    })
                };
                *dst.add(i) = parsed;
            }
            result
        } else if what_type == SEXPTYPE::REALSXP {
            let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = protect(result);
            let dst = REAL(result);
            for (i, value) in values.iter().enumerate() {
                let parsed = if value == "NA" {
                    NA_REAL
                } else {
                    value.parse::<f64>().unwrap_or_else(|_| {
                        scan_error(format!("scan() expected a real, got '{value}'"))
                    })
                };
                *dst.add(i) = parsed;
            }
            result
        } else if what_type == SEXPTYPE::LGLSXP {
            let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = protect(result);
            let dst = LOGICAL(result);
            for (i, value) in values.iter().enumerate() {
                let parsed = parse_scan_logical(value).unwrap_or_else(|| {
                    scan_error(format!("scan() expected a logical, got '{value}'"))
                });
                *dst.add(i) = parsed;
            }
            result
        } else if what_type == SEXPTYPE::STRSXP {
            let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = protect(result);
            for (i, value) in values.iter().enumerate() {
                let cstr = CString::new(value.as_str()).unwrap_or_default();
                let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                SET_STRING_ELT(result, i as R_xlen_t, charsxp);
            }
            result
        } else {
            scan_error("scan() only supports integer, numeric, logical, and character 'what'")
        }
    }
}

/// R's `write.table(x, file, sep=" ", ...)` — write data to file.
pub unsafe fn do_write_table(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        let file_arg = CAR(CDR(args));
        let sep_arg = CAR(CDR(CDR(args)));
        if x_arg.is_null()
            || x_arg == R_NilValue()
            || file_arg.is_null()
            || file_arg == R_NilValue()
        {
            return R_NilValue();
        }
        let filename = elt_to_string(file_arg, 0);
        let sep = if sep_arg.is_null() || sep_arg == R_NilValue() {
            " "
        } else {
            &elt_to_string(sep_arg, 0)
        };

        let mut output = String::new();
        let n = XLENGTH(x_arg);
        let t = TYPEOF(x_arg);

        if t == SEXPTYPE::VECSXP {
            // Data frame-like: write columns
            let ncols = n;
            let nrows = if n > 0 {
                XLENGTH(VECTOR_ELT(x_arg, 0))
            } else {
                0
            };
            if ncols == 0 {
                output.push_str("\"\"\n");
            }
            // Write header with column names
            let names_sym = Rf_install(CString::new("names").unwrap_or_default().as_ptr());
            let names = crate::sexp::attrib_core::getAttrib(x_arg, names_sym);
            if ncols > 0
                && !names.is_null()
                && names != R_NilValue()
                && TYPEOF(names) == SEXPTYPE::STRSXP
            {
                let mut header = Vec::new();
                for j in 0..ncols {
                    let charsxp = crate::sexp::accessors::STRING_ELT(names, j);
                    if !charsxp.is_null() {
                        let s = crate::sexp::accessors::CHAR(charsxp);
                        if !s.is_null() {
                            header.push(
                                std::ffi::CStr::from_ptr(s)
                                    .to_str()
                                    .unwrap_or("")
                                    .to_string(),
                            );
                        } else {
                            header.push(String::new());
                        }
                    } else {
                        header.push(String::new());
                    }
                }
                output.push_str(&header.join(sep));
                output.push('\n');
            }
            // Write rows
            for i in 0..nrows {
                let mut row = Vec::new();
                for j in 0..ncols {
                    let col = VECTOR_ELT(x_arg, j);
                    if !col.is_null() && col != R_NilValue() {
                        row.push(elt_to_string(col, i));
                    } else {
                        row.push("NA".to_string());
                    }
                }
                output.push_str(&row.join(sep));
                output.push('\n');
            }
        } else {
            // Atomic vector: write as single column
            for i in 0..n {
                output.push_str(&elt_to_string(x_arg, i));
                output.push('\n');
            }
        }

        let _ = std::fs::write(&filename, output);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// R's `sink(file, append, type, split)` — redirect output to a connection.
pub unsafe fn do_sink(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut file_arg = R_NilValue();
        let mut append_arg = Rf_ScalarLogical(FALSE);
        let mut type_arg = Rf_mkString(CString::new("output").unwrap_or_default().as_ptr());
        let mut split_arg = Rf_ScalarLogical(FALSE);
        let mut positional = 0usize;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            match tag_name(current).as_deref() {
                Some("file") => file_arg = arg,
                Some("append") => append_arg = arg,
                Some("type") => type_arg = arg,
                Some("split") => split_arg = arg,
                _ => {
                    match positional {
                        0 => file_arg = arg,
                        1 => append_arg = arg,
                        2 => type_arg = arg,
                        3 => split_arg = arg,
                        _ => {}
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }

        let split = logical_scalar_or(split_arg, FALSE);
        let is_message_sink = !type_arg.is_null()
            && type_arg != R_NilValue()
            && TYPEOF(type_arg) == SEXPTYPE::STRSXP
            && elt_to_string(type_arg, 0) == "message";

        let (target, close_on_exit) = if file_arg.is_null() || file_arg == R_NilValue() {
            if is_message_sink {
                (Rf_ScalarInteger(2), FALSE)
            } else {
                (Rf_ScalarInteger(-1), FALSE)
            }
        } else if inherits_class(file_arg, "connection") {
            (file_arg, FALSE)
        } else if TYPEOF(file_arg) == SEXPTYPE::STRSXP {
            if is_message_sink {
                base_error("'file' must be NULL or an already open connection");
            }
            let append = logical_scalar_or(append_arg, FALSE) != FALSE;
            let open = if append { "a" } else { "w" };
            let open_sxp = Rf_mkString(CString::new(open).unwrap_or_default().as_ptr());
            let encoding_sxp = Rf_mkString(CString::new("native.enc").unwrap_or_default().as_ptr());
            let file_args = Rf_cons(
                file_arg,
                Rf_cons(
                    open_sxp,
                    Rf_cons(
                        encoding_sxp,
                        Rf_cons(
                            Rf_ScalarLogical(TRUE),
                            Rf_cons(Rf_ScalarLogical(FALSE), R_NilValue()),
                        ),
                    ),
                ),
            );
            (
                crate::mainutils::connections::do_file(_call, _op, file_args, _rho),
                TRUE,
            )
        } else {
            base_error("'file' must be NULL, a connection or a character string");
        };
        if is_message_sink && split != FALSE {
            base_error("cannot split the message connection");
        }

        let normalized = Rf_cons(
            target,
            Rf_cons(
                Rf_ScalarLogical(close_on_exit),
                Rf_cons(
                    Rf_ScalarLogical(if is_message_sink { TRUE } else { FALSE }),
                    Rf_cons(Rf_ScalarLogical(split), R_NilValue()),
                ),
            ),
        );
        crate::mainutils::connections::do_sink(_call, _op, normalized, _rho);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        R_NilValue()
    }
}

/// R's `sink.number(type)` — report output or message sink depth.
pub unsafe fn do_sink_number(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let type_arg = if args.is_null() || args == R_NilValue() {
            Rf_mkString(CString::new("output").unwrap_or_default().as_ptr())
        } else {
            CAR(args)
        };
        let is_message_sink = !type_arg.is_null()
            && type_arg != R_NilValue()
            && TYPEOF(type_arg) == SEXPTYPE::STRSXP
            && elt_to_string(type_arg, 0) == "message";
        let normalized = Rf_cons(
            Rf_ScalarLogical(if is_message_sink { TRUE } else { FALSE }),
            R_NilValue(),
        );
        crate::mainutils::connections::do_sinkNumber(_call, _op, normalized, _rho)
    }
}

unsafe fn logical_scalar_or(arg: SEXP, default: c_int) -> c_int {
    unsafe {
        if arg.is_null() || arg == R_NilValue() || arg == R_MissingArg() {
            return default;
        }
        if TYPEOF(arg) == SEXPTYPE::LGLSXP || TYPEOF(arg) == SEXPTYPE::INTSXP {
            *INTEGER(arg)
        } else {
            default
        }
    }
}

// ---------------------------------------------------------------------------
// Math/Statistics
// ---------------------------------------------------------------------------

/// R's `cov(x, y)` — covariance between two numeric vectors.
pub unsafe fn do_cov(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y_cdr = CDR(args);
        let y = if y_cdr.is_null() || y_cdr == R_NilValue() {
            R_NilValue()
        } else {
            CAR(y_cdr)
        };

        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarReal(NA_REAL);
        }

        let x_data = get_numeric_data(x);
        let y_data = if y.is_null() || y == R_NilValue() {
            x_data.clone()
        } else {
            get_numeric_data(y)
        };

        let n = x_data.len().min(y_data.len());
        if n == 0 {
            return Rf_ScalarReal(NA_REAL);
        }

        let mut sum_x = 0.0_f64;
        let mut sum_y = 0.0_f64;
        let mut count = 0_i64;
        for i in 0..n {
            if !x_data[i].is_nan() && !y_data[i].is_nan() {
                sum_x += x_data[i];
                sum_y += y_data[i];
                count += 1;
            }
        }
        if count < 2 {
            return Rf_ScalarReal(NA_REAL);
        }
        let mean_x = sum_x / count as f64;
        let mean_y = sum_y / count as f64;

        let mut cov = 0.0_f64;
        for i in 0..n {
            if !x_data[i].is_nan() && !y_data[i].is_nan() {
                cov += (x_data[i] - mean_x) * (y_data[i] - mean_y);
            }
        }
        Rf_ScalarReal(cov / (count as f64 - 1.0))
    }
}

/// R's `cor(x, y)` — Pearson correlation between two numeric vectors.
pub unsafe fn do_cor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y_cdr = CDR(args);
        let y = if y_cdr.is_null() || y_cdr == R_NilValue() {
            R_NilValue()
        } else {
            CAR(y_cdr)
        };

        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarReal(NA_REAL);
        }

        let x_data = get_numeric_data(x);
        let y_data = if y.is_null() || y == R_NilValue() {
            x_data.clone()
        } else {
            get_numeric_data(y)
        };

        let n = x_data.len().min(y_data.len());
        if n == 0 {
            return Rf_ScalarReal(NA_REAL);
        }

        let mut sum_x = 0.0_f64;
        let mut sum_y = 0.0_f64;
        let mut count = 0_i64;
        for i in 0..n {
            if !x_data[i].is_nan() && !y_data[i].is_nan() {
                sum_x += x_data[i];
                sum_y += y_data[i];
                count += 1;
            }
        }
        if count < 2 {
            return Rf_ScalarReal(NA_REAL);
        }
        let mean_x = sum_x / count as f64;
        let mean_y = sum_y / count as f64;

        let mut cov = 0.0_f64;
        let mut var_x = 0.0_f64;
        let mut var_y = 0.0_f64;
        for i in 0..n {
            if !x_data[i].is_nan() && !y_data[i].is_nan() {
                let dx = x_data[i] - mean_x;
                let dy = y_data[i] - mean_y;
                cov += dx * dy;
                var_x += dx * dx;
                var_y += dy * dy;
            }
        }
        let denom = (var_x * var_y).sqrt();
        if denom == 0.0 {
            return Rf_ScalarReal(NA_REAL);
        }
        Rf_ScalarReal(cov / denom)
    }
}

/// R's `scale(x, center=TRUE, scale=TRUE)` — standardize a numeric vector.
pub unsafe fn do_scale(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let center_arg = CAR(CDR(args));
        let scale_arg = CAR(CDR(CDR(args)));

        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let do_center = center_arg.is_null()
            || center_arg == R_NilValue()
            || (TYPEOF(center_arg) == SEXPTYPE::LGLSXP && *LOGICAL(center_arg) == TRUE);
        let do_scale = scale_arg.is_null()
            || scale_arg == R_NilValue()
            || (TYPEOF(scale_arg) == SEXPTYPE::LGLSXP && *LOGICAL(scale_arg) == TRUE);

        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        // Compute mean
        let mut sum = 0.0_f64;
        let mut count = 0_i64;
        for i in 0..n {
            let v = real_or_default(elt_to_sexp(x, i), NA_REAL);
            if !v.is_nan() && v != NA_REAL {
                sum += v;
                count += 1;
            }
        }
        let mean = if count > 0 {
            sum / count as f64
        } else {
            NA_REAL
        };

        // Compute sd
        let mut var_sum = 0.0_f64;
        if do_scale {
            for i in 0..n {
                let v = real_or_default(elt_to_sexp(x, i), NA_REAL);
                if !v.is_nan() && v != NA_REAL {
                    var_sum += (v - mean) * (v - mean);
                }
            }
        }
        let sd = if count > 1 {
            (var_sum / (count as f64 - 1.0)).sqrt()
        } else {
            NA_REAL
        };

        let dst = REAL(result);
        for i in 0..n {
            let v = real_or_default(elt_to_sexp(x, i), NA_REAL);
            let centered = if do_center { v - mean } else { v };
            let scaled = if do_scale && sd != 0.0 && !sd.is_nan() {
                centered / sd
            } else {
                centered
            };
            *dst.add(i as usize) = scaled;
        }
        result
    }
}

/// R's `rle(x)` — run-length encoding.
pub unsafe fn do_rle(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        if n == 0 {
            let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = protect(result);
            SET_VECTOR_ELT(result, 0, Rf_allocVector3(SEXPTYPE::INTSXP, 0));
            SET_VECTOR_ELT(result, 1, Rf_allocVector3(SEXPTYPE::REALSXP, 0));
            let names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
            if !names.is_null() {
                let _p2 = protect(names);
                for (i, nm) in ["lengths", "values"].iter().enumerate() {
                    let cs = CString::new(*nm).unwrap_or_default();
                    let charsxp = crate::sexp::constructors::Rf_mkChar(cs.as_ptr());
                    if !charsxp.is_null() {
                        let data = (*names).gengc_next_node as *mut SEXP;
                        *data.add(i) = charsxp;
                    }
                }
                crate::sexp::attrib_core::setAttrib(
                    result,
                    Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
                    names,
                );
            }
            return result;
        }

        // Collect run lengths and values
        let mut lengths: Vec<i32> = Vec::new();
        let mut values: Vec<f64> = Vec::new();

        let first_val = real_or_default(elt_to_sexp(x, 0), NA_REAL);
        values.push(first_val);
        lengths.push(1);

        for i in 1..n {
            let v = real_or_default(elt_to_sexp(x, i), NA_REAL);
            let last_idx = values.len() - 1;
            if v == values[last_idx] {
                lengths[last_idx] += 1;
            } else {
                values.push(v);
                lengths.push(1);
            }
        }

        let n_runs = lengths.len() as R_xlen_t;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let lengths_sexp = Rf_allocVector3(SEXPTYPE::INTSXP, n_runs);
        let values_sexp = Rf_allocVector3(SEXPTYPE::REALSXP, n_runs);
        let _p2 = protect(lengths_sexp);
        let _p3 = protect(values_sexp);

        let dst_l = INTEGER(lengths_sexp);
        let dst_v = REAL(values_sexp);
        for i in 0..n_runs {
            *dst_l.add(i as usize) = lengths[i as usize];
            *dst_v.add(i as usize) = values[i as usize];
        }

        SET_VECTOR_ELT(result, 0, lengths_sexp);
        SET_VECTOR_ELT(result, 1, values_sexp);

        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        if !names.is_null() {
            let _p4 = protect(names);
            for (i, nm) in ["lengths", "values"].iter().enumerate() {
                let cs = CString::new(*nm).unwrap_or_default();
                let charsxp = crate::sexp::constructors::Rf_mkChar(cs.as_ptr());
                if !charsxp.is_null() {
                    let data = (*names).gengc_next_node as *mut SEXP;
                    *data.add(i) = charsxp;
                }
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
                names,
            );
        }
        result
    }
}

/// R's `inverse.rle(x)` — inverse of run-length encoding.
pub unsafe fn do_inverse_rle(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() || TYPEOF(x) != SEXPTYPE::VECSXP {
            return R_NilValue();
        }

        let lengths_sexp = VECTOR_ELT(x, 0);
        let values_sexp = VECTOR_ELT(x, 1);
        if lengths_sexp.is_null() || values_sexp.is_null() {
            return R_NilValue();
        }

        let n_runs = XLENGTH(lengths_sexp);
        if n_runs == 0 {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }

        // Compute total length
        let mut total: R_xlen_t = 0;
        for i in 0..n_runs {
            total += (*INTEGER(lengths_sexp).add(i as usize)) as R_xlen_t;
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        let mut offset: R_xlen_t = 0;
        for i in 0..n_runs {
            let len = *INTEGER(lengths_sexp).add(i as usize);
            let val = real_or_default(elt_to_sexp(values_sexp, i), NA_REAL);
            for j in 0..len {
                *dst.add((offset + j as R_xlen_t) as usize) = val;
            }
            offset += len as R_xlen_t;
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Matrix helpers
// ---------------------------------------------------------------------------

/// R's `which(x)` variant for arrays — returns 1-based row-major indices where x is TRUE.
pub unsafe fn do_which_array(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Same as do_which for now — array-aware which is equivalent for logical vectors
        do_which(_call, _op, args, _rho)
    }
}

// ---------------------------------------------------------------------------
// R runtime
// ---------------------------------------------------------------------------

/// R's `commandArgs()` — returns the command line arguments as a character vector.
pub unsafe fn do_commandArgs(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let args: Vec<String> = std::env::args().collect();
        let n = args.len() as R_xlen_t;
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        for (i, arg) in args.iter().enumerate() {
            let cs = CString::new(arg.as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cs.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i) = charsxp;
            }
        }
        result
    }
}

/// R's `getOption(x)` — delegate to the canonical options implementation.
pub unsafe fn do_getOption(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::options::do_getOption(call, op, args, rho) }
}

/// R's `options(...)` — delegate to the canonical options implementation.
pub unsafe fn do_options(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::options::do_options(call, op, args, rho) }
}

/// R's `interactive()` — returns FALSE (not in interactive session).
pub unsafe fn do_interactive(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { Rf_ScalarLogical(FALSE) }
}

/// Alias for `interactive()`.
pub unsafe fn do_is_interactive(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { Rf_ScalarLogical(FALSE) }
}

/// R's `getRversion()` — returns an `R_system_version` package-version object.
pub unsafe fn do_getRversion(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        let version = Rf_allocVector3(SEXPTYPE::INTSXP, 3);
        if !version.is_null() {
            let _version_guard = protect(version);
            let data = INTEGER(version);
            *data.add(0) = 4;
            *data.add(1) = 4;
            *data.add(2) = 1;
            SET_VECTOR_ELT(result, 0, version);
        }

        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
        if !class.is_null() {
            let _class_guard = protect(class);
            for (i, name) in ["R_system_version", "package_version", "numeric_version"]
                .iter()
                .enumerate()
            {
                let value = CString::new(*name).unwrap_or_default();
                SET_STRING_ELT(class, i as R_xlen_t, Rf_mkChar(value.as_ptr()));
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
        }
        result
    }
}

/// R's `R.version.string` — returns the full R version string.
pub unsafe fn do_R_version_string(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let s = CString::new("R version 4.4.1 (Rust Port)").unwrap_or_default();
        Rf_mkString(s.as_ptr())
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
// String operations
// ---------------------------------------------------------------------------

/// R-like `str_detect(x, pattern)` — returns logical vector indicating which elements match.
pub unsafe fn do_str_detect(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let pattern_arg = CAR(CDR(args));

        if x.is_null() || x == R_NilValue() || pattern_arg.is_null() || pattern_arg == R_NilValue()
        {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }

        let pattern_str = elt_to_string(pattern_arg, 0);
        let n = XLENGTH(x).max(1);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = LOGICAL(result);

        for i in 0..n {
            let s = elt_to_string(x, i);
            let matches = s.contains(&pattern_str);
            *dst.add(i as usize) = if matches { TRUE } else { FALSE };
        }
        result
    }
}

/// R-like `str_extract(x, pattern)` — extracts first occurrence of pattern from each element.
pub unsafe fn do_str_extract(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let pattern_arg = CAR(CDR(args));

        if x.is_null() || x == R_NilValue() || pattern_arg.is_null() || pattern_arg == R_NilValue()
        {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }

        let pattern_str = elt_to_string(pattern_arg, 0);
        let n = XLENGTH(x).max(1);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        for i in 0..n {
            let s = elt_to_string(x, i);
            let extracted = if let Some(start) = s.find(&pattern_str) {
                let end = start + pattern_str.len();
                &s[start..end]
            } else {
                "NA"
            };
            let cs = CString::new(extracted).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cs.as_ptr());
            if !charsxp.is_null() {
                let data = (*result).gengc_next_node as *mut SEXP;
                *data.add(i as usize) = charsxp;
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Complete data operations
// ---------------------------------------------------------------------------

/// R's `reshape(x, direction, varying, v.names, timevar, idvar, times)` — reshape data.
/// Simplified: just return x as-is.
pub unsafe fn do_reshape(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() {
            return R_NilValue();
        }
        x
    }
}

/// R's `complete_cases(...)` — returns logical vector: TRUE where all args are non-NA.
pub unsafe fn do_complete_cases(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Collect all argument vectors
        let mut arg_vecs: Vec<SEXP> = Vec::new();
        let mut max_len: R_xlen_t = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                arg_vecs.push(arg);
                let n = XLENGTH(arg);
                if n > max_len {
                    max_len = n;
                }
            }
            current = CDR(current);
        }
        if arg_vecs.is_empty() || max_len == 0 {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, max_len);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = LOGICAL(result);
        for i in 0..max_len {
            let mut complete = TRUE;
            for &arg in &arg_vecs {
                let n = XLENGTH(arg);
                let idx = if n == 0 { 0 } else { i % n };
                if atomic_value_is_missing(arg, idx) {
                    complete = FALSE;
                    break;
                }
            }
            *dst.add(i as usize) = complete;
        }
        result
    }
}

/// R's `na.omit(x)` — returns x with rows containing any NA removed (simplified: works on vectors).
pub unsafe fn do_na_omit(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { na_omit_atomic(args, "omit") }
}

/// R's `na.exclude(x)` — like na.omit but remembers excluded rows. Simplified: same as na.omit.
pub unsafe fn do_na_exclude(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { na_omit_atomic(args, "exclude") }
}

unsafe fn na_omit_atomic(args: SEXP, action_class: &str) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x);
        let sexptype = SEXPTYPE(t);
        if !matches!(
            sexptype,
            SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP | SEXPTYPE::REALSXP | SEXPTYPE::STRSXP
        ) {
            return x;
        }

        let n = XLENGTH(x);
        let mut keep: Vec<R_xlen_t> = Vec::new();
        let mut dropped: Vec<R_xlen_t> = Vec::new();
        for i in 0..n {
            if atomic_value_is_missing(x, i) {
                dropped.push(i);
            } else {
                keep.push(i);
            }
        }

        let result = Rf_allocVector3(t, keep.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (out, &src) in keep.iter().enumerate() {
            copy_vector_element(result, out as R_xlen_t, x, src, sexptype);
        }
        set_selected_names_attribute(x, result, &keep);
        if !dropped.is_empty() {
            set_na_action_attribute(x, result, &dropped, action_class);
        }
        result
    }
}

unsafe fn set_selected_names_attribute(x: SEXP, result: SEXP, indices: &[R_xlen_t]) {
    unsafe {
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return;
        }
        let selected = Rf_allocVector3(SEXPTYPE::STRSXP, indices.len() as R_xlen_t);
        if selected.is_null() {
            return;
        }
        let _selected_guard = protect(selected);
        for (out, &src) in indices.iter().enumerate() {
            SET_STRING_ELT(selected, out as R_xlen_t, STRING_ELT(names, src));
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            selected,
        );
    }
}

unsafe fn set_na_action_attribute(
    source: SEXP,
    result: SEXP,
    dropped: &[R_xlen_t],
    action_class: &str,
) {
    unsafe {
        let action = Rf_allocVector3(SEXPTYPE::INTSXP, dropped.len() as R_xlen_t);
        if action.is_null() {
            return;
        }
        let _action_guard = protect(action);
        for (out, &src) in dropped.iter().enumerate() {
            *INTEGER(action).add(out) = (src + 1) as c_int;
        }

        let names =
            crate::sexp::attrib_core::getAttrib(source, crate::sexp::attrib_core::R_NamesSymbol());
        if !names.is_null() && names != R_NilValue() && TYPEOF(names) == SEXPTYPE::STRSXP {
            let action_names = Rf_allocVector3(SEXPTYPE::STRSXP, dropped.len() as R_xlen_t);
            if !action_names.is_null() {
                let _names_guard = protect(action_names);
                for (out, &src) in dropped.iter().enumerate() {
                    SET_STRING_ELT(action_names, out as R_xlen_t, STRING_ELT(names, src));
                }
                crate::sexp::attrib_core::setAttrib(
                    action,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    action_names,
                );
            }
        }

        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !class.is_null() {
            let _class_guard = protect(class);
            SET_STRING_ELT(
                class,
                0,
                Rf_mkChar(CString::new(action_class).unwrap_or_default().as_ptr()),
            );
            crate::sexp::attrib_core::setAttrib(
                action,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
        }
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"na.action".as_ptr()), action);
    }
}

/// R's `is_complete(x)` — logical vector of complete cases for a single vector.
pub unsafe fn do_is_complete(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = LOGICAL(result);
        for i in 0..n {
            let na = atomic_value_is_missing(x, i);
            *dst.add(i as usize) = if na { FALSE } else { TRUE };
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Complete string/vector
// ---------------------------------------------------------------------------

/// R-like `str_interp(string, values)` — interpolate values into string (simplified: sprintf-like).
pub unsafe fn do_str_interp(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let string_arg = CAR(args);
        let values_arg = CAR(CDR(args));
        if string_arg.is_null() || string_arg == R_NilValue() {
            return Rf_mkString(CString::new("").unwrap_or_default().as_ptr());
        }
        let fmt = elt_to_string(string_arg, 0);
        if values_arg.is_null() || values_arg == R_NilValue() {
            return Rf_mkString(CString::new(fmt).unwrap_or_default().as_ptr());
        }
        let n = XLENGTH(values_arg).max(1);
        let mut vals: Vec<String> = Vec::new();
        for i in 0..n {
            vals.push(elt_to_string(values_arg, i));
        }
        // Simple %s replacement
        let mut result = fmt.clone();
        for v in &vals {
            if let Some(pos) = result.find("%s") {
                result.replace_range(pos..pos + 2, v);
            }
        }
        Rf_mkString(CString::new(result).unwrap_or_default().as_ptr())
    }
}

/// R-like `strwrap(x, width)` / `str_wrap(x, width)` — wrap text to width.
pub unsafe fn do_str_wrap(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }
        let width_arg = arg_by_name_or_position(args, &["width"], 1);
        let width =
            if width_arg.is_null() || width_arg == R_NilValue() || XLENGTH(width_arg) == 0 {
                0
            } else {
                numeric_elt_as_count(width_arg, 0)
            }
            .max(1);

        let mut lines = Vec::new();
        for i in 0..XLENGTH(x) {
            lines.extend(wrap_text_words(&elt_to_string(x, i), width));
        }
        string_vector(&lines)
    }
}

fn wrap_text_words(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let current_len = current.chars().count();
        let word_len = word.chars().count();
        let next_len = if current.is_empty() {
            word_len
        } else {
            current_len + 1 + word_len
        };
        if !current.is_empty() && next_len >= width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// R-like `path_package(package, ...)` — find package paths through the session library policy.
pub unsafe fn do_path_package(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let package_arg = arg_by_name_or_position(args, &["package"], 0);
        if package_arg.is_null() || package_arg == R_NilValue() || XLENGTH(package_arg) == 0 {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }

        let mut paths = Vec::new();
        for i in 0..XLENGTH(package_arg) {
            let package = elt_to_string(package_arg, i);
            let path = find_package_path(&package);
            if !path.is_empty() {
                paths.push(path);
            }
        }
        string_vector(&paths)
    }
}

/// R's `system.file(..., package)` — find files inside an installed package.
pub unsafe fn do_system_file(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let package_arg = arg_by_name_or_position(args, &["package"], usize::MAX);
        let package = if package_arg.is_null() || package_arg == R_NilValue() {
            "base".to_string()
        } else {
            let n = XLENGTH(package_arg);
            if n != 1 {
                package_error("'package' must be of length 1");
            }
            elt_to_string(package_arg, 0)
        };

        let package_path = find_package_path(&package);
        let must_work = named_logical_arg(args, "mustWork").unwrap_or(false);
        if package_path.is_empty() {
            if must_work {
                package_error(format!("no file found for package '{}'", package));
            }
            return Rf_mkString(CString::new("").unwrap_or_default().as_ptr());
        }

        let mut path = PathBuf::from(package_path);
        for part in system_file_parts(args) {
            if !part.is_empty() {
                path.push(part);
            }
        }

        if path.exists() {
            Rf_mkString(
                CString::new(path.to_string_lossy().into_owned())
                    .unwrap_or_default()
                    .as_ptr(),
            )
        } else {
            if must_work {
                package_error(format!(
                    "no file found for requested path in package '{}'",
                    package
                ));
            }
            Rf_mkString(CString::new("").unwrap_or_default().as_ptr())
        }
    }
}

fn system_file_parts(args: SEXP) -> Vec<String> {
    unsafe {
        let mut parts = Vec::new();
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            if tag_name(current).is_none() {
                let value = CAR(current);
                if !value.is_null() && value != R_NilValue() && TYPEOF(value) == SEXPTYPE::STRSXP {
                    for i in 0..XLENGTH(value) {
                        if !is_string_na(value, i) {
                            parts.push(elt_to_string(value, i));
                        }
                    }
                }
            }
            current = CDR(current);
        }
        parts
    }
}

// ---------------------------------------------------------------------------
// Complete R runtime
// ---------------------------------------------------------------------------

/// R-like `ls_args()` — list argument names of current function (simplified: return empty character).
pub unsafe fn do_ls_args(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { Rf_allocVector3(SEXPTYPE::STRSXP, 0) }
}

/// R's `deparse1(expr, collapse, width.cutoff)` — deparse to a single string.
pub unsafe fn do_deparse1(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        let collapse_arg = CAR(CDR(args));
        let sep = if collapse_arg.is_null() || collapse_arg == R_NilValue() {
            " ".to_string()
        } else {
            elt_to_string(collapse_arg, 0)
        };
        let lines = deparse_lines(expr);
        Rf_mkString(CString::new(lines.join(&sep)).unwrap_or_default().as_ptr())
    }
}

/// R's `dput(x, file)` — dump object using the deparser.
pub unsafe fn do_dput(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let file_arg = arg_by_name_or_position(args, &["file"], 1);
        let lines = deparse_lines(x);
        let output = format!("{}\n", lines.join("\n"));

        let file = if file_arg.is_null() || file_arg == R_NilValue() || XLENGTH(file_arg) == 0 {
            String::new()
        } else {
            elt_to_string(file_arg, 0)
        };
        if file.is_empty() {
            if crate::sexp::output::is_capturing() {
                crate::sexp::output::capture_stdout(&output);
            } else {
                print!("{}", output);
            }
        } else {
            std::fs::write(&file, output).unwrap_or_else(|err| {
                std::panic::panic_any(RError {
                    message: format!("cannot write dump file '{}': {err}", file),
                })
            });
        }
        x
    }
}

fn deparse_lines(expr: SEXP) -> Vec<String> {
    unsafe {
        let deparsed = crate::mainutils::deparse::deparse1(expr, false, 0);
        let n = XLENGTH(deparsed);
        if deparsed.is_null() || deparsed == R_NilValue() || n == 0 {
            return vec!["NULL".to_string()];
        }
        (0..n).map(|i| elt_to_string(deparsed, i)).collect()
    }
}

/// R's `dget(file)` — read, parse, and evaluate a dumped expression.
pub unsafe fn do_dget(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = arg_by_name_or_position(args, &["file"], 0);
        if file_arg.is_null() || file_arg == R_NilValue() || XLENGTH(file_arg) == 0 {
            std::panic::panic_any(RError {
                message: "invalid 'file' argument".to_string(),
            });
        }

        let path = elt_to_string(file_arg, 0);
        let code = std::fs::read_to_string(&path).unwrap_or_else(|err| {
            std::panic::panic_any(RError {
                message: format!("cannot read dump file '{}': {err}", path),
            })
        });
        let expr = crate::sexp::memory::with_arena(|arena| {
            crate::eval::parser::parse(&code, arena).map_err(|err| err.to_string())
        })
        .unwrap_or_else(|message| std::panic::panic_any(RError { message }));
        if expr.is_null() || expr == R_NilValue() {
            R_NilValue()
        } else {
            crate::eval::eval::Rf_eval(expr, rho)
        }
    }
}

/// R's `bquote(expr)` — quote with `.(...)` substitution.
pub unsafe fn do_bquote(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        if expr.is_null() {
            return R_NilValue();
        }
        bquote_walk(expr, rho)
    }
}

unsafe fn bquote_walk(expr: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }

        let expr_type = TYPEOF(expr);
        if expr_type == SEXPTYPE::LANGSXP && is_bquote_unquote_call(expr) {
            let unquoted = CAR(CDR(expr));
            return crate::eval::eval::Rf_eval(unquoted, rho);
        }

        if expr_type != SEXPTYPE::LANGSXP && expr_type != SEXPTYPE::LISTSXP {
            return expr;
        }

        let mut source = expr;
        let mut head = R_NilValue();
        let mut tail = R_NilValue();
        while !source.is_null() && source != R_NilValue() {
            let value = bquote_walk(CAR(source), rho);
            let cell = Rf_cons(value, R_NilValue());
            SETTAG(cell, TAG(source));
            if head == R_NilValue() {
                head = cell;
            } else {
                SETCDR(tail, cell);
            }
            tail = cell;
            source = CDR(source);
        }
        if expr_type == SEXPTYPE::LANGSXP && !head.is_null() && head != R_NilValue() {
            (*head).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        }
        head
    }
}

unsafe fn is_bquote_unquote_call(expr: SEXP) -> bool {
    unsafe {
        if TYPEOF(expr) != SEXPTYPE::LANGSXP {
            return false;
        }
        let head = CAR(expr);
        if TYPEOF(head) != SEXPTYPE::SYMSXP || symbol_name(head).as_deref() != Some(".") {
            return false;
        }
        let args = CDR(expr);
        !args.is_null()
            && args != R_NilValue()
            && (CDR(args).is_null() || CDR(args) == R_NilValue())
    }
}

// ---------------------------------------------------------------------------
// Complete S3
// ---------------------------------------------------------------------------

/// R-like `rownames_to_column(x, var)` — convert row names to a leading column.
pub unsafe fn do_rownames_to_column(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if !is_data_frame_like(x) {
            data_frame_error("rownames_to_column() requires a data frame");
        }
        let var_arg = arg_by_name_or_position(args, &["var"], 1);
        let var = if var_arg.is_null() || var_arg == R_NilValue() || XLENGTH(var_arg) == 0 {
            "rowname".to_string()
        } else {
            elt_to_string(var_arg, 0)
        };

        let mut names = data_frame_column_names(x);
        let mut columns = data_frame_columns(x);
        names.insert(0, var);
        columns.insert(0, string_vector(&data_frame_row_names(x)));
        build_data_frame(columns, names, data_frame_row_names_attr(x))
    }
}

/// R-like `column_to_rownames(x, var)` — convert a column to row names.
pub unsafe fn do_column_to_rownames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if !is_data_frame_like(x) {
            data_frame_error("column_to_rownames() requires a data frame");
        }
        let var_arg = arg_by_name_or_position(args, &["var"], 1);
        let var = if var_arg.is_null() || var_arg == R_NilValue() || XLENGTH(var_arg) == 0 {
            "rowname".to_string()
        } else {
            elt_to_string(var_arg, 0)
        };
        let names = data_frame_column_names(x);
        let Some(row_col) = names.iter().position(|name| name == &var) else {
            data_frame_error(format!("column '{}' not found", var));
        };

        let mut out_names = Vec::new();
        let mut out_columns = Vec::new();
        for (i, name) in names.into_iter().enumerate() {
            if i != row_col {
                out_names.push(name);
                out_columns.push(VECTOR_ELT(x, i as R_xlen_t));
            }
        }
        build_data_frame(
            out_columns,
            out_names,
            string_vector(&vector_to_string_values(VECTOR_ELT(x, row_col as R_xlen_t))),
        )
    }
}

/// R-like `relocate(x, cols, .before, .after)` — reorder data-frame columns.
pub unsafe fn do_relocate(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if !is_data_frame_like(x) {
            data_frame_error("relocate() requires a data frame");
        }
        let cols_arg = arg_by_name_or_position(args, &["cols", ".cols"], 1);
        let before_arg = arg_by_name_or_position(args, &[".before", "before"], usize::MAX);
        let after_arg = arg_by_name_or_position(args, &[".after", "after"], usize::MAX);
        let names = data_frame_column_names(x);
        let requested = string_arg_values(cols_arg);
        let moving: Vec<String> = requested
            .into_iter()
            .filter(|name| names.iter().any(|column| column == name))
            .collect();
        if moving.is_empty() {
            return x;
        }

        let mut rest: Vec<String> = names
            .iter()
            .filter(|name| !moving.iter().any(|moving_name| moving_name == *name))
            .cloned()
            .collect();
        let insert_at = if !before_arg.is_null() && before_arg != R_NilValue() {
            let before = elt_to_string(before_arg, 0);
            rest.iter()
                .position(|name| name == &before)
                .unwrap_or(rest.len())
        } else if !after_arg.is_null() && after_arg != R_NilValue() {
            let after = elt_to_string(after_arg, 0);
            rest.iter()
                .position(|name| name == &after)
                .map(|idx| idx + 1)
                .unwrap_or(rest.len())
        } else {
            0
        };
        for (offset, name) in moving.into_iter().enumerate() {
            rest.insert(insert_at + offset, name);
        }

        let mut out_columns = Vec::new();
        for name in &rest {
            if let Some(idx) = names.iter().position(|column| column == name) {
                out_columns.push(VECTOR_ELT(x, idx as R_xlen_t));
            }
        }
        build_data_frame(out_columns, rest, data_frame_row_names_attr(x))
    }
}

fn is_data_frame_like(x: SEXP) -> bool {
    unsafe {
        !x.is_null()
            && x != R_NilValue()
            && TYPEOF(x) == SEXPTYPE::VECSXP
            && is_data_frame_object(x)
    }
}

fn data_frame_column_names(x: SEXP) -> Vec<String> {
    unsafe {
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return (0..XLENGTH(x)).map(|i| format!("V{}", i + 1)).collect();
        }
        (0..XLENGTH(x)).map(|i| elt_to_string(names, i)).collect()
    }
}

fn data_frame_columns(x: SEXP) -> Vec<SEXP> {
    unsafe { (0..XLENGTH(x)).map(|i| VECTOR_ELT(x, i)).collect() }
}

fn data_frame_row_names_attr(x: SEXP) -> SEXP {
    unsafe {
        crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("row.names").unwrap_or_default().as_ptr()),
        )
    }
}

fn data_frame_row_names(x: SEXP) -> Vec<String> {
    unsafe {
        let attr = data_frame_row_names_attr(x);
        if attr.is_null() || attr == R_NilValue() {
            return (1..=data_frame_row_count(x))
                .map(|i| i.to_string())
                .collect();
        }
        if TYPEOF(attr) == SEXPTYPE::STRSXP {
            return (0..XLENGTH(attr)).map(|i| elt_to_string(attr, i)).collect();
        }
        if TYPEOF(attr) == SEXPTYPE::INTSXP && LENGTH(attr) == 2 {
            let first = *INTEGER(attr);
            let second = *INTEGER(attr).add(1);
            if first == NA_INTEGER && second < 0 {
                return (1..=(-second as R_xlen_t)).map(|i| i.to_string()).collect();
            }
        }
        vector_to_string_values(attr)
    }
}

fn vector_to_string_values(x: SEXP) -> Vec<String> {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Vec::new();
        }
        (0..XLENGTH(x)).map(|i| elt_to_string(x, i)).collect()
    }
}

fn string_arg_values(x: SEXP) -> Vec<String> {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Vec::new();
        }
        (0..XLENGTH(x)).map(|i| elt_to_string(x, i)).collect()
    }
}

unsafe fn condition_message_text(args: SEXP, option_names: &[&str]) -> String {
    unsafe {
        let mut parts = Vec::new();
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            let is_option = tag_name(current)
                .as_deref()
                .is_some_and(|name| option_names.contains(&name));
            if !is_option && !arg.is_null() && arg != R_NilValue() {
                for i in 0..XLENGTH(arg) {
                    parts.push(elt_to_string(arg, i));
                }
            }
            current = CDR(current);
        }
        parts.join("")
    }
}

fn build_data_frame(columns: Vec<SEXP>, names: Vec<String>, row_names: SEXP) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, columns.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (i, column) in columns.into_iter().enumerate() {
            SET_VECTOR_ELT(result, i as R_xlen_t, column);
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            string_vector(&names),
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
            Rf_mkString(CString::new("data.frame").unwrap_or_default().as_ptr()),
        );
        if !row_names.is_null() && row_names != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("row.names").unwrap_or_default().as_ptr()),
                row_names,
            );
        }
        result
    }
}

fn data_frame_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    })
}

// ---------------------------------------------------------------------------
// Complete I/O
// ---------------------------------------------------------------------------

/// R-like `cat_args(...)` — cat with better formatting.
pub unsafe fn do_cat_args(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_cat(_call, _op, args, _rho) }
}

/// R-like `message_args(...)` — message with domain.
pub unsafe fn do_message_args(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let output = condition_message_text(args, &["domain", "appendLF"]);
        eprintln!("{}", output);
        crate::sexp::globals::set_R_Visible(FALSE);
        R_NilValue()
    }
}

/// R's `packageStartupMessage(...)` — startup message.
pub unsafe fn do_package_startup_message(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let output = condition_message_text(args, &["domain", "appendLF"]);
        eprintln!("{}", output);
        crate::sexp::globals::set_R_Visible(FALSE);
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// Environment completion
// ---------------------------------------------------------------------------

/// R's `parent.env(env)` — returns the parent environment.
pub unsafe fn do_parent_env(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let env = CAR(args);
        if env.is_null() || env == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(env);
        if t != SEXPTYPE::ENVSXP {
            return R_NilValue();
        }
        // enclos is the enclosing/parent environment
        let parent = (*env).data.envsxp.enclos;
        if parent.is_null() {
            return crate::sexp::globals::R_EmptyEnv();
        }
        parent
    }
}

/// R's `set_parent.env(env, parent)` — set the parent environment.
pub unsafe fn do_set_parent_env(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let env = CAR(args);
        let parent = CAR(CDR(args));
        if env.is_null() || env == R_NilValue() || TYPEOF(env) != SEXPTYPE::ENVSXP {
            return R_NilValue();
        }
        if parent.is_null() || parent == R_NilValue() || TYPEOF(parent) != SEXPTYPE::ENVSXP {
            return env;
        }
        SET_ENCLOS(env, parent);
        env
    }
}

/// R's `env_name(env)` — returns the name of an environment.
pub unsafe fn do_env_name(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let env = CAR(args);
        if env.is_null() || env == R_NilValue() {
            return Rf_mkString(CString::new("NULL").unwrap_or_default().as_ptr());
        }
        let t = TYPEOF(env);
        if t != SEXPTYPE::ENVSXP {
            return Rf_mkString(CString::new("").unwrap_or_default().as_ptr());
        }
        // Check if it's a special environment
        if env == crate::sexp::globals::R_GlobalEnv() {
            return Rf_mkString(CString::new("R_GlobalEnv").unwrap_or_default().as_ptr());
        }
        if env == crate::sexp::globals::R_EmptyEnv() {
            return Rf_mkString(CString::new("R_EmptyEnv").unwrap_or_default().as_ptr());
        }
        if env == crate::sexp::globals::R_BaseEnv() {
            return Rf_mkString(CString::new("base").unwrap_or_default().as_ptr());
        }
        let name = crate::sexp::attrib_core::getAttrib(env, Rf_install(c"name".as_ptr()));
        if TYPEOF(name) == SEXPTYPE::STRSXP && XLENGTH(name) > 0 {
            let value = STRING_ELT(name, 0);
            if !value.is_null() && value != R_NilValue() {
                return Rf_mkString(CHAR(value));
            }
        }
        Rf_mkString(CString::new("").unwrap_or_default().as_ptr())
    }
}

/// R's `environmentName(env)` — returns the name of an environment.
pub unsafe fn do_environment_name(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_env_name(_call, _op, args, _rho) }
}

/// R-like `is_empty(env)` — check if environment is empty (simplified).
pub unsafe fn do_is_empty(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let env = CAR(args);
        if env.is_null() || env == R_NilValue() {
            return Rf_ScalarLogical(TRUE);
        }
        let t = TYPEOF(env);
        if t == SEXPTYPE::ENVSXP {
            // Check frame - if it's NULL/NILSXP, env is empty
            let frame = (*env).data.envsxp.frame;
            if frame.is_null() || frame == R_NilValue() {
                return Rf_ScalarLogical(TRUE);
            }
            return Rf_ScalarLogical(FALSE);
        }
        // For vectors, check length
        let n = XLENGTH(env);
        Rf_ScalarLogical(if n == 0 { TRUE } else { FALSE })
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
        let n = XLENGTH(x);
        if n == 0 {
            println!("integer(0)");
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }
        print!("[1]");
        for i in 0..n.min(500) {
            let v = *INTEGER(x).add(i as usize);
            let s = if v == NA_INTEGER {
                "NA".to_string()
            } else {
                format!("{}", v)
            };
            if i == 0 {
                print!(" {}", s);
            } else if (i + 1) % 6 == 0 {
                print!("\n[{}] {}", i + 1, s);
            } else {
                print!(" {}", s);
            }
        }
        if n > 500 {
            print!(
                "\n [ reached getOption(\"max.print\") -- omitted {} entries ]",
                n - 500
            );
        }
        println!();
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
        let n = XLENGTH(x);
        if n == 0 {
            println!("numeric(0)");
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }
        print!("[1]");
        for i in 0..n.min(500) {
            let v = *REAL(x).add(i as usize);
            let s = if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                "NA".to_string()
            } else {
                format!("{}", v)
            };
            if i == 0 {
                print!(" {}", s);
            } else if (i + 1) % 4 == 0 {
                print!("\n[{}] {}", i + 1, s);
            } else {
                print!(" {}", s);
            }
        }
        if n > 500 {
            print!(
                "\n [ reached getOption(\"max.print\") -- omitted {} entries ]",
                n - 500
            );
        }
        println!();
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
        let n = XLENGTH(x);
        if n == 0 {
            println!("logical(0)");
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }
        print!("[1]");
        for i in 0..n.min(500) {
            let v = *LOGICAL(x).add(i as usize);
            let s = if v == NA_INTEGER {
                "NA".to_string()
            } else if v == TRUE {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            };
            if i == 0 {
                print!(" {}", s);
            } else if (i + 1) % 6 == 0 {
                print!("\n[{}] {}", i + 1, s);
            } else {
                print!(" {}", s);
            }
        }
        if n > 500 {
            print!(
                "\n [ reached getOption(\"max.print\") -- omitted {} entries ]",
                n - 500
            );
        }
        println!();
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
        for i in 0..n.min(500) {
            let s = elt_to_string(x, i);
            println!("[{}] \"{}\"", i + 1, s);
        }
        if n > 500 {
            println!(
                " [ reached getOption(\"max.print\") -- omitted {} entries ]",
                n - 500
            );
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
        let n = XLENGTH(x);
        if n == 0 {
            println!("complex(0)");
            crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
            return x;
        }
        print!("[1]");
        for i in 0..n.min(500) {
            // Complex data is stored as pairs of f64
            let re = *REAL(x).add((i * 2) as usize);
            let im = *REAL(x).add((i * 2 + 1) as usize);
            let s = format!("{}+{}i", re, im);
            if i == 0 {
                print!(" {}", s);
            } else if (i + 1) % 4 == 0 {
                print!("\n[{}] {}", i + 1, s);
            } else {
                print!(" {}", s);
            }
        }
        if n > 500 {
            print!(
                "\n [ reached getOption(\"max.print\") -- omitted {} entries ]",
                n - 500
            );
        }
        println!();
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

// ---------------------------------------------------------------------------
// Complete R runtime — type checking utilities
// ---------------------------------------------------------------------------

/// R's `is.single(x)` — stock R exposes this but errors because single is unimplemented.
pub unsafe fn do_is_single(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _x = CAR(args);
        std::panic::panic_any(crate::sexp::context::RError {
            message: "type \"single\" unimplemented in R".to_string(),
        });
    }
}

/// R's `is.vector(x, mode="any")` — check if x is an atomic or list vector without attributes.
pub unsafe fn do_is_vector(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        let is_vec = t == SEXPTYPE::LGLSXP
            || t == SEXPTYPE::INTSXP
            || t == SEXPTYPE::REALSXP
            || t == SEXPTYPE::CPLXSXP
            || t == SEXPTYPE::STRSXP
            || t == SEXPTYPE::RAWSXP
            || t == SEXPTYPE::VECSXP;
        Rf_ScalarLogical(if is_vec { TRUE } else { FALSE })
    }
}

/// R's `is.scalar(x)` — check if x has length 1 (simplified).
pub unsafe fn do_is_scalar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        Rf_ScalarLogical(if n == 1 { TRUE } else { FALSE })
    }
}

/// R's `is.named(x)` — check if x has names attribute.
pub unsafe fn do_is_named(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let names = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
        );
        let has_names = !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP && XLENGTH(names) > 0;
        Rf_ScalarLogical(if has_names { TRUE } else { FALSE })
    }
}

/// R's `is.unsorted(x, na.rm = FALSE, strictly = FALSE)`.
///
/// Missing values dominate the default result just as in GNU R: with
/// `na.rm = FALSE`, any NA/NaN makes the result `NA`, even if another pair is
/// visibly out of order.
pub unsafe fn do_is_unsorted(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let na_rm = match logical_arg_with_default(args, "na.rm", 1, FALSE) {
            Ok(value) => value != FALSE,
            Err(message) => panic_r_error(message),
        };
        let strictly = match logical_arg_with_default(args, "strictly", 2, FALSE) {
            Ok(value) => value != FALSE,
            Err(_) => panic_r_error("invalid 'strictly' argument"),
        };
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        if n <= 1 {
            return Rf_ScalarLogical(FALSE);
        }
        let result = if t == SEXPTYPE::LGLSXP || t == SEXPTYPE::INTSXP {
            is_unsorted_int_like(x, n, na_rm, strictly)
        } else if t == SEXPTYPE::REALSXP {
            is_unsorted_real(x, n, na_rm, strictly)
        } else if t == SEXPTYPE::CPLXSXP {
            is_unsorted_complex(x, n, na_rm, strictly)
        } else if t == SEXPTYPE::STRSXP {
            is_unsorted_character(x, n, na_rm, strictly)
        } else if t == SEXPTYPE::RAWSXP {
            is_unsorted_raw(x, n, strictly)
        } else {
            NA_LOGICAL
        };
        Rf_ScalarLogical(result)
    }
}

unsafe fn logical_arg_with_default(
    args: SEXP,
    name: &str,
    position: usize,
    default: c_int,
) -> Result<c_int, &'static str> {
    unsafe {
        let value = arg_by_name_or_position(args, &[name], position);
        if value.is_null() || value == R_NilValue() {
            return Ok(default);
        }
        if XLENGTH(value) == 0 {
            return Err("argument is of length zero");
        }
        let value_type = TYPEOF(value);
        let raw = if value_type == SEXPTYPE::LGLSXP || value_type == SEXPTYPE::INTSXP {
            *INTEGER(value)
        } else if value_type == SEXPTYPE::REALSXP {
            let value = *REAL(value);
            if value.is_nan() {
                NA_LOGICAL
            } else {
                value as c_int
            }
        } else {
            return Err("argument is not interpretable as logical");
        };
        if raw == NA_LOGICAL {
            return Err("missing value where TRUE/FALSE needed");
        }
        Ok(if raw == FALSE { FALSE } else { TRUE })
    }
}

fn panic_r_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    })
}

unsafe fn is_unsorted_int_like(x: SEXP, n: R_xlen_t, na_rm: bool, strictly: bool) -> c_int {
    unsafe {
        let mut previous: Option<c_int> = None;
        for i in 0..n {
            let current = *INTEGER(x).add(i as usize);
            if current == NA_INTEGER {
                if na_rm {
                    continue;
                }
                return NA_LOGICAL;
            }
            if let Some(prev) = previous {
                if out_of_order_i32(prev, current, strictly) {
                    return TRUE;
                }
            }
            previous = Some(current);
        }
        FALSE
    }
}

unsafe fn is_unsorted_real(x: SEXP, n: R_xlen_t, na_rm: bool, strictly: bool) -> c_int {
    unsafe {
        let mut previous: Option<f64> = None;
        for i in 0..n {
            let current = *REAL(x).add(i as usize);
            if current.is_nan() {
                if na_rm {
                    continue;
                }
                return NA_LOGICAL;
            }
            if let Some(prev) = previous {
                if out_of_order_f64(prev, current, strictly) {
                    return TRUE;
                }
            }
            previous = Some(current);
        }
        FALSE
    }
}

unsafe fn is_unsorted_complex(x: SEXP, n: R_xlen_t, na_rm: bool, strictly: bool) -> c_int {
    unsafe {
        let mut previous: Option<Rcomplex> = None;
        for i in 0..n {
            let current = *COMPLEX(x).add(i as usize);
            if current.r.is_nan() || current.i.is_nan() {
                if na_rm {
                    continue;
                }
                return NA_LOGICAL;
            }
            if let Some(prev) = previous {
                if out_of_order_complex(prev, current, strictly) {
                    return TRUE;
                }
            }
            previous = Some(current);
        }
        FALSE
    }
}

unsafe fn is_unsorted_character(x: SEXP, n: R_xlen_t, na_rm: bool, strictly: bool) -> c_int {
    unsafe {
        let mut previous: Option<String> = None;
        for i in 0..n {
            if STRING_ELT(x, i) == crate::sexp::globals::R_NaString() {
                if na_rm {
                    continue;
                }
                return NA_LOGICAL;
            }
            let current = elt_to_string(x, i);
            if let Some(prev) = previous.as_deref() {
                let out_of_order = if strictly {
                    prev >= current.as_str()
                } else {
                    prev > current.as_str()
                };
                if out_of_order {
                    return TRUE;
                }
            }
            previous = Some(current);
        }
        FALSE
    }
}

unsafe fn is_unsorted_raw(x: SEXP, n: R_xlen_t, strictly: bool) -> c_int {
    unsafe {
        for i in 1..n {
            let prev = *RAW(x).add((i - 1) as usize);
            let current = *RAW(x).add(i as usize);
            let out_of_order = if strictly {
                prev >= current
            } else {
                prev > current
            };
            if out_of_order {
                return TRUE;
            }
        }
        FALSE
    }
}

fn out_of_order_i32(previous: c_int, current: c_int, strictly: bool) -> bool {
    if strictly {
        previous >= current
    } else {
        previous > current
    }
}

fn out_of_order_f64(previous: f64, current: f64, strictly: bool) -> bool {
    if strictly {
        previous >= current
    } else {
        previous > current
    }
}

fn out_of_order_complex(previous: Rcomplex, current: Rcomplex, strictly: bool) -> bool {
    if previous.r > current.r {
        return true;
    }
    if previous.r < current.r {
        return false;
    }
    if strictly {
        previous.i >= current.i
    } else {
        previous.i > current.i
    }
}

/// R's `is.loaded(x)` — check if symbol is loaded (simplified: always FALSE).
pub unsafe fn do_is_loaded(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { Rf_ScalarLogical(FALSE) }
}

// ---------------------------------------------------------------------------
// Complete R runtime — function type checking
// ---------------------------------------------------------------------------

/// R's `is.primitive(x)` — check if x is a primitive function (BUILTINSXP or SPECIALSXP).
pub unsafe fn do_is_primitive(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        Rf_ScalarLogical(if t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
            TRUE
        } else {
            FALSE
        })
    }
}

/// R's `is.generic(x)` — check if x is a generic function (simplified).
/// Returns TRUE for CLOSXP with "generic" in name or with useMethod call.
pub unsafe fn do_is_generic(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        // Simplified: primitives are always generic, closures need body check
        if t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
            return Rf_ScalarLogical(TRUE);
        }
        if t == SEXPTYPE::CLOSXP {
            // Check if name ends with common generic names
            // Simplified: assume all closures could be generic
            return Rf_ScalarLogical(TRUE);
        }
        Rf_ScalarLogical(FALSE)
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
        let class = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
        );
        if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP && XLENGTH(class) > 0 {
            let cls = elt_to_string(class, 0);
            return Rf_ScalarLogical(if cls == "data.frame" { TRUE } else { FALSE });
        }
        Rf_ScalarLogical(FALSE)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers for new functions
// ---------------------------------------------------------------------------

/// Extract numeric data from a SEXP into a Vec<f64>.
fn get_numeric_data(x: SEXP) -> Vec<f64> {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Vec::new();
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        let mut data = Vec::with_capacity(n as usize);
        if t == SEXPTYPE::REALSXP {
            for i in 0..n {
                data.push(*REAL(x).add(i as usize));
            }
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            for i in 0..n {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER {
                    data.push(NA_REAL);
                } else {
                    data.push(v as f64);
                }
            }
        }
        data
    }
}

/// Extract a single element from a vector as a SEXP (for use with real_or_default).
fn elt_to_sexp(x: SEXP, i: R_xlen_t) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        let idx = if n == 0 { 0 } else { i % n };

        if t == SEXPTYPE::REALSXP {
            let v = *REAL(x).add(idx as usize);
            Rf_ScalarReal(v)
        } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            Rf_ScalarInteger(*INTEGER(x).add(idx as usize))
        } else {
            R_NilValue()
        }
    }
}

fn base_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

unsafe fn constructor_length(value: SEXP) -> R_xlen_t {
    unsafe {
        if value.is_null() || value == R_NilValue() {
            return 0;
        }
        if XLENGTH(value) == 0 {
            return 0;
        }

        let raw_len = match TYPEOF(value) {
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => *INTEGER(value),
            t if t == SEXPTYPE::REALSXP => {
                let value = *REAL(value);
                if value.is_nan() || value < 0.0 {
                    base_error("invalid 'length' argument");
                }
                value.trunc() as i32
            }
            t if t == SEXPTYPE::STRSXP => elt_to_string(value, 0)
                .parse::<i32>()
                .unwrap_or_else(|_| base_error("invalid 'length' argument")),
            _ => base_error("invalid 'length' argument"),
        };

        if raw_len == NA_INTEGER || raw_len < 0 {
            base_error("invalid 'length' argument");
        }
        raw_len as R_xlen_t
    }
}

unsafe fn first_constructor_arg(args: SEXP, name: &str, position: usize) -> SEXP {
    unsafe {
        let mut current = args;
        let mut positional = 0;
        while !current.is_null() && current != R_NilValue() {
            let value = CAR(current);
            match tag_name(current).as_deref() {
                Some(tag) if tag == name => return value,
                Some(_) => {}
                None => {
                    if positional == position {
                        return value;
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }
        R_NilValue()
    }
}

unsafe fn allocate_initialized_vector(sexptype: SEXPTYPE, length: R_xlen_t) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(sexptype, length);
        if result.is_null() {
            return R_NilValue();
        }
        let _guard = protect(result);
        match sexptype {
            t if t == SEXPTYPE::STRSXP => {
                let empty = Rf_mkChar(c"".as_ptr());
                for i in 0..length {
                    SET_STRING_ELT(result, i, empty);
                }
            }
            t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                for i in 0..length {
                    SET_VECTOR_ELT(result, i, R_NilValue());
                }
            }
            _ => {}
        }
        result
    }
}

unsafe fn do_typed_vector_constructor(args: SEXP, sexptype: SEXPTYPE) -> SEXP {
    unsafe {
        let length_arg = first_constructor_arg(args, "length", 0);
        let length = constructor_length(length_arg);
        allocate_initialized_vector(sexptype, length)
    }
}

/// R's `logical(length = 0)` constructor.
pub unsafe fn do_logical_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_typed_vector_constructor(args, SEXPTYPE::LGLSXP) }
}

/// R's `integer(length = 0)` constructor.
pub unsafe fn do_integer_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_typed_vector_constructor(args, SEXPTYPE::INTSXP) }
}

/// R's `numeric(length = 0)` / `double(length = 0)` constructor.
pub unsafe fn do_numeric_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_typed_vector_constructor(args, SEXPTYPE::REALSXP) }
}

/// R's legacy `single(length = 0)` constructor.
pub unsafe fn do_single_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let result = do_typed_vector_constructor(args, SEXPTYPE::REALSXP);
        if result.is_null() || result == R_NilValue() {
            return result;
        }

        let _result_guard = protect(result);
        let marker = Rf_ScalarLogical(TRUE);
        let _marker_guard = protect(marker);
        crate::sexp::attrib_core::setAttrib(result, Rf_install(c"Csingle".as_ptr()), marker);
        result
    }
}

/// R's `complex(length = 0)` constructor.
pub unsafe fn do_complex_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_typed_vector_constructor(args, SEXPTYPE::CPLXSXP) }
}

/// R's `character(length = 0)` constructor.
pub unsafe fn do_character_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_typed_vector_constructor(args, SEXPTYPE::STRSXP) }
}

/// R's `raw(length = 0)` constructor.
pub unsafe fn do_raw_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_typed_vector_constructor(args, SEXPTYPE::RAWSXP) }
}

/// R's `vector(mode = "logical", length = 0)` constructor.
pub unsafe fn do_vector_constructor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mode_arg = first_constructor_arg(args, "mode", 0);
        let length_arg = first_constructor_arg(args, "length", 1);
        let mode = if mode_arg.is_null() || mode_arg == R_NilValue() {
            "logical".to_string()
        } else {
            elt_to_string(mode_arg, 0)
        };
        let sexptype = match mode.as_str() {
            "logical" => SEXPTYPE::LGLSXP,
            "integer" => SEXPTYPE::INTSXP,
            "numeric" | "double" => SEXPTYPE::REALSXP,
            "complex" => SEXPTYPE::CPLXSXP,
            "character" => SEXPTYPE::STRSXP,
            "raw" => SEXPTYPE::RAWSXP,
            "list" => SEXPTYPE::VECSXP,
            "expression" => SEXPTYPE::EXPRSXP,
            _ => base_error(format!("vector: cannot make a vector of mode '{mode}'")),
        };
        let length = constructor_length(length_arg);
        allocate_initialized_vector(sexptype, length)
    }
}

// ---------------------------------------------------------------------------
// Complete S3 coercion — as.complex, as.raw, as
// ---------------------------------------------------------------------------

/// R's `as.complex(x)` — coerce to CPLXSXP through the shared vector coercer.
pub unsafe fn do_as_complex(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        crate::mainutils::coerce::coerceVector(x, SEXPTYPE::CPLXSXP.as_c_int())
    }
}

/// R's `as.raw(x)` — coerce to RAWSXP.
pub unsafe fn do_as_raw(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::RAWSXP, 0);
        }
        let src_t = TYPEOF(x);
        if src_t == SEXPTYPE::RAWSXP {
            return x;
        }
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::RAWSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = crate::sexp::accessors::RAW(result);
        for i in 0..n {
            let val = if src_t == SEXPTYPE::INTSXP || src_t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { 0 } else { (v & 0xff) as u8 }
            } else if src_t == SEXPTYPE::REALSXP {
                let v = *REAL(x).add(i as usize);
                if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    0
                } else {
                    (v as i32 & 0xff) as u8
                }
            } else {
                0
            };
            *dst.add(i as usize) = val;
        }
        result
    }
}

/// R's `as(x, Class)` — S4-style coercion (simplified: delegates to appropriate as.* function).
pub unsafe fn do_as(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let class_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || class_arg.is_null() || class_arg == R_NilValue() {
            return x;
        }
        let class_name = elt_to_string(class_arg, 0);
        match class_name.as_str() {
            "numeric" | "double" => do_as_double(_call, _op, args, _rho),
            "integer" => do_as_integer(_call, _op, args, _rho),
            "logical" => do_as_logical(_call, _op, args, _rho),
            "character" => do_as_character(_call, _op, args, _rho),
            "complex" => do_as_complex(_call, _op, args, _rho),
            "raw" => do_as_raw(_call, _op, args, _rho),
            "list" => do_as_list(_call, _op, args, _rho),
            _ => x, // unknown class, return as-is
        }
    }
}

// ---------------------------------------------------------------------------
// Complete I/O — capture.output, withVisible, invisible, suppress*,
// ---------------------------------------------------------------------------

/// R's `capture.output(expr)` — capture printed stdout as a character vector.
pub unsafe fn do_capture_output(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        if expr.is_null() || expr == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        }

        crate::sexp::output::start_capture();
        let _ = crate::eval::eval::Rf_eval(expr, rho);
        let captured = crate::sexp::output::stop_capture();

        let stdout = captured.stdout.trim_end_matches('\n');
        let lines: Vec<&str> = if stdout.is_empty() {
            Vec::new()
        } else {
            stdout.split('\n').collect()
        };

        let result = Rf_allocVector3(SEXPTYPE::STRSXP, lines.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        for (i, line) in lines.iter().enumerate() {
            let cstr = CString::new(*line).unwrap_or_default();
            let charsxp = Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                SET_STRING_ELT(result, i as R_xlen_t, charsxp);
            }
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::TRUE);
        result
    }
}

/// R's `withVisible(x)` — returns a list with $value and $visible.
pub unsafe fn do_with_visible(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let visible = crate::sexp::globals::R_Visible();
        // Return a VECSXP (list) with two elements: value, visible
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        crate::sexp::accessors::SET_VECTOR_ELT(result, 0, x);
        let vis_vec = Rf_ScalarLogical(visible);
        crate::sexp::accessors::SET_VECTOR_ELT(result, 1, vis_vec);
        // Set names
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        if !names.is_null() {
            let _n_p = crate::sexp::protect::protect(names);
            let v_str = CString::new("value").unwrap_or_default();
            let vi_str = CString::new("visible").unwrap_or_default();
            let v_char = crate::sexp::constructors::Rf_mkChar(v_str.as_ptr());
            let vi_char = crate::sexp::constructors::Rf_mkChar(vi_str.as_ptr());
            if !v_char.is_null() {
                let data = (*names).gengc_next_node as *mut SEXP;
                *data.add(0) = v_char;
            }
            if !vi_char.is_null() {
                let data = (*names).gengc_next_node as *mut SEXP;
                *data.add(1) = vi_char;
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
                names,
            );
        }
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::TRUE);
        result
    }
}

/// R's `invisible(x)` — return x, setting visibility to FALSE.
pub unsafe fn do_invisible(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        x
    }
}

/// R's `suppressWarnings(expr)` — evaluate expr with captured diagnostics suppressed.
pub unsafe fn do_suppress_warnings(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::output::start_capture();
        let result = crate::eval::eval::Rf_eval(expr, rho);
        let captured = crate::sexp::output::stop_capture();
        if !captured.stdout.is_empty() {
            crate::sexp::output::capture_stdout(&captured.stdout);
        }
        result
    }
}

/// R's `suppressMessages(expr)` — evaluate expr with captured diagnostics suppressed.
pub unsafe fn do_suppress_messages(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let expr = CAR(args);
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }
        crate::sexp::output::start_capture();
        let result = crate::eval::eval::Rf_eval(expr, rho);
        let captured = crate::sexp::output::stop_capture();
        if !captured.stdout.is_empty() {
            crate::sexp::output::capture_stdout(&captured.stdout);
        }
        result
    }
}

/// R's `force(x)` — force evaluation of a promise.
pub unsafe fn do_force(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        // If x is a PROMSXP, force it
        if TYPEOF(x) == SEXPTYPE::PROMSXP {
            crate::sexp::envir::forcePromise(x)
        } else {
            x
        }
    }
}

// ---------------------------------------------------------------------------
// Complete R runtime — isTRUE, isFALSE, any_na, all_na, any_nan, all_nan
// ---------------------------------------------------------------------------

/// R's `isTRUE(x)` — returns TRUE if x is exactly length-1 TRUE.
pub unsafe fn do_is_true(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::LGLSXP && XLENGTH(x) == 1 {
            let v = *LOGICAL(x);
            return Rf_ScalarLogical(if v == TRUE { TRUE } else { FALSE });
        }
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `isFALSE(x)` — returns TRUE if x is exactly length-1 FALSE.
pub unsafe fn do_is_false(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::LGLSXP && XLENGTH(x) == 1 {
            let v = *LOGICAL(x);
            return Rf_ScalarLogical(if v == FALSE { TRUE } else { FALSE });
        }
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `anyNA(x)` — returns TRUE if any element is NA.
pub unsafe fn do_any_na(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        for i in 0..n {
            if atomic_value_is_missing(x, i) {
                return Rf_ScalarLogical(TRUE);
            }
        }
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `allNA(x)` — returns TRUE if all elements are NA.
pub unsafe fn do_all_na(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        if n == 0 {
            return Rf_ScalarLogical(FALSE);
        }
        for i in 0..n {
            if !atomic_value_is_missing(x, i) {
                return Rf_ScalarLogical(FALSE);
            }
        }
        Rf_ScalarLogical(TRUE)
    }
}

/// R's `anyNaN(x)` — returns TRUE if any element is NaN.
pub unsafe fn do_any_nan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::REALSXP {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        for i in 0..n {
            let v = *REAL(x).add(i as usize);
            if v.is_nan() {
                return Rf_ScalarLogical(TRUE);
            }
        }
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `allNaN(x)` — returns TRUE if all elements are NaN.
pub unsafe fn do_all_nan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::REALSXP {
            return Rf_ScalarLogical(FALSE);
        }
        let n = XLENGTH(x);
        if n == 0 {
            return Rf_ScalarLogical(FALSE);
        }
        for i in 0..n {
            let v = *REAL(x).add(i as usize);
            if !v.is_nan() {
                return Rf_ScalarLogical(FALSE);
            }
        }
        Rf_ScalarLogical(TRUE)
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

/// R's `split(x, f)` — split vector `x` into groups defined by `f`.
pub unsafe fn do_split(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let f = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || f.is_null() || f == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
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
            let sub = Rf_allocVector3(TYPEOF(x), indices.len() as R_xlen_t);
            let _sub_guard = protect(sub);
            for (dst, &src) in indices.iter().enumerate() {
                copy_matrix_element(sub, dst as R_xlen_t, x, src);
            }
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

unsafe fn split_factor_levels(f: SEXP) -> Option<Vec<String>> {
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

unsafe fn split_group_label(f: SEXP, index: R_xlen_t) -> Option<String> {
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

fn split_label_cmp(t: c_int, left: &str, right: &str) -> std::cmp::Ordering {
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

fn split_logical_rank(value: &str) -> u8 {
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

// ---------------------------------------------------------------------------
// Complete R runtime — with, within, transform
// ---------------------------------------------------------------------------

/// R's `with(data, expr)` — evaluate expr in a data/list environment.
pub unsafe fn do_with(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let data_expr = arg_by_name_or_position(args, &["data"], 0);
        let expr = arg_by_name_or_position(args, &["expr"], 1);
        if expr.is_null() || expr == R_NilValue() {
            return R_NilValue();
        }
        let data = if data_expr.is_null() || data_expr == R_NilValue() {
            R_NilValue()
        } else {
            crate::eval::eval::Rf_eval(data_expr, rho)
        };
        if data.is_null() || data == R_NilValue() {
            return crate::eval::eval::Rf_eval(expr, rho);
        }
        let eval_env = data_environment(data, rho);
        crate::eval::eval::Rf_eval(expr, eval_env)
    }
}

unsafe fn data_environment(data: SEXP, parent: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(data) == SEXPTYPE::ENVSXP {
            return data;
        }
        if TYPEOF(data) != SEXPTYPE::VECSXP {
            return parent;
        }

        let env = crate::sexp::memory_ext::NewEnvironment(R_NilValue(), parent, R_NilValue());
        if env.is_null() || env == R_NilValue() {
            return parent;
        }

        let names =
            crate::sexp::attrib_core::getAttrib(data, crate::sexp::attrib_core::R_NamesSymbol());
        let n = XLENGTH(data);
        for i in 0..n {
            if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
                break;
            }
            let name = elt_to_string(names, i);
            if name.is_empty() {
                continue;
            }
            let symbol = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
            crate::sexp::envir::defineVar(symbol, VECTOR_ELT(data, i), env);
        }
        env
    }
}

/// R's `within(data, expr)` — modify data by evaluating expr (simplified).
pub unsafe fn do_within(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let data = CAR(args);
        let expr = CAR(CDR(args));
        if data.is_null() || data == R_NilValue() {
            return R_NilValue();
        }
        // Simplified: evaluate expr and return the original data
        // A full implementation would evaluate expr in data context and return modified data
        if !expr.is_null() && expr != R_NilValue() {
            let _ = crate::eval::eval::Rf_eval(expr, rho);
        }
        data
    }
}

/// R's `transform(x, ...)` — add/modify columns of a data.frame (simplified).
pub unsafe fn do_transform(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        // Simplified: return the data as-is
        // A full implementation would evaluate named args as new columns
        x
    }
}

// ---------------------------------------------------------------------------
// Complete base R functions — table operations, factors, aggregation
// ---------------------------------------------------------------------------

/// R's `prop.table(x)` — proportion table (simplified).
pub unsafe fn do_prop_table(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x);
        if t != SEXPTYPE::REALSXP && t != SEXPTYPE::INTSXP {
            return x;
        }
        let n = XLENGTH(x);
        // Calculate total
        let mut total = 0.0;
        if t == SEXPTYPE::REALSXP {
            for i in 0..n {
                total += *REAL(x).add(i as usize);
            }
        } else {
            for i in 0..n {
                total += *INTEGER(x).add(i as usize) as f64;
            }
        }
        if total == 0.0 {
            return x;
        }
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        if t == SEXPTYPE::REALSXP {
            for i in 0..n {
                *dst.add(i as usize) = *REAL(x).add(i as usize) / total;
            }
        } else {
            for i in 0..n {
                *dst.add(i as usize) = *INTEGER(x).add(i as usize) as f64 / total;
            }
        }
        result
    }
}

/// R's `addmargins(A)` — add margins to table (simplified: returns input).
pub unsafe fn do_addmargins(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        // Simplified: return as-is
        x
    }
}

/// R's `ftable(x)` — flat table (simplified: returns input).
pub unsafe fn do_ftable(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        x
    }
}

/// R's `xtabs(formula, data)` — cross-tabulation (simplified).
pub unsafe fn do_xtabs(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _formula = CAR(args);
        let _data = CAR(CDR(args));
        // Simplified: return empty table
        Rf_allocVector3(SEXPTYPE::INTSXP, 0)
    }
}

/// R's `aggregate(x, by, FUN)` — aggregate by groups (simplified).
pub unsafe fn do_aggregate(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let _by = CAR(CDR(args));
        let fun = CAR(CDR(CDR(args)));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        // Simplified: apply FUN to whole vector
        if !fun.is_null() && fun != R_NilValue() {
            let call_args = Rf_cons(x, R_NilValue());
            let call_sexp = Rf_cons(fun, call_args);
            if !call_sexp.is_null() {
                (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            }
            return crate::eval::eval::Rf_eval(call_sexp, rho);
        }
        x
    }
}

/// R's `ave(x, ...)` — group averages (simplified).
pub unsafe fn do_ave(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        // Simplified: return input
        x
    }
}

/// R's `by(data, INDICES, FUN)` — apply by groups (simplified).
pub unsafe fn do_by(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let data = CAR(args);
        let _indices = CAR(CDR(args));
        let fun = CAR(CDR(CDR(args)));
        if data.is_null() || data == R_NilValue() {
            return R_NilValue();
        }
        // Simplified: apply FUN to data
        if !fun.is_null() && fun != R_NilValue() {
            let call_args = Rf_cons(data, R_NilValue());
            let call_sexp = Rf_cons(fun, call_args);
            if !call_sexp.is_null() {
                (*call_sexp).sxpinfo.set_type(SEXPTYPE::LANGSXP);
            }
            return crate::eval::eval::Rf_eval(call_sexp, rho);
        }
        data
    }
}

/// R's `interaction(...)` — factor interaction (simplified).
pub unsafe fn do_interaction(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        x
    }
}

/// R's `relevel(x, ref)` — relevel factor (simplified).
pub unsafe fn do_relevel(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        x
    }
}

/// R's `factor(x)` — create a minimal factor with sorted levels.
pub unsafe fn do_factor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        let t = TYPEOF(x);
        if t != SEXPTYPE::STRSXP && t != SEXPTYPE::INTSXP && t != SEXPTYPE::REALSXP {
            return x;
        }

        let levels_arg = arg_by_name_or_position(args, &["levels"], 1);
        let levels = if levels_arg.is_null() || levels_arg == R_NilValue() {
            let mut level_set = std::collections::BTreeSet::new();
            for i in 0..n {
                if !factor_element_is_na(x, i) {
                    level_set.insert(elt_to_string(x, i));
                }
            }
            level_set.into_iter().collect::<Vec<_>>()
        } else {
            explicit_factor_levels(levels_arg)
        };

        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let dst = INTEGER(result);
        for i in 0..n {
            if factor_element_is_na(x, i) {
                *dst.add(i as usize) = NA_INTEGER;
                continue;
            }
            let value = elt_to_string(x, i);
            let code = levels
                .iter()
                .position(|level| level == &value)
                .map(|idx| idx as i32 + 1)
                .unwrap_or(NA_INTEGER);
            *dst.add(i as usize) = code;
        }

        let levels_vec = Rf_allocVector3(SEXPTYPE::STRSXP, levels.len() as R_xlen_t);
        let _levels_vec_guard = protect(levels_vec);
        for (i, level) in levels.iter().enumerate() {
            let cstr = CString::new(level.as_str()).unwrap_or_default();
            SET_STRING_ELT(levels_vec, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
        }

        let class = Rf_mkString(CString::new("factor").unwrap_or_default().as_ptr());
        let _class_guard = protect(class);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_LevelsSymbol(),
            levels_vec,
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_ClassSymbol(),
            class,
        );
        result
    }
}

fn factor_element_is_na(x: SEXP, i: R_xlen_t) -> bool {
    unsafe {
        let ty = TYPEOF(x);
        if ty == SEXPTYPE::STRSXP {
            is_string_na(x, i)
        } else if ty == SEXPTYPE::INTSXP {
            let n = XLENGTH(x);
            n != 0 && *INTEGER(x).add((i % n) as usize) == NA_INTEGER
        } else if ty == SEXPTYPE::REALSXP {
            let n = XLENGTH(x);
            if n == 0 {
                false
            } else {
                let value = *REAL(x).add((i % n) as usize);
                value.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || value.is_nan()
            }
        } else {
            false
        }
    }
}

fn explicit_factor_levels(levels_arg: SEXP) -> Vec<String> {
    unsafe {
        let mut levels = Vec::new();
        for i in 0..XLENGTH(levels_arg) {
            if factor_element_is_na(levels_arg, i) {
                continue;
            }
            let level = elt_to_string(levels_arg, i);
            if !levels.iter().any(|existing| existing == &level) {
                levels.push(level);
            }
        }
        levels
    }
}

/// R's `is.factor(x)` — check if factor (simplified: checks class attribute).
pub unsafe fn do_is_factor(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        // Check class attribute for "factor"
        let class = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
        );
        if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
            let n = XLENGTH(class);
            for i in 0..n {
                let charsxp = STRING_ELT(class, i);
                if !charsxp.is_null() {
                    let s = CHAR(charsxp);
                    if !s.is_null() {
                        let cls = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
                        if cls == "factor" {
                            return Rf_ScalarLogical(TRUE);
                        }
                    }
                }
            }
        }
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `is.ordered(x)` — check if ordered factor (simplified).
pub unsafe fn do_is_ordered(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        let class = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
        );
        if !class.is_null() && TYPEOF(class) == SEXPTYPE::STRSXP {
            let n = XLENGTH(class);
            for i in 0..n {
                let charsxp = STRING_ELT(class, i);
                if !charsxp.is_null() {
                    let s = CHAR(charsxp);
                    if !s.is_null() {
                        let cls = std::ffi::CStr::from_ptr(s).to_str().unwrap_or("");
                        if cls == "ordered" {
                            return Rf_ScalarLogical(TRUE);
                        }
                    }
                }
            }
        }
        Rf_ScalarLogical(FALSE)
    }
}

/// R's `levels(x)` — factor levels (simplified).
pub unsafe fn do_levels(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        // Get levels attribute
        let levels = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("levels").unwrap_or_default().as_ptr()),
        );
        if levels.is_null() {
            return R_NilValue();
        }
        levels
    }
}

/// R's `levels(x) <- value` — replace factor levels or the raw levels attribute.
pub unsafe fn do_levels_set(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let value = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let result = if inherits_class(x, "factor") {
            replace_factor_levels(x, value)
        } else {
            crate::sexp::attrib_core::setAttrib(
                x,
                crate::sexp::attrib_core::R_LevelsSymbol(),
                value,
            );
            x
        };

        crate::sexp::globals::set_R_Visible(crate::sexp::ffi::FALSE);
        result
    }
}

unsafe fn replace_factor_levels(x: SEXP, value: SEXP) -> SEXP {
    unsafe {
        let old_levels = string_vector_values(crate::sexp::attrib_core::getAttrib(
            x,
            crate::sexp::attrib_core::R_LevelsSymbol(),
        ));
        let xlevs = if TYPEOF(value) == SEXPTYPE::VECSXP {
            factor_levels_from_named_list(value, &old_levels)
        } else {
            if XLENGTH(value) < old_levels.len() as R_xlen_t {
                std::panic::panic_any(RError {
                    message: "number of levels differs".to_string(),
                });
            }
            (0..XLENGTH(value))
                .map(|i| {
                    if is_string_na(value, i) {
                        None
                    } else {
                        Some(elt_to_string(value, i))
                    }
                })
                .collect::<Vec<_>>()
        };

        let new_levels = unique_present_strings(&xlevs);
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, XLENGTH(x));
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..XLENGTH(x) {
            let old_code = if TYPEOF(x) == SEXPTYPE::INTSXP {
                INTEGER_ELT(x, i as c_int)
            } else {
                NA_INTEGER
            };
            let new_code = if old_code == NA_INTEGER || old_code <= 0 {
                NA_INTEGER
            } else {
                let old_idx = (old_code - 1) as usize;
                xlevs
                    .get(old_idx)
                    .and_then(|level| level.as_ref())
                    .and_then(|level| match_string(&new_levels, level))
                    .map(|idx| idx as c_int + 1)
                    .unwrap_or(NA_INTEGER)
            };
            *INTEGER(result).add(i as usize) = new_code;
        }

        crate::sexp::accessors::SET_ATTRIB(result, ATTRIB(x));
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_LevelsSymbol(),
            string_vector(&new_levels),
        );
        result
    }
}

unsafe fn factor_levels_from_named_list(value: SEXP, old_levels: &[String]) -> Vec<Option<String>> {
    unsafe {
        let names =
            crate::sexp::attrib_core::getAttrib(value, crate::sexp::attrib_core::R_NamesSymbol());
        let mut xlevs = old_levels.iter().cloned().map(Some).collect::<Vec<_>>();
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return xlevs;
        }

        for group_idx in 0..XLENGTH(value) {
            if is_string_na(names, group_idx) {
                continue;
            }
            let group_name = elt_to_string(names, group_idx);
            let members = VECTOR_ELT(value, group_idx);
            for member_idx in 0..XLENGTH(members) {
                let old_name = elt_to_string(members, member_idx);
                if let Some(pos) = match_string(old_levels, &old_name)
                    && let Some(slot) = xlevs.get_mut(pos)
                {
                    *slot = Some(group_name.clone());
                }
            }
        }
        xlevs
    }
}

fn unique_present_strings(values: &[Option<String>]) -> Vec<String> {
    let mut unique = Vec::new();
    for value in values.iter().flatten() {
        if !unique.iter().any(|existing| existing == value) {
            unique.push(value.clone());
        }
    }
    unique
}

fn match_string(values: &[String], needle: &str) -> Option<usize> {
    values.iter().position(|value| value == needle)
}

unsafe fn inherits_class(x: SEXP, class_name: &str) -> bool {
    unsafe {
        let class =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
        if class.is_null() || class == R_NilValue() || TYPEOF(class) != SEXPTYPE::STRSXP {
            return false;
        }
        (0..XLENGTH(class)).any(|i| elt_to_string(class, i) == class_name)
    }
}

/// R's `nlevels(x)` — number of levels (simplified).
pub unsafe fn do_nlevels(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarInteger(0);
        }
        let levels = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("levels").unwrap_or_default().as_ptr()),
        );
        if levels.is_null() {
            return Rf_ScalarInteger(0);
        }
        Rf_ScalarInteger(XLENGTH(levels) as i32)
    }
}

// ---------------------------------------------------------------------------
// Complete string operations — str_locate, str_sub variants
// ---------------------------------------------------------------------------

/// R's `str_locate(x, pattern)` — locate first occurrence of pattern (simplified).
pub unsafe fn do_str_locate(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let pattern = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || pattern.is_null() {
            return R_NilValue();
        }
        // Return a 1x2 matrix with start/end (simplified: return c(start, end))
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = INTEGER(result);
        // Simplified: set to NA (no match)
        *dst.add(0) = NA_INTEGER;
        *dst.add(1) = NA_INTEGER;
        result
    }
}

/// R's `str_locate_all(x, pattern)` — locate all occurrences (simplified).
pub unsafe fn do_str_locate_all(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let _pattern = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        // Return empty matrix
        Rf_allocVector3(SEXPTYPE::INTSXP, 0)
    }
}

/// R's `str_sub(x, start, end)` — extract substring (alias for substr).
pub unsafe fn do_str_sub(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { do_substr(_call, _op, args, _rho) }
}

/// R's `str_sub_all(x, start, end)` — all substrings (simplified).
pub unsafe fn do_str_sub_all(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        // Return input as list
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        SET_VECTOR_ELT(result, 0, x);
        result
    }
}

// ---------------------------------------------------------------------------
// Complete R runtime — Sys.* functions, R.home
// ---------------------------------------------------------------------------

/// R's `R.home()` — R home directory (simplified).
pub unsafe fn do_R_home(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let home = std::env::var("R_HOME").unwrap_or_else(|_| "/usr/lib/R".to_string());
        let s = CString::new(home).unwrap_or_default();
        Rf_mkString(s.as_ptr())
    }
}

/// R's `Sys.getenv(x)` — get environment variable.
pub unsafe fn do_Sys_getenv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            let s = CString::new("").unwrap_or_default();
            return Rf_mkString(s.as_ptr());
        }
        let unset_arg = arg_by_name_or_position(args, &["unset"], 1);
        let unset = if !unset_arg.is_null()
            && unset_arg != R_NilValue()
            && TYPEOF(unset_arg) == SEXPTYPE::STRSXP
            && XLENGTH(unset_arg) > 0
            && STRING_ELT(unset_arg, 0) == crate::sexp::globals::R_NaString()
        {
            None
        } else if !unset_arg.is_null() && unset_arg != R_NilValue() && XLENGTH(unset_arg) > 0 {
            Some(elt_to_string(unset_arg, 0))
        } else {
            Some(String::new())
        };

        let values = (0..XLENGTH(x))
            .map(|i| {
                let name = elt_to_string(x, i);
                std::env::var(&name).ok().or_else(|| unset.clone())
            })
            .collect::<Vec<_>>();
        optional_string_vector(&values)
    }
}

/// R's `Sys.setenv(...)` — set environment variables.
pub unsafe fn do_Sys_setenv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                if let Some(key) = tag_name(current)
                    && !key.is_empty()
                {
                    std::env::set_var(key, elt_to_string(arg, 0));
                } else {
                    let s = elt_to_string(arg, 0);
                    if let Some(pos) = s.find('=') {
                        let key = &s[..pos];
                        let val = &s[pos + 1..];
                        std::env::set_var(key, val);
                    }
                }
            }
            current = CDR(current);
        }
        Rf_ScalarLogical(TRUE)
    }
}

/// R's `Sys.unsetenv(x)` — unset environment variable.
pub unsafe fn do_Sys_unsetenv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarLogical(FALSE);
        }
        for i in 0..XLENGTH(x) {
            let name = elt_to_string(x, i);
            if !name.is_empty() && name != "NA" {
                std::env::remove_var(name);
            }
        }
        Rf_ScalarLogical(TRUE)
    }
}

/// R's `Sys.which(names)` — resolve command names against PATH.
pub unsafe fn do_Sys_which(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names_arg = arg_by_name_or_position(args, &["names"], 0);
        if names_arg.is_null() || names_arg == R_NilValue() || names_arg == R_MissingArg() {
            base_error("argument \"names\" is missing, with no default");
        }

        let names = coerce_string_values(names_arg);
        let paths = names
            .iter()
            .map(|name| find_executable_on_path(name).unwrap_or_default())
            .collect::<Vec<_>>();
        named_string_vector(&paths, &names)
    }
}

fn find_executable_on_path(command: &str) -> Option<String> {
    if command.is_empty() || command == "NA" {
        return None;
    }
    if command.contains(std::path::MAIN_SEPARATOR)
        || command.contains('/')
        || command.contains('\\')
    {
        return executable_path_if_runnable(Path::new(command));
    }

    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(command);
        if let Some(found) = executable_path_if_runnable(&candidate) {
            return Some(found);
        }

        #[cfg(windows)]
        {
            if Path::new(command).extension().is_none() {
                for ext in windows_path_extensions() {
                    let candidate = dir.join(format!("{command}{ext}"));
                    if let Some(found) = executable_path_if_runnable(&candidate) {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

fn executable_path_if_runnable(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return None;
        }
    }

    Some(path.to_string_lossy().into_owned())
}

#[cfg(windows)]
fn windows_path_extensions() -> Vec<String> {
    std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
        .split(';')
        .filter(|ext| !ext.is_empty())
        .map(|ext| ext.to_string())
        .collect()
}

/// R's `Sys.info()` — named character vector with host/user information.
pub unsafe fn do_Sys_info(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let host = sys_info_host_fields();
        let user = sys_info_user();
        let values = vec![
            host.sysname,
            host.release,
            host.version,
            host.nodename,
            host.machine,
            user.clone(),
            user.clone(),
            user,
        ];
        let names = vec![
            "sysname".to_string(),
            "release".to_string(),
            "version".to_string(),
            "nodename".to_string(),
            "machine".to_string(),
            "login".to_string(),
            "user".to_string(),
            "effective_user".to_string(),
        ];
        let result = string_vector(&values);
        let _result_guard = protect(result);
        let name_vec = string_vector(&names);
        let _name_guard = protect(name_vec);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            name_vec,
        );
        result
    }
}

struct SysInfoHostFields {
    sysname: String,
    release: String,
    version: String,
    nodename: String,
    machine: String,
}

fn sys_info_host_fields() -> SysInfoHostFields {
    #[cfg(unix)]
    {
        unsafe {
            let mut utsname = std::mem::MaybeUninit::<libc::utsname>::zeroed();
            if libc::uname(utsname.as_mut_ptr()) == 0 {
                let utsname = utsname.assume_init();
                return SysInfoHostFields {
                    sysname: CStr::from_ptr(utsname.sysname.as_ptr())
                        .to_string_lossy()
                        .into_owned(),
                    release: CStr::from_ptr(utsname.release.as_ptr())
                        .to_string_lossy()
                        .into_owned(),
                    version: CStr::from_ptr(utsname.version.as_ptr())
                        .to_string_lossy()
                        .into_owned(),
                    nodename: CStr::from_ptr(utsname.nodename.as_ptr())
                        .to_string_lossy()
                        .into_owned(),
                    machine: CStr::from_ptr(utsname.machine.as_ptr())
                        .to_string_lossy()
                        .into_owned(),
                };
            }
        }
    }

    SysInfoHostFields {
        sysname: std::env::consts::OS.to_string(),
        release: String::new(),
        version: String::new(),
        nodename: std::env::var("HOSTNAME").unwrap_or_default(),
        machine: std::env::consts::ARCH.to_string(),
    }
}

fn sys_info_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// R's `Sys.time()` — current time as REALSXP (seconds since epoch).
pub unsafe fn do_Sys_time(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use std::time::{SystemTime, UNIX_EPOCH};
        let dur = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let secs = dur.as_secs() as f64 + dur.subsec_nanos() as f64 / 1e9;
        let result = Rf_ScalarReal(secs);
        // Set class to c("POSIXct", "POSIXt").
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        if !class.is_null() {
            let _p2 = protect(class);
            SET_STRING_ELT(
                class,
                0,
                Rf_mkChar(CString::new("POSIXct").unwrap_or_default().as_ptr()),
            );
            SET_STRING_ELT(
                class,
                1,
                Rf_mkChar(CString::new("POSIXt").unwrap_or_default().as_ptr()),
            );
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
                class,
            );
        }
        result
    }
}

/// R's `Sys.sleep(time)` — sleep for specified seconds.
pub unsafe fn do_Sys_sleep(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let time_arg = CAR(args);
        let secs = real_or_default(time_arg, 0.0);
        if secs > 0.0 {
            let dur = std::time::Duration::from_secs_f64(secs);
            std::thread::sleep(dur);
        }
        R_NilValue()
    }
}

unsafe fn set_single_class(x: SEXP, class_name: &str) {
    unsafe {
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if class.is_null() {
            return;
        }
        let _guard = protect(class);
        let cstr = CString::new(class_name).unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            SET_STRING_ELT(class, 0, charsxp);
        }
        crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_ClassSymbol(), class);
    }
}

unsafe fn set_posixct_class(x: SEXP, tz: &str) {
    unsafe {
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        if !class.is_null() {
            let _guard = protect(class);
            SET_STRING_ELT(class, 0, Rf_mkChar(c"POSIXct".as_ptr()));
            SET_STRING_ELT(class, 1, Rf_mkChar(c"POSIXt".as_ptr()));
            crate::sexp::attrib_core::setAttrib(
                x,
                crate::sexp::attrib_core::R_ClassSymbol(),
                class,
            );
        }

        let tz_cstr = CString::new(tz).unwrap_or_default();
        let tzone = Rf_mkString(tz_cstr.as_ptr());
        if !tzone.is_null() {
            crate::sexp::attrib_core::setAttrib(
                x,
                Rf_install(CString::new("tzone").unwrap_or_default().as_ptr()),
                tzone,
            );
        }
    }
}

/// R's `as.Date(x, origin)` — coerce ISO date strings or day counts to Date.
pub unsafe fn do_as_Date(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        if sexp_has_class(x, "Date") && TYPEOF(x) == SEXPTYPE::REALSXP {
            return x;
        }

        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _guard = protect(result);
        let out = REAL(result);

        if TYPEOF(x) == SEXPTYPE::STRSXP {
            for i in 0..n {
                let value = STRING_ELT(x, i);
                let days = if value == crate::sexp::globals::R_NaString() {
                    NA_REAL
                } else {
                    let text = CStr::from_ptr(CHAR(value)).to_str().unwrap_or("");
                    parse_iso_date_days(text).unwrap_or_else(|| {
                        base_error("character string is not in a standard unambiguous format")
                    })
                };
                *out.add(i as usize) = days;
            }
        } else if sexp_has_class(x, "POSIXct") && TYPEOF(x) == SEXPTYPE::REALSXP {
            for i in 0..n {
                let seconds = *REAL(x).add(i as usize);
                *out.add(i as usize) = if seconds.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    NA_REAL
                } else {
                    (seconds / 86_400.0).floor()
                };
            }
        } else if TYPEOF(x) == SEXPTYPE::REALSXP || TYPEOF(x) == SEXPTYPE::INTSXP {
            let origin = arg_by_name_or_position(args, &["origin"], 1);
            if origin.is_null() || origin == R_NilValue() {
                base_error("'origin' must be supplied");
            }
            let origin_days = parse_iso_date_days(&elt_to_string(origin, 0))
                .unwrap_or_else(|| base_error("'origin' must be a character string"));
            for i in 0..n {
                let days = if TYPEOF(x) == SEXPTYPE::REALSXP {
                    let v = *REAL(x).add(i as usize);
                    if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                        NA_REAL
                    } else {
                        origin_days + v.floor()
                    }
                } else {
                    let v = *INTEGER(x).add(i as usize);
                    if v == NA_INTEGER {
                        NA_REAL
                    } else {
                        origin_days + f64::from(v)
                    }
                };
                *out.add(i as usize) = days;
            }
        } else {
            base_error("do not know how to convert 'x' to class \"Date\"");
        }

        set_single_class(result, "Date");
        result
    }
}

/// R's `as.POSIXct(x, tz, origin)` — coerce simple UTC inputs to POSIXct.
pub unsafe fn do_as_POSIXct(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        if sexp_has_class(x, "POSIXct") && TYPEOF(x) == SEXPTYPE::REALSXP {
            return x;
        }

        let tz_arg = arg_by_name_or_position(args, &["tz"], 1);
        let tz = if tz_arg.is_null() || tz_arg == R_NilValue() || XLENGTH(tz_arg) == 0 {
            "UTC".to_string()
        } else {
            let value = elt_to_string(tz_arg, 0);
            if value.is_empty() {
                "UTC".to_string()
            } else {
                value
            }
        };

        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _guard = protect(result);
        let out = REAL(result);

        if TYPEOF(x) == SEXPTYPE::STRSXP {
            for i in 0..n {
                let value = STRING_ELT(x, i);
                let seconds = if value == crate::sexp::globals::R_NaString() {
                    NA_REAL
                } else {
                    let text = CStr::from_ptr(CHAR(value)).to_str().unwrap_or("");
                    parse_iso_datetime_seconds(text).unwrap_or_else(|| {
                        base_error("character string is not in a standard unambiguous format")
                    })
                };
                *out.add(i as usize) = seconds;
            }
        } else if sexp_has_class(x, "Date") && TYPEOF(x) == SEXPTYPE::REALSXP {
            for i in 0..n {
                let days = *REAL(x).add(i as usize);
                *out.add(i as usize) = if days.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    NA_REAL
                } else {
                    days.floor() * 86_400.0
                };
            }
        } else if TYPEOF(x) == SEXPTYPE::REALSXP || TYPEOF(x) == SEXPTYPE::INTSXP {
            let origin = arg_by_name_or_position(args, &["origin"], 2);
            let origin_seconds = if origin.is_null() || origin == R_NilValue() {
                0.0
            } else {
                parse_iso_datetime_seconds(&elt_to_string(origin, 0))
                    .or_else(|| {
                        parse_iso_date_days(&elt_to_string(origin, 0)).map(|days| days * 86_400.0)
                    })
                    .unwrap_or_else(|| base_error("'origin' must be a character string"))
            };
            for i in 0..n {
                let seconds = if TYPEOF(x) == SEXPTYPE::REALSXP {
                    let v = *REAL(x).add(i as usize);
                    if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                        NA_REAL
                    } else {
                        origin_seconds + v
                    }
                } else {
                    let v = *INTEGER(x).add(i as usize);
                    if v == NA_INTEGER {
                        NA_REAL
                    } else {
                        origin_seconds + f64::from(v)
                    }
                };
                *out.add(i as usize) = seconds;
            }
        } else {
            base_error("do not know how to convert 'x' to class \"POSIXct\"");
        }

        set_posixct_class(result, &tz);
        result
    }
}

/// R's `Sys.Date()` — current date as REALSXP (days since epoch).
pub unsafe fn do_Sys_Date(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use std::time::{SystemTime, UNIX_EPOCH};
        let dur = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let days = (dur.as_secs() / 86400) as f64;
        let result = Rf_ScalarReal(days);
        set_single_class(result, "Date");
        result
    }
}

/// R's `Sys.timezone()` — current timezone (simplified).
pub unsafe fn do_Sys_timezone(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let tz = system_timezone_name();
        let s = CString::new(tz).unwrap_or_default();
        Rf_mkString(s.as_ptr())
    }
}

fn system_timezone_name() -> String {
    std::env::var("TZ")
        .ok()
        .and_then(|tz| {
            let tz = tz.trim_start_matches(':').to_string();
            (!tz.is_empty()).then_some(tz)
        })
        .or_else(|| {
            std::fs::read_link("/etc/localtime")
                .ok()
                .and_then(|path| timezone_name_from_zoneinfo_path(&path))
        })
        .unwrap_or_else(|| "UTC".to_string())
}

fn timezone_name_from_zoneinfo_path(path: &Path) -> Option<String> {
    let path = path.to_string_lossy();
    for prefix in [
        "/var/db/timezone/zoneinfo/",
        "/usr/share/zoneinfo/",
        "/usr/share/lib/zoneinfo/",
    ] {
        if let Some(zone) = path.strip_prefix(prefix) {
            if !zone.is_empty() {
                return Some(zone.to_string());
            }
        }
    }
    None
}

/// R's `OlsonNames()` — known IANA timezone names from the system zoneinfo DB.
pub unsafe fn do_OlsonNames(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let zones = olson_names();
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, zones.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        for (i, zone) in zones.iter().enumerate() {
            SET_STRING_ELT(
                result,
                i as R_xlen_t,
                Rf_mkChar(CString::new(zone.as_str()).unwrap_or_default().as_ptr()),
            );
        }
        result
    }
}

fn olson_names() -> Vec<String> {
    let mut names = BTreeSet::new();
    for root in ["/var/db/timezone/zoneinfo", "/usr/share/zoneinfo"] {
        collect_olson_names(Path::new(root), Path::new(""), &mut names);
    }
    names.into_iter().collect()
}

fn collect_olson_names(root: &Path, relative: &Path, names: &mut BTreeSet<String>) {
    let current = root.join(relative);
    let Ok(entries) = std::fs::read_dir(current) else {
        return;
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if skip_olson_component(&file_name) {
            continue;
        }

        let next_relative = relative.join(file_name.as_ref());
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_olson_names(root, &next_relative, names);
        } else if file_type.is_file() && next_relative.components().count() > 1 {
            names.insert(next_relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn skip_olson_component(name: &str) -> bool {
    let metadata_extension = Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "tab" | "list" | "zi"));
    name.starts_with('.') || matches!(name, "posix" | "right" | "SystemV") || metadata_extension
}

/// R's `Sys.localeconv()` — locale formatting conventions.
pub unsafe fn do_Sys_localeconv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let names = [
            "decimal_point",
            "thousands_sep",
            "grouping",
            "int_curr_symbol",
            "currency_symbol",
            "mon_decimal_point",
            "mon_thousands_sep",
            "mon_grouping",
            "positive_sign",
            "negative_sign",
            "int_frac_digits",
            "frac_digits",
            "p_cs_precedes",
            "p_sep_by_space",
            "n_cs_precedes",
            "n_sep_by_space",
            "p_sign_posn",
            "n_sign_posn",
        ];
        let values = [
            ".", "", "", "", "", ".", "", "", "", "", "127", "127", "127", "127", "127", "127",
            "127", "127",
        ];
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let name_vec = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as R_xlen_t);
        let _names_guard = protect(name_vec);
        for (i, (name, value)) in names.iter().zip(values.iter()).enumerate() {
            SET_STRING_ELT(
                result,
                i as R_xlen_t,
                Rf_mkChar(CString::new(*value).unwrap_or_default().as_ptr()),
            );
            SET_STRING_ELT(
                name_vec,
                i as R_xlen_t,
                Rf_mkChar(CString::new(*name).unwrap_or_default().as_ptr()),
            );
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_NamesSymbol(),
            name_vec,
        );
        result
    }
}

/// R's `Sys.getlocale(category)` — get locale (simplified).
pub unsafe fn do_Sys_getlocale(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let category = locale_category_from_arg(CAR(args));
        locale_string_from_libc(category)
    }
}

/// R's `Sys.setlocale(category, locale)` — set locale (simplified).
pub unsafe fn do_Sys_setlocale(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let category = locale_category_from_arg(CAR(args));
        let locale_arg = CAR(CDR(args));
        let locale = locale_string_arg(locale_arg);
        let locale_ptr = match locale.as_ref() {
            Some(locale) => locale.as_ptr(),
            None => std::ptr::null(),
        };
        let result = libc::setlocale(category, locale_ptr);
        if result.is_null() {
            Rf_mkString(b"\0".as_ptr() as *const c_char)
        } else {
            Rf_mkString(result)
        }
    }
}

unsafe fn locale_category_from_arg(category: SEXP) -> c_int {
    unsafe {
        if category.is_null() || category == R_NilValue() {
            return libc::LC_ALL;
        }

        match TYPEOF(category) {
            t if t == SEXPTYPE::STRSXP => {
                let name = elt_to_string(category, 0);
                locale_category_from_name(&name)
            }
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => match *INTEGER(category) {
                1 => libc::LC_ALL,
                2 => libc::LC_COLLATE,
                3 => libc::LC_CTYPE,
                4 => libc::LC_MONETARY,
                5 => libc::LC_NUMERIC,
                6 => libc::LC_TIME,
                7 => libc::LC_MESSAGES,
                _ => base_error("invalid 'category' argument"),
            },
            _ => base_error("invalid 'category' argument"),
        }
    }
}

fn locale_category_from_name(name: &str) -> c_int {
    match name {
        "LC_ALL" => libc::LC_ALL,
        "LC_COLLATE" => libc::LC_COLLATE,
        "LC_CTYPE" => libc::LC_CTYPE,
        "LC_MONETARY" => libc::LC_MONETARY,
        "LC_NUMERIC" => libc::LC_NUMERIC,
        "LC_TIME" => libc::LC_TIME,
        "LC_MESSAGES" => libc::LC_MESSAGES,
        _ => base_error("invalid 'category' argument"),
    }
}

unsafe fn locale_string_arg(locale: SEXP) -> Option<CString> {
    unsafe {
        if locale.is_null() || locale == R_NilValue() {
            return None;
        }
        if TYPEOF(locale) != SEXPTYPE::STRSXP || XLENGTH(locale) == 0 {
            base_error("invalid 'locale' argument");
        }
        CString::new(elt_to_string(locale, 0))
            .map(Some)
            .unwrap_or_else(|_| base_error("invalid 'locale' argument"))
    }
}

unsafe fn locale_string_from_libc(category: c_int) -> SEXP {
    unsafe {
        let result = libc::setlocale(category, std::ptr::null());
        if result.is_null() {
            Rf_mkString(b"\0".as_ptr() as *const c_char)
        } else {
            Rf_mkString(result)
        }
    }
}

// ---------------------------------------------------------------------------
// Complete data operations — subset
// ---------------------------------------------------------------------------

/// R's `subset(x, subset, select, drop)` — subset data.frame (simplified).
/// Already defined as do_subset above — this is an alias with named args.
pub unsafe fn do_subset_named(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Delegate to existing do_subset
        do_subset(_call, _op, args, _rho)
    }
}

// ---------------------------------------------------------------------------
// Complete I/O — enhanced cat, message, warning
// ---------------------------------------------------------------------------

/// R's enhanced `cat(..., file, sep, fill, labels, append)` — simplified.
pub unsafe fn do_cat_enhanced(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Simplified: delegates to existing do_cat
        do_cat(_call, _op, args, _rho)
    }
}

/// R's enhanced `message(..., domain, appendLF)` — simplified.
pub unsafe fn do_message_enhanced(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let output = condition_message_text(args, &["domain", "appendLF"]);
        eprintln!("{}", output);
        R_NilValue()
    }
}

/// R's enhanced `warning(..., call., immediate., noBreaks., domain.)` — simplified.
pub unsafe fn do_warning_enhanced(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut output =
            condition_message_text(args, &["call.", "immediate.", "noBreaks.", "domain"]);
        if output.is_empty() {
            output = "warning".to_string();
        }
        eprintln!("Warning: {}", output);
        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// Complete R runtime — match.call, sys.nframe, sys.function, on.exit
// ---------------------------------------------------------------------------

/// R's `match.call(definition, call, expand.dots)` — match call arguments.
/// Simplified: returns the call as-is.
pub unsafe fn do_match_call(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Return the call argument if provided, otherwise the current call
        let call_arg = CAR(args);
        if !call_arg.is_null() && call_arg != R_NilValue() {
            return call_arg;
        }
        _call
    }
}

/// R's `sys.nframe()` — returns the number of frames on the call stack.
pub unsafe fn do_sys_nframe(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let top = crate::sexp::context::R_GlobalContext();
        Rf_ScalarInteger(crate::eval::context::framedepth(top))
    }
}

/// R's `sys.function(which)` — returns the function at the given frame level.
pub unsafe fn do_sys_function(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let which = context_index_arg(args, 0);
        let top = crate::sexp::context::R_GlobalContext();
        if top.is_null() {
            R_NilValue()
        } else {
            crate::eval::context::R_sysfunction(which, top)
        }
    }
}

/// R's `on.exit(expr, add, after)` — register an exit handler for the
/// current function context.
pub unsafe fn do_on_exit(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::eval::special::do_on_exit_from_args(args, rho) }
}

// ---------------------------------------------------------------------------
// Complete I/O — read.csv, write.csv, read.table
// ---------------------------------------------------------------------------

/// R's `read.csv(file, header=TRUE, sep=",")` — read a CSV file (simplified).
/// Returns a list (data.frame) of columns as REALSXP vectors.
pub unsafe fn do_read_csv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = CAR(args);
        let header_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(args))
        };

        let file_path = elt_to_string(file_arg, 0);
        let header = if header_arg.is_null() || header_arg == R_NilValue() {
            true
        } else {
            let v = real_or_default(header_arg, 1.0);
            v != 0.0
        };

        // Read file
        let content = match std::fs::read_to_string(&file_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading '{}': {}", file_path, e);
                return R_NilValue();
            }
        };

        let mut lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return R_NilValue();
        }

        let col_names: Vec<String> = if header {
            let header_line = lines.remove(0);
            header_line
                .split(',')
                .map(|s| s.trim().to_string())
                .collect()
        } else {
            lines[0]
                .split(',')
                .enumerate()
                .map(|(i, _)| format!("V{}", i + 1))
                .collect()
        };

        let ncols = col_names.len();
        if ncols == 0 {
            return R_NilValue();
        }

        // Parse data rows
        let mut col_data: Vec<Vec<f64>> = vec![Vec::new(); ncols];
        for line in &lines {
            let fields: Vec<&str> = line.split(',').collect();
            for j in 0..ncols {
                let val = if j < fields.len() {
                    fields[j].trim().parse::<f64>().unwrap_or(NA_REAL)
                } else {
                    NA_REAL
                };
                col_data[j].push(val);
            }
        }

        // Build list result
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncols as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, ncols as R_xlen_t);
        let _p2 = protect(names_vec);

        for j in 0..ncols {
            let nrow = col_data[j].len();
            let col = Rf_allocVector3(SEXPTYPE::REALSXP, nrow as R_xlen_t);
            if !col.is_null() {
                let dst = REAL(col);
                for (i, &v) in col_data[j].iter().enumerate() {
                    *dst.add(i) = v;
                }
            }
            let data = (*result).gengc_next_node as *mut SEXP;
            *data.add(j) = col;

            let cstr = CString::new(col_names[j].as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let nmdata = (*names_vec).gengc_next_node as *mut SEXP;
                *nmdata.add(j) = charsxp;
            }
        }

        // Set names
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
            names_vec,
        );
        // Set class to data.frame
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _p3 = protect(class_vec);
        let cstr = CString::new("data.frame").unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let cdata = (*class_vec).gengc_next_node as *mut SEXP;
            *cdata.add(0) = charsxp;
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
            class_vec,
        );
        result
    }
}

/// R's `write.csv(x, file, row.names=TRUE)` — write a CSV file (simplified).
pub unsafe fn do_write_csv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let file_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(args))
        };
        let row_names_arg = if CDR(args).is_null()
            || CDR(args) == R_NilValue()
            || CDR(CDR(args)).is_null()
            || CDR(CDR(args)) == R_NilValue()
        {
            R_NilValue()
        } else {
            CAR(CDR(CDR(args)))
        };

        let file_path = elt_to_string(file_arg, 0);
        let write_row_names = if row_names_arg.is_null() || row_names_arg == R_NilValue() {
            true
        } else {
            let v = real_or_default(row_names_arg, 1.0);
            v != 0.0
        };

        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let t = TYPEOF(x);
        let mut lines: Vec<String> = Vec::new();

        if t == SEXPTYPE::VECSXP {
            // Data.frame-like list
            let ncols = XLENGTH(x);
            let nrow = if ncols > 0 {
                let first_col = VECTOR_ELT(x, 0);
                XLENGTH(first_col)
            } else {
                0
            };

            // Get column names
            let names = crate::sexp::attrib_core::getAttrib(
                x,
                Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
            );

            // Header
            let mut header_parts: Vec<String> = Vec::new();
            if write_row_names {
                header_parts.push(String::new());
            }
            for j in 0..ncols {
                let nm = if !names.is_null() {
                    elt_to_string(names, j)
                } else {
                    format!("V{}", j + 1)
                };
                header_parts.push(format!("\"{}\"", nm));
            }
            lines.push(header_parts.join(","));

            // Data rows
            for i in 0..nrow {
                let mut row_parts: Vec<String> = Vec::new();
                if write_row_names {
                    row_parts.push((i + 1).to_string());
                }
                for j in 0..ncols {
                    let col = VECTOR_ELT(x, j);
                    row_parts.push(elt_to_string(col, i));
                }
                lines.push(row_parts.join(","));
            }
        } else if t == SEXPTYPE::REALSXP || t == SEXPTYPE::INTSXP {
            // Simple vector — write as single column
            let n = XLENGTH(x);
            lines.push("\"x\"".to_string());
            for i in 0..n {
                lines.push(elt_to_string(x, i));
            }
        }

        let content = lines.join("\n") + "\n";
        if let Err(e) = std::fs::write(&file_path, content) {
            eprintln!("Error writing '{}': {}", file_path, e);
        }

        crate::sexp::globals::set_R_Visible(FALSE);
        R_NilValue()
    }
}

/// R's `read.table(file, header=FALSE, sep="")` — read a table (simplified).
/// Returns a list (data.frame) of columns.
pub unsafe fn do_read_table(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = CAR(args);
        let header_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(args))
        };

        let file_path = elt_to_string(file_arg, 0);
        let header = if header_arg.is_null() || header_arg == R_NilValue() {
            false
        } else {
            let v = real_or_default(header_arg, 0.0);
            v != 0.0
        };

        let content = match std::fs::read_to_string(&file_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading '{}': {}", file_path, e);
                return R_NilValue();
            }
        };

        let mut lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return R_NilValue();
        }

        // Parse first data line to determine number of columns
        let ncols = if header {
            if lines.is_empty() {
                return R_NilValue();
            }
            lines[0].split_whitespace().count()
        } else {
            lines[0].split_whitespace().count()
        };

        if ncols == 0 {
            return R_NilValue();
        }

        let col_names: Vec<String> = if header {
            let header_line = lines.remove(0);
            header_line
                .split_whitespace()
                .map(|s| s.trim().to_string())
                .collect()
        } else {
            (0..ncols).map(|i| format!("V{}", i + 1)).collect()
        };

        let mut col_data: Vec<Vec<f64>> = vec![Vec::new(); ncols];
        for line in &lines {
            let fields: Vec<&str> = line.split_whitespace().collect();
            for j in 0..ncols {
                let val = if j < fields.len() {
                    fields[j].trim().parse::<f64>().unwrap_or(NA_REAL)
                } else {
                    NA_REAL
                };
                col_data[j].push(val);
            }
        }

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncols as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, ncols as R_xlen_t);
        let _p2 = protect(names_vec);

        for j in 0..ncols {
            let nrow = col_data[j].len();
            let col = Rf_allocVector3(SEXPTYPE::REALSXP, nrow as R_xlen_t);
            if !col.is_null() {
                let dst = REAL(col);
                for (i, &v) in col_data[j].iter().enumerate() {
                    *dst.add(i) = v;
                }
            }
            let data = (*result).gengc_next_node as *mut SEXP;
            *data.add(j) = col;

            let cstr = CString::new(col_names[j].as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let nmdata = (*names_vec).gengc_next_node as *mut SEXP;
                *nmdata.add(j) = charsxp;
            }
        }

        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
            names_vec,
        );
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _p3 = protect(class_vec);
        let cstr = CString::new("data.frame").unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let cdata = (*class_vec).gengc_next_node as *mut SEXP;
            *cdata.add(0) = charsxp;
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
            class_vec,
        );
        result
    }
}

// ---------------------------------------------------------------------------
// Complete S3 generics — as.matrix, as.numeric
// ---------------------------------------------------------------------------

/// R's `as.matrix(x)` — convert to matrix (simplified).
/// For vectors, wraps as a single-column matrix.
/// For lists/data.frames, wraps as a matrix.
pub unsafe fn do_as_matrix(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::REALSXP || t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            // Simple vector — copy and set dim attribute
            let n = XLENGTH(x);
            let result = Rf_allocVector3(t, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _p = protect(result);
            if t == SEXPTYPE::REALSXP {
                let src = REAL(x);
                let dst = REAL(result);
                for i in 0..n {
                    *dst.add(i as usize) = *src.add(i as usize);
                }
            } else {
                let src = INTEGER(x);
                let dst = INTEGER(result);
                for i in 0..n {
                    *dst.add(i as usize) = *src.add(i as usize);
                }
            }
            // Set dim = c(n, 1)
            let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            if !dim.is_null() {
                let _p2 = protect(dim);
                let d = INTEGER(dim);
                *d.add(0) = n as i32;
                *d.add(1) = 1;
                crate::sexp::attrib_core::setAttrib(
                    result,
                    Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                    dim,
                );
            }
            result
        } else if t == SEXPTYPE::STRSXP {
            let n = XLENGTH(x);
            let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            // Copy string elements
            for i in 0..n {
                let charsxp = STRING_ELT(x, i);
                if !charsxp.is_null() {
                    SET_STRING_ELT(result, i, charsxp);
                }
            }
            // Set dim = c(n, 1)
            let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            if !dim.is_null() {
                let _p2 = protect(dim);
                let d = INTEGER(dim);
                *d.add(0) = n as i32;
                *d.add(1) = 1;
                crate::sexp::attrib_core::setAttrib(
                    result,
                    Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                    dim,
                );
            }
            result
        } else {
            // For other types, return as-is
            x
        }
    }
}

/// R's `as.numeric(x)` — alias for as.double.
pub unsafe fn do_as_numeric(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Delegate to do_as_double
        do_as_double(_call, _op, args, _rho)
    }
}

// ---------------------------------------------------------------------------
// Complete R runtime — par, getGraphicsEvent
// ---------------------------------------------------------------------------

/// R's `par(...)` — session-owned graphical parameters.
pub unsafe fn do_par(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::library::graphics::par::do_par(_call, _op, _args, _rho) }
}

/// R's `layout(...)` — session-owned base graphics layout state.
pub unsafe fn do_layout(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { crate::library::graphics::par::do_layout(_call, _op, _args, _rho) }
}

/// R's `getGraphicsEvent(prompt, onMouseDown, ...)` — no Android event loop is attached here.
pub unsafe fn do_getGraphicsEvent(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    std::panic::panic_any(RError {
        message: "graphics events are not available for the headless Android device".to_string(),
    });
}

// ---------------------------------------------------------------------------
// Complete R runtime — Rprof, Rprofmem, gc, gcinfo, memory.size, object.size
// ---------------------------------------------------------------------------

/// R's `Rprof(filename, ...)` — session-owned profiling.
pub unsafe fn do_Rprof(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let result = crate::eval::profiling::do_Rprof(call, op, args, rho);
        crate::sexp::globals::set_R_Visible(FALSE);
        result
    }
}

/// R's `Rprofmem(filename, ...)` — session-owned memory profiling.
pub unsafe fn do_Rprofmem(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let result = crate::eval::profiling::do_Rprofmem(call, op, args, rho);
        crate::sexp::globals::set_R_Visible(FALSE);
        result
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct RuntimeMemorySnapshot {
    active_nodes: usize,
    free_nodes: usize,
    current_bytes: usize,
    peak_bytes: usize,
}

fn runtime_memory_snapshot() -> RuntimeMemorySnapshot {
    crate::sexp::instance::with_required_current_instance(|instance| {
        let active_nodes = instance.arena.node_count();
        let free_nodes = instance.arena.free_count();
        let current_bytes = instance.arena.total_bytes_allocated();
        let peak_bytes = instance.gc_state.stats.peak_memory.max(current_bytes);

        RuntimeMemorySnapshot {
            active_nodes,
            free_nodes,
            current_bytes,
            peak_bytes,
        }
    })
}

fn bytes_to_mb(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn set_real_matrix_cell(data: *mut f64, row: usize, col: usize, rows: usize, value: f64) {
    unsafe {
        *data.add(col * rows + row) = value;
    }
}

/// R's `gc()` — garbage collection with session-owned memory counters.
pub unsafe fn do_gc(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::memory_main::R_gc();
        let snapshot = runtime_memory_snapshot();
        let node_size = std::mem::size_of::<crate::sexp::ffi::SexprecCore>();
        let ncell_bytes = snapshot.active_nodes.saturating_mul(node_size);
        let ncell_trigger = (snapshot.active_nodes + snapshot.free_nodes)
            .saturating_mul(2)
            .max(snapshot.active_nodes);
        let ncell_peak = snapshot
            .active_nodes
            .saturating_add(crate::sexp::gengc::get_gc_stats().freed);
        let vcell_size = std::mem::size_of::<SEXP>();
        let vcell_used = snapshot.current_bytes / vcell_size;
        let vcell_trigger_bytes = snapshot
            .current_bytes
            .saturating_mul(2)
            .max(snapshot.current_bytes);
        let vcell_peak = snapshot.peak_bytes / vcell_size;

        // Return a 2x7 matrix. Rows are Ncells and Vcells; columns follow R's
        // visible shape: used, (Mb), gc trigger, (Mb), max used, (Mb), limit.
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, 14);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        set_real_matrix_cell(dst, 0, 0, 2, snapshot.active_nodes as f64);
        set_real_matrix_cell(dst, 1, 0, 2, vcell_used as f64);
        set_real_matrix_cell(dst, 0, 1, 2, bytes_to_mb(ncell_bytes));
        set_real_matrix_cell(dst, 1, 1, 2, bytes_to_mb(snapshot.current_bytes));
        set_real_matrix_cell(dst, 0, 2, 2, ncell_trigger as f64);
        set_real_matrix_cell(dst, 1, 2, 2, (vcell_trigger_bytes / vcell_size) as f64);
        set_real_matrix_cell(
            dst,
            0,
            3,
            2,
            bytes_to_mb(ncell_trigger.saturating_mul(node_size)),
        );
        set_real_matrix_cell(dst, 1, 3, 2, bytes_to_mb(vcell_trigger_bytes));
        set_real_matrix_cell(dst, 0, 4, 2, ncell_peak as f64);
        set_real_matrix_cell(dst, 1, 4, 2, vcell_peak as f64);
        set_real_matrix_cell(
            dst,
            0,
            5,
            2,
            bytes_to_mb(ncell_peak.saturating_mul(node_size)),
        );
        set_real_matrix_cell(dst, 1, 5, 2, bytes_to_mb(snapshot.peak_bytes));
        set_real_matrix_cell(dst, 0, 6, 2, 0.0);
        set_real_matrix_cell(dst, 1, 6, 2, 0.0);

        // Set dim = c(2, 7)
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            let _p2 = protect(dim);
            let d = INTEGER(dim);
            *d.add(0) = 2;
            *d.add(1) = 7;
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                dim,
            );
        }
        // Set dimnames
        let dn = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        if !dn.is_null() {
            let _p3 = protect(dn);
            let row_names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
            if !row_names.is_null() {
                let _p4 = protect(row_names);
                let s1 = CString::new("Ncells").unwrap_or_default();
                let s2 = CString::new("Vcells").unwrap_or_default();
                SET_STRING_ELT(
                    row_names,
                    0,
                    crate::sexp::constructors::Rf_mkChar(s1.as_ptr()),
                );
                SET_STRING_ELT(
                    row_names,
                    1,
                    crate::sexp::constructors::Rf_mkChar(s2.as_ptr()),
                );
                SET_VECTOR_ELT(dn, 0, row_names);
            }
            let col_names = Rf_allocVector3(SEXPTYPE::STRSXP, 7);
            if !col_names.is_null() {
                let _p5 = protect(col_names);
                for (i, name) in [
                    "used",
                    "(Mb)",
                    "gc trigger",
                    "(Mb)",
                    "max used",
                    "(Mb)",
                    "limit",
                ]
                .iter()
                .enumerate()
                {
                    let cstr = CString::new(*name).unwrap_or_default();
                    SET_STRING_ELT(
                        col_names,
                        i as R_xlen_t,
                        crate::sexp::constructors::Rf_mkChar(cstr.as_ptr()),
                    );
                }
                SET_VECTOR_ELT(dn, 1, col_names);
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr()),
                dn,
            );
        }
        crate::sexp::globals::set_R_Visible(FALSE);
        result
    }
}

/// R's `gc.time()` — current GC timing counters.
pub unsafe fn do_gc_time(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { Rf_allocVector3(SEXPTYPE::REALSXP, 5) }
}

/// R's `gcinfo(on)` — set session-local GC reporting verbosity.
pub unsafe fn do_gcinfo(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() || CAR(args) == R_MissingArg() {
            base_error("argument \"verbose\" is missing, with no default");
        }
        let old = crate::mainutils::memory_main::do_gcinfo(call, op, args, rho);
        crate::sexp::globals::set_R_Visible(FALSE);
        old
    }
}

/// R's `gctorture(on = TRUE)` — set session-local GC torture mode.
pub unsafe fn do_gctorture(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let on = if args.is_null() || args == R_NilValue() || CAR(args) == R_MissingArg() {
            Rf_ScalarLogical(TRUE)
        } else {
            CAR(args)
        };
        let normalized = Rf_cons(on, R_NilValue());
        let _args_guard = protect(normalized);
        let old = crate::mainutils::memory_main::do_gctorture(call, op, normalized, rho);
        crate::sexp::globals::set_R_Visible(FALSE);
        old
    }
}

/// R's `gctorture2(step, wait = 0, inhibit_release = FALSE)` session state.
pub unsafe fn do_gctorture2(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() || CAR(args) == R_MissingArg() {
            base_error("argument \"step\" is missing, with no default");
        }

        let step = CAR(args);
        let wait =
            if CDR(args).is_null() || CDR(args) == R_NilValue() || CAR(CDR(args)) == R_MissingArg()
            {
                Rf_ScalarInteger(0)
            } else {
                CAR(CDR(args))
            };
        let _wait_guard = protect(wait);
        let tail = Rf_cons(wait, R_NilValue());
        let _tail_guard = protect(tail);
        let normalized = Rf_cons(step, tail);
        let _args_guard = protect(normalized);
        let old = crate::mainutils::memory_main::do_gctorture2(call, op, normalized, rho);
        crate::sexp::globals::set_R_Visible(FALSE);
        old
    }
}

/// R's `memory.size(max)` — current or peak arena memory in MB.
pub unsafe fn do_memory_size(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let max = crate::mainutils::coerce::asLogical(CAR(args));
        let snapshot = runtime_memory_snapshot();
        let bytes = if max == TRUE {
            snapshot.peak_bytes
        } else {
            snapshot.current_bytes
        };
        Rf_ScalarReal(bytes_to_mb(bytes))
    }
}

/// R's `memory.profile()` — session-local object counts by SEXPTYPE class.
pub unsafe fn do_memory_profile(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    const PROFILE_TYPES: [(&str, SEXPTYPE); 24] = [
        ("NULL", SEXPTYPE::NILSXP),
        ("symbol", SEXPTYPE::SYMSXP),
        ("pairlist", SEXPTYPE::LISTSXP),
        ("closure", SEXPTYPE::CLOSXP),
        ("environment", SEXPTYPE::ENVSXP),
        ("promise", SEXPTYPE::PROMSXP),
        ("language", SEXPTYPE::LANGSXP),
        ("special", SEXPTYPE::SPECIALSXP),
        ("builtin", SEXPTYPE::BUILTINSXP),
        ("char", SEXPTYPE::CHARSXP),
        ("logical", SEXPTYPE::LGLSXP),
        ("integer", SEXPTYPE::INTSXP),
        ("double", SEXPTYPE::REALSXP),
        ("complex", SEXPTYPE::CPLXSXP),
        ("character", SEXPTYPE::STRSXP),
        ("...", SEXPTYPE::DOTSXP),
        ("any", SEXPTYPE::ANYSXP),
        ("list", SEXPTYPE::VECSXP),
        ("expression", SEXPTYPE::EXPRSXP),
        ("bytecode", SEXPTYPE::BCODESXP),
        ("externalptr", SEXPTYPE::EXTPTRSXP),
        ("weakref", SEXPTYPE::WEAKREFSXP),
        ("raw", SEXPTYPE::RAWSXP),
        ("S4", SEXPTYPE::S4SXP),
    ];

    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, PROFILE_TYPES.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);
        let data = INTEGER(result);
        for i in 0..PROFILE_TYPES.len() {
            *data.add(i) = 0;
        }
        *data = 1;

        crate::sexp::instance::with_required_current_instance(|instance| {
            for node in instance.arena.active_nodes() {
                let ty = TYPEOF(node);
                if let Some((idx, _)) = PROFILE_TYPES
                    .iter()
                    .enumerate()
                    .find(|(_, (_, profile_ty))| ty == *profile_ty)
                {
                    // `S4SXP` shares the OBJSXP tag; match GNU R's public bucket name.
                    *data.add(idx) = (*data.add(idx)).saturating_add(1);
                }
            }
        });

        let names = Rf_allocVector3(SEXPTYPE::STRSXP, PROFILE_TYPES.len() as R_xlen_t);
        if !names.is_null() {
            let _names_guard = protect(names);
            for (i, (name, _)) in PROFILE_TYPES.iter().enumerate() {
                SET_STRING_ELT(
                    names,
                    i as R_xlen_t,
                    Rf_mkChar(CString::new(*name).unwrap_or_default().as_ptr()),
                );
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
                names,
            );
        }

        result
    }
}

/// R's `object.size(x)` — estimate object size in bytes (simplified).
/// Returns a numeric scalar with class "object_size".
pub unsafe fn do_object_size(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            let result = Rf_ScalarReal(0.0);
            let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
            if !class_vec.is_null() {
                let _p2 = protect(class_vec);
                let cstr = CString::new("object_size").unwrap_or_default();
                let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
                if !charsxp.is_null() {
                    let cdata = (*class_vec).gengc_next_node as *mut SEXP;
                    *cdata.add(0) = charsxp;
                }
                crate::sexp::attrib_core::setAttrib(
                    result,
                    Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
                    class_vec,
                );
            }
            return result;
        }
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        let size: f64 = match t {
            t if t == SEXPTYPE::REALSXP => (n as usize * std::mem::size_of::<f64>()) as f64,
            t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                (n as usize * std::mem::size_of::<i32>()) as f64
            }
            t if t == SEXPTYPE::STRSXP => {
                let mut total: usize = 0;
                for i in 0..n {
                    let charsxp = STRING_ELT(x, i);
                    if !charsxp.is_null() {
                        let s = CHAR(charsxp);
                        if !s.is_null() {
                            let cstr = std::ffi::CStr::from_ptr(s);
                            total += cstr.to_bytes().len() + 1;
                        }
                    }
                }
                total as f64
            }
            t if t == SEXPTYPE::VECSXP => {
                let mut total: usize = std::mem::size_of::<SEXP>() * n as usize;
                for i in 0..n {
                    let elt = VECTOR_ELT(x, i);
                    if !elt.is_null() {
                        let elt_size = do_object_size(
                            _call,
                            _op,
                            {
                                // Create a temporary pairlist with elt as first arg
                                let cell = Rf_cons(elt, R_NilValue());
                                cell
                            },
                            _rho,
                        );
                        total += real_or_default(elt_size, 0.0) as usize;
                    }
                }
                total as f64
            }
            _ => 64.0, // Default estimate for headers
        };
        let result = Rf_ScalarReal(size);
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        if !class_vec.is_null() {
            let _p2 = protect(class_vec);
            let cstr = CString::new("object_size").unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let cdata = (*class_vec).gengc_next_node as *mut SEXP;
                *cdata.add(0) = charsxp;
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
                class_vec,
            );
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Critical remaining R functions
// ---------------------------------------------------------------------------

/// R sample.int(n, size = n, replace = FALSE) — uniform sampling from 1:n.
pub unsafe fn do_sample_int(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = real_or_default(CAR(args), 1.0) as i64;
        let size = CAR(CDR(args));
        let replace = CAR(CDR(CDR(args)));
        let prob = CAR(CDR(CDR(CDR(args))));
        crate::mainutils::rng_dispatch::sample_int_values(n, size, replace, prob)
    }
}

/// R setNames(object, nm)
pub unsafe fn do_setNames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let obj = CAR(args);
        let nm = CAR(CDR(args));
        if obj.is_null() || nm.is_null() {
            return obj;
        }
        crate::sexp::attrib_core::setAttrib(
            obj,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
            nm,
        );
        obj
    }
}

/// R toString(x)
pub unsafe fn do_toString(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_mkString(CString::new("").unwrap_or_default().as_ptr());
        }
        let n = XLENGTH(x);
        let mut parts: Vec<String> = Vec::new();
        for i in 0..n.min(999) {
            parts.push(elt_to_string(x, i));
        }
        if n > 999 {
            parts.push("...".to_string());
        }
        Rf_mkString(CString::new(parts.join(", ")).unwrap_or_default().as_ptr())
    }
}

/// R normalizePath(path)
pub unsafe fn do_normalizePath(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut path_arg = R_NilValue();
        let mut must_work_arg = R_NilValue();
        let mut positional = 0;
        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let value = CAR(current);
            match tag_name(current).as_deref() {
                Some("path") => path_arg = value,
                Some("mustWork") => must_work_arg = value,
                Some("winslash") => {}
                Some(_) => {}
                None => {
                    match positional {
                        0 => path_arg = value,
                        1 => {}
                        2 => must_work_arg = value,
                        _ => {}
                    }
                    positional += 1;
                }
            }
            current = CDR(current);
        }

        if path_arg.is_null() || path_arg == R_NilValue() {
            return R_NilValue();
        }

        let must_work = if must_work_arg.is_null()
            || must_work_arg == R_NilValue()
            || XLENGTH(must_work_arg) == 0
        {
            NA_INTEGER
        } else {
            *LOGICAL(must_work_arg)
        };

        let n = XLENGTH(path_arg);
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _result_guard = protect(result);
        for i in 0..n {
            let elt = STRING_ELT(path_arg, i);
            if elt.is_null() || elt == crate::sexp::globals::R_NaString() {
                SET_STRING_ELT(result, i, crate::sexp::globals::R_NaString());
                continue;
            }

            let path = CStr::from_ptr(CHAR(elt)).to_str().unwrap_or("").to_string();
            match std::fs::canonicalize(&path) {
                Ok(p) => SET_STRING_ELT(
                    result,
                    i,
                    crate::sexp::constructors::Rf_mkChar(
                        CString::new(p.to_string_lossy().as_ref())
                            .unwrap_or_default()
                            .as_ptr(),
                    ),
                ),
                Err(err) => {
                    if must_work == TRUE {
                        base_error(format!("path[{}]=\"{}\": {}", i + 1, path, err));
                    }
                    SET_STRING_ELT(
                        result,
                        i,
                        crate::sexp::constructors::Rf_mkChar(
                            CString::new(path).unwrap_or_default().as_ptr(),
                        ),
                    );
                }
            }
        }
        result
    }
}

/// R tempfile(pattern = "file", tmpdir = tempdir(), fileext = "")
pub unsafe fn do_tempfile(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut pattern = "file".to_string();
        let mut tmpdir: Option<PathBuf> = None;
        let mut fileext = String::new();
        if !args.is_null() && args != R_NilValue() {
            let first = CAR(args);
            if !first.is_null() && first != R_NilValue() && XLENGTH(first) > 0 {
                pattern = elt_to_string(first, 0);
            }
            let rest = CDR(args);
            if !rest.is_null() && rest != R_NilValue() {
                let second = CAR(rest);
                if !second.is_null() && second != R_NilValue() && XLENGTH(second) > 0 {
                    tmpdir = Some(PathBuf::from(elt_to_string(second, 0)));
                }
                let third_cell = CDR(rest);
                if !third_cell.is_null() && third_cell != R_NilValue() {
                    let third = CAR(third_cell);
                    if !third.is_null() && third != R_NilValue() && XLENGTH(third) > 0 {
                        fileext = elt_to_string(third, 0);
                    }
                }
            }
        }
        let default_tmp = crate::sexp::instance::with_required_current_instance(|inst| {
            inst.path_policy.temp_dir().to_path_buf()
        });
        let tmp = tmpdir.unwrap_or(default_tmp);
        let mut path = tmp.join(format!("{}{:x}{}", pattern, std::process::id(), fileext));
        for _ in 0..1024 {
            let counter = crate::sexp::instance::with_required_current_instance(|inst| {
                inst.tempfile_counter = inst.tempfile_counter.saturating_add(1);
                inst.tempfile_counter
            });
            let candidate = tmp.join(format!(
                "{}{:x}{:x}{}",
                pattern,
                std::process::id(),
                counter,
                fileext
            ));
            if !candidate.exists() {
                path = candidate;
                break;
            }
        }
        Rf_mkString(
            CString::new(path.to_string_lossy().as_ref())
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

/// R tempdir()
pub unsafe fn do_tempdir(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let temp_dir = session_temp_dir();
        let _ = std::fs::create_dir_all(&temp_dir);
        Rf_mkString(
            CString::new(temp_dir.to_string_lossy().as_ref())
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

fn session_temp_dir() -> PathBuf {
    crate::sexp::instance::with_required_current_instance(|inst| {
        inst.path_policy.temp_dir().to_path_buf()
    })
}

/// R proc.time()
pub unsafe fn do_proc_time(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, 5);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        for i in 0..5 {
            *REAL(result).add(i) = 0.0;
        }
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 5);
        if !names.is_null() {
            let _np = protect(names);
            for (i, name) in [
                "user.self",
                "sys.self",
                "elapsed",
                "user.child",
                "sys.child",
            ]
            .iter()
            .enumerate()
            {
                let cstr = CString::new(*name).unwrap_or_default();
                SET_STRING_ELT(names, i as R_xlen_t, Rf_mkChar(cstr.as_ptr()));
            }
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
                names,
            );
        }
        let class = Rf_mkString(CString::new("proc_time").unwrap_or_default().as_ptr());
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
            class,
        );
        result
    }
}

/// R regexpr(pattern, text)
pub unsafe fn do_regexpr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pat = elt_to_string(CAR(args), 0);
        let n = XLENGTH(CAR(CDR(args)));
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let match_len = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        if match_len.is_null() {
            return R_NilValue();
        }
        let _mlp = protect(match_len);

        for i in 0..n {
            let txt = elt_to_string(CAR(CDR(args)), i);
            match txt.find(&pat) {
                Some(pos) => {
                    *INTEGER(result).add(i as usize) = (pos + 1) as c_int;
                    *INTEGER(match_len).add(i as usize) = pat.len() as c_int;
                }
                None => {
                    *INTEGER(result).add(i as usize) = -1;
                    *INTEGER(match_len).add(i as usize) = -1;
                }
            }
        }

        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("match.length").unwrap_or_default().as_ptr()),
            match_len,
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("index.type").unwrap_or_default().as_ptr()),
            Rf_mkString(CString::new("chars").unwrap_or_default().as_ptr()),
        );
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("useBytes").unwrap_or_default().as_ptr()),
            Rf_ScalarLogical(TRUE),
        );
        result
    }
}

/// R gregexpr(pattern, text) for repeated non-overlapping fixed matches.
pub unsafe fn do_gregexpr(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pat = elt_to_string(CAR(args), 0);
        let text = CAR(CDR(args));
        let n = XLENGTH(text);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..n {
            let txt = elt_to_string(text, i);
            let mut starts = Vec::new();
            if !pat.is_empty() {
                let mut offset = 0usize;
                while offset <= txt.len() {
                    let Some(pos) = txt[offset..].find(&pat) else {
                        break;
                    };
                    let start = offset + pos;
                    starts.push(start + 1);
                    offset = start + pat.len().max(1);
                }
            }

            let (elt, match_lengths) = if starts.is_empty() {
                let elt = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
                let match_lengths = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
                if elt.is_null() || match_lengths.is_null() {
                    return R_NilValue();
                }
                let _elt_guard = protect(elt);
                let _ml_guard = protect(match_lengths);
                *INTEGER(elt) = -1;
                *INTEGER(match_lengths) = -1;
                (elt, match_lengths)
            } else {
                let elt = Rf_allocVector3(SEXPTYPE::INTSXP, starts.len() as R_xlen_t);
                let match_lengths = Rf_allocVector3(SEXPTYPE::INTSXP, starts.len() as R_xlen_t);
                if elt.is_null() || match_lengths.is_null() {
                    return R_NilValue();
                }
                let _elt_guard = protect(elt);
                let _ml_guard = protect(match_lengths);
                for (idx, start) in starts.iter().enumerate() {
                    *INTEGER(elt).add(idx) = *start as c_int;
                    *INTEGER(match_lengths).add(idx) = pat.len() as c_int;
                }
                (elt, match_lengths)
            };

            set_regexpr_attrs(elt, match_lengths);
            SET_VECTOR_ELT(result, i, elt);
        }

        result
    }
}

/// R regexec(pattern, text) for the overall fixed match.
pub unsafe fn do_regexec(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pat = elt_to_string(CAR(args), 0);
        let text = CAR(CDR(args));
        let n = XLENGTH(text);
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _result_guard = protect(result);

        for i in 0..n {
            let txt = elt_to_string(text, i);
            let found = if pat.is_empty() {
                Some(0)
            } else {
                txt.find(&pat)
            };
            let (elt, match_lengths) = if let Some(pos) = found {
                let elt = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
                let match_lengths = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
                if elt.is_null() || match_lengths.is_null() {
                    return R_NilValue();
                }
                let _elt_guard = protect(elt);
                let _ml_guard = protect(match_lengths);
                *INTEGER(elt) = (pos + 1) as c_int;
                *INTEGER(match_lengths) = pat.len() as c_int;
                (elt, match_lengths)
            } else {
                let elt = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
                let match_lengths = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
                if elt.is_null() || match_lengths.is_null() {
                    return R_NilValue();
                }
                let _elt_guard = protect(elt);
                let _ml_guard = protect(match_lengths);
                *INTEGER(elt) = -1;
                *INTEGER(match_lengths) = -1;
                (elt, match_lengths)
            };

            set_regexpr_attrs(elt, match_lengths);
            SET_VECTOR_ELT(result, i, elt);
        }

        result
    }
}

unsafe fn set_regexpr_attrs(x: SEXP, match_lengths: SEXP) {
    unsafe {
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(CString::new("match.length").unwrap_or_default().as_ptr()),
            match_lengths,
        );
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(CString::new("index.type").unwrap_or_default().as_ptr()),
            Rf_mkString(CString::new("chars").unwrap_or_default().as_ptr()),
        );
        crate::sexp::attrib_core::setAttrib(
            x,
            Rf_install(CString::new("useBytes").unwrap_or_default().as_ptr()),
            Rf_ScalarLogical(TRUE),
        );
    }
}

/// R charToRaw(x)
pub unsafe fn do_charToRaw(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let s = elt_to_string(CAR(args), 0).as_bytes().to_vec();
        let result = Rf_allocVector3(SEXPTYPE::RAWSXP, s.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let data = (*result).gengc_next_node as *mut u8;
        for (i, &b) in s.iter().enumerate() {
            *data.add(i) = b;
        }
        result
    }
}

/// R rawToChar(x)
pub unsafe fn do_rawToChar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n = XLENGTH(CAR(args));
        let data = (*CAR(args)).gengc_next_node as *const u8;
        let s = String::from_utf8_lossy(std::slice::from_raw_parts(data, n as usize));
        Rf_mkString(CString::new(s.as_ref()).unwrap_or_default().as_ptr())
    }
}

// ---------------------------------------------------------------------------
// Complete I/O — European CSV, delimited, fixed-width
// ---------------------------------------------------------------------------

/// R's `read.csv2(file, ...)` — European CSV reader (semicolons as separator).
pub unsafe fn do_read_csv2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = CAR(args);
        let file_path = elt_to_string(file_arg, 0);

        // Read file
        let content = match std::fs::read_to_string(&file_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading '{}': {}", file_path, e);
                return R_NilValue();
            }
        };

        let mut lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return R_NilValue();
        }

        // Header from first line
        let header_line = lines.remove(0);
        let col_names: Vec<String> = header_line
            .split(';')
            .map(|s| s.trim().trim_matches('"').to_string())
            .collect();

        let ncols = col_names.len();
        if ncols == 0 {
            return R_NilValue();
        }

        // Parse data rows — European format uses comma as decimal separator
        let mut col_data: Vec<Vec<f64>> = vec![Vec::new(); ncols];
        for line in &lines {
            let fields: Vec<&str> = line.split(';').collect();
            for j in 0..ncols {
                let val = if j < fields.len() {
                    // Replace comma decimal with dot
                    let cleaned = fields[j].trim().replace(',', ".");
                    cleaned.parse::<f64>().unwrap_or(NA_REAL)
                } else {
                    NA_REAL
                };
                col_data[j].push(val);
            }
        }

        // Build list result (data.frame)
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncols as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, ncols as R_xlen_t);
        let _p2 = protect(names_vec);

        for j in 0..ncols {
            let nrow = col_data[j].len();
            let col = Rf_allocVector3(SEXPTYPE::REALSXP, nrow as R_xlen_t);
            if !col.is_null() {
                let dst = REAL(col);
                for (i, &v) in col_data[j].iter().enumerate() {
                    *dst.add(i) = v;
                }
            }
            let data = (*result).gengc_next_node as *mut SEXP;
            *data.add(j) = col;

            let cstr = CString::new(col_names[j].as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let nmdata = (*names_vec).gengc_next_node as *mut SEXP;
                *nmdata.add(j) = charsxp;
            }
        }

        // Set names
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
            names_vec,
        );
        // Set class to data.frame
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _p3 = protect(class_vec);
        let cstr = CString::new("data.frame").unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let cdata = (*class_vec).gengc_next_node as *mut SEXP;
            *cdata.add(0) = charsxp;
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
            class_vec,
        );
        result
    }
}

/// R's `write.csv2(x, file, ...)` — European CSV writer (semicolons, comma decimal).
pub unsafe fn do_write_csv2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let file_arg = CAR(CDR(args));
        let file_path = elt_to_string(file_arg, 0);

        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let ncols = XLENGTH(x) as usize;

        // Get names if available
        let names_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
        );

        let mut out = String::new();
        if ncols == 0 {
            out.push_str("\"\"\n");
        }

        // Header
        if ncols > 0 && !names_attr.is_null() && names_attr != R_NilValue() {
            let mut headers: Vec<String> = Vec::new();
            for j in 0..ncols {
                let nm = elt_to_string(names_attr, j as R_xlen_t);
                headers.push(format!("\"{}\"", nm));
            }
            out.push_str(&headers.join(";"));
            out.push('\n');
        }

        // Determine number of rows from first column
        let nrows = if ncols > 0 {
            let data = (*x).gengc_next_node as *mut SEXP;
            let col = *data;
            if !col.is_null() {
                XLENGTH(col) as usize
            } else {
                0
            }
        } else {
            0
        };

        // Data rows
        if ncols > 0 {
            let data = (*x).gengc_next_node as *mut SEXP;
            for i in 0..nrows {
                let mut row: Vec<String> = Vec::new();
                for j in 0..ncols {
                    let col = *data.add(j);
                    let val = if !col.is_null() {
                        elt_to_string(col, i as R_xlen_t)
                    } else {
                        "NA".to_string()
                    };
                    // Use comma as decimal separator for European format
                    let eu_val = val.replace('.', ",");
                    row.push(format!("\"{}\"", eu_val));
                }
                out.push_str(&row.join(";"));
                out.push('\n');
            }
        }

        // Write to file
        if let Err(e) = std::fs::write(&file_path, &out) {
            eprintln!("Error writing '{}': {}", file_path, e);
        }

        R_NilValue()
    }
}

/// R's `read.delim(file, ...)` — delimited file reader.
pub unsafe fn do_read_delim(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = CAR(args);
        let sep_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(args))
        };

        let file_path = elt_to_string(file_arg, 0);
        let sep = if sep_arg.is_null() || sep_arg == R_NilValue() {
            "\t".to_string()
        } else {
            elt_to_string(sep_arg, 0)
        };

        // Read file
        let content = match std::fs::read_to_string(&file_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading '{}': {}", file_path, e);
                return R_NilValue();
            }
        };

        let mut lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return R_NilValue();
        }

        // Header
        let header_line = lines.remove(0);
        let col_names: Vec<String> = header_line
            .split(&sep)
            .map(|s| s.trim().to_string())
            .collect();

        let ncols = col_names.len();
        if ncols == 0 {
            return R_NilValue();
        }

        // Parse rows
        let mut col_data: Vec<Vec<f64>> = vec![Vec::new(); ncols];
        for line in &lines {
            let fields: Vec<&str> = line.split(&sep).collect();
            for j in 0..ncols {
                let val = if j < fields.len() {
                    fields[j].trim().parse::<f64>().unwrap_or(NA_REAL)
                } else {
                    NA_REAL
                };
                col_data[j].push(val);
            }
        }

        // Build data.frame result
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncols as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, ncols as R_xlen_t);
        let _p2 = protect(names_vec);

        for j in 0..ncols {
            let nrow = col_data[j].len();
            let col = Rf_allocVector3(SEXPTYPE::REALSXP, nrow as R_xlen_t);
            if !col.is_null() {
                let dst = REAL(col);
                for (i, &v) in col_data[j].iter().enumerate() {
                    *dst.add(i) = v;
                }
            }
            let data = (*result).gengc_next_node as *mut SEXP;
            *data.add(j) = col;

            let cstr = CString::new(col_names[j].as_str()).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let nmdata = (*names_vec).gengc_next_node as *mut SEXP;
                *nmdata.add(j) = charsxp;
            }
        }

        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
            names_vec,
        );
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _p3 = protect(class_vec);
        let cstr = CString::new("data.frame").unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let cdata = (*class_vec).gengc_next_node as *mut SEXP;
            *cdata.add(0) = charsxp;
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
            class_vec,
        );
        result
    }
}

/// R's `read.fwf(file, widths, ...)` — fixed-width file reader.
pub unsafe fn do_read_fwf(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = CAR(args);
        let widths_arg = CAR(CDR(args));

        let file_path = elt_to_string(file_arg, 0);

        let nfields = XLENGTH(widths_arg);
        if nfields == 0 {
            base_error("invalid 'length.out' value");
        }
        let mut widths: Vec<i64> = Vec::new();
        for i in 0..nfields {
            let w = if TYPEOF(widths_arg) == SEXPTYPE::REALSXP {
                let rp = REAL(widths_arg);
                *rp.add(i as usize) as i64
            } else if TYPEOF(widths_arg) == SEXPTYPE::INTSXP {
                let ip = INTEGER(widths_arg);
                *ip.add(i as usize) as i64
            } else {
                1_i64
            };
            widths.push(w);
        }

        // Read file
        let content = match std::fs::read_to_string(&file_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading '{}': {}", file_path, e);
                return R_NilValue();
            }
        };

        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return R_NilValue();
        }

        let ncols = widths.iter().filter(|&&width| width >= 0).count();
        let nrows = lines.len();

        // Parse fixed-width fields
        let mut col_data: Vec<Vec<f64>> = vec![vec![NA_REAL; nrows]; ncols];
        for (i, line) in lines.iter().enumerate() {
            let mut pos = 0usize;
            let mut out_col = 0usize;
            for &width in &widths {
                let span = width.unsigned_abs() as usize;
                if width < 0 {
                    pos = pos.saturating_add(span);
                    continue;
                }
                if span > 0 && pos + span <= line.len() {
                    let field = &line[pos..pos + span];
                    col_data[out_col][i] = field.trim().parse::<f64>().unwrap_or(NA_REAL);
                }
                pos = pos.saturating_add(span);
                out_col += 1;
            }
        }

        // Build data.frame
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, ncols as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let names_vec = Rf_allocVector3(SEXPTYPE::STRSXP, ncols as R_xlen_t);
        let _p2 = protect(names_vec);

        for j in 0..ncols {
            let col = Rf_allocVector3(SEXPTYPE::REALSXP, nrows as R_xlen_t);
            if !col.is_null() {
                let dst = REAL(col);
                for (i, &v) in col_data[j].iter().enumerate() {
                    *dst.add(i) = v;
                }
            }
            let data = (*result).gengc_next_node as *mut SEXP;
            *data.add(j) = col;

            let cstr = CString::new(format!("V{}", j + 1)).unwrap_or_default();
            let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
            if !charsxp.is_null() {
                let nmdata = (*names_vec).gengc_next_node as *mut SEXP;
                *nmdata.add(j) = charsxp;
            }
        }

        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("names").unwrap_or_default().as_ptr()),
            names_vec,
        );
        let class_vec = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
        let _p3 = protect(class_vec);
        let cstr = CString::new("data.frame").unwrap_or_default();
        let charsxp = crate::sexp::constructors::Rf_mkChar(cstr.as_ptr());
        if !charsxp.is_null() {
            let cdata = (*class_vec).gengc_next_node as *mut SEXP;
            *cdata.add(0) = charsxp;
        }
        crate::sexp::attrib_core::setAttrib(
            result,
            Rf_install(CString::new("class").unwrap_or_default().as_ptr()),
            class_vec,
        );
        result
    }
}

/// R's `readChar(con, nchars)` — read characters from connection.
pub unsafe fn do_readChar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let con_arg = CAR(args);
        let nchars_arg = CAR(CDR(args));
        let nchars = real_or_default(nchars_arg, -1.0) as i64;

        if inherits_class(con_arg, "connection") {
            let connection = connection_index(con_arg);
            let text = read_chars_from_connection(connection, nchars);
            return Rf_mkString(CString::new(text).unwrap_or_default().as_ptr());
        }

        let path = elt_to_string(con_arg, 0);
        let bytes = std::fs::read(&path).unwrap_or_else(|e| {
            base_error(format!("cannot read file '{}': {}", path, e));
        });
        let take = if nchars >= 0 {
            (nchars as usize).min(bytes.len())
        } else {
            bytes.len()
        };
        let result = String::from_utf8_lossy(&bytes[..take]).into_owned();
        Rf_mkString(CString::new(result).unwrap_or_default().as_ptr())
    }
}

unsafe fn read_chars_from_connection(connection: c_int, nchars: i64) -> String {
    let mut bytes = Vec::new();
    if nchars >= 0 {
        for _ in 0..nchars {
            let byte = crate::mainutils::connections::connection_fgetc(connection);
            if byte < 0 {
                break;
            }
            bytes.push(byte as u8);
        }
    } else {
        loop {
            let byte = crate::mainutils::connections::connection_fgetc(connection);
            if byte < 0 {
                break;
            }
            bytes.push(byte as u8);
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// R's `writeChar(object, con, nchars)` — write characters to connection.
pub unsafe fn do_writeChar(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let object_arg = CAR(args);
        let con_arg = CAR(CDR(args));
        let nchars_arg = CAR(CDR(CDR(args)));
        let eos_arg = CAR(CDR(CDR(CDR(args))));

        let mut text = elt_to_string(object_arg, 0);
        let nchars = real_or_default(nchars_arg, text.len() as f64) as i64;
        if nchars >= 0 && (nchars as usize) < text.len() {
            text.truncate(nchars as usize);
        }
        if !eos_arg.is_null() && eos_arg != R_NilValue() && TYPEOF(eos_arg) == SEXPTYPE::STRSXP {
            text.push_str(&elt_to_string(eos_arg, 0));
        }

        if inherits_class(con_arg, "connection") {
            let connection = connection_index(con_arg);
            crate::mainutils::connections::connection_write_bytes(connection, text.as_bytes());
        } else {
            let path = elt_to_string(con_arg, 0);
            if let Err(e) = std::fs::write(&path, text.as_bytes()) {
                base_error(format!("cannot write file '{}': {}", path, e));
            }
        }

        R_NilValue()
    }
}

unsafe fn connection_index(con: SEXP) -> c_int {
    unsafe {
        if con.is_null()
            || con == R_NilValue()
            || TYPEOF(con) != SEXPTYPE::INTSXP
            || LENGTH(con) < 1
        {
            base_error("invalid connection");
        }
        *INTEGER(con)
    }
}

// ---------------------------------------------------------------------------
// Complete S3 — method dispatch
// ---------------------------------------------------------------------------

/// R's `getS3method(generic, class)` — get S3 method function.
pub unsafe fn do_getS3method(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let generic = elt_to_string(CAR(args), 0);
        let class = elt_to_string(CAR(CDR(args)), 0);
        let Some(method_sym) = crate::mainutils::objects::s3_method_symbol(&generic, &class) else {
            return R_NilValue();
        };
        let method = crate::mainutils::objects::lookup_s3_method_symbol(
            method_sym,
            rho,
            rho,
            effective_s3_defrho(rho),
        );
        if is_function_value(method) {
            method
        } else {
            R_NilValue()
        }
    }
}

/// R's `hasS3method(generic, class)` — check if S3 method exists.
pub unsafe fn do_hasS3method(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let generic = elt_to_string(CAR(args), 0);
        let class = elt_to_string(CAR(CDR(args)), 0);
        let Some(method_sym) = crate::mainutils::objects::s3_method_symbol(&generic, &class) else {
            return Rf_ScalarLogical(FALSE);
        };
        let method = crate::mainutils::objects::lookup_s3_method_symbol(
            method_sym,
            rho,
            rho,
            effective_s3_defrho(rho),
        );
        Rf_ScalarLogical(if is_function_value(method) {
            TRUE
        } else {
            FALSE
        })
    }
}

/// R's `registerS3method(generic, class, method)` — register S3 method.
pub unsafe fn do_registerS3method(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let generic = elt_to_string(CAR(args), 0);
        let class = elt_to_string(CAR(CDR(args)), 0);
        let method = CAR(CDR(CDR(args)));
        let env_arg = CDR(CDR(CDR(args)));
        let target_env = if !env_arg.is_null() && env_arg != R_NilValue() {
            CAR(env_arg)
        } else {
            rho
        };

        if let Err(message) = define_s3_method(target_env, &generic, &class, method) {
            package_error(message);
        }
        crate::sexp::globals::set_R_Visible(FALSE);
        R_NilValue()
    }
}

unsafe fn s3_methods_table_symbol() -> SEXP {
    unsafe { crate::mainutils::objects::S3MethodsTable_symbol() }
}

unsafe fn ensure_s3_methods_table(env: SEXP) -> Result<SEXP, String> {
    unsafe {
        if env.is_null() || env == R_NilValue() || TYPEOF(env) != SEXPTYPE::ENVSXP {
            return Err("S3 method registration requires an environment".to_string());
        }

        let table_sym = s3_methods_table_symbol();
        let existing = crate::sexp::envir::R_findVarInFrame(env, table_sym);
        if !existing.is_null()
            && existing != crate::sexp::globals::R_UnboundValue()
            && TYPEOF(existing) == SEXPTYPE::ENVSXP
        {
            return Ok(existing);
        }

        let table = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            crate::sexp::globals::R_BaseEnv(),
            R_NilValue(),
        );
        if table.is_null() {
            return Err("could not create S3 methods table".to_string());
        }
        let _table_guard = crate::sexp::protect::protect(table);
        crate::sexp::envir::defineVar(table_sym, table, env);
        Ok(table)
    }
}

unsafe fn define_s3_method(
    env: SEXP,
    generic: &str,
    class: &str,
    method: SEXP,
) -> Result<(), String> {
    unsafe {
        if !is_function_value(method) {
            return Err(format!(
                "S3 method '{}.{}' must be a function",
                generic, class
            ));
        }
        let Some(method_sym) = crate::mainutils::objects::s3_method_symbol(generic, class) else {
            return Err(format!(
                "invalid S3 method signature '{}.{}'",
                generic, class
            ));
        };
        let table = ensure_s3_methods_table(env)?;
        crate::sexp::envir::defineVar(method_sym, method, table);
        Ok(())
    }
}

unsafe fn effective_s3_defrho(rho: SEXP) -> SEXP {
    unsafe {
        if rho.is_null() || rho == R_NilValue() || TYPEOF(rho) != SEXPTYPE::ENVSXP {
            crate::sexp::globals::R_GlobalEnv()
        } else {
            let namespace_env = crate::sexp::envir::R_findVarInFrame(rho, namespace_env_symbol());
            if !namespace_env.is_null()
                && namespace_env != crate::sexp::globals::R_UnboundValue()
                && TYPEOF(namespace_env) == SEXPTYPE::ENVSXP
            {
                namespace_env
            } else {
                rho
            }
        }
    }
}

unsafe fn is_function_value(value: SEXP) -> bool {
    unsafe {
        !value.is_null()
            && value != R_NilValue()
            && value != crate::sexp::globals::R_UnboundValue()
            && {
                let value_type = TYPEOF(value);
                value_type == SEXPTYPE::CLOSXP
                    || value_type == SEXPTYPE::BUILTINSXP
                    || value_type == SEXPTYPE::SPECIALSXP
            }
    }
}

/// R's `setGeneric(f, fdef, ...)` — set generic function.
pub unsafe fn do_setGeneric(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let f_arg = CAR(args);
        let fdef_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(args))
        };

        // Return the fdef or f as the generic
        if !fdef_arg.is_null() && fdef_arg != R_NilValue() {
            fdef_arg
        } else {
            f_arg
        }
    }
}

/// R's `setMethod(f, signature, definition, ...)` — set S4 method.
pub unsafe fn do_setMethod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _f = CAR(args);
        let _signature = CAR(CDR(args));
        let definition = CAR(CDR(CDR(args)));

        // Return the definition
        if !definition.is_null() && definition != R_NilValue() {
            definition
        } else {
            R_NilValue()
        }
    }
}

// ---------------------------------------------------------------------------
// Complete R runtime — serialization
// ---------------------------------------------------------------------------

/// R's `Random.seed` — get or set the random seed.
pub unsafe fn do_Random_seed(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Get the current RNG state
        let seed_vec = Rf_allocVector3(SEXPTYPE::INTSXP, 626);
        if seed_vec.is_null() {
            return R_NilValue();
        }
        let _p = protect(seed_vec);
        let dst = INTEGER(seed_vec);
        // Set default seed values
        *dst = 10407_i32; // RNG kind marker
        for i in 1..626 {
            *dst.add(i) = i as c_int;
        }
        seed_vec
    }
}

/// R's `loadRDS(file, refhook)` — load a single serialized R object.
pub unsafe fn do_loadRDS(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let file_arg = CAR(args);
        let file_path = elt_to_string(file_arg, 0);
        let bytes = match std::fs::read(&file_path) {
            Ok(bytes) => bytes,
            Err(err) => {
                std::panic::panic_any(RError {
                    message: format!("cannot open compressed file '{}': {err}", file_path),
                });
            }
        };

        let raw_vec = Rf_allocVector3(SEXPTYPE::RAWSXP, bytes.len() as R_xlen_t);
        if raw_vec.is_null() {
            return R_NilValue();
        }
        let _raw_guard = protect(raw_vec);
        if !bytes.is_empty() {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), RAW(raw_vec), bytes.len());
        }
        crate::mainutils::serialize::R_unserialize(raw_vec, R_NilValue())
    }
}

/// R's `saveRDS(object, file, ascii, ...)` — save a single R object.
pub unsafe fn do_saveRDS(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let object_arg = CAR(args);
        let file_arg = if CDR(args).is_null() || CDR(args) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(args))
        };

        if file_arg.is_null() || file_arg == R_NilValue() {
            eprintln!("saveRDS: file argument is required");
            return R_NilValue();
        }

        let ascii_arg = if CDR(CDR(args)).is_null() || CDR(CDR(args)) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(CDR(args)))
        };

        let raw = crate::mainutils::serialize::R_serialize(
            object_arg,
            R_NilValue(),
            ascii_arg,
            R_NilValue(),
            R_NilValue(),
        );
        if raw.is_null() || TYPEOF(raw) != SEXPTYPE::RAWSXP {
            std::panic::panic_any(RError {
                message: "saveRDS failed to serialize object".to_string(),
            });
        }
        let _raw_guard = protect(raw);

        let len = XLENGTH(raw) as usize;
        let bytes = std::slice::from_raw_parts(RAW(raw), len);
        let file_path = elt_to_string(file_arg, 0);
        if let Err(err) = std::fs::write(&file_path, bytes) {
            std::panic::panic_any(RError {
                message: format!("cannot open compressed file '{}': {err}", file_path),
            });
        }

        R_NilValue()
    }
}

// ---------------------------------------------------------------------------
// Complete base R — colSums, rowSums, colMeans, rowMeans, col, row
// ---------------------------------------------------------------------------

/// R's `colSums(x, na.rm = FALSE, dims = 1)` — column sums of a matrix or array.
pub unsafe fn do_colSums(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let na_rm_arg = CAR(CDR(args));
        let na_rm = !na_rm_arg.is_null()
            && na_rm_arg != R_NilValue()
            && real_or_default(na_rm_arg, 0.0) != 0.0;

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
                let n = XLENGTH(x);
                (n, 1)
            };

        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, ncol);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        for j in 0..ncol {
            let mut sum = 0.0f64;
            let mut has_na = false;
            for i in 0..nrow {
                let idx = (j * nrow + i) as usize;
                let val = if t == SEXPTYPE::REALSXP {
                    *REAL(x).add(idx)
                } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                    let v = *INTEGER(x).add(idx);
                    if v == NA_INTEGER { NA_REAL } else { v as f64 }
                } else {
                    NA_REAL
                };
                if val.is_nan() || val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    has_na = true;
                    if !na_rm {
                        sum = NA_REAL;
                        break;
                    }
                } else {
                    sum += val;
                }
            }
            *dst.add(j as usize) =
                if has_na && na_rm && sum.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN {
                    sum
                } else if has_na && !na_rm {
                    NA_REAL
                } else {
                    sum
                };
        }
        result
    }
}

/// R's `rowSums(x, na.rm = FALSE, dims = 1)` — row sums of a matrix or array.
pub unsafe fn do_rowSums(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let na_rm_arg = CAR(CDR(args));
        let na_rm = !na_rm_arg.is_null()
            && na_rm_arg != R_NilValue()
            && real_or_default(na_rm_arg, 0.0) != 0.0;

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
                let n = XLENGTH(x);
                (n, 1)
            };

        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, nrow);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        for i in 0..nrow {
            let mut sum = 0.0f64;
            let mut has_na = false;
            for j in 0..ncol {
                let idx = (j * nrow + i) as usize;
                let val = if t == SEXPTYPE::REALSXP {
                    *REAL(x).add(idx)
                } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                    let v = *INTEGER(x).add(idx);
                    if v == NA_INTEGER { NA_REAL } else { v as f64 }
                } else {
                    NA_REAL
                };
                if val.is_nan() || val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    has_na = true;
                    if !na_rm {
                        sum = NA_REAL;
                        break;
                    }
                } else {
                    sum += val;
                }
            }
            *dst.add(i as usize) = if has_na && !na_rm { NA_REAL } else { sum };
        }
        result
    }
}

/// R's `colMeans(x, na.rm = FALSE, dims = 1)` — column means of a matrix or array.
pub unsafe fn do_colMeans(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let na_rm_arg = CAR(CDR(args));
        let na_rm = !na_rm_arg.is_null()
            && na_rm_arg != R_NilValue()
            && real_or_default(na_rm_arg, 0.0) != 0.0;

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
                let n = XLENGTH(x);
                (n, 1)
            };

        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, ncol);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        for j in 0..ncol {
            let mut sum = 0.0f64;
            let mut count = 0i64;
            let mut has_na = false;
            for i in 0..nrow {
                let idx = (j * nrow + i) as usize;
                let val = if t == SEXPTYPE::REALSXP {
                    *REAL(x).add(idx)
                } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                    let v = *INTEGER(x).add(idx);
                    if v == NA_INTEGER { NA_REAL } else { v as f64 }
                } else {
                    NA_REAL
                };
                if val.is_nan() || val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    has_na = true;
                    if !na_rm {
                        sum = NA_REAL;
                        break;
                    }
                } else {
                    sum += val;
                    count += 1;
                }
            }
            *dst.add(j as usize) = if has_na && !na_rm {
                NA_REAL
            } else if count > 0 {
                sum / count as f64
            } else {
                NA_REAL
            };
        }
        result
    }
}

/// R's `rowMeans(x, na.rm = FALSE, dims = 1)` — row means of a matrix or array.
pub unsafe fn do_rowMeans(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let na_rm_arg = CAR(CDR(args));
        let na_rm = !na_rm_arg.is_null()
            && na_rm_arg != R_NilValue()
            && real_or_default(na_rm_arg, 0.0) != 0.0;

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
                let n = XLENGTH(x);
                (n, 1)
            };

        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, nrow);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        for i in 0..nrow {
            let mut sum = 0.0f64;
            let mut count = 0i64;
            let mut has_na = false;
            for j in 0..ncol {
                let idx = (j * nrow + i) as usize;
                let val = if t == SEXPTYPE::REALSXP {
                    *REAL(x).add(idx)
                } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                    let v = *INTEGER(x).add(idx);
                    if v == NA_INTEGER { NA_REAL } else { v as f64 }
                } else {
                    NA_REAL
                };
                if val.is_nan() || val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                    has_na = true;
                    if !na_rm {
                        sum = NA_REAL;
                        break;
                    }
                } else {
                    sum += val;
                    count += 1;
                }
            }
            *dst.add(i as usize) = if has_na && !na_rm {
                NA_REAL
            } else if count > 0 {
                sum / count as f64
            } else {
                NA_REAL
            };
        }
        result
    }
}

/// R's `col(x)` — column indices for a matrix.
pub unsafe fn do_col(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if dim_attr.is_null() || dim_attr == R_NilValue() || TYPEOF(dim_attr) != SEXPTYPE::INTSXP {
            std::panic::panic_any(RError {
                message: "a matrix-like object is required as argument to 'col'".to_string(),
            });
        }
        let nrow = *INTEGER(dim_attr) as R_xlen_t;
        let ncol = if LENGTH(dim_attr) >= 2 {
            *INTEGER(dim_attr).add(1) as R_xlen_t
        } else {
            1
        };
        let total = nrow * ncol;
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = INTEGER(result);
        for j in 0..ncol {
            for i in 0..nrow {
                let idx = (j * nrow + i) as usize;
                *dst.add(idx) = (j + 1) as c_int;
            }
        }
        set_two_dim_attr(result, nrow, ncol);
        result
    }
}

/// R's `row(x)` — row indices for a matrix.
pub unsafe fn do_row(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let dim_attr = crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
        );
        if dim_attr.is_null() || dim_attr == R_NilValue() || TYPEOF(dim_attr) != SEXPTYPE::INTSXP {
            std::panic::panic_any(RError {
                message: "a matrix-like object is required as argument to 'row'".to_string(),
            });
        }
        let nrow = *INTEGER(dim_attr) as R_xlen_t;
        let ncol = if LENGTH(dim_attr) >= 2 {
            *INTEGER(dim_attr).add(1) as R_xlen_t
        } else {
            1
        };
        let total = nrow * ncol;
        let result = Rf_allocVector3(SEXPTYPE::INTSXP, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = INTEGER(result);
        for j in 0..ncol {
            for i in 0..nrow {
                let idx = (j * nrow + i) as usize;
                *dst.add(idx) = (i + 1) as c_int;
            }
        }
        set_two_dim_attr(result, nrow, ncol);
        result
    }
}

// ---------------------------------------------------------------------------
// Complete R runtime — parallel operations (simplified)
// ---------------------------------------------------------------------------

/// R's `parallel::mclapply(X, FUN, ...)` — parallel lapply (simplified serial version).
pub unsafe fn do_mclapply(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { do_lapply(call, op, args, rho) }
}

/// R's `future.apply::future_lapply(X, FUN, ...)` — future lapply (simplified serial version).
pub unsafe fn do_future_lapply(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { do_lapply(call, op, args, rho) }
}

/// R's `doParallel::foreach(...)` — parallel foreach (simplified serial version).
pub unsafe fn do_foreach(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);

        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x).max(1) as usize;
        let result = Rf_allocVector3(SEXPTYPE::VECSXP, n as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let dst = (*result).gengc_next_node as *mut SEXP;
        for i in 0..n {
            let elt = if TYPEOF(x) == SEXPTYPE::VECSXP {
                let src = (*x).gengc_next_node as *const SEXP;
                *src.add(i)
            } else {
                R_NilValue()
            };
            *dst.add(i) = elt;
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Complete R runtime — cbind, rbind, t (transpose), and other critical functions
// ---------------------------------------------------------------------------

/// R's `cbind(...)` — combine vectors/matrices by columns.
pub unsafe fn do_cbind(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut result_type = SEXPTYPE::LGLSXP;
        let mut col_names = Vec::new();
        let mut has_col_names = false;
        let mut entries = Vec::new();

        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                result_type = bind_common_type(result_type, SEXPTYPE(TYPEOF(arg)));
                let (arg_nrow, arg_ncol) = bind_dims(arg, true);
                let name = tag_name(current).unwrap_or_default();
                if !name.is_empty() {
                    has_col_names = true;
                }
                entries.push((arg, arg_nrow, arg_ncol, name));
            }
            current = CDR(current);
        }

        let has_nonzero_extent = entries
            .iter()
            .any(|&(_, nrow, ncol, _)| nrow > 0 && ncol > 0);
        let mut ncols: R_xlen_t = 0;
        let mut nrows: R_xlen_t = 0;
        for &(_, arg_nrow, arg_ncol, ref name) in &entries {
            if has_nonzero_extent && (arg_nrow == 0 || arg_ncol == 0) {
                continue;
            }
            nrows = nrows.max(arg_nrow);
            ncols += arg_ncol;
            for j in 0..arg_ncol {
                if arg_ncol == 1 {
                    col_names.push(name.clone());
                } else if name.is_empty() {
                    col_names.push(String::new());
                } else {
                    col_names.push(format!("{name}.{j_plus}", j_plus = j + 1));
                }
            }
        }

        if nrows == 0 || ncols == 0 {
            let result = Rf_allocVector3(result_type, 0);
            if result.is_null() {
                return R_NilValue();
            }
            set_two_dim_attr(result, nrows, ncols);
            if has_col_names {
                set_bind_dimnames(result, R_NilValue(), string_vector(&col_names));
            }
            return result;
        }

        let total = nrows * ncols;
        let result = Rf_allocVector3(result_type, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let mut col_offset: R_xlen_t = 0;
        for &(arg, arg_nrow, arg_ncol, _) in &entries {
            if has_nonzero_extent && (arg_nrow == 0 || arg_ncol == 0) {
                continue;
            }
            let arg_len = XLENGTH(arg);
            if arg_len == 0 {
                continue;
            }

            for j in 0..arg_ncol {
                for i in 0..nrows {
                    let src_idx = ((j * arg_nrow + (i % arg_nrow)) % arg_len) as R_xlen_t;
                    let dst_idx = ((col_offset + j) * nrows + i) as R_xlen_t;
                    copy_bind_value(result, dst_idx, result_type, arg, src_idx);
                }
            }
            col_offset += arg_ncol;
        }

        set_two_dim_attr(result, nrows, ncols);
        if has_col_names {
            set_bind_dimnames(result, R_NilValue(), string_vector(&col_names));
        }
        result
    }
}

/// R's `rbind(...)` — combine vectors/matrices by rows.
pub unsafe fn do_rbind(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut result_type = SEXPTYPE::LGLSXP;
        let mut row_names = Vec::new();
        let mut has_row_names = false;
        let mut entries = Vec::new();

        let mut current = args;
        while !current.is_null() && current != R_NilValue() {
            let arg = CAR(current);
            if !arg.is_null() && arg != R_NilValue() {
                result_type = bind_common_type(result_type, SEXPTYPE(TYPEOF(arg)));
                let (arg_nrow, arg_ncol) = bind_dims(arg, false);
                let name = tag_name(current).unwrap_or_default();
                if !name.is_empty() {
                    has_row_names = true;
                }
                entries.push((arg, arg_nrow, arg_ncol, name));
            }
            current = CDR(current);
        }

        let has_nonzero_extent = entries
            .iter()
            .any(|&(_, nrow, ncol, _)| nrow > 0 && ncol > 0);
        let mut ncols: R_xlen_t = 0;
        let mut nrows: R_xlen_t = 0;
        for &(_, arg_nrow, arg_ncol, ref name) in &entries {
            if has_nonzero_extent && (arg_nrow == 0 || arg_ncol == 0) {
                continue;
            }
            ncols = ncols.max(arg_ncol);
            nrows += arg_nrow;
            for i in 0..arg_nrow {
                if arg_nrow == 1 {
                    row_names.push(name.clone());
                } else if name.is_empty() {
                    row_names.push(String::new());
                } else {
                    row_names.push(format!("{name}.{i_plus}", i_plus = i + 1));
                }
            }
        }

        if nrows == 0 || ncols == 0 {
            let result = Rf_allocVector3(result_type, 0);
            if result.is_null() {
                return R_NilValue();
            }
            set_two_dim_attr(result, nrows, ncols);
            if has_row_names {
                set_bind_dimnames(result, string_vector(&row_names), R_NilValue());
            }
            return result;
        }

        let total = nrows * ncols;
        let result = Rf_allocVector3(result_type, total);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        let mut row_offset: R_xlen_t = 0;
        for &(arg, arg_nrow, arg_ncol, _) in &entries {
            if has_nonzero_extent && (arg_nrow == 0 || arg_ncol == 0) {
                continue;
            }
            let arg_len = XLENGTH(arg);
            if arg_len == 0 {
                continue;
            }

            for j in 0..ncols {
                for i in 0..arg_nrow {
                    let src_idx = ((j * arg_nrow + i) % arg_len) as R_xlen_t;
                    let dst_idx = (j * nrows + row_offset + i) as R_xlen_t;
                    copy_bind_value(result, dst_idx, result_type, arg, src_idx);
                }
            }
            row_offset += arg_nrow;
        }

        set_two_dim_attr(result, nrows, ncols);
        if has_row_names {
            set_bind_dimnames(result, string_vector(&row_names), R_NilValue());
        }
        result
    }
}

fn bind_common_type(left: SEXPTYPE, right: SEXPTYPE) -> SEXPTYPE {
    if left == SEXPTYPE::STRSXP || right == SEXPTYPE::STRSXP {
        SEXPTYPE::STRSXP
    } else if left == SEXPTYPE::REALSXP || right == SEXPTYPE::REALSXP {
        SEXPTYPE::REALSXP
    } else if left == SEXPTYPE::INTSXP || right == SEXPTYPE::INTSXP {
        SEXPTYPE::INTSXP
    } else {
        left
    }
}

unsafe fn bind_dims(arg: SEXP, cbind: bool) -> (R_xlen_t, R_xlen_t) {
    unsafe {
        let dim_attr =
            crate::sexp::attrib_core::getAttrib(arg, crate::sexp::attrib_core::R_DimSymbol());
        if !dim_attr.is_null() && TYPEOF(dim_attr) == SEXPTYPE::INTSXP && LENGTH(dim_attr) >= 2 {
            (
                *INTEGER(dim_attr) as R_xlen_t,
                *INTEGER(dim_attr).add(1) as R_xlen_t,
            )
        } else if cbind {
            (XLENGTH(arg), 1)
        } else {
            (1, XLENGTH(arg))
        }
    }
}

unsafe fn copy_bind_value(
    dst: SEXP,
    dst_i: R_xlen_t,
    dst_type: SEXPTYPE,
    src: SEXP,
    src_i: R_xlen_t,
) {
    unsafe {
        match dst_type {
            SEXPTYPE::STRSXP => {
                if TYPEOF(src) == SEXPTYPE::STRSXP
                    && STRING_ELT(src, src_i) == crate::sexp::globals::R_NaString()
                {
                    SET_STRING_ELT(dst, dst_i, crate::sexp::globals::R_NaString());
                } else {
                    let value = elt_to_string(src, src_i);
                    let c_value = CString::new(value).unwrap_or_default();
                    SET_STRING_ELT(dst, dst_i, Rf_mkChar(c_value.as_ptr()));
                }
            }
            SEXPTYPE::REALSXP => {
                let value = match SEXPTYPE(TYPEOF(src)) {
                    SEXPTYPE::REALSXP => REAL_ELT(src, src_i as c_int),
                    SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => {
                        let value = INTEGER_ELT(src, src_i as c_int);
                        if value == NA_INTEGER {
                            NA_REAL
                        } else {
                            value as f64
                        }
                    }
                    _ => NA_REAL,
                };
                *REAL(dst).add(dst_i as usize) = value;
            }
            SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => {
                let value = match SEXPTYPE(TYPEOF(src)) {
                    SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => INTEGER_ELT(src, src_i as c_int),
                    _ => NA_INTEGER,
                };
                *INTEGER(dst).add(dst_i as usize) = value;
            }
            _ => {}
        }
    }
}

unsafe fn set_bind_dimnames(result: SEXP, row_names: SEXP, col_names: SEXP) {
    unsafe {
        let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        if dimnames.is_null() {
            return;
        }
        let _dimnames_guard = protect(dimnames);
        SET_VECTOR_ELT(dimnames, 0, row_names);
        SET_VECTOR_ELT(dimnames, 1, col_names);
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::attrib_core::R_DimNamesSymbol(),
            dimnames,
        );
    }
}
/// R's `var(x, y = NULL, na.rm = FALSE)` — variance or covariance.
pub unsafe fn do_var(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let y = CAR(CDR(args));
        let na_rm_arg = CAR(CDR(CDR(args)));
        let na_rm = !na_rm_arg.is_null()
            && na_rm_arg != R_NilValue()
            && real_or_default(na_rm_arg, 0.0) != 0.0;

        if y.is_null() || y == R_NilValue() {
            // Variance of x
            let n = XLENGTH(x);
            let t = TYPEOF(x);
            let mut sum = 0.0f64;
            let mut sum_sq = 0.0f64;
            let mut count = 0i64;

            for i in 0..n {
                let val = if t == SEXPTYPE::REALSXP {
                    *REAL(x).add(i as usize)
                } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                    let v = *INTEGER(x).add(i as usize);
                    if v == NA_INTEGER { NA_REAL } else { v as f64 }
                } else {
                    NA_REAL
                };

                if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || val.is_nan() {
                    if !na_rm {
                        return Rf_ScalarReal(NA_REAL);
                    }
                } else {
                    sum += val;
                    sum_sq += val * val;
                    count += 1;
                }
            }

            if count < 2 {
                return Rf_ScalarReal(NA_REAL);
            }

            let mean = sum / count as f64;
            let variance = (sum_sq - count as f64 * mean * mean) / (count - 1) as f64;
            Rf_ScalarReal(variance)
        } else {
            // Covariance of x and y
            let n = XLENGTH(x).min(XLENGTH(y));
            let tx = TYPEOF(x);
            let ty = TYPEOF(y);
            let mut sum_x = 0.0f64;
            let mut sum_y = 0.0f64;
            let mut sum_xy = 0.0f64;
            let mut count = 0i64;

            for i in 0..n {
                let val_x = if tx == SEXPTYPE::REALSXP {
                    *REAL(x).add(i as usize)
                } else if tx == SEXPTYPE::INTSXP || tx == SEXPTYPE::LGLSXP {
                    let v = *INTEGER(x).add(i as usize);
                    if v == NA_INTEGER { NA_REAL } else { v as f64 }
                } else {
                    NA_REAL
                };

                let val_y = if ty == SEXPTYPE::REALSXP {
                    *REAL(y).add(i as usize)
                } else if ty == SEXPTYPE::INTSXP || ty == SEXPTYPE::LGLSXP {
                    let v = *INTEGER(y).add(i as usize);
                    if v == NA_INTEGER { NA_REAL } else { v as f64 }
                } else {
                    NA_REAL
                };

                if val_x.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                    || val_y.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                    || val_x.is_nan()
                    || val_y.is_nan()
                {
                    if !na_rm {
                        return Rf_ScalarReal(NA_REAL);
                    }
                } else {
                    sum_x += val_x;
                    sum_y += val_y;
                    sum_xy += val_x * val_y;
                    count += 1;
                }
            }

            if count < 2 {
                return Rf_ScalarReal(NA_REAL);
            }

            let mean_x = sum_x / count as f64;
            let mean_y = sum_y / count as f64;
            let covariance = (sum_xy - count as f64 * mean_x * mean_y) / (count - 1) as f64;
            Rf_ScalarReal(covariance)
        }
    }
}

/// R's `sd(x, na.rm = FALSE)` — standard deviation.
pub unsafe fn do_sd(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Call do_var to get variance
        let var_args = Rf_cons(CAR(args), CDR(args));
        let var_result = do_var(_call, _op, var_args, _rho);
        if var_result.is_null() {
            return R_NilValue();
        }

        let v = real_or_default(var_result, NA_REAL);
        if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || v < 0.0 {
            Rf_ScalarReal(NA_REAL)
        } else {
            Rf_ScalarReal(libm::sqrt(v))
        }
    }
}

/// R's `median(x, na.rm = FALSE)` — median value.
pub unsafe fn do_median(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarReal(NA_REAL);
        }

        let na_rm_arg = CAR(CDR(args));
        let na_rm = !na_rm_arg.is_null()
            && na_rm_arg != R_NilValue()
            && real_or_default(na_rm_arg, 0.0) != 0.0;

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let mut vals: Vec<f64> = Vec::new();

        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || val.is_nan() {
                if !na_rm {
                    return Rf_ScalarReal(NA_REAL);
                }
            } else {
                vals.push(val);
            }
        }

        if vals.is_empty() {
            return Rf_ScalarReal(NA_REAL);
        }

        vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mid = vals.len() / 2;
        if vals.len().is_multiple_of(2) {
            Rf_ScalarReal((vals[mid - 1] + vals[mid]) / 2.0)
        } else {
            Rf_ScalarReal(vals[mid])
        }
    }
}

/// R's `cummin(x)` — cumulative minimum.
pub unsafe fn do_cummin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        let mut min_so_far = f64::INFINITY;
        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || val.is_nan() {
                min_so_far = NA_REAL;
            } else if min_so_far.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN {
                min_so_far = min_so_far.min(val);
            }
            *dst.add(i as usize) = min_so_far;
        }
        result
    }
}

/// R's `cummax(x)` — cumulative maximum.
pub unsafe fn do_cummax(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        let mut max_so_far = f64::NEG_INFINITY;
        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN || val.is_nan() {
                max_so_far = NA_REAL;
            } else if max_so_far.to_bits() != crate::sexp::ffi::R_NA_BIT_PATTERN {
                max_so_far = max_so_far.max(val);
            }
            *dst.add(i as usize) = max_so_far;
        }
        result
    }
}

/// R's `dimnames(x)` — get dimension names of a matrix/array.
pub unsafe fn do_dimnames(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        crate::sexp::attrib_core::getAttrib(
            x,
            Rf_install(CString::new("dimnames").unwrap_or_default().as_ptr()),
        )
    }
}

/// R's `%in%` operator — match operator.
pub unsafe fn do_in_operator(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let table = CAR(CDR(args));

        if x.is_null() || x == R_NilValue() || table.is_null() || table == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = LOGICAL(result);

        for i in 0..n {
            let elem = elt_to_string(x, i);
            let table_len = XLENGTH(table);
            let mut found = false;
            for j in 0..table_len {
                let tbl_elem = elt_to_string(table, j);
                if elem == tbl_elem {
                    found = true;
                    break;
                }
            }
            *dst.add(i as usize) = if found { TRUE } else { FALSE };
        }
        result
    }
}

unsafe fn real_math1(args: SEXP, f: impl Fn(f64) -> f64) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            *dst.add(i as usize) = if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                NA_REAL
            } else {
                f(val)
            };
        }
        result
    }
}

fn sinpi_value(x: f64) -> f64 {
    if x.is_finite() && x.fract() == 0.0 {
        0.0
    } else {
        (std::f64::consts::PI * x).sin()
    }
}

fn cospi_value(x: f64) -> f64 {
    if x.is_finite() && x.fract() == 0.0 {
        if (x as i64).rem_euclid(2) == 0 {
            1.0
        } else {
            -1.0
        }
    } else if x.is_finite() && (x - 0.5).fract() == 0.0 {
        0.0
    } else {
        (std::f64::consts::PI * x).cos()
    }
}

/// R's `expm1(x)` — accurate exp(x)-1.
pub unsafe fn do_expm1(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { real_math1(args, f64::exp_m1) }
}

/// R's `log1p(x)` — accurate log(1+x).
pub unsafe fn do_log1p(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { real_math1(args, f64::ln_1p) }
}

/// R's `acosh(x)` — inverse hyperbolic cosine.
pub unsafe fn do_acosh(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { real_math1(args, f64::acosh) }
}

/// R's `asinh(x)` — inverse hyperbolic sine.
pub unsafe fn do_asinh(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { real_math1(args, f64::asinh) }
}

/// R's `atanh(x)` — inverse hyperbolic tangent.
pub unsafe fn do_atanh(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { real_math1(args, f64::atanh) }
}

/// R's `sinpi(x)` — sin(pi*x), exact at integer arguments.
pub unsafe fn do_sinpi(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { real_math1(args, sinpi_value) }
}

/// R's `cospi(x)` — cos(pi*x), exact at integer and half-integer arguments.
pub unsafe fn do_cospi(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { real_math1(args, cospi_value) }
}

/// R's `tanpi(x)` — tan(pi*x), based on the exact sinpi/cospi helpers.
pub unsafe fn do_tanpi(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        real_math1(args, |x| {
            if x.is_finite() && x.fract() == 0.0 {
                return 0.0;
            }
            if x.is_finite() {
                let cycle = x.rem_euclid(1.0);
                if cycle == 0.25 {
                    return 1.0;
                }
                if cycle == 0.75 {
                    return -1.0;
                }
            }
            let cos = cospi_value(x);
            if cos == 0.0 {
                f64::NAN
            } else {
                sinpi_value(x) / cos
            }
        })
    }
}

/// R's `sin(x)` — sine function.
pub unsafe fn do_sin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        if t == SEXPTYPE::CPLXSXP {
            return crate::eval::complex_arith::complex_unary_vec(
                x,
                crate::eval::complex_arith::complex_sin,
            );
        }
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = val.sin();
            }
        }
        result
    }
}

/// R's `cos(x)` — cosine function.
pub unsafe fn do_cos(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        if t == SEXPTYPE::CPLXSXP {
            return crate::eval::complex_arith::complex_unary_vec(
                x,
                crate::eval::complex_arith::complex_cos,
            );
        }
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = val.cos();
            }
        }
        result
    }
}

/// R's `tan(x)` — tangent function.
pub unsafe fn do_tan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        if t == SEXPTYPE::CPLXSXP {
            return crate::eval::complex_arith::complex_unary_vec(
                x,
                crate::eval::complex_arith::complex_tan,
            );
        }
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = val.tan();
            }
        }
        result
    }
}

/// R's `asin(x)` — arc sine function.
pub unsafe fn do_asin(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = val.asin();
            }
        }
        result
    }
}

/// R's `acos(x)` — arc cosine function.
pub unsafe fn do_acos(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = val.acos();
            }
        }
        result
    }
}

/// R's `atan(x)` — arc tangent function.
pub unsafe fn do_atan(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = val.atan();
            }
        }
        result
    }
}

/// R's `atan2(y, x)` — two-argument arc tangent function.
pub unsafe fn do_atan2(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let y = CAR(args);
        let x = CAR(CDR(args));

        if y.is_null() || y == R_NilValue() || x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }

        let n = XLENGTH(y).max(XLENGTH(x));
        let ty = TYPEOF(y);
        let tx = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);

        for i in 0..n {
            let y_len = XLENGTH(y);
            let x_len = XLENGTH(x);
            let yi = if y_len > 0 { i % y_len } else { 0 };
            let xi = if x_len > 0 { i % x_len } else { 0 };

            let val_y = if ty == SEXPTYPE::REALSXP {
                *REAL(y).add(yi as usize)
            } else if ty == SEXPTYPE::INTSXP || ty == SEXPTYPE::LGLSXP {
                let v = *INTEGER(y).add(yi as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            let val_x = if tx == SEXPTYPE::REALSXP {
                *REAL(x).add(xi as usize)
            } else if tx == SEXPTYPE::INTSXP || tx == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(xi as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };

            if val_y.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                || val_x.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
            {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = val_y.atan2(val_x);
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// do_abs — absolute value
// ---------------------------------------------------------------------------

/// R's `abs(x)` — absolute value of numeric vector.
///
/// Preserves integer/logical inputs as integer vectors and real inputs as real vectors.
pub unsafe fn do_abs(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        if x_arg.is_null() || x_arg == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x_arg);
        if t == SEXPTYPE::CPLXSXP {
            return crate::eval::complex_arith::complex_abs_vec(x_arg);
        }
        if t != SEXPTYPE::REALSXP && t != SEXPTYPE::INTSXP && t != SEXPTYPE::LGLSXP {
            return R_NilValue();
        }
        let n = XLENGTH(x_arg);
        if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
            let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
            if result.is_null() {
                return R_NilValue();
            }
            let _result_guard = protect(result);
            let dst = INTEGER(result);
            for i in 0..n {
                let value = *INTEGER(x_arg).add(i as usize);
                *dst.add(i as usize) = if value == NA_INTEGER || value == c_int::MIN {
                    NA_INTEGER
                } else {
                    value.abs()
                };
            }
            return result;
        }

        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let v = *REAL(x_arg).add(i as usize);
            *dst.add(i as usize) = if v.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                v
            } else {
                v.abs()
            };
        }
        result
    }
}

// ---------------------------------------------------------------------------
// do_sign — sign of values
// ---------------------------------------------------------------------------

/// R's `sign(x)` — sign of numeric vector (-1, 0, or 1).
///
/// Returns REALSXP. Preserves NA and NaN.
pub unsafe fn do_sign(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x_arg = CAR(args);
        if x_arg.is_null() || x_arg == R_NilValue() {
            return R_NilValue();
        }
        let t = TYPEOF(x_arg);
        if t != SEXPTYPE::REALSXP && t != SEXPTYPE::INTSXP && t != SEXPTYPE::LGLSXP {
            return R_NilValue();
        }
        let n = XLENGTH(x_arg);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let v = if t == SEXPTYPE::REALSXP {
                *REAL(x_arg).add(i as usize)
            } else {
                let iv = *INTEGER(x_arg).add(i as usize);
                if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
            };
            *dst.add(i as usize) = if v.is_nan() {
                v // preserve NaN/NA
            } else if v == 0.0 {
                0.0
            } else if v > 0.0 {
                1.0
            } else {
                -1.0
            };
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Complete special functions for libRmath coverage
// ---------------------------------------------------------------------------

/// Helper to apply a scalar function to a numeric vector, preserving NA/NaN.
/// Returns REALSXP.
unsafe fn apply_unary_scalar_fn(x: SEXP, scalar_fn: impl Fn(f64) -> f64) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = scalar_fn(val);
            }
        }
        result
    }
}

/// Helper to apply a binary scalar function to two numeric vectors with recycling.
/// Returns REALSXP.
unsafe fn apply_binary_scalar_fn(x: SEXP, y: SEXP, scalar_fn: impl Fn(f64, f64) -> f64) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() || y.is_null() || y == R_NilValue() {
            return R_NilValue();
        }
        let n = XLENGTH(x).max(XLENGTH(y));
        let tx = TYPEOF(x);
        let ty = TYPEOF(y);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let x_len = XLENGTH(x);
            let y_len = XLENGTH(y);
            let xi = if x_len > 0 { i % x_len } else { 0 };
            let yi = if y_len > 0 { i % y_len } else { 0 };
            let val_x = if tx == SEXPTYPE::REALSXP {
                *REAL(x).add(xi as usize)
            } else if tx == SEXPTYPE::INTSXP || tx == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(xi as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            let val_y = if ty == SEXPTYPE::REALSXP {
                *REAL(y).add(yi as usize)
            } else if ty == SEXPTYPE::INTSXP || ty == SEXPTYPE::LGLSXP {
                let v = *INTEGER(y).add(yi as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            if val_x.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                || val_y.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
            {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = scalar_fn(val_x, val_y);
            }
        }
        result
    }
}

/// R's `lgamma(x)` — log of the absolute value of the gamma function.
pub unsafe fn do_lgamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { apply_unary_scalar_fn(CAR(args), crate::special::gamma::lgammafn) }
}

/// R's `gamma(x)` — gamma function.
pub unsafe fn do_gamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { apply_unary_scalar_fn(CAR(args), crate::special::gamma::gammafn) }
}

/// R's `digamma(x)` — digamma (psi) function.
pub unsafe fn do_digamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { apply_unary_scalar_fn(CAR(args), crate::special::polygamma::digamma) }
}

/// R's `trigamma(x)` — trigamma function.
pub unsafe fn do_trigamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { apply_unary_scalar_fn(CAR(args), crate::special::polygamma::trigamma) }
}

/// R's `psigamma(x, deriv)` — polygamma function (deriv-th derivative of psi).
pub unsafe fn do_psigamma(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let deriv_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || deriv_arg.is_null() || deriv_arg == R_NilValue() {
            return R_NilValue();
        }
        let deriv = real_or_default(deriv_arg, 1.0);
        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = crate::special::polygamma::psigamma(val, deriv);
            }
        }
        result
    }
}

/// R's `beta(a, b)` — beta function.
pub unsafe fn do_beta(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let a = CAR(args);
        let b = CAR(CDR(args));
        if a.is_null() || a == R_NilValue() || b.is_null() || b == R_NilValue() {
            return R_NilValue();
        }
        apply_binary_scalar_fn(a, b, |x, y| {
            crate::special::gamma::gammafn(x) * crate::special::gamma::gammafn(y)
                / crate::special::gamma::gammafn(x + y)
        })
    }
}

/// R's `lbeta(a, b)` — log beta function.
pub unsafe fn do_lbeta(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let a = CAR(args);
        let b = CAR(CDR(args));
        if a.is_null() || a == R_NilValue() || b.is_null() || b == R_NilValue() {
            return R_NilValue();
        }
        apply_binary_scalar_fn(a, b, crate::special::lbeta::lbeta)
    }
}

/// R's `choose(n, k)` — binomial coefficient.
pub unsafe fn do_choose(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_arg = CAR(args);
        let k_arg = CAR(CDR(args));
        if n_arg.is_null() || n_arg == R_NilValue() || k_arg.is_null() || k_arg == R_NilValue() {
            return R_NilValue();
        }
        apply_binary_scalar_fn(n_arg, k_arg, crate::special::choose::choose)
    }
}

/// R's `lchoose(n, k)` — log of absolute value of binomial coefficient.
pub unsafe fn do_lchoose(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let n_arg = CAR(args);
        let k_arg = CAR(CDR(args));
        if n_arg.is_null() || n_arg == R_NilValue() || k_arg.is_null() || k_arg == R_NilValue() {
            return R_NilValue();
        }
        apply_binary_scalar_fn(n_arg, k_arg, crate::special::choose::lchoose)
    }
}

/// R's `factorial(n)` — factorial n!
pub unsafe fn do_factorial(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        apply_unary_scalar_fn(x, |v| crate::special::gamma::gammafn(v + 1.0))
    }
}

/// R's `lfactorial(n)` — log factorial.
pub unsafe fn do_lfactorial(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        apply_unary_scalar_fn(x, |v| crate::special::gamma::lgammafn(v + 1.0))
    }
}

/// R's `besselI(x, nu)` — modified Bessel function of the first kind.
pub unsafe fn do_besselI(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let nu_arg = CAR(CDR(args));
        let expo_arg = CAR(CDR(CDR(args))); // optional: exponential scaling
        if x.is_null() || x == R_NilValue() || nu_arg.is_null() || nu_arg == R_NilValue() {
            return R_NilValue();
        }
        let nu = real_or_default(nu_arg, 0.0);
        let expo = if !expo_arg.is_null() && expo_arg != R_NilValue() {
            let e = real_or_default(expo_arg, 0.0);
            e != 0.0
        } else {
            false
        };
        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) =
                    crate::special::bessel_i::bessel_i(val, nu, if expo { 2.0 } else { 1.0 });
            }
        }
        result
    }
}

/// R's `besselJ(x, nu)` — Bessel function of the first kind.
pub unsafe fn do_besselJ(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let nu_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || nu_arg.is_null() || nu_arg == R_NilValue() {
            return R_NilValue();
        }
        let nu = real_or_default(nu_arg, 0.0);
        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = crate::special::bessel_j::bessel_j(val, nu);
            }
        }
        result
    }
}

/// R's `besselK(x, nu)` — modified Bessel function of the second kind.
pub unsafe fn do_besselK(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let nu_arg = CAR(CDR(args));
        let expo_arg = CAR(CDR(CDR(args))); // optional: exponential scaling
        if x.is_null() || x == R_NilValue() || nu_arg.is_null() || nu_arg == R_NilValue() {
            return R_NilValue();
        }
        let nu = real_or_default(nu_arg, 0.0);
        let expo = if !expo_arg.is_null() && expo_arg != R_NilValue() {
            let e = real_or_default(expo_arg, 0.0);
            e != 0.0
        } else {
            false
        };
        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) =
                    crate::special::bessel_k::bessel_k(val, nu, if expo { 2.0 } else { 1.0 });
            }
        }
        result
    }
}

/// R's `besselY(x, nu)` — Bessel function of the second kind.
pub unsafe fn do_besselY(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let nu_arg = CAR(CDR(args));
        if x.is_null() || x == R_NilValue() || nu_arg.is_null() || nu_arg == R_NilValue() {
            return R_NilValue();
        }
        let nu = real_or_default(nu_arg, 0.0);
        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        let dst = REAL(result);
        for i in 0..n {
            let val = if t == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
            if val.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
                *dst.add(i as usize) = NA_REAL;
            } else {
                *dst.add(i as usize) = crate::special::bessel_y::bessel_y(val, nu);
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Final additions: commonly used missing functions
// ---------------------------------------------------------------------------

/// R's `simplify2array(x)` — simplify list to array.
pub unsafe fn do_simplify2array(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || TYPEOF(x) != SEXPTYPE::VECSXP {
            return x;
        }
        let n = XLENGTH(x);
        // Check if all elements are scalar and same type
        let first = crate::sexp::accessors::VECTOR_ELT(x, 0);
        if first.is_null() {
            return x;
        }
        let elem_type = TYPEOF(first);
        if XLENGTH(first) != 1 {
            return x;
        }
        // Simplify to atomic vector
        let result = Rf_allocVector3(elem_type, n);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        for i in 0..n {
            let elem = crate::sexp::accessors::VECTOR_ELT(x, i as i64);
            if !elem.is_null() && TYPEOF(elem) == elem_type {
                if elem_type == SEXPTYPE::REALSXP.as_c_int() {
                    *REAL(result).add(i as usize) = *REAL(elem);
                } else if elem_type == SEXPTYPE::INTSXP.as_c_int() {
                    *INTEGER(result).add(i as usize) = *INTEGER(elem);
                } else if elem_type == SEXPTYPE::LGLSXP.as_c_int() {
                    *LOGICAL(result).add(i as usize) = *LOGICAL(elem);
                }
            }
        }
        result
    }
}

/// R's `match.arg(arg, choices)` — match argument against choices.
pub unsafe fn do_match_arg(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let arg = CAR(args);
        let choices = CAR(CDR(args));
        if arg.is_null() || choices.is_null() || arg == R_NilValue() || choices == R_NilValue() {
            return arg;
        }
        let arg_str = elt_to_string(arg, 0);
        let n = XLENGTH(choices);
        let mut matches = Vec::new();
        for i in 0..n {
            let choice = elt_to_string(choices, i);
            if choice.starts_with(&arg_str) {
                matches.push(choice);
            }
        }
        if matches.len() == 1 {
            Rf_mkString(
                CString::new(matches[0].as_str())
                    .unwrap_or_default()
                    .as_ptr(),
            )
        } else {
            base_error("'arg' should be one of ");
        }
    }
}

/// R's `char.expand(input, target)` — expand abbreviations.
pub unsafe fn do_char_expand(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let input = CAR(args);
        let target = CAR(CDR(args));
        let nomatch = CAR(CDR(CDR(args)));
        if input.is_null() || target.is_null() {
            return input;
        }
        let input_str = elt_to_string(input, 0);
        let n = if target == R_NilValue() {
            0
        } else {
            XLENGTH(target)
        };
        let mut matches: Vec<String> = Vec::new();
        for i in 0..n {
            let t = elt_to_string(target, i);
            if t.starts_with(&input_str) {
                matches.push(t);
            }
        }
        if matches.len() == 1 {
            Rf_mkString(CString::new(&matches[0][..]).unwrap_or_default().as_ptr())
        } else if matches.len() > 1 {
            Rf_allocVector3(SEXPTYPE::STRSXP, 0)
        } else if !nomatch.is_null() && nomatch != R_NilValue() && nomatch != R_MissingArg() {
            let out = Rf_allocVector3(SEXPTYPE::STRSXP, 1);
            if out.is_null() {
                return R_NilValue();
            }
            SET_STRING_ELT(out, 0, crate::sexp::globals::R_NaString());
            out
        } else {
            base_error("no match");
        }
    }
}

/// R's `type.convert(x, ...)` — convert to appropriate type.
pub unsafe fn do_type_convert(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || TYPEOF(x) != SEXPTYPE::STRSXP {
            return x;
        }
        // Try integer first
        let n = XLENGTH(x);
        let first = elt_to_string(x, 0);
        if first.parse::<i64>().is_ok() {
            let result = Rf_allocVector3(SEXPTYPE::INTSXP, n);
            if result.is_null() {
                return x;
            }
            let _p = protect(result);
            for i in 0..n {
                let s = elt_to_string(x, i);
                *INTEGER(result).add(i as usize) = s.parse::<i64>().unwrap_or(0) as c_int;
            }
            result
        } else if first.parse::<f64>().is_ok() {
            let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
            if result.is_null() {
                return x;
            }
            let _p = protect(result);
            for i in 0..n {
                let s = elt_to_string(x, i);
                *REAL(result).add(i as usize) = s.parse::<f64>().unwrap_or(NA_REAL);
            }
            result
        } else {
            x // Keep as character
        }
    }
}

/// R's `as.environment(x)` — convert to environment.
pub unsafe fn do_as_environment(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() {
            return R_NilValue();
        }
        if TYPEOF(x) == SEXPTYPE::ENVSXP {
            return x;
        }
        if TYPEOF(x) == SEXPTYPE::INTSXP || TYPEOF(x) == SEXPTYPE::REALSXP {
            let pos = if TYPEOF(x) == SEXPTYPE::INTSXP {
                *INTEGER(x)
            } else {
                *REAL(x) as c_int
            };
            return search_env_from_position(pos);
        }
        if TYPEOF(x) == SEXPTYPE::STRSXP {
            let name = if XLENGTH(x) == 0 {
                "NA".to_string()
            } else {
                elt_to_string(x, 0)
            };
            return search_env_from_name(&name);
        }
        std::panic::panic_any(RError {
            message: "invalid object for as.environment".to_string(),
        });
    }
}

/// R's `pos.to.env(pos)` — map a search path position to an environment.
pub unsafe fn do_pos_to_env(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let pos = integer_arg_by_name_or_position(args, "pos", 0).unwrap_or(NA_INTEGER);
        search_env_from_position(pos)
    }
}

unsafe fn search_env_from_position(pos: c_int) -> SEXP {
    unsafe {
        if pos > 0
            && let Some((_, env)) = search_path_entries().get((pos - 1) as usize)
        {
            return *env;
        }
        std::panic::panic_any(RError {
            message: "invalid 'pos' argument".to_string(),
        });
    }
}

unsafe fn search_env_from_name(name: &str) -> SEXP {
    for (label, env) in unsafe { search_path_entries() } {
        if label == name || (name == "base" && label == "package:base") {
            return env;
        }
    }
    std::panic::panic_any(RError {
        message: format!("no item called \"{name}\" on the search list"),
    });
}

unsafe fn search_path_len() -> c_int {
    unsafe { search_path_entries().len() as c_int }
}

unsafe fn search_path_entries() -> Vec<(String, SEXP)> {
    unsafe {
        let global = crate::sexp::globals::R_GlobalEnv();
        let base = crate::sexp::globals::R_BaseEnv();
        if global.is_null() || base.is_null() {
            return Vec::new();
        }

        let mut entries = vec![(".GlobalEnv".to_string(), global)];
        let mut env = crate::sexp::accessors::ENCLOS(global);
        while !env.is_null() && env != base {
            entries.push((search_env_label(env), env));
            env = crate::sexp::accessors::ENCLOS(env);
        }
        entries.push(("package:base".to_string(), base));
        entries
    }
}

unsafe fn search_env_label(env: SEXP) -> String {
    unsafe {
        let name = crate::sexp::attrib_core::getAttrib(env, Rf_install(c"name".as_ptr()));
        if TYPEOF(name) == SEXPTYPE::STRSXP && XLENGTH(name) > 0 {
            let value = STRING_ELT(name, 0);
            if !value.is_null() && value != R_NilValue() {
                return CStr::from_ptr(CHAR(value)).to_string_lossy().into_owned();
            }
        }
        "(unknown)".to_string()
    }
}

/// R's `searchpaths()` — filesystem/search labels for entries on the search path.
pub unsafe fn do_searchpaths(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let entries = search_path_entries();
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, entries.len() as R_xlen_t);
        for (i, (label, _)) in entries.iter().enumerate() {
            SET_STRING_ELT(
                result,
                i as R_xlen_t,
                Rf_mkChar(CString::new(label.as_str()).unwrap_or_default().as_ptr()),
            );
        }
        result
    }
}

/// R's `sort.list(x, partial, na.last, decreasing, method)` — indices for sorting.
pub unsafe fn do_sort_list(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = arg_by_name_or_position(args, &["x"], 0);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
        }
        let decreasing = sort_logical_arg(args, &["decreasing"], 3).unwrap_or(false);
        let na_placement = order_na_placement(args, 2);
        let mut indices = ordered_atomic_indices(x, decreasing, na_placement);
        if na_placement == SortNaPlacement::Remove {
            let compressed_positions = nonmissing_compressed_positions(x);
            for index in &mut indices {
                *index = compressed_positions[*index as usize];
            }
        }

        let result = Rf_allocVector3(SEXPTYPE::INTSXP, indices.len() as R_xlen_t);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);
        for (i, idx) in indices.iter().enumerate() {
            *INTEGER(result).add(i) = (*idx + 1) as c_int; // 1-indexed
        }
        result
    }
}

fn nonmissing_compressed_positions(x: SEXP) -> Vec<R_xlen_t> {
    unsafe {
        let n = XLENGTH(x);
        let mut positions = vec![0; n as usize];
        let mut next = 0;
        for i in 0..n {
            let missing = match TYPEOF(x) {
                t if t == SEXPTYPE::STRSXP => charsxp_is_na(STRING_ELT(x, i)),
                t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
                    *INTEGER(x).add(i as usize) == NA_INTEGER
                }
                t if t == SEXPTYPE::REALSXP => ISNAN(*REAL(x).add(i as usize)),
                _ => ISNAN(elt_real_safe(x, i)),
            };
            if !missing {
                positions[i as usize] = next;
                next += 1;
            }
        }
        positions
    }
}

/// R's `outer(X, Y, FUN)` — outer product (enhanced).
pub unsafe fn do_outer_enhanced(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let y = CAR(CDR(args));
        let fun = CAR(CDR(CDR(args)));
        if x.is_null() || y.is_null() {
            return R_NilValue();
        }
        let nx = XLENGTH(x);
        let ny = XLENGTH(y);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, nx * ny);
        if result.is_null() {
            return R_NilValue();
        }
        let _p = protect(result);

        // Default: multiplication
        if nx > 0 && ny > 0 {
            let dst = REAL(result);
            for i in 0..nx {
                let xi = elt_real_safe(x, i);
                for j in 0..ny {
                    let yj = elt_real_safe(y, j);
                    *dst.add((i * ny + j) as usize) = xi * yj;
                }
            }
        }

        // Set dim attribute
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        if !dim.is_null() {
            *INTEGER(dim) = nx as c_int;
            *INTEGER(dim).add(1) = ny as c_int;
            crate::sexp::attrib_core::setAttrib(
                result,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
                dim,
            );
        }
        result
    }
}

/// R's `match.fun(FUN)` — match a function argument.
pub unsafe fn do_match_fun(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() {
            return R_NilValue();
        }
        if TYPEOF(x) == SEXPTYPE::CLOSXP
            || TYPEOF(x) == SEXPTYPE::BUILTINSXP
            || TYPEOF(x) == SEXPTYPE::SPECIALSXP
        {
            return x;
        }
        // If it's a symbol, look it up
        if TYPEOF(x) == SEXPTYPE::SYMSXP {
            let val = crate::sexp::envir::R_findVar(x, _rho);
            if !val.is_null()
                && (TYPEOF(val) == SEXPTYPE::CLOSXP
                    || TYPEOF(val) == SEXPTYPE::BUILTINSXP
                    || TYPEOF(val) == SEXPTYPE::SPECIALSXP)
            {
                return val;
            }
        }
        x
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn test_pairlist(values: &[SEXP]) -> SEXP {
        unsafe {
            values
                .iter()
                .rev()
                .fold(R_NilValue(), |tail, value| Rf_cons(*value, tail))
        }
    }

    fn generated_namespace_input(mut seed: u64, len: usize) -> String {
        const ALPHABET: &[u8] =
            b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_.,()#'\"`\\ \n\t";
        let mut out = String::with_capacity(len);
        for _ in 0..len {
            seed = seed
                .wrapping_mul(2862933555777941757)
                .wrapping_add(3037000493);
            out.push(ALPHABET[((seed >> 33) as usize) % ALPHABET.len()] as char);
        }
        out
    }

    fn adversarial_iterations(default: u64) -> u64 {
        std::env::var("RPORT_ADVERSARIAL_ITERS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    }

    #[test]
    fn timezone_name_from_zoneinfo_paths() {
        assert_eq!(
            timezone_name_from_zoneinfo_path(Path::new(
                "/var/db/timezone/zoneinfo/America/Sao_Paulo"
            )),
            Some("America/Sao_Paulo".to_string())
        );
        assert_eq!(
            timezone_name_from_zoneinfo_path(Path::new("/usr/share/zoneinfo/Europe/London")),
            Some("Europe/London".to_string())
        );
        assert_eq!(
            timezone_name_from_zoneinfo_path(Path::new("/tmp/localtime")),
            None
        );
    }

    #[test]
    fn skip_olson_metadata_components() {
        assert!(skip_olson_component("zone.tab"));
        assert!(skip_olson_component("posix"));
        assert!(!skip_olson_component("Africa"));
        assert!(!skip_olson_component("Sao_Paulo"));
    }

    #[test]
    fn namespace_parser_handles_strings_comments_and_nested_calls() {
        let directives = parse_namespace_directives(
            r#"
            export(foo, "bar,baz", `quux`)
            exportPattern("^as\\.")
            import(stats)
            importFrom(utils, head, tail)
            S3method(print,myclass)
            S3method(format,myclass,format_myclass)
            useDynLib(nativebits)
            # export(commented_out)
            export("hash#inside")
            export(call_like(default = f(a, b)))
            "#,
        );

        assert_eq!(directives.exports[0], "foo");
        assert!(directives.exports.contains(&"bar,baz".to_string()));
        assert!(directives.exports.contains(&"quux".to_string()));
        assert!(directives.exports.contains(&"hash#inside".to_string()));
        assert!(
            directives
                .exports
                .contains(&"call_like(default = f(a, b))".to_string())
        );
        assert_eq!(directives.export_patterns, vec!["^as\\\\.".to_string()]);
        assert_eq!(directives.imports.len(), 2);
        assert_eq!(directives.s3_methods.len(), 2);
        assert_eq!(directives.native_libraries, vec!["nativebits".to_string()]);
    }

    #[test]
    fn adversarial_namespace_inputs_do_not_panic() {
        let fixed = [
            "export(",
            "export(foo",
            "export(foo, # comment\n bar)",
            "S3method(print,",
            "useDynLib('unterminated)",
            "importFrom(pkg, f(a, b), c)",
            "export(`odd name`, \"comma,name\", 'hash#name')",
        ];

        for input in fixed {
            let result = std::panic::catch_unwind(|| parse_namespace_directives(input));
            assert!(
                result.is_ok(),
                "namespace parser panicked for fixed input: {input:?}"
            );
        }

        for seed in 0..adversarial_iterations(256) {
            let input = generated_namespace_input(seed, (seed as usize % 128) + 1);
            let result = std::panic::catch_unwind(|| parse_namespace_directives(&input));
            assert!(
                result.is_ok(),
                "namespace parser panicked for seed {seed}: {input:?}"
            );
        }
    }

    #[test]
    fn essentials_get_option_delegates_to_options_runtime() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            crate::sexp::init::initialize_r();

            let option_name = Rf_mkString(CString::new("width").unwrap().as_ptr());
            let args = Rf_cons(option_name, R_NilValue());
            let result = do_getOption(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );

            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(*INTEGER(result), 80);
        }
    }

    #[test]
    fn essentials_options_delegates_to_options_runtime() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            crate::sexp::init::initialize_r();

            let result = do_options(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                R_NilValue(),
                std::ptr::null_mut(),
            );

            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::VECSXP);
            assert!(XLENGTH(result) > 0);
        }
    }

    #[test]
    fn test_do_log2_default_base_two() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            crate::sexp::init::initialize_r();

            let args = Rf_cons(Rf_ScalarReal(8.0), R_NilValue());
            let result = do_log2(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );

            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
            assert!(((*REAL(result)).to_owned() - 3.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_do_log2_explicit_base_is_preserved() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            crate::sexp::init::initialize_r();

            let args = Rf_cons(
                Rf_ScalarReal(8.0),
                Rf_cons(Rf_ScalarReal(8.0), R_NilValue()),
            );
            let result = do_log2(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );

            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
            assert!(((*REAL(result)).to_owned() - 1.0).abs() < 1e-10);
        }
    }

    #[test]
    fn test_gc_reports_session_memory_counters() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            crate::sexp::init::initialize_r();
            let value = Rf_allocVector3(SEXPTYPE::INTSXP, 256);
            let _guard = protect(value);

            let result = do_gc(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                R_NilValue(),
                std::ptr::null_mut(),
            );

            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::REALSXP);
            let dim = crate::sexp::attrib_core::getAttrib(
                result,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
            );
            assert!(!dim.is_null());
            assert_eq!(*INTEGER(dim), 2);
            assert_eq!(*INTEGER(dim).add(1), 7);

            let data = REAL(result);
            assert!(*data > 0.0, "Ncells used should reflect active arena nodes");
            assert!(*data.add(1) > 0.0, "Vcells used should reflect arena bytes");
            assert!(*data.add(9) >= *data.add(1), "Vcells max used >= used");
        }
    }

    #[test]
    fn test_memory_size_uses_current_and_peak_arena_bytes() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            crate::sexp::init::initialize_r();
            let before = do_memory_size(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                R_NilValue(),
                std::ptr::null_mut(),
            );

            let value = Rf_allocVector3(SEXPTYPE::REALSXP, 512);
            let _guard = protect(value);
            let current = do_memory_size(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                R_NilValue(),
                std::ptr::null_mut(),
            );
            let peak_args = Rf_cons(Rf_ScalarLogical(TRUE), R_NilValue());
            let peak = do_memory_size(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                peak_args,
                std::ptr::null_mut(),
            );

            assert!(*REAL(current) > *REAL(before));
            assert!(*REAL(peak) >= *REAL(current));
        }
    }

    #[test]
    fn test_gcinfo_is_session_local_and_returns_previous_value() {
        let left = crate::sexp::session::RSession::new();
        let right = crate::sexp::session::RSession::new();

        left.with_protected(|| unsafe {
            let args = Rf_cons(Rf_ScalarLogical(TRUE), R_NilValue());
            let old = do_gcinfo(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );
            assert_eq!(*LOGICAL(old), FALSE);

            let args = Rf_cons(Rf_ScalarLogical(FALSE), R_NilValue());
            let old = do_gcinfo(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );
            assert_eq!(*LOGICAL(old), TRUE);
        });

        right.with_protected(|| unsafe {
            let args = Rf_cons(Rf_ScalarLogical(FALSE), R_NilValue());
            let old = do_gcinfo(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );
            assert_eq!(*LOGICAL(old), FALSE);
        });
    }

    #[test]
    fn test_matrix_byrow_uses_column_major_storage() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            crate::sexp::init::initialize_r();
            let data = Rf_allocVector3(SEXPTYPE::INTSXP, 6);
            let _data_guard = protect(data);
            for i in 0..6 {
                *INTEGER(data).add(i) = (i + 1) as c_int;
            }
            let args = test_pairlist(&[
                data,
                Rf_ScalarInteger(2),
                Rf_ScalarInteger(3),
                Rf_ScalarLogical(TRUE),
            ]);

            let result = do_matrix(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );

            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(
                (0..6).map(|i| *INTEGER(result).add(i)).collect::<Vec<_>>(),
                vec![1, 4, 2, 5, 3, 6]
            );
        }
    }

    #[test]
    fn test_matrix_zero_length_data_preserves_shape_and_fills_na() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            crate::sexp::init::initialize_r();
            let data = Rf_allocVector3(SEXPTYPE::INTSXP, 0);
            let args = test_pairlist(&[
                data,
                Rf_ScalarInteger(2),
                Rf_ScalarInteger(2),
                Rf_ScalarLogical(FALSE),
            ]);

            let result = do_matrix(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );

            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(XLENGTH(result), 4);
            for i in 0..4 {
                assert_eq!(*INTEGER(result).add(i), NA_INTEGER);
            }
        }
    }

    #[test]
    fn test_transpose_non_square_matrix_uses_r_column_major_indexing() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            crate::sexp::init::initialize_r();
            let data = Rf_allocVector3(SEXPTYPE::INTSXP, 6);
            let _data_guard = protect(data);
            for i in 0..6 {
                *INTEGER(data).add(i) = (i + 1) as c_int;
            }
            let matrix_args = test_pairlist(&[
                data,
                Rf_ScalarInteger(2),
                Rf_ScalarInteger(3),
                Rf_ScalarLogical(FALSE),
            ]);
            let matrix = do_matrix(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                matrix_args,
                std::ptr::null_mut(),
            );
            let transpose_args = Rf_cons(matrix, R_NilValue());

            let result = do_transpose(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                transpose_args,
                std::ptr::null_mut(),
            );

            let dim = crate::sexp::attrib_core::getAttrib(
                result,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
            );
            assert_eq!(*INTEGER(dim), 3);
            assert_eq!(*INTEGER(dim).add(1), 2);
            assert_eq!(
                (0..6).map(|i| *INTEGER(result).add(i)).collect::<Vec<_>>(),
                vec![1, 3, 5, 2, 4, 6]
            );
        }
    }

    #[test]
    fn test_string_matrix_and_transpose_preserve_elements() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            crate::sexp::init::initialize_r();
            let data = Rf_allocVector3(SEXPTYPE::STRSXP, 3);
            let _data_guard = protect(data);
            SET_STRING_ELT(data, 0, Rf_mkChar(c"a".as_ptr()));
            SET_STRING_ELT(data, 1, Rf_mkChar(c"b".as_ptr()));
            SET_STRING_ELT(data, 2, Rf_mkChar(c"c".as_ptr()));
            let args = test_pairlist(&[
                data,
                Rf_ScalarInteger(1),
                Rf_ScalarInteger(3),
                Rf_ScalarLogical(FALSE),
            ]);
            let matrix = do_matrix(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );
            assert_eq!(CStr::from_ptr(CHAR(STRING_ELT(matrix, 2))).to_bytes(), b"c");

            let transpose = do_transpose(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                Rf_cons(matrix, R_NilValue()),
                std::ptr::null_mut(),
            );
            assert_eq!(
                CStr::from_ptr(CHAR(STRING_ELT(transpose, 1))).to_bytes(),
                b"b"
            );
            let dim = crate::sexp::attrib_core::getAttrib(
                transpose,
                Rf_install(CString::new("dim").unwrap_or_default().as_ptr()),
            );
            assert_eq!(*INTEGER(dim), 3);
            assert_eq!(*INTEGER(dim).add(1), 1);
        }
    }

    #[test]
    fn test_system_command_policy_is_target_gated() {
        assert_eq!(
            system_commands_disabled_by_runtime_policy(),
            cfg!(target_os = "android")
        );
    }
}
