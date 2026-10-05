//! Essential R built-in functions — c(), seq(), rep(), paste(), cat(), typeof(), is.na(), names().
//!
//! These are the most fundamental R functions that every R program uses.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CStr, CString};
use std::os::raw::c_int;
use std::path::{Path, PathBuf};

#[allow(unused_imports)]
use crate::sexp::accessors::{
    ATTRIB, CADR, CAR, CDR, CHAR, COMPLEX, FORMALS, FRAME, HASHTAB, INTEGER, INTEGER_ELT, LENGTH,
    LOGICAL, LOGICAL_ELT, PRINTNAME, RAW, REAL, REAL_ELT, SET_ENCLOS, SET_OBJECT, SET_STRING_ELT,
    SET_VECTOR_ELT, SETCAR, SETCDR, SETTAG, STRING_ELT, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
};
#[allow(unused_imports)]
use crate::sexp::constructors::{
    Rf_ScalarInteger, Rf_ScalarLogical, Rf_ScalarReal, Rf_allocVector3, Rf_cons, Rf_lang2,
    Rf_lang3, Rf_mkChar, Rf_mkString,
};

use crate::sexp::context::RError;
use crate::sexp::ffi::{
    ISNAN, NA_INTEGER, NA_LOGICAL, NA_REAL, R_NA_BIT_PATTERN, R_xlen_t, SEXP, SEXPTYPE, TRUE,
};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use super::*;

mod namespace_conditions;

thread_local! {
    static CACHING_ATTACHED_S4: Cell<bool> = const { Cell::new(false) };
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum DatetimeVectorClass {
    Date,
    Posixct,
}

pub(crate) unsafe fn leading_datetime_class(args: SEXP) -> Option<(DatetimeVectorClass, SEXP)> {
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

pub(crate) unsafe fn posixct_tzone_string(source: SEXP) -> String {
    unsafe {
        let tzone = crate::sexp::attrib_core::getAttrib(source, Rf_install(c"tzone".as_ptr()));
        if tzone.is_null() || tzone == R_NilValue() || TYPEOF(tzone) != SEXPTYPE::STRSXP {
            return String::new();
        }
        if XLENGTH(tzone) == 0 {
            return String::new();
        }
        let value = STRING_ELT(tzone, 0);
        if value.is_null() || value == crate::sexp::globals::R_NaString() {
            return String::new();
        }
        CStr::from_ptr(CHAR(value))
            .to_str()
            .unwrap_or("")
            .to_string()
    }
}

pub(crate) unsafe fn set_datetime_class_from(
    result: SEXP,
    source: SEXP,
    class: DatetimeVectorClass,
) {
    unsafe {
        match class {
            DatetimeVectorClass::Date => set_single_class(result, "Date"),
            DatetimeVectorClass::Posixct => {
                set_posixct_class(result, &posixct_tzone_string(source))
            }
        }
    }
}

pub(crate) unsafe fn integer_or_logical_elt(value: SEXP, index: c_int) -> c_int {
    unsafe {
        match TYPEOF(value) {
            t if t == SEXPTYPE::INTSXP => INTEGER_ELT(value, index),
            t if t == SEXPTYPE::LGLSXP => LOGICAL_ELT(value, index),
            _ => NA_INTEGER,
        }
    }
}

pub unsafe fn do_cache_class(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::seq::check1arg(args, call, c"class".as_ptr());
        if args.is_null() || args == R_NilValue() || CDR(args) == R_NilValue() {
            std::panic::panic_any(RError {
                message: "invalid class argument to internal .class_cache".to_string(),
            });
        }
        let class = CAR(args);
        if class.is_null()
            || class == R_NilValue()
            || TYPEOF(class) != SEXPTYPE::STRSXP
            || XLENGTH(class) < 1
        {
            std::panic::panic_any(RError {
                message: "invalid class argument to internal .class_cache".to_string(),
            });
        }
        let class_chars = STRING_ELT(class, 0);
        if class_chars.is_null() {
            std::panic::panic_any(RError {
                message: "invalid class argument to internal .class_cache".to_string(),
            });
        }
        let name = std::ffi::CStr::from_ptr(CHAR(class_chars))
            .to_string_lossy()
            .into_owned();
        crate::mainutils::objects::cache_class(&name, CADR(args))
    }
}

pub unsafe fn do_xtfrm(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            return R_NilValue();
        }
        let mut ans = R_NilValue();
        if crate::eval::dispatch::DispatchOrEval(
            call,
            op,
            c"xtfrm".as_ptr(),
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

        if sexp_has_class(x, "Date") || sexp_has_class(x, "POSIXct") || sexp_has_class(x, "POSIXt")
        {
            return do_xtfrm_Date(call, op, args, rho);
        }
        // GNU sort.c do_xtfrm: DispatchOrEval miss → xtfrm.default.
        let fn_ = crate::sexp::envir::findFun(Rf_install(c"xtfrm.default".as_ptr()), rho);
        if !fn_.is_null()
            && fn_ != crate::sexp::globals::R_UnboundValue()
            && (TYPEOF(fn_) == SEXPTYPE::CLOSXP
                || TYPEOF(fn_) == SEXPTYPE::BUILTINSXP
                || TYPEOF(fn_) == SEXPTYPE::SPECIALSXP)
        {
            let expr = Rf_lang2(fn_, x);
            let _expr = protect(expr);
            return crate::eval::eval::Rf_eval(expr, rho);
        }
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

/// The `@` operator (main/attrib.c `do_AT`).
///
/// Upstream dispatches an internal generic for S3 objects first; with no
/// `@` method registered (the only case in this port) it falls through to
/// the stock checks: only S4 objects (or `.Data`) may be slot-extracted,
/// otherwise "no applicable method for `@` ..." with the object's class.
pub unsafe fn do_at(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() || CDR(args) == R_NilValue() {
            return R_NilValue();
        }
        let object = crate::eval::eval::Rf_eval(CAR(args), rho);
        let _object_guard = protect(object);

        // GNU do_AT: OBJECT && !S4 → fixSubset3Args + DispatchOrEval("@").
        if crate::eval::attrib_core::isObject(object) != 0
            && crate::mainutils::coerce::IS_S4_OBJECT(object) == crate::sexp::ffi::FALSE
        {
            let fixed =
                crate::mainutils::subset::fixSubset3Args(call, args, rho, std::ptr::null_mut());
            let _fixed = protect(fixed);
            SETCAR(
                fixed,
                crate::sexp::memory_ext::R_mkEVPROMISE(CAR(fixed), object),
            );
            let mut dispatched = R_NilValue();
            if crate::eval::dispatch::DispatchOrEval(
                call,
                op,
                c"@".as_ptr(),
                fixed,
                rho,
                &mut dispatched,
                0,
                0,
            ) != 0
            {
                return dispatched;
            }
        }

        let nlist = CADR(args);

        // do_AT name check: symbol or non-NA scalar string.
        let name_ok = !nlist.is_null()
            && (TYPEOF(nlist) == SEXPTYPE::SYMSXP
                || (TYPEOF(nlist) == SEXPTYPE::STRSXP
                    && LENGTH(nlist) == 1
                    && STRING_ELT(nlist, 0) != crate::sexp::globals::R_NaString()));
        if !name_ok {
            let msg = "invalid type or length for slot name".to_string();
            std::panic::panic_any(RError { message: msg });
        }

        // Only `.Data` bypasses the S4 requirement; anything else on a
        // non-S4 object is upstream's dispatch failure message.
        if crate::mainutils::essentials::s4::name_of(nlist).as_deref() != Some(".Data")
            && crate::mainutils::coerce::IS_S4_OBJECT(object) == crate::sexp::ffi::FALSE
        {
            let class_str = crate::mainutils::essentials::s4::first_class_display(object);
            let msg = format!(
                "no applicable method for `@` applied to an object of class \"{}\"",
                class_str
            );
            crate::mainutils::errors::errorcall_str(call, &msg);
        }

        crate::mainutils::essentials::s4::R_do_slot(object, nlist)
    }
}

pub unsafe fn do_at_set(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() {
            return R_NilValue();
        }
        let mut object = crate::eval::eval::Rf_eval(CAR(args), rho);
        let _object = protect(object);
        let name = CADR(args);
        let slot = replacement_name(name);
        if crate::sexp::accessors::NAMED(object) > 0
            && TYPEOF(object) == SEXPTYPE::CLOSXP
            && crate::mainutils::coerce::IS_S4_OBJECT(object) != 0
            && (slot == "target" || slot == "defined")
        {
            object = crate::mainutils::duplicate::shallow_duplicate(object);
            crate::sexp::accessors::SET_NAMED(object, 0);
        }
        let _dup = protect(object);
        let value_cell = CDR(CDR(args));
        let value = if value_cell.is_null() || value_cell == R_NilValue() {
            R_NilValue()
        } else {
            crate::eval::eval::Rf_eval(CAR(value_cell), rho)
        };
        let _value = protect(value);
        let evaled = Rf_cons(value, R_NilValue());
        let _e1 = protect(evaled);
        let evaled = Rf_cons(name, evaled);
        let _e2 = protect(evaled);
        let evaled = Rf_cons(object, evaled);
        let _e3 = protect(evaled);
        do_set_slot(call, op, evaled, rho)
    }
}

