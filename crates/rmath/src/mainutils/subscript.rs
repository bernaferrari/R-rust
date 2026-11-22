#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's src/main/subscript.c — subscript indexing utilities.
//!
//! Provides functions for computing subscripts used by R's `[`, `[[`, `[<-`,
//! and `[[<-` operators. These handle integer, real, logical, string, and
//! matrix-based indexing with proper NA handling, bounds checking, and
//! negative/positive subscript rules.
//!
//! Ported public functions:
//!   OneIndex()        — single index for [[<- (subassign.c)
//!   get1index()       — single index for [[ (subset.c, subassign.c)
//!   vectorIndex()     — recursive indexing for [[ and [[<- with vector args
//!   mat2indsub()      — matrix subscript to linear index conversion
//!   strmat2intmat()   — character matrix subscript to integer matrix
//!   makeSubscript()   — subscript creation for [ and [<-
//!   int_arraySubscript() — array subscript by dimension
//!   arraySubscript()  — public API wrapping int_arraySubscript
//!
//! Ported static helper functions (module-private):
//!   integerOneIndex(), nullSubscript(), logicalSubscript(),
//!   negativeSubscript(), positiveSubscript(), integerSubscript(),
//!   realSubscript(), stringSubscript()

use std::os::raw::{c_double, c_int};

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, R_xlen_t, Rboolean, SEXP, SEXPTYPE};
use crate::sexp::globals::{R_MissingArg, R_NilValue};

use crate::eval::attrib_core::{
    R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib,
};
use crate::sexp::protect::protect;
use crate::sexp::{Sexp, SexpMut, object::SexpResult, owner::OwnerToken};

fn error(msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    });
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// R's NA_REAL sentinel (specific NaN bit pattern).
const NA_REAL: c_double = crate::sexp::ffi::NA_REAL;

/// Interval at which to check interrupts (~subsecond on current hw).
const NINTERRUPT: R_xlen_t = 10_000_000;

// ---------------------------------------------------------------------------
// Error helpers
// ---------------------------------------------------------------------------

fn sexptype_name(x: SEXP) -> String {
    unsafe {
        if x.is_null() {
            "NULL".to_string()
        } else {
            std::ffi::CStr::from_ptr(crate::mainutils::util_main::type2char(TYPEOF(x)))
                .to_string_lossy()
                .into_owned()
        }
    }
}

fn subscript_index_name(sindex: SEXP) -> String {
    if sindex.is_null() || sindex == R_NilValue() {
        return "<missing>".to_string();
    }
    let kind = SEXPTYPE(TYPEOF(sindex));
    let value = match kind {
        SEXPTYPE::STRSXP => STRING_ELT(sindex, 0),
        SEXPTYPE::CHARSXP => sindex,
        SEXPTYPE::SYMSXP => PRINTNAME(sindex),
        _ => return format!("<{}>", sexptype_name(sindex)),
    };
    if value.is_null() || value == R_NilValue() {
        return if kind == SEXPTYPE::SYMSXP {
            "<invalid symbol>"
        } else {
            "<NA>"
        }
        .into();
    }
    char_cstr_bytes_opt(value)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_else(|| {
            if kind == SEXPTYPE::SYMSXP {
                "<invalid symbol>"
            } else {
                "<invalid string>"
            }
            .into()
        })
}

/// Report an out-of-bounds error.
pub fn ECALL_OutOfBounds(x: SEXP, subscript: c_int, index: R_xlen_t, _call: SEXP) {
    let msg = format!(
        "subscript out of bounds: type '{}' subscript {} index {}",
        sexptype_name(x),
        subscript,
        index
    );
    error(&msg);
}

/// Report a missing subscript error.
pub fn ECALL_MissingSubs(_call: SEXP) {
    error("subscript is missing");
}

/// Report an out-of-bounds error for character subscripts.
pub fn ECALL_OutOfBoundsCHAR(x: SEXP, subscript: c_int, sindex: SEXP, _call: SEXP) {
    let msg = format!(
        "subscript out of bounds: type '{}' subscript {} index {}",
        sexptype_name(x),
        subscript,
        subscript_index_name(sindex)
    );
    error(&msg);
}

// ---------------------------------------------------------------------------
// integerOneIndex — convert a single integer to a 0-based index
// ---------------------------------------------------------------------------

/// Convert a single integer index to a 0-based index.
///
/// This allows for the unusual case where `x` is of length 2, and
/// `x[[-m]]` selects one element for m = 1, 2. So `len` is only
/// used if it is 2 and `i` is negative.
///
/// Returns -1 on error (in full R this would be unreachable due to error calls).
#[inline]
fn integerOneIndex(i: c_int, len: R_xlen_t, _call: SEXP) -> R_xlen_t {
    {
        let mut indx: R_xlen_t = -1;
        if i > 0 {
            indx = (i - 1) as R_xlen_t;
        } else if i == 0 || len < 2 {
            error("attempt to select less than one element in integerOneIndex");
        } else if len == 2 && i > -3 {
            indx = (2 + i) as R_xlen_t;
        } else {
            error("attempt to select more than one element in integerOneIndex");
        }
        indx
    }
}

// ---------------------------------------------------------------------------
// OneIndex — used for [[<- in subassign.c
// ---------------------------------------------------------------------------

/// Compute a single index for `[[<-` assignment.
///
/// Returns the 0-based index, or `nx` if no match found (for string/symbol
/// subscripts, meaning "append at end"). The second tuple field is the new
/// name or R_NilValue. The caller must root it before an operation that can
/// allocate or invoke runtime callbacks.
pub(crate) fn one_index(
    x: SEXP,
    s: SEXP,
    nx: R_xlen_t,
    partial: c_int,
    pos: c_int,
    call: SEXP,
) -> (R_xlen_t, SEXP) {
    checked_index(OwnerToken::with_current(|owner| {
        let _x = (!x.is_null()).then(|| checked_index(owner.sexp(x)));
        let _call = (!call.is_null()).then(|| checked_index(owner.sexp(call)));
        let n = XLENGTH(s);
        if pos < 0 && n > 1 {
            error("attempt to select more than one element in OneIndex");
        }
        if pos < 0 && n < 1 {
            error("attempt to select less than one element in OneIndex");
        }
        let source = checked_index(owner.sexp(s));
        let pos = i64::from(pos.max(0));
        let nil = R_NilValue();
        match source.typeof_() {
            SEXPTYPE::LGLSXP => {
                let i = checked_index(source.try_logical_elt(pos));
                checked_index(owner.require_active());
                if i == NA_LOGICAL {
                    // Upstream (subassign.c) raises this while processing the
                    // subscript, before any typed assignment arm.
                    error("NAs are not allowed in subscripted assignments");
                }
                (integerOneIndex(i, nx, call), nil)
            }
            SEXPTYPE::INTSXP => {
                let i = checked_index(source.try_integer_elt(pos));
                checked_index(owner.require_active());
                (integerOneIndex(i, nx, call), nil)
            }
            SEXPTYPE::REALSXP => {
                let v = checked_index(source.try_real_elt(pos));
                checked_index(owner.require_active());
                let index = if v.is_nan() {
                    -1
                } else if v >= 1.0 {
                    (v - 1.0) as R_xlen_t
                } else if v > -1.0 || nx < 2 {
                    error("attempt to select less than one element in OneIndex")
                } else if nx == 2 && v > -3.0 {
                    (2.0 + v.trunc()) as R_xlen_t
                } else {
                    error("attempt to select more than one element in OneIndex")
                };
                (index, nil)
            }
            kind @ (SEXPTYPE::STRSXP | SEXPTYPE::SYMSXP) => {
                let names = getAttrib(x, R_NamesSymbol());
                let names = if names.is_null() { nil } else { names };
                let names = checked_index(owner.sexp(names));
                let is_symbol = kind == SEXPTYPE::SYMSXP;
                let name = if is_symbol {
                    checked_index(owner.sexp(PRINTNAME(s)))
                } else {
                    checked_index(source.try_string_elt(pos))
                };
                checked_index(owner.require_active());
                let bytes = char_cstr_bytes(name.as_raw());
                let index = find_single_name(
                    owner,
                    &names,
                    &bytes,
                    nx,
                    if is_symbol {
                        0
                    } else {
                        i32::from(partial != 0)
                    },
                    None,
                    false,
                    is_symbol,
                );
                (if index == -1 { nx } else { index }, name.as_raw())
            }
            _ => error("invalid subscript type 'unknown'"),
        }
    }))
}

