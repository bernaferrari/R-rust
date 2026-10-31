use super::*;

/// Admit original checked generations before reading any input header/payload.
struct CoercionEntry<'s> {
    source: Sexp<'s>,
    factory: crate::sexp::object::SessionNodeFactory<'s>,
    pin: Option<crate::sexp::owner::OwnerPin>,
}
impl<'s> CoercionEntry<'s> {
    fn new(source: &Sexp<'s>) -> Result<Self, String> {
        let owner = unsafe { crate::sexp::owner::OwnerToken::current() }
            .map_err(|error| error.to_string())?;
        let pin = owner.pin().map_err(|error| error.to_string())?;
        let factory = owner.node_factory();
        factory
            .require_active()
            .map_err(|error| error.to_string())?;
        // Validate the supplied allocation generation before projecting its
        // address; a reused header must not acquire the old handle's authority.
        factory.link(source).map_err(|error| error.to_string())?;
        let source = factory
            .wrap(source.as_raw())
            .map_err(|error| error.to_string())?;
        Ok(Self {
            source,
            factory,
            pin,
        })
    }
    fn admit(&self, value: &Sexp<'_>) -> Result<(), String> {
        self.factory
            .require_active()
            .map_err(|error| error.to_string())?;
        self.factory
            .link(value)
            .map_err(|error| error.to_string())?;
        Ok(())
    }
    fn check(&self) {
        if let Some(pin) = &self.pin {
            pin.require_live()
                .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
        }
        self.factory
            .require_active()
            .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    }
}

/// Keep same-type atomic results in the source's checked allocation domain.
/// Attributed inputs get a fresh, attribute-free vector; plain inputs retain
/// the existing fast return. No borrowed payload spans a provider callback.
fn same_type_atomic(kind: SEXPTYPE, entry: &CoercionEntry<'_>) -> Result<SEXP, String> {
    let source = &entry.source;
    let factory = &entry.factory;
    let check = || entry.check();
    if source
        .try_attrib()
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()))
        .is_nil()
    {
        return Ok(source.as_raw());
    }
    let length = source.len();
    check();
    // Preserve the runtime's typed resource denial and allocation GC retry.
    let raw = unsafe { Rf_allocVector3(kind, length) };
    let output = factory
        .wrap(raw)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    check();
    let mut output = crate::sexp::object::SexpMut::try_from_checked(output)
        .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
    for index in 0..length {
        macro_rules! copy_element {
            ($read:ident, $write:ident) => {{
                let value = source
                    .$read(index)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
                check();
                output
                    .$write(index, value)
                    .unwrap_or_else(|error| crate::sexp::context::r_error(error.to_string()));
            }};
        }
        match kind {
            SEXPTYPE::LGLSXP => copy_element!(try_logical_elt, try_set_logical_elt),
            SEXPTYPE::INTSXP => copy_element!(try_integer_elt, try_set_integer_elt),
            SEXPTYPE::REALSXP => copy_element!(try_real_elt, try_set_real_elt),
            SEXPTYPE::CPLXSXP => copy_element!(try_complex_elt, try_set_complex_elt),
            SEXPTYPE::RAWSXP => copy_element!(try_raw_elt, try_set_raw_elt),
            SEXPTYPE::STRSXP => copy_element!(try_string_elt, try_set_string_elt),
            _ => unreachable!("same-type atomic coercion requires an atomic vector"),
        }
    }
    check();
    Ok(output.freeze().as_raw())
}

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

/// Rust compatibility entry for `as.vector` coercion.
///
/// GNU R has no separate `do_coerce` mode-string primitive. Keep this
/// entry point as an alias of the same checked semantics as `do_asvector`.
pub fn coerce_vector_safe<'a>(x: Sexp<'a>, mode_str: Sexp<'a>) -> Result<SEXP, String> {
    as_vector_safe(x, mode_str)
}

/// Safe version of `do_asatomic` using `Sexp<'a>`.
///
/// Strips attributes and returns a clean atomic vector of the target type.
/// The `op` value selects the target type (0=character, 1=integer, 2=double,
/// 3=complex, 4=logical, 5=raw).
pub fn as_atomic_safe(x: Sexp<'_>, op: i32) -> Result<SEXP, String> {
    let entry = CoercionEntry::new(&x)?;
    let type_: c_int = match op {
        0 => SEXPTYPE::STRSXP.into(),
        1 => SEXPTYPE::INTSXP.into(),
        2 => SEXPTYPE::REALSXP.into(),
        3 => SEXPTYPE::CPLXSXP.into(),
        4 => SEXPTYPE::LGLSXP.into(),
        5 => SEXPTYPE::RAWSXP.into(),
        _ => SEXPTYPE::STRSXP.into(),
    };

    let x_raw = entry.source.as_raw();
    unsafe {
        if TYPEOF(x_raw) == type_ {
            return same_type_atomic(SEXPTYPE(type_), &entry);
        }

        let ans = coerceVector(x_raw, type_);
        entry.check();
        CLEAR_ATTRIB(ans);
        Ok(ans)
    }
}

