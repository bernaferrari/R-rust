use super::*;

// ---------------------------------------------------------------------------
// R-level entry points (do_* functions)
// ---------------------------------------------------------------------------

/// R-level `as.character()` for factors (internal).
pub unsafe fn do_asCharacterFactor(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        asCharacterFactor(x)
    }
}

// ---------------------------------------------------------------------------
// Safe wrapper functions using Sexp<'a>
// ---------------------------------------------------------------------------

/// Safe version of `do_coerce` using `Sexp<'a>`.
///
/// Parses the mode string from the second argument and coerces the input
/// SEXP to the target type. Returns `Result<SEXP, String>` for error handling.
pub fn coerce_vector_safe<'a>(x: Sexp<'a>, mode_str: Sexp<'a>) -> Result<SEXP, String> {
    if mode_str.clone().typeof_() != SEXPTYPE::STRSXP || mode_str.clone().len() != 1 {
        return Err("invalid 'mode' argument".to_string());
    }
    let mode_chars = mode_str.string_elt(0).ok_or("invalid 'mode' argument")?;
    let s = unsafe {
        let ptr = CHAR(mode_chars.as_raw());
        if ptr.is_null() {
            return Err("invalid 'mode' argument".to_string());
        }
        CStr::from_ptr(ptr).to_str().unwrap_or("").to_string()
    };

    let type_: c_int = match s.as_str() {
        "logical" => SEXPTYPE::LGLSXP.into(),
        "integer" => SEXPTYPE::INTSXP.into(),
        "double" | "numeric" => SEXPTYPE::REALSXP.into(),
        "complex" => SEXPTYPE::CPLXSXP.into(),
        "character" => SEXPTYPE::STRSXP.into(),
        "raw" => SEXPTYPE::RAWSXP.into(),
        "list" => SEXPTYPE::VECSXP.into(),
        "expression" => SEXPTYPE::EXPRSXP.into(),
        "pairlist" => SEXPTYPE::LISTSXP.into(),
        "any" => return Ok(x.as_raw()),
        "symbol" | "name" => SEXPTYPE::SYMSXP.into(),
        _ => return Err("invalid 'mode' argument".to_string()),
    };

    let x_raw = x.as_raw();
    unsafe {
        if TYPEOF(x_raw) == type_ {
            match SEXPTYPE(type_) {
                SEXPTYPE::LGLSXP
                | SEXPTYPE::INTSXP
                | SEXPTYPE::REALSXP
                | SEXPTYPE::CPLXSXP
                | SEXPTYPE::STRSXP
                | SEXPTYPE::RAWSXP => {
                    let attr = ATTRIB(x_raw);
                    if isNull(attr) {
                        return Ok(x_raw);
                    }
                    let ans = Rf_allocVector3(type_, xlength(x_raw));
                    let _ans_guard = protect(ans);
                    let src = DATAPTR(x_raw);
                    let dst = DATAPTR(ans);
                    let elem_size = match SEXPTYPE(type_) {
                        SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP => std::mem::size_of::<c_int>(),
                        SEXPTYPE::REALSXP => std::mem::size_of::<c_double>(),
                        SEXPTYPE::CPLXSXP => std::mem::size_of::<Rcomplex>(),
                        SEXPTYPE::RAWSXP => std::mem::size_of::<Rbyte>(),
                        _ => std::mem::size_of::<SEXP>(),
                    };
                    if !src.is_null() && !dst.is_null() {
                        ptr::copy_nonoverlapping(
                            src as *const u8,
                            dst as *mut u8,
                            xlength(x_raw) as usize * elem_size,
                        );
                    }
                    return Ok(ans);
                }
                _ => return Ok(x_raw),
            }
        }

        let ans = ascommon(ptr::null_mut(), x_raw, type_);
        match SEXPTYPE(TYPEOF(ans)) {
            SEXPTYPE::LGLSXP
            | SEXPTYPE::INTSXP
            | SEXPTYPE::REALSXP
            | SEXPTYPE::CPLXSXP
            | SEXPTYPE::STRSXP
            | SEXPTYPE::RAWSXP => {
                CLEAR_ATTRIB(ans);
            }
            _ => {} // intentionally unhandled: SEXPTYPE does not require attribute clearing
        }
        Ok(ans)
    }
}