/// Raw-pointer compatibility adapter for `[[<-` index selection.
///
/// Returns the 0-based index, or `nx` if no match found (for string/symbol
/// subscripts, meaning "append at end"). Writes the CHARSXP to install as a
/// name to `*newname` when a string subscript is provided.
///
/// # Safety
/// A non-null `newname` must be writable through the call.
pub unsafe fn OneIndex(
    x: SEXP,
    s: SEXP,
    nx: R_xlen_t,
    partial: c_int,
    newname: *mut SEXP,
    pos: c_int,
    call: SEXP,
) -> R_xlen_t {
    let (index, name) = one_index(x, s, nx, partial, pos, call);
    if !newname.is_null() {
        // SAFETY: the caller retains a writable output slot.
        unsafe { newname.write(name) };
    }
    index
}

/// Shared name matcher for `[[`/`[[<-` index selection: exact pass first,
/// then a partial pass only when `partial != 0`. Returns the match offset,
/// `-1` for no match, or `-2` for an ambiguous partial match.
/// `warning_call` carries the caller's call only when partial matches should
/// warn (GNU `pok == -1`).
fn find_single_name(
    owner: OwnerToken<'_>,
    names: &Sexp<'_>,
    target: &[u8],
    limit: R_xlen_t,
    partial: c_int,
    warning_call: Option<SEXP>,
    skip_na: bool,
    allow_empty_exact: bool,
) -> R_xlen_t {
    let read_name = |i| {
        let name = checked_index(names.try_string_elt(i));
        checked_index(owner.require_active());
        if skip_na && name.is_na_string() {
            None
        } else {
            Some(char_cstr_bytes(name.as_raw()))
        }
    };
    let end = names.len().min(limit);
    for i in 0..end {
        if let Some(name) = read_name(i)
            && (allow_empty_exact || !name.is_empty())
            && name == target
        {
            return i;
        }
    }
    if partial == 0 {
        return -1;
    }
    let mut found = -1;
    for i in 0..end {
        let Some(name) = read_name(i) else {
            continue;
        };
        if name.is_empty() || !name.starts_with(target) {
            continue;
        }
        let ambiguous = found != -1;
        if let Some(call) = warning_call {
            let lead = if ambiguous {
                "further partial match"
            } else {
                "partial match"
            };
            let message = std::ffi::CString::new(format!(
                "{lead} of '{}' to '{}'",
                String::from_utf8_lossy(target),
                String::from_utf8_lossy(&name)
            ))
            .expect("names were copied as C strings");
            if !ambiguous && (call.is_null() || call == R_NilValue()) {
                crate::mainutils::errors::Rf_warning_cstr(&message);
            } else {
                crate::mainutils::errors::warningcall_cstr(
                    if ambiguous { R_NilValue() } else { call },
                    &message,
                );
            }
            checked_index(owner.require_active());
        }
        if ambiguous {
            return -2;
        }
        found = i;
    }
    found
}

// ---------------------------------------------------------------------------
// get1index — used for [[ in subset.c and subassign.c
// ---------------------------------------------------------------------------

/// Get a single index for the `[[` and `[[<-` operators.
///
/// Checks that only one index is being selected.
/// Returns -1 for no match.
///
/// - `s` is the subscript
/// - `names` is the names of the object or dimension
/// - `len` is the length of the object or dimension
/// - `pos` is len-1 or -1 for `[[`, -1 for `[[<-`
/// - `pok` is "partial ok" (1 = allow, -1 = warn and allow, 0 = no)
pub fn get1index(
    s: SEXP,
    names: SEXP,
    len: R_xlen_t,
    pok: c_int,
    pos: c_int,
    call: SEXP,
) -> R_xlen_t {
    checked_index(OwnerToken::with_current(|owner| {
        let _call = (!call.is_null()).then(|| checked_index(owner.sexp(call)));
        let names = checked_index(owner.sexp(if names.is_null() {
            R_NilValue()
        } else {
            names
        }));
        let n = XLENGTH(s);
        if pos < 0 && n != 1 {
            if n > 1 {
                error("attempt to select more than one element in get1index");
            } else {
                error("attempt to select less than one element in get1index");
            }
        } else if i64::from(pos) >= n {
            error("internal error in use of recursive indexing");
        }
        let pos = i64::from(pos.max(0));
        let source = checked_index(owner.sexp(s));
        let mut indx: R_xlen_t = -1;
        match source.typeof_() {
            SEXPTYPE::LGLSXP => {
                let i = checked_index(source.try_logical_elt(pos));
                checked_index(owner.require_active());
                if i == NA_LOGICAL {
                    // Upstream (subassign.c) raises this while processing the
                    // subscript, before any typed assignment arm.
                    error("NAs are not allowed in subscripted assignments");
                }
                if i != NA_INTEGER {
                    indx = integerOneIndex(i, len, call);
                }
            }
            SEXPTYPE::INTSXP => {
                let i = checked_index(source.try_integer_elt(pos));
                checked_index(owner.require_active());
                if i != NA_INTEGER {
                    indx = integerOneIndex(i, len, call);
                }
            }
            SEXPTYPE::REALSXP => {
                let v = checked_index(source.try_real_elt(pos));
                checked_index(owner.require_active());
                if !v.is_nan() {
                    if v >= 1.0 {
                        if v.is_finite() {
                            indx = (v - 1.0) as R_xlen_t;
                        }
                    } else if v <= -1.0 {
                        if len == 2 && v > -3.0 {
                            indx = (2.0 + v.trunc()) as R_xlen_t;
                        } else {
                            crate::mainutils::errors::record_error_call(call, true);
                            error("invalid negative subscript in get1index <real>");
                        }
                    } else {
                        error("attempt to select less than one element in get1index");
                    }
                }
            }
            SEXPTYPE::STRSXP => {
                let target = checked_index(source.try_string_elt(pos));
                checked_index(owner.require_active());
                // NA and "" match nothing.
                if target.is_na_string() {
                    return -1;
                }
                let bytes = char_cstr_bytes(target.as_raw());
                if bytes.is_empty() {
                    return -1;
                }
                return find_single_name(
                    owner,
                    &names,
                    &bytes,
                    names.len(),
                    pok,
                    (pok == -1).then_some(call),
                    true,
                    false,
                );
            }
            SEXPTYPE::SYMSXP => {
                if s == R_MissingArg() {
                    crate::mainutils::errors::signal_missing_subscript(call, None);
                }
                let name = PRINTNAME(s);
                let bytes = char_cstr_bytes(name);
                return find_single_name(owner, &names, &bytes, names.len(), 0, None, true, true);
            }
            _ => error("invalid subscript type 'unknown'"),
        }
        indx
    }))
}

// ---------------------------------------------------------------------------
// vectorIndex — recursive indexing for [[ and [[<- with vector args
// ---------------------------------------------------------------------------

/// Perform recursive indexing for `[[` and `[[<-` with a vector of indices.
///
/// `x` is a list or pairlist, indexed recursively from level `start` to
/// `stop-1`. For `[[<-` it needs to duplicate if substructure might be shared.
pub fn vectorIndex(
    x: SEXP,
    thesub: SEXP,
    start: c_int,
    stop: c_int,
    pok: c_int,
    call: SEXP,
    dup: Rboolean,
) -> SEXP {
    if x.is_null() {
        return R_NilValue();
    }
    checked_index(OwnerToken::with_current(|owner| {
        let mut value = checked_index(owner.sexp(x));
        let _sub = (!thesub.is_null()).then(|| checked_index(owner.sexp(thesub)));
        let _call = (!call.is_null()).then(|| checked_index(owner.sexp(call)));

        // If dup requested, duplicate the outermost level
        if dup != 0 && start < stop {
            let newx = crate::mainutils::duplicate::duplicate(x);
            checked_index(owner.require_active());
            if !newx.is_null() {
                value = checked_index(owner.sexp(newx));
            }
        }

        for i in start..stop {
            // Get the index for this level
            let y = value.as_raw();
            let names = if Rf_isVector(y) != 0 {
                getAttrib(y, R_NamesSymbol())
            } else {
                R_NilValue()
            };
            let len = XLENGTH(y);
            let indx = get1index(thesub, names, len, pok, i, call);
            checked_index(owner.require_active());
            if indx < 0 || indx >= len {
                error("no such index at level 1");
            }
            if Rf_isVector(y) != 0 {
                if TYPEOF(y) == SEXPTYPE::VECSXP || TYPEOF(y) == SEXPTYPE::EXPRSXP {
                    value = checked_index(value.try_vector_elt(indx));
                } else {
                    error("recursive indexing failed at level 1");
                }
            } else if TYPEOF(y) == SEXPTYPE::LISTSXP || TYPEOF(y) == SEXPTYPE::LANGSXP {
                let mut p = y;
                for _ in 0..indx {
                    p = CDR(p);
                }
                value = checked_index(owner.sexp(CAR(p)));
            } else {
                error("attempt to select more than one element in vectorIndex");
            }
            checked_index(owner.require_active());
        }
        value.as_raw()
    }))
}