pub(crate) unsafe fn replacement_name(arg: SEXP) -> String {
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

fn is_data_frame_object(x: SEXP) -> bool {
    unsafe {
        let cls = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
        if cls.is_null() || cls == R_NilValue() || TYPEOF(cls) != crate::sexp::ffi::SEXPTYPE::STRSXP
        {
            return false;
        }
        (0..XLENGTH(cls)).any(|i| {
            let s = STRING_ELT(cls, i as i64);
            !s.is_null() && std::ffi::CStr::from_ptr(CHAR(s)).to_bytes() == b"data.frame"
        })
    }
}

fn frame_row_count(df: SEXP) -> i64 {
    unsafe {
        let rn = crate::sexp::attrib_core::getAttrib(
            df,
            crate::mainutils::subset::installTrChar(STRING_ELT(
                crate::sexp::constructors::Rf_mkString(c"row.names".as_ptr()),
                0,
            )),
        );
        // compact row.names: c(NA_integer_, -n)
        if !rn.is_null()
            && rn != R_NilValue()
            && TYPEOF(rn) == crate::sexp::ffi::SEXPTYPE::INTSXP
            && XLENGTH(rn) == 2
            && crate::sexp::accessors::INTEGER_ELT(rn, 1) < 0
        {
            -(crate::sexp::accessors::INTEGER_ELT(rn, 1) as i64)
        } else if !rn.is_null() && rn != R_NilValue() && XLENGTH(rn) > 0 {
            XLENGTH(rn)
        } else if XLENGTH(df) > 0 {
            // fall back to the first column's length
            XLENGTH(crate::sexp::accessors::VECTOR_ELT(df, 0))
        } else {
            0
        }
    }
}

/// Copy element `from` of `src` to element `to` of `dst` for any vector type.
unsafe fn copy_vector_element(src: SEXP, from: i64, dst: SEXP, to: i64) {
    unsafe {
        match TYPEOF(src) {
            t if t == crate::sexp::ffi::SEXPTYPE::INTSXP => {
                *crate::sexp::accessors::INTEGER(dst).add(to as usize) =
                    crate::sexp::accessors::INTEGER_ELT(src, from as i32);
            }
            t if t == crate::sexp::ffi::SEXPTYPE::REALSXP => {
                *crate::sexp::accessors::REAL(dst).add(to as usize) =
                    crate::sexp::accessors::REAL_ELT(src, from as i32);
            }
            t if t == crate::sexp::ffi::SEXPTYPE::LGLSXP => {
                *crate::sexp::accessors::LOGICAL(dst).add(to as usize) =
                    crate::sexp::accessors::LOGICAL_ELT(src, from as i32);
            }
            t if t == crate::sexp::ffi::SEXPTYPE::STRSXP => {
                crate::sexp::accessors::SET_STRING_ELT(
                    dst,
                    to,
                    crate::sexp::accessors::STRING_ELT(src, from),
                );
            }
            t if t == crate::sexp::ffi::SEXPTYPE::VECSXP => {
                crate::sexp::accessors::SET_VECTOR_ELT(
                    dst,
                    to,
                    crate::sexp::accessors::VECTOR_ELT(src, from),
                );
            }
            _ => {}
        }
    }
}

pub unsafe fn do_dollar_set(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() || CDR(args) == R_NilValue() {
            return R_NilValue();
        }
        let call_name = if !_call.is_null()
            && _call != R_NilValue()
            && TYPEOF(CAR(_call)) == SEXPTYPE::SYMSXP
        {
            std::ffi::CStr::from_ptr(CHAR(PRINTNAME(CAR(_call))))
                .to_string_lossy()
                .into_owned()
        } else {
            String::new()
        };
        if call_name == "$<-" {
            let object = crate::eval::eval::Rf_eval(CAR(args), rho);
            let _object_guard = protect(object);
            if crate::eval::attrib_core::isObject(object) != 0 {
                let fixed = crate::mainutils::subset::fixSubset3Args(
                    _call,
                    args,
                    rho,
                    std::ptr::null_mut(),
                );
                let _fixed = protect(fixed);
                SETCAR(
                    fixed,
                    crate::sexp::memory_ext::R_mkEVPROMISE(CAR(fixed), object),
                );
                let mut dispatched = R_NilValue();
                if crate::eval::dispatch::DispatchOrEval(
                    _call,
                    _op,
                    c"$<-".as_ptr(),
                    fixed,
                    rho,
                    &mut dispatched,
                    0,
                    0,
                ) != 0
                {
                    return dispatched;
                }
            }
        }

        let object = crate::eval::eval::Rf_eval(CAR(args), rho);
        let _object_eval = protect(object);
        let mut name_arg = CAR(CDR(args));
        let mut value = if CDR(CDR(args)).is_null() || CDR(CDR(args)) == R_NilValue() {
            R_NilValue()
        } else {
            CAR(CDR(CDR(args)))
        };
        // NextMethod rematches `$<-` onto the method formals, so the
        // primitive argument list can be (x, fun, value) while the call
        // still has the user's name and the value to store.
        if !_call.is_null() && _call != R_NilValue() {
            let call_name = CAR(CDR(CDR(_call)));
            let call_value = CAR(CDR(CDR(CDR(_call))));
            if !call_name.is_null()
                && (TYPEOF(call_name) == SEXPTYPE::SYMSXP || TYPEOF(call_name) == SEXPTYPE::STRSXP)
            {
                name_arg = call_name;
            }
            if !call_value.is_null() && call_value != R_NilValue() {
                value = call_value;
            }
        }
        if TYPEOF(name_arg) == SEXPTYPE::PROMSXP {
            let expr = crate::sexp::accessors::PRCODE(name_arg);
            name_arg = if TYPEOF(expr) == SEXPTYPE::SYMSXP {
                expr
            } else {
                let forced = crate::eval::eval::Rf_eval(name_arg, rho);
                let _forced = protect(forced);
                forced
            };
        }
        let field = replacement_name(name_arg);
        if TYPEOF(value) == SEXPTYPE::PROMSXP
            || TYPEOF(value) == SEXPTYPE::SYMSXP
            || TYPEOF(value) == SEXPTYPE::LANGSXP
        {
            value = crate::eval::eval::Rf_eval(value, rho);
        }
        let _value_eval = protect(value);

        let object = if !object.is_null()
            && object == R_NilValue()
            && !field.is_empty()
            && !value.is_null()
            && value != R_NilValue()
        {
            Rf_allocVector3(SEXPTYPE::VECSXP, 0)
        } else {
            object
        };
        let object = crate::mainutils::duplicate::shallow_duplicate_if_shared(object);
        let _object_guard = protect(object);
        if object.is_null() || object == R_NilValue() || field.is_empty() {
            return object;
        }
        if TYPEOF(object) == SEXPTYPE::LISTSXP
            || TYPEOF(object) == SEXPTYPE::LANGSXP
            || TYPEOF(object) == SEXPTYPE::ENVSXP
        {
            let nlist = Rf_install(CString::new(field.as_str()).unwrap_or_default().as_ptr());
            return crate::mainutils::subassign::R_subassign3_dflt(_call, object, nlist, value);
        }
        if TYPEOF(object) != SEXPTYPE::VECSXP {
            return object;
        }
        // GNU `$<-` on a list: `x$foo <- NULL` deletes the component.
        if value.is_null() || value == R_NilValue() {
            let nlist = Rf_install(CString::new(field.as_str()).unwrap_or_default().as_ptr());
            return crate::mainutils::subassign::R_subassign3_dflt(_call, object, nlist, value);
        }

        // `$<-.data.frame` recycles a length-1 atomic value to the
        // frame's row count (`df$f <- factor("", levels=lv)` gives an
        // n-row column). Plain lists keep the value as-is.
        let value = if is_data_frame_object(object) && XLENGTH(value) == 1 && value != R_NilValue()
        {
            let nrow = frame_row_count(object);
            if nrow > 1 {
                let rep = Rf_allocVector3(TYPEOF(value), nrow);
                let _rep_guard = protect(rep);
                for i in 0..nrow {
                    copy_vector_element(value, 0, rep, i);
                }
                crate::mainutils::array::copyMostAttrib(value, rep);
                rep
            } else {
                value
            }
        } else {
            value
        };

        let names_sym = crate::sexp::attrib_core::R_NamesSymbol();
        let names = crate::sexp::attrib_core::getAttrib(object, names_sym);
        let n = XLENGTH(object);

        if !names.is_null() && names != R_NilValue() && TYPEOF(names) == SEXPTYPE::STRSXP {
            for i in 0..n {
                let name = STRING_ELT(names, i);
                if !name.is_null() && CStr::from_ptr(CHAR(name)).to_string_lossy() == field {
                    let old_n = crate::mainutils::subassign::posixlt_obs_length(object);
                    SET_VECTOR_ELT(object, i, value);
                    crate::mainutils::subassign::mark_posixlt_dollar_balanced(object, value, old_n);
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

        let value = if sexp_has_class(object, "data.frame") && !sexp_has_class(value, "AsIs") {
            let stripped = crate::mainutils::duplicate::duplicate(value);
            crate::sexp::attrib_core::setAttrib(stripped, names_sym, R_NilValue());
            stripped
        } else {
            value
        };
        SET_VECTOR_ELT(out, n, value);
        let field_c = CString::new(field).unwrap_or_default();
        SET_STRING_ELT(out_names, n, Rf_mkChar(field_c.as_ptr()));
        crate::sexp::attrib_core::setAttrib(out, names_sym, out_names);

        // Preserve the object's remaining attributes (class, row.names, dim,
        // ...): appending a column must not demote a data.frame to a plain
        // list. `key$type <- factor(...)` in whisker's getKeyInfo relied on
        // the frame keeping its class and rows across the assignment.
        let mut ap = ATTRIB(object);
        while !ap.is_null() && ap != R_NilValue() {
            let tag = TAG(ap);
            if !tag.is_null() && tag != R_NilValue() && tag != names_sym {
                crate::sexp::attrib_core::setAttrib(out, tag, CAR(ap));
            }
            ap = CDR(ap);
        }

        // A data.frame's rows must match the appended column: extend the
        // compact row names when the new column is longer, and recycle
        // shorter ones like upstream `$<-.data.frame` -> `[[<-`.
        if sexp_has_class(out, "data.frame") {
            let rownames = crate::sexp::attrib_core::getAttrib(
                out,
                crate::sexp::attrib_core::R_RowNamesSymbol(),
            );
            let rows: R_xlen_t = if rownames.is_null() || rownames == R_NilValue() {
                0
            } else if TYPEOF(rownames) == SEXPTYPE::INTSXP
                && XLENGTH(rownames) == 2
                && *INTEGER(rownames) == NA_INTEGER
            {
                -(*INTEGER(rownames).add(1) as R_xlen_t)
            } else {
                XLENGTH(rownames)
            };
            let t = TYPEOF(value);
            let value_len: R_xlen_t = if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP {
                let mut len: R_xlen_t = 0;
                let mut p = value;
                while !p.is_null() && p != R_NilValue() {
                    len += 1;
                    p = CDR(p);
                }
                len
            } else if t == SEXPTYPE::VECSXP
                || t == SEXPTYPE::EXPRSXP
                || t == SEXPTYPE::REALSXP
                || t == SEXPTYPE::INTSXP
                || t == SEXPTYPE::LGLSXP
                || t == SEXPTYPE::CPLXSXP
                || t == SEXPTYPE::STRSXP
                || t == SEXPTYPE::RAWSXP
            {
                XLENGTH(value)
            } else {
                1
            };
            let new_rows = if rows == 0 {
                value_len
            } else if value_len == rows || value_len == 1 || value_len == 0 {
                rows
            } else if value_len > rows && value_len % rows == 0 {
                value_len
            } else if rows % value_len == 0 {
                rows
            } else {
                base_error(format!("replacement has {value_len} rows, data has {rows}"))
            };
            if new_rows != rows {
                crate::mainutils::essentials::functional::set_compact_row_names(out, new_rows);
            }
        }
        crate::mainutils::subassign::mark_posixlt_dollar_balanced(
            out,
            value,
            crate::mainutils::subassign::posixlt_obs_length(object),
        );
        out
    }
}

pub(crate) unsafe fn datetime_c_value(
    source: SEXP,
    index: R_xlen_t,
    class: DatetimeVectorClass,
) -> f64 {
    unsafe {
        match class {
            DatetimeVectorClass::Date => {
                if TYPEOF(source) == SEXPTYPE::STRSXP {
                    let value = STRING_ELT(source, index);
                    if value.is_null() || value == crate::sexp::globals::R_NaString() {
                        return NA_REAL;
                    }
                    let text = CStr::from_ptr(CHAR(value)).to_str().unwrap_or("");
                    if text.is_empty() {
                        return NA_REAL;
                    }
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
// Helpers
// ---------------------------------------------------------------------------

pub(crate) unsafe fn string_vector(values: &[String]) -> SEXP {
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

pub(crate) unsafe fn static_string_vector(values: &[&str]) -> SEXP {
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
                Rf_mkChar(CString::new(*value).unwrap_or_default().as_ptr()),
            );
        }
        result
    }
}

pub(crate) unsafe fn optional_string_vector(values: &[Option<String>]) -> SEXP {
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

pub(crate) unsafe fn named_string_vector(values: &[String], names: &[String]) -> SEXP {
    unsafe {
        let result = string_vector(values);
        if result.is_null() || result == R_NilValue() {
            return result;
        }
        set_string_names(result, names);
        result
    }
}

pub(crate) unsafe fn named_string_list(items: impl IntoIterator<Item = (String, String)>) -> SEXP {
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
pub(crate) fn find_package_path(package: &str) -> String {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst)
            .path_policy
            .find_package_path(package)
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default()
    })
}

pub(crate) fn package_description_fields(
    package: &str,
) -> Result<BTreeMap<String, String>, String> {
    if package.is_empty() || package == "NA" {
        return Err("invalid package name".to_string());
    }

    let package_path = find_package_path(package);
    if package_path.is_empty() {
        if package == "datasets" {
            return Ok(description_fields(include_str!(
                "../../library/datasets/assets/DESCRIPTION"
            )));
        }
        return Err(format!("there is no package called '{}'", package));
    }

    let description = Path::new(&package_path).join("DESCRIPTION");
    let content = std::fs::read_to_string(&description)
        .map_err(|err| format!("could not read {}: {err}", description.display()))?;
    Ok(description_fields(&content))
}

pub(crate) unsafe fn load_package_namespace_by_name(package: &str) -> Result<SEXP, String> {
    unsafe {
        let package = package.strip_prefix("package:").unwrap_or(package);
        if package.is_empty() || package == "NA" {
            return Err("invalid package name".to_string());
        }
        // GNU getNamespace("base") / asNamespace("base") is .BaseNamespaceEnv,
        // not the search-path base environment and not library/base.
        if package == "base" {
            return Ok(crate::sexp::envir::R_BaseNamespace());
        }
        // GNU keeps loaded namespace identities when .libPaths() changes.
        // Existing closures and imports still refer to that original graph.
        if let Some(namespace) = cached_namespace_by_name(package) {
            return Ok(namespace);
        }

        #[cfg(feature = "renderplot-device")]
        if package == "grid" {
            return Ok(crate::mainutils::portable_grid::namespace());
        }

        let package_path = find_package_path(package);
        if package_path.is_empty() {
            if package == "methods" {
                return crate::library::methods::portable::namespace()
                    .map(|namespace| namespace.as_raw());
            }
            if let Some(image) = crate::library::portable_package::image(package) {
                return image.namespace().map(|namespace| namespace.as_raw());
            }
            if package == "datasets" {
                return crate::library::datasets::namespace().map(|namespace| namespace.as_raw());
            }
            return Err(format!("there is no package called '{}'", package));
        }
        let package_dir = Path::new(&package_path);
        let description = package_dir.join("DESCRIPTION");
        if package_needs_compilation(&description)? && !is_builtin_package_dependency(package) {
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

pub(crate) const INSTALLED_PACKAGE_COLUMNS: [&str; 16] = [
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
pub(crate) struct InstalledPackageRow {
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

pub(crate) fn installed_package_rows() -> Vec<InstalledPackageRow> {
    let library_paths = crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).path_policy.library_paths().to_vec()
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

pub(crate) unsafe fn installed_packages_matrix(packages: &[InstalledPackageRow]) -> SEXP {
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

pub(crate) fn package_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: message.into(),
    });
}

pub(crate) unsafe fn package_name_symbol() -> SEXP {
    unsafe { Rf_install(c".packageName".as_ptr()) }
}

pub(crate) unsafe fn name_symbol() -> SEXP {
    unsafe { Rf_install(c"name".as_ptr()) }
}

pub(crate) unsafe fn namespace_env_symbol() -> SEXP {
    unsafe { Rf_install(c".namespaceEnv".as_ptr()) }
}

pub(crate) unsafe fn lazy_data_names_symbol() -> SEXP {
    unsafe { Rf_install(c".lazyDataNames".as_ptr()) }
}

pub(crate) unsafe fn package_name_binding(env: SEXP) -> Option<String> {
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

pub(crate) unsafe fn package_attached(package: &str) -> bool {
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

unsafe fn attached_package_env(package: &str) -> Option<SEXP> {
    unsafe {
        let mut env = crate::sexp::accessors::ENCLOS(crate::sexp::globals::R_GlobalEnv());
        let base = crate::sexp::globals::R_BaseEnv();
        while !env.is_null() && env != base {
            if package_name_binding(env).as_deref() == Some(package) {
                return Some(env);
            }
            env = crate::sexp::accessors::ENCLOS(env);
        }
        None
    }
}

pub(crate) unsafe fn load_pure_r_package(package: &str, package_dir: &Path) -> Result<(), String> {
    let mut loading = Vec::<String>::new();
    unsafe { load_pure_r_package_recursive(package, package_dir, &mut loading) }
}

pub(crate) unsafe fn load_pure_r_package_recursive(
    package: &str,
    package_dir: &Path,
    loading_packages: &mut Vec<String>,
) -> Result<(), String> {
    unsafe {
        let description = package_dir.join("DESCRIPTION");
        if !description.is_file() {
            return Err(format!("package '{}' has no DESCRIPTION", package));
        }
        if loading_packages.iter().any(|entry| entry == package) {
            return Err(format!(
                "cyclic package dependency while loading '{}': {} -> {}",
                package,
                loading_packages.join(" -> "),
                package
            ));
        }
        if package_needs_compilation(&description)? && !is_builtin_package_dependency(package) {
            return Err(format!(
                "package '{}' declares NeedsCompilation: yes; this pure-R Android runtime does not load compiled package code",
                package
            ));
        }

        loading_packages.push(package.to_string());
        let result = (|| {
            load_package_dependencies(package, package_dir, loading_packages)?;

            let mut loading = vec![package.to_string()];
            let (package_env, namespace) =
                load_package_namespace(package, package_dir, &mut loading)?;
            let _package_env_guard = crate::sexp::protect::protect(package_env);
            let attach_env = if package == "utils" {
                make_package_attach_env_lenient(package, namespace.as_ref(), package_env)?
            } else {
                make_package_attach_env(package, namespace.as_ref(), package_env)?
            };
            if package == "methods" {
                bind_methods_base_primitives(package_env);
                purge_missing_arg_placeholders(package_env);
                retarget_methods_generics(package_env);
            }
            attach_package_env(attach_env);
            if package == "utils" {
                install_utils_str_option(package_env);
            }
            if package == "methods" {
                run_methods_onload_cache_metadata(package_env);
                export_s4_metadata_to_package_env(package_env, attach_env);
                retarget_envref_object_parent(package_env);
            }

            Ok(())
        })();
        loading_packages.pop();
        result
    }
}

pub(crate) unsafe fn load_package_dependencies(
    package: &str,
    package_dir: &Path,
    loading_packages: &mut Vec<String>,
) -> Result<(), String> {
    unsafe {
        for dependency in package_depends_names(package_dir)? {
            if is_builtin_package_dependency(&dependency) || package_attached(&dependency) {
                continue;
            }
            let dependency_path = find_package_path(&dependency);
            if dependency_path.is_empty() {
                return Err(format!(
                    "package '{}' depends on missing package '{}'",
                    package, dependency
                ));
            }
            load_pure_r_package_recursive(
                &dependency,
                Path::new(&dependency_path),
                loading_packages,
            )?;
        }
        Ok(())
    }
}

pub(crate) fn package_needs_compilation(description: &Path) -> Result<bool, String> {
    let content = std::fs::read_to_string(description)
        .map_err(|err| format!("could not read {}: {err}", description.display()))?;
    Ok(description_fields(&content)
        .get("NeedsCompilation")
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("yes") || value.eq_ignore_ascii_case("true")
        }))
}

pub(crate) fn package_declares_lazy_data(package_dir: &Path) -> Result<bool, String> {
    let description = package_dir.join("DESCRIPTION");
    let content = std::fs::read_to_string(&description)
        .map_err(|err| format!("could not read {}: {err}", description.display()))?;
    Ok(description_fields(&content)
        .get("LazyData")
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("yes") || value.eq_ignore_ascii_case("true")
        }))
}

pub(crate) fn description_fields(description: &str) -> BTreeMap<String, String> {
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

pub(crate) fn description_file_list(value: &str) -> Vec<String> {
    let mut files = Vec::new();
    let mut chars = value.char_indices().peekable();

    while let Some((_, ch)) = chars.peek().copied() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }

        let mut file = String::new();
        if ch == '\'' || ch == '"' {
            let quote = ch;
            chars.next();
            let mut escaped = false;
            for (_, next) in chars.by_ref() {
                if escaped {
                    file.push(next);
                    escaped = false;
                } else if next == '\\' {
                    escaped = true;
                } else if next == quote {
                    break;
                } else {
                    file.push(next);
                }
            }
        } else {
            while let Some((_, next)) = chars.peek().copied() {
                if next.is_whitespace() {
                    break;
                }
                file.push(next);
                chars.next();
            }
        }

        if !file.trim().is_empty() {
            push_unique(&mut files, file.trim().to_string());
        }
    }

    files
}

pub(crate) fn description_package_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .filter_map(|entry| {
            let name = entry
                .split_once('(')
                .map(|(name, _)| name)
                .unwrap_or(entry)
                .trim();
            (!name.is_empty()).then(|| name.to_string())
        })
        .fold(Vec::<String>::new(), |mut names, name| {
            push_unique(&mut names, name);
            names
        })
}

pub(crate) fn package_depends_names(package_dir: &Path) -> Result<Vec<String>, String> {
    let description = package_dir.join("DESCRIPTION");
    let content = std::fs::read_to_string(&description)
        .map_err(|err| format!("could not read {}: {err}", description.display()))?;
    Ok(description_fields(&content)
        .get("Depends")
        .map(|value| description_package_list(value))
        .unwrap_or_default())
}
pub(crate) fn is_builtin_package_dependency(package: &str) -> bool {
    matches!(
        package,
        "R" | "base"
            | "compiler"
            | "datasets"
            | "grDevices"
            | "graphics"
            | "grid"
            | "methods"
            | "parallel"
            | "splines"
            | "stats"
            | "stats4"
            | "tcltk"
            | "tools"
            | "utils"
    )
}
pub(crate) unsafe fn bind_methods_base_primitives(ns: SEXP) {
    unsafe {
        for name in [
            "standardGeneric",
            "ngettext",
            "gettext",
            "bindtextdomain",
            "dgettext",
            "asS4",
            ".asS4",
            "parent.env",
            "parent.env<-",
            "list2env",
            "as.environment",
        ] {
            let symbol = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
            let value = crate::sexp::envir::R_findVarInFrame(ns, symbol);
            let kind = TYPEOF(value);
            if kind == SEXPTYPE::CLOSXP
                || kind == SEXPTYPE::BUILTINSXP
                || kind == SEXPTYPE::SPECIALSXP
            {
                continue;
            }
            let prim = crate::eval::primitive::make_primitive_binding(name, SEXPTYPE::BUILTINSXP);
            if !prim.is_null() && prim != R_NilValue() {
                crate::sexp::envir::defineVar(symbol, prim, ns);
            }
        }
    }
}

/// Finish the same original namespace lifecycle for installed and portable images.
/// No runtime/field loan remains live while a step can evaluate R code.
pub(crate) unsafe fn finalize_methods_namespace(namespace: SEXP) {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let namespace = owner
            .sexp(namespace)
            .and_then(crate::sexp::object::Sexp::into_owned)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let authority = crate::sexp::owner::StoredOwner::from_value(&namespace)
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let _pin = owner
            .pin()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        for step in [
            bind_methods_base_primitives as unsafe fn(SEXP),
            purge_missing_arg_placeholders,
            retarget_methods_generics,
            run_methods_onload_cache_metadata,
            retarget_envref_object_parent,
        ] {
            authority
                .require_active()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            step(namespace.as_raw());
            authority
                .require_active()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
    }
}

/// Restore the session-specific methods dispatch state after loading its image.
/// The installed image contains its class and implicit-generic tables already;
/// GNU's installed `.onLoad` initializes dispatch without recaching that namespace.
pub(crate) unsafe fn run_methods_onload_cache_metadata(where_env: SEXP) {
    unsafe {
        if where_env.is_null() || where_env == R_NilValue() {
            return;
        }
        let Some(ns) = cached_namespace_by_name("methods") else {
            return;
        };
        eval_methods_ns_fun(ns, c".initImplicitGenerics", where_env, None);
        let captured_image = crate::sexp::instance::with_required_current_instance(|instance| {
            (*instance)
                .package_namespace_cache
                .get("methods")
                .is_some_and(|(path, namespace)| {
                    *namespace == ns && path == Path::new("<builtin:methods>")
                })
        });
        // The original installed image includes .implicitTable and its method
        // environments after registration. Re-registering replays loading work
        // that GNU's installed .onLoad does not perform.
        if !captured_image {
            register_implicit_generics_table(ns);
        }
        // GNU zzz.R: initMethodDispatch(where); .Call(C_R_set_method_dispatch, TRUE).
        // The installed image already ran onLoad, so the session must still
        // install the standardGeneric pointer or .isMethodsDispatchOn() stays FALSE.
        // Set table_dispatch_on first: R_initMethodDispatch reads that bit to
        // choose R_dispatchGeneric / R_quick_dispatch (GNU post-onload state).
        let on = Rf_ScalarLogical(TRUE);
        let _on = protect(on);
        crate::library::methods::methods_list_dispatch::R_set_method_dispatch(on);
        crate::library::methods::methods_list_dispatch::R_initMethodDispatch(ns);

        if let Some(attach_env) = attached_package_env("methods") {
            export_s4_metadata_to_package_env(ns, attach_env);
        }
        {
            let primitive = crate::sexp::envir::R_findVarInFrame(
                crate::sexp::globals::R_BaseEnv(),
                Rf_install(c"$".as_ptr()),
            );
            // Admit primitive dispatch for both installed and captured namespaces;
            // restore the original
            // generic when the first S4 lookup resets the method table.
            crate::mainutils::objects::do_set_prim_method(
                primitive,
                c"reset".as_ptr(),
                R_NilValue(),
                R_NilValue(),
            );
        }
        if !captured_image {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::mainutils::gram_main::R_ParseEvalString(
                    c"cd <- tryCatch(getClassDef(\"envRefClass\"), error=function(e) NULL)
if (!is.null(cd) && is.environment(cd@refMethods)) {
  op <- cd@refMethods$.objectParent
  ns <- asNamespace(\"methods\")
  if (is.environment(op) && !identical(op, ns)) parent.env(op) <- ns
}

NULL"
                        .as_ptr(),
                    ns,
                )
            }));
        }
    }
}

/// Materialize the original methods generic only when S4 primitive dispatch
/// first requests it. Ordinary console and plotting work need no S4 table.
pub(crate) unsafe fn ensure_methods_primitive_generic(op: SEXP) {
    unsafe {
        let name = std::ffi::CStr::from_ptr(crate::mainutils::relop::PRIMNAME(op));
        if name != c"$" || crate::mainutils::objects::R_primitive_generic(op) != R_NilValue() {
            return;
        }
        let Some(ns) = cached_namespace_by_name("methods") else {
            return;
        };
        register_original_dollar_primitive(ns);
    }
}

/// GNU methods onLoad resets `$` with getGeneric("$"). The pinned image
/// already contains that exact generic in .BasicFunsList; use the original
/// definition without rerunning getGeneric's recursive cache discovery.
unsafe fn register_original_dollar_primitive(ns: SEXP) {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| package_error(error.to_string()));
        let namespace = owner
            .sexp(ns)
            .and_then(crate::sexp::object::Sexp::into_owned)
            .unwrap_or_else(|error| package_error(error.to_string()));
        let authority = crate::sexp::owner::StoredOwner::from_value(&namespace)
            .unwrap_or_else(|error| package_error(error.to_string()));
        let _pin = owner
            .pin()
            .unwrap_or_else(|error| package_error(error.to_string()));
        let table =
            crate::sexp::envir::R_findVarInFrame(ns, Rf_install(c".BasicFunsList".as_ptr()));
        let table = owner
            .sexp(table)
            .and_then(crate::sexp::object::Sexp::into_owned)
            .unwrap_or_else(|error| package_error(error.to_string()));
        let table_raw = if TYPEOF(table.as_raw()) == SEXPTYPE::PROMSXP {
            crate::sexp::envir::forcePromise(table.as_raw())
        } else {
            table.as_raw()
        };
        authority
            .require_active()
            .unwrap_or_else(|error| package_error(error.to_string()));
        let table = owner
            .sexp(table_raw)
            .and_then(crate::sexp::object::Sexp::into_owned)
            .unwrap_or_else(|error| package_error(error.to_string()));
        let names = owner
            .sexp(crate::sexp::attrib_core::getAttrib(
                table.as_raw(),
                crate::sexp::attrib_core::R_NamesSymbol(),
            ))
            .and_then(crate::sexp::object::Sexp::into_owned)
            .unwrap_or_else(|error| package_error(error.to_string()));
        for index in 0..table.len() {
            if names
                .try_string_elt(index)
                .and_then(|name| name.try_char_eq(b"$"))
                .unwrap_or_else(|error| package_error(error.to_string()))
            {
                let generic = table
                    .try_vector_elt(index)
                    .unwrap_or_else(|error| package_error(error.to_string()));
                let primitive = crate::sexp::envir::R_findVarInFrame(
                    crate::sexp::globals::R_BaseEnv(),
                    Rf_install(c"$".as_ptr()),
                );
                crate::mainutils::objects::do_set_prim_method(
                    primitive,
                    c"reset".as_ptr(),
                    generic.as_raw(),
                    R_NilValue(),
                );
                return;
            }
        }
        package_error("methods namespace has no dollar generic");
    }
}

