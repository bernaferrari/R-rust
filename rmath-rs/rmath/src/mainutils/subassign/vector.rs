#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(unused_imports)]

//! `VectorAssign` — the `[<-` core for atomic vectors and lists, plus vector
//! enlargement, subscript type fixing, and list-element deletion helpers.

use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::mainutils::subscript::{
    OneIndex, get1index, int_arraySubscript, makeSubscript, mat2indsub, strmat2intmat, vectorIndex,
};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::envir::defineVar;
use crate::sexp::ffi::{FALSE, NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory_ext::{allocList, allocSExp};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use super::support::{SET_S4_OBJECT, SET_TRUELENGTH, UNSET_S4_OBJECT};
use super::*;

// ---------------------------------------------------------------------------
// Internal helper functions
// ---------------------------------------------------------------------------

/// Port of `getNames()` -- retrieves names attribute from a vector,
/// deferring to getAttrib if a 'dim' attribute is present.
pub(crate) unsafe fn getNames(x: SEXP) -> SEXP {
    unsafe {
        use crate::eval::attrib_core::R_DimSymbol;

        let mut attr = ATTRIB(x);
        while !isNull(attr) {
            if TAG(attr) == R_DimSymbol() {
                return getAttrib(x, crate::eval::attrib_core::R_NamesSymbol());
            }
            attr = CDR(attr);
        }

        // Don't use getAttrib since that would mark as immutable
        attr = ATTRIB(x);
        while !isNull(attr) {
            if TAG(attr) == crate::eval::attrib_core::R_NamesSymbol() {
                return CAR(attr);
            }
            attr = CDR(attr);
        }

        R_NilValue()
    }
}

/// Port of `EnlargeVector()` -- changes vector length to newlen,
/// allowing assignment past the end of a vector.
pub(crate) unsafe fn EnlargeVector(x: SEXP, newlen: R_xlen_t) -> SEXP {
    unsafe {
        let len = XLENGTH(x);
        let newtruelen: R_xlen_t;
        if newlen > len {
            let expanded_nlen = (newlen as f64) * 1.05;
            if expanded_nlen <= R_XLEN_T_MAX as f64 {
                newtruelen = expanded_nlen as R_xlen_t;
            } else {
                newtruelen = newlen;
            }
        } else {
            newtruelen = newlen;
        }

        let _x_guard = protect(x);
        let newx = Rf_allocVector3(TYPEOF(x), newtruelen);
        let _newx_guard = protect(newx);

        // Copy the elements into place.
        let xtype = TYPEOF(x);
        if xtype == LGLSXP || xtype == INTSXP {
            let px = INTEGER(newx);
            let px_src = INTEGER(x);
            for i in 0..len {
                *px.add(i as usize) = *px_src.add(i as usize);
            }
            for i in len..newtruelen {
                *px.add(i as usize) = NA_INTEGER;
            }
        } else if xtype == REALSXP {
            let px = REAL(newx);
            let px_src = REAL(x);
            for i in 0..len {
                *px.add(i as usize) = *px_src.add(i as usize);
            }
            for i in len..newtruelen {
                *px.add(i as usize) = NA_REAL;
            }
        } else if xtype == CPLXSXP {
            let px = COMPLEX(newx);
            let px_src = COMPLEX(x);
            for i in 0..len {
                *px.add(i as usize) = *px_src.add(i as usize);
            }
            for i in len..newtruelen {
                (*px.add(i as usize)).r = NA_REAL;
                (*px.add(i as usize)).i = 0.0;
            }
        } else if xtype == STRSXP {
            for i in 0..len {
                SET_STRING_ELT(newx, i, STRING_ELT(x, i));
            }
            for i in len..newtruelen {
                SET_STRING_ELT(newx, i, NA_STRING());
            }
        } else if xtype == EXPRSXP || xtype == VECSXP {
            for i in 0..len {
                SET_VECTOR_ELT(newx, i, VECTOR_ELT(x, i));
            }
            for i in len..newtruelen {
                SET_VECTOR_ELT(newx, i, R_NilValue());
            }
        } else if xtype == RAWSXP {
            let px = RAW(newx);
            let px_src = RAW(x);
            for i in 0..len {
                *px.add(i as usize) = *px_src.add(i as usize);
            }
            for i in len..newtruelen {
                *px.add(i as usize) = 0;
            }
        }

        if newlen < newtruelen {
            SET_GROWABLE_BIT(newx);
            SET_TRUELENGTH(newx, newtruelen as c_int);
            SET_STDVEC_LENGTH(newx, newlen);
        }

        // Adjust the attribute list.
        let names = getNames(x);
        if !isNull(names) {
            let enlarged = EnlargeNames(names, len, newlen);
            setAttrib(newx, crate::eval::attrib_core::R_NamesSymbol(), enlarged);
        }
        copyMostAttrib(x, newx);
        newx
    }
}

/// Port of `EnlargeNames()` -- grows a names attribute vector.
pub(crate) unsafe fn EnlargeNames(names: SEXP, len: R_xlen_t, newlen: R_xlen_t) -> SEXP {
    unsafe {
        if TYPEOF(names) != STRSXP || XLENGTH(names) != len {
            // Error case - just return names unchanged
            return names;
        }
        let newnames = EnlargeVector(names, newlen);
        let _newnames_guard = protect(newnames);
        for i in len..newlen {
            SET_STRING_ELT(newnames, i, R_BlankString());
        }
        newnames
    }
}

/// Port of `embedInVector()` -- embeds a non-vector in a list for
/// SubassignTypeFix (used for S4 objects).
pub(crate) unsafe fn embedInVector(v: SEXP, _call: SEXP) -> SEXP {
    unsafe {
        let ans = Rf_allocVector3(VECSXP, 1);
        let _ans_guard = protect(ans);
        SET_VECTOR_ELT(ans, 0, v);
        ans
    }
}

/// Port of `dispatch_asvector()` -- dispatches as.vector method.
pub(crate) unsafe fn dispatch_asvector(_x: *mut SEXP, _call: SEXP, _rho: SEXP) -> bool {
    false
}

/// Port of `SubassignTypeFix()` -- coerces LHS/RHS to compatible types
/// for subassignment. Returns the type code `100 * TYPEOF(x) + TYPEOF(y)`.
pub(crate) unsafe fn SubassignTypeFix(
    x: *mut SEXP,
    y: *mut SEXP,
    stretch: R_xlen_t,
    level: c_int,
    call: SEXP,
    rho: SEXP,
) -> c_int {
    unsafe {
        let mut redo_which = true;
        let which = 100 * TYPEOF(*x) + TYPEOF(*y);
        let x_is_object = isObject(*x);

        match which {
            // No coercion needed
            1000 | 1300 | 1400 | 1500 | 1600 | 1900 | 2000 | 2400 | 1010 | 1310 | 1410 | 1510
            | 1313 | 1413 | 1513 | 1414 | 1514 | 1515 | 1616 | 1919 | 2020 | 2424 => {
                redo_which = false;
            }

            1013 => {
                // logical <- integer
                *x = coerceVector(*x, INTSXP);
            }

            1014 | 1314 => {
                // logical/integer <- real
                *x = coerceVector(*x, REALSXP);
            }

            1015 | 1315 | 1415 => {
                // logical/integer/real <- complex
                *x = coerceVector(*x, CPLXSXP);
            }

            1610 | 1613 | 1614 | 1615 => {
                // character <- logical/integer/real/complex
                *y = coerceVector(*y, STRSXP);
            }

            1016 | 1316 | 1416 | 1516 => {
                // logical/integer/real/complex <- character
                *x = coerceVector(*x, STRSXP);
            }

            1901 | 1902 | 1904 | 1905 | 1906 | 1910 | 1913 | 1914 | 1915 | 1916 | 1920 | 1921
            | 1922 | 1923 | 1924 | 1903 | 1907 | 1908 | 1999 => {
                // vector <- various
                if level == 1 {
                    *y = coerceVector(*y, VECSXP);
                } else {
                    redo_which = false;
                }
            }

            1925 => {
                // vector <- S4/OBJ
                if level == 1 {
                    *y = embedInVector(*y, call);
                } else {
                    redo_which = false;
                }
            }

            1019 | 1319 | 1419 | 1519 | 1619 | 2419 => {
                // various <- vector
                *x = coerceVector(*x, VECSXP);
            }

            1020 | 1320 | 1420 | 1520 | 1620 | 2420 => {
                // various <- expression
                *x = coerceVector(*x, EXPRSXP);
            }

            2001 | 2002 | 2006 | 2010 | 2013 | 2014 | 2015 | 2016 | 2019 => {
                // expression <- various
                if level == 1 {
                    *y = coerceVector(*y, VECSXP);
                } else {
                    redo_which = false;
                }
            }

            2025 => {
                // expression <- S4/OBJ
                if level == 1 {
                    *y = embedInVector(*y, call);
                } else {
                    redo_which = false;
                }
            }

            1025 | 1325 | 1425 | 1525 | 1625 | 2425 => {
                // various <- S4|OBJ
                if dispatch_asvector(y, call, rho) {
                    // dispatch_asvector() leaves the new *y unprotected; the
                    // recursive call below may allocate (coerceVector), so the
                    // new value has to be protected (upstream GC fix):
                    let y_guard = protect(*y);
                    let which = SubassignTypeFix(x, y, stretch, level, call, rho);
                    drop(y_guard);
                    return which;
                }
            }

            _ => {
                // Incompatible types - just return which
            }
        }

        if stretch > 0 {
            let _y_guard = protect(*y);
            *x = EnlargeVector(*x, stretch);
        }
        SET_OBJECT(*x, x_is_object as c_int);

        if redo_which {
            100 * TYPEOF(*x) + TYPEOF(*y)
        } else {
            which
        }
    }
}

/// Port of `gi()` -- gets an index value from an integer or real subscript vector.
pub(crate) unsafe fn gi(indx: SEXP, i: R_xlen_t) -> R_xlen_t {
    unsafe {
        if TYPEOF(indx) == REALSXP {
            let d = REAL_ELT(indx, i as c_int);
            if R_FINITE(d) {
                d as R_xlen_t
            } else {
                NA_INTEGER as R_xlen_t
            }
        } else {
            INTEGER_ELT(indx, i as c_int) as R_xlen_t
        }
    }
}

/// Port of `DeleteListElements()` -- removes specified elements from a vector list.
pub(crate) unsafe fn DeleteListElements(x: SEXP, which: SEXP) -> SEXP {
    unsafe {
        let len = XLENGTH(x);
        let lenw = XLENGTH(which);

        let include = Rf_allocVector3(INTSXP, len);
        let _include_guard = protect(include);
        let pinclude = INTEGER(include);
        for i in 0..len {
            *pinclude.add(i as usize) = 1;
        }
        for i in 0..lenw {
            let ii = gi(which, i);
            if ii > 0 && ii <= len {
                *pinclude.add((ii - 1) as usize) = 0;
            }
        }

        let mut ii: R_xlen_t = 0;
        for i in 0..len {
            ii += *pinclude.add(i as usize) as R_xlen_t;
        }
        if ii == len {
            return x;
        }

        let xnew = Rf_allocVector3(TYPEOF(x), ii);
        let _xnew_guard = protect(xnew);
        let mut k: R_xlen_t = 0;
        for i in 0..len {
            if *pinclude.add(i as usize) == 1 {
                SET_VECTOR_ELT(xnew, k, VECTOR_ELT(x, i));
                k += 1;
            }
        }

        let xnames = getAttrib(x, crate::eval::attrib_core::R_NamesSymbol());
        let _xnames_guard = protect(xnames);
        if !isNull(xnames) {
            let xnewnames = Rf_allocVector3(STRSXP, ii);
            let _xnewnames_guard = protect(xnewnames);
            k = 0;
            for i in 0..len {
                if *pinclude.add(i as usize) == 1 {
                    SET_STRING_ELT(xnewnames, k, STRING_ELT(xnames, i));
                    k += 1;
                }
            }
            setAttrib(xnew, crate::eval::attrib_core::R_NamesSymbol(), xnewnames);
        }
        copyMostAttrib(x, xnew);
        xnew
    }
}

/// Port of `VECTOR_ELT_FIX_NAMED()` -- sets NAMED=NAMEDMAX if needed for PR15098.
pub(crate) unsafe fn VECTOR_ELT_FIX_NAMED(y: SEXP, i: R_xlen_t) -> SEXP {
    unsafe {
        let val = VECTOR_ELT(y, i);
        if NAMED(y) != 0 || NAMED(val) != 0 {
            ENSURE_NAMEDMAX(val);
        }
        val
    }
}

// ---------------------------------------------------------------------------
// VectorAssign
// ---------------------------------------------------------------------------

/// Port of `VectorAssign()` -- handles `x[s] <- y` for vectors.
pub(crate) unsafe fn VectorAssign(call: SEXP, rho: SEXP, x: SEXP, s: SEXP, y: SEXP) -> SEXP {
    unsafe {
        use crate::eval::attrib_core::R_DimSymbol;

        // Quick return for simple scalar case
        if isNull(ATTRIB(s)) && TYPEOF(x) == REALSXP && IS_SCALAR(y, REALSXP) != 0 {
            // Note: IS_SCALAR only inspects the scalar flag; the element type
            // must be verified separately before using the typed accessors.
            if TYPEOF(s) == INTSXP && IS_SCALAR(s, INTSXP) != 0 {
                let ival = SCALAR_IVAL(s) as R_xlen_t;
                let ival_ok = ival != NA_INTEGER as i64 && ival >= 1 && ival <= XLENGTH(x);
                if ival_ok {
                    *REAL(x).add((ival - 1) as usize) = SCALAR_DVAL(y);
                    return x;
                }
            } else if TYPEOF(s) == REALSXP && IS_SCALAR(s, REALSXP) != 0 {
                let dval = SCALAR_DVAL(s);
                if R_FINITE(dval) {
                    let ival = dval as R_xlen_t;
                    if ival >= 1 && ival <= XLENGTH(x) {
                        *REAL(x).add((ival - 1) as usize) = SCALAR_DVAL(y);
                        return x;
                    }
                }
            }
        }

        if isNull(x) && isNull(y) {
            return R_NilValue();
        }

        // Check for special matrix subscripting.
        let mut s = s;
        let mut s_guard = protect(s);
        if !isNull(ATTRIB(s)) {
            let dim = getAttrib(x, R_DimSymbol());
            if isMatrix(s) && isArray(x) && ncols(s) == Rf_length(dim) {
                if isString(s) {
                    let dnames = GetArrayDimnames(x);
                    let dnames_guard = protect(dnames);
                    let intmat = strmat2intmat(s, dnames, call, x);
                    drop(dnames_guard);
                    drop(s_guard);
                    s = intmat;
                    s_guard = protect(s);
                }
                if isInteger(s) || isReal(s) {
                    let indsub = mat2indsub(dim, s, R_NilValue(), x);
                    drop(s_guard);
                    s = indsub;
                    s_guard = protect(s);
                }
            }
        }

        let stretch: R_xlen_t = 1;
        let indx = makeSubscript(x, s, &stretch as *const _ as *mut R_xlen_t, R_NilValue());
        let _indx_guard = protect(indx);
        let n = XLENGTH(indx);

        // NAs are not allowed in subscripted assignments. Upstream
        // (subassign.c) raises this while processing the subscript, before any
        // typed assignment arm; `gi()` maps NA indices to the NA_INTEGER
        // sentinel for both INTSXP and expanded-logical subscripts.
        for i in 0..n {
            if gi(indx, i) == NA_INTEGER as R_xlen_t {
                crate::mainutils::errors::Rf_error(
                    b"NAs are not allowed in subscripted assignments\0".as_ptr()
                        as *const core::ffi::c_char,
                );
            }
        }

        let old_x = x;
        let mut x = x;
        let mut y = y;
        let which = SubassignTypeFix(&mut x, &mut y, stretch, 1, call, rho);

        if n == 0 {
            return x;
        }

        let ny = XLENGTH(y);
        let nx = XLENGTH(x);
        let _x_guard = protect(x);

        let is_list_target = TYPEOF(x) == VECSXP || TYPEOF(x) == EXPRSXP;
        if !is_list_target || isNull(y) {
            // Check length compatibility
            if n > 0 && ny == 0 {
                crate::mainutils::errors::Rf_error(
                    b"replacement has length zero\0".as_ptr() as *const core::ffi::c_char
                );
            }
        }

        // Warn about non-multiple recycling
        if ny != 0 && n % ny != 0 {
            crate::mainutils::errors::warningcall(
                call,
                b"number of items to replace is not a multiple of replacement length\0".as_ptr()
                    as *const core::ffi::c_char,
            );
        }

        // Duplicate y if x == y
        let _y_guard = if x == y {
            y = shallow_duplicate(y);
            protect(y)
        } else {
            protect(y)
        };

        match which {
            1010 | 1310 | 1313 => {
                // logical <- logical, integer <- logical, integer <- integer
                let px = INTEGER(x);
                let y_is_int = TYPEOF(y) == SEXPTYPE::INTSXP;
                let mut iny: R_xlen_t = 0;
                for idx in 0..n {
                    let ii = gi(indx, idx);
                    if ii == NA_INTEGER as R_xlen_t {
                        continue;
                    }
                    let ii = ii - 1;
                    *px.add(ii as usize) = if y_is_int {
                        INTEGER_ELT(y, iny as c_int)
                    } else {
                        LOGICAL_ELT(y, iny as c_int)
                    };
                    iny += 1;
                    if iny >= ny {
                        iny = 0;
                    }
                }
            }

            1410 | 1413 => {
                // real <- logical/integer
                let px = REAL(x);
                let y_is_int = TYPEOF(y) == SEXPTYPE::INTSXP;
                let mut iny: R_xlen_t = 0;
                for idx in 0..n {
                    let ii = gi(indx, idx);
                    if ii == NA_INTEGER as R_xlen_t {
                        continue;
                    }
                    let ii = ii - 1;
                    let iy = if y_is_int {
                        INTEGER_ELT(y, iny as c_int)
                    } else {
                        LOGICAL_ELT(y, iny as c_int)
                    };
                    if iy == NA_INTEGER {
                        *px.add(ii as usize) = NA_REAL;
                    } else {
                        *px.add(ii as usize) = iy as c_double;
                    }
                    iny += 1;
                    if iny >= ny {
                        iny = 0;
                    }
                }
            }

            1414 => {
                // real <- real
                let px = REAL(x);
                let mut iny: R_xlen_t = 0;
                for idx in 0..n {
                    let ii = gi(indx, idx);
                    if ii == NA_INTEGER as R_xlen_t {
                        continue;
                    }
                    let ii = ii - 1;
                    *px.add(ii as usize) = REAL_ELT(y, iny as c_int);
                    iny += 1;
                    if iny >= ny {
                        iny = 0;
                    }
                }
            }

            1510 | 1513 => {
                // complex <- logical/integer
                let px = COMPLEX(x);
                let mut iny: R_xlen_t = 0;
                for idx in 0..n {
                    let ii = gi(indx, idx);
                    if ii == NA_INTEGER as R_xlen_t {
                        continue;
                    }
                    let ii = ii - 1;
                    let iy = if TYPEOF(y) == SEXPTYPE::INTSXP {
                        INTEGER_ELT(y, iny as c_int)
                    } else {
                        LOGICAL_ELT(y, iny as c_int)
                    };
                    if iy == NA_INTEGER {
                        (*px.add(ii as usize)).r = NA_REAL;
                        (*px.add(ii as usize)).i = 0.0;
                    } else {
                        (*px.add(ii as usize)).r = iy as c_double;
                        (*px.add(ii as usize)).i = 0.0;
                    }
                    iny += 1;
                    if iny >= ny {
                        iny = 0;
                    }
                }
            }

            1514 => {
                // complex <- real
                let px = COMPLEX(x);
                let mut iny: R_xlen_t = 0;
                for idx in 0..n {
                    let ii = gi(indx, idx);
                    if ii == NA_INTEGER as R_xlen_t {
                        continue;
                    }
                    let ii = ii - 1;
                    let ry = REAL_ELT(y, iny as c_int);
                    if ISNA(ry) {
                        (*px.add(ii as usize)).r = NA_REAL;
                        (*px.add(ii as usize)).i = 0.0;
                    } else {
                        (*px.add(ii as usize)).r = ry;
                        (*px.add(ii as usize)).i = 0.0;
                    }
                    iny += 1;
                    if iny >= ny {
                        iny = 0;
                    }
                }
            }

            1515 => {
                // complex <- complex
                let px = COMPLEX(x);
                let mut iny: R_xlen_t = 0;
                for idx in 0..n {
                    let ii = gi(indx, idx);
                    if ii == NA_INTEGER as R_xlen_t {
                        continue;
                    }
                    let ii = ii - 1;
                    *px.add(ii as usize) = COMPLEX_ELT(y, iny as c_int);
                    iny += 1;
                    if iny >= ny {
                        iny = 0;
                    }
                }
            }

            1610 | 1613 | 1614 | 1615 | 1616 => {
                // character <- various
                let mut iny: R_xlen_t = 0;
                for idx in 0..n {
                    let ii = gi(indx, idx);
                    if ii == NA_INTEGER as R_xlen_t {
                        continue;
                    }
                    let ii = ii - 1;
                    SET_STRING_ELT(x, ii, STRING_ELT(y, iny));
                    iny += 1;
                    if iny >= ny {
                        iny = 0;
                    }
                }
            }

            1919 => {
                // vector <- vector
                let mut iny: R_xlen_t = 0;
                for idx in 0..n {
                    let ii = gi(indx, idx);
                    if ii == NA_INTEGER as R_xlen_t {
                        continue;
                    }
                    let ii = ii - 1;
                    if (idx as R_xlen_t) >= ny {
                        ENSURE_NAMEDMAX(VECTOR_ELT(y, iny as R_xlen_t));
                    }
                    SET_VECTOR_ELT(x, ii, VECTOR_ELT_FIX_NAMED(y, iny as R_xlen_t));
                    iny += 1;
                    if iny >= ny {
                        iny = 0;
                    }
                }
            }

            2019 | 2020 => {
                // expression <- vector/expression
                let mut iny: R_xlen_t = 0;
                for idx in 0..n {
                    let ii = gi(indx, idx);
                    if ii == NA_INTEGER as R_xlen_t {
                        continue;
                    }
                    let ii = ii - 1;
                    SET_VECTOR_ELT(x, ii, VECTOR_ELT(y, iny as R_xlen_t));
                    iny += 1;
                    if iny >= ny {
                        iny = 0;
                    }
                }
            }

            1900 | 2000 => {
                // vector/expression <- null
                x = DeleteListElements(x, indx);
                return x;
            }

            2424 => {
                // raw <- raw
                let px = RAW(x);
                let mut iny: R_xlen_t = 0;
                for idx in 0..n {
                    let ii = gi(indx, idx);
                    if ii == NA_INTEGER as R_xlen_t {
                        continue;
                    }
                    let ii = ii - 1;
                    *px.add(ii as usize) = RAW_ELT(y, iny as c_int);
                    iny += 1;
                    if iny >= ny {
                        iny = 0;
                    }
                }
            }

            _ => {
                // Warning case
            }
        }

        // Check for additional named elements.
        // Note makeSubscript passes the additional names back as the
        // use.names attribute (a vector list) of the generated subscript
        // vector (see trunk subassign.c VectorAssign tail).
        let newnames = getAttrib(indx, crate::eval::attrib_core::R_UseNamesSymbol());
        if !isNull(newnames) {
            let mut oldnames = getAttrib(x, crate::eval::attrib_core::R_NamesSymbol());
            if !isNull(oldnames) {
                for i in 0..n {
                    if !VECTOR_ELT(newnames, i).is_null() && VECTOR_ELT(newnames, i) != R_NilValue()
                    {
                        let mut ii = gi(indx, i);
                        if ii == NA_INTEGER as R_xlen_t {
                            continue;
                        }
                        ii -= 1;
                        SET_STRING_ELT(oldnames, ii, VECTOR_ELT(newnames, i));
                    }
                }
            } else {
                oldnames = Rf_allocVector3(SEXPTYPE::STRSXP, nx);
                let _oldnames_guard = protect(oldnames);
                for i in 0..nx {
                    SET_STRING_ELT(oldnames, i, R_BlankString());
                }
                for i in 0..n {
                    if !VECTOR_ELT(newnames, i).is_null() && VECTOR_ELT(newnames, i) != R_NilValue()
                    {
                        let mut ii = gi(indx, i);
                        if ii == NA_INTEGER as R_xlen_t {
                            continue;
                        }
                        ii -= 1;
                        SET_STRING_ELT(oldnames, ii, VECTOR_ELT(newnames, i));
                    }
                }
                setAttrib(x, crate::eval::attrib_core::R_NamesSymbol(), oldnames);
            }
        }

        x
    }
}
