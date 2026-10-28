use super::*;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// R's NA_REAL sentinel (specific NaN bit pattern).
pub const NA_REAL: c_double = crate::sexp::ffi::NA_REAL;

/// R's specific NA value as f64.
#[inline]
pub fn R_NA_REAL() -> f64 {
    f64::from_bits(R_NA_BIT_PATTERN)
}

/// Check if a double is R's NA (not just any NaN).
#[inline]
pub fn R_IsNA(x: f64) -> bool {
    x.to_bits() == R_NA_BIT_PATTERN
}

/// Check if a double is any NaN (including R's NA).
#[inline]
pub fn ISNAN(x: f64) -> bool {
    x.is_nan()
}

/// Check if a double is finite (not NaN and not Inf/-Inf).
#[inline]
pub fn R_FINITE(x: f64) -> bool {
    x.is_finite()
}

/// Check if a double is NaN but NOT R's NA.
#[inline]
pub fn R_IsNaN(x: f64) -> bool {
    x.is_nan() && x.to_bits() != R_NA_BIT_PATTERN
}

// ---------------------------------------------------------------------------
// Coercion warning flags (must match R's C defines)
// ---------------------------------------------------------------------------

/// Warning: NAs introduced by coercion.
pub const WARN_NA: c_int = 1;

/// Warning: NAs introduced by coercion to integer range.
pub const WARN_INT_NA: c_int = 2;

/// Warning: imaginary parts discarded in coercion.
pub const WARN_IMAG: c_int = 4;

/// Warning: out-of-range values treated as 0 in coercion to raw.
pub const WARN_RAW: c_int = 8;

// ---------------------------------------------------------------------------
// Internal helper: NA_STRING sentinel
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Internal helpers: xlength, isBlankString wrappers
// ---------------------------------------------------------------------------

/// Get the extended length of an SEXP (handles NULL).
#[inline]
pub unsafe fn xlength(x: SEXP) -> R_xlen_t {
    unsafe { XLENGTH(x) }
}

/// Check if an SEXP is a vector atomic type.
#[inline]
pub fn isVectorAtomic(x: SEXP) -> bool {
    crate::sexp::object::raw_is_atomic_vector(x)
}

/// Check if an SEXP is a vector type (atomic or list).
#[inline]
pub fn isVector(x: SEXP) -> bool {
    crate::sexp::object::raw_is_vector(x)
}

/// Check if an SEXP is a vector list type.
#[inline]
pub unsafe fn isVectorList(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP
    }
}

/// Check if an SEXP is a function.
#[inline]
pub unsafe fn isFunction(x: SEXP) -> bool {
    unsafe { Rf_isFunction(x) != 0 }
}

/// Check if an SEXP is an environment.
#[inline]
pub unsafe fn isEnvironment(x: SEXP) -> bool {
    unsafe { Rf_isEnvironment(x) != 0 }
}

/// Check if an SEXP is a symbol.
#[inline]
pub unsafe fn isSymbol(x: SEXP) -> bool {
    unsafe { Rf_isSymbol(x) != 0 }
}

/// Check if an SEXP is a string vector.
#[inline]
pub unsafe fn isString(x: SEXP) -> bool {
    unsafe { Rf_isString(x) != 0 }
}

/// Check if an SEXP is NULL.
#[inline]
pub unsafe fn isNull(x: SEXP) -> bool {
    unsafe { Rf_isNull(x) != 0 }
}

/// Check if an SEXP is a list (pairlist).
#[inline]
pub unsafe fn isList(x: SEXP) -> bool {
    unsafe { Rf_isList(x) != 0 }
}

/// Check if an SEXP is a language object.
#[inline]
pub unsafe fn isLanguage(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::LANGSXP }
}

/// Check GNU list admission using original-owner leases and typed graph edges.
#[inline]
pub unsafe fn isVectorizable(x: SEXP) -> bool {
    if x.is_null()
        || crate::sexp::globals::immutable_singleton_lease(x)
            .is_some_and(|lease| lease.snapshot().sxpinfo.type_of() == SEXPTYPE::NILSXP)
    {
        return true;
    }
    let result = (|| {
        // SAFETY: this translated entry supplies the active original owner;
        // the closed execution scope retains it through provider callbacks.
        let owner = unsafe { crate::sexp::owner::OwnerToken::current() }?
            .weak_owner()
            .ok_or(crate::sexp::object::SexpError::RootUnavailable)?;
        crate::sexp::owner::with_runtime(&owner, |access| {
            let value = access.domain().wrap(x)?.into_owned()?;
            super::vectorizable::check(value, access)
        })?
    })();
    match result {
        Ok(value) => value,
        Err(error) => crate::sexp::context::r_error(error.to_string()),
    }
}