// ---------------------------------------------------------------------------
// mat2indsub — matrix subscript to linear index
// ---------------------------------------------------------------------------

/// Convert matrix subscripts to linear indices for array indexing.
///
/// Handles the case `x[i]` where `x` is an n-way array and `i` is a matrix
/// with n columns. Returns a vector of 1-based linear indices.
///
/// Negative indices are not allowed. Zero/NA propagates to the result.
pub fn mat2indsub(dims: SEXP, s: SEXP, call: SEXP, x: SEXP) -> SEXP {
    checked_index(OwnerToken::with_current(|owner| {
        // Root the operands before any read: attribute and element access
        // may run providers that collect or detach attributes.
        let dims = checked_index(owner.sexp(dims));
        let matrix = checked_index(owner.sexp(s));
        let _x = (!x.is_null()).then(|| checked_index(owner.sexp(x)));
        let _call = (!call.is_null()).then(|| checked_index(owner.sexp(call)));

        let (_shape, rows, cols) =
            matrix_subscript_shape(owner, &matrix, "subscript is not a matrix");
        if cols != dims.len() {
            error("incorrect number of columns in matrix subscript");
        }

        // Build (extent, stride) pairs once with overflow checking.
        let mut length: i64 = 1;
        let dimensions: Vec<(i64, i64)> = (0..cols)
            .map(|j| {
                let dim = i64::from(checked_index(dims.try_integer_elt(j)));
                if dim < 0 {
                    error("invalid array dimensions");
                }
                let stride = length;
                length = length
                    .checked_mul(dim)
                    .unwrap_or_else(|| error("array dimensions are too large"));
                (dim, stride)
            })
            .collect();
        checked_index(owner.require_active());

        let long = length > i64::from(c_int::MAX);
        // The checked result exists before any coercion, so a warning
        // handler cannot strand a raw destination pointer.
        let mut ans = new_index(
            owner,
            if long {
                SEXPTYPE::REALSXP
            } else {
                SEXPTYPE::INTSXP
            },
            rows,
        );

        let source = match matrix.typeof_() {
            SEXPTYPE::STRSXP => {
                let names = getAttrib(x, R_DimNamesSymbol());
                string_matrix_indices(owner, &matrix, names, call, x)
            }
            SEXPTYPE::INTSXP => matrix.clone(),
            SEXPTYPE::REALSXP if long => matrix.clone(),
            _ => {
                let coerced = crate::mainutils::coerce::coerceToInteger(matrix.as_raw());
                checked_index(owner.require_active());
                checked_index(owner.sexp(coerced))
            }
        };

        for i in 0..rows {
            let mut index = Some(1i64);
            for (j, &(dim, stride)) in dimensions.iter().enumerate() {
                let off = i + j as i64 * rows;
                let value = if source.typeof_() == SEXPTYPE::REALSXP {
                    let raw = checked_index(source.try_real_elt(off));
                    checked_index(owner.require_active());
                    if raw.is_nan() {
                        index = None;
                        break;
                    }
                    let k = raw.trunc();
                    if k >= 1.0 && k > dim as f64 {
                        ECALL_OutOfBounds(x, (j + 1) as c_int, k as i64, call);
                    }
                    k as i64
                } else {
                    let k = checked_index(source.try_integer_elt(off));
                    checked_index(owner.require_active());
                    if k == NA_INTEGER {
                        index = None;
                        break;
                    }
                    i64::from(k)
                };
                if value < 0 {
                    error("negative values are not allowed in a matrix subscript");
                }
                if value == 0 {
                    index = Some(0);
                    break;
                }
                if value > dim {
                    ECALL_OutOfBounds(x, (j + 1) as c_int, value, call);
                }
                index = Some(index.unwrap() + (value - 1) * stride);
            }
            if long {
                checked_index(ans.try_set_real_elt(i, index.map_or(NA_REAL, |v| v as f64)));
            } else {
                checked_index(ans.try_set_integer_elt(
                    i,
                    index.map_or(NA_INTEGER, |v| {
                        c_int::try_from(v)
                            .unwrap_or_else(|_| error("matrix subscript exceeds integer range"))
                    }),
                ));
            }
        }
        ans.freeze().as_raw()
    }))
}

// ---------------------------------------------------------------------------
// strmat2intmat — character matrix subscript to integer matrix
// ---------------------------------------------------------------------------

/// Convert a character matrix subscript to an integer matrix.
///
/// For the case `x[i]` where `x` is an n-way array and `i` is a character
/// matrix with n columns, this matches column values against dimnames of `x`.
/// Unmatched entries result in an out-of-bounds error.
pub fn strmat2intmat(s: SEXP, dnamelist: SEXP, call: SEXP, x: SEXP) -> SEXP {
    checked_index(OwnerToken::with_current(|owner| {
        let _x = (!x.is_null()).then(|| checked_index(owner.sexp(x)));
        let _call = (!call.is_null()).then(|| checked_index(owner.sexp(call)));
        let matrix = checked_index(owner.sexp(s));
        string_matrix_indices(owner, &matrix, dnamelist, call, x).as_raw()
    }))
}

/// Shared character-matrix matcher: resolves each entry of `matrix` against
/// the corresponding column of `dnamelist`, returning a rooted integer
/// matrix carrying the subscript's dim attribute.
fn string_matrix_indices<'s>(
    owner: OwnerToken<'s>,
    matrix: &Sexp<'_>,
    dnamelist: SEXP,
    call: SEXP,
    x: SEXP,
) -> Sexp<'s> {
    // Root the name list before the shape read: a dimension provider may
    // allocate.
    let names = if dnamelist.is_null() || dnamelist == R_NilValue() {
        None
    } else {
        Some(checked_index(owner.sexp(dnamelist)))
    };
    let (shape, rows, cols) =
        matrix_subscript_shape(owner, matrix, "no 'dimnames' attribute for array");
    let Some(names) = names else {
        if !call.is_null() {
            crate::mainutils::errors::record_error_call(call, true);
        }
        error("no 'dimnames' attribute for array");
    };

    let mut ans = new_index(owner, SEXPTYPE::INTSXP, rows * cols);
    setAttrib(ans.as_raw(), R_DimSymbol(), shape.as_raw());
    checked_index(owner.require_active());

    for j in 0..cols {
        let column = if j < names.len() {
            Some(checked_index(names.try_vector_elt(j)))
        } else {
            None
        };
        checked_index(owner.require_active());
        for i in 0..rows {
            let k = i + j * rows;
            let target = checked_index(matrix.try_string_elt(k));
            checked_index(owner.require_active());
            let value = if target.is_na_string() {
                NA_INTEGER
            } else {
                let mut found = 0;
                // A literal empty string never matches, even an empty name.
                if !checked_index(target.try_char_cstr_eq(b"")) {
                    if let Some(ref column) = column {
                        for n in 0..column.len() {
                            let entry = checked_index(column.try_string_elt(n));
                            checked_index(owner.require_active());
                            if !entry.is_na_string()
                                && crate::mainutils::relop::Seql(
                                    target.as_raw(),
                                    entry.as_raw(),
                                ) != 0
                            {
                                found = c_int::try_from(n + 1).unwrap_or_else(|_| {
                                    error("matrix subscript exceeds integer range")
                                });
                                break;
                            }
                        }
                    }
                }
                if found == 0 {
                    ECALL_OutOfBoundsCHAR(x, (j + 1) as c_int, target.as_raw(), call);
                }
                found
            };
            checked_index(ans.try_set_integer_elt(k, value));
        }
    }
    ans.freeze()
}

