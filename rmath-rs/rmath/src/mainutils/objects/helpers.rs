#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables, unused_imports)]

use super::*;

// ---------------------------------------------------------------------------
// Helper: CHAR wrapper that returns a *const c_char from a CHARSXP
// ---------------------------------------------------------------------------

// /// Get the C string from a CHARSXP (CHAR macro equivalent).
// /// Note: The main CHAR() is in accessors.rs; we use it directly from there.
// ---------------------------------------------------------------------------
// Helper: isString check
// ---------------------------------------------------------------------------

/// Check if x is a character vector (STRSXP).
unsafe fn isString(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return FALSE;
        }
        if TYPEOF(x) == SEXPTYPE::STRSXP {
            TRUE
        } else {
            FALSE
        }
    }
}

/// Check if x is an environment.
unsafe fn isEnvironment(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return FALSE;
        }
        if TYPEOF(x) == SEXPTYPE::ENVSXP {
            TRUE
        } else {
            FALSE
        }
    }
}

/// Check if x is a logical vector.
unsafe fn isLogical(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return FALSE;
        }
        if TYPEOF(x) == SEXPTYPE::LGLSXP {
            TRUE
        } else {
            FALSE
        }
    }
}

/// Check if x is a function (closure, builtin, or special).
unsafe fn isFunction(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return FALSE;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::CLOSXP || t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
            TRUE
        } else {
            FALSE
        }
    }
}

/// Check if x is a primitive (builtin or special).
unsafe fn isPrimitive(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return FALSE;
        }
        let t = TYPEOF(x);
        if t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
            TRUE
        } else {
            FALSE
        }
    }
}

/// Check if x is a closure.
unsafe fn isClosure(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return FALSE;
        }
        if TYPEOF(x) == SEXPTYPE::CLOSXP {
            TRUE
        } else {
            FALSE
        }
    }
}

/// Check if a string is valid and non-empty.
unsafe fn isValidString(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() || TYPEOF(x) != SEXPTYPE::STRSXP || LENGTH(x) != 1 {
            return FALSE;
        }
        let s = STRING_ELT(x, 0);
        if s.is_null() {
            return FALSE;
        }
        let cs = CHAR(s);
        if cs.is_null() {
            return FALSE;
        }
        if *cs == 0 {
            return FALSE;
        }
        TRUE
    }
}

unsafe fn asRbool(x: SEXP, call: SEXP) -> c_int {
    unsafe { crate::mainutils::coerce::asRbool(x, call) }
}

unsafe fn asLogical(x: SEXP) -> c_int {
    unsafe { crate::mainutils::coerce::asLogical(x) }
}

unsafe fn asInteger(x: SEXP) -> c_int {
    unsafe { crate::mainutils::coerce::asInteger(x) }
}

/// isNull check.
unsafe fn isNull(x: SEXP) -> c_int {
    unsafe { Rf_isNull(x) }
}

/// asChar: coerce to a single character string.
pub(crate) unsafe fn asChar(x: SEXP) -> SEXP {
    unsafe {
        if isString(x) != FALSE {
            return STRING_ELT(x, 0);
        }
        if TYPEOF(x) == SEXPTYPE::SYMSXP {
            return PRINTNAME(x);
        }
        R_NilValue()
    }
}

/// Get the length of an object.
unsafe fn length(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        match TYPEOF(x) {
            t if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP || t == SEXPTYPE::DOTSXP => {
                let mut n = 0;
                let mut current = x;
                while !current.is_null() && current != R_NilValue() {
                    n += 1;
                    current = CDR(current);
                }
                n
            }
            _ => LENGTH(x),
        }
    }
}

/// Check whether x is a promise that has been evaluated.
unsafe fn PROMISE_IS_EVALUATED(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() || TYPEOF(x) != SEXPTYPE::PROMSXP {
            return FALSE;
        }
        let val = (*x).data.promsxp.value;
        if val.is_null() || val == R_NilValue() {
            FALSE
        } else {
            TRUE
        }
    }
}

/// Get the promise value (PRVALUE).
unsafe fn PRVALUE(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() || TYPEOF(x) != SEXPTYPE::PROMSXP {
            return R_NilValue();
        }
        (*x).data.promsxp.value
    }
}

