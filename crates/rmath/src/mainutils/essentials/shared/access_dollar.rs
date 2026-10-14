//! Datetime class helpers plus $.at/xtfrm/cache_class accessors.

use super::*;

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

pub(super) fn is_data_frame_object(x: SEXP) -> bool {
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

pub(super) fn frame_row_count(df: SEXP) -> i64 {
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
pub(super) unsafe fn copy_vector_element(src: SEXP, from: i64, dst: SEXP, to: i64) {
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
            // GNU `[[<-.data.frame` measures the replacement with
            // `N <- NROW(value)`: an array value contributes its first
            // dimension, not its total cell count (a 3x2 character matrix
            // is one 3-row column, see PR#14974's data1$y <- y).
            let dim =
                crate::sexp::attrib_core::getAttrib(value, crate::sexp::attrib_core::R_DimSymbol());
            let value_len: R_xlen_t = if !dim.is_null()
                && dim != R_NilValue()
                && TYPEOF(dim) == SEXPTYPE::INTSXP
                && XLENGTH(dim) >= 1
            {
                crate::sexp::accessors::INTEGER_ELT(dim, 0).max(0) as R_xlen_t
            } else if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP {
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