/// Read and root the dim attribute of a subscript matrix, returning the
/// rooted dim vector plus its (rows, cols) extents.
fn matrix_subscript_shape<'s>(
    owner: OwnerToken<'s>,
    matrix: &Sexp<'_>,
    missing: &str,
) -> (Sexp<'s>, R_xlen_t, R_xlen_t) {
    let dim = getAttrib(matrix.as_raw(), R_DimSymbol());
    if dim.is_null() || dim == R_NilValue() {
        error(missing);
    }
    let dim = checked_index(owner.sexp(dim));
    if dim.len() < 2 {
        error(missing);
    }
    let rows = i64::from(checked_index(dim.try_integer_elt(0)));
    let cols = i64::from(checked_index(dim.try_integer_elt(1)));
    checked_index(owner.require_active());
    if rows < 0 || cols < 0 || rows * cols != matrix.len() {
        error("invalid matrix subscript dimensions");
    }
    (dim, rows, cols)
}

fn checked_index<T>(result: SexpResult<T>) -> T {
    result.unwrap_or_else(|e| crate::sexp::context::r_error(e.to_string()))
}

fn new_index<'s>(owner: OwnerToken<'s>, kind: SEXPTYPE, len: R_xlen_t) -> SexpMut<'s> {
    checked_index(owner.require_active());
    let p = Rf_allocVector3(kind, len);
    checked_index(owner.require_active());
    checked_index(SexpMut::try_from_checked(checked_index(owner.sexp(p))))
}

fn check_subscript_count(actual: R_xlen_t, expected: R_xlen_t) {
    if actual != expected {
        error("subscript changed during indexing");
    }
}

// ---------------------------------------------------------------------------
// nullSubscript — create a 1:n index vector
// ---------------------------------------------------------------------------

/// Create a null subscript (a vector of 1, 2, ..., n).
///
/// Used when the subscript is missing (R_MissingArg).
fn nullSubscript(owner: OwnerToken<'_>, n: R_xlen_t) -> SEXP {
    let mut ans = new_index(owner, SEXPTYPE::INTSXP, n.max(0));
    for i in 0..n {
        checked_index(ans.try_set_integer_elt(i, (i + 1) as c_int));
    }
    ans.freeze().as_raw()
}

// ---------------------------------------------------------------------------
// logicalSubscript — logical vector to index vector
// ---------------------------------------------------------------------------

/// Expand a logical subscript into integer indices.
///
/// TRUE values select their position, NA_LOGICAL inserts NA, FALSE skips.
/// The logical vector is recycled to length `nx` if shorter.
fn logicalSubscript(
    owner: OwnerToken<'_>,
    s: &Sexp<'_>,
    ns: R_xlen_t,
    nx: R_xlen_t,
    stretch: &mut R_xlen_t,
) -> SEXP {
    // Trunk semantics: entry value > 0 means stretching is allowed; when
    // the logical subscript is longer than x, the caller must grow x to
    // `ns` (not merely to the count of TRUEs, which could leave the
    // returned indices out of range of the grown vector).
    let canstretch = *stretch > 0;
    *stretch = if ns > nx { ns } else { 0 };
    if ns == 0 {
        return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
    }
    if !canstretch && ns > nx {
        error("(subscript) logical subscript too long");
    }

    let out_len = if ns < nx { nx } else { ns };

    // The pinned GNU oracle silently recycles logical indices even
    // when the target length is not an exact multiple. This admission
    // also holds with warn=2; dimension-length rejection remains above.

    // Count TRUE and NA values to determine result length. R logical
    // subscripts recycle to the target length when shorter; NA selects an
    // NA slot in the result, just like stock subscript.c.
    let mut count: R_xlen_t = 0;
    for i in 0..out_len {
        let v = checked_index(s.try_logical_elt(i % ns));
        if v == 1 || v == NA_LOGICAL {
            count += 1;
        }
    }

    // Fill in indices through typed cells so provider callbacks and
    // collection cannot invalidate the destination.
    let mut ans = new_index(owner, SEXPTYPE::INTSXP, count);
    let mut j: R_xlen_t = 0;
    for i in 0..out_len {
        let v = checked_index(s.try_logical_elt(i % ns));
        let selected = match v {
            1 => (i + 1) as c_int,
            NA_LOGICAL => NA_INTEGER,
            _ => continue,
        };
        checked_index(ans.try_set_integer_elt(j, selected));
        j += 1;
    }
    check_subscript_count(j, count);
    ans.freeze().as_raw()
}

// ---------------------------------------------------------------------------
// negativeSubscript — negative integer subscript handling
// ---------------------------------------------------------------------------

/// Process a negative subscript by creating a logical mask and inverting.
fn negativeSubscript(
    owner: OwnerToken<'_>,
    s: &Sexp<'_>,
    ns: R_xlen_t,
    nx: R_xlen_t,
) -> SEXP {
    if nx <= 0 {
        return Rf_allocVector3(SEXPTYPE::INTSXP, 0);
    }

    // Create a logical mask: TRUE for selected, FALSE for excluded
    let mut mask = new_index(owner, SEXPTYPE::LGLSXP, nx);
    for i in 0..nx {
        checked_index(mask.try_set_logical_elt(i, 1));
    }

    // Mark excluded indices from the subscript
    for i in 0..ns {
        let v = checked_index(s.try_integer_elt(i));
        if v == NA_INTEGER {
            error("NA's in subscript are not allowed");
        }
        let idx = -i64::from(v);
        if (1..=nx).contains(&idx) {
            checked_index(mask.try_set_logical_elt(idx - 1, 0));
        }
        if v > 0 {
            error("only 0's may be mixed with negative subscripts");
        }
    }

    // Now use logicalSubscript on the mask
    let mut stretch: R_xlen_t = 0;
    logicalSubscript(owner, &mask.freeze(), nx, nx, &mut stretch)
}

// ---------------------------------------------------------------------------
// positiveSubscript — positive integer subscript handling
// ---------------------------------------------------------------------------

/// Process a positive subscript by removing zeros.
fn positiveSubscript(owner: OwnerToken<'_>, s: &Sexp<'_>, ns: R_xlen_t) -> SEXP {
    // Upstream (subscript.c positiveSubscript) drops zeros and keeps
    // every positive index — including on zero-length targets, where
    // the caller (integerSubscript) has already either raised the
    // out-of-bounds error or recorded the stretch. An nx == 0 early
    // return is a misport: it made `r[1:2] <- list(...)` on an empty
    // list enlarge `r` (SubassignTypeFix) yet assign nothing, leaving
    // NULL elements behind.

    // Count non-zero values
    let mut count: R_xlen_t = 0;
    for i in 0..ns {
        if checked_index(s.try_integer_elt(i)) != 0 {
            count += 1;
        }
    }

    let mut ans = new_index(owner, SEXPTYPE::INTSXP, count);
    let mut j: R_xlen_t = 0;
    for i in 0..ns {
        let v = checked_index(s.try_integer_elt(i));
        if v != 0 {
            checked_index(ans.try_set_integer_elt(j, v));
            j += 1;
        }
    }
    check_subscript_count(j, count);
    ans.freeze().as_raw()
}

// ---------------------------------------------------------------------------
// integerSubscript — integer subscript handling
// ---------------------------------------------------------------------------