/// Check if a SEXP is numeric (integer or real, but not logical).
#[inline]
pub unsafe fn isNumeric(x: SEXP) -> bool {
    unsafe {
        let t = TYPEOF(x);
        (t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP) && isVector(x)
    }
}

/// Check if a SEXP is logical.
#[inline]
pub unsafe fn isLogical(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::LGLSXP && isVector(x) }
}

/// Check if an SEXP has the S4 object bit set.
#[inline]
pub unsafe fn IS_S4_OBJECT(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        // S4 is stored in bit 4 of gp (level field, upper bits)
        ((*x).sxpinfo.gp() >> 4) as c_int & 1
    }
}

/// Check if an SEXP has the OBJECT bit set.
#[inline]
pub unsafe fn isObject(x: SEXP) -> bool {
    unsafe { OBJECT(x) != 0 }
}

/// Check if an SEXP is a new list (generic vector).
#[inline]
pub unsafe fn isNewList(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::VECSXP }
}

/// Check if a SEXP is an expression.
#[inline]
pub unsafe fn isExpression(x: SEXP) -> bool {
    unsafe { TYPEOF(x) == SEXPTYPE::EXPRSXP }
}

/// Check if a SEXP is a matrix (has non-null dim attribute of length 2).
#[inline]
pub unsafe fn isMatrix(x: SEXP) -> bool {
    unsafe {
        let dim = getAttrib(x, R_DimSymbol());
        !isNull(dim) && LENGTH(dim) == 2
    }
}

/// Check if a SEXP is an array (has non-null dim attribute).
#[inline]
pub unsafe fn isArray(x: SEXP) -> bool {
    unsafe { !isNull(getAttrib(x, R_DimSymbol())) }
}

pub unsafe fn SET_TYPEOF(x: SEXP, v: c_int) {
    unsafe {
        if !x.is_null() {
            crate::sexp::accessors::SET_TYPEOF(x, SEXPTYPE(v).as_c_int());
        }
    }
}

/// Panic with an R error message.
pub unsafe fn error(msg: &str) -> ! {
    std::panic::panic_any(RError {
        message: msg.to_string(),
    });
}

/// Panic with an R error message (call-specific).
pub unsafe fn errorcall(_call: SEXP, msg: &str) -> ! {
    unsafe {
        error(msg);
    }
}

/// Get R_NaString -- returns the NA string CHARSXP.
pub fn R_NaString() -> SEXP {
    unsafe { R_GlobalNaString() }
}

/// Get R_BlankString -- returns the empty string CHARSXP.
pub fn R_BlankString() -> SEXP {
    unsafe { Rf_mkChar(c"".as_ptr()) }
}

// ---------------------------------------------------------------------------
// SHALLOW_DUPLICATE_ATTRIB and CLEAR_ATTRIB helpers
// ---------------------------------------------------------------------------