/// Safe version of `do_asatomic` using `Sexp<'a>`.
///
/// Strips attributes and returns a clean atomic vector of the target type.
/// The `op` value selects the target type (0=character, 1=integer, 2=double,
/// 3=complex, 4=logical, 5=raw).
pub fn as_atomic_safe(x: Sexp<'_>, op: i32) -> Result<SEXP, String> {
    let type_: c_int = match op {
        0 => SEXPTYPE::STRSXP.into(),
        1 => SEXPTYPE::INTSXP.into(),
        2 => SEXPTYPE::REALSXP.into(),
        3 => SEXPTYPE::CPLXSXP.into(),
        4 => SEXPTYPE::LGLSXP.into(),
        5 => SEXPTYPE::RAWSXP.into(),
        _ => SEXPTYPE::STRSXP.into(),
    };

    let x_raw = x.as_raw();
    unsafe {
        if TYPEOF(x_raw) == type_ {
            if isNull(ATTRIB(x_raw)) {
                return Ok(x_raw);
            }
            let ans = Rf_allocVector3(type_, xlength(x_raw));
            let _ans_guard = protect(ans);
            let src = DATAPTR(x_raw);
            let dst = DATAPTR(ans);
            let byte_len = xlength(x_raw) as usize
                * match SEXPTYPE(type_) {
                    SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP => std::mem::size_of::<c_int>(),
                    SEXPTYPE::REALSXP => std::mem::size_of::<c_double>(),
                    SEXPTYPE::CPLXSXP => std::mem::size_of::<Rcomplex>(),
                    SEXPTYPE::RAWSXP => std::mem::size_of::<Rbyte>(),
                    _ => std::mem::size_of::<SEXP>(),
                };
            if !src.is_null() && !dst.is_null() {
                ptr::copy_nonoverlapping(src as *const u8, dst as *mut u8, byte_len);
            }
            CLEAR_ATTRIB(ans);
            return Ok(ans);
        }

        let ans = coerceVector(x_raw, type_);
        CLEAR_ATTRIB(ans);
        Ok(ans)
    }
}