/// Process an integer subscript, dispatching to negative or positive handling.
///
/// Detects mixed NA/negative/positive patterns and enforces R's subscript rules:
/// - Only 0's may be mixed with negative subscripts
/// - Positive subscripts beyond nx may trigger stretching
fn integerSubscript(
    owner: OwnerToken<'_>,
    s: &Sexp<'_>,
    ns: R_xlen_t,
    nx: R_xlen_t,
    stretch: &mut R_xlen_t,
    call: SEXP,
    x: SEXP,
) -> SEXP {
    // Trunk semantics (subscript.c): the entry value of *stretch > 0 means
    // the caller allows growth; on return *stretch is 0, or the new
    // required length of x when the largest positive index exceeds nx.
    let canstretch = *stretch > 0;
    *stretch = 0;
    let mut max: c_int = 0;
    let mut isna = false;
    let mut neg = false;
    for i in 0..ns {
        let ii = checked_index(s.try_integer_elt(i));
        if ii < 0 {
            if ii == NA_INTEGER {
                isna = true;
            } else {
                neg = true;
            }
        } else if ii > max {
            max = ii;
        }
    }
    let umax = max as R_xlen_t;
    if umax > nx {
        if canstretch {
            *stretch = umax;
        } else {
            ECALL_OutOfBounds(x, -1, umax, call);
        }
    }
    if neg {
        if max == 0 && !isna {
            return negativeSubscript(owner, s, ns, nx);
        } else {
            error("only 0's may be mixed with negative subscripts");
        }
    }
    positiveSubscript(owner, s, ns)
}

// ---------------------------------------------------------------------------
// realSubscript — real (double) subscript handling
// ---------------------------------------------------------------------------

/// Process a real (double) subscript.
///
/// Converts to integer indices, handling NA, Inf, truncation, and
/// negative values according to R's subscript rules.
fn realSubscript(
    owner: OwnerToken<'_>,
    s: &Sexp<'_>,
    ns: R_xlen_t,
    nx: R_xlen_t,
    stretch: &mut R_xlen_t,
    call: SEXP,
    x: SEXP,
) -> SEXP {
    // Trunk semantics (subscript.c realSubscript): entry *stretch > 0
    // allows growth; fractional/finite values truncate, NA/NaN/Inf map to
    // NA indices. May return a REALSXP index vector when any value
    // exceeds INT_MAX.
    let canstretch = *stretch > 0;
    *stretch = 0;
    let mut min = 0.0f64;
    let mut max = 0.0f64;
    let mut isna = false;
    for i in 0..ns {
        let ii = checked_index(s.try_real_elt(i));
        if ii.is_finite() {
            if ii < min {
                min = ii;
            }
            if ii > max {
                max = ii;
            }
        } else {
            isna = true;
        }
    }
    if max >= nx as f64 + 1.0 {
        if canstretch {
            *stretch = max as R_xlen_t;
        } else {
            ECALL_OutOfBounds(x, -1, max as R_xlen_t, call);
        }
    }
    if min <= -1.0 {
        if max < 1.0 && !isna {
            let mut stretch2: R_xlen_t = 0;
            let mut indx = new_index(owner, SEXPTYPE::LGLSXP, nx);
            for i in 0..nx {
                checked_index(indx.try_set_logical_elt(i, 1));
            }
            for i in 0..ns {
                let dx = checked_index(s.try_real_elt(i));
                if dx.is_finite() && dx <= -1.0 && -dx < nx as f64 + 1.0 {
                    let ix = (-dx - 1.0) as R_xlen_t;
                    checked_index(indx.try_set_logical_elt(ix, 0));
                }
            }
            return logicalSubscript(owner, &indx.freeze(), nx, nx, &mut stretch2);
        } else {
            error("only 0's may be mixed with negative subscripts");
        }
    }
    // Positive path: return a REALSXP index only if needed.
    let mut int_ok = true;
    let mut cnt: R_xlen_t = 0;
    for i in 0..ns {
        let ds = checked_index(s.try_real_elt(i));
        if ds.is_finite() && ds > c_int::MAX as f64 {
            int_ok = false;
        }
        if !ds.is_finite() || (ds as R_xlen_t) != 0 {
            cnt += 1;
        }
    }
    if int_ok {
        let mut indx = new_index(owner, SEXPTYPE::INTSXP, cnt);
        let mut j: R_xlen_t = 0;
        for i in 0..ns {
            let ds = checked_index(s.try_real_elt(i));
            let ia: c_int = if !ds.is_finite() {
                NA_INTEGER
            } else {
                ds as c_int
            };
            if ia != 0 {
                checked_index(indx.try_set_integer_elt(j, ia));
                j += 1;
            }
        }
        check_subscript_count(j, cnt);
        indx.freeze().as_raw()
    } else {
        let mut indx = new_index(owner, SEXPTYPE::REALSXP, cnt);
        let mut j: R_xlen_t = 0;
        for i in 0..ns {
            let ds = checked_index(s.try_real_elt(i));
            if !ds.is_finite() || (ds as R_xlen_t) != 0 {
                checked_index(indx.try_set_real_elt(j, ds));
                j += 1;
            }
        }
        check_subscript_count(j, cnt);
        indx.freeze().as_raw()
    }
}

// ---------------------------------------------------------------------------
// stringSubscript — string/character subscript handling
// ---------------------------------------------------------------------------

/// Process a string (character) subscript.
///
/// Matches strings against names on the vector. For assignment contexts,
/// new names cause the vector to be "stretched". Uses hashing for large
/// name sets (ns * nx > 15*nx + ns) for performance.
fn stringSubscript(
    owner: OwnerToken<'_>,
    s: SEXP,
    ns: R_xlen_t,
    nx: R_xlen_t,
    names: SEXP,
    stretch: &mut R_xlen_t,
    call: SEXP,
    x: SEXP,
    dim: c_int,
) -> SEXP {
    let _ = (call, dim);
    // Trunk semantics (subscript.c stringSubscript): unmatched names are
    // an error unless stretching is allowed; when stretching, the new
    // names are returned as the "use.names" attribute (a VECSXP) of the
    // generated subscript vector, and *stretch becomes the new required
    // length of x. VectorAssign consumes it to name the new elements.
    let canstretch = *stretch > 0;
    *stretch = 0;

    let _names = (TYPEOF(names) == SEXPTYPE::STRSXP).then(|| checked_index(owner.sexp(names)));
    let string_at = |x: SEXP, i: R_xlen_t| {
        let value = STRING_ELT(x, i);
        checked_index(owner.require_active());
        value
    };

    let is_non_null_string = |elt: SEXP| -> bool {
        !elt.is_null() && elt != R_NilValue() && elt != crate::sexp::globals::R_NaString()
    };

    // First pass: match against the names on x. Unmatched entries stay 0.
    // (Trunk only hashes for large inputs; linear matching is equivalent.)
    let mut indx = new_index(owner, SEXPTYPE::INTSXP, ns);
    let mut guards: Vec<_> = Vec::new();
    for i in 0..ns {
        let mut sub: R_xlen_t = 0;
        let si = string_at(s, i);
        if si == crate::sexp::globals::R_NaString() {
            checked_index(indx.try_set_integer_elt(i, crate::sexp::ffi::NA_INTEGER));
            continue;
        }
        if is_non_null_string(si) {
            let sbytes = &crate::sexp::accessors::char_cstr_bytes(si)[..];
            if !names.is_null() && TYPEOF(names) == SEXPTYPE::STRSXP {
                for j in 0..nx {
                    let name_j = string_at(names, j);
                    if is_non_null_string(name_j) {
                        let nbytes = &crate::sexp::accessors::char_cstr_bytes(name_j)[..];
                        if !sbytes.is_empty() && nbytes == sbytes {
                            sub = j + 1;
                            break;
                        }
                    }
                }
            }
        }
        checked_index(indx.try_set_integer_elt(i, sub as c_int));
    }

    // Second pass: resolve unmatched entries against earlier subscripts
    // (duplicates of an already-seen new name share its slot); genuinely
    // new names extend the vector when allowed.
    let nnames = nx;
    let mut extra = nnames;
    let mut indexnames: SEXP = R_NilValue();
    let mut sindx: Option<Sexp<'_>> = None;
    for i in 0..ns {
        let mut sub = indx
            .integer_elt(i)
            .unwrap_or_else(|| error("invalid subscript staging index"))
            as R_xlen_t;
        if sub == 0 {
            let si = string_at(s, i);
            if is_non_null_string(si) {
                if indexnames.is_null() || indexnames == R_NilValue() {
                    // match(s, s, 0): first-occurrence index of each element
                    let mut matches = new_index(owner, SEXPTYPE::INTSXP, ns);
                    for a in 0..ns {
                        let ea = string_at(s, a);
                        let mut found: R_xlen_t = 0;
                        if is_non_null_string(ea) {
                            let abytes = &crate::sexp::accessors::char_cstr_bytes(ea)[..];
                            for b in 0..a {
                                let eb = string_at(s, b);
                                if is_non_null_string(eb) {
                                    let bbytes =
                                        &crate::sexp::accessors::char_cstr_bytes(eb)[..];
                                    if bbytes == abytes {
                                        found = b + 1;
                                        break;
                                    }
                                }
                            }
                        }
                        checked_index(matches.try_set_integer_elt(a, found as c_int));
                    }
                    sindx = Some(matches.freeze());
                    indexnames = Rf_allocVector3(SEXPTYPE::VECSXP, ns);
                    guards.push(protect(indexnames));
                    for z in 0..ns {
                        SET_VECTOR_ELT(indexnames, z, R_NilValue());
                    }
                }
                let j = i64::from(checked_index(
                    sindx
                        .as_ref()
                        .expect("initialized index matches")
                        .try_integer_elt(i),
                ));
                if j >= 1 {
                    sub = indx
                        .integer_elt(j - 1)
                        .unwrap_or_else(|| error("invalid subscript staging index"))
                        as R_xlen_t;
                    SET_VECTOR_ELT(indexnames, i, string_at(s, j - 1));
                }
            }
        }
        if sub == 0 {
            if !canstretch {
                ECALL_OutOfBoundsCHAR(x, dim, string_at(s, i), call);
            }
            extra += 1;
            sub = extra;
            SET_VECTOR_ELT(indexnames, i, string_at(s, i));
        }
        checked_index(indx.try_set_integer_elt(i, sub as c_int));
    }

    if extra != nnames {
        use crate::eval::attrib_core::setAttrib;
        setAttrib(
            indx.as_raw(),
            crate::eval::attrib_core::R_UseNamesSymbol(),
            indexnames,
        );
        if canstretch {
            *stretch = extra;
        }
    }
    indx.freeze().as_raw()
}

