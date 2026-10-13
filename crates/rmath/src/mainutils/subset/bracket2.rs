use super::*;

// ---------------------------------------------------------------------------
// do_subset2 -- the `[[` subset operator
// ---------------------------------------------------------------------------

/// The `[[` subset operator. Designed to be fast for extracting single elements.
pub unsafe fn do_subset2(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut ans: SEXP = ptr::null_mut();

        /* DispatchOrEval internal generic: [[ */
        if R_DispatchOrEvalSP(
            call,
            op,
            b"[[\0".as_ptr() as *const c_char,
            args,
            rho,
            &mut ans,
        ) != 0
        {
            if NAMED(ans) != 0 {
                ENSURE_NAMEDMAX(ans);
            }
            return ans;
        }

        let orig = CAR(ans);
        if crate::mainutils::essentials::sexp_has_class(orig, "POSIXlt")
            && TYPEOF(orig) == SEXPTYPE::VECSXP
        {
            return subset_posixlt_obs(orig, CADR(ans), call, op, rho);
        }
        do_subset2_dflt(call, op, ans, rho)
    }
}

// ---------------------------------------------------------------------------
// do_subset2_dflt -- default method for `[[`
// ---------------------------------------------------------------------------

/// Default method for `[[`. Handles vector indexing, matrix/array indexing,
/// pair-list indexing, and environment subsetting.
/// Port of `vectorIndex()` from subscript.c — used by `[[` with a vector of
/// indices of length > 1. Recursively descends into list/pairlist elements
/// from level `start` to level `stop - 1`.
unsafe fn vectorIndex(
    mut x: SEXP,
    thesub: SEXP,
    start: c_int,
    stop: c_int,
    pok: c_int,
    call: SEXP,
) -> SEXP {
    unsafe {
        let mut i = start;
        while i < stop {
            if !isVectorList(x) && !isPairList(x) {
                if i != 0 {
                    errorcall(
                        call,
                        &format!("recursive indexing failed at level {}", i + 1),
                    );
                } else {
                    errorcall(
                        call,
                        "attempt to select more than one element in vectorIndex",
                    );
                }
            }
            let x_guard = protect(x);
            let names = getAttrib(x, sym_Names());
            let names_guard = protect(names);
            let offset = get1index(thesub, names, xlength(x), pok, i, call);
            drop(names_guard);
            drop(x_guard);
            if offset < 0 || offset >= xlength(x) {
                errorcall(call, &format!("no such index at level {}", i + 1));
            }
            if isPairList(x) {
                let cx = nthcdr(x, offset as c_int);
                RAISE_NAMED(CAR(cx), NAMED(x));
                x = CAR(cx);
            } else {
                let cx = x;
                x = VECTOR_ELT(cx, offset);
                RAISE_NAMED(x, NAMED(cx));
            }
            i += 1;
        }
        x
    }
}