/// Safe version of `do_asvector` using `Sexp<'a>`.
///
/// Coerces to a vector of the specified mode, stripping attributes for
/// atomic types but preserving them for list/expression/pairlist types.
pub fn as_vector_safe<'a>(x: Sexp<'a>, mode_str: Sexp<'a>) -> Result<SEXP, String> {
    if mode_str.clone().typeof_() != SEXPTYPE::STRSXP || mode_str.clone().len() != 1 {
        return Err("invalid 'mode' argument".to_string());
    }
    let mode_chars = mode_str.string_elt(0).ok_or("invalid 'mode' argument")?;
    let s = unsafe {
        let ptr = CHAR(mode_chars.as_raw());
        if ptr.is_null() {
            return Err("invalid 'mode' argument".to_string());
        }
        CStr::from_ptr(ptr).to_str().unwrap_or("").to_string()
    };

    let type_: c_int = match s.as_str() {
        "logical" => SEXPTYPE::LGLSXP.into(),
        "integer" => SEXPTYPE::INTSXP.into(),
        "double" | "numeric" => SEXPTYPE::REALSXP.into(),
        "complex" => SEXPTYPE::CPLXSXP.into(),
        "character" => SEXPTYPE::STRSXP.into(),
        "raw" => SEXPTYPE::RAWSXP.into(),
        "list" => SEXPTYPE::VECSXP.into(),
        "expression" => SEXPTYPE::EXPRSXP.into(),
        "pairlist" => SEXPTYPE::LISTSXP.into(),
        "symbol" | "name" => SEXPTYPE::SYMSXP.into(),
        "function" => SEXPTYPE::CLOSXP.into(),
        "any" => return Ok(x.as_raw()),
        _ => return Err("invalid 'mode' argument".to_string()),
    };

    let x_raw = x.as_raw();
    unsafe {
        if TYPEOF(x_raw) == type_ {
            match SEXPTYPE(type_) {
                SEXPTYPE::LGLSXP
                | SEXPTYPE::INTSXP
                | SEXPTYPE::REALSXP
                | SEXPTYPE::CPLXSXP
                | SEXPTYPE::STRSXP
                | SEXPTYPE::RAWSXP => {
                    if isNull(ATTRIB(x_raw)) {
                        return Ok(x_raw);
                    }
                    let ans = Rf_allocVector3(type_, xlength(x_raw));
                    let _ans_guard = protect(ans);
                    let src = DATAPTR(x_raw);
                    let dst = DATAPTR(ans);
                    let elem_size = match SEXPTYPE(type_) {
                        SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP => std::mem::size_of::<c_int>(),
                        SEXPTYPE::REALSXP => std::mem::size_of::<c_double>(),
                        SEXPTYPE::CPLXSXP => std::mem::size_of::<Rcomplex>(),
                        SEXPTYPE::RAWSXP => std::mem::size_of::<Rbyte>(),
                        _ => std::mem::size_of::<SEXP>(),
                    };
                    if !src.is_null() && !dst.is_null() {
                        ptr::copy_nonoverlapping(
                            src as *const u8,
                            dst as *mut u8,
                            xlength(x_raw) as usize * elem_size,
                        );
                    }
                    CLEAR_ATTRIB(ans);
                    return Ok(ans);
                }
                _ => return Ok(x_raw),
            }
        }

        let ans = ascommon(ptr::null_mut(), x_raw, type_);
        match SEXPTYPE(TYPEOF(ans)) {
            SEXPTYPE::NILSXP
            | SEXPTYPE::LISTSXP
            | SEXPTYPE::LANGSXP
            | SEXPTYPE::VECSXP
            | SEXPTYPE::EXPRSXP => {}
            _ => {
                CLEAR_ATTRIB(ans);
            }
        }
        Ok(ans)
    }
}