// ---------------------------------------------------------------------------
// int_arraySubscript — array subscript by dimension (internal)
// ---------------------------------------------------------------------------

/// Compute the subscript for one dimension of an array.
///
/// This is the internal implementation used by `[i,j,...]` and `[<-...`.
/// The public API is `arraySubscript`.
pub fn int_arraySubscript(dim: c_int, s: SEXP, dims: SEXP, x: SEXP, call: SEXP) -> SEXP {
    checked_index(OwnerToken::with_current(|owner| {
        let _x = (!x.is_null()).then(|| checked_index(owner.sexp(x)));
        let _dims = (!dims.is_null()).then(|| checked_index(owner.sexp(dims)));
        let source = checked_index(owner.sexp(s));

        let mut stretch: R_xlen_t = 0;
        let ns = if TYPEOF(s) == SEXPTYPE::SYMSXP {
            0
        } else {
            LENGTH(s)
        };
        let nd = INTEGER_ELT(dims, dim);
        checked_index(owner.require_active());
        crate::mainutils::subset::note_oob_subscript(dim + 1);
        let stype = TYPEOF(s);
        let ans = if stype == SEXPTYPE::NILSXP {
            Rf_allocVector3(SEXPTYPE::INTSXP, 0)
        } else if stype == SEXPTYPE::LGLSXP {
            logicalSubscript(owner, &source, ns as R_xlen_t, nd as R_xlen_t, &mut stretch)
        } else if stype == SEXPTYPE::INTSXP {
            integerSubscript(
                owner,
                &source,
                ns as R_xlen_t,
                nd as R_xlen_t,
                &mut stretch,
                call,
                x,
            )
        } else if stype == SEXPTYPE::REALSXP {
            realSubscript(
                owner,
                &source,
                ns as R_xlen_t,
                nd as R_xlen_t,
                &mut stretch,
                call,
                x,
            )
        } else if stype == SEXPTYPE::STRSXP {
            let dnames = crate::eval::attrib_core::getAttrib(
                x,
                crate::eval::attrib_core::R_DimNamesSymbol(),
            );
            if dnames.is_null() || dnames == R_NilValue() {
                error("no 'dimnames' attribute for array");
            }
            let _dnames_guard = protect(dnames);
            let dnames_col = if dim < LENGTH(dnames) {
                VECTOR_ELT(dnames, dim as R_xlen_t)
            } else {
                R_NilValue()
            };
            checked_index(owner.require_active());
            stringSubscript(
                owner,
                s,
                ns as R_xlen_t,
                nd as R_xlen_t,
                dnames_col,
                &mut stretch,
                call,
                x,
                dim,
            )
        } else if stype == SEXPTYPE::SYMSXP {
            nullSubscript(owner, nd as R_xlen_t)
        } else {
            error("invalid subscript type 'unknown'");
        };
        crate::mainutils::subset::note_oob_subscript(0);
        checked_index(owner.require_active());
        ans
    }))
}

// ---------------------------------------------------------------------------
// arraySubscript — public API for array subscripting
// ---------------------------------------------------------------------------

/// Compute the subscript for one dimension of an array (public API).
///
/// This is used by packages arules, cba, proxy, and seriation.
/// Delegates to `int_arraySubscript` with `call = R_NilValue`.
pub fn arraySubscript(
    dim: c_int,
    s: SEXP,
    dims: SEXP,
    _dng: usize,
    _strg: usize,
    x: SEXP,
) -> SEXP {
    // Note: dng and strg are function pointer parameters in C
    // (AttrGetter and StringEltGetter typedefs), ignored in this port.
    int_arraySubscript(dim, s, dims, x, R_NilValue())
}

// ---------------------------------------------------------------------------
// make_subscript — subscript creation for [ and [<-
// ---------------------------------------------------------------------------

/// Create a subscript vector for `[` and `[<-` operators.
///
/// Handles all R subscript types: NULL (empty), logical, integer, real,
/// string, and missing. For simple in-range scalar indices, returns the
/// input directly without copying.
///
/// If `*stretch` is 0 on entry, the vector `x` cannot be stretched.
/// Otherwise, `*stretch` returns the new required length for `x`.
pub(crate) fn make_subscript(
    x: SEXP,
    s: SEXP,
    stretch: &mut R_xlen_t,
    call: SEXP,
) -> SEXP {
    checked_index(OwnerToken::with_current(|owner| {
        let _x = (!x.is_null()).then(|| checked_index(owner.sexp(x)));
        let source = checked_index(owner.sexp(s));

        let nx = XLENGTH(x);

        // Special case for simple scalar indices — does not duplicate
        let stype = TYPEOF(s);
        if stype == SEXPTYPE::INTSXP && IS_SCALAR(s, SEXPTYPE::INTSXP.as_c_int()) != 0 {
            let i = SCALAR_IVAL(s);
            if i > 0 && (i as R_xlen_t) <= nx {
                *stretch = 0;
                return s;
            }
        } else if stype == SEXPTYPE::REALSXP && IS_SCALAR(s, SEXPTYPE::REALSXP.as_c_int()) != 0 {
            let di = SCALAR_DVAL(s);
            if di >= 1.0 && (di as R_xlen_t) <= nx {
                *stretch = 0;
                return s;
            }
        }
        // Scalar reads may have invoked a provider; re-validate.
        checked_index(owner.require_active());

        let ns = if matches!(
            SEXPTYPE(TYPEOF(s)),
            SEXPTYPE::NILSXP
                | SEXPTYPE::LGLSXP
                | SEXPTYPE::INTSXP
                | SEXPTYPE::REALSXP
                | SEXPTYPE::STRSXP
        ) {
            XLENGTH(s)
        } else {
            0
        };

        let stype2 = TYPEOF(s);
        let ans = if stype2 == SEXPTYPE::NILSXP {
            *stretch = 0;
            Rf_allocVector3(SEXPTYPE::INTSXP, 0)
        } else if stype2 == SEXPTYPE::LGLSXP {
            logicalSubscript(owner, &source, ns, nx, stretch)
        } else if stype2 == SEXPTYPE::INTSXP {
            integerSubscript(owner, &source, ns, nx, stretch, call, x)
        } else if stype2 == SEXPTYPE::REALSXP {
            realSubscript(owner, &source, ns, nx, stretch, call, x)
        } else if stype2 == SEXPTYPE::STRSXP {
            let names =
                crate::eval::attrib_core::getAttrib(x, crate::eval::attrib_core::R_NamesSymbol());
            stringSubscript(owner, s, ns, nx, names, stretch, call, x, -1)
        } else if stype2 == SEXPTYPE::SYMSXP {
            *stretch = 0;
            nullSubscript(owner, nx)
        } else {
            error("invalid subscript type 'unknown'");
        };
        checked_index(owner.require_active());
        ans
    }))
}