/// Check if two CHARSXP values are equal (Seql).
unsafe fn Seql(a: SEXP, b: SEXP) -> c_int {
    unsafe {
        if a == b {
            return TRUE;
        }
        if a.is_null() || b.is_null() {
            return FALSE;
        }
        let ca = CHAR(a);
        let cb = CHAR(b);
        if ca.is_null() || cb.is_null() {
            return FALSE;
        }
        if libc::strcmp(ca, cb) == 0 {
            TRUE
        } else {
            FALSE
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: stringPositionTr -- find a string in a character vector
// ---------------------------------------------------------------------------

/// Find the position of string `what` in character vector `klass`.
/// Returns the 0-based index, or -1 if not found.
/// This is the Rust equivalent of R's `stringPositionTr()`.
unsafe fn stringPositionTr(klass: SEXP, what: *const c_char) -> c_int {
    unsafe {
        if klass.is_null() || what.is_null() {
            return -1;
        }
        let n = LENGTH(klass);
        for i in 0..n {
            let elt = STRING_ELT(klass, i as R_xlen_t);
            if !elt.is_null() {
                let cs = CHAR(elt);
                if !cs.is_null() && libc::strcmp(cs, what) == 0 {
                    return i;
                }
            }
        }
        -1
    }
}

// ---------------------------------------------------------------------------
// Helper: stringSuffix -- get a suffix of a character vector starting at pos
// ---------------------------------------------------------------------------

/// Return a new character vector consisting of elements klass[pos..].
unsafe fn stringSuffix(klass: SEXP, pos: c_int) -> SEXP {
    unsafe {
        if klass.is_null() || pos < 0 {
            return R_NilValue();
        }
        let n = LENGTH(klass);
        if pos >= n {
            return R_NilValue();
        }
        let len = n - pos;
        let ans = Rf_allocVector(SEXPTYPE::STRSXP, len);
        let _ans_guard = protect(ans);
        for i in 0..len {
            let src = STRING_ELT(klass, (pos + i) as R_xlen_t);
            SET_STRING_ELT(ans, i as R_xlen_t, src);
        }
        ans
    }
}

// ---------------------------------------------------------------------------
// Helper: translateChar -- get the translated character string from a CHARSXP
// ---------------------------------------------------------------------------

unsafe fn translateChar(x: SEXP) -> *const c_char {
    unsafe { crate::sexp::accessors::translateChar(x) }
}

// ---------------------------------------------------------------------------
// Helper: R_data_class2 -- S4-aware class lookup
// ---------------------------------------------------------------------------

/// Get the class of an object, with S4 awareness.
/// For S4 objects, uses extends() to compute the full class vector.
/// For S3 objects, falls back to R_data_class.
unsafe fn R_data_class2(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            return R_NilValue();
        }
        if IS_S4_OBJECT(x) != FALSE {
            // S4 objects: for now, use the class attribute directly.
            // A full implementation would call extends() via the methods package.
            let class_val = getAttrib(x, R_ClassSymbol());
            if class_val.is_null() || class_val == R_NilValue() {
                // Try implicit class
                return R_data_class(x);
            }
            return class_val;
        }
        R_data_class(x)
    }
}

// ---------------------------------------------------------------------------
// Helper: topenv -- find the top-level environment
// ---------------------------------------------------------------------------

/// Find the top-level environment by walking ENCLOS.
/// If `what` is not R_NilValue, search for it starting from `env`.
unsafe fn topenv(_what: SEXP, env: SEXP) -> SEXP {
    unsafe {
        if env.is_null() {
            return R_NilValue();
        }
        let mut rho = env;
        loop {
            if rho == R_EmptyEnv() {
                return rho;
            }
            if rho == R_GlobalEnv() || rho == R_BaseEnv() {
                return rho;
            }
            rho = ENCLOS(rho);
            if rho.is_null() {
                return R_NilValue();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: listAppend -- append two lists
// ---------------------------------------------------------------------------

/// Append list `s` to the end of list `t`. Returns t (modified in place).
unsafe fn listAppend(t: SEXP, s: SEXP) -> SEXP {
    unsafe {
        if t.is_null() || t == R_NilValue() {
            return s;
        }
        if s.is_null() || s == R_NilValue() {
            return t;
        }
        let mut current = t;
        loop {
            let cdr = CDR(current);
            if cdr.is_null() || cdr == R_NilValue() {
                SETCDR(current, s);
                return t;
            }
            current = cdr;
        }
    }
}

// ---------------------------------------------------------------------------
// R_BlankScalarString placeholder
// ---------------------------------------------------------------------------

/// Get R_BlankScalarString (a blank character scalar).
unsafe fn R_BlankScalarString_placeholder() -> SEXP {
    unsafe { Rf_mkString(b"\x00".as_ptr() as *const c_char) }
}

// ---------------------------------------------------------------------------
// INTEGER_ELT_mut helper
// ---------------------------------------------------------------------------

/// Mutable access to INTEGER_ELT. Used for setting values in integer vectors.
unsafe fn INTEGER_ELT_mut(x: SEXP, i: c_int) -> *mut c_int {
    unsafe {
        if x.is_null() {
            return ptr::null_mut();
        }
        let base = INTEGER(x);
        if base.is_null() {
            return ptr::null_mut();
        }
        base.offset(i as isize)
    }
}

