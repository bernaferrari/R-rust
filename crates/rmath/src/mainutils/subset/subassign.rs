use super::*;

// ---------------------------------------------------------------------------
// do_subassign -- the `[<-` assignment operator
// ---------------------------------------------------------------------------

/// The `[<-` assignment operator.
///
/// Dispatches to the appropriate method or falls through to default.
/// Pinned GNU `[<-.POSIXlt` (R-devel): floor `$sec` before `as.POSIXct`,
/// drop fractions when `|ct| >= 2^53`, convert to `tz`, then add the
/// fraction back onto the converted `$sec`.
unsafe fn posixlt_convert_tz(value: SEXP, value_tz: &str, x_tz: &str) -> SEXP {
    unsafe {
        let orig_sec = VECTOR_ELT(value, 0);
        if orig_sec.is_null() || TYPEOF(orig_sec) != SEXPTYPE::REALSXP {
            let ct = crate::mainutils::datetime::convert_posixlt_to_posixct(value, value_tz);
            let _ct = protect(ct);
            return crate::mainutils::datetime::convert_posixct_to_posixlt(ct, x_tz);
        }
        let ns = XLENGTH(orig_sec);
        let floored = Rf_allocVector3(SEXPTYPE::REALSXP, ns);
        let _f = protect(floored);
        let mut frac = vec![0.0; ns.max(0) as usize];
        for i in 0..ns {
            let os = *REAL(orig_sec).add(i as usize);
            if os.is_finite() {
                let fl = os.floor();
                frac[i as usize] = os - fl;
                *REAL(floored).add(i as usize) = fl;
            } else {
                *REAL(floored).add(i as usize) = os;
            }
        }
        let dup = crate::mainutils::duplicate::Rf_duplicate(value);
        let _d = protect(dup);
        SET_VECTOR_ELT(dup, 0, floored);
        let ct = crate::mainutils::datetime::convert_posixlt_to_posixct(dup, value_tz);
        let _ct = protect(ct);
        let exact = 2.0f64.powi(53);
        if TYPEOF(ct) == SEXPTYPE::REALSXP {
            let nc = XLENGTH(ct);
            if (frac.len() as R_xlen_t) != nc && nc > 0 && !frac.is_empty() {
                let mut recycled = vec![0.0; nc as usize];
                for i in 0..nc {
                    recycled[i as usize] = frac[(i as usize) % frac.len()];
                }
                frac = recycled;
            }
            for i in 0..XLENGTH(ct) {
                let v = *REAL(ct).add(i as usize);
                if v.is_finite() && v.abs() >= exact {
                    if (i as usize) < frac.len() {
                        frac[i as usize] = 0.0;
                    }
                }
            }
        }
        let converted = crate::mainutils::datetime::convert_posixct_to_posixlt(ct, x_tz);
        let _cv = protect(converted);
        if TYPEOF(converted) == SEXPTYPE::VECSXP && XLENGTH(converted) > 0 {
            let dst = VECTOR_ELT(converted, 0);
            if TYPEOF(dst) == SEXPTYPE::REALSXP && !frac.is_empty() {
                let nd = XLENGTH(dst);
                for i in 0..nd {
                    let cs = *REAL(dst).add(i as usize);
                    let s = frac[(i as usize) % frac.len()];
                    if cs.is_finite() && s != 0.0 {
                        *REAL(dst).add(i as usize) = cs + s;
                    }
                }
            }
        }
        converted
    }
}