/// Safe version of `do_is` using `Sexp<'a>`.
///
/// Returns `Result<c_int, String>` where the c_int is 0 or 1 (logical value).
/// The `op` value selects the predicate to test.
pub fn is_type_safe(x: Sexp<'_>, op: i32) -> Result<c_int, String> {
    let ans = match op {
        0 => is_null_safe(x),
        10 => (x.typeof_() == SEXPTYPE::LGLSXP) as c_int,
        13 => {
            let t = x.clone().typeof_();
            if t == SEXPTYPE::INTSXP {
                let x_raw = x.as_raw();
                unsafe {
                    let is_factor = crate::mainutils::objects::inherits2(
                        x_raw,
                        b"factor\0".as_ptr() as *const c_char,
                    ) != 0;
                    let is_ordered = crate::mainutils::objects::inherits2(
                        x_raw,
                        b"ordered\0".as_ptr() as *const c_char,
                    ) != 0;
                    if is_factor || is_ordered { 0 } else { 1 }
                }
            } else {
                0
            }
        }
        14 => (x.typeof_() == SEXPTYPE::REALSXP) as c_int,
        15 => (x.typeof_() == SEXPTYPE::CPLXSXP) as c_int,
        16 => (x.typeof_() == SEXPTYPE::STRSXP) as c_int,
        1 => {
            let x_raw = x.as_raw();
            unsafe {
                if IS_S4_OBJECT(x_raw) != 0 && TYPEOF(x_raw) == SEXPTYPE::OBJSXP {
                    let dot_x_data = crate::mainutils::subassign::R_getS4DataSlot(
                        x_raw,
                        SEXPTYPE::SYMSXP.into(),
                    );
                    (TYPEOF(dot_x_data) == SEXPTYPE::SYMSXP) as c_int
                } else {
                    (TYPEOF(x_raw) == SEXPTYPE::SYMSXP) as c_int
                }
            }
        }
        4 => {
            let x_raw = x.as_raw();
            unsafe {
                if IS_S4_OBJECT(x_raw) != 0 && TYPEOF(x_raw) == SEXPTYPE::OBJSXP {
                    let dot_x_data = crate::mainutils::subassign::R_getS4DataSlot(
                        x_raw,
                        SEXPTYPE::ENVSXP.into(),
                    );
                    (TYPEOF(dot_x_data) == SEXPTYPE::ENVSXP) as c_int
                } else {
                    (TYPEOF(x_raw) == SEXPTYPE::ENVSXP) as c_int
                }
            }
        }
        19 => {
            let t = x.typeof_();
            (t == SEXPTYPE::VECSXP || t == SEXPTYPE::LISTSXP) as c_int
        }
        2 => {
            let t = x.typeof_();
            (t == SEXPTYPE::LISTSXP || t == SEXPTYPE::NILSXP) as c_int
        }
        20 => (x.typeof_() == SEXPTYPE::EXPRSXP) as c_int,
        24 => (x.typeof_() == SEXPTYPE::RAWSXP) as c_int,
        6 => (x.typeof_() == SEXPTYPE::LANGSXP) as c_int,
        50 => unsafe { crate::sexp::accessors::OBJECT(x.as_raw()) },
        51 => unsafe { IS_S4_OBJECT(x.as_raw()) },
        100 => is_numeric_safe(x),
        101 => is_matrix_safe(x),
        102 => is_array_safe(x),
        200 => is_atomic_safe(x),
        201 => {
            let t = x.typeof_();
            matches!(
                t,
                SEXPTYPE::VECSXP
                    | SEXPTYPE::LISTSXP
                    | SEXPTYPE::CLOSXP
                    | SEXPTYPE::ENVSXP
                    | SEXPTYPE::PROMSXP
                    | SEXPTYPE::LANGSXP
                    | SEXPTYPE::SPECIALSXP
                    | SEXPTYPE::BUILTINSXP
                    | SEXPTYPE::EXPRSXP
            ) as c_int
        }
        300 => (x.typeof_() == SEXPTYPE::LANGSXP) as c_int,
        301 => {
            let t = x.typeof_();
            (t == SEXPTYPE::SYMSXP || t == SEXPTYPE::LANGSXP || t == SEXPTYPE::EXPRSXP) as c_int
        }
        302 => is_function_safe(x),
        _ => 0,
    };
    Ok(ans)
}

/// Safe version of `do_isvector` using `Sexp<'a>`.
///
/// Checks whether the SEXP is a vector of the specified mode, and whether
/// it has only a "names" attribute (no other attributes).
pub fn is_vector_type_safe<'a>(x: Sexp<'a>, mode_str: Sexp<'a>) -> Result<c_int, String> {
    if mode_str.clone().typeof_() != SEXPTYPE::STRSXP || mode_str.clone().len() != 1 {
        return Err("invalid 'mode' argument".to_string());
    }
    let mode_chars = mode_str.string_elt(0).ok_or("invalid 'mode' argument")?;
    let s = unsafe {
        let ptr = CHAR(mode_chars.as_raw());
        if ptr.is_null() {
            return Err("invalid 'mode' argument".to_string());
        }
        CStr::from_ptr(ptr).to_str().unwrap_or("").to_string()
    };

    let is_vec = if s == "any" {
        x.clone().is_vector()
    } else if s == "numeric" {
        is_numeric_safe(x.clone()) != 0 && is_logical_safe(x.clone()) == 0
    } else {
        let type_name = match x.clone().typeof_() {
            SEXPTYPE::LGLSXP => "logical",
            SEXPTYPE::INTSXP => "integer",
            SEXPTYPE::REALSXP => "double",
            SEXPTYPE::CPLXSXP => "complex",
            SEXPTYPE::STRSXP => "character",
            SEXPTYPE::RAWSXP => "raw",
            SEXPTYPE::VECSXP => "list",
            SEXPTYPE::EXPRSXP => "expression",
            SEXPTYPE::LISTSXP => "pairlist",
            _ => "",
        };
        s == type_name || (s == "name" && type_name == "symbol")
    };

    if !is_vec {
        return Ok(0);
    }

    // Check that only a "names" attribute is present
    let x_raw = x.as_raw();
    unsafe {
        let mut a = ATTRIB(x_raw);
        while !isNull(a) {
            if !isNull(TAG(a)) && TAG(a) != R_NamesSymbol() {
                return Ok(0);
            }
            a = CDR(a);
        }
    }
    Ok(1)
}