/// Raw-pointer compatibility adapter for subscript construction.
///
/// # Safety
/// A non-null `stretch` must be readable and writable through the call.
pub unsafe fn makeSubscript(x: SEXP, s: SEXP, stretch: *mut R_xlen_t, call: SEXP) -> SEXP {
    let mut value = if stretch.is_null() {
        0
    } else {
        // SAFETY: the caller supplies a readable slot.
        unsafe { stretch.read() }
    };
    let result = make_subscript(x, s, &mut value, call);
    if !stretch.is_null() {
        // SAFETY: the caller retains a writable slot.
        unsafe { stretch.write(value) };
    }
    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::sexp::ffi::{FALSE, NA_INTEGER, TRUE};
    use std::ptr;
    use std::os::raw::{c_char, c_double, c_int};
    use std::panic::{self, AssertUnwindSafe};

    use super::*;

    fn make_int_vector(values: &[c_int]) -> SEXP {
        unsafe {
            let x = Rf_allocVector3(SEXPTYPE::INTSXP, values.len() as R_xlen_t);
            let ints = INTEGER(x);
            for (i, v) in values.iter().enumerate() {
                *ints.add(i) = *v;
            }
            x
        }
    }

    fn make_real_vector(values: &[c_double]) -> SEXP {
        unsafe {
            let x = Rf_allocVector3(SEXPTYPE::REALSXP, values.len() as R_xlen_t);
            let reals = REAL(x);
            for (i, v) in values.iter().enumerate() {
                *reals.add(i) = *v;
            }
            x
        }
    }

    fn make_string_vector(values: &[&[u8]]) -> SEXP {
        unsafe {
            let x = Rf_allocVector3(SEXPTYPE::STRSXP, values.len() as R_xlen_t);
            for (i, v) in values.iter().enumerate() {
                SET_STRING_ELT(x, i as R_xlen_t, Rf_mkChar(v.as_ptr() as *const c_char));
            }
            x
        }
    }

    fn make_dim_matrix(values: &[c_int], nrow: c_int, ncol: c_int) -> SEXP {
        unsafe {
            let x = make_int_vector(values);
            let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            let dim_ints = INTEGER(dim);
            *dim_ints = nrow;
            *dim_ints.add(1) = ncol;
            crate::eval::attrib_core::setAttrib(x, crate::eval::attrib_core::R_DimSymbol(), dim);
            x
        }
    }

    fn make_dimnames_list(first: &[&[u8]], second: &[&[u8]]) -> SEXP {
        unsafe {
            let dimnames = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            let first = make_string_vector(first);
            let second = make_string_vector(second);
            SET_VECTOR_ELT(dimnames, 0, first);
            SET_VECTOR_ELT(dimnames, 1, second);
            dimnames
        }
    }

    fn panic_message<F>(f: F) -> String
    where
        F: FnOnce(),
    {
        match panic::catch_unwind(AssertUnwindSafe(f)) {
            Ok(_) => panic!("expected panic"),
            Err(payload) => {
                if let Some(err) = payload.downcast_ref::<crate::sexp::context::RError>() {
                    err.message.clone()
                } else if let Some(s) = payload.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = payload.downcast_ref::<&'static str>() {
                    (*s).to_string()
                } else {
                    panic!("unexpected panic payload type")
                }
            }
        }
    }

    #[test]
    fn test_integer_one_index_positive() {
        let _session = crate::sexp::session::RSession::new();
        {
            // 1-based index 3 -> 0-based index 2
            assert_eq!(integerOneIndex(3, 10, ptr::null_mut()), 2);
            // 1-based index 1 -> 0-based index 0
            assert_eq!(integerOneIndex(1, 10, ptr::null_mut()), 0);
        }
    }

    #[test]
    #[should_panic]
    fn test_integer_one_index_zero() {
        let _session = crate::sexp::session::RSession::new();
        {
            integerOneIndex(0, 10, ptr::null_mut());
        }
    }

    #[test]
    fn test_integer_one_index_negative_len2() {
        let _session = crate::sexp::session::RSession::new();
        {
            // For length-2 vector, -1 selects element 1, -2 selects element 0
            assert_eq!(integerOneIndex(-1, 2, ptr::null_mut()), 1);
            assert_eq!(integerOneIndex(-2, 2, ptr::null_mut()), 0);
        }
    }

    #[test]
    #[should_panic]
    fn test_integer_one_index_negative_len2_out_of_range() {
        let _session = crate::sexp::session::RSession::new();
        {
            integerOneIndex(-3, 2, ptr::null_mut());
        }
    }

    #[test]
    #[should_panic]
    fn test_integer_one_index_negative_other_lengths() {
        let _session = crate::sexp::session::RSession::new();
        {
            integerOneIndex(-1, 1, ptr::null_mut());
        }
    }

    #[test]
    #[should_panic]
    fn test_one_index_null_args() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut newname: SEXP = ptr::null_mut();
            OneIndex(
                ptr::null_mut(),
                ptr::null_mut(),
                10,
                0,
                &mut newname,
                -1,
                ptr::null_mut(),
            );
        }
    }

    #[test]
    #[should_panic]
    fn test_get1index_null_args() {
        let _session = crate::sexp::session::RSession::new();
        get1index(ptr::null_mut(), ptr::null_mut(), 10, 0, -1, ptr::null_mut());
    }

    #[test]
    #[should_panic]
    fn test_get1index_pos_out_of_range() {
        let _session = crate::sexp::session::RSession::new();
        get1index(ptr::null_mut(), ptr::null_mut(), 10, 0, 5, ptr::null_mut());
    }

    #[test]
    fn test_vector_index_null_input_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        let result = vectorIndex(
            ptr::null_mut(),
            ptr::null_mut(),
            0,
            1,
            0,
            ptr::null_mut(),
            0,
        );
        assert_eq!(result, R_NilValue());
    }

    #[test]
    fn test_mat2indsub_converts_matrix_subscripts() {
        let _session = crate::sexp::session::RSession::new();
        let dims = make_int_vector(&[2, 3]);
        let s = make_dim_matrix(&[1, 2, 1, 3], 2, 2);
        let result = mat2indsub(dims, s, ptr::null_mut(), ptr::null_mut());
        assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
        assert_eq!(LENGTH(result), 2);
        assert_eq!(INTEGER_ELT(result, 0), 1);
        assert_eq!(INTEGER_ELT(result, 1), 6);
    }

    #[test]
    fn test_strmat2intmat_converts_matrix_names() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let dimnames = make_dimnames_list(&[b"a\0", b"b\0"], &[b"c\0", b"d\0"]);
            let x = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
            crate::eval::attrib_core::setAttrib(
                x,
                crate::eval::attrib_core::R_DimNamesSymbol(),
                dimnames,
            );

            let s = make_string_vector(&[b"a\0", b"b\0", b"c\0", b"d\0"]);
            let sdim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            *INTEGER(sdim) = 2;
            *INTEGER(sdim).add(1) = 2;
            crate::eval::attrib_core::setAttrib(s, crate::eval::attrib_core::R_DimSymbol(), sdim);

            let result = strmat2intmat(s, dimnames, ptr::null_mut(), x);
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(result), 4);
            assert_eq!(INTEGER_ELT(result, 0), 1);
            assert_eq!(INTEGER_ELT(result, 1), 2);
            assert_eq!(INTEGER_ELT(result, 2), 1);
            assert_eq!(INTEGER_ELT(result, 3), 2);
        }
    }

    #[test]
    fn test_ecall_out_of_bounds_reports_context() {
        let _session = crate::sexp::session::RSession::new();
        {
            let x = make_int_vector(&[1, 2, 3]);
            let msg = panic_message(|| {
                ECALL_OutOfBounds(x, 2, 7, ptr::null_mut());
            });
            assert!(msg.contains("subscript out of bounds"));
            assert!(msg.contains("integer"));
            assert!(msg.contains("subscript 2"));
            assert!(msg.contains("index 7"));
        }
    }

    #[test]
    fn test_ecall_missing_subs_reports_context() {
        let _session = crate::sexp::session::RSession::new();
        let msg = panic_message(|| {
            ECALL_MissingSubs(ptr::null_mut());
        });
        assert_eq!(msg, "subscript is missing");
    }

    #[test]
    fn test_ecall_out_of_bounds_char_reports_context() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = make_real_vector(&[1.0]);
            let sindex = crate::sexp::constructors::mk_string_str("foo");
            let msg = panic_message(|| {
                ECALL_OutOfBoundsCHAR(x, 1, sindex, ptr::null_mut());
            });
            assert!(msg.contains("subscript out of bounds"));
            assert!(msg.contains("double"));
            assert!(msg.contains("subscript 1"));
            assert!(msg.contains("foo"));
        }
    }

    #[test]
    fn test_make_subscript_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut stretch: R_xlen_t = 0;
            let result =
                makeSubscript(ptr::null_mut(), R_NilValue(), &mut stretch, ptr::null_mut());
            // NILSXP case -> stretch = 0, returns empty INTSXP
            assert_eq!(stretch, 0);
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(result), 0);
        }
    }

    #[test]
    fn test_make_subscript_scalar_int_in_range() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = make_int_vector(&[10, 20, 30]);
            let s = Rf_ScalarInteger(2);
            SET_SCALAR(s, 1);
            let mut stretch: R_xlen_t = 1;
            let result = makeSubscript(x, s, &mut stretch, ptr::null_mut());
            assert_eq!(result, s);
            assert_eq!(stretch, 0);
        }
    }

    #[test]
    fn test_make_subscript_scalar_real_in_range() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = make_int_vector(&[10, 20, 30]);
            let s = Rf_ScalarReal(2.0);
            SET_SCALAR(s, 1);
            let mut stretch: R_xlen_t = 1;
            let result = makeSubscript(x, s, &mut stretch, ptr::null_mut());
            assert_eq!(result, s);
            assert_eq!(stretch, 0);
        }
    }

    #[test]
    fn test_int_array_subscript_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = int_arraySubscript(
                0,
                R_NilValue(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            // Returns empty INTSXP for NILSXP subscript
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(result), 0);
        }
    }

    #[test]
    fn test_array_subscript_delegates() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = arraySubscript(0, R_NilValue(), ptr::null_mut(), 0, 0, ptr::null_mut());
            // arraySubscript delegates to int_arraySubscript which returns empty INTSXP
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(result), 0);
        }
    }

    /// A variable logical provider may collect mid-fill; the typed
    /// destination stays valid because SexpMut retains the owner.
    #[test]
    fn subscript_result_survives_collection_during_altrep_fill() {
        use crate::sexp::{
            RSession,
            altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
        };
        use std::{cell::Cell, rc::Rc};

        struct CollectLogical(Rc<Cell<usize>>);
        impl AltrepClass for CollectLogical {
            fn vector_type(&self) -> SEXPTYPE {
                SEXPTYPE::LGLSXP
            }
            fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
                Ok(3)
            }
            fn element<'s>(
                &self,
                _: &AltrepContext<'s>,
                i: i64,
            ) -> SexpResult<AltrepElement<'s>> {
                let calls = self.0.get() + 1;
                self.0.set(calls);
                if calls == 4 {
                    crate::sexp::gengc::full_gc();
                }
                Ok(AltrepElement::Logical([1, 0, NA_LOGICAL][i as usize]))
            }
        }

        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let reads = Rc::new(Cell::new(0));
            let class = session
                .register_altrep_class("subscript-logical-gc", CollectLogical(reads.clone()))
                .unwrap();
            let s = AltrepBuilder::new(class).build().unwrap();
            let owner = session.owner_token().unwrap();
            let x = owner
                .sexp(Rf_allocVector3(SEXPTYPE::INTSXP, 3))
                .unwrap();
            let mut stretch = 1;
            let result = make_subscript(x.as_raw(), s.as_raw(), &mut stretch, R_NilValue());
            assert_eq!(reads.get(), 6);
            assert_eq!(stretch, 0);
            assert_eq!(XLENGTH(result), 2);
            assert_eq!(INTEGER_ELT(result, 0), 1);
            assert_eq!(INTEGER_ELT(result, 1), NA_INTEGER);
        });
    }

    /// Matrix subscript reads stay lazy: each cell is fetched on demand and
    /// the typed destination survives a collection mid-scan.
    #[test]
    fn matrix_subscript_keeps_altrep_lazy_during_collection() {
        use crate::sexp::{
            RSession,
            altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
        };
        use std::{cell::Cell, rc::Rc};

        struct CollectInteger(Rc<Cell<usize>>);
        impl AltrepClass for CollectInteger {
            fn vector_type(&self) -> SEXPTYPE {
                SEXPTYPE::INTSXP
            }
            fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
                Ok(4)
            }
            fn element<'s>(
                &self,
                _: &AltrepContext<'s>,
                i: i64,
            ) -> SexpResult<AltrepElement<'s>> {
                let calls = self.0.get() + 1;
                self.0.set(calls);
                if calls == 2 {
                    crate::sexp::gengc::full_gc();
                }
                Ok(AltrepElement::Integer([1, 2, 1, 3][i as usize]))
            }
        }

        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let reads = Rc::new(Cell::new(0));
            let class = session
                .register_altrep_class("subscript-matrix-gc", CollectInteger(reads.clone()))
                .unwrap();
            let source = AltrepBuilder::new(class).build().unwrap();
            let owner = session.owner_token().unwrap();

            let mut sdim = new_index(owner, SEXPTYPE::INTSXP, 2);
            checked_index(sdim.try_set_integer_elt(0, 2));
            checked_index(sdim.try_set_integer_elt(1, 2));
            let sdim = sdim.freeze();
            setAttrib(source.as_raw(), R_DimSymbol(), sdim.as_raw());

            let mut dims = new_index(owner, SEXPTYPE::INTSXP, 2);
            checked_index(dims.try_set_integer_elt(0, 2));
            checked_index(dims.try_set_integer_elt(1, 3));
            let dims = dims.freeze();

            reads.set(0);
            let result = mat2indsub(
                dims.as_raw(),
                source.as_raw(),
                R_NilValue(),
                ptr::null_mut(),
            );
            assert_eq!(reads.get(), 4);
            let result = owner.sexp(result).unwrap();
            assert_eq!(result.try_integer_elt(0).unwrap(), 1);
            assert_eq!(result.try_integer_elt(1).unwrap(), 6);
        });
    }

    /// The string target is copied before a names provider runs, so a
    /// collection mid-match cannot invalidate it.
    #[test]
    fn single_index_keeps_names_until_altrep_collection() {
        use crate::sexp::{
            RSession,
            altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
        };

        struct CollectNames {
            target: bool,
        }
        impl AltrepClass for CollectNames {
            fn vector_type(&self) -> SEXPTYPE {
                SEXPTYPE::STRSXP
            }
            fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
                Ok(if self.target { 1 } else { 2 })
            }
            fn element<'s>(
                &self,
                context: &AltrepContext<'s>,
                i: i64,
            ) -> SexpResult<AltrepElement<'s>> {
                if self.target {
                    return Ok(AltrepElement::String(context.string("alph")?));
                }
                crate::sexp::gengc::full_gc();
                Ok(AltrepElement::String(context.string(if i == 0 {
                    "beta"
                } else {
                    "alpha"
                })?))
            }
        }

        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let names_class = session
                .register_altrep_class("single-index-gc", CollectNames { target: false })
                .unwrap();
            let names = AltrepBuilder::new(names_class).build().unwrap();
            let target_class = session
                .register_altrep_class("single-index-target", CollectNames { target: true })
                .unwrap();
            let target = AltrepBuilder::new(target_class).build().unwrap();
            assert_eq!(
                get1index(target.as_raw(), names.as_raw(), 2, 1, -1, R_NilValue()),
                1
            );
        });
    }

    #[test]
    fn test_na_real_is_nan() {
        let _session = crate::sexp::session::RSession::new();
        assert!(NA_REAL.is_nan());
    }

    #[test]
    fn test_constants() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(NA_INTEGER, c_int::MIN);
        assert_eq!(NA_LOGICAL, c_int::MIN);
        assert_eq!(TRUE, 1);
        assert_eq!(FALSE, 0);
    }
}