pub unsafe fn do_subset2_dflt(call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let _args_guard = protect(args);

        let mut drop: c_int = 1;
        ExtractDropArg(args, &mut drop);
        let exact = ExtractExactArg(args);
        let pok = if exact == -1 {
            exact
        } else {
            if exact == 0 { 1 } else { 0 }
        };

        let x = CAR(args);
        let subs = CDR(args);
        let nsubs = length_int(subs);

        if nsubs == 0 {
            crate::mainutils::errors::R_MissingSubscriptError(x, call);
        }

        /* Handle NULL case */
        if isNull(x) {
            if !isNull(subs) && !isNull(CAR(subs)) && isSymbol(CAR(subs)) {
                let pn = PRINTNAME(CAR(subs));
                if !isNull(pn) {
                    let c = CHAR(pn);
                    if !c.is_null() && *c == 0 {
                        errorcallMissingSubs(x, call);
                    }
                }
            }
            return x;
        }

        let dims = getAttrib(x, sym_Dim());
        if nsubs == 2 && is_data_frame(x) && TYPEOF(x) == SEXPTYPE::VECSXP {
            let ncols = XLENGTH(x);
            let names = getAttrib(x, crate::sexp::attrib_core::R_NamesSymbol());
            let j = crate::mainutils::subscript::get1index(CADR(subs), names, ncols, pok, 0, call);
            if j < 0 || j >= ncols {
                errorcall(call, "undefined columns selected");
            }
            let col = VECTOR_ELT(x, j);
            let nrows = XLENGTH(col);
            let rownames = getAttrib(x, crate::sexp::attrib_core::R_RowNamesSymbol());
            let i =
                crate::mainutils::subscript::get1index(CAR(subs), rownames, nrows, pok, 0, call);
            if i < 0 || i >= nrows {
                errorcall(call, "subscript out of bounds");
            }
            return match TYPEOF(col) {
                t if t == SEXPTYPE::REALSXP => Rf_ScalarReal(*REAL(col).add(i as usize)),
                t if t == SEXPTYPE::INTSXP => Rf_ScalarInteger(*INTEGER(col).add(i as usize)),
                t if t == SEXPTYPE::LGLSXP => Rf_ScalarLogical(*LOGICAL(col).add(i as usize)),
                t if t == SEXPTYPE::STRSXP => Rf_ScalarString(STRING_ELT(col, i)),
                _ => {
                    errorcall(call, "incorrect number of subscripts");
                    R_NilValue()
                }
            };
        }
        let ndims = length_int(dims);
        if nsubs > 1 && nsubs != ndims {
            errorcall(call, "incorrect number of subscripts");
        }

        let _x_guard = protect(x);

        /* Environment subsetting */
        if isEnvironment(x) {
            if nsubs != 1 || !isString(CAR(subs)) || length_int(CAR(subs)) != 1 {
                errorcall(call, "wrong arguments for subsetting an environment");
            }
            let sym = installTrChar(STRING_ELT(CAR(subs), 0));
            let mut ans = R_findVarInFrame(x, sym);
            if isPromise(ans) {
                let _promise_guard = protect(ans);
                ans = crate::sexp::envir::forcePromise(ans);
            } else {
                ENSURE_NAMEDMAX(ans);
            }

            if ans == R_UnboundValue() {
                return R_NilValue();
            }
            if NAMED(ans) != 0 {
                ENSURE_NAMEDMAX(ans);
            }
            return ans;
        }

        /* Check subsettable types */
        if !isVector(x) && !isPairList(x) && !isLanguage(x) {
            errorcallNotSubsettable(x, call);
        }

        let named_x = NAMED(x);

        if nsubs == 1 {
            /* Vector indexing */
            let thesub = CAR(subs);
            let len = length_int(thesub);

            if len > 1 {
                /* Multi-element index: recursively descend via vectorIndex,
                then apply the final index to the resulting element. */
                let x = vectorIndex(x, thesub, 0, (len - 1) as c_int, pok, call);
                let _x_guard = protect(x);
                let named_x = NAMED(x);
                let xnames = getAttrib(x, sym_Names());
                let _xnames_guard = protect(xnames);
                let offset = get1index(thesub, xnames, xlength(x), pok, (len - 1) as c_int, call);
                if offset < 0 || offset >= xlength(x) {
                    if offset < 0
                        && (isVectorList(x) || isExpression(x) || isPairList(x) || isLanguage(x))
                    {
                        return R_NilValue();
                    } else {
                        errorcallOutOfBoundsSEXP(x, -1, thesub, call);
                    }
                }
                /* Extract the final element */
                if isPairList(x) {
                    let ans = CAR(nthcdr(x, offset as c_int));
                    RAISE_NAMED(ans, named_x);
                    return ans;
                } else if isVectorList(x) {
                    let ans = VECTOR_ELT(x, offset);
                    RAISE_NAMED(ans, named_x);
                    return ans;
                } else {
                    /* atomic: return single-element vector */
                    let ans = Rf_allocVector3(TYPEOF(x), 1);
                    let _ans_guard = protect(ans);
                    match TYPEOF(x) {
                        t if t == SEXPTYPE::LGLSXP => {
                            *LOGICAL(ans).add(0) = LOGICAL_ELT(x, offset as c_int);
                        }
                        t if t == SEXPTYPE::INTSXP => {
                            *INTEGER(ans).add(0) = INTEGER_ELT(x, offset as c_int);
                        }
                        t if t == SEXPTYPE::REALSXP => {
                            *REAL(ans).add(0) = REAL_ELT(x, offset as c_int);
                        }
                        t if t == SEXPTYPE::CPLXSXP => {
                            *COMPLEX(ans).add(0) = COMPLEX_ELT(x, offset as c_int);
                        }
                        t if t == SEXPTYPE::STRSXP => {
                            SET_STRING_ELT(ans, 0, STRING_ELT(x, offset));
                        }
                        t if t == SEXPTYPE::RAWSXP => {
                            *RAW(ans).add(0) = RAW_ELT(x, offset as c_int);
                        }
                        _ => {} // intentionally unhandled: unsupported SEXPTYPE for scalar subset
                    }
                    return ans;
                }
            }

            /* Single-element index */
            let mut xnames = getAttrib(x, sym_Names());
            if (isNull(xnames) || xnames == R_NilValue()) && (isPairList(x) || isLanguage(x)) {
                xnames = pairlist_tag_names(x);
            }
            let _xnames_guard = protect(xnames);
            let offset = get1index(thesub, xnames, xlength(x), pok, -1, call);

            if offset < 0 || offset >= xlength(x) {
                if offset < 0
                    && (isVectorList(x) || isExpression(x) || isPairList(x) || isLanguage(x))
                {
                    return R_NilValue();
                } else {
                    errorcallOutOfBoundsSEXP(x, -1, thesub, call);
                }
            }

            /* Extract the element */
            if isPairList(x) {
                let ans = CAR(nthcdr(x, offset as c_int));
                RAISE_NAMED(ans, named_x);
                return ans;
            } else if isVectorList(x) {
                let ans = VECTOR_ELT(x, offset);
                RAISE_NAMED(ans, named_x);
                return ans;
            } else {
                /* Atomic vector: return scalar */
                let ans = Rf_allocVector3(TYPEOF(x), 1);
                let _ans_guard = protect(ans);
                match TYPEOF(x) {
                    t if t == SEXPTYPE::LGLSXP => {
                        *LOGICAL(ans).add(0) = LOGICAL_ELT(x, offset as c_int);
                    }
                    t if t == SEXPTYPE::INTSXP => {
                        *INTEGER(ans).add(0) = INTEGER_ELT(x, offset as c_int);
                    }
                    t if t == SEXPTYPE::REALSXP => {
                        *REAL(ans).add(0) = REAL_ELT(x, offset as c_int);
                    }
                    t if t == SEXPTYPE::CPLXSXP => {
                        *COMPLEX(ans).add(0) = COMPLEX_ELT(x, offset as c_int);
                    }
                    t if t == SEXPTYPE::STRSXP => {
                        SET_STRING_ELT(ans, 0, STRING_ELT(x, offset));
                    }
                    t if t == SEXPTYPE::RAWSXP => {
                        *RAW(ans).add(0) = RAW_ELT(x, offset as c_int);
                    }
                    _ => {} // intentionally unhandled: unsupported SEXPTYPE for scalar subset
                }
                return ans;
            }
        } else {
            /* nsubs == ndims >= 2 : matrix|array indexing */
            let pdims = INTEGER(dims);
            let dimnames = getAttrib(x, sym_DimNames());
            let ndn = length_int(dimnames);

            let indx = Rf_allocVector(SEXPTYPE::INTSXP, nsubs);
            let _indx_guard = protect(indx);
            let pindx = INTEGER(indx);

            let mut cur_subs = subs;
            for i in 0..(nsubs as usize) {
                let thesub = CAR(cur_subs);
                let dname = if (i as c_int) < ndn {
                    VECTOR_ELT(dimnames, i as R_xlen_t)
                } else {
                    R_NilValue()
                };
                let idx = get1index(thesub, dname, *pdims.add(i) as R_xlen_t, pok, -1, call);
                *pindx.add(i) = idx as c_int;
                cur_subs = CDR(cur_subs);
                if idx < 0 || idx >= (*pdims.add(i)) as R_xlen_t {
                    errorcallOutOfBoundsSEXP(x, i as c_int, thesub, call);
                }
            }

            /* Compute linear offset */
            let mut offset: R_xlen_t = 0;
            let mut i: usize = (nsubs - 1) as usize;
            loop {
                offset = (offset + *pindx.add(i) as R_xlen_t) * (*pdims.add(i - 1)) as R_xlen_t;
                if i == 1 {
                    break;
                }
                i -= 1;
            }
            offset += *pindx.add(0) as R_xlen_t;

            /* Extract the element */
            let ans: SEXP;
            if isPairList(x) {
                ans = CAR(nthcdr(x, offset as c_int));
                RAISE_NAMED(ans, named_x);
            } else if isVectorList(x) {
                ans = VECTOR_ELT(x, offset);
                RAISE_NAMED(ans, named_x);
            } else {
                ans = Rf_allocVector3(TYPEOF(x), 1);
                let _ans_guard = protect(ans);
                match TYPEOF(x) {
                    t if t == SEXPTYPE::LGLSXP => {
                        *LOGICAL(ans).add(0) = LOGICAL_ELT(x, offset as c_int);
                    }
                    t if t == SEXPTYPE::INTSXP => {
                        *INTEGER(ans).add(0) = INTEGER_ELT(x, offset as c_int);
                    }
                    t if t == SEXPTYPE::REALSXP => {
                        *REAL(ans).add(0) = REAL_ELT(x, offset as c_int);
                    }
                    t if t == SEXPTYPE::CPLXSXP => {
                        *COMPLEX(ans).add(0) = COMPLEX_ELT(x, offset as c_int);
                    }
                    t if t == SEXPTYPE::STRSXP => {
                        SET_STRING_ELT(ans, 0, STRING_ELT(x, offset));
                    }
                    t if t == SEXPTYPE::RAWSXP => {
                        *RAW(ans).add(0) = RAW_ELT(x, offset as c_int);
                    }
                    _ => {} // intentionally unhandled: unsupported SEXPTYPE for scalar subset
                }
            }

            ans
        }
    }
}

// ---------------------------------------------------------------------------
// dispatch_subset2 -- dispatch [[ on objects
// ---------------------------------------------------------------------------

/// Dispatch the `[[` operator on an object. If `x` is an object, uses
/// `do_subset2`; otherwise extracts the element directly.
pub unsafe fn dispatch_subset2(x: SEXP, i: R_xlen_t, call: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if isObject(x) {
            let args = list2(x, Rf_ScalarReal(i as c_double + 1.0));
            let _args_guard = protect(args);
            let bracket_op = crate::mainutils::names::R_Primitive(c"[[".as_ptr());
            let x_elt = do_subset2(call, bracket_op, args, rho);
            x_elt
        } else {
            VECTOR_ELT(x, i)
        }
    }
}
