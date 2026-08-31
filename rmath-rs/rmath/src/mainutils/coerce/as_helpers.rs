use super::*;


// ---------------------------------------------------------------------------
// asLogical -- coerce first element to logical
// ---------------------------------------------------------------------------

/// Convert the first element of a vector to a logical value.
///
/// This is R's `asLogical()` from coerce.c. Returns NA_LOGICAL for
/// empty vectors, and dispatches based on the vector's type.
pub unsafe fn asLogical(x: SEXP) -> c_int {
    unsafe { asLogical2(x, 0, R_NilValue()) }
}

/// Convert the first element of a vector to a logical value, with length checking.
///
/// This is R's `asLogical2()` from coerce.c.
pub unsafe fn asLogical2(x: SEXP, checking: c_int, _call: SEXP) -> c_int {
    unsafe {
        let mut warn: c_int = 0;

        if isVectorAtomic(x) {
            if xlength(x) < 1 {
                return NA_LOGICAL;
            }
            if checking != 0 && xlength(x) > 1 {
                // In R this calls errorcall; we just proceed
            }
            match TYPEOF(x) {
                t if t == SEXPTYPE::LGLSXP => LOGICAL_ELT(x, 0),
                t if t == SEXPTYPE::INTSXP => LogicalFromInteger(INTEGER_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::REALSXP => LogicalFromReal(REAL_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::CPLXSXP => LogicalFromComplex(COMPLEX_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::STRSXP => LogicalFromString(STRING_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::RAWSXP => LogicalFromInteger(RAW_ELT(x, 0) as c_int, &mut warn),
                _ => NA_LOGICAL,
            }
        } else if TYPEOF(x) == SEXPTYPE::CHARSXP {
            LogicalFromString(x, &mut warn)
        } else {
            NA_LOGICAL
        }
    }
}

// ---------------------------------------------------------------------------
// asInteger -- coerce first element to integer
// ---------------------------------------------------------------------------

/// Convert the first element of a vector to an integer value.
///
/// This is R's `asInteger()` from coerce.c.
pub unsafe fn asInteger(x: SEXP) -> c_int {
    unsafe {
        let mut warn: c_int = 0;

        if isVectorAtomic(x) && xlength(x) >= 1 {
            let res = match TYPEOF(x) {
                t if t == SEXPTYPE::RAWSXP => RAW_ELT(x, 0) as c_int,
                t if t == SEXPTYPE::LGLSXP => IntegerFromLogical(LOGICAL_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::INTSXP => INTEGER_ELT(x, 0),
                t if t == SEXPTYPE::REALSXP => IntegerFromReal(REAL_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::CPLXSXP => IntegerFromComplex(COMPLEX_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::STRSXP => IntegerFromString(STRING_ELT(x, 0), &mut warn),
                _ => NA_INTEGER,
            };
            if warn != 0 {
                CoercionWarning(warn);
            }
            return res;
        } else if TYPEOF(x) == SEXPTYPE::CHARSXP {
            let res = IntegerFromString(x, &mut warn);
            if warn != 0 {
                CoercionWarning(warn);
            }
            return res;
        }

        NA_INTEGER
    }
}

// ---------------------------------------------------------------------------
// asReal -- coerce first element to real (double)
// ---------------------------------------------------------------------------

/// Convert the first element of a vector to a real (double) value.
///
/// This is R's `asReal()` from coerce.c.
pub unsafe fn asReal(x: SEXP) -> c_double {
    unsafe {
        let mut warn: c_int = 0;

        if isVectorAtomic(x) && xlength(x) >= 1 {
            let res = match TYPEOF(x) {
                t if t == SEXPTYPE::LGLSXP => RealFromLogical(LOGICAL_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::INTSXP => RealFromInteger(INTEGER_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::REALSXP => REAL_ELT(x, 0),
                t if t == SEXPTYPE::CPLXSXP => RealFromComplex(COMPLEX_ELT(x, 0), &mut warn),
                t if t == SEXPTYPE::STRSXP => RealFromString(STRING_ELT(x, 0), &mut warn),
                _ => NA_REAL,
            };
            if warn != 0 {
                CoercionWarning(warn);
            }
            return res;
        } else if TYPEOF(x) == SEXPTYPE::CHARSXP {
            let res = RealFromString(x, &mut warn);
            if warn != 0 {
                CoercionWarning(warn);
            }
            return res;
        }

        NA_REAL
    }
}

/// Predicate helper kept for compatibility with translated library code.
pub unsafe fn isReal(x: SEXP) -> c_int {
    unsafe { crate::mainutils::relop::isReal(x) }
}

// ---------------------------------------------------------------------------
// asComplex -- coerce first element to complex
// ---------------------------------------------------------------------------

/// Convert the first element of a vector to a complex value.
///
/// This is R's `asComplex()` from coerce.c.
pub unsafe fn asComplex(x: SEXP) -> Rcomplex {
    unsafe {
        let mut warn: c_int = 0;
        let mut z = Rcomplex {
            r: NA_REAL,
            i: NA_REAL,
        };

        if isVectorAtomic(x) && xlength(x) >= 1 {
            match TYPEOF(x) {
                t if t == SEXPTYPE::LGLSXP => {
                    z = ComplexFromLogical(LOGICAL_ELT(x, 0), &mut warn);
                }
                t if t == SEXPTYPE::INTSXP => {
                    z = ComplexFromInteger(INTEGER_ELT(x, 0), &mut warn);
                }
                t if t == SEXPTYPE::REALSXP => {
                    z = ComplexFromReal(REAL_ELT(x, 0), &mut warn);
                }
                t if t == SEXPTYPE::CPLXSXP => {
                    z = COMPLEX_ELT(x, 0);
                }
                t if t == SEXPTYPE::STRSXP => {
                    z = ComplexFromString(STRING_ELT(x, 0), &mut warn);
                }
                _ => {} // intentionally unhandled: unsupported SEXPTYPE for complex coercion
            }
            if warn != 0 {
                CoercionWarning(warn);
            }
            return z;
        } else if TYPEOF(x) == SEXPTYPE::CHARSXP {
            z = ComplexFromString(x, &mut warn);
            if warn != 0 {
                CoercionWarning(warn);
            }
            return z;
        }

        z
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// asRaw -- coerce first element to raw byte
// ---------------------------------------------------------------------------

/// Convert the first element of a vector to a raw byte value.
///
/// This follows the same pattern as asInteger/asReal, returning 0 for
/// out-of-range or NA values.
pub unsafe fn asRaw(x: SEXP) -> Rbyte {
    unsafe {
        if isVectorAtomic(x) && xlength(x) >= 1 {
            let val = asInteger(x);
            if val == NA_INTEGER || val < 0 || val > 255 {
                return 0;
            }
            return val as Rbyte;
        }
        0
    }
}

// ---------------------------------------------------------------------------
// asRbool / asBool -- coerce to boolean (error on NA)
// ---------------------------------------------------------------------------

/// Coerce to Rboolean (c_int), erroring on NA_LOGICAL.
/// This matches R's asRboolean() from coerce.c.
pub unsafe fn asRbool(x: SEXP, call: SEXP) -> c_int {
    unsafe {
        let ans = asLogical2(x, 1, call);
        if ans == NA_LOGICAL {
            errorcall(call, "NA in coercion to boolean");
        }
        ans
    }
}

/// Coerce to bool, erroring on NA_LOGICAL.
/// This matches R's asBool() from coerce.c.
pub unsafe fn asBool(x: SEXP) -> c_int {
    unsafe {
        let ans = asLogical2(x, 1, R_NilValue());
        if ans == NA_LOGICAL {
            error("NA in coercion to boolean");
        }
        ans
    }
}

// ---------------------------------------------------------------------------
// asCharacterFactor -- convert factor to character
// ---------------------------------------------------------------------------

/// Convert a factor to a character vector using its levels.
///
/// This is R's `asCharacterFactor()` from coerce.c.
pub unsafe fn asCharacterFactor(x: SEXP) -> SEXP {
    unsafe {
        let n = xlength(x);
        let labels = getAttrib(x, R_LevelsSymbol());
        if TYPEOF(labels) != SEXPTYPE::STRSXP {
            error("malformed factor");
        }
        let nl = LENGTH(labels);

        let ans = Rf_allocVector3(SEXPTYPE::STRSXP, n);
        let _ans_guard = protect(ans);
        for i in 0..n {
            let ii = INTEGER_ELT(x, i as c_int);
            if ii == NA_INTEGER {
                SET_STRING_ELT(ans, i, R_NaString());
            } else if ii >= 1 && ii <= nl {
                SET_STRING_ELT(ans, i, STRING_ELT(labels, (ii - 1) as R_xlen_t));
            } else {
                error("malformed factor");
            }
        }

        ans
    }
}