/// Copy attributes from `from` to `to` (shallow duplicate).
/// This matches R's SHALLOW_DUPLICATE_ATTRIB macro from coerce.c.
pub unsafe fn SHALLOW_DUPLICATE_ATTRIB(to: SEXP, from: SEXP) {
    unsafe {
        let attr_from = ATTRIB(from);
        if !isNull(attr_from) {
            let _attributes_guard = protect(attr_from);
            // Preserve the existing class/shape checks, then transfer the
            // remaining attributes as internal metadata, as GNU does.
            let class = getAttrib(from, R_ClassSymbol());
            if !isNull(class) && class_fits(to, class) {
                setAttrib(to, R_ClassSymbol(), class);
            }
            let dim = getAttrib(from, R_DimSymbol());
            if !isNull(dim) {
                setAttrib(to, R_DimSymbol(), dim);
            }
            let dimnames = getAttrib(from, R_DimNamesSymbol());
            if !isNull(dimnames) {
                setAttrib(to, R_DimNamesSymbol(), dimnames);
            }
            let names = getAttrib(from, R_NamesSymbol());
            if !isNull(names) {
                setAttrib(to, R_NamesSymbol(), names);
            }
            // Coercion before subassignment must retain raw `tsp` and custom
            // metadata too. Internal transfer must not revalidate attributes
            // against a temporary result (which may be enlarged afterward).
            let mut attr = attr_from;
            while !isNull(attr) {
                let tag = TAG(attr);
                if tag != R_ClassSymbol()
                    && tag != R_DimSymbol()
                    && tag != R_DimNamesSymbol()
                    && tag != R_NamesSymbol()
                {
                    crate::sexp::attrib_core::installAttrib(to, tag, CAR(attr));
                }
                attr = CDR(attr);
            }
        }
    }
}

/// A factor or ordered class is only legal on an integer vector.
unsafe fn class_fits(to: SEXP, class: SEXP) -> bool {
    unsafe {
        use crate::sexp::ffi::SEXPTYPE;
        if crate::sexp::accessors::TYPEOF(to) == SEXPTYPE::INTSXP.as_c_int() {
            return true;
        }
        if crate::sexp::accessors::TYPEOF(class) != SEXPTYPE::STRSXP.as_c_int() {
            return true;
        }
        for i in 0..crate::sexp::accessors::XLENGTH(class) {
            let s = std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(
                crate::sexp::accessors::STRING_ELT(class, i),
            ));
            if s.to_bytes() == b"factor" || s.to_bytes() == b"ordered" {
                return false;
            }
        }
        true
    }
}

/// Clear all attributes from an SEXP, and reset object/S4 bits.
/// This matches R's CLEAR_ATTRIB macro from coerce.c.
pub unsafe fn CLEAR_ATTRIB(x: SEXP) {
    unsafe {
        let attr = ATTRIB(x);
        if !isNull(attr) {
            SET_ATTRIB(x, R_NilValue());
            if OBJECT(x) != 0 {
                SET_OBJECT(x, 0);
            }
            if IS_S4_OBJECT(x) != 0 {
                // Clear the S4 bit (bit 4 of gp)
                (*x).sxpinfo.set_gp((*x).sxpinfo.gp() & !(1 << 4));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// CoercionWarning

/// Issue coercion warnings based on the warning flags.
///
/// This is the equivalent of R's `CoercionWarning()` from coerce.c.
pub unsafe fn CoercionWarning(warn: c_int) {
    if warn & (WARN_NA | WARN_INT_NA | WARN_IMAG | WARN_RAW) == 0 {
        return;
    }
    let result: crate::sexp::object::SexpResult<()> = (|| {
        let owner = unsafe { crate::sexp::owner::OwnerToken::current() }?
            .weak_owner()
            .ok_or(crate::sexp::object::SexpError::RootUnavailable)?;
        crate::sexp::owner::with_runtime(&owner, |access| {
            // The existing session-local warning scope owns the override.
            // Also retain this actual call across every handler: a nested
            // scope may replace the slot or revoke the original runtime.
            let attributed = access.with_native(|token| {
                let call = crate::mainutils::errors::warning_call_override();
                if call.is_null() {
                    Ok(None)
                } else {
                    unsafe { token.sexp(call)?.into_owned().map(Some) }
                }
            })?;
            for (flag, message) in [
                (WARN_NA, c"NAs introduced by coercion"),
                (WARN_INT_NA, c"NAs introduced by coercion to integer range"),
                (WARN_IMAG, c"imaginary parts discarded in coercion"),
                (
                    WARN_RAW,
                    c"out-of-range values treated as 0 in coercion to raw",
                ),
            ] {
                if warn & flag != 0 {
                    access.with_native(|_| {
                        unsafe {
                            if let Some(call) = &attributed {
                                crate::mainutils::errors::warningcall(
                                    call.as_raw(),
                                    message.as_ptr(),
                                );
                            } else {
                                crate::mainutils::errors::Rf_warning(message.as_ptr());
                            }
                        }
                        Ok(())
                    })?;
                }
            }
            Ok(())
        })?
    })();
    result.unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
}