/// GNU `library()`/`attach()`: `methods:::cacheMetaData(env, TRUE)`.
/// GNU bytecode for cacheMetaData GETFUN/CALL-loops on a package attach
/// env; run the stored source for that one call and restore BODY after.
pub(crate) unsafe fn cache_attached_package_metadata(attach_env: SEXP) {
    unsafe {
        if attach_env.is_null() {
            return;
        }
        let Some(methods_ns) = cached_namespace_by_name("methods") else {
            return;
        };
        let mut fun =
            crate::sexp::envir::R_findVarInFrame(methods_ns, Rf_install(c"cacheMetaData".as_ptr()));
        if fun.is_null() || fun == crate::sexp::globals::R_UnboundValue() {
            return;
        }
        if TYPEOF(fun) == SEXPTYPE::PROMSXP {
            fun = crate::sexp::envir::forcePromise(fun);
        }
        if TYPEOF(fun) != SEXPTYPE::CLOSXP {
            return;
        }
        let _source_body = methods_metadata_source_body(fun);
        let attach = Rf_ScalarLogical(TRUE);
        let _attach = protect(attach);
        let call = crate::sexp::constructors::Rf_lang3(fun, attach_env, attach);
        let _call = protect(call);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::eval::eval::Rf_eval(call, crate::sexp::globals::R_GlobalEnv())
        }));
    }
}

/// Register the installed implicit prototypes without importing their packages.
/// GNU methods keeps the stats-labeled toeplitz prototype in this table even
/// while the actual stats namespace is unloaded. Its closure is supplied later
/// by the ordinary stats namespace loader, retaining that namespace's identity.
unsafe fn register_implicit_generics_table(ns: SEXP) {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current()
            .unwrap_or_else(|error| package_error(error.to_string()));
        let runtime = owner
            .weak_owner()
            .unwrap_or_else(|| package_error("implicit generics require a managed runtime"));
        let namespace = owner
            .sexp(ns)
            .and_then(crate::sexp::object::Sexp::into_owned)
            .unwrap_or_else(|error| package_error(error.to_string()));
        crate::sexp::owner::with_runtime(&runtime, |access| {
            let domain = access.domain();
            let parsed = access.with_arena(|arena| {
                crate::eval::parser::parse_expressions(
                    "registerImplicitGenerics(where = asNamespace(\"methods\"))",
                    arena,
                    domain,
                )
            })?;
            access.with_native(|_| {
                crate::eval::parser::flush_literal_warnings();
                Ok(())
            })?;
            let Ok(expressions) = parsed else {
                return Ok(());
            };
            // Keep the original namespace and actual parsed expression owners
            // through every allocating/collecting callback. Original cleanup
            // remains valid if callbacks revoke the session.
            let evaluation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                for expression in &expressions {
                    access.with_native(|token| {
                        let result =
                            crate::eval::eval::Rf_eval(expression.as_raw(), namespace.as_raw());
                        token.sexp(result)?.into_owned()?;
                        Ok(())
                    })?;
                }
                Ok::<(), crate::sexp::object::SexpError>(())
            }));
            access.require_active()?;
            if let Ok(result) = evaluation {
                result?;
            }
            Ok::<(), crate::sexp::object::SexpError>(())
        })
        .and_then(|result| result)
        .unwrap_or_else(|error| package_error(error.to_string()));
    }
}

/// Retain the exact compiled body while evaluating its stored R expression.
/// Only cacheMetaData uses this fallback. Its invocation must also suspend
/// JIT compilation: applyClosure checks JIT before reading BODY and otherwise
/// recompiles this temporary source immediately. Arguments and environments
/// remain unchanged; the owner's prior JIT setting is restored after the call.
struct MethodsMetadataSourceBody<'s> {
    function: crate::sexp::object::Sexp<'s>,
    original: crate::sexp::object::Sexp<'s>,
    allocation: crate::sexp::heap::CheckedNode,
    owner: crate::sexp::owner::OwnerToken<'s>,
    availability: crate::sexp::instance::InstanceLiveness,
    jit_enabled: i32,
}

impl Drop for MethodsMetadataSourceBody<'_> {
    fn drop(&mut self) {
        let original = self
            .original
            .allocation()
            .expect("compiled body has an allocation");
        if self.availability.is_live() {
            // Both nodes remain owned; no storage loan or callback crosses the
            // barrier. Teardown still restores the retained canonical body.
            unsafe {
                if !crate::sexp::gengc::write_barrier_in(
                    self.owner.as_ptr(),
                    self.function.as_raw(),
                    self.original.as_raw(),
                ) {
                    std::alloc::handle_alloc_error(std::alloc::Layout::new::<SEXP>());
                }
            }
        }
        self.allocation
            .heap_identity()
            .set_edge(
                &self.allocation,
                crate::sexp::ffi::EdgeField::ClosureBody,
                crate::sexp::heap::ReferenceChild::Node(original),
            )
            .expect("owned methods closure retains its original body");
        if self.availability.is_live()
            && crate::eval::jit::get_R_jit_enabled_in(self.owner.as_ptr()) == 0
        {
            // Preserve an explicit callback change to a different JIT level.
            crate::eval::jit::set_R_jit_enabled_in(self.owner.as_ptr(), self.jit_enabled);
        }
    }
}

unsafe fn methods_metadata_source_body<'s>(fun: SEXP) -> Option<MethodsMetadataSourceBody<'s>> {
    unsafe {
        let owner = crate::sexp::owner::OwnerToken::current().ok()?;
        let factory = crate::sexp::object::SessionNodeFactory::new(owner);
        let function = factory.wrap(fun).ok()?;
        let original = function.try_body().ok()?;
        if !original.is_bytecode() {
            return None;
        }
        let source = factory
            .wrap(crate::eval::bc_eval::BCODE_EXPR(original.as_raw()))
            .ok()?;
        if source.typeof_() != SEXPTYPE::LANGSXP {
            return None;
        }
        let allocation = function.allocation().ok()?.clone();
        let source_allocation = source.allocation().ok()?;
        let guard = MethodsMetadataSourceBody {
            function,
            original,
            allocation,
            owner,
            availability: crate::sexp::instance::instance_liveness(owner.as_ptr()),
            jit_enabled: crate::eval::jit::get_R_jit_enabled_in(owner.as_ptr()),
        };
        if !crate::sexp::gengc::write_barrier_in(
            owner.as_ptr(),
            guard.function.as_raw(),
            source.as_raw(),
        ) {
            std::alloc::handle_alloc_error(std::alloc::Layout::new::<SEXP>());
        }
        guard.allocation.heap_identity().set_edge(
            &guard.allocation,
            crate::sexp::ffi::EdgeField::ClosureBody,
            crate::sexp::heap::ReferenceChild::Node(source_allocation),
        )?;
        crate::eval::jit::set_R_jit_enabled_in(owner.as_ptr(), 0);
        Some(guard)
    }
}

unsafe fn eval_methods_ns_fun(
    ns: SEXP,
    name: &std::ffi::CStr,
    where_env: SEXP,
    attach: Option<SEXP>,
) {
    unsafe {
        let mut fun = crate::sexp::envir::R_findVarInFrame(ns, Rf_install(name.as_ptr()));
        if fun.is_null() || fun == crate::sexp::globals::R_UnboundValue() {
            return;
        }
        if TYPEOF(fun) == SEXPTYPE::PROMSXP {
            fun = crate::sexp::envir::forcePromise(fun);
        }
        if TYPEOF(fun) != SEXPTYPE::CLOSXP {
            return;
        }
        let _source_body = if name == c"cacheMetaData" {
            methods_metadata_source_body(fun)
        } else {
            None
        };
        let call = if let Some(attach) = attach {
            crate::sexp::constructors::Rf_lang3(fun, where_env, attach)
        } else {
            crate::sexp::constructors::Rf_lang2(fun, where_env)
        };
        let _call = protect(call);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::eval::eval::Rf_eval(call, ns)
        }));
    }
}