unsafe fn subassign_posixlt_time(
    x: SEXP,
    i: SEXP,
    value: SEXP,
    call: SEXP,
    op: SEXP,
    env: SEXP,
) -> SEXP {
    unsafe {
        let ncomp = XLENGTH(x);
        let mut n: R_xlen_t = 0;
        for j in 0..ncomp {
            let len = XLENGTH(VECTOR_ELT(x, j));
            if len > n {
                n = len;
            }
        }
        let x_tz = {
            let tzone = getAttrib(x, sym_Tzone());
            if !isNull(tzone) && TYPEOF(tzone) == SEXPTYPE::STRSXP && XLENGTH(tzone) > 0 {
                let ch = STRING_ELT(tzone, 0);
                if !ch.is_null() && ch != crate::sexp::globals::R_NaString() {
                    std::ffi::CStr::from_ptr(CHAR(ch))
                        .to_string_lossy()
                        .into_owned()
                } else {
                    "UTC".to_string()
                }
            } else {
                "UTC".to_string()
            }
        };
        let rhs = if crate::mainutils::essentials::sexp_has_class(value, "POSIXlt")
            && TYPEOF(value) == SEXPTYPE::VECSXP
        {
            let value_tz = {
                let tzone = getAttrib(value, sym_Tzone());
                if !isNull(tzone) && TYPEOF(tzone) == SEXPTYPE::STRSXP && XLENGTH(tzone) > 0 {
                    let ch = STRING_ELT(tzone, 0);
                    if !ch.is_null() && ch != crate::sexp::globals::R_NaString() {
                        std::ffi::CStr::from_ptr(CHAR(ch))
                            .to_string_lossy()
                            .into_owned()
                    } else {
                        x_tz.clone()
                    }
                } else {
                    x_tz.clone()
                }
            };
            if value_tz == x_tz
                || ((value_tz == "UTC" || value_tz == "GMT") && (x_tz == "UTC" || x_tz == "GMT"))
            {
                value
            } else {
                posixlt_convert_tz(value, &value_tz, &x_tz)
            }
        } else {
            let tz_s = Rf_mkString(
                std::ffi::CString::new(x_tz.as_str())
                    .unwrap_or_default()
                    .as_ptr(),
            );
            let _z = protect(tz_s);
            crate::mainutils::datetime::do_as_POSIXlt(
                call,
                op,
                Rf_cons(value, Rf_cons(tz_s, R_NilValue())),
                env,
            )
        };

        let _rhs = protect(rhs);
        let mut rn: R_xlen_t = 0;
        let rcomp = if TYPEOF(rhs) == SEXPTYPE::VECSXP {
            XLENGTH(rhs)
        } else {
            0
        };
        for j in 0..rcomp {
            let len = XLENGTH(VECTOR_ELT(rhs, j));
            if len > rn {
                rn = len;
            }
        }
        let ans = Rf_allocVector3(SEXPTYPE::VECSXP, ncomp);
        let _a = protect(ans);
        for j in 0..ncomp {
            let col = crate::mainutils::datetime::recycle_posixlt_component(VECTOR_ELT(x, j), n);
            let _c = protect(col);
            let rcol = if j < rcomp {
                crate::mainutils::datetime::recycle_posixlt_component(VECTOR_ELT(rhs, j), rn.max(1))
            } else {
                R_NilValue()
            };
            let _r = protect(rcol);
            let assigned = if rcol.is_null() || rcol == R_NilValue() {
                col
            } else {
                let args = Rf_cons(col, Rf_cons(i, Rf_cons(rcol, R_NilValue())));
                let _g = protect(args);
                crate::mainutils::subassign::do_subassign_dflt(call, op, args, env)
            };
            SET_VECTOR_ELT(ans, j, assigned);
        }
        let names = getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
        if !isNull(names) {
            setAttrib(ans, crate::sexp::attrib_core::R_NamesSymbol(), names);
        }
        setAttrib(ans, sym_Class(), getAttrib(x, sym_Class()));
        let tzone = getAttrib(x, sym_Tzone());
        if !isNull(tzone) {
            setAttrib(ans, sym_Tzone(), tzone);
        }
        // GNU `[<-.POSIXlt` keeps balanced via unCfill+class<- when the
        // source is balanced and i introduces no NA (same rule as `[`).
        let keep_balanced = posixlt_index_preserves_balance(x, i, n, call, op, env);
        setAttrib(
            ans,
            Rf_install(c"balanced".as_ptr()),
            Rf_ScalarLogical(if keep_balanced { TRUE } else { NA_LOGICAL }),
        );
        ans
    }
}

