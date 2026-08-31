use super::*;

// ---------------------------------------------------------------------------
// Vector coercion functions
// ---------------------------------------------------------------------------

/// Coerce a vector to logical type.
pub unsafe fn coerceToLogical(v: SEXP) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;
        let n = xlength(v);
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let _ans_guard = protect(ans);
        SHALLOW_DUPLICATE_ATTRIB(ans, v);
        let pa = LOGICAL(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            *pa.add(i as usize) = match vtype {
                t if t == SEXPTYPE::INTSXP => LogicalFromInteger(INTEGER_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::REALSXP => LogicalFromReal(REAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::CPLXSXP => LogicalFromComplex(COMPLEX_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::STRSXP => LogicalFromString(STRING_ELT(v, i), &mut warn),
                t if t == SEXPTYPE::RAWSXP => {
                    LogicalFromInteger(RAW_ELT(v, ii) as c_int, &mut warn)
                }
                _ => NA_LOGICAL,
            };
        }

        if warn != 0 {
            CoercionWarning(warn);
        }
        ans
    }
}

/// Coerce a vector to integer type.
pub unsafe fn coerceToInteger(v: SEXP) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;
        let n = xlength(v);
        let ans = Rf_allocVector3(SEXPTYPE::INTSXP, n);
        let _ans_guard = protect(ans);
        SHALLOW_DUPLICATE_ATTRIB(ans, v);
        let pa = INTEGER(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            *pa.add(i as usize) = match vtype {
                t if t == SEXPTYPE::LGLSXP => IntegerFromLogical(LOGICAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::REALSXP => IntegerFromReal(REAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::CPLXSXP => IntegerFromComplex(COMPLEX_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::STRSXP => IntegerFromString(STRING_ELT(v, i), &mut warn),
                t if t == SEXPTYPE::RAWSXP => RAW_ELT(v, ii) as c_int,
                _ => NA_INTEGER,
            };
        }

        if warn != 0 {
            CoercionWarning(warn);
        }
        ans
    }
}

/// Coerce a vector to real (double) type.
pub unsafe fn coerceToReal(v: SEXP) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;
        let n = xlength(v);
        let ans = Rf_allocVector3(SEXPTYPE::REALSXP, n);
        let _ans_guard = protect(ans);
        SHALLOW_DUPLICATE_ATTRIB(ans, v);
        let pa = REAL(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            *pa.add(i as usize) = match vtype {
                t if t == SEXPTYPE::LGLSXP => RealFromLogical(LOGICAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::INTSXP => RealFromInteger(INTEGER_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::CPLXSXP => RealFromComplex(COMPLEX_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::STRSXP => RealFromString(STRING_ELT(v, i), &mut warn),
                t if t == SEXPTYPE::RAWSXP => RealFromInteger(RAW_ELT(v, ii) as c_int, &mut warn),
                _ => NA_REAL,
            };
        }

        if warn != 0 {
            CoercionWarning(warn);
        }
        ans
    }
}

/// Coerce a vector to complex type.
pub unsafe fn coerceToComplex(v: SEXP) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;
        let n = xlength(v);
        let ans = Rf_allocVector3(SEXPTYPE::CPLXSXP, n);
        let _ans_guard = protect(ans);
        SHALLOW_DUPLICATE_ATTRIB(ans, v);
        let pa = COMPLEX(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            *pa.add(i as usize) = match vtype {
                t if t == SEXPTYPE::LGLSXP => ComplexFromLogical(LOGICAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::INTSXP => ComplexFromInteger(INTEGER_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::REALSXP => ComplexFromReal(REAL_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::STRSXP => ComplexFromString(STRING_ELT(v, i), &mut warn),
                t if t == SEXPTYPE::RAWSXP => {
                    ComplexFromInteger(RAW_ELT(v, ii) as c_int, &mut warn)
                }
                _ => Rcomplex {
                    r: NA_REAL,
                    i: NA_REAL,
                },
            };
        }

        if warn != 0 {
            CoercionWarning(warn);
        }
        ans
    }
}

/// Coerce a vector to raw type.
pub unsafe fn coerceToRaw(v: SEXP) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;
        let n = xlength(v);
        let ans = Rf_allocVector3(SEXPTYPE::RAWSXP, n);
        let _ans_guard = protect(ans);
        SHALLOW_DUPLICATE_ATTRIB(ans, v);
        let pa = RAW(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            let tmp: c_int = match vtype {
                t if t == SEXPTYPE::LGLSXP => {
                    let val = IntegerFromLogical(LOGICAL_ELT(v, ii), &mut warn);
                    if val == NA_INTEGER {
                        warn |= WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                t if t == SEXPTYPE::INTSXP => {
                    let val = INTEGER_ELT(v, ii);
                    if val == NA_INTEGER || val < 0 || val > 255 {
                        warn |= WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                t if t == SEXPTYPE::REALSXP => {
                    let val = IntegerFromReal(REAL_ELT(v, ii), &mut warn);
                    if val == NA_INTEGER || val < 0 || val > 255 {
                        warn |= WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                t if t == SEXPTYPE::CPLXSXP => {
                    let val = IntegerFromComplex(COMPLEX_ELT(v, ii), &mut warn);
                    if val == NA_INTEGER || val < 0 || val > 255 {
                        warn |= WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                t if t == SEXPTYPE::STRSXP => {
                    let val = IntegerFromString(STRING_ELT(v, i), &mut warn);
                    if val == NA_INTEGER || val < 0 || val > 255 {
                        warn |= WARN_RAW;
                        0
                    } else {
                        val
                    }
                }
                _ => 0,
            };
            *pa.add(i as usize) = tmp as Rbyte;
        }

        if warn != 0 {
            CoercionWarning(warn);
        }
        ans
    }
}

/// Coerce a vector to string (character) type.
pub unsafe fn coerceToString(v: SEXP) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;
        let n = xlength(v);
        let ans = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _ans_guard = protect(ans);
        SHALLOW_DUPLICATE_ATTRIB(ans, v);

        let vtype = TYPEOF(v);
        // stock coerce.c:772-778 (REALSXP) and 781-787 (CPLXSXP): pin the
        // printing digits to DBL_DIG (MAX precision) around the
        // StringFromReal loop so as.character(<double>) renders 15
        // significant digits regardless of options("digits"). The port's
        // formatReal reads options("digits") live, so pin the option.
        let pin_digits = vtype == SEXPTYPE::REALSXP || vtype == SEXPTYPE::CPLXSXP;
        let saved_digits = if pin_digits {
            let digits_sym = Rf_install(b"digits\0".as_ptr() as *const c_char);
            let saved = crate::mainutils::options::GetOption1(digits_sym);
            let _saved_guard = protect(saved);
            let max = Rf_ScalarInteger(15);
            let _max_guard = protect(max);
            crate::mainutils::options::R_SetOption(digits_sym, max);
            saved
        } else {
            R_NilValue()
        };
        let _digits_guard = RestoreDigitsOnScopeEnd {
            active: pin_digits,
            saved_digits,
        };
        struct RestoreDigitsOnScopeEnd {
            active: bool,
            saved_digits: SEXP,
        }
        impl Drop for RestoreDigitsOnScopeEnd {
            fn drop(&mut self) {
                if self.active {
                    let digits_sym = unsafe { Rf_install(b"digits\0".as_ptr() as *const c_char) };
                    unsafe {
                        crate::mainutils::options::R_SetOption(digits_sym, self.saved_digits);
                    }
                }
            }
        }
        for i in 0..n {
            let ii = i as c_int;
            let s = match vtype {
                t if t == SEXPTYPE::LGLSXP => StringFromLogical(LOGICAL_ELT(v, ii)),
                t if t == SEXPTYPE::INTSXP => StringFromInteger(INTEGER_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::REALSXP => {
                    crate::mainutils::printutils::StringFromReal(REAL_ELT(v, ii), &mut warn)
                }
                t if t == SEXPTYPE::CPLXSXP => StringFromComplex(COMPLEX_ELT(v, ii), &mut warn),
                t if t == SEXPTYPE::RAWSXP => StringFromRaw(RAW_ELT(v, ii), &mut warn),
                _ => R_NaString(),
            };
            SET_STRING_ELT(ans, i, s);
        }

        if warn != 0 {
            CoercionWarning(warn);
        }
        ans
    }
}

/// Coerce a vector to expression type.
pub unsafe fn coerceToExpression(v: SEXP) -> SEXP {
    unsafe {
        if !isVectorAtomic(v) {
            let ans = Rf_allocVector3(SEXPTYPE::EXPRSXP, 1);
            let _ans_guard = protect(ans);
            SET_VECTOR_ELT(ans, 0, v);
            return ans;
        }

        let n = xlength(v);
        let ans = Rf_allocVector3(SEXPTYPE::EXPRSXP, n);
        let _ans_guard = protect(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            let elt = match vtype {
                t if t == SEXPTYPE::LGLSXP => Rf_ScalarLogical(LOGICAL_ELT(v, ii)),
                t if t == SEXPTYPE::INTSXP => Rf_ScalarInteger(INTEGER_ELT(v, ii)),
                t if t == SEXPTYPE::REALSXP => Rf_ScalarReal(REAL_ELT(v, ii)),
                t if t == SEXPTYPE::CPLXSXP => Rf_ScalarComplex(COMPLEX_ELT(v, ii)),
                t if t == SEXPTYPE::STRSXP => Rf_ScalarString(STRING_ELT(v, i)),
                t if t == SEXPTYPE::RAWSXP => Rf_ScalarRaw(RAW_ELT(v, ii)),
                _ => R_NilValue(),
            };
            SET_VECTOR_ELT(ans, i, elt);
        }

        ans
    }
}

/// Coerce a vector to generic vector (list) type.
pub unsafe fn coerceToVectorList(v: SEXP) -> SEXP {
    unsafe {
        let n = xlength(v);
        let ans = Rf_allocVector3(SEXPTYPE::VECSXP, n);
        let _ans_guard = protect(ans);

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            let elt = match vtype {
                t if t == SEXPTYPE::LGLSXP => Rf_ScalarLogical(LOGICAL_ELT(v, ii)),
                t if t == SEXPTYPE::INTSXP => Rf_ScalarInteger(INTEGER_ELT(v, ii)),
                t if t == SEXPTYPE::REALSXP => Rf_ScalarReal(REAL_ELT(v, ii)),
                t if t == SEXPTYPE::CPLXSXP => Rf_ScalarComplex(COMPLEX_ELT(v, ii)),
                t if t == SEXPTYPE::STRSXP => Rf_ScalarString(STRING_ELT(v, i)),
                t if t == SEXPTYPE::RAWSXP => Rf_ScalarRaw(RAW_ELT(v, ii)),
                t if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP => CAR(v.add(i as usize)),
                _ => R_NilValue(),
            };
            SET_VECTOR_ELT(ans, i, elt);
        }

        // Copy names attribute if present
        let names = getAttrib(v, R_NamesSymbol());
        if !isNull(names) {
            setAttrib(ans, R_NamesSymbol(), names);
        }

        ans
    }
}

/// Coerce a vector to pairlist type.
pub unsafe fn coerceToPairList(v: SEXP) -> SEXP {
    unsafe {
        let n = LENGTH(v);
        let ans = Rf_allocList(n);
        let _ans_guard = protect(ans);
        let mut ansp = ans;

        let vtype = TYPEOF(v);
        for i in 0..n {
            let ii = i as c_int;
            match vtype {
                t if t == SEXPTYPE::LGLSXP => {
                    let elt = Rf_allocVector3(SEXPTYPE::LGLSXP, 1);
                    *LOGICAL(elt) = LOGICAL_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::INTSXP => {
                    let elt = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
                    *INTEGER(elt) = INTEGER_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::REALSXP => {
                    let elt = Rf_allocVector3(SEXPTYPE::REALSXP, 1);
                    *REAL(elt) = REAL_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::CPLXSXP => {
                    let elt = Rf_allocVector3(SEXPTYPE::CPLXSXP, 1);
                    *COMPLEX(elt) = COMPLEX_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::STRSXP => {
                    SETCAR(ansp, Rf_ScalarString(STRING_ELT(v, i as R_xlen_t)));
                }
                t if t == SEXPTYPE::RAWSXP => {
                    let elt = Rf_allocVector3(SEXPTYPE::RAWSXP, 1);
                    *RAW(elt) = RAW_ELT(v, ii);
                    SETCAR(ansp, elt);
                }
                t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                    SETCAR(ansp, VECTOR_ELT(v, i as R_xlen_t));
                }
                _ => {} // intentionally unhandled: unsupported SEXPTYPE for coercion
            }
            ansp = CDR(ansp);
        }

        // Copy names attribute if present
        let names = getAttrib(v, R_NamesSymbol());
        if !isNull(names) {
            setAttrib(ans, R_NamesSymbol(), names);
        }

        ans
    }
}

/// Coerce a pairlist (LISTSXP/LANGSXP) to the given type.
pub unsafe fn coercePairList(v: SEXP, type_: SEXPTYPE) -> SEXP {
    unsafe {
        if type_ == SEXPTYPE::EXPRSXP {
            let rval = Rf_allocVector3(SEXPTYPE::EXPRSXP, 1);
            let _rval_guard = protect(rval);
            SET_VECTOR_ELT(rval, 0, v);
            return rval;
        }

        if type_ == SEXPTYPE::STRSXP {
            // bind length: pairlists carry no meaningful raw length field,
            // so count the cells (trunk length()).
            let mut n: c_int = 0;
            {
                let mut counter = v;
                while !counter.is_null() && !isNull(counter) {
                    n += 1;
                    counter = CDR(counter);
                }
            }
            let rval = Rf_allocVector3(SEXPTYPE::STRSXP, n as R_xlen_t);
            let _rval_guard = protect(rval);
            let mut vp = v;
            for i in 0..n {
                let car = CAR(vp);
                if isString(car) && LENGTH(car) == 1 {
                    SET_STRING_ELT(rval, i as R_xlen_t, STRING_ELT(car, 0));
                } else {
                    // coerce.c coercePairList: deparse non-trivial cells
                    // onto a single line (deparse1line).
                    let dep = crate::mainutils::deparse::deparse1line(car, false);
                    if !dep.is_null()
                        && dep != R_NilValue()
                        && TYPEOF(dep) == SEXPTYPE::STRSXP
                        && xlength(dep) > 0
                    {
                        SET_STRING_ELT(rval, i as R_xlen_t, STRING_ELT(dep, 0));
                    } else {
                        SET_STRING_ELT(rval, i as R_xlen_t, R_NaString());
                    }
                }
                vp = CDR(vp);
            }
            return rval;
        }

        if type_ == SEXPTYPE::VECSXP {
            // PairToVectorList
            let mut len: c_int = 0;
            let mut xptr = v;
            while !xptr.is_null() && !isNull(xptr) {
                len += 1;
                xptr = CDR(xptr);
            }
            let xnew = Rf_allocVector3(SEXPTYPE::VECSXP, len as R_xlen_t);
            let _xnew_guard = protect(xnew);
            let mut xptr = v;
            for i in 0..len {
                SET_VECTOR_ELT(xnew, i as R_xlen_t, CAR(xptr));
                xptr = CDR(xptr);
            }
            return xnew;
        }

        if isVectorizable(v) {
            let n = LENGTH(v);
            let rval = Rf_allocVector3(type_.0, n as R_xlen_t);
            let _rval_guard = protect(rval);
            let mut vp = v;
            for i in 0..n {
                match type_.0 {
                    t if t == SEXPTYPE::LGLSXP => {
                        *LOGICAL(rval).add(i as usize) = asLogical(CAR(vp));
                    }
                    t if t == SEXPTYPE::INTSXP => {
                        *INTEGER(rval).add(i as usize) = asInteger(CAR(vp));
                    }
                    t if t == SEXPTYPE::REALSXP => {
                        *REAL(rval).add(i as usize) = asReal(CAR(vp));
                    }
                    t if t == SEXPTYPE::CPLXSXP => {
                        *COMPLEX(rval).add(i as usize) = asComplex(CAR(vp));
                    }
                    t if t == SEXPTYPE::RAWSXP => {
                        *RAW(rval).add(i as usize) = asInteger(CAR(vp)) as Rbyte;
                    }
                    _ => {} // intentionally unhandled: unsupported SEXPTYPE for coercion
                }
                vp = CDR(vp);
            }
            return rval;
        }

        error("cannot coerce type to vector");
    }
}

/// Coerce a vector list (VECSXP/EXPRSXP) to the given type.
pub unsafe fn coerceVectorList(v: SEXP, type_: SEXPTYPE) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;

        // expression -> list: just change the type tag
        if type_ == SEXPTYPE::VECSXP && TYPEOF(v) == SEXPTYPE::EXPRSXP {
            let rval = Rf_allocVector3(SEXPTYPE::VECSXP, xlength(v));
            // Copy the data pointers
            let src = DATAPTR(v);
            let dst = DATAPTR(rval);
            if !src.is_null() && !dst.is_null() {
                ptr::copy_nonoverlapping(src as *const SEXP, dst as *mut SEXP, xlength(v) as usize);
            }
            return rval;
        }

        // list -> expression: just change the type tag
        if type_ == SEXPTYPE::EXPRSXP && TYPEOF(v) == SEXPTYPE::VECSXP {
            let rval = Rf_allocVector3(SEXPTYPE::EXPRSXP, xlength(v));
            let src = DATAPTR(v);
            let dst = DATAPTR(rval);
            if !src.is_null() && !dst.is_null() {
                ptr::copy_nonoverlapping(src as *const SEXP, dst as *mut SEXP, xlength(v) as usize);
            }
            return rval;
        }

        // list -> pairlist
        if type_ == SEXPTYPE::LISTSXP {
            // VectorToPairList
            let n = LENGTH(v);
            let x = Rf_allocList(n);
            let _x_guard = protect(x);
            let names = getAttrib(v, R_NamesSymbol());
            let _names_guard = protect(names);
            let mut xptr = x;
            for i in 0..n {
                SETCAR(xptr, VECTOR_ELT(v, i as R_xlen_t));
                xptr = CDR(xptr);
            }
            if !isNull(names) {
                let mut xptr2 = x;
                for i in 0..n {
                    let name_elt = STRING_ELT(names, i as R_xlen_t);
                    if !isNull(name_elt) {
                        let pname = CHAR(name_elt);
                        if !pname.is_null() && *pname != 0 {
                            SETTAG(xptr2, Rf_install(pname));
                        }
                    }
                    xptr2 = CDR(xptr2);
                }
            }
            return x;
        }

        // list -> string
        if type_ == SEXPTYPE::STRSXP {
            let n = xlength(v);
            let rval = Rf_allocVector3(SEXPTYPE::STRSXP, n);
            let _rval_guard = protect(rval);
            for i in 0..n {
                let elt = VECTOR_ELT(v, i);
                if isString(elt) && LENGTH(elt) == 1 {
                    SET_STRING_ELT(rval, i, STRING_ELT(elt, 0));
                } else {
                    // coerce.c coerceVectorList: non-trivial entries are
                    // deparsed onto a single line (deparse1line_ex with
                    // NICE_NAMES); e.g. calls -> "f(1)", NULL -> "NULL",
                    // .Primitive("sin") -> ".Primitive(\"sin\")".
                    let dep = crate::mainutils::deparse::deparse1line(elt, false);
                    if !dep.is_null()
                        && dep != R_NilValue()
                        && TYPEOF(dep) == SEXPTYPE::STRSXP
                        && xlength(dep) > 0
                    {
                        SET_STRING_ELT(rval, i, STRING_ELT(dep, 0));
                    } else {
                        SET_STRING_ELT(rval, i, R_NaString());
                    }
                }
            }
            return rval;
        }

        if isVectorizable(v) {
            let n = xlength(v);
            let rval = Rf_allocVector3(type_.0, n);
            let _rval_guard = protect(rval);
            match type_.0 {
                t if t == SEXPTYPE::LGLSXP => {
                    for i in 0..n {
                        *LOGICAL(rval).add(i as usize) = asLogical(VECTOR_ELT(v, i));
                    }
                }
                t if t == SEXPTYPE::INTSXP => {
                    for i in 0..n {
                        *INTEGER(rval).add(i as usize) = asInteger(VECTOR_ELT(v, i));
                    }
                }
                t if t == SEXPTYPE::REALSXP => {
                    for i in 0..n {
                        *REAL(rval).add(i as usize) = asReal(VECTOR_ELT(v, i));
                    }
                }
                t if t == SEXPTYPE::CPLXSXP => {
                    for i in 0..n {
                        *COMPLEX(rval).add(i as usize) = asComplex(VECTOR_ELT(v, i));
                    }
                }
                t if t == SEXPTYPE::RAWSXP => {
                    for i in 0..n {
                        let tmp = asInteger(VECTOR_ELT(v, i));
                        if tmp < 0 || tmp > 255 {
                            warn |= WARN_RAW;
                        }
                        *RAW(rval).add(i as usize) = if tmp < 0 || tmp > 255 {
                            0
                        } else {
                            tmp as Rbyte
                        };
                    }
                }
                _ => {} // intentionally unhandled: unsupported SEXPTYPE for coercion
            }
            if warn != 0 {
                CoercionWarning(warn);
            }
            let names = getAttrib(v, R_NamesSymbol());
            if !isNull(names) {
                setAttrib(rval, R_NamesSymbol(), names);
            }
            return rval;
        }

        error("list object cannot be coerced to type");
    }
}

/// Coerce to a symbol.
pub unsafe fn coerceToSymbol(v: SEXP) -> SEXP {
    unsafe {
        let mut warn: c_int = 0;
        if LENGTH(v) <= 0 {
            error("invalid data of mode (too short)");
        }

        let ans = match TYPEOF(v) {
            t if t == SEXPTYPE::LGLSXP => StringFromLogical(LOGICAL_ELT(v, 0)),
            t if t == SEXPTYPE::INTSXP => StringFromInteger(INTEGER_ELT(v, 0), &mut warn),
            t if t == SEXPTYPE::REALSXP => {
                crate::mainutils::printutils::StringFromReal(REAL_ELT(v, 0), &mut warn)
            }
            t if t == SEXPTYPE::CPLXSXP => StringFromComplex(COMPLEX_ELT(v, 0), &mut warn),
            t if t == SEXPTYPE::STRSXP => STRING_ELT(v, 0),
            t if t == SEXPTYPE::RAWSXP => StringFromRaw(RAW_ELT(v, 0), &mut warn),
            _ => R_NilValue(),
        };
        let _ans_guard = protect(ans);

        if warn != 0 {
            CoercionWarning(warn);
        }

        let sym = Rf_install(CHAR(ans));
        sym
    }
}

/// Coerce a symbol (SYMSXP) to the given type.
/// This matches R's coerceSymbol() from coerce.c.
pub unsafe fn coerceSymbol(v: SEXP, type_: SEXPTYPE) -> SEXP {
    unsafe {
        if type_ == SEXPTYPE::EXPRSXP {
            let rval = Rf_allocVector3(type_.0, 1);
            let _rval_guard = protect(rval);
            SET_VECTOR_ELT(rval, 0, v);
            rval
        } else if type_ == SEXPTYPE::CHARSXP {
            PRINTNAME(v)
        } else if type_ == SEXPTYPE::STRSXP {
            Rf_ScalarString(PRINTNAME(v))
        } else {
            let target_name = CStr::from_ptr(type2char(type_.0)).to_string_lossy();
            error(&format!(
                "'symbol' object cannot be coerced to type '{}'",
                target_name
            ));
        }
    }
}

/// Create a tag (symbol) from an SEXP.
/// If x is already a symbol or NULL, return it. If x is a string of length >= 1,
/// install it as a symbol.
pub unsafe fn CreateTag(x: SEXP) -> SEXP {
    unsafe {
        if isNull(x) || isSymbol(x) {
            return x;
        }
        if isString(x) && LENGTH(x) >= 1 {
            let s = STRING_ELT(x, 0);
            if !isNull(s) {
                let cs = CHAR(s);
                if !cs.is_null() && *cs != 0 {
                    return installTrChar(s);
                }
            }
        }
        // fallback: return NULL
        R_NilValue()
    }
}

/// Convert an SEXP to a function (closure).
/// This matches R's asFunction() from coerce.c.
pub unsafe fn asFunction(x: SEXP) -> SEXP {
    unsafe {
        if isFunction(x) {
            return x;
        }
        let f = allocSExp(SEXPTYPE::CLOSXP);
        let _f_guard = protect(f);
        SET_CLOENV(f, R_GlobalEnv());
        // For simplicity, create a closure with empty formals and body = x
        SET_FORMALS(f, R_NilValue());
        SET_BODY(f, x);
        f
    }
}

/// Common coercion helper for as.vector / typed coercion dispatch.
/// This matches R's ascommon() from coerce.c.
pub unsafe fn ascommon(call: SEXP, u: SEXP, type_: c_int) -> SEXP {
    unsafe {
        let target_type = SEXPTYPE(type_);

        if target_type == SEXPTYPE::CLOSXP {
            return asFunction(u);
        }

        if isVector(u)
            || isList(u)
            || isLanguage(u)
            || (isSymbol(u) && target_type == SEXPTYPE::EXPRSXP)
        {
            let v = if type_ != SEXPTYPE::ANYSXP && TYPEOF(u) != type_ {
                coerceVector(u, type_)
            } else {
                u
            };

            // Drop attributes for certain types (as.pairlist behavior)
            if target_type == SEXPTYPE::LISTSXP
                && TYPEOF(u) != SEXPTYPE::LANGSXP
                && TYPEOF(u) != SEXPTYPE::LISTSXP
                && TYPEOF(u) != SEXPTYPE::EXPRSXP
                && TYPEOF(u) != SEXPTYPE::VECSXP
            {
                // Clear attributes
                let attr = ATTRIB(v);
                if !isNull(attr) {
                    SET_ATTRIB(v, R_NilValue());
                }
            }
            return v;
        }

        if isSymbol(u) && target_type == SEXPTYPE::STRSXP {
            return Rf_ScalarString(PRINTNAME(u));
        }
        if isSymbol(u) && target_type == SEXPTYPE::SYMSXP {
            return u;
        }
        if isSymbol(u) && target_type == SEXPTYPE::VECSXP {
            let v = Rf_allocVector3(SEXPTYPE::VECSXP, 1);
            SET_VECTOR_ELT(v, 0, u);
            return v;
        }

        errorcall(call, "cannot coerce type to vector of type");
    }
}

// ---------------------------------------------------------------------------
// coerceVector -- main coercion dispatcher
// ---------------------------------------------------------------------------

/// Coerce a vector from one type to another.
///
/// This is the main entry point for type coercion in R, equivalent to
/// R's `coerceVector()` from coerce.c. It dispatches to the appropriate
/// type-specific coercion function based on the source and target types.
pub unsafe fn coerceVector(v: SEXP, type_: c_int) -> SEXP {
    unsafe {
        if v.is_null() {
            return ptr::null_mut();
        }
        let target = SEXPTYPE(type_);

        // If already the right type, return as-is
        if TYPEOF(v) == type_ {
            return v;
        }

        let _v_guard = protect(v);

        let ans = match TYPEOF(v) {
            t if t == SEXPTYPE::SYMSXP => coerceSymbol(v, target),
            t if t == SEXPTYPE::NILSXP => {
                // Trunk coerceVector: NULL coerces to a zero-length vector of
                // the target type (needed for `NULL[2] <- 'z'` growth, where
                // do_subassign_dflt coerces the NULL LHS to TYPEOF(y)).
                Rf_allocVector3(target, 0)
            }
            t if t == SEXPTYPE::LISTSXP => {
                if type_ == SEXPTYPE::LISTSXP {
                    v // already pairlist
                } else {
                    coercePairList(v, target)
                }
            }
            t if t == SEXPTYPE::LANGSXP => {
                if type_ != SEXPTYPE::STRSXP {
                    coercePairList(v, target)
                } else {
                    // LANGSXP -> STRSXP: special handling for operator names
                    let n = LENGTH(v);
                    let ans = Rf_allocVector3(SEXPTYPE::STRSXP, n as R_xlen_t);
                    let mut vp = v;
                    for i in 0..n as R_xlen_t {
                        let car = CAR(vp);
                        if isString(car) && LENGTH(car) == 1 {
                            SET_STRING_ELT(ans, i, STRING_ELT(car, 0));
                        } else if isSymbol(car) {
                            SET_STRING_ELT(ans, i, PRINTNAME(car));
                        } else {
                            SET_STRING_ELT(ans, i, StringFromLogical(0));
                        }
                        vp = CDR(vp);
                    }
                    ans
                }
            }
            t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => coerceVectorList(v, target),
            t if t == SEXPTYPE::ENVSXP => {
                error("environments cannot be coerced to other types");
            }
            // Atomic vector types
            t if t == SEXPTYPE::LGLSXP
                || t == SEXPTYPE::INTSXP
                || t == SEXPTYPE::REALSXP
                || t == SEXPTYPE::CPLXSXP
                || t == SEXPTYPE::STRSXP
                || t == SEXPTYPE::RAWSXP =>
            {
                match type_ {
                    t if t == SEXPTYPE::SYMSXP => coerceToSymbol(v),
                    t if t == SEXPTYPE::LGLSXP => coerceToLogical(v),
                    t if t == SEXPTYPE::INTSXP => coerceToInteger(v),
                    t if t == SEXPTYPE::REALSXP => coerceToReal(v),
                    t if t == SEXPTYPE::CPLXSXP => coerceToComplex(v),
                    t if t == SEXPTYPE::RAWSXP => coerceToRaw(v),
                    t if t == SEXPTYPE::STRSXP => coerceToString(v),
                    t if t == SEXPTYPE::EXPRSXP => coerceToExpression(v),
                    t if t == SEXPTYPE::VECSXP => coerceToVectorList(v),
                    t if t == SEXPTYPE::LISTSXP => coerceToPairList(v),
                    _ => {
                        error("cannot coerce type to vector of type");
                    }
                }
            }
            _ => {
                error("cannot coerce type to vector of type");
            }
        };

        ans
    }
}