unsafe fn purge_missing_arg_placeholders(env: SEXP) {
    unsafe {
        let missing = crate::sexp::globals::R_MissingArg();
        let mut doomed = Vec::new();
        for name in frame_binding_names(env, true) {
            let Ok(cname) = CString::new(name.as_str()) else {
                continue;
            };
            let symbol = Rf_install(cname.as_ptr());
            let value = crate::sexp::envir::R_findVarInFrame(env, symbol);
            if value.is_null() || value == crate::sexp::globals::R_UnboundValue() {
                continue;
            }
            let kind = TYPEOF(value);
            let empty_symbol =
                kind == SEXPTYPE::SYMSXP && symbol_name(value).is_none_or(|n| n.is_empty());
            let shadows_base = kind == SEXPTYPE::SYMSXP
                && (crate::eval::builtin::has_builtin_handler(&name)
                    || crate::eval::primitive::fun_tab_index_by_name(&name).is_some());
            if value == missing || empty_symbol || shadows_base {
                doomed.push(name);
            }
        }
        for name in doomed {
            let Ok(cname) = CString::new(name) else {
                continue;
            };
            crate::sexp::envir::remove_binding_raw(env, Rf_install(cname.as_ptr()));
        }
    }
}

unsafe fn retarget_methods_generics(ns: SEXP) {
    unsafe {
        let methods_ns_sym = Rf_install(c".methodsNamespace".as_ptr());
        let old_ns = crate::sexp::envir::R_findVarInFrame(ns, methods_ns_sym);
        // `.InitRefClasses` captured this stub as envRefClass@refMethods$.objectParent.
        if !old_ns.is_null()
            && old_ns != crate::sexp::globals::R_UnboundValue()
            && TYPEOF(old_ns) == SEXPTYPE::ENVSXP
            && old_ns != ns
            && !env_chain_contains(ns, old_ns)
        {
            SET_ENCLOS(old_ns, ns);
        }

        crate::sexp::envir::defineVar(methods_ns_sym, ns, ns);
        for name in ["initialize", "new", "setClass", "getClass", "getClassDef"] {
            let symbol = Rf_install(CString::new(name).unwrap_or_default().as_ptr());
            let mut value = crate::sexp::envir::R_findVarInFrame(ns, symbol);
            if value.is_null() || value == crate::sexp::globals::R_UnboundValue() {
                continue;
            }
            if TYPEOF(value) == SEXPTYPE::PROMSXP {
                value = crate::sexp::accessors::PRVALUE(value);
            }
            if TYPEOF(value) != SEXPTYPE::CLOSXP {
                continue;
            }
            let cloenv = crate::sexp::accessors::CLOENV(value);
            if cloenv.is_null() || cloenv == ns {
                continue;
            }
            // Do not steal a generic's table environment; only enclose it.
            if crate::sexp::accessors::ENCLOS(cloenv) != ns {
                crate::sexp::accessors::SET_ENCLOS(cloenv, ns);
            }
        }
    }
}

unsafe fn env_chain_contains(mut env: SEXP, target: SEXP) -> bool {
    unsafe {
        for _ in 0..64 {
            if env.is_null() || env == R_NilValue() {
                return false;
            }
            if env == target {
                return true;
            }
            if TYPEOF(env) != SEXPTYPE::ENVSXP {
                return false;
            }
            env = crate::sexp::accessors::ENCLOS(env);
        }
        false
    }
}

/// After methods `.onLoad`, the captured `.objectParent` stub must enclose
/// the methods namespace so ref methods see unexported helpers (`classLabel`).
unsafe fn retarget_envref_object_parent(ns: SEXP) {
    unsafe {
        let class_def =
            crate::sexp::envir::R_findVarInFrame(ns, Rf_install(c".__C__envRefClass".as_ptr()));
        if class_def.is_null()
            || class_def == crate::sexp::globals::R_UnboundValue()
            || class_def == R_NilValue()
            || TYPEOF(class_def) == SEXPTYPE::PROMSXP
        {
            return;
        }
        retarget_envref_definition_parent(class_def, ns);
    }
}

/// Repair the restored reference-class definition when its lazy record is
/// actually requested, retaining GNU's deferred class-loading behavior.
pub(crate) unsafe fn retarget_envref_definition_parent(class_def: SEXP, ns: SEXP) {
    unsafe {
        let ref_methods =
            crate::eval::attrib_core::getAttrib(class_def, Rf_install(c"refMethods".as_ptr()));
        if TYPEOF(ref_methods) != SEXPTYPE::ENVSXP {
            return;
        }
        let object_parent = crate::sexp::envir::R_findVarInFrame(
            ref_methods,
            Rf_install(c".objectParent".as_ptr()),
        );
        if object_parent.is_null()
            || object_parent == crate::sexp::globals::R_UnboundValue()
            || TYPEOF(object_parent) != SEXPTYPE::ENVSXP
            || object_parent == ns
            || env_chain_contains(ns, object_parent)
        {
            return;
        }
        SET_ENCLOS(object_parent, ns);
    }
}

/// GNU `utils:::.onLoad` sets `options(str = strOptions())`.
unsafe fn install_utils_str_option(env: SEXP) {
    unsafe {
        let src = crate::sexp::constructors::Rf_mkString(
            b"if (is.null(getOption(\"str\"))) options(str = strOptions())\0".as_ptr()
                as *const std::os::raw::c_char,
        );
        let _src = crate::sexp::protect::protect(src);
        let mut status = 0i32;
        let parsed = crate::mainutils::gram_main::R_ParseVector(
            src,
            -1,
            &mut status,
            crate::sexp::globals::R_NilValue(),
        );
        if status == 1 && !parsed.is_null() {
            let _parsed = crate::sexp::protect::protect(parsed);
            let expr = crate::sexp::accessors::VECTOR_ELT(parsed, 0);
            let _ = crate::eval::eval::Rf_eval(expr, env);
        }
    }
}

/// Pops a non-reentrant load from the session that pushed it.
struct NamespaceLoadGuard {
    instance: *mut crate::sexp::instance::RInstance,
    package: String,
    reentered: bool,
}

impl NamespaceLoadGuard {
    fn enter(package: &str) -> Self {
        let Some(instance) = crate::sexp::instance::current_instance_ptr() else {
            return Self {
                instance: std::ptr::null_mut(),
                package: package.to_string(),
                reentered: false,
            };
        };
        // The pointer is copied out of the thread-local before the stack is
        // touched, so this does not re-borrow that cell. Drop pops the same
        // session even if another session is ambient.
        let reentered = unsafe {
            let stack = &mut (*instance).namespace_loads_in_progress;
            let reentered = stack.iter().any(|name| name == package);
            if !reentered {
                stack.push(package.to_string());
            }
            reentered
        };
        Self {
            instance,
            package: package.to_string(),
            reentered,
        }
    }
}

impl Drop for NamespaceLoadGuard {
    fn drop(&mut self) {
        if self.reentered || self.instance.is_null() {
            return;
        }
        unsafe {
            let stack = &mut (*self.instance).namespace_loads_in_progress;
            if let Some(index) = stack.iter().rposition(|name| name == &self.package) {
                stack.remove(index);
            }
        }
    }
}

pub(crate) unsafe fn load_package_namespace(
    package: &str,
    package_dir: &Path,
    loading: &mut Vec<String>,
) -> Result<(SEXP, Option<NamespaceDirectives>), String> {
    unsafe {
        if package == "compiler" {
            let env = crate::eval::compiler::namespace();
            return Ok((env, None));
        }
        // The namespace is registered before its R code runs, but a cache hit
        // also reinstalls native helpers. Those helpers evaluate package code
        // (`RweaveLatexRuncode <- ...` in utils) which can lazy-load a
        // namespace reference and call back into this function. Returning the
        // in-flight environment here matches GNU: the registry entry is the
        // namespace under construction, and `.onLoad` does not run again.
        let load_guard = NamespaceLoadGuard::enter(package);
        if load_guard.reentered {
            if let Some(env) = cached_package_namespace(package, package_dir) {
                let directives = read_namespace_directives(package_dir).ok().flatten();
                return Ok((env, directives));
            }
            return Err(format!("cyclic namespace load while loading '{package}'"));
        }
        if let Some(env) = cached_package_namespace(package, package_dir) {
            if package == "methods" {
                crate::library::methods::native_calls::install_methods_call_symbols(env);
                retarget_methods_generics(env);
            }

            if package == "tools" {
                crate::library::tools::native_calls::install_tools_call_symbols(env);
                crate::library::tools::native_calls::install_tools_assert_closures(env);
            }

            if package == "stats" {
                crate::library::stats::random::install_stats_call_symbols(env);
            }
            if package == "splines" {
                crate::library::splines::splines::install_splines_call_symbols(env);
            }
            if package == "utils" {
                crate::library::utils::install_utils_call_symbols(env);
            }
            if package == "grDevices" {
                crate::library::grdevices::install_call_symbols(env);
                crate::library::grdevices::colors::initPalette();
            }
            if package == "grid" {
                crate::library::grid::install_call_symbols(env);
            }
            if package == "graphics" {
                crate::library::graphics::install_call_symbols(env);
            }

            let directives = read_namespace_directives(package_dir)?;
            ensure_namespace_info(package, package_dir, env, directives.as_ref());
            return Ok((env, directives));
        }

        // GNU `makeNamespace`: the namespace's enclosure chain ends at
        // `.BaseNamespaceEnv` (parent `.GlobalEnv`), not `baseenv()`
        // (parent `emptyenv()`). `getFunction` stops at `isBaseNamespace`;
        // a chain that never reaches that environment loops on
        // `parent.env(emptyenv())`.
        let package_env = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            crate::sexp::envir::R_BaseNamespace(),
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
        if !is_builtin_package_dependency(package) {
            reject_unsupported_internal_data(package, package_dir)?;
            reject_unsupported_lazyload_code(package, package_dir)?;
        }

        // Register the in-flight namespace before sourcing its R files:
        // upstream loadNamespace records the namespace first, so package
        // code that calls `asNamespace(pkg)` mid-load (crayon's
        // `assign(style, make_style(style), envir = asNamespace("crayon"))`)
        // receives the environment under construction instead of triggering
        // a nested load of the same package.
        cache_package_namespace(package, package_dir, package_env);
        if package == "methods" {
            crate::library::methods::native_calls::install_methods_call_symbols(package_env);
        }
        if package == "grDevices" {
            // Its sourced initialization calls color wrappers before the
            // post-load setup; their typed native descriptors must exist now.
            crate::library::grdevices::install_call_symbols(package_env);
        }

        let populated = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            populate_package_namespace(package, package_dir, package_env, loading)
        }));
        let namespace = match populated {
            Ok(result) => match result {
                Ok(namespace) => namespace,
                Err(message) => {
                    uncache_package_namespace(package);
                    return Err(message);
                }
            },
            Err(payload) => {
                // Sourced R code errors unwind as RSignal panics; drop the
                // half-built namespace so a later attempt reloads cleanly.
                uncache_package_namespace(package);
                std::panic::resume_unwind(payload);
            }
        };
        if package == "methods" {
            finalize_methods_namespace(package_env);
        }

        if package == "stats" {
            crate::library::stats::random::install_stats_call_symbols(package_env);
        }
        if package == "splines" {
            crate::library::splines::splines::install_splines_call_symbols(package_env);
        }

        if package == "tools" {
            crate::library::tools::native_calls::install_tools_call_symbols(package_env);
            crate::library::tools::native_calls::install_tools_assert_closures(package_env);
        }
        if package == "utils" {
            crate::library::utils::install_utils_call_symbols(package_env);
        }
        if package == "grDevices" {
            crate::library::grdevices::install_call_symbols(package_env);
            crate::library::grdevices::colors::initPalette();
        }
        if package == "grid" {
            crate::library::grid::install_call_symbols(package_env);
        }
        if package == "graphics" {
            crate::library::graphics::install_call_symbols(package_env);
        }
        ensure_namespace_info(package, package_dir, package_env, namespace.as_ref());

        Ok((package_env, namespace))
    }
}

pub(crate) fn normalized_package_dir(package_dir: &Path) -> PathBuf {
    std::fs::canonicalize(package_dir).unwrap_or_else(|_| package_dir.to_path_buf())
}

pub(crate) fn cached_package_namespace(package: &str, package_dir: &Path) -> Option<SEXP> {
    let package_dir = normalized_package_dir(package_dir);
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst)
            .package_namespace_cache
            .get(package)
            .and_then(|(cached_dir, env)| (*cached_dir == package_dir).then_some(*env))
    })
}

pub(crate) fn cache_package_namespace(package: &str, package_dir: &Path, package_env: SEXP) {
    let package_dir = normalized_package_dir(package_dir);
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        if let Some((cached_dir, _)) = (*inst).package_namespace_cache.get(package) {
            if cached_dir.to_string_lossy().starts_with("<builtin:") {
                return;
            }
        }
        (*inst)
            .package_namespace_cache
            .insert(package.to_string(), (package_dir, package_env));
    });
}

pub(crate) fn uncache_package_namespace(package: &str) {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst).package_namespace_cache.remove(package);
    });
}

pub(crate) fn cached_namespace_by_name(package: &str) -> Option<SEXP> {
    if package == "base" {
        return Some(unsafe { crate::sexp::envir::R_BaseNamespace() });
    }
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        (*inst)
            .package_namespace_cache
            .get(package)
            .map(|(_, env)| *env)
    })
}

pub(crate) fn package_arg_values(package_arg: SEXP) -> Vec<String> {
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

pub(crate) fn list_package_data_sets(packages: &[String]) -> Vec<String> {
    let mut names = Vec::<String>::new();
    if find_package_path("datasets").is_empty()
        && (packages.is_empty() || packages.iter().any(|package| package == "datasets"))
    {
        names.extend(crate::library::datasets::topics());
    }
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
                .is_some_and(|ext| {
                    ["r", "rda", "rdata"]
                        .iter()
                        .any(|candidate| ext.eq_ignore_ascii_case(candidate))
                })
                && let Some(name) = path.file_stem().and_then(|stem| stem.to_str())
            {
                push_unique(&mut names, name.to_string());
            }
        }
    }
    names.sort();
    names
}

pub(crate) fn data_package_dirs(packages: &[String]) -> Vec<PathBuf> {
    crate::sexp::instance::with_required_current_instance(|inst| unsafe {
        if packages.is_empty() {
            return (*inst)
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
            .filter_map(|package| (*inst).path_policy.find_package_path(package))
            .collect()
    })
}

pub(crate) unsafe fn load_package_data_set(
    topic: &str,
    packages: &[String],
    target_env: SEXP,
) -> Result<bool, String> {
    unsafe {
        for package_dir in data_package_dirs(packages) {
            let data_dir = package_dir.join("data");
            let source_file = data_dir.join(format!("{topic}.R"));
            if source_file.is_file() {
                source_r_file_into_env(&source_file, target_env)?;
                return Ok(true);
            }
            let rdb = data_dir.join("Rdata.rdb");
            if rdb.is_file() {
                let scratch = crate::sexp::memory_ext::NewEnvironment(
                    R_NilValue(),
                    crate::sexp::globals::R_EmptyEnv(),
                    R_NilValue(),
                );
                let _scratch = protect(scratch);
                let base = data_dir.join("Rdata");
                eager_lazy_load_package_db(&base, scratch, &[])?;
                let sym = crate::sexp::symbol::Rf_install(
                    std::ffi::CString::new(topic).unwrap_or_default().as_ptr(),
                );
                let value = crate::sexp::envir::R_findVarInFrame(scratch, sym);
                if !value.is_null() && value != crate::sexp::globals::R_UnboundValue() {
                    crate::sexp::envir::defineVar(sym, value, target_env);
                    return Ok(true);
                }
            }

            let per_topic = [
                data_dir.join(format!("{topic}.rda")),
                data_dir.join(format!("{topic}.RData")),
            ];
            if let Some(path) = per_topic.iter().find(|path| path.is_file()) {
                super::package_data::load_file(path, target_env)?;
                return Ok(true);
            }
        }
        if find_package_path("datasets").is_empty()
            && (packages.is_empty() || packages.iter().any(|package| package == "datasets"))
        {
            let owner = crate::sexp::owner::OwnerToken::current().map_err(|e| e.to_string())?;
            let target = owner
                .sexp(target_env)
                .and_then(crate::sexp::object::Sexp::into_owned)
                .map_err(|e| e.to_string())?;
            return crate::library::datasets::load_topic(topic, target);
        }
        Ok(false)
    }
}

pub(crate) unsafe fn source_r_file_into_env(file: &Path, env: SEXP) -> Result<(), String> {
    unsafe {
        let code = std::fs::read_to_string(file)
            .map_err(|err| format!("could not read {}: {err}", file.display()))?;
        let parser_factory = crate::eval::parser::active_factory();
        let expr = crate::sexp::memory::with_arena(|arena| {
            crate::eval::parser::parse(&code, arena, parser_factory.clone())
                .map_err(|err| err.to_string())
        })?;
        let _ = crate::eval::eval::Rf_eval(expr.clone().as_raw(), env);
        Ok(())
    }
}

pub(crate) unsafe fn define_package_metadata(package: &str, package_env: SEXP) {
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
        let package_path = find_package_path(package);
        if !package_path.is_empty() {
            let path_string = Rf_mkString(CString::new(package_path).unwrap_or_default().as_ptr());
            if !path_string.is_null() {
                crate::sexp::attrib_core::setAttrib(
                    package_env,
                    Rf_install(c"path".as_ptr()),
                    path_string,
                );
            }
        }
    }
}