pub unsafe fn do_subassign(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let mut ans: SEXP = ptr::null_mut();

        /* DispatchOrEval internal generic: [<- */
        if R_DispatchOrEvalSP(
            call,
            op,
            b"[<-\0".as_ptr() as *const c_char,
            args,
            env,
            &mut ans,
        ) != 0
        {
            if NAMED(ans) != 0 {
                ENSURE_NAMEDMAX(ans);
            }
            return ans;
        }

        /* Fall through to default -- delegated to subassign module */
        // Base R ships `[<-.factor`: character values assigned into a
        // factor are matched against its LEVELS (new levels become NA
        // with a warning upstream); the factor stays integer-coded. The
        // engine default would widen the vector to character, losing the
        // factor. Translate character values to level codes first.
        let target = CAR(ans);
        if has_class_factor(target)
            && TYPEOF(target) == SEXPTYPE::INTSXP
            && TYPEOF(CADDR(ans)) == SEXPTYPE::STRSXP
        {
            let levels = getAttrib(target, crate::eval::attrib_core::R_LevelsSymbol());
            if !isNull(levels) && TYPEOF(levels) == SEXPTYPE::STRSXP {
                let value = CADDR(ans);
                let nv = XLENGTH(value);
                let codes = Rf_allocVector3(SEXPTYPE::INTSXP, nv);
                let _cg = protect(codes);
                let mut warned = false;
                for i in 0..nv {
                    let vs = crate::sexp::accessors::STRING_ELT(value, i);
                    let mut code: c_int = NA_INTEGER;
                    if !vs.is_null() && vs != crate::sexp::globals::R_NaString() {
                        let vb = crate::sexp::accessors::CHAR(vs);
                        for (j, lev) in (0..XLENGTH(levels)).enumerate() {
                            let ls = crate::sexp::accessors::STRING_ELT(levels, lev);
                            if !ls.is_null()
                                && ls != crate::sexp::globals::R_NaString()
                                && std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(ls))
                                    .to_bytes()
                                    == std::ffi::CStr::from_ptr(vb).to_bytes()
                            {
                                code = (j + 1) as c_int;
                                break;
                            }
                        }
                    }
                    if code == NA_INTEGER
                        && !warned
                        && !vs.is_null()
                        && vs != crate::sexp::globals::R_NaString()
                    {
                        warned = true;
                        crate::mainutils::errors::warningcall(
                            call,
                            b"invalid factor level, NA generated\0".as_ptr()
                                as *const core::ffi::c_char,
                        );
                    }
                    *crate::sexp::accessors::INTEGER(codes).add(i as usize) = code;
                }
                SETCAR(CDDR(ans), codes);
            }
        }
        if crate::mainutils::essentials::sexp_has_class(target, "POSIXlt")
            && TYPEOF(target) == SEXPTYPE::VECSXP
        {
            let idx = CADR(ans);
            let value = CADDR(ans);
            let rest = CDDDR(ans);
            let only_time_index = !idx.is_null()
                && idx != R_NilValue()
                && idx != R_MissingArg()
                && (rest.is_null() || rest == R_NilValue());
            if only_time_index {
                return subassign_posixlt_time(target, idx, value, call, op, env);
            }
        }
        crate::mainutils::subassign::do_subassign_dflt(call, op, ans, env)
    }
}

// ---------------------------------------------------------------------------
// do_subassign2 -- the `[[<-` assignment operator
// ---------------------------------------------------------------------------

/// The `[[<-` assignment operator.
pub unsafe fn do_subassign2(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let mut ans: SEXP = ptr::null_mut();

        /* DispatchOrEval internal generic: [[<- */
        if R_DispatchOrEvalSP(
            call,
            op,
            b"[[<-\0".as_ptr() as *const c_char,
            args,
            env,
            &mut ans,
        ) != 0
        {
            if NAMED(ans) != 0 {
                ENSURE_NAMEDMAX(ans);
            }
            return ans;
        }

        let target = CAR(ans);
        if crate::mainutils::essentials::sexp_has_class(target, "POSIXlt")
            && TYPEOF(target) == SEXPTYPE::VECSXP
        {
            let idx = CADR(ans);
            let value = CADDR(ans);
            let rest = CDDDR(ans);
            let only_time_index = !idx.is_null()
                && idx != R_NilValue()
                && idx != R_MissingArg()
                && (rest.is_null() || rest == R_NilValue());
            if only_time_index {
                return subassign_posixlt_time(target, idx, value, call, op, env);
            }
        }
        crate::mainutils::subassign::do_subassign2_dflt(call, op, ans, env)
    }
}

// ---------------------------------------------------------------------------
// do_subassign3 -- the `$<-` assignment operator
// ---------------------------------------------------------------------------

/// The `$<-` assignment operator.
pub unsafe fn do_subassign3(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let mut ans: SEXP = ptr::null_mut();

        /* DispatchOrEval internal generic: $<- */
        if R_DispatchOrEvalSP(
            call,
            op,
            b"$<-\0".as_ptr() as *const c_char,
            args,
            env,
            &mut ans,
        ) != 0
        {
            if NAMED(ans) != 0 {
                ENSURE_NAMEDMAX(ans);
            }
            return ans;
        }

        /* Fall through to default -- delegated to subassign module */
        /* For now, return the args unchanged */
        args
    }
}
