//! Datetime class helpers plus $.at/xtfrm/cache_class accessors.

use super::*;
use crate::sexp::object::SexpMut;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum DatetimeVectorClass {
    Date,
    Posixct,
}

pub(crate) fn leading_datetime_class(args: SEXP) -> Option<(DatetimeVectorClass, SEXP)> {
    {
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

pub(crate) fn posixct_tzone_string(source: SEXP) -> String {
    unsafe {
        let tzone = crate::sexp::attrib_core::getAttrib(source, Rf_install(c"tzone".as_ptr()));
        if tzone.is_null() || tzone == R_NilValue() || TYPEOF(tzone) != SEXPTYPE::STRSXP {
            return String::new();
        }
        if XLENGTH(tzone) == 0 {
            return String::new();
        }
        owner_sexp(active_owner(), tzone)
            .try_string_value_elt(0)
            .ok()
            .flatten()
            .unwrap_or_default()
    }
}

pub(crate) fn set_datetime_class_from(result: SEXP, source: SEXP, class: DatetimeVectorClass) {
    {
        match class {
            DatetimeVectorClass::Date => set_single_class(result, "Date"),
            DatetimeVectorClass::Posixct => {
                set_posixct_class(result, &posixct_tzone_string(source))
            }
        }
    }
}

pub(crate) fn integer_or_logical_elt(value: SEXP, index: c_int) -> c_int {
    match TYPEOF(value) {
        t if t == SEXPTYPE::INTSXP || t == SEXPTYPE::LGLSXP => {
            integer_like_elt(&owner_sexp(active_owner(), value), index as R_xlen_t)
        }
        _ => NA_INTEGER,
    }
}

pub fn do_cache_class(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
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
        let name = match owner_sexp(active_owner(), class).try_string_value_elt(0) {
            Ok(Some(text)) => text,
            _ => {
                std::panic::panic_any(RError {
                    message: "invalid class argument to internal .class_cache".to_string(),
                });
            }
        };
        crate::mainutils::objects::cache_class(&name, CADR(args))
    }
}

pub fn do_xtfrm(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
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

                let x_owned = owner_sexp(active_owner(), x);
                let mut values = Vec::with_capacity(n as usize);
                for i in 0..n {
                    let value = x_owned
                        .try_string_value_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
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
pub fn do_at(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
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
                    && matches!(
                        owner_sexp(active_owner(), nlist).try_string_value_elt(0),
                        Ok(Some(_))
                    )));
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

pub fn do_at_set(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
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

pub(crate) fn replacement_name(arg: SEXP) -> String {
    {
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
    let cls = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_ClassSymbol());
    if cls.is_null() || cls == R_NilValue() || TYPEOF(cls) != crate::sexp::ffi::SEXPTYPE::STRSXP {
        return false;
    }
    let cls_owned = owner_sexp(active_owner(), cls);
    (0..XLENGTH(cls)).any(|i| {
        matches!(
            cls_owned.try_string_value_elt(i),
            Ok(Some(ref text)) if text == "data.frame"
        )
    })
}

pub(super) fn frame_row_count(df: SEXP) -> i64 {
    unsafe {
        let owner = active_owner();
        let rn_sym = owner_sexp(
            owner,
            crate::sexp::constructors::Rf_mkString(c"row.names".as_ptr()),
        )
        .try_string_elt(0)
        .map(|elt| elt.as_raw())
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        let rn = crate::sexp::attrib_core::getAttrib(
            df,
            crate::mainutils::subset::installTrChar(rn_sym),
        );
        let rn_owned = owner_sexp(owner, rn);
        // compact row.names: c(NA_integer_, -n)
        if !rn.is_null()
            && rn != R_NilValue()
            && TYPEOF(rn) == crate::sexp::ffi::SEXPTYPE::INTSXP
            && XLENGTH(rn) == 2
            && rn_owned
                .try_integer_elt(1)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                < 0
        {
            -(rn_owned
                .try_integer_elt(1)
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                as i64)
        } else if !rn.is_null() && rn != R_NilValue() && XLENGTH(rn) > 0 {
            XLENGTH(rn)
        } else if XLENGTH(df) > 0 {
            // fall back to the first column's length
            let first = owner_sexp(owner, df)
                .try_vector_elt(0)
                .map(|elt| elt.as_raw())
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            XLENGTH(first)
        } else {
            0
        }
    }
}

/// Copy element `from` of `src` to element `to` of `dst` for any vector type.
pub(super) fn copy_vector_element(src: SEXP, from: i64, dst: SEXP, to: i64) {
    let owner = active_owner();
    let src_owned = owner_sexp(owner, src);
    let mut dst_mut = owner
        .sexp(dst)
        .and_then(SexpMut::try_from_checked)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    fn fail<T>(error: crate::sexp::object::SexpError) -> T {
        crate::sexp::context::r_error(error.to_string())
    }
    match TYPEOF(src) {
        t if t == crate::sexp::ffi::SEXPTYPE::INTSXP => {
            let v = src_owned.try_integer_elt(from).unwrap_or_else(fail);
            dst_mut.try_set_integer_elt(to, v).unwrap_or_else(fail);
        }
        t if t == crate::sexp::ffi::SEXPTYPE::REALSXP => {
            let v = src_owned.try_real_elt(from).unwrap_or_else(fail);
            dst_mut.try_set_real_elt(to, v).unwrap_or_else(fail);
        }
        t if t == crate::sexp::ffi::SEXPTYPE::LGLSXP => {
            let v = src_owned.try_logical_elt(from).unwrap_or_else(fail);
            dst_mut.try_set_logical_elt(to, v).unwrap_or_else(fail);
        }
        t if t == crate::sexp::ffi::SEXPTYPE::STRSXP => {
            let elt = src_owned.try_string_elt(from).unwrap_or_else(fail);
            dst_mut.try_set_string_elt(to, elt).unwrap_or_else(fail);
        }
        t if t == crate::sexp::ffi::SEXPTYPE::VECSXP => {
            let elt = src_owned.try_vector_elt(from).unwrap_or_else(fail);
            dst_mut.try_set_vector_elt(to, elt).unwrap_or_else(fail);
        }
        _ => {}
    }
}

pub fn do_dollar_set(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if args.is_null() || args == R_NilValue() || CDR(args) == R_NilValue() {
            return R_NilValue();
        }
        let call_name = if !_call.is_null()
            && _call != R_NilValue()
            && TYPEOF(CAR(_call)) == SEXPTYPE::SYMSXP
        {
            charsxp_text(PRINTNAME(CAR(_call))).unwrap_or_default()
        } else {
            String::new()
        };
        if call_name == "$<-" {
            let object = crate::eval::eval::Rf_eval(CAR(args), rho);
            let _object_guard = protect(object);
            if crate::mainutils::coerce::IS_S4_OBJECT(object) != 0 {
                // The `$<-` primitive must use R_DispatchOrEvalSP for formal
                // methods, then R_subassign3_dflt for classes without a
                // method. The custom path below is for ordinary/S3 values.
                let primitive = crate::sexp::envir::R_findVarInFrame(
                    crate::sexp::globals::R_BaseEnv(),
                    Rf_install(c"$<-".as_ptr()),
                );
                let _primitive_guard = protect(primitive);
                return crate::mainutils::subassign::do_subassign3(_call, primitive, args, rho);
            }
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
        // S4 objects that extend environments store their mutable bindings
        // in the .xData environment. Keep the original S4 wrapper as the
        // assignment result while delegating the data-slot update to the
        // GNU default `$<-` implementation. The object and replacement value
        // remain protected across slot access and any runtime allocation.
        if TYPEOF(object) == SEXPTYPE::OBJSXP {
            let name = Rf_install(CString::new(field.as_str()).unwrap_or_default().as_ptr());
            return crate::mainutils::subassign::R_subassign3_dflt(_call, object, name, value);
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

        let owner = active_owner();
        let names_owned = owner_sexp(owner, names);
        let object_owned = owner_sexp(owner, object);
        if !names.is_null() && names != R_NilValue() && TYPEOF(names) == SEXPTYPE::STRSXP {
            for i in 0..n {
                let matches_field = names_owned
                    .try_string_value_elt(i)
                    .map(|v| v.as_deref() == Some(field.as_str()))
                    .unwrap_or(false);
                if matches_field {
                    let old_n = crate::mainutils::subassign::posixlt_obs_length(object);
                    let value_owned = owner
                        .sexp(value)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                    let mut object_mut = owner
                        .sexp(object)
                        .and_then(SexpMut::try_from_checked)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                    object_mut
                        .try_set_vector_elt(i, value_owned)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
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

        if let (Ok(mut out_mut), Ok(mut out_names_mut), Ok(blank_owned)) = (
            owner.sexp(out).and_then(SexpMut::try_from_checked),
            owner.sexp(out_names).and_then(SexpMut::try_from_checked),
            owner.sexp(blank),
        ) {
            for i in 0..n {
                if let Ok(elt) = object_owned.try_vector_elt(i) {
                    let _ = out_mut.try_set_vector_elt(i, elt);
                }
                let name = if !names.is_null()
                    && names != R_NilValue()
                    && TYPEOF(names) == SEXPTYPE::STRSXP
                    && i < XLENGTH(names)
                {
                    names_owned
                        .try_string_elt(i)
                        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                } else {
                    blank_owned.clone()
                };
                let _ = out_names_mut.try_set_string_elt(i, name);
            }
        }

        let value = if sexp_has_class(object, "data.frame") && !sexp_has_class(value, "AsIs") {
            let stripped = crate::mainutils::duplicate::duplicate(value);
            crate::sexp::attrib_core::setAttrib(stripped, names_sym, R_NilValue());
            stripped
        } else {
            value
        };
        if let (Ok(mut out_mut), Ok(mut out_names_mut), Ok(value_owned), Ok(field_chars)) = (
            owner.sexp(out).and_then(SexpMut::try_from_checked),
            owner.sexp(out_names).and_then(SexpMut::try_from_checked),
            owner.sexp(value),
            owner.sexp(Rf_mkChar(CString::new(field).unwrap_or_default().as_ptr())),
        ) {
            let _ = out_mut.try_set_vector_elt(n, value_owned);
            let _ = out_names_mut.try_set_string_elt(n, field_chars);
        }
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
            let rownames_owned = owner_sexp(owner, rownames);
            let rows: R_xlen_t = if rownames.is_null() || rownames == R_NilValue() {
                0
            } else if TYPEOF(rownames) == SEXPTYPE::INTSXP
                && XLENGTH(rownames) == 2
                && rownames_owned
                    .try_integer_elt(0)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                    == NA_INTEGER
            {
                -(rownames_owned
                    .try_integer_elt(1)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                    as R_xlen_t)
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
                owner_sexp(owner, dim)
                    .try_integer_elt(0)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
                    .max(0) as R_xlen_t
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