unsafe fn ensure_s3methods_slot(info: SEXP) {
    unsafe {
        let sym = Rf_install(c"S3methods".as_ptr());
        let existing = crate::sexp::envir::R_findVarInFrame(info, sym);
        if !existing.is_null() && existing != crate::sexp::globals::R_UnboundValue() {
            return;
        }
        let methods = Rf_allocVector3(SEXPTYPE::STRSXP, 0);
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        *INTEGER(dim) = 0;
        *INTEGER(dim).add(1) = 4;
        crate::sexp::attrib_core::setAttrib(methods, crate::sexp::attrib_core::R_DimSymbol(), dim);
        crate::sexp::envir::defineVar(sym, methods, info);
    }
}

unsafe fn record_s3_method_row(package_env: SEXP, generic: &str, class: &str, method: &str) {
    unsafe {
        let info_sym = Rf_install(c".__NAMESPACE__.".as_ptr());
        let info = crate::sexp::envir::R_findVarInFrame(package_env, info_sym);
        if info.is_null()
            || info == crate::sexp::globals::R_UnboundValue()
            || TYPEOF(info) != SEXPTYPE::ENVSXP
        {
            return;
        }
        ensure_s3methods_slot(info);
        let sym = Rf_install(c"S3methods".as_ptr());
        let old = crate::sexp::envir::R_findVarInFrame(info, sym);
        let old_n = if old.is_null() || TYPEOF(old) != SEXPTYPE::STRSXP {
            0
        } else {
            let dim =
                crate::sexp::attrib_core::getAttrib(old, crate::sexp::attrib_core::R_DimSymbol());
            if dim.is_null() || XLENGTH(dim) < 1 {
                0
            } else {
                INTEGER_ELT(dim, 0).max(0) as i64
            }
        };
        let new_n = old_n + 1;
        let methods = Rf_allocVector3(SEXPTYPE::STRSXP, new_n * 4);
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        *INTEGER(dim) = new_n as i32;
        *INTEGER(dim).add(1) = 4;
        crate::sexp::attrib_core::setAttrib(methods, crate::sexp::attrib_core::R_DimSymbol(), dim);
        if old_n > 0 {
            for col in 0..4 {
                for row in 0..old_n {
                    SET_STRING_ELT(
                        methods,
                        row + col * new_n,
                        STRING_ELT(old, row + col * old_n),
                    );
                }
            }
        }
        let put = |col: i64, text: &str| {
            let c = CString::new(text).unwrap_or_default();
            SET_STRING_ELT(methods, old_n + col * new_n, Rf_mkChar(c.as_ptr()));
        };
        put(0, generic);
        put(1, class);
        put(2, method);
        SET_STRING_ELT(
            methods,
            old_n + 3 * new_n,
            crate::sexp::globals::R_NaString(),
        );
        crate::sexp::envir::defineVar(sym, methods, info);
    }
}

/// GNU `makeNamespace` + `namespaceExport`: `.__NAMESPACE__.` holds
/// `exports` so `.isExported` / `.minimalName` / `show()` work.
unsafe fn ensure_namespace_info(
    package: &str,
    package_dir: &Path,
    package_env: SEXP,
    directives: Option<&NamespaceDirectives>,
) {
    unsafe {
        let info_sym = Rf_install(c".__NAMESPACE__.".as_ptr());
        let existing = crate::sexp::envir::R_findVarInFrame(package_env, info_sym);
        if !existing.is_null()
            && existing != crate::sexp::globals::R_UnboundValue()
            && TYPEOF(existing) == SEXPTYPE::ENVSXP
        {
            ensure_s3methods_slot(existing);
            return;
        }

        let info = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            crate::sexp::globals::R_BaseEnv(),
            R_NilValue(),
        );
        let _info = protect(info);
        let exports = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            crate::sexp::globals::R_BaseEnv(),
            R_NilValue(),
        );
        let _exports = protect(exports);

        for name in namespace_exports(directives, package_env) {
            let Ok(cname) = CString::new(name.as_str()) else {
                continue;
            };
            let symbol = Rf_install(cname.as_ptr());
            let value = crate::sexp::envir::R_findVarInFrame(package_env, symbol);
            if value.is_null() || value == crate::sexp::globals::R_UnboundValue() {
                continue;
            }
            crate::sexp::envir::defineVar(symbol, value, exports);
        }

        crate::sexp::envir::defineVar(Rf_install(c"exports".as_ptr()), exports, info);

        let spec_names = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _spec_names = protect(spec_names);
        crate::sexp::accessors::SET_STRING_ELT(
            spec_names,
            0,
            crate::sexp::constructors::Rf_mkChar(c"name".as_ptr()),
        );
        crate::sexp::accessors::SET_STRING_ELT(
            spec_names,
            1,
            crate::sexp::constructors::Rf_mkChar(c"version".as_ptr()),
        );
        let spec = crate::sexp::constructors::Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _spec = protect(spec);
        crate::sexp::accessors::SET_STRING_ELT(
            spec,
            0,
            crate::sexp::constructors::Rf_mkChar(
                CString::new(package).unwrap_or_default().as_ptr(),
            ),
        );
        crate::sexp::accessors::SET_STRING_ELT(
            spec,
            1,
            crate::sexp::constructors::Rf_mkChar(c"".as_ptr()),
        );
        crate::sexp::attrib_core::setAttrib(
            spec,
            crate::sexp::attrib_core::R_NamesSymbol(),
            spec_names,
        );
        crate::sexp::envir::defineVar(Rf_install(c"spec".as_ptr()), spec, info);
        ensure_s3methods_slot(info);

        let path = Rf_mkString(
            CString::new(package_dir.to_string_lossy().as_ref())
                .unwrap_or_default()
                .as_ptr(),
        );
        crate::sexp::envir::defineVar(Rf_install(c"path".as_ptr()), path, info);
        crate::sexp::envir::defineVar(info_sym, info, package_env);
    }
}

pub(crate) fn reject_unsupported_internal_data(
    package: &str,
    package_dir: &Path,
) -> Result<(), String> {
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

pub(crate) fn package_lazy_db_base(package_dir: &Path, package: &str) -> PathBuf {
    package_dir.join("R").join(package)
}

pub(crate) fn package_has_lazy_db(package_dir: &Path, package: &str) -> bool {
    let base = package_lazy_db_base(package_dir, package);
    base.with_extension("rdx").is_file() && base.with_extension("rdb").is_file()
}

pub(crate) fn reject_unsupported_lazyload_code(
    package: &str,
    package_dir: &Path,
) -> Result<(), String> {
    let r_dir = package_dir.join("R");
    if !r_dir.is_dir() {
        return Ok(());
    }

    let mut has_rdb = false;
    let mut has_rdx = false;
    let entries = std::fs::read_dir(&r_dir)
        .map_err(|err| format!("could not read R directory for package '{package}': {err}"))?;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        if ext.eq_ignore_ascii_case("rdb") {
            has_rdb = true;
        } else if ext.eq_ignore_ascii_case("rdx") {
            has_rdx = true;
        }
    }

    if has_rdb ^ has_rdx {
        let orphan = if has_rdb { "rdb" } else { "rdx" };
        return Err(format!(
            "package '{}' uses unsupported byte-compiled/lazyload R code {}; incomplete lazy-load database (missing .{orphan})",
            package,
            r_dir.display()
        ));
    }

    Ok(())
}

pub(crate) unsafe fn load_rds_file(path: &Path) -> Result<SEXP, String> {
    unsafe {
        let bytes = std::fs::read(path)
            .map_err(|err| format!("cannot read RDS file '{}': {err}", path.display()))?;
        let raw_vec = Rf_allocVector3(SEXPTYPE::RAWSXP, bytes.len() as R_xlen_t);
        if raw_vec.is_null() {
            return Err(format!(
                "allocation failed while reading '{}'",
                path.display()
            ));
        }
        let _raw_guard = protect(raw_vec);
        if !bytes.is_empty() {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), RAW(raw_vec), bytes.len());
        }
        Ok(crate::mainutils::serialize::R_unserialize(
            raw_vec,
            R_NilValue(),
        ))
    }
}

pub(crate) unsafe fn lazy_load_db_fetch_value(
    key: SEXP,
    datafile: SEXP,
    compressed: SEXP,
) -> Result<SEXP, String> {
    unsafe {
        let args = Rf_cons(
            key,
            Rf_cons(
                datafile,
                Rf_cons(compressed, Rf_cons(R_NilValue(), R_NilValue())),
            ),
        );
        let value = crate::mainutils::serialize::do_lazyLoadDBfetch(
            R_NilValue(),
            R_NilValue(),
            args,
            R_NilValue(),
        );
        if value.is_null() {
            Err("lazy-load database fetch returned NULL".to_string())
        } else {
            Ok(value)
        }
    }
}

pub(crate) unsafe fn eager_lazy_load_package_db(
    filebase: &Path,
    envir: SEXP,
    skip: &[&str],
) -> Result<(), String> {
    unsafe { lazy_lazy_load_package_db(filebase, envir, skip) }
}

pub(crate) unsafe fn lazy_lazy_load_package_db(
    filebase: &Path,
    envir: SEXP,
    skip: &[&str],
) -> Result<(), String> {
    unsafe {
        if envir.is_null() || TYPEOF(envir) != SEXPTYPE::ENVSXP {
            return Err("lazy-load requires a package environment".to_string());
        }

        let rdx_path = filebase.with_extension("rdx");
        let rdb_path = filebase.with_extension("rdb");
        let map = load_rds_file(&rdx_path)?;
        let _map_guard = protect(map);

        let variables = list_element_by_name(map, "variables").ok_or_else(|| {
            format!(
                "lazy-load map '{}' is missing a variables table",
                rdx_path.display()
            )
        })?;
        if TYPEOF(variables) != SEXPTYPE::VECSXP {
            return Err(format!(
                "lazy-load map '{}' has an invalid variables table",
                rdx_path.display()
            ));
        }

        let compressed = list_element_by_name(map, "compressed").unwrap_or(Rf_ScalarInteger(0));
        let datafile = Rf_mkString(
            CString::new(rdb_path.to_string_lossy().into_owned())
                .unwrap_or_default()
                .as_ptr(),
        );
        if datafile.is_null() {
            return Err(format!(
                "could not create lazy-load data path for '{}'",
                rdb_path.display()
            ));
        }
        let _datafile_guard = protect(datafile);

        let hook = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            crate::sexp::globals::R_EmptyEnv(),
            R_NilValue(),
        );
        let _hook_guard = protect(hook);
        let cache = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            crate::sexp::globals::R_EmptyEnv(),
            R_NilValue(),
        );
        let _cache_guard = protect(cache);
        crate::sexp::envir::defineVar(Rf_install(c"cache".as_ptr()), cache, hook);
        crate::sexp::envir::defineVar(Rf_install(c"datafile".as_ptr()), datafile, hook);
        crate::sexp::envir::defineVar(Rf_install(c"compressed".as_ptr()), compressed, hook);
        if let Some(refs) = list_element_by_name(map, "references") {
            let refs_env = crate::sexp::memory_ext::NewEnvironment(
                R_NilValue(),
                crate::sexp::globals::R_EmptyEnv(),
                R_NilValue(),
            );
            let _refs_guard = protect(refs_env);
            if TYPEOF(refs) == SEXPTYPE::VECSXP {
                let _ = crate::mainutils::essentials::do_list2env(
                    R_NilValue(),
                    R_NilValue(),
                    Rf_cons(refs, Rf_cons(refs_env, R_NilValue())),
                    R_NilValue(),
                );
            }
            crate::sexp::envir::defineVar(Rf_install(c"refs".as_ptr()), refs_env, hook);
        }

        let names = crate::sexp::attrib_core::getAttrib(
            variables,
            crate::sexp::attrib_core::R_NamesSymbol(),
        );

        let mut lazy_names = Vec::new();
        let mut lazy_keys = Vec::new();
        for index in 0..XLENGTH(variables) {
            let name =
                if !names.is_null() && names != R_NilValue() && TYPEOF(names) == SEXPTYPE::STRSXP {
                    string_at_or_empty(names, index)
                } else {
                    String::new()
                };
            if name.is_empty() || skip.iter().any(|candidate| *candidate == name) {
                continue;
            }
            lazy_names.push(name);
            lazy_keys.push(VECTOR_ELT(variables, index));
        }

        if lazy_names.is_empty() {
            return Ok(());
        }

        let fetch_sym = Rf_install(c"lazyLoadDBfetch".as_ptr());
        let template = Rf_cons(
            fetch_sym,
            Rf_cons(
                R_NilValue(),
                Rf_cons(datafile, Rf_cons(compressed, Rf_cons(hook, R_NilValue()))),
            ),
        );
        if !template.is_null() {
            crate::sexp::accessors::SET_TYPEOF(template, SEXPTYPE::LANGSXP.as_c_int());
        }
        let _template_guard = protect(template);

        for (index, name) in lazy_names.iter().enumerate() {
            let key = lazy_keys[index];
            let expr0 = crate::mainutils::duplicate::duplicate(template);
            let _expr0_guard = protect(expr0);
            SETCAR(CDR(expr0), key);
            let symbol = Rf_install(CString::new(name.clone()).unwrap_or_default().as_ptr());
            crate::sexp::envir::defineVar(
                symbol,
                crate::sexp::memory_ext::mkPROMISE(expr0, envir),
                envir,
            );
        }

        Ok(())
    }
}

pub(crate) unsafe fn source_package_r_files(
    package: &str,
    package_dir: &Path,
    package_env: SEXP,
) -> Result<(), String> {
    unsafe {
        let r_dir = package_dir.join("R");
        if !r_dir.is_dir() {
            return Ok(());
        }

        let description = std::fs::read_to_string(package_dir.join("DESCRIPTION"))
            .map(|content| description_fields(&content))
            .unwrap_or_default();
        let collate_files = description
            .get("Collate")
            .map(|value| description_file_list(value))
            .unwrap_or_default();

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

        if !collate_files.is_empty() {
            let mut ordered = Vec::with_capacity(files.len());
            let mut collated_names = BTreeSet::<String>::new();
            for file_name in collate_files {
                if file_name.contains('/') || file_name.contains('\\') {
                    return Err(format!(
                        "package '{}' has unsupported Collate entry '{}'",
                        package, file_name
                    ));
                }
                let file = r_dir.join(&file_name);
                if !file.is_file() {
                    return Err(format!(
                        "package '{}' Collate entry '{}' does not exist",
                        package, file_name
                    ));
                }
                collated_names.insert(file_name);
                ordered.push(file);
            }
            for file in files {
                let Some(name) = file.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                if !collated_names.contains(name) {
                    ordered.push(file);
                }
            }
            files = ordered;
        }

        // Source with the package directory installed as the resolution
        // root for relative file paths: package code evaluated at install
        // time upstream (crayon's ansi-palette.R reads
        // "tools/ansi-palettes.txt") sees the package root, not the host
        // process CWD.
        let saved_package_dir = crate::sexp::instance::with_required_current_instance(|inst| {
            (*inst)
                .loading_package_dir
                .replace(normalized_package_dir(package_dir))
        });
        let sourced: Result<(), String> =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                for file in files {
                    source_r_file_into_env(&file, package_env)?;
                }
                Ok(())
            }))
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
        crate::sexp::instance::with_required_current_instance(|inst| {
            (*inst).loading_package_dir = saved_package_dir;
        });
        sourced?;

        if package_has_lazy_db(package_dir, package) {
            let filebase = package_lazy_db_base(package_dir, package);
            eager_lazy_load_package_db(&filebase, package_env, &[".__NAMESPACE__."])?;
        }

        Ok(())
    }
}