// ---------------------------------------------------------------------------
// Safe helper predicates
// ---------------------------------------------------------------------------

pub fn is_null_safe(x: Sexp) -> c_int {
    (x.is_nil()) as c_int
}

pub fn is_numeric_safe(x: Sexp) -> c_int {
    let t = x.clone().typeof_();
    if (t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP) && x.is_vector() {
        1
    } else {
        0
    }
}

pub fn is_logical_safe(x: Sexp) -> c_int {
    (x.clone().typeof_() == SEXPTYPE::LGLSXP && x.is_vector()) as c_int
}

pub fn is_function_safe(x: Sexp) -> c_int {
    unsafe { (Rf_isFunction(x.as_raw()) != 0) as c_int }
}

pub fn is_matrix_safe(x: Sexp) -> c_int {
    unsafe {
        let dim = getAttrib(x.as_raw(), R_DimSymbol());
        (!isNull(dim) && LENGTH(dim) == 2) as c_int
    }
}

pub fn is_array_safe(x: Sexp) -> c_int {
    unsafe { (!isNull(getAttrib(x.as_raw(), R_DimSymbol()))) as c_int }
}

pub fn is_atomic_safe(x: Sexp) -> c_int {
    let t = x.typeof_();
    (t == SEXPTYPE::CHARSXP
        || t == SEXPTYPE::LGLSXP
        || t == SEXPTYPE::INTSXP
        || t == SEXPTYPE::REALSXP
        || t == SEXPTYPE::CPLXSXP
        || t == SEXPTYPE::STRSXP
        || t == SEXPTYPE::RAWSXP) as c_int
}

// ---------------------------------------------------------------------------
// FFI entry points delegating to safe wrappers
// ---------------------------------------------------------------------------

/// R-level coercion entry point (`as.logical`, `as.integer`, etc.).
///
/// This is the `do_asatomic()` function from coerce.c, handling
/// `as.character`, `as.integer`, `as.double`, `as.complex`, `as.logical`, `as.raw`.
pub unsafe fn do_asatomic(call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let args_s =
            Sexp::try_from_raw(args).unwrap_or_else(|err| errorcall(call, &err.to_string()));
        let x = args_s
            .try_pairlist_arg(0)
            .unwrap_or_else(|err| errorcall(call, &err.to_string()));
        let op0 = PRIMVAL(op);
        as_atomic_safe(x, op0).unwrap_or_else(|message| errorcall(call, &message))
    }
}

/// R-level `as.vector()` entry point.
///
/// This is the `do_asvector()` function from coerce.c.
pub unsafe fn do_asvector(call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let args_s =
            Sexp::try_from_raw(args).unwrap_or_else(|err| errorcall(call, &err.to_string()));
        let x = args_s
            .clone()
            .try_pairlist_arg(0)
            .clone()
            .unwrap_or_else(|err| errorcall(call, &err.to_string()));
        let mode_str = match args_s.try_pairlist_arg(1) {
            Ok(s) => s,
            Err(_) => return x.as_raw(),
        };
        as_vector_safe(x, mode_str).unwrap_or_else(|message| errorcall(call, &message))
    }
}

