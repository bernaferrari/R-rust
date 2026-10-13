use super::*;

/// Build a fresh argument list for `c.<class>()` with every element passed
/// through `convert` (GNU's `lapply(list(...), function(e) ...)`) and the
/// `recursive` formal dropped. Tags are preserved.
unsafe fn convert_c_args(args: SEXP, rho: SEXP, convert: &dyn Fn(SEXP) -> SEXP) -> SEXP {
    unsafe {
        let mut out = R_NilValue();
        let mut tail: SEXP = std::ptr::null_mut();
        let mut guards: Vec<crate::sexp::protect::ProtectGuard<'static>> = Vec::new();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let skip = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag))).to_string_lossy() == "recursive"
            } else {
                false
            };
            if !skip {
                let v = crate::eval::eval::Rf_eval(CAR(cell), rho);
                guards.push(protect(v));
                let converted = convert(v);
                guards.push(protect(converted));
                let node = Rf_cons(converted, R_NilValue());
                guards.push(protect(node));
                if !tag.is_null() && tag != R_NilValue() {
                    SETTAG(node, tag);
                }
                if out == R_NilValue() {
                    out = node;
                } else {
                    SETCDR(tail, node);
                }
                tail = node;
            }
            cell = CDR(cell);
        }

        out
    }
}

/// GNU `c.Date(...)`:
/// `.Date(c(unlist(lapply(list(...), function(e) unclass(as.Date(e))))))`
pub unsafe fn do_c_Date(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let converted = convert_c_args(args, rho, &|v| {
            if sexp_has_class(v, "Date") {
                v
            } else {
                do_as_Date(call, op, Rf_cons(v, R_NilValue()), rho)
            }
        });
        let _c = protect(converted);
        let r = crate::mainutils::bind::do_c_dflt(call, op, converted, rho);
        let _r = protect(r);
        drop_empty_names(r);
        set_single_class(r, "Date");
        r
    }
}

/// GNU `c.POSIXct(...)`:
/// `x <- lapply(list(...), function(e) unclass(as.POSIXct(e)))`;
/// `tz <- if(length(unique(tzones)) == 1L) tzones[[1L]] else NULL`;
/// `.POSIXct(c(unlist(x)), tz)`
pub unsafe fn do_c_POSIXct(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let converted = convert_c_args(args, rho, &|v| {
            if sexp_has_class(v, "POSIXct") {
                v
            } else {
                do_as_POSIXct(call, op, Rf_cons(v, R_NilValue()), rho)
            }
        });
        let _c = protect(converted);

        // Collect the tzone attribute of every converted element; the result
        // carries it only when the list is non-empty and all agree.
        let mut tzones: Vec<Option<String>> = Vec::new();
        let mut cell = converted;
        while !cell.is_null() && cell != R_NilValue() {
            let attr = crate::sexp::attrib_core::getAttrib(
                CAR(cell),
                crate::sexp::symbol::Rf_install(c"tzone".as_ptr()),
            );
            let this = if !attr.is_null()
                && attr != R_NilValue()
                && TYPEOF(attr) == SEXPTYPE::STRSXP
                && XLENGTH(attr) > 0
            {
                let ch = STRING_ELT(attr, 0);
                if ch.is_null() {
                    None
                } else {
                    Some(
                        std::ffi::CStr::from_ptr(CHAR(ch))
                            .to_string_lossy()
                            .into_owned(),
                    )
                }
            } else {
                None
            };
            tzones.push(this);
            cell = CDR(cell);
        }
        let tz = if let Some(first) = tzones.first()
            && first.is_some()
            && tzones.iter().all(|t| t == first)
        {
            first.clone()
        } else {
            None
        };

        let r = crate::mainutils::bind::do_c_dflt(call, op, converted, rho);
        let _r = protect(r);
        drop_empty_names(r);
        let class = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        let _class = protect(class);
        SET_STRING_ELT(class, 0, Rf_mkChar(c"POSIXct".as_ptr()));
        SET_STRING_ELT(class, 1, Rf_mkChar(c"POSIXt".as_ptr()));
        crate::sexp::attrib_core::setAttrib(r, crate::sexp::attrib_core::R_ClassSymbol(), class);
        if let Some(tz) = tz {
            let tzone = Rf_mkString(CString::new(tz.as_str()).unwrap_or_default().as_ptr());
            let _t = protect(tzone);
            crate::sexp::attrib_core::setAttrib(
                r,
                crate::sexp::symbol::Rf_install(c"tzone".as_ptr()),
                tzone,
            );
        }
        r
    }
}

