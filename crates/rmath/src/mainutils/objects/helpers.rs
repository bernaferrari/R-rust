#![allow(
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_imports
)]

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
pub(crate) unsafe fn isString(x: SEXP) -> c_int {
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
pub(crate) unsafe fn isLogical(x: SEXP) -> c_int {
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
pub(crate) unsafe fn isFunction(x: SEXP) -> c_int {
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
pub(crate) unsafe fn isPrimitive(x: SEXP) -> c_int {
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
pub(crate) unsafe fn isClosure(x: SEXP) -> c_int {
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
pub(crate) unsafe fn isValidString(x: SEXP) -> c_int {
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

pub(crate) unsafe fn asRbool(x: SEXP, call: SEXP) -> c_int {
    unsafe { crate::mainutils::coerce::asRbool(x, call) }
}

pub(crate) unsafe fn asLogical(x: SEXP) -> c_int {
    unsafe { crate::mainutils::coerce::asLogical(x) }
}

pub(crate) unsafe fn asInteger(x: SEXP) -> c_int {
    unsafe { crate::mainutils::coerce::asInteger(x) }
}

/// isNull check.
pub(crate) unsafe fn isNull(x: SEXP) -> c_int {
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
pub(crate) unsafe fn length(x: SEXP) -> c_int {
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
pub(crate) unsafe fn Seql(a: SEXP, b: SEXP) -> c_int {
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
        if std::ffi::CStr::from_ptr(ca).to_bytes() == std::ffi::CStr::from_ptr(cb).to_bytes() {
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
pub(crate) unsafe fn stringPositionTr(klass: SEXP, what: *const c_char) -> c_int {
    unsafe {
        if klass.is_null() || what.is_null() {
            return -1;
        }
        let n = LENGTH(klass);
        for i in 0..n {
            let elt = STRING_ELT(klass, i as R_xlen_t);
            if !elt.is_null() {
                let cs = CHAR(elt);
                if !cs.is_null()
                    && std::ffi::CStr::from_ptr(cs).to_bytes()
                        == std::ffi::CStr::from_ptr(what).to_bytes()
                {
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
pub(crate) unsafe fn stringSuffix(klass: SEXP, pos: c_int) -> SEXP {
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
pub(crate) unsafe fn R_data_class2(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() || x == R_NilValue() {
            return Rf_mkString(c"NULL".as_ptr());
        }
        let class_val = getAttrib(x, R_ClassSymbol());
        if !class_val.is_null() && class_val != R_NilValue() && XLENGTH(class_val) > 0 {
            return class_val;
        }
        implicit_s3_class(x)
    }
}

/// GNU `Type2DefaultClass` implicit S3 classes for unclassed objects.
unsafe fn implicit_s3_class(x: SEXP) -> SEXP {
    unsafe {
        let dim = getAttrib(x, crate::eval::attrib_core::R_DimSymbol());
        let nd = if dim.is_null() || dim == R_NilValue() {
            0
        } else {
            XLENGTH(dim)
        };
        let t = TYPEOF(x);
        let (type_name, extra_numeric) = if t == SEXPTYPE::REALSXP {
            (c"double", true)
        } else if t == SEXPTYPE::INTSXP {
            (c"integer", true)
        } else if t == SEXPTYPE::LGLSXP {
            (c"logical", false)
        } else if t == SEXPTYPE::CPLXSXP {
            (c"complex", false)
        } else if t == SEXPTYPE::STRSXP {
            (c"character", false)
        } else if t == SEXPTYPE::RAWSXP {
            (c"raw", false)
        } else if t == SEXPTYPE::VECSXP {
            (c"list", false)
        } else if t == SEXPTYPE::CLOSXP || t == SEXPTYPE::SPECIALSXP || t == SEXPTYPE::BUILTINSXP {
            (c"function", false)
        } else if t == SEXPTYPE::SYMSXP {
            (c"name", false)
        } else if t == SEXPTYPE::NILSXP {
            (c"NULL", false)
        } else {
            return R_data_class(x);
        };
        let mut names: Vec<&std::ffi::CStr> = Vec::new();
        if nd == 2 {
            names.push(c"matrix");
            names.push(c"array");
        } else if nd > 0 {
            names.push(c"array");
        }
        names.push(type_name);
        if extra_numeric {
            names.push(c"numeric");
        }
        let result = Rf_allocVector3(SEXPTYPE::STRSXP, names.len() as i64);
        for (i, name) in names.iter().enumerate() {
            SET_STRING_ELT(result, i as i64, Rf_mkChar(name.as_ptr()));
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Helper: topenv -- find the top-level environment
// ---------------------------------------------------------------------------

/// Find the top-level environment by walking ENCLOS.
/// GNU: GlobalEnv, BaseEnv, BaseNamespace, package/namespace, or `.packageName`.
pub(crate) unsafe fn topenv(what: SEXP, env: SEXP) -> SEXP {
    unsafe {
        if env.is_null() {
            return R_NilValue();
        }
        let mut rho = env;
        loop {
            if rho == R_EmptyEnv() {
                return R_GlobalEnv();
            }
            if (!what.is_null() && what != R_NilValue() && rho == what)
                || rho == R_GlobalEnv()
                || rho == R_BaseEnv()
            {
                return rho;
            }
            let pkg = crate::sexp::envir::R_findVarInFrame(
                rho,
                crate::sexp::symbol::Rf_install(c".packageName".as_ptr()),
            );
            if pkg != R_UnboundValue() {
                return rho;
            }
            rho = ENCLOS(rho);
            if rho.is_null() {
                return R_GlobalEnv();
            }
        }
    }
}

/// R's `topenv(envir, matchThisEnv)` — `.Internal(topenv(...))` / primitive.
pub unsafe fn do_topenv(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let mut envir = if args.is_null() || args == R_NilValue() || CAR(args) == R_MissingArg() {
            rho
        } else {
            CAR(args)
        };
        if TYPEOF(envir) != SEXPTYPE::ENVSXP {
            envir = rho;
        }
        let mut target = if !args.is_null() && args != R_NilValue() {
            let rest = CDR(args);
            if rest.is_null() || rest == R_NilValue() || CAR(rest) == R_MissingArg() {
                R_NilValue()
            } else {
                CAR(rest)
            }
        } else {
            R_NilValue()
        };
        if target != R_NilValue() && TYPEOF(target) != SEXPTYPE::ENVSXP {
            target = R_NilValue();
        }
        topenv(target, envir)
    }
}


// ---------------------------------------------------------------------------
// Helper: listAppend -- append two lists
// ---------------------------------------------------------------------------

/// Append list `s` to the end of list `t`. Returns t (modified in place).
pub(crate) unsafe fn listAppend(t: SEXP, s: SEXP) -> SEXP {
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
pub(crate) unsafe fn R_BlankScalarString_placeholder() -> SEXP {
    unsafe { Rf_mkString(b"\x00".as_ptr() as *const c_char) }
}

// ---------------------------------------------------------------------------
// INTEGER_ELT_mut helper
// ---------------------------------------------------------------------------

/// Mutable access to INTEGER_ELT. Used for setting values in integer vectors.
pub(crate) unsafe fn INTEGER_ELT_mut(x: SEXP, i: c_int) -> *mut c_int {
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