/// R-level `typeof()` entry point.
///
/// This is the `do_typeof()` function from coerce.c.
/// Note: canonical version lives in inspect.rs; this is kept as
/// coerce_typeof for internal use.
pub unsafe fn coerce_typeof(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if TYPEOF(x) == SEXPTYPE::OBJSXP && IS_S4_OBJECT(x) == 0 {
            return Rf_mkString(c"object".as_ptr());
        }
        let type_name = match SEXPTYPE(TYPEOF(x)) {
            SEXPTYPE::NILSXP => "NULL",
            SEXPTYPE::SYMSXP => "symbol",
            SEXPTYPE::LISTSXP => "pairlist",
            SEXPTYPE::CLOSXP => "closure",
            SEXPTYPE::ENVSXP => "environment",
            SEXPTYPE::PROMSXP => "promise",
            SEXPTYPE::LANGSXP => "language",
            SEXPTYPE::SPECIALSXP => "special",
            SEXPTYPE::BUILTINSXP => "builtin",
            SEXPTYPE::CHARSXP => "character",
            SEXPTYPE::LGLSXP => "logical",
            SEXPTYPE::INTSXP => "integer",
            SEXPTYPE::REALSXP => "double",
            SEXPTYPE::CPLXSXP => "complex",
            SEXPTYPE::STRSXP => "character",
            SEXPTYPE::DOTSXP => "...",
            SEXPTYPE::ANYSXP => "any",
            SEXPTYPE::VECSXP => "list",
            SEXPTYPE::EXPRSXP => "expression",
            SEXPTYPE::RAWSXP => "raw",
            SEXPTYPE::OBJSXP => "object",
            _ => "unknown",
        };
        Rf_mkString(
            std::ffi::CString::new(type_name)
                .unwrap_or_default()
                .as_ptr(),
        )
    }
}

/// Check if a single element is NA — matches C's LIST_VEC_NA macro.
/// Returns 1 if the element is a length-1 vector containing NA, 0 otherwise.
pub unsafe fn elem_is_na(s: SEXP) -> c_int {
    unsafe {
        if !isVector(s) || xlength(s) != 1 {
            return 0;
        }
        match TYPEOF(s) {
            t if t == SEXPTYPE::LGLSXP => (LOGICAL_ELT(s, 0) == NA_LOGICAL) as c_int,
            t if t == SEXPTYPE::INTSXP => (INTEGER_ELT(s, 0) == NA_INTEGER) as c_int,
            t if t == SEXPTYPE::REALSXP => ISNAN(REAL_ELT(s, 0)) as c_int,
            t if t == SEXPTYPE::STRSXP => (STRING_ELT(s, 0) == R_NaString()) as c_int,
            t if t == SEXPTYPE::CPLXSXP => {
                let v = COMPLEX_ELT(s, 0);
                (ISNAN(v.r) || ISNAN(v.i)) as c_int
            }
            _ => 0,
        }
    }
}

/// R-level `is.*` predicate dispatcher.
///
/// This is the `do_is()` function from coerce.c, implementing is.null,
/// is.logical, is.integer, is.double, is.complex, is.character, etc.
pub unsafe fn do_is(_call: SEXP, op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let args_s = match Sexp::try_from_raw(args) {
            Ok(s) => s,
            Err(_) => return Rf_ScalarLogical(0),
        };
        let x = match args_s.try_pairlist_arg(0) {
            Ok(s) => s,
            Err(_) => return Rf_ScalarLogical(0),
        };
        let op0 = PRIMVAL(op);
        match is_type_safe(x, op0) {
            Ok(result) => Rf_ScalarLogical(result),
            Err(_) => Rf_ScalarLogical(0),
        }
    }))
    .unwrap_or_else(|_| unsafe { Rf_ScalarLogical(0) })
}

/// R-level `is.vector()` entry point.
///
/// This is the `do_isvector()` function from coerce.c.
pub unsafe fn do_isvector(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let args_s = match Sexp::try_from_raw(args) {
            Ok(s) => s,
            Err(_) => return Rf_ScalarLogical(0),
        };
        let x = match args_s.clone().try_pairlist_arg(0).clone() {
            Ok(s) => s,
            Err(_) => return Rf_ScalarLogical(0),
        };
        let mode_arg = match args_s.try_pairlist_arg(1) {
            Ok(s) => s,
            Err(_) => return Rf_ScalarLogical(0),
        };
        match is_vector_type_safe(x, mode_arg) {
            Ok(result) => Rf_ScalarLogical(result),
            Err(_) => Rf_ScalarLogical(0),
        }
    }))
    .unwrap_or_else(|_| unsafe { Rf_ScalarLogical(0) })
}