pub(crate) unsafe fn source_package_lazy_data(
    package: &str,
    package_dir: &Path,
    package_env: SEXP,
) -> Result<Vec<String>, String> {
    unsafe {
        let data_dir = package_dir.join("data");
        if !data_dir.is_dir() {
            return Ok(Vec::new());
        }

        let rdata_base = data_dir.join("Rdata");
        if rdata_base.with_extension("rdx").is_file() {
            eager_lazy_load_package_db(&rdata_base, package_env, &[])?;
        } else if !package_declares_lazy_data(package_dir)? {
            // Ordinary source datasets belong to explicit data(..., envir=).
            // Attaching them here leaks those values into the search path.
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

        let mut names = frame_binding_names(package_env, true);
        if !rdata_base.with_extension("rdx").is_file() {
            names.retain(|name| !before.contains(name.as_str()));
        }
        names.retain(|name| !name.starts_with('.'));
        names.sort();
        names.dedup();
        Ok(names)
    }
}

pub(crate) unsafe fn define_lazy_data_names(package_env: SEXP, names: &[String]) {
    unsafe {
        let values = string_vector(names);
        if !values.is_null() {
            crate::sexp::envir::defineVar(lazy_data_names_symbol(), values, package_env);
        }
    }
}

pub(crate) unsafe fn lazy_data_names_binding(package_env: SEXP) -> Vec<String> {
    use crate::sexp::{
        object::{Sexp, SexpError, SexpResult},
        owner::{OwnerToken, RuntimeAccess, StoredOwner, with_runtime},
    };

    fn fail(error: SexpError) -> ! {
        std::panic::panic_any(RError {
            message: error.to_string(),
        })
    }
    fn lookup(
        access: &RuntimeAccess,
        environment: &Sexp<'static>,
        name: &std::ffi::CStr,
    ) -> SexpResult<Sexp<'static>> {
        access.domain().link(environment)?;
        let symbol = access
            .with_native(|owner| unsafe { owner.sexp(Rf_install(name.as_ptr()))?.into_owned() })?;
        let value = access.with_native(|owner| unsafe {
            owner
                .sexp(crate::sexp::envir::R_findVarInFrame(
                    environment.as_raw(),
                    symbol.as_raw(),
                ))?
                .into_owned()
        })?;
        if value.typeof_() == SEXPTYPE::PROMSXP {
            access.with_native(|owner| unsafe {
                owner
                    .sexp(crate::sexp::envir::forcePromise(value.as_raw()))?
                    .into_owned()
            })
        } else {
            Ok(value)
        }
    }
    fn binding_names(
        access: &RuntimeAccess,
        environment: &Sexp<'static>,
    ) -> SexpResult<Vec<String>> {
        let unbound = access.domain().unbound();
        let mut chains = Vec::new();
        chains
            .try_reserve(1)
            .map_err(|_| SexpError::AllocationFailed {
                object: "namespace lazy data names",
            })?;
        chains.push(environment.try_frame()?);
        let table = environment.try_hashtab()?;
        if !table.is_nil() {
            if table.typeof_() != SEXPTYPE::VECSXP {
                return Err(SexpError::TypeMismatch {
                    expected: "environment hash table",
                    actual: table.typeof_(),
                });
            }
            for index in 0..table.len() {
                chains
                    .try_reserve(1)
                    .map_err(|_| SexpError::AllocationFailed {
                        object: "namespace lazy data names",
                    })?;
                chains.push(table.try_vector_elt(index)?);
            }
        }
        let mut names = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for mut cell in chains {
            while !cell.is_nil() {
                seen.try_reserve(1)
                    .map_err(|_| SexpError::AllocationFailed {
                        object: "namespace lazy data names",
                    })?;
                let identity = cell
                    .allocation()?
                    .link()
                    .ok_or(SexpError::StaleAllocation)?;
                if !seen.insert(identity) {
                    return Err(SexpError::EvaluationFailed {
                        message: "cyclic namespace binding chain".into(),
                    });
                }
                if cell.try_car()? != unbound {
                    let tag = cell.try_tag()?;
                    if tag.typeof_() == SEXPTYPE::SYMSXP {
                        let name = tag.try_printname()?.try_as_string()?;
                        if name != ".packageName" && name != ".namespaceEnv" {
                            names
                                .try_reserve(1)
                                .map_err(|_| SexpError::AllocationFailed {
                                    object: "namespace lazy data names",
                                })?;
                            names.push(name);
                        }
                    }
                }
                cell = cell.try_cdr()?;
            }
        }
        names.sort();
        names.dedup();
        Ok(names)
    }

    // Retain the original namespace and physical runtime through metadata
    // lookup/force callbacks; no replacement ambient runtime may be adopted.
    let namespace = unsafe { OwnerToken::current() }
        .and_then(|owner| owner.sexp(package_env))
        .and_then(Sexp::into_owned)
        .unwrap_or_else(|error| fail(error));
    let authority = StoredOwner::from_value(&namespace).unwrap_or_else(|error| fail(error));
    let owner = authority
        .managed()
        .unwrap_or_else(|| fail(SexpError::RootUnavailable));
    let _pin = owner.pin().unwrap_or_else(|error| fail(error));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_runtime(&owner, |access| -> SexpResult<Vec<String>> {
            let info = lookup(access, &namespace, c".__NAMESPACE__.")?;
            if info.typeof_() == SEXPTYPE::ENVSXP {
                let lazy = lookup(access, &info, c"lazydata")?;
                if lazy.typeof_() == SEXPTYPE::ENVSXP {
                    return binding_names(access, &lazy);
                }
            }
            // Preserve installed packages that still expose the legacy index.
            let value = lookup(access, &namespace, c".lazyDataNames")?;
            if value.typeof_() != SEXPTYPE::STRSXP {
                return Ok(Vec::new());
            }
            let mut names = Vec::new();
            for index in 0..value.len() {
                if let Some(name) = value.try_string_value_elt(index)?
                    && !name.is_empty()
                    && name != "NA"
                {
                    names
                        .try_reserve(1)
                        .map_err(|_| SexpError::AllocationFailed {
                            object: "namespace lazy data names",
                        })?;
                    names.push(name);
                }
            }
            Ok(names)
        })
    }));
    authority
        .require_active()
        .unwrap_or_else(|error| fail(error));
    match result {
        Ok(result) => result
            .and_then(|result| result)
            .unwrap_or_else(|error| fail(error)),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct NamespaceDirectives {
    pub(crate) exports: Vec<String>,
    pub(crate) export_patterns: Vec<String>,
    pub(crate) imports: Vec<NamespaceImport>,
    pub(crate) s3_methods: Vec<S3MethodDirective>,
    pub(crate) native_libraries: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum NamespaceImport {
    All { package: String },
    From { package: String, names: Vec<String> },
}

#[derive(Clone, Debug)]
pub(crate) struct S3MethodDirective {
    generic: String,
    class: String,
    method: Option<String>,
}

pub(crate) unsafe fn populate_package_namespace(
    package: &str,
    package_dir: &Path,
    package_env: SEXP,
    loading: &mut Vec<String>,
) -> Result<Option<NamespaceDirectives>, String> {
    unsafe {
        let namespace = read_namespace_directives(package_dir)?;
        apply_description_depends(package, package_dir, package_env, loading)?;
        if let Some(directives) = namespace.as_ref() {
            if !is_builtin_package_dependency(package) {
                reject_native_namespace_directives(package, directives)?;
            }
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

pub(crate) unsafe fn apply_description_depends(
    package: &str,
    package_dir: &Path,
    package_env: SEXP,
    loading: &mut Vec<String>,
) -> Result<(), String> {
    unsafe {
        for dependency in package_depends_names(package_dir)? {
            if is_builtin_package_dependency(&dependency) {
                continue;
            }
            if loading.iter().any(|entry| entry == &dependency) {
                return Err(format!(
                    "package '{}' has cyclic Depends dependency involving '{}'",
                    package, dependency
                ));
            }

            let dependency_dir = find_package_path(&dependency);
            if dependency_dir.is_empty() {
                return Err(format!(
                    "package '{}' depends on missing package '{}'",
                    package, dependency
                ));
            }

            loading.push(dependency.clone());
            let import = NamespaceImport::All {
                package: dependency.clone(),
            };
            let result = import_namespace_bindings(
                package,
                package_env,
                Path::new(&dependency_dir),
                &import,
                loading,
            );
            loading.pop();
            result?;
        }
        Ok(())
    }
}

/// GNU `importFrom(methods, show)` copies the live generic. Builtin
/// packages are not loaded from a library tree, but their namespaces
/// are already in the session cache after `require(methods)`.
unsafe fn bind_cached_builtin_imports(
    package_env: SEXP,
    import: &NamespaceImport,
) -> Result<(), String> {
    unsafe {
        let import_package = match import {
            NamespaceImport::All { package } | NamespaceImport::From { package, .. } => {
                package.as_str()
            }
        };
        let Some(import_env) = cached_namespace_by_name(import_package) else {
            return Ok(());
        };
        let imported_names = match import {
            NamespaceImport::All { .. } => {
                let names = namespace_info_export_names(import_env);
                if names.is_empty() {
                    namespace_exports(None, import_env)
                } else {
                    names
                }
            }
            NamespaceImport::From { names, .. } => names.clone(),
        };
        for name in imported_names {
            let Ok(symbol_name) = CString::new(name.as_str()) else {
                continue;
            };
            let symbol = Rf_install(symbol_name.as_ptr());
            let value = crate::sexp::envir::R_findVarInFrame(import_env, symbol);
            if value.is_null()
                || value == R_NilValue()
                || value == crate::sexp::globals::R_UnboundValue()
            {
                continue;
            }
            crate::sexp::envir::defineVar(symbol, value, namespace_imports_env(package_env));
        }
        Ok(())
    }
}

pub(crate) unsafe fn apply_namespace_imports(
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

            // Base-distribution packages have no extra library tree, but
            // `importFrom(methods, show)` still has to copy the live
            // generic from the cached namespace. Skip only when that
            // namespace is not in the session yet.
            if is_builtin_package_dependency(import_package) {
                bind_cached_builtin_imports(package_env, import)?;
                continue;
            }

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

pub(crate) unsafe fn import_namespace_bindings(
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
                crate::sexp::envir::defineVar(symbol, value, namespace_imports_env(package_env));
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
unsafe fn namespace_imports_env(package_env: SEXP) -> SEXP {
    unsafe {
        let parent = crate::sexp::accessors::ENCLOS(package_env);
        let marker = Rf_install(c".__imports__.".as_ptr());
        if !parent.is_null()
            && parent != R_NilValue()
            && TYPEOF(parent) == SEXPTYPE::ENVSXP
            && crate::sexp::envir::R_findVarInFrame(parent, marker)
                != crate::sexp::globals::R_UnboundValue()
        {
            return parent;
        }
        let imports = crate::sexp::memory_ext::NewEnvironment(
            R_NilValue(),
            if parent.is_null() {
                crate::sexp::envir::R_BaseNamespace()
            } else {
                parent
            },
            R_NilValue(),
        );
        crate::sexp::envir::defineVar(marker, Rf_ScalarLogical(1), imports);
        crate::sexp::accessors::SET_ENCLOS(package_env, imports);
        imports
    }
}

pub(crate) unsafe fn make_package_attach_env(
    package: &str,
    namespace: Option<&NamespaceDirectives>,
    package_env: SEXP,
) -> Result<SEXP, String> {
    unsafe { make_package_attach_env_inner(package, namespace, package_env, false) }
}

pub(crate) unsafe fn make_package_attach_env_lenient(
    package: &str,
    namespace: Option<&NamespaceDirectives>,
    package_env: SEXP,
) -> Result<SEXP, String> {
    unsafe { make_package_attach_env_inner(package, namespace, package_env, true) }
}

unsafe fn make_package_attach_env_inner(
    package: &str,
    namespace: Option<&NamespaceDirectives>,
    package_env: SEXP,
    skip_missing: bool,
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
        for name in namespace_info_export_names(package_env) {
            push_unique(&mut exports, name);
        }
        // GNU exportMethods also exports the per-generic method tables
        // (.__T__coef:stats4, .__M__...) onto the attached package env.
        // exportClasses copies .__C__* class metadata so cacheMetaData
        // and getClass(where="package:pkg") see the definitions.
        for name in frame_binding_names(package_env, true) {
            if name.starts_with(".__T__")
                || name.starts_with(".__M__")
                || name.starts_with(".__C__")
            {
                push_unique(&mut exports, name);
            }
        }

        // Crayon-style dynamic exports: top-level package code may
        // `assign(name, value, envir = asNamespace("pkg"))` AFTER the
        // files are sourced (its `sapply(names(builtin_styles), ...)`
        // block). Those bindings live in the namespace env but were never
        // declared as lexical assignments, so a strict frame check misses
        // them. Fall back to the full namespace-env lookup before
        // declaring an export missing.
        let mut missing = Vec::new();
        for export in exports {
            let Ok(symbol_name) = CString::new(export.as_str()) else {
                if !skip_missing {
                    missing.push(export);
                }
                continue;
            };
            let symbol = Rf_install(symbol_name.as_ptr());
            let mut value = crate::sexp::envir::R_findVarInFrame(package_env, symbol);
            if value.is_null()
                || value == R_NilValue()
                || value == crate::sexp::globals::R_UnboundValue()
            {
                value = crate::sexp::envir::R_findVar(symbol, package_env);
            }
            if package == "graphics"
                && export == "plot"
                && value == crate::sexp::globals::R_UnboundValue()
                && let Some(primitive) = crate::eval::eval::primitive_for_symbol(
                    crate::sexp::object::Sexp::from_raw_unchecked(symbol),
                )
            {
                value = primitive.as_raw();
            }
            if value.is_null()
                || value == R_NilValue()
                || value == crate::sexp::globals::R_UnboundValue()
            {
                if !skip_missing {
                    missing.push(export);
                }
                continue;
            }
            crate::sexp::envir::defineVar(symbol, value, attach_env);
        }

        if package == "methods" {
            bind_methods_base_primitives(package_env);
            purge_missing_arg_placeholders(package_env);
            retarget_methods_generics(package_env);
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

pub(crate) fn read_namespace_directives(
    package_dir: &Path,
) -> Result<Option<NamespaceDirectives>, String> {
    let owner = unsafe { crate::sexp::owner::OwnerToken::current() }.ok();
    let _pin = owner
        .map(|owner| owner.pin())
        .transpose()
        .map_err(|error| error.to_string())?;
    if let Some(owner) = owner
        && let Some(hit) = unsafe {
            (*owner.as_ptr())
                .namespace_directives_cache
                .get(package_dir)
                .cloned()
        }
    {
        return Ok(Some(hit));
    }
    let namespace = package_dir.join("NAMESPACE");
    if !namespace.is_file() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&namespace)
        .map_err(|err| format!("could not read {}: {err}", namespace.display()))?;
    let content = if parse_namespace_calls(&strip_namespace_comments(&content))
        .iter()
        .any(|(name, _)| name == "if")
    {
        namespace_conditions::selected_source(&content)?
    } else {
        content
    };
    let directives = parse_namespace_directives(&content);
    if let Some(owner) = owner {
        owner.require_active().map_err(|error| error.to_string())?;
        unsafe {
            (*owner.as_ptr())
                .namespace_directives_cache
                .insert(package_dir.to_owned(), directives.clone());
        }
    }
    Ok(Some(directives))
}

/// `::` checks exports on every lookup. Parsing `NAMESPACE` again each time
/// dominates `UseMethod` on bytecode. The file is immutable for a session.
pub(crate) fn cached_namespace_directives(package_dir: &Path) -> Option<NamespaceDirectives> {
    read_namespace_directives(package_dir).ok().flatten()
}

/// Public `::` export check. Directive strings are cached; `exportPattern`
/// is tested against this name and the live frame, because the first lookup
/// often happens before the namespace frame is full.
pub(crate) unsafe fn namespace_exports_contains(
    package_dir: &Path,
    package_env: SEXP,
    name: &str,
) -> bool {
    unsafe {
        let Some(directives) = cached_namespace_directives(package_dir) else {
            return frame_binding_names(package_env, false)
                .iter()
                .any(|n| n == name);
        };
        if directives.exports.iter().any(|export| export == name) {
            return true;
        }
        if !directives
            .export_patterns
            .iter()
            .any(|pattern| simple_namespace_pattern_matches(pattern, name))
        {
            return false;
        }
        let c_name = CString::new(name).unwrap_or_else(|_| CString::new("").unwrap());
        let value = crate::sexp::envir::R_findVarInFrame(package_env, Rf_install(c_name.as_ptr()));
        !value.is_null() && value != crate::sexp::globals::R_UnboundValue()
    }
}

pub(crate) fn parse_namespace_directives(content: &str) -> NamespaceDirectives {
    let mut directives = NamespaceDirectives::default();
    let uncommented = strip_namespace_comments(content);
    for (directive, args) in parse_namespace_calls(&uncommented) {
        match directive.as_str() {
            "export" | "exportMethods" => {
                for name in split_namespace_args(&args)
                    .into_iter()
                    .filter_map(clean_namespace_name)
                {
                    push_unique(&mut directives.exports, name);
                }
            }
            "exportClasses" => {
                for name in split_namespace_args(&args)
                    .into_iter()
                    .filter_map(clean_namespace_name)
                {
                    push_unique(&mut directives.exports, format!(".__C__{name}"));
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

pub(crate) fn reject_native_namespace_directives(
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

pub(crate) unsafe fn register_namespace_s3_methods(
    package: &str,
    package_env: SEXP,
    directives: &NamespaceDirectives,
) -> Result<(), String> {
    unsafe {
        for method in &directives.s3_methods {
            // Upstream loadNamespace strips a "pkg::" qualifier from the
            // generic when deriving the default method function name:
            // S3method(utils::.DollarNames, R6) registers the function
            // `.DollarNames.R6` for generic `.DollarNames`
            // (base/R/namespace.R: paste0(sub("^.*::", "", generic), ".", class)).
            let local_generic = method.generic.rsplit("::").next().unwrap_or("");
            if local_generic.is_empty() {
                continue;
            }
            let method_name = method
                .method
                .clone()
                .unwrap_or_else(|| format!("{}.{}", local_generic, method.class));
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
                // GNU registerS3methods() skips methods that are not bound
                // in the namespace (Windows-only utils methods on Unix).
                continue;
            }
            // GNU registers namespace methods lazily. Dispatch forces the
            // promise; initialization must not deserialize every method body.
            define_s3_method(package_env, local_generic, &method.class, method_value)?;
            record_s3_method_row(package_env, local_generic, &method.class, &method_name);

            // GNU registerS3method: methods for a closure generic live in
            // environment(fdef); primitives use .BaseNamespaceEnv.
            if let Ok(generic_cstr) = CString::new(local_generic) {
                let generic_sym = Rf_install(generic_cstr.as_ptr());
                let fdef = crate::sexp::envir::findFun(generic_sym, package_env);
                if is_function_value(fdef) {
                    let defenv = if TYPEOF(fdef) == SEXPTYPE::CLOSXP {
                        crate::sexp::accessors::CLOENV(fdef)
                    } else {
                        crate::sexp::globals::R_BaseEnv()
                    };
                    if !defenv.is_null()
                        && TYPEOF(defenv) == SEXPTYPE::ENVSXP
                        && defenv != package_env
                    {
                        define_s3_method(defenv, local_generic, &method.class, method_value)?;
                    }
                }
                let base_fdef =
                    crate::sexp::envir::findFun(generic_sym, crate::sexp::globals::R_BaseEnv());
                if TYPEOF(base_fdef) == SEXPTYPE::BUILTINSXP
                    || TYPEOF(base_fdef) == SEXPTYPE::SPECIALSXP
                {
                    define_s3_method(
                        crate::sexp::globals::R_BaseEnv(),
                        local_generic,
                        &method.class,
                        method_value,
                    )?;
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn strip_namespace_comments(content: &str) -> String {
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

pub(crate) fn parse_namespace_calls(content: &str) -> Vec<(String, String)> {
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

pub(crate) fn find_namespace_call_args(content: &str, open_idx: usize) -> Option<(usize, &str)> {
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

pub(crate) fn split_namespace_args(args: &str) -> Vec<&str> {
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

pub(crate) fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

pub(crate) unsafe fn namespace_exports(
    namespace: Option<&NamespaceDirectives>,
    package_env: SEXP,
) -> Vec<String> {
    unsafe {
        let Some(directives) = namespace else {
            return frame_binding_names(package_env, false);
        };

        let mut exports = directives.exports.clone();
        for name in frame_binding_names(package_env, true) {
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

unsafe fn namespace_info_export_names(package_env: SEXP) -> Vec<String> {
    unsafe {
        let mut info = crate::sexp::envir::R_findVarInFrame(
            package_env,
            Rf_install(c".__NAMESPACE__.".as_ptr()),
        );
        if TYPEOF(info) == SEXPTYPE::PROMSXP {
            info = crate::sexp::envir::forcePromise(info);
        }
        if info.is_null()
            || info == crate::sexp::globals::R_UnboundValue()
            || TYPEOF(info) != SEXPTYPE::ENVSXP
        {
            return Vec::new();
        }
        export_env_binding_names(info)
    }
}

unsafe fn export_env_binding_names(info: SEXP) -> Vec<String> {
    unsafe {
        let mut exports =
            crate::sexp::envir::R_findVarInFrame(info, Rf_install(c"exports".as_ptr()));
        if TYPEOF(exports) == SEXPTYPE::PROMSXP {
            exports = crate::sexp::envir::forcePromise(exports);
        }
        if exports.is_null()
            || exports == crate::sexp::globals::R_UnboundValue()
            || TYPEOF(exports) != SEXPTYPE::ENVSXP
        {
            return Vec::new();
        }
        frame_binding_names(exports, true)
    }
}

pub(crate) unsafe fn frame_binding_names(env: SEXP, include_hidden: bool) -> Vec<String> {
    unsafe {
        let mut names = Vec::new();
        collect_binding_names(FRAME(env), include_hidden, &mut names);
        let hashtab = HASHTAB(env);
        if !hashtab.is_null() && hashtab != R_NilValue() && TYPEOF(hashtab) == SEXPTYPE::VECSXP {
            for i in 0..XLENGTH(hashtab) {
                collect_binding_names(VECTOR_ELT(hashtab, i), include_hidden, &mut names);
            }
        }
        names.sort();
        names.dedup();
        names
    }
}

unsafe fn collect_binding_names(mut frame: SEXP, include_hidden: bool, names: &mut Vec<String>) {
    unsafe {
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
    }
}

pub(crate) fn simple_namespace_pattern_matches(pattern: &str, name: &str) -> bool {
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

pub(crate) fn clean_namespace_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let quoted = (trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2)
        || (trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2);
    let inner = if quoted {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed.trim_matches('`').trim()
    };
    if inner.is_empty() {
        return None;
    }
    Some(if quoted {
        unescape_r_string(inner)
    } else {
        inner.to_string()
    })
}

fn unescape_r_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// GNU `exportPattern("^\\.__C__")` / `.__M__` / `.__T__`: class and method
/// metadata live in the methods namespace and must also be visible on the
/// attached `package:methods` env so `findClass` / `.findAll` / `exists(...,
/// inherits=FALSE)` see them. Without this, `setClass(..., contains="signature")`
/// copies `.__C__signature` into `.GlobalEnv`, and a later `removeClass` of
/// global classes uncaches `initialize,signature`.
unsafe fn export_s4_metadata_to_package_env(namespace: SEXP, attach_env: SEXP) {
    unsafe {
        if namespace.is_null()
            || attach_env.is_null()
            || namespace == R_NilValue()
            || attach_env == R_NilValue()
        {
            return;
        }
        for name in frame_binding_names(namespace, true) {
            if !(name.starts_with(".__C__")
                || name.starts_with(".__M__")
                || name.starts_with(".__T__"))
            {
                continue;
            }
            let Ok(cname) = CString::new(name.as_str()) else {
                continue;
            };
            let symbol = Rf_install(cname.as_ptr());
            let value = crate::sexp::envir::R_findVarInFrame(namespace, symbol);
            if value.is_null()
                || value == R_NilValue()
                || value == crate::sexp::globals::R_UnboundValue()
            {
                continue;
            }
            crate::sexp::envir::defineVar(symbol, value, attach_env);
        }
    }
}

pub(crate) unsafe fn attach_package_env(package_env: SEXP) {
    unsafe {
        let global = crate::sexp::globals::R_GlobalEnv();
        let old_enclos = crate::sexp::accessors::ENCLOS(global);
        crate::sexp::accessors::SET_ENCLOS(global, package_env);
        crate::sexp::accessors::SET_ENCLOS(package_env, old_enclos);
        // The installed methods image already contains its namespace metadata.
        // Re-running it on package:methods walks every methods generic.
        // Still copy GNU exportPattern class/method metadata onto the
        // attached env so findClass/exists(inherits=FALSE) see .__C__*.
        if attached_search_name(package_env).as_deref() == Some("package:methods") {
            let ns = crate::sexp::envir::R_findVarInFrame(package_env, namespace_env_symbol());
            if !ns.is_null()
                && ns != crate::sexp::globals::R_UnboundValue()
                && TYPEOF(ns) == SEXPTYPE::ENVSXP
            {
                export_s4_metadata_to_package_env(ns, package_env);
            } else if let Some(ns) = cached_namespace_by_name("methods") {
                export_s4_metadata_to_package_env(ns, package_env);
            }
            return;
        }
        CACHING_ATTACHED_S4.with(|flag| {
            if flag.get() {
                return;
            }
            flag.set(true);
            cache_attached_package_metadata(package_env);
            flag.set(false);
        });
    }
}

unsafe fn attached_search_name(package_env: SEXP) -> Option<String> {
    unsafe {
        let name = crate::sexp::attrib_core::getAttrib(package_env, name_symbol());
        if TYPEOF(name) != SEXPTYPE::STRSXP || LENGTH(name) < 1 {
            return None;
        }
        let raw = CHAR(STRING_ELT(name, 0));
        if raw.is_null() {
            return None;
        }
        CStr::from_ptr(raw).to_str().ok().map(str::to_string)
    }
}

/// Try to find a demo file for a topic.
pub(crate) fn find_package_demo(topic: &str) -> String {
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
pub(crate) fn find_package_example(topic: &str) -> String {
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
pub(crate) fn real_or_default(x: SEXP, default: f64) -> f64 {
    unsafe {
        if x.is_null() || x == R_NilValue() || XLENGTH(x) < 1 {
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

pub(crate) fn base_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

pub(crate) unsafe fn tag_name(cell: SEXP) -> Option<String> {
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

pub(crate) fn real_elt_or_default(x: SEXP, i: R_xlen_t, default: f64) -> f64 {
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

pub(crate) fn numeric_elt_as_count(x: SEXP, i: R_xlen_t) -> usize {
    let value = real_elt_or_default(x, i, 0.0);
    if value.is_finite() {
        (value as i64).max(0) as usize
    } else {
        0
    }
}

pub(crate) fn named_logical_arg(args: SEXP, name: &str) -> Option<bool> {
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

pub(crate) fn logical_arg_by_name_or_position(
    args: SEXP,
    name: &str,
    position: usize,
) -> Option<bool> {
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

pub(crate) fn integer_arg_by_name_or_position(
    args: SEXP,
    name: &str,
    position: usize,
) -> Option<c_int> {
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

pub(crate) fn arg_by_name_or_position(args: SEXP, names: &[&str], position: usize) -> SEXP {
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

pub(crate) fn is_string_na(x: SEXP, i: R_xlen_t) -> bool {
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

pub(crate) fn element_coerces_to_character_na(x: SEXP, i: R_xlen_t) -> bool {
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

pub(crate) fn string_contains(text: &str, pattern: &str, ignore_case: bool) -> bool {
    fixed_find(text, pattern, ignore_case).is_some()
}

pub(crate) fn fixed_find(
    text: &str,
    pattern: &str,
    ignore_case: bool,
) -> Option<crate::mainutils::grep::RegexMatch> {
    let hay = text.as_bytes();
    let needle = pattern.as_bytes();
    if needle.is_empty() {
        return Some(crate::mainutils::grep::RegexMatch { start: 0, end: 0 });
    }
    if hay.len() < needle.len() {
        return None;
    }
    for start in 0..=(hay.len() - needle.len()) {
        let matched = needle.iter().enumerate().all(|(idx, expected)| {
            let actual = hay[start + idx];
            if ignore_case {
                actual.eq_ignore_ascii_case(expected)
            } else {
                actual == *expected
            }
        });
        if matched {
            return Some(crate::mainutils::grep::RegexMatch {
                start,
                end: start + needle.len(),
            });
        }
    }
    None
}

pub(crate) fn grep_value_matches(
    text: &str,
    pattern: &str,
    ignore_case: bool,
    perl: bool,
    fixed: bool,
) -> bool {
    if fixed {
        string_contains(text, pattern, ignore_case)
    } else if perl {
        crate::mainutils::grep::perl_find(pattern, text, ignore_case).is_some()
    } else {
        crate::mainutils::grep::ere_is_match(pattern, text, ignore_case)
    }
}

/// GNU grep.c: `fixed = TRUE` wins, warns, and then runs as a fixed match.
pub(crate) unsafe fn ignore_perl_when_fixed(perl: &mut bool, fixed: bool) {
    if fixed && *perl {
        let msg = CString::new("argument 'perl = TRUE' will be ignored").unwrap_or_default();
        unsafe {
            crate::mainutils::errors::Rf_warning(msg.as_ptr());
        }
        *perl = false;
    }
}

pub(crate) fn grep_match_indices(
    x: SEXP,
    pattern: &str,
    ignore_case: bool,
    perl: bool,
    fixed: bool,
    invert: bool,
) -> Vec<R_xlen_t> {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Vec::new();
        }
        let n = XLENGTH(x);
        let mut matches = Vec::new();
        let mut utf8_warned = false;
        for i in 0..n {
            if is_string_na(x, i) {
                if invert {
                    matches.push(i);
                }
                continue;
            }
            if !utf8_warned && TYPEOF(x) == SEXPTYPE::STRSXP {
                let elt = STRING_ELT(x, i);
                if crate::sexp::accessors::getCharCE(elt) == 2 {
                    let bytes = std::ffi::CStr::from_ptr(CHAR(elt)).to_bytes();
                    if std::str::from_utf8(bytes).is_err() {
                        utf8_warned = true;
                        let msg = format!("input string {} is invalid UTF-8\0", i + 1);
                        crate::mainutils::errors::warningcall(
                            crate::sexp::globals::R_NilValue(),
                            msg.as_ptr() as *const std::os::raw::c_char,
                        );
                    }
                }
            }
            let matched =
                grep_value_matches(&elt_to_string(x, i), pattern, ignore_case, perl, fixed);
            if if invert { !matched } else { matched } {
                matches.push(i);
            }
        }
        matches
    }
}

pub(crate) fn environment_arg_or_default(
    args: SEXP,
    names: &[&str],
    position: usize,
    default: SEXP,
) -> SEXP {
    unsafe {
        let arg = arg_by_name_or_position(args, names, position);
        if !arg.is_null() && arg != R_NilValue() && TYPEOF(arg) == SEXPTYPE::ENVSXP {
            arg
        } else {
            default
        }
    }
}

pub(crate) fn copy_vector_elt(dst: SEXP, dst_idx: R_xlen_t, src: SEXP, src_idx: R_xlen_t) {
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
            t if t == SEXPTYPE::CPLXSXP => {
                *COMPLEX(dst).add(dst_idx as usize) = *COMPLEX(src).add(src_idx as usize);
            }
            t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                SET_VECTOR_ELT(dst, dst_idx, VECTOR_ELT(src, src_idx));
            }
            t if t == SEXPTYPE::RAWSXP => {
                *RAW(dst).add(dst_idx as usize) = *RAW(src).add(src_idx as usize);
            }
            _ => {}
        }
    }
}

pub(crate) fn map_path_strings(x: SEXP, f: fn(&str) -> String) -> SEXP {
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

pub(crate) fn trim_trailing_separators(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() && !path.is_empty() {
        &path[..1]
    } else {
        trimmed
    }
}

pub(crate) fn r_basename(path: &str) -> String {
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

pub(crate) fn r_dirname(path: &str) -> String {
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
        let n = match SEXPTYPE(t) {
            SEXPTYPE::NILSXP => 0,
            SEXPTYPE::REALSXP
            | SEXPTYPE::INTSXP
            | SEXPTYPE::LGLSXP
            | SEXPTYPE::STRSXP
            | SEXPTYPE::CPLXSXP
            | SEXPTYPE::RAWSXP
            | SEXPTYPE::VECSXP
            | SEXPTYPE::EXPRSXP => XLENGTH(x),
            _ => 1,
        };
        if n == 0 {
            return String::new();
        }
        let idx = i.rem_euclid(n);

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
            } else if let Some(label) = factor_label_at(x, v) {
                label
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
        } else if t == SEXPTYPE::CPLXSXP {
            let c = *COMPLEX(x).add(idx as usize);
            if c.r.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
                || c.i.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN
            {
                "NA".to_string()
            } else {
                let imag = if c.i == 0.0 { 0.0 } else { c.i };
                if imag.is_sign_negative() {
                    format!("{}{}i", c.r, imag)
                } else {
                    format!("{}+{}i", c.r, imag)
                }
            }
        } else if t == SEXPTYPE::RAWSXP {
            format!("{:02x}", *RAW(x).add(idx as usize))
        } else if t == SEXPTYPE::STRSXP {
            let charsxp = crate::sexp::accessors::STRING_ELT(x, idx);
            if charsxp.is_null() || charsxp == crate::sexp::globals::R_NaString() {
                "NA".to_string()
            } else {
                let bytes = crate::sexp::accessors::charsxp_as_utf8(charsxp);
                String::from_utf8(bytes)
                    .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
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
        } else if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP {
            let elt = crate::sexp::accessors::VECTOR_ELT(x, idx);
            sexp_key(elt)
        } else {
            format!("{:?}", t)
        }
    }
}

/// Stable key for one SEXP, used by duplicated() on lists of rows.
pub(crate) fn sexp_key(x: SEXP) -> String {
    unsafe {
        if x.is_null() || x == crate::sexp::globals::R_NilValue() {
            return "NULL".to_string();
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::LANGSXP || t == SEXPTYPE::SYMSXP || t == SEXPTYPE::CLOSXP {
            let text = crate::mainutils::deparse::deparse1line(x, false);
            if !text.is_null() && text != crate::sexp::globals::R_NilValue() && XLENGTH(text) > 0 {
                let charsxp = crate::sexp::accessors::STRING_ELT(text, 0);
                let bytes = crate::sexp::accessors::charsxp_as_utf8(charsxp);
                return String::from_utf8_lossy(&bytes).into_owned();
            }
        }
        if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP {
            let n = XLENGTH(x);
            let mut parts = Vec::with_capacity(n as usize);
            for i in 0..n {
                parts.push(sexp_key(crate::sexp::accessors::VECTOR_ELT(x, i)));
            }
            return format!("L({})", parts.join("\x1f"));
        }
        let n = XLENGTH(x).max(1);
        let mut parts = Vec::with_capacity(n as usize);
        for i in 0..n {
            parts.push(elt_to_string(x, i));
        }
        format!("V:{}", parts.join("\x1f"))
    }
}

pub(crate) unsafe fn factor_label_at(x: SEXP, code: i32) -> Option<String> {
    unsafe {
        if code <= 0 {
            return None;
        }
        let levels =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_LevelsSymbol());
        if levels.is_null() || levels == R_NilValue() || TYPEOF(levels) != SEXPTYPE::STRSXP {
            return None;
        }
        let index = (code - 1) as R_xlen_t;
        if index >= XLENGTH(levels) {
            return None;
        }
        let charsxp = STRING_ELT(levels, index);
        if charsxp.is_null() {
            None
        } else if charsxp == crate::sexp::globals::R_NaString() {
            Some("NA".to_string())
        } else {
            Some(CStr::from_ptr(CHAR(charsxp)).to_string_lossy().into_owned())
        }
    }
}

/// `cat()` rendering: GNU print digits for real/complex/raw. paste/as.character
/// keep using `elt_to_string` (15-digit-style conversion).
pub(crate) fn cat_elt_to_string(x: SEXP, i: R_xlen_t) -> String {
    unsafe {
        let t = TYPEOF(x);
        let n = XLENGTH(x);
        if n <= 0 {
            return elt_to_string(x, i);
        }
        let idx = i.rem_euclid(n);
        if t == SEXPTYPE::REALSXP {
            crate::sexp::output::format_cat_real(*REAL(x).add(idx as usize))
        } else if t == SEXPTYPE::CPLXSXP {
            crate::sexp::output::format_complex_value(*COMPLEX(x).add(idx as usize))
        } else if t == SEXPTYPE::RAWSXP {
            crate::sexp::output::format_raw_value(*RAW(x).add(idx as usize))
        } else {
            elt_to_string(x, i)
        }
    }
}

pub(crate) fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub(crate) fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

pub(crate) fn days_from_civil(mut year: i64, month: i64, day: i64) -> i64 {
    year -= if month <= 2 { 1 } else { 0 };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * month_prime + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub(crate) fn civil_from_days(mut days: i64) -> (i64, i64, i64) {
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
    if text == "Inf" || text == "+Inf" {
        return Some(f64::INFINITY);
    }
    if text == "-Inf" {
        return Some(f64::NEG_INFINITY);
    }
    if text.eq_ignore_ascii_case("NaN") {
        return Some(f64::NAN);
    }
    // GNU accepts '/' when it is the only separator.
    let sep = if text.contains('/') && !text.contains('-') {
        '/'
    } else {
        '-'
    };
    let mut parts = text.split(sep);
    let year = parts.next()?.parse::<i64>().ok()?;
    let month = parts.next()?.parse::<i64>().ok()?;
    let day_tok = parts.next()?;
    let (day_digits, rest) = day_tok
        .split_once(char::is_whitespace)
        .unwrap_or((day_tok, ""));
    let day = day_digits.parse::<i64>().ok()?;
    let rest = rest.trim();
    if !rest.is_empty() && rest.split(':').any(|piece| piece.parse::<u32>().is_err()) {
        return None;
    }
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
    if days.to_bits() == crate::sexp::ffi::R_NA_BIT_PATTERN {
        return Some("NA".to_string());
    }
    if days.is_nan() {
        return Some("NaN".to_string());
    }
    if days.is_infinite() {
        return Some(if days > 0.0 { "Inf" } else { "-Inf" }.to_string());
    }
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
    if text == "Inf" || text == "+Inf" {
        return Some(f64::INFINITY);
    }
    if text == "-Inf" {
        return Some(f64::NEG_INFINITY);
    }
    if text.eq_ignore_ascii_case("NaN") {
        return Some(f64::NAN);
    }
    let mut fields = text.split_whitespace();
    let date = fields.next()?;
    let time = fields.next().unwrap_or("00:00:00");
    if fields.next().is_some() {
        return None;
    }
    let days = parse_iso_date_days(date)?;
    if !days.is_finite() {
        return Some(days);
    }
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

pub(crate) fn elt_real_safe(x: SEXP, i: R_xlen_t) -> f64 {
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

#[cfg(test)]
mod methods_startup_tests {
    use super::*;

    #[test]
    fn stats_only_namespace_startup_registers_method_table_and_survives_gc() {
        let mut session = crate::sexp::session::RSession::new_without_default_packages();
        // This is an actual installed-package startup test. The base-only
        // collector fixture intentionally has no host paths, so opt in to
        // the same library discovery policy as an ordinary desktop session.
        session.set_library_paths(
            crate::mainutils::paths::RuntimePathPolicy::from_env()
                .library_paths()
                .to_vec(),
        );
        session.with_active(|| unsafe {
            let factory = session.owner_token().unwrap().node_factory();
            let namespace = factory
                .wrap(
                    load_package_namespace_by_name("stats")
                        .expect("stats namespace must load independently of default attachments"),
                )
                .unwrap();
            assert_eq!(namespace.typeof_(), SEXPTYPE::ENVSXP);
            let table = factory
                .wrap(crate::sexp::envir::R_findVarInFrame(
                    namespace.as_raw(),
                    Rf_install(c".__S3MethodsTable__.".as_ptr()),
                ))
                .unwrap();
            assert_eq!(table.typeof_(), SEXPTYPE::ENVSXP);
            let method_symbol = factory.wrap(Rf_install(c"predict.lm".as_ptr())).unwrap();
            let method =
                crate::sexp::envir::find_var_in_frame_result(table.clone(), method_symbol.clone())
                    .unwrap()
                    .expect("stats must register predict.lm");
            assert!(is_function_value(method.as_raw()));
            session.owner_token().unwrap().full_gc().unwrap();
            assert_eq!(
                crate::sexp::envir::find_var_in_frame_result(table, method_symbol)
                    .unwrap()
                    .unwrap(),
                method
            );
            assert_eq!(
                factory
                    .wrap(cached_namespace_by_name("stats").unwrap())
                    .unwrap(),
                namespace
            );
        });
    }

    #[test]
    fn methods_source_guard_prevents_recompilation_at_actual_closure_entry() {
        use std::{cell::Cell, rc::Rc};
        let mut session = crate::sexp::session::RSession::new_without_default_packages();
        let (allocation, _function_root) = {
            let (result, _, _) = session.eval_script_with_output_capture("function() { 1L }");
            let function = result.unwrap();
            let allocation = function.allocation().unwrap().clone();
            let root = allocation.root_lease().unwrap();
            (allocation, root)
        };
        session.with_active(|| unsafe {
            crate::eval::jit::set_R_jit_enabled(3);
            let owner = session.owner_token().unwrap();
            let function = owner
                .node_factory()
                .wrap(
                    allocation
                        .heap_identity()
                        .node_projection(&allocation)
                        .unwrap(),
                )
                .unwrap();
            crate::eval::jit::set_R_min_jit_score_in(owner.as_ptr(), 0);
            assert!(crate::eval::jit::R_cmpfun(function.as_raw()));
            let original = function.try_body().unwrap();
            assert!(original.is_bytecode());
            let seen = Rc::new(Cell::new(0));
            let observed = seen.clone();
            let inspected = function.allocation().unwrap().clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                observed.set(observed.get() + 1);
                let heap = inspected.heap_identity();
                let body = heap.node_snapshot(&inspected).unwrap().data.closure().body;
                let Some(crate::sexp::heap::ResolvedLink::Node { allocation, .. }) =
                    heap.resolve_link(body)
                else {
                    panic!("metadata source body must retain its exact language allocation");
                };
                assert_eq!(
                    heap.node_snapshot(&allocation).unwrap().sxpinfo.type_of(),
                    SEXPTYPE::LANGSXP
                );
                assert_eq!(crate::eval::jit::get_R_jit_enabled(), 0);
            }));
            {
                let _source = methods_metadata_source_body(function.as_raw()).unwrap();
                (*owner.as_ptr()).memory_state.gc_force_gap = 1;
                (*owner.as_ptr()).memory_state.gc_force_wait = 1;
                let mut call = crate::sexp::object::PairlistBuilder::new_in(owner);
                call.push(function.clone(), None).unwrap();
                let call = call.finish_as_type(SEXPTYPE::LANGSXP).unwrap();
                let raw =
                    crate::eval::eval::Rf_eval(call.as_raw(), crate::sexp::globals::R_GlobalEnv());
                let result = owner.node_factory().wrap(raw).unwrap();
                assert_eq!(result.try_integer_elt(0).unwrap(), 1);
                assert_eq!(function.try_body().unwrap().typeof_(), SEXPTYPE::LANGSXP);
                assert!(seen.get() > 0);
            }
            assert_eq!(function.try_body().unwrap(), original);
            assert_eq!(crate::eval::jit::get_R_jit_enabled(), 3);
            // The same guard restores state during unwind and preserves a
            // callback's explicit change rather than overwriting it.
            let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _source = methods_metadata_source_body(function.as_raw()).unwrap();
                crate::sexp::context::r_error("source guard unwind");
            }));
            assert!(failed.is_err());
            assert_eq!(function.try_body().unwrap(), original);
            assert_eq!(crate::eval::jit::get_R_jit_enabled(), 3);
            {
                let _source = methods_metadata_source_body(function.as_raw()).unwrap();
                crate::eval::jit::set_R_jit_enabled(2);
            }
            assert_eq!(function.try_body().unwrap(), original);
            assert_eq!(crate::eval::jit::get_R_jit_enabled(), 2);
        });
    }

    #[test]
    fn default_methods_startup_populates_metadata_and_restores_compiled_body() {
        let mut session = crate::sexp::session::RSession::new();
        let (result, output, _) = session.eval_script_with_output_capture(
            "c(.isMethodsDispatchOn(),
               !is.null(methods::getClassDef('envRefClass')),
               !is.null(methods:::.getClassesFromCache('envRefClass')),
               is.environment(methods::getClassDef('envRefClass')@refMethods))",
        );
        let metadata = result.unwrap_or_else(|error| {
            panic!("default methods metadata failed: {error:?}; output: {output:?}")
        });
        assert_eq!(metadata.len(), 4);
        for index in 0..4 {
            assert_eq!(metadata.try_logical_elt(index).unwrap(), TRUE);
        }
        drop(metadata);
        session.with_active(|| unsafe {
            let ns = cached_namespace_by_name("methods").expect("default methods namespace loaded");
            let raw =
                crate::sexp::envir::R_findVarInFrame(ns, Rf_install(c"cacheMetaData".as_ptr()));
            let raw = if TYPEOF(raw) == SEXPTYPE::PROMSXP {
                crate::sexp::envir::forcePromise(raw)
            } else {
                raw
            };
            let factory =
                crate::sexp::object::SessionNodeFactory::new(session.owner_token().unwrap());
            let function = factory.wrap(raw).unwrap();
            // Successful default startup must have restored the installed
            // bytecode, rather than permanently replacing the package image.
            let original = function.try_body().unwrap();
            assert!(original.is_bytecode());
            let original_link = factory.link(&original).unwrap();
            let before = crate::sexp::protect::R_ProtectCount();
            let jit_before = crate::eval::jit::get_R_jit_enabled();
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _source_body = methods_metadata_source_body(function.as_raw())
                    .expect("compiled cacheMetaData has its stored language expression");
                assert_eq!(function.try_body().unwrap().typeof_(), SEXPTYPE::LANGSXP);
                assert_eq!(crate::eval::jit::get_R_jit_enabled(), 0);
                assert_eq!(
                    crate::eval::jit::R_CheckJIT(function.as_raw()),
                    crate::sexp::ffi::FALSE
                );
                crate::sexp::gengc::full_gc();
                assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
                crate::sexp::context::r_error("metadata evaluation unwind regression");
            }))
            .expect_err("injected R error must unwind through body restoration");
            assert!(
                failure
                    .downcast_ref::<crate::sexp::context::RError>()
                    .is_some()
            );
            let restored = function.try_body().unwrap();
            assert_eq!(restored, original);
            assert_eq!(factory.link(&restored).unwrap(), original_link);
            assert_eq!(crate::eval::jit::get_R_jit_enabled(), jit_before);
            assert_eq!(crate::sexp::protect::R_ProtectCount(), before);
            crate::sexp::gengc::full_gc();
            assert!(function.try_body().unwrap().is_bytecode());
        });
    }
}

#[cfg(test)]
#[path = "shared/implicit_generics_tests.rs"]
mod implicit_generics_tests;

#[cfg(test)]
#[path = "shared/methods_cache_tests.rs"]
mod methods_cache_tests;