fn drop_empty_names(x: SEXP) {
    unsafe {
        let names =
            crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if !names_have_label(names) {
            crate::sexp::attrib_core::setAttrib(
                x,
                crate::sexp::attrib_core::R_NamesSymbol(),
                R_NilValue(),
            );
        }
    }
}

fn names_have_label(names: SEXP) -> bool {
    unsafe {
        if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
            return false;
        }
        for i in 0..XLENGTH(names) {
            let ch = STRING_ELT(names, i);
            if ch.is_null() || ch == crate::sexp::globals::R_NaString() {
                continue;
            }
            if !std::ffi::CStr::from_ptr(CHAR(ch)).to_bytes().is_empty() {
                return true;
            }
        }
        false
    }
}

/// GNU `c.POSIXlt(...)`.
pub unsafe fn do_c_POSIXlt(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut converted = R_NilValue();
        let mut fracs: Vec<f64> = Vec::new();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            let v = crate::eval::eval::Rf_eval(CAR(cell), rho);
            let _v = protect(v);
            let tag = TAG(cell);
            let skip = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag))).to_string_lossy() == "recursive"
            } else {
                false
            };
            if !skip {
                let v = if crate::mainutils::objects::inherits2(v, c"factor".as_ptr()) != 0 {
                    // GNU c.POSIXlt: factor goes through as.character().
                    let levels = crate::sexp::attrib_core::getAttrib(
                        v,
                        crate::eval::attrib_core::R_LevelsSymbol(),
                    );
                    let n = XLENGTH(v);
                    let text = Rf_allocVector3(SEXPTYPE::STRSXP, n);
                    let _t = protect(text);
                    for i in 0..n {
                        let code = INTEGER_ELT(v, i as i32);
                        let s = if code == crate::sexp::ffi::NA_INTEGER || code < 1 {
                            crate::sexp::globals::R_NaString()
                        } else {
                            STRING_ELT(levels, (code - 1) as R_xlen_t)
                        };
                        SET_STRING_ELT(text, i, s);
                    }
                    let lt = crate::mainutils::datetime::do_as_POSIXlt(
                        call,
                        op,
                        Rf_cons(text, R_NilValue()),
                        rho,
                    );
                    let _ltv = protect(lt);
                    lt
                } else if TYPEOF(v) == SEXPTYPE::STRSXP {
                    let lt = crate::mainutils::datetime::do_as_POSIXlt(
                        call,
                        op,
                        Rf_cons(v, R_NilValue()),
                        rho,
                    );
                    let _ltv = protect(lt);
                    lt
                } else {
                    v
                };
                let ct = if sexp_has_class(v, "POSIXlt") && TYPEOF(v) == SEXPTYPE::VECSXP {
                    let nobs = crate::mainutils::subassign::posixlt_obs_length(v);
                    let sec = if XLENGTH(v) > 0 {
                        VECTOR_ELT(v, 0)
                    } else {
                        R_NilValue()
                    };
                    let nsec = if sec.is_null() || sec == R_NilValue() {
                        0
                    } else {
                        XLENGTH(sec)
                    };
                    for i in 0..nobs {
                        let s = if nsec == 0 {
                            0.0
                        } else if TYPEOF(sec) == SEXPTYPE::REALSXP {
                            *REAL(sec).add((i % nsec) as usize)
                        } else if TYPEOF(sec) == SEXPTYPE::INTSXP {
                            let iv = *INTEGER(sec).add((i % nsec) as usize);
                            if iv == NA_INTEGER { NA_REAL } else { iv as f64 }
                        } else {
                            0.0
                        };
                        fracs.push(if s.is_finite() { s - s.floor() } else { 0.0 });
                    }
                    let dup = crate::mainutils::duplicate::Rf_duplicate(v);
                    let _d = protect(dup);
                    if XLENGTH(dup) > 0 {
                        let dsec = VECTOR_ELT(dup, 0);
                        if TYPEOF(dsec) == SEXPTYPE::REALSXP {
                            for i in 0..XLENGTH(dsec) {
                                let s = *REAL(dsec).add(i as usize);
                                if s.is_finite() {
                                    *REAL(dsec).add(i as usize) = s.floor();
                                }
                            }
                        }
                    }
                    do_as_POSIXct(call, op, Rf_cons(dup, R_NilValue()), rho)
                } else if sexp_has_class(v, "POSIXct")
                    && (TYPEOF(v) == SEXPTYPE::REALSXP || TYPEOF(v) == SEXPTYPE::INTSXP)
                {
                    fracs.extend(std::iter::repeat_n(0.0, XLENGTH(v) as usize));
                    v
                } else {
                    let ct = do_as_POSIXct(call, op, Rf_cons(v, R_NilValue()), rho);
                    fracs.extend(std::iter::repeat_n(0.0, XLENGTH(ct) as usize));
                    ct
                };
                let _ct_one = protect(ct);
                let node = Rf_cons(ct, converted);
                SETTAG(node, tag);
                converted = node;
                let _converted = protect(converted);
            }
            cell = CDR(cell);
        }

        let mut rev = R_NilValue();
        let mut c = converted;
        while !c.is_null() && c != R_NilValue() {
            let node = Rf_cons(CAR(c), rev);
            SETTAG(node, TAG(c));
            rev = node;
            c = CDR(c);
        }
        let _a = protect(rev);
        let ct = do_c_POSIXct(call, op, rev, rho);
        let _ct = protect(ct);
        let mut any_tag = false;
        let mut cell = rev;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            if !tag.is_null() && tag != R_NilValue() {
                let s = std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)));
                if !s.to_bytes().is_empty() {
                    any_tag = true;
                    break;
                }
            }
            cell = CDR(cell);
        }
        if any_tag && XLENGTH(ct) > 0 {
            let nm = Rf_allocVector3(SEXPTYPE::STRSXP, XLENGTH(ct));
            let _nm = protect(nm);
            let blank = Rf_mkChar(c"".as_ptr());
            let mut i: i64 = 0;
            let mut cell = rev;
            while !cell.is_null() && cell != R_NilValue() && i < XLENGTH(ct) {
                let tag = TAG(cell);
                let ch = if !tag.is_null() && tag != R_NilValue() {
                    PRINTNAME(tag)
                } else {
                    blank
                };
                let nv = XLENGTH(CAR(cell)).max(1);
                for _ in 0..nv {
                    if i >= XLENGTH(ct) {
                        break;
                    }
                    SET_STRING_ELT(nm, i, ch);
                    i += 1;
                }
                cell = CDR(cell);
            }
            crate::sexp::attrib_core::setAttrib(ct, crate::sexp::attrib_core::R_NamesSymbol(), nm);
        }

        let lt =
            crate::mainutils::datetime::do_as_POSIXlt(call, op, Rf_cons(ct, R_NilValue()), rho);
        let _lt = protect(lt);

        let names =
            crate::sexp::attrib_core::getAttrib(ct, crate::sexp::attrib_core::R_NamesSymbol());
        if TYPEOF(lt) == SEXPTYPE::VECSXP && XLENGTH(lt) >= 6 {
            let year = VECTOR_ELT(lt, 5);
            if names_have_label(names) {
                crate::sexp::attrib_core::setAttrib(
                    year,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    names,
                );
            } else {
                crate::sexp::attrib_core::setAttrib(
                    year,
                    crate::sexp::attrib_core::R_NamesSymbol(),
                    R_NilValue(),
                );
            }
        }
        if TYPEOF(lt) == SEXPTYPE::VECSXP && XLENGTH(lt) > 0 {
            let sec = VECTOR_ELT(lt, 0);
            if TYPEOF(sec) == SEXPTYPE::REALSXP {
                let n = XLENGTH(sec).min(fracs.len() as i64);
                for i in 0..n {
                    let s = *REAL(sec).add(i as usize);
                    let frac = fracs[i as usize];
                    if s.is_finite() && frac != 0.0 {
                        *REAL(sec).add(i as usize) = s + frac;
                    }
                }
            }
        }

        lt
    }
}