/// R-level `is.na()` entry point.
///
/// This is the `do_isna()` function from coerce.c.
pub unsafe fn do_isna(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = xlength(x);
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let _ans_guard = protect(ans);
        let pa = LOGICAL(ans);

        match TYPEOF(x) {
            t if t == SEXPTYPE::LGLSXP => {
                for i in 0..n {
                    *pa.add(i as usize) = (LOGICAL_ELT(x, i as c_int) == NA_LOGICAL) as c_int;
                }
            }
            t if t == SEXPTYPE::INTSXP => {
                for i in 0..n {
                    *pa.add(i as usize) = (INTEGER_ELT(x, i as c_int) == NA_INTEGER) as c_int;
                }
            }
            t if t == SEXPTYPE::REALSXP => {
                for i in 0..n {
                    *pa.add(i as usize) = ISNAN(REAL_ELT(x, i as c_int)) as c_int;
                }
            }
            t if t == SEXPTYPE::CPLXSXP => {
                for i in 0..n {
                    let v = COMPLEX_ELT(x, i as c_int);
                    *pa.add(i as usize) = (ISNAN(v.r) || ISNAN(v.i)) as c_int;
                }
            }
            t if t == SEXPTYPE::STRSXP => {
                for i in 0..n {
                    *pa.add(i as usize) = (STRING_ELT(x, i) == R_NaString()) as c_int;
                }
            }
            t if t == SEXPTYPE::RAWSXP => {
                for i in 0..n {
                    *pa.add(i as usize) = 0;
                }
            }
            t if t == SEXPTYPE::LISTSXP => {
                let mut elt = x;
                for i in 0..n {
                    let s = CAR(elt);
                    *pa.add(i as usize) = elem_is_na(s);
                    elt = CDR(elt);
                }
            }
            t if t == SEXPTYPE::VECSXP => {
                for i in 0..n {
                    let s = VECTOR_ELT(x, i);
                    *pa.add(i as usize) = elem_is_na(s);
                }
            }
            t if t == SEXPTYPE::NILSXP => {}
            _ => {
                for i in 0..n {
                    *pa.add(i as usize) = 0;
                }
            }
        }

        // Copy dim and names
        if isVector(x) {
            let dims = getAttrib(x, R_DimSymbol());
            if !isNull(dims) {
                setAttrib(ans, R_DimSymbol(), dims);
            }
            let names = if isArray(x) {
                getAttrib(x, R_DimNamesSymbol())
            } else {
                getAttrib(x, R_NamesSymbol())
            };
            if !isNull(names) {
                if isArray(x) {
                    setAttrib(ans, R_DimNamesSymbol(), names);
                } else {
                    setAttrib(ans, R_NamesSymbol(), names);
                }
            }
        }

        ans
    }
}

/// R-level `is.nan()` entry point.
///
/// This is the `do_isnan()` function from coerce.c.
pub unsafe fn do_isnan(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let n = xlength(x);
        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let _ans_guard = protect(ans);
        let pa = LOGICAL(ans);

        match TYPEOF(x) {
            t if t == SEXPTYPE::REALSXP => {
                for i in 0..n {
                    *pa.add(i as usize) = R_IsNaN(REAL_ELT(x, i as c_int)) as c_int;
                }
            }
            t if t == SEXPTYPE::CPLXSXP => {
                for i in 0..n {
                    let v = COMPLEX_ELT(x, i as c_int);
                    *pa.add(i as usize) = (R_IsNaN(v.r) || R_IsNaN(v.i)) as c_int;
                }
            }
            _ => {
                for i in 0..n {
                    *pa.add(i as usize) = 0;
                }
            }
        }

        if isVector(x) {
            let dims = getAttrib(x, R_DimSymbol());
            if !isNull(dims) {
                setAttrib(ans, R_DimSymbol(), dims);
            }
            let names = if isArray(x) {
                getAttrib(x, R_DimNamesSymbol())
            } else {
                getAttrib(x, R_NamesSymbol())
            };
            if !isNull(names) {
                if isArray(x) {
                    setAttrib(ans, R_DimNamesSymbol(), names);
                } else {
                    setAttrib(ans, R_NamesSymbol(), names);
                }
            }
        }

        ans
    }
}