pub(crate) fn parse_as_vector_mode(
    mode: &crate::sexp::object::Sexp<'_>,
    check: impl Fn(&Sexp<'_>),
) -> Result<&'static str, String> {
    if mode.typeof_() != SEXPTYPE::STRSXP || mode.len() != 1 {
        return Err("invalid 'mode' argument".into());
    }
    let first = mode
        .try_string_elt(0)
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
    check(&first);
    if first
        .try_char_cstr_eq(b"function")
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
    {
        return Ok("function");
    }
    let second = mode
        .try_string_elt(0)
        .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()));
    check(&second);
    for name in [
        "any",
        "logical",
        "integer",
        "numeric",
        "double",
        "complex",
        "character",
        "raw",
        "list",
        "expression",
        "pairlist",
        "symbol",
        "name",
    ] {
        if second
            .try_char_cstr_eq(name.as_bytes())
            .unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
        {
            return Ok(name);
        }
    }
    Err("invalid 'mode' argument".into())
}

/// Safe version of `do_asvector` using `Sexp<'a>`.
///
/// Coerces to a vector of the specified mode, stripping attributes for
/// atomic types but preserving them for list/expression/pairlist types.
pub fn as_vector_safe<'a>(x: Sexp<'a>, mode_str: Sexp<'a>) -> Result<SEXP, String> {
    let entry = CoercionEntry::new(&x)?;
    entry.admit(&mode_str)?;
    let mode_str = entry
        .factory
        .wrap(mode_str.as_raw())
        .map_err(|error| error.to_string())?;
    let mode = parse_as_vector_mode(&mode_str, |chars| {
        entry.check();
        entry
            .admit(chars)
            .unwrap_or_else(|message| crate::sexp::context::r_error(message));
    })?;
    let parsed: Option<c_int> = match mode {
        "logical" => Some(SEXPTYPE::LGLSXP.into()),
        "integer" => Some(SEXPTYPE::INTSXP.into()),
        "double" | "numeric" => Some(SEXPTYPE::REALSXP.into()),
        "complex" => Some(SEXPTYPE::CPLXSXP.into()),
        "character" => Some(SEXPTYPE::STRSXP.into()),
        "raw" => Some(SEXPTYPE::RAWSXP.into()),
        "list" => Some(SEXPTYPE::VECSXP.into()),
        "expression" => Some(SEXPTYPE::EXPRSXP.into()),
        "pairlist" => Some(SEXPTYPE::LISTSXP.into()),
        "symbol" | "name" => Some(SEXPTYPE::SYMSXP.into()),
        "function" => Some(SEXPTYPE::CLOSXP.into()),
        "any" => None,
        _ => unreachable!("validated vector mode"),
    };
    entry.check();
    let Some(type_) = parsed else {
        let kind = entry.source.typeof_();
        if kind.is_atomic_type() {
            return same_type_atomic(kind, &entry);
        }
        let result = unsafe {
            ascommon(
                ptr::null_mut(),
                entry.source.as_raw(),
                SEXPTYPE::ANYSXP.as_c_int(),
            )
        };
        entry.check();
        return Ok(result);
    };

    let x_raw = entry.source.as_raw();
    unsafe {
        if TYPEOF(x_raw) == type_ {
            match SEXPTYPE(type_) {
                SEXPTYPE::LGLSXP
                | SEXPTYPE::INTSXP
                | SEXPTYPE::REALSXP
                | SEXPTYPE::CPLXSXP
                | SEXPTYPE::STRSXP
                | SEXPTYPE::RAWSXP => {
                    return same_type_atomic(SEXPTYPE(type_), &entry);
                }
                _ => return Ok(x_raw),
            }
        }

        let ans = ascommon(ptr::null_mut(), x_raw, type_);
        entry.check();
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
    if (t == SEXPTYPE::INTSXP || t == SEXPTYPE::REALSXP)
        && x.is_vector()
        && unsafe { crate::mainutils::objects::inherits2(x.as_raw(), c"factor".as_ptr()) == 0 }
    {
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