/// GNU `is.numeric.Date` / `is.numeric.POSIXt`.
pub unsafe fn do_is_numeric_Date(_call: SEXP, _op: SEXP, _args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { Rf_ScalarLogical(FALSE) }
}

/// GNU `xtfrm.Date(x)` is `as.numeric(x)`.
pub unsafe fn do_xtfrm_Date(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_allocVector3(SEXPTYPE::REALSXP, 0);
        }
        let x = if TYPEOF(x) == SEXPTYPE::VECSXP
            && crate::mainutils::essentials::sexp_has_class(x, "POSIXlt")
        {
            crate::mainutils::datetime::convert_posixlt_to_posixct(x, "UTC")
        } else {
            x
        };
        let n = XLENGTH(x);
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _r = protect(result);
        for i in 0..n {
            *REAL(result).add(i as usize) = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                let v = *INTEGER(x).add(i as usize);
                if v == NA_INTEGER { NA_REAL } else { v as f64 }
            } else {
                NA_REAL
            };
        }
        result
    }
}

/// GNU `diff.Date(x)`.
pub unsafe fn do_diff_Date(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            let empty = Rf_allocVector3(SEXPTYPE::REALSXP, 0);
            let _e = protect(empty);
            set_single_class(empty, "difftime");
            crate::sexp::attrib_core::setAttrib(
                empty,
                crate::sexp::symbol::Rf_install(c"units".as_ptr()),
                Rf_mkString(c"days".as_ptr()),
            );
            return empty;
        }
        let n = XLENGTH(x);
        if n < 2 {
            let empty = Rf_allocVector3(SEXPTYPE::REALSXP, 0);
            let _e = protect(empty);
            set_single_class(empty, "difftime");
            crate::sexp::attrib_core::setAttrib(
                empty,
                crate::sexp::symbol::Rf_install(c"units".as_ptr()),
                Rf_mkString(c"days".as_ptr()),
            );
            return empty;
        }
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, n - 1);
        let _r = protect(result);
        for i in 0..(n - 1) {
            let a = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add(i as usize)
            } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                *INTEGER(x).add(i as usize) as f64
            } else {
                0.0
            };
            let b = if TYPEOF(x) == SEXPTYPE::REALSXP {
                *REAL(x).add((i + 1) as usize)
            } else if TYPEOF(x) == SEXPTYPE::INTSXP {
                *INTEGER(x).add((i + 1) as usize) as f64
            } else {
                0.0
            };
            *REAL(result).add(i as usize) = b - a;
        }
        set_single_class(result, "difftime");
        crate::sexp::attrib_core::setAttrib(
            result,
            crate::sexp::symbol::Rf_install(c"units".as_ptr()),
            Rf_mkString(c"days".as_ptr()),
        );
        result
    }
}

/// GNU `.Date(xx, cl = "Date")` — `class<-`(xx, cl).
pub unsafe fn do_dot_Date(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let xx = CAR(args);
        if xx.is_null() || xx == R_NilValue() {
            return R_NilValue();
        }
        let cl_arg = CADR(args);
        let cl = if cl_arg.is_null()
            || cl_arg == R_NilValue()
            || cl_arg == crate::sexp::globals::R_MissingArg()
        {
            Rf_mkString(c"Date".as_ptr())
        } else {
            cl_arg
        };
        let pair = Rf_cons(cl, R_NilValue());
        let _p = protect(pair);
        let call_args = Rf_cons(xx, pair);
        let _a = protect(call_args);
        crate::mainutils::attrib::do_classgets(_call, _op, call_args, rho)
    }
}
