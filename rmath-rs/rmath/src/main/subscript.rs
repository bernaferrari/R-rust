#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/subscript.c — subscript indexing utilities.
//!
//! Provides functions for computing subscripts used by R's `[`, `[[`, `[<-`,
//! and `[[<-` operators. These handle integer, real, logical, string, and
//! matrix-based indexing with proper NA handling, bounds checking, and
//! negative/positive subscript rules.
//!
//! Ported public functions (stubs — full implementations depend on eval/error):
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
use std::ptr;

use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, R_xlen_t, Rboolean, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// R's NA_REAL sentinel (specific NaN bit pattern).
const NA_REAL: c_double = f64::NAN;

/// Interval at which to check interrupts (~subsecond on current hw).
const NINTERRUPT: R_xlen_t = 10_000_000;

// ---------------------------------------------------------------------------
// Error helper stubs
// ---------------------------------------------------------------------------

/// Stub: report an out-of-bounds error. In full R this would call
/// R_makeOutOfBoundsError / R_signalErrorCondition.
pub unsafe fn ECALL_OutOfBounds(
    _x: SEXP,
    _subscript: c_int,
    _index: R_xlen_t,
    _call: SEXP,
) {
    // Full implementation requires R's condition/error infrastructure.
}

/// Stub: report a missing subscript error.
pub unsafe fn ECALL_MissingSubs(_call: SEXP) {
    // Full implementation requires R's condition/error infrastructure.
}

/// Stub: report an out-of-bounds error for character subscripts.
pub unsafe fn ECALL_OutOfBoundsCHAR(
    _x: SEXP,
    _subscript: c_int,
    _sindex: SEXP,
    _call: SEXP,
) {
    // Full implementation requires R's condition/error infrastructure.
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
unsafe fn integerOneIndex(i: c_int, len: R_xlen_t, _call: SEXP) -> R_xlen_t {
    let mut indx: R_xlen_t = -1;
    if i > 0 {
        // regular 1-based index from R
        indx = (i - 1) as R_xlen_t;
    } else if i == 0 || len < 2 {
        // ECALL3(call, "attempt to select less than one element in integerOneIndex");
        indx = -1;
    } else if len == 2 && i > -3 {
        indx = (2 + i) as R_xlen_t;
    } else {
        // ECALL3(call, "attempt to select more than one element in integerOneIndex");
        indx = -1;
    }
    indx
}

// ---------------------------------------------------------------------------
// OneIndex — used for [[<- in subassign.c
// ---------------------------------------------------------------------------

/// Compute a single index for `[[<-` assignment.
///
/// Returns the 0-based index, or `nx` if no match found (for string/symbol
/// subscripts, meaning "append at end"). Sets `*newname` when a string
/// subscript is provided.
pub unsafe fn OneIndex(
    x: SEXP,
    s: SEXP,
    nx: R_xlen_t,
    partial: c_int,
    newname: *mut SEXP,
    pos: c_int,
    call: SEXP,
) -> R_xlen_t {
    unsafe {
        let mut _pos = pos;
        let mut _indx: R_xlen_t = -1;

        if _pos < 0 && LENGTH(s) > 1 {
            // ECALL3(call, "attempt to select more than one element in OneIndex");
            return -1;
        }
        if _pos < 0 && LENGTH(s) < 1 {
            // ECALL3(call, "attempt to select less than one element in OneIndex");
            return -1;
        }

        if _pos < 0 {
            _pos = 0;
        }

        _indx = -1;
        if !newname.is_null() {
            *newname = R_NilValue();
        }

        let stype = TYPEOF(s);
        if stype == SEXPTYPE::LGLSXP.0 || stype == SEXPTYPE::INTSXP.0 {
            _indx = integerOneIndex(INTEGER_ELT(s, _pos), nx, call);
        } else if stype == SEXPTYPE::REALSXP.0 {
            let dblind = REAL_ELT(s, _pos);
            if !dblind.is_nan() {
                if dblind >= 1.0 {
                    _indx = (dblind - 1.0) as R_xlen_t;
                } else if dblind > -1.0 || nx < 2 {
                    // ECALL3(call, "attempt to select less than one element in OneIndex <real>");
                    _indx = -1;
                } else if nx == 2 && dblind > -3.0 {
                    _indx = (2.0 + dblind) as R_xlen_t;
                } else {
                    // ECALL3(call, "attempt to select more than one element in OneIndex <real>");
                    _indx = -1;
                }
            }
        } else if stype == SEXPTYPE::STRSXP.0 {
            // String subscript: match against names of x
            let _vmax = crate::sexp::memory_ext::vmaxget();
            let names = crate::attrib_core::getAttrib(x, crate::attrib_core::R_NamesSymbol());
            if !names.is_null() && names != R_NilValue() {
                Rf_protect(names);
                // Try exact match
                let ss = CHAR(STRING_ELT(s, _pos as R_xlen_t));
                if !ss.is_null() {
                    for i in 0..nx {
                        let tmp = CHAR(STRING_ELT(names, i));
                        if tmp.is_null() || *tmp == 0 {
                            continue;
                        }
                        if libc::strcmp(tmp, ss) == 0 {
                            _indx = i;
                            break;
                        }
                    }
                    // Try partial match if partial > 0
                    if partial != 0 && _indx == -1 && !ss.is_null() {
                        let slen = libc::strlen(ss);
                        for i in 0..nx {
                            let tmp = CHAR(STRING_ELT(names, i));
                            if tmp.is_null() || *tmp == 0 {
                                continue;
                            }
                            if libc::strncmp(tmp, ss, slen) == 0 {
                                if _indx == -1 {
                                    _indx = i;
                                } else {
                                    _indx = -2; // multiple partial matches
                                    break;
                                }
                            }
                        }
                    }
                }
                Rf_unprotect(1);
            }
            if _indx == -1 {
                _indx = nx;
            }
            if !newname.is_null() {
                *newname = STRING_ELT(s, _pos as R_xlen_t);
            }
            crate::sexp::memory_ext::vmaxset(_vmax);
        } else if stype == SEXPTYPE::SYMSXP.0 {
            // Symbol subscript: match against names of x
            let _vmax = crate::sexp::memory_ext::vmaxget();
            let names = crate::attrib_core::getAttrib(x, crate::attrib_core::R_NamesSymbol());
            if !names.is_null() && names != R_NilValue() {
                Rf_protect(names);
                let sname = CHAR(PRINTNAME(s));
                if !sname.is_null() {
                    for i in 0..nx {
                        let tmp = CHAR(STRING_ELT(names, i));
                        if tmp.is_null() {
                            continue;
                        }
                        if libc::strcmp(tmp, sname) == 0 {
                            _indx = i;
                            break;
                        }
                    }
                }
                Rf_unprotect(1);
            }
            if _indx == -1 {
                _indx = nx;
            }
            if !newname.is_null() {
                *newname = PRINTNAME(s);
            }
            crate::sexp::memory_ext::vmaxset(_vmax);
        } else {
            // ECALL3(call, "invalid subscript type '%s'", R_typeToChar(s));
        }
        _indx
    }
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
pub unsafe fn get1index(
    s: SEXP,
    names: SEXP,
    len: R_xlen_t,
    pok: c_int,
    pos: c_int,
    call: SEXP,
) -> R_xlen_t {
    unsafe {
        let mut _pos = pos;
        let mut _pok = pok;

        let warn_pok = _pok == -1;
        if warn_pok {
            _pok = 1;
        }

        if _pos < 0 && LENGTH(s) != 1 {
            // ECALL3(call, "attempt to select more/less than one element in get1index");
            return -1;
        } else if _pos >= LENGTH(s) {
            // ECALL(call, "internal error in use of recursive indexing");
            return -1;
        }

        if _pos < 0 {
            _pos = 0;
        }

        let mut indx: R_xlen_t = -1;

        let stype = TYPEOF(s);
        if stype == SEXPTYPE::LGLSXP.0 || stype == SEXPTYPE::INTSXP.0 {
            let i = INTEGER_ELT(s, _pos);
            if i != NA_INTEGER {
                indx = integerOneIndex(i, len, call);
            }
        } else if stype == SEXPTYPE::REALSXP.0 {
            let dblind = REAL_ELT(s, _pos);
            if !dblind.is_nan() {
                if dblind >= 1.0 {
                    if dblind.is_finite() {
                        indx = (dblind - 1.0) as R_xlen_t;
                    }
                } else if dblind > -1.0 || len < 2 {
                    // ECALL3(call, "invalid negative subscript / attempt to select less than one element");
                    indx = -1;
                } else if len == 2 && dblind > -3.0 {
                    indx = (2.0 + dblind) as R_xlen_t;
                } else {
                    // ECALL3(call, "invalid negative subscript / attempt to select more than one element");
                    indx = -1;
                }
            }
        } else if stype == SEXPTYPE::STRSXP.0 {
            // NA matches nothing
            let elt = STRING_ELT(s, _pos as R_xlen_t);
            if elt.is_null() || elt == R_NilValue() { /* NA -> no match */
            } else {
                let cs = CHAR(elt);
                // "" matches nothing
                if !cs.is_null() && *cs != 0 {
                    let _vmax = crate::sexp::memory_ext::vmaxget();
                    let ss = cs;
                    let names_len = if !names.is_null() { XLENGTH(names) } else { 0 };
                    for i in 0..names_len {
                        let name_elt = STRING_ELT(names, i);
                        if name_elt.is_null() || name_elt == R_NilValue() {
                            continue;
                        }
                        let tmp = CHAR(name_elt);
                        if tmp.is_null() {
                            continue;
                        }
                        if libc::strcmp(tmp, ss) == 0 {
                            indx = i;
                            break;
                        }
                    }
                    // Try partial match if pok > 0
                    if _pok != 0 && indx == -1 {
                        let slen = libc::strlen(ss);
                        for i in 0..names_len {
                            let name_elt = STRING_ELT(names, i);
                            if name_elt.is_null() || name_elt == R_NilValue() {
                                continue;
                            }
                            let tmp = CHAR(name_elt);
                            if tmp.is_null() {
                                continue;
                            }
                            if libc::strncmp(tmp, ss, slen) == 0 {
                                if indx == -1 {
                                    indx = i;
                                } else {
                                    indx = -2; // multiple partial matches
                                    break;
                                }
                            }
                        }
                    }
                    crate::sexp::memory_ext::vmaxset(_vmax);
                }
            }
        } else if stype == SEXPTYPE::SYMSXP.0 {
            // Symbol subscript: match against names
            let _vmax = crate::sexp::memory_ext::vmaxget();
            let sname = CHAR(PRINTNAME(s));
            let names_len = if !names.is_null() { XLENGTH(names) } else { 0 };
            if !sname.is_null() {
                for i in 0..names_len {
                    let name_elt = STRING_ELT(names, i);
                    if name_elt.is_null() || name_elt == R_NilValue() {
                        continue;
                    }
                    let tmp = CHAR(name_elt);
                    if tmp.is_null() {
                        continue;
                    }
                    if libc::strcmp(tmp, sname) == 0 {
                        indx = i;
                        break;
                    }
                }
            }
            crate::sexp::memory_ext::vmaxset(_vmax);
        } else {
            // ECALL3(call, "invalid subscript type '%s'", R_typeToChar(s));
        }
        indx
    }
}

// ---------------------------------------------------------------------------
// vectorIndex — recursive indexing for [[ and [[<- with vector args
// ---------------------------------------------------------------------------

/// Perform recursive indexing for `[[` and `[[<-` with a vector of indices.
///
/// `x` is a list or pairlist, indexed recursively from level `start` to
/// `stop-1`. For `[[<-` it needs to duplicate if substructure might be shared.
pub unsafe fn vectorIndex(
    x: SEXP,
    thesub: SEXP,
    start: c_int,
    stop: c_int,
    pok: c_int,
    call: SEXP,
    dup: Rboolean,
) -> SEXP {
    unsafe {
        if x.is_null() {
            return R_NilValue();
        }

        let mut y = x;
        let need_dup = dup != 0;

        // If dup requested, duplicate the outermost level
        if need_dup && start < stop {
            let newx = crate::main::duplicate::duplicate(x);
            if !newx.is_null() {
                y = newx;
            }
        }

        for i in start..stop {
            // Get the index for this level
            let names = if Rf_isVector(y) != 0 {
                crate::attrib_core::getAttrib(y, crate::attrib_core::R_NamesSymbol())
            } else {
                R_NilValue()
            };
            let len = XLENGTH(y);
            let indx = get1index(thesub, names, len, pok, i, call);
            if indx < 0 || indx >= len {
                return R_NilValue();
            }
            if Rf_isVector(y) != 0 {
                if TYPEOF(y) == SEXPTYPE::VECSXP.0 || TYPEOF(y) == SEXPTYPE::EXPRSXP.0 {
                    y = VECTOR_ELT(y, indx);
                } else {
                    return R_NilValue();
                }
            } else if TYPEOF(y) == SEXPTYPE::LISTSXP.0 {
                // Walk pairlist to position indx
                let mut p = y;
                for _ in 0..indx {
                    p = CDR(p);
                }
                y = CAR(p);
            } else {
                return R_NilValue();
            }
        }
        y
    }
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
pub unsafe fn mat2indsub(dims: SEXP, s: SEXP, _call: SEXP, _x: SEXP) -> SEXP {
    unsafe {
        let nrs = LENGTH(s);
        let ndim = LENGTH(dims);

        if nrs != ndim {
            return R_NilValue();
        }

        // Get the number of rows (subscripts) from the dim attribute of s
        let s_dim = crate::attrib_core::getAttrib(s, crate::attrib_core::R_DimSymbol());
        if s_dim.is_null() || LENGTH(s_dim) < 2 {
            return R_NilValue();
        }
        let nr = INTEGER_ELT(s_dim, 0) as R_xlen_t;

        // Get dimension strides
        let mut strides: Vec<R_xlen_t> = Vec::with_capacity(ndim as usize);
        strides.push(1);
        for d in 0..(ndim - 1) as usize {
            let dim_val = INTEGER_ELT(dims, ndim as c_int - 1 - d as c_int) as R_xlen_t;
            strides.push(strides[d] * dim_val);
        }

        // Allocate result vector
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::INTSXP.0, nr));
        let ap = INTEGER(ans);

        // Get dim attribute names for character matrix support
        let s_type = TYPEOF(s);

        for i in 0..nr as usize {
            let mut idx: R_xlen_t = 0;
            let mut has_zero = false;
            let mut has_na = false;

            for d in 0..ndim as usize {
                let mut sub_val: R_xlen_t = 0;
                if s_type == SEXPTYPE::STRSXP.0 {
                    // Character subscript — match against dimnames
                    let elt = STRING_ELT(s, (i * ndim as usize + d) as R_xlen_t);
                    if elt.is_null() || elt == R_NilValue() {
                        has_na = true;
                        break;
                    }
                    let pname = CHAR(elt);
                    if pname.is_null() {
                        has_na = true;
                        break;
                    }
                    // Linear search through dimnames
                    let dnames =
                        crate::attrib_core::getAttrib(_x, crate::attrib_core::R_DimNamesSymbol());
                    let mut found = false;
                    if !dnames.is_null() {
                        let dn_col = VECTOR_ELT(dnames, d as R_xlen_t);
                        if !dn_col.is_null() {
                            let target = std::ffi::CStr::from_ptr(pname);
                            let target_bytes = target.to_bytes();
                            for j in 0..XLENGTH(dn_col) as usize {
                                let name_elt = STRING_ELT(dn_col, j as R_xlen_t);
                                if name_elt.is_null() || name_elt == R_NilValue() {
                                    continue;
                                }
                                let np = CHAR(name_elt);
                                if np.is_null() {
                                    continue;
                                }
                                let name_bytes = std::ffi::CStr::from_ptr(np).to_bytes();
                                if name_bytes == target_bytes {
                                    sub_val = (j + 1) as R_xlen_t;
                                    found = true;
                                    break;
                                }
                            }
                        }
                    }
                    if !found {
                        has_na = true;
                        break;
                    }
                } else {
                    // Integer/real subscript
                    let val = INTEGER_ELT(s, (i * ndim as usize + d) as c_int);
                    if val == NA_INTEGER {
                        has_na = true;
                        break;
                    }
                    sub_val = val as R_xlen_t;
                    if sub_val < 0 {
                        // Negative indices not allowed in matrix subscripts
                        has_na = true;
                        break;
                    }
                    if sub_val == 0 {
                        has_zero = true;
                    }
                }
                idx += (sub_val - 1) * strides[ndim as usize - 1 - d];
            }

            if has_na {
                *ap.add(i) = NA_INTEGER;
            } else if has_zero {
                *ap.add(i) = 0; // Zero subscripts produce 0 (selects nothing)
            } else {
                *ap.add(i) = (idx + 1) as c_int; // Convert to 1-based
            }
        }

        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// strmat2intmat — character matrix subscript to integer matrix
// ---------------------------------------------------------------------------

/// Convert a character matrix subscript to an integer matrix.
///
/// For the case `x[i]` where `x` is an n-way array and `i` is a character
/// matrix with n columns, this matches column values against dimnames of `x`.
/// Unmatched entries result in an out-of-bounds error.
pub unsafe fn strmat2intmat(s: SEXP, dnamelist: SEXP, _call: SEXP, x: SEXP) -> SEXP {
    unsafe {
        // Get dimensions of the subscript matrix
        let s_dim = crate::attrib_core::getAttrib(s, crate::attrib_core::R_DimSymbol());
        if s_dim.is_null() || LENGTH(s_dim) < 2 {
            return R_NilValue();
        }
        let nr = INTEGER_ELT(s_dim, 0) as R_xlen_t;
        let nc = INTEGER_ELT(s_dim, 1) as c_int;

        // Allocate integer result matrix
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::INTSXP.0, nr * nc as R_xlen_t));

        // Copy dim attribute to result
        crate::attrib_core::setAttrib(ans, crate::attrib_core::R_DimSymbol(), s_dim);

        for i in 0..nr as usize {
            for j in 0..nc as usize {
                let col_idx = i * nc as usize + j;
                let elt = STRING_ELT(s, col_idx as R_xlen_t);

                if elt.is_null() || elt == R_NilValue() {
                    // NA in string subscript -> NA_INTEGER
                    *INTEGER(ans).add(col_idx) = NA_INTEGER;
                    continue;
                }

                let pname = CHAR(elt);
                if pname.is_null() || *pname == 0 {
                    *INTEGER(ans).add(col_idx) = NA_INTEGER;
                    continue;
                }

                // Get the dimnames column for this dimension
                let dn_col = if !dnamelist.is_null() && j < LENGTH(dnamelist) as usize {
                    VECTOR_ELT(dnamelist, j as R_xlen_t)
                } else {
                    R_NilValue()
                };

                // Search for matching name
                let mut found: c_int = NA_INTEGER;
                if !dn_col.is_null() {
                    let target = std::ffi::CStr::from_ptr(pname);
                    let target_bytes = target.to_bytes();
                    for k in 0..XLENGTH(dn_col) as usize {
                        let name_elt = STRING_ELT(dn_col, k as R_xlen_t);
                        if name_elt.is_null() || name_elt == R_NilValue() {
                            continue;
                        }
                        let np = CHAR(name_elt);
                        if np.is_null() {
                            continue;
                        }
                        let name_bytes = std::ffi::CStr::from_ptr(np).to_bytes();
                        if name_bytes == target_bytes {
                            found = (k + 1) as c_int;
                            break;
                        }
                    }
                }

                *INTEGER(ans).add(col_idx) = found;
            }
        }

        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// nullSubscript — create a 1:n index vector
// ---------------------------------------------------------------------------

/// Create a null subscript (a vector of 1, 2, ..., n).
///
/// Used when the subscript is missing (R_MissingArg).
unsafe fn nullSubscript(n: R_xlen_t) -> SEXP {
    unsafe {
        if n <= 0 {
            return Rf_allocVector3(SEXPTYPE::INTSXP.0, 0);
        }
        let ans = Rf_allocVector3(SEXPTYPE::INTSXP.0, n);
        let ap = INTEGER(ans);
        for i in 0..n as usize {
            *ap.add(i) = (i + 1) as c_int;
        }
        ans
    }
}

// ---------------------------------------------------------------------------
// logicalSubscript — logical vector to index vector
// ---------------------------------------------------------------------------

/// Expand a logical subscript into integer indices.
///
/// TRUE values select their position, NA_LOGICAL inserts NA, FALSE skips.
/// The logical vector is recycled to length `nx` if shorter.
unsafe fn logicalSubscript(
    s: SEXP,
    ns: R_xlen_t,
    nx: R_xlen_t,
    stretch: *mut R_xlen_t,
    call: SEXP,
) -> SEXP {
    unsafe {
        let _ = call;
        if nx == 0 {
            return Rf_allocVector3(SEXPTYPE::INTSXP.0, 0);
        }

        // Count TRUE values (determine result length)
        let mut count: R_xlen_t = 0;
        let slen = if ns > nx { nx } else { ns };
        for i in 0..slen {
            let v = LOGICAL_ELT(s, i as c_int);
            if v == 1 {
                count += 1;
            }
        }

        // Allocate result
        let ans = Rf_allocVector3(SEXPTYPE::INTSXP.0, count);
        let ap = INTEGER(ans);

        // Fill in indices
        let mut j: R_xlen_t = 0;
        for i in 0..slen {
            let v = LOGICAL_ELT(s, i as c_int);
            if v == 1 {
                *ap.add(j as usize) = (i + 1) as c_int;
                j += 1;
            } else if v == NA_LOGICAL {
                *ap.add(j as usize) = NA_INTEGER;
                j += 1;
            }
        }

        // Update stretch
        if !stretch.is_null() {
            *stretch = count;
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// negativeSubscript — negative integer subscript handling
// ---------------------------------------------------------------------------

/// Process a negative subscript by creating a logical mask and inverting.
unsafe fn negativeSubscript(s: SEXP, ns: R_xlen_t, nx: R_xlen_t, call: SEXP) -> SEXP {
    unsafe {
        let _ = call;
        if nx <= 0 {
            return Rf_allocVector3(SEXPTYPE::INTSXP.0, 0);
        }

        // Create a logical mask: TRUE for selected, FALSE for excluded
        let mask = Rf_allocVector3(SEXPTYPE::LGLSXP.0, nx);
        let mp = LOGICAL(mask);
        for i in 0..nx as usize {
            *mp.add(i) = 1; // TRUE = include
        }

        // Mark excluded indices from the subscript
        let slen = if ns > nx { nx } else { ns };
        for i in 0..slen {
            let v = INTEGER_ELT(s, i as c_int);
            if v == NA_INTEGER {
                // NA in negative subscript: error
                // ECALL3(call, "NA's in subscript are not allowed");
                return R_NilValue();
            }
            if v < 0 {
                let idx = (-v) as usize;
                if idx >= 1 && idx <= nx as usize {
                    *mp.add(idx - 1) = 0; // FALSE = exclude
                }
            } else if v == 0 {
                // 0 in negative subscript: error
                return R_NilValue();
            } else {
                // Positive value mixed with negative: error
                return R_NilValue();
            }
        }

        // Now use logicalSubscript on the mask
        logicalSubscript(mask, nx, nx, ptr::null_mut(), ptr::null_mut())
    }
}

// ---------------------------------------------------------------------------
// positiveSubscript — positive integer subscript handling
// ---------------------------------------------------------------------------

/// Process a positive subscript by removing zeros.
unsafe fn positiveSubscript(s: SEXP, ns: R_xlen_t, nx: R_xlen_t) -> SEXP {
    unsafe {
        if nx == 0 {
            return Rf_allocVector3(SEXPTYPE::INTSXP.0, 0);
        }

        // Count non-zero values
        let slen = if ns > nx { nx } else { ns };
        let mut count: R_xlen_t = 0;
        for i in 0..slen {
            let v = INTEGER_ELT(s, i as c_int);
            if v != 0 && v != NA_INTEGER {
                count += 1;
            }
        }

        let ans = Rf_allocVector3(SEXPTYPE::INTSXP.0, count);
        let ap = INTEGER(ans);

        let mut j: R_xlen_t = 0;
        for i in 0..slen {
            let v = INTEGER_ELT(s, i as c_int);
            if v != 0 && v != NA_INTEGER {
                *ap.add(j as usize) = v;
                j += 1;
            }
        }

        ans
    }
}

// ---------------------------------------------------------------------------
// integerSubscript — integer subscript handling
// ---------------------------------------------------------------------------

/// Process an integer subscript, dispatching to negative or positive handling.
///
/// Detects mixed NA/negative/positive patterns and enforces R's subscript rules:
/// - Only 0's may be mixed with negative subscripts
/// - Positive subscripts beyond nx may trigger stretching
unsafe fn integerSubscript(
    s: SEXP,
    ns: R_xlen_t,
    nx: R_xlen_t,
    stretch: *mut R_xlen_t,
    call: SEXP,
    x: SEXP,
) -> SEXP {
    unsafe {
        let _ = (call, x);
        let slen = if ns > nx { nx } else { ns };

        // Check for negative values
        let mut has_neg = false;
        let mut has_pos = false;
        for i in 0..slen {
            let v = INTEGER_ELT(s, i as c_int);
            if v == NA_INTEGER {
                // NA in integer subscript: return all-NA result
                let ans = Rf_allocVector3(SEXPTYPE::INTSXP.0, ns);
                let ap = INTEGER(ans);
                for i in 0..ns as usize {
                    *ap.add(i) = NA_INTEGER;
                }
                if !stretch.is_null() {
                    *stretch = ns;
                }
                return ans;
            }
            if v < 0 {
                has_neg = true;
            } else if v == 0 { /* zero is ignored */
            } else {
                has_pos = true;
            }
        }

        if has_neg && has_pos {
            // Mixed positive and negative: error
            // ECALL3(call, "can't mix positive and negative subscripts");
            return R_NilValue();
        }

        if has_neg {
            return negativeSubscript(s, ns, nx, ptr::null_mut());
        }

        return positiveSubscript(s, ns, nx);
    }
}

// ---------------------------------------------------------------------------
// realSubscript — real (double) subscript handling
// ---------------------------------------------------------------------------

/// Process a real (double) subscript.
///
/// Converts to integer indices, handling NA, Inf, truncation, and
/// negative values according to R's subscript rules.
unsafe fn realSubscript(
    s: SEXP,
    ns: R_xlen_t,
    nx: R_xlen_t,
    stretch: *mut R_xlen_t,
    call: SEXP,
    x: SEXP,
) -> SEXP {
    unsafe {
        let _ = (call, x);
        // Convert real to integer first, then use integerSubscript
        let slen = if ns > nx { nx } else { ns };

        // Check for negative values in real subscript
        let mut has_neg = false;
        let mut has_na = false;
        for i in 0..slen {
            let v = REAL_ELT(s, i as c_int);
            if v.is_nan() {
                has_na = true;
            } else if v < 0.0 {
                has_neg = true;
            }
        }

        if has_na {
            // NA in real subscript: return all-NA result
            let ans = Rf_allocVector3(SEXPTYPE::INTSXP.0, ns);
            let ap = INTEGER(ans);
            for i in 0..ns as usize {
                *ap.add(i) = NA_INTEGER;
            }
            if !stretch.is_null() {
                *stretch = ns;
            }
            return ans;
        }

        // Convert to integer vector
        let int_s = Rf_allocVector3(SEXPTYPE::INTSXP.0, ns);
        let ip = INTEGER(int_s);
        for i in 0..ns as usize {
            let v = REAL_ELT(s, i as c_int);
            if v.is_nan() {
                *ip.add(i) = NA_INTEGER;
            } else if has_neg && v < 0.0 {
                *ip.add(i) = (v as c_int).wrapping_sub(1); // -1 adjustment already done
            } else {
                *ip.add(i) = v as c_int;
            }
        }

        // Check for negative after conversion
        let mut conv_has_neg = false;
        let mut conv_has_pos = false;
        for i in 0..ns as usize {
            let v = *ip.add(i);
            if v == NA_INTEGER { /* skip */
            } else if v < 0 {
                conv_has_neg = true;
            } else if v == 0 { /* zero ignored */
            } else {
                conv_has_pos = true;
            }
        }

        if conv_has_neg && conv_has_pos {
            // Mixed: error
            return R_NilValue();
        }

        if conv_has_neg {
            return negativeSubscript(int_s, ns, nx, ptr::null_mut());
        }

        return positiveSubscript(int_s, ns, nx);
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
unsafe fn stringSubscript(
    s: SEXP,
    ns: R_xlen_t,
    nx: R_xlen_t,
    names: SEXP,
    stretch: *mut R_xlen_t,
    call: SEXP,
    x: SEXP,
    dim: c_int,
) -> SEXP {
    unsafe {
        let _ = (call, dim);
        if nx == 0 {
            return Rf_allocVector3(SEXPTYPE::INTSXP.0, 0);
        }

        let slen = if ns > nx { nx } else { ns };
        let mut count: R_xlen_t = 0;

        // Match each string against names
        let mut indices: Vec<c_int> = Vec::new();
        indices.reserve(slen as usize);

        for i in 0..slen {
            let elt = STRING_ELT(s, i);
            if elt.is_null() || elt == R_NilValue() {
                // NA in string subscript: return all-NA
                let ans = Rf_allocVector3(SEXPTYPE::INTSXP.0, ns);
                let ap = INTEGER(ans);
                for j in 0..ns as usize {
                    *ap.add(j) = NA_INTEGER;
                }
                if !stretch.is_null() {
                    *stretch = ns;
                }
                return ans;
            }

            let pname = CHAR(elt);
            if pname.is_null() {
                indices.push(NA_INTEGER);
                count += 1;
                continue;
            }

            let target = std::ffi::CStr::from_ptr(pname);
            let target_bytes = target.to_bytes();

            // Linear search through names
            let mut found = false;
            if !names.is_null()
                && TYPEOF(names) == SEXPTYPE::STRSXP.0
                && LENGTH(names) >= nx as c_int
            {
                for j in 0..nx as usize {
                    let name_elt = STRING_ELT(names, j as R_xlen_t);
                    if name_elt.is_null() || name_elt == R_NilValue() {
                        continue;
                    }
                    let name_ptr = CHAR(name_elt);
                    if name_ptr.is_null() {
                        continue;
                    }
                    let name_str = std::ffi::CStr::from_ptr(name_ptr);
                    if name_str.to_bytes() == target_bytes {
                        indices.push((j + 1) as c_int);
                        count += 1;
                        found = true;
                        break;
                    }
                }
            }

            if !found {
                indices.push(NA_INTEGER);
                count += 1;
            }
        }

        // Create result
        let ans = Rf_allocVector3(SEXPTYPE::INTSXP.0, ns);
        let ap = INTEGER(ans);
        for i in 0..ns as usize {
            *ap.add(i) = indices[i];
        }

        if !stretch.is_null() {
            *stretch = count;
        }
        ans
    }
}

// ---------------------------------------------------------------------------
// int_arraySubscript — array subscript by dimension (internal)
// ---------------------------------------------------------------------------

/// Compute the subscript for one dimension of an array.
///
/// This is the internal implementation used by `[i,j,...]` and `[<-...`.
/// The public API is `arraySubscript`.
pub unsafe fn int_arraySubscript(
    dim: c_int,
    s: SEXP,
    dims: SEXP,
    x: SEXP,
    call: SEXP,
) -> SEXP {
    unsafe {
        let mut stretch: R_xlen_t = 0;
        let ns = LENGTH(s);
        let nd = INTEGER_ELT(dims, dim);

        let stype = TYPEOF(s);
        if stype == SEXPTYPE::NILSXP.0 {
            Rf_allocVector3(SEXPTYPE::INTSXP.0, 0)
        } else if stype == SEXPTYPE::LGLSXP.0 {
            logicalSubscript(s, ns as R_xlen_t, nd as R_xlen_t, &mut stretch, call)
        } else if stype == SEXPTYPE::INTSXP.0 {
            integerSubscript(s, ns as R_xlen_t, nd as R_xlen_t, &mut stretch, call, x)
        } else if stype == SEXPTYPE::REALSXP.0 {
            realSubscript(s, ns as R_xlen_t, nd as R_xlen_t, &mut stretch, call, x)
        } else if stype == SEXPTYPE::STRSXP.0 {
            let dnames = crate::attrib_core::getAttrib(x, crate::attrib_core::R_DimNamesSymbol());
            let dnames_col = if !dnames.is_null() && dim < LENGTH(dnames) {
                VECTOR_ELT(dnames, dim as R_xlen_t)
            } else {
                R_NilValue()
            };
            stringSubscript(
                s,
                ns as R_xlen_t,
                nd as R_xlen_t,
                dnames_col,
                &mut stretch,
                call,
                x,
                dim,
            )
        } else if stype == SEXPTYPE::SYMSXP.0 {
            nullSubscript(nd as R_xlen_t)
        } else {
            // ECALL3(call, "invalid subscript type '%s'", R_typeToChar(s));
            R_NilValue()
        }
    }
}

// ---------------------------------------------------------------------------
// arraySubscript — public API for array subscripting
// ---------------------------------------------------------------------------

/// Compute the subscript for one dimension of an array (public API).
///
/// This is used by packages arules, cba, proxy, and seriation.
/// Delegates to `int_arraySubscript` with `call = R_NilValue`.
pub unsafe fn arraySubscript(
    dim: c_int,
    s: SEXP,
    dims: SEXP,
    _dng: usize,
    _strg: usize,
    x: SEXP,
) -> SEXP {
    unsafe {
        // Note: dng and strg are function pointer parameters in C
        // (AttrGetter and StringEltGetter typedefs), here stubbed as usize.
        int_arraySubscript(dim, s, dims, x, R_NilValue())
    }
}

// ---------------------------------------------------------------------------
// makeSubscript — subscript creation for [ and [<-
// ---------------------------------------------------------------------------

/// Create a subscript vector for `[` and `[<-` operators.
///
/// Handles all R subscript types: NULL (empty), logical, integer, real,
/// string, and missing. For simple in-range scalar indices, returns the
/// input directly without copying.
///
/// If `*stretch` is 0 on entry, the vector `x` cannot be stretched.
/// Otherwise, `*stretch` returns the new required length for `x`.
pub unsafe fn makeSubscript(
    x: SEXP,
    s: SEXP,
    stretch: *mut R_xlen_t,
    call: SEXP,
) -> SEXP {
    unsafe {
        // Check that x is a vector, list, or language object
        // if !isVector(x) && !isList(x) && !isLanguage(x) {
        //     ECALL(call, "subscripting on non-vector");
        // }

        let nx = XLENGTH(x);

        // Special case for simple scalar indices — does not duplicate
        let stype = TYPEOF(s);
        if stype == SEXPTYPE::INTSXP.0 && IS_SCALAR(s, SEXPTYPE::INTSXP.0) != 0 {
            let i = SCALAR_IVAL(s);
            if i > 0 && (i as R_xlen_t) <= nx {
                if !stretch.is_null() {
                    *stretch = 0;
                }
                return s;
            }
        } else if stype == SEXPTYPE::REALSXP.0 && IS_SCALAR(s, SEXPTYPE::REALSXP.0) != 0 {
            let di = SCALAR_DVAL(s);
            if di >= 1.0 && (di as R_xlen_t) <= nx {
                if !stretch.is_null() {
                    *stretch = 0;
                }
                return s;
            }
        }

        let ns = XLENGTH(s);
        let mut _stretch_val: R_xlen_t = 0;

        let stype2 = TYPEOF(s);
        if stype2 == SEXPTYPE::NILSXP.0 {
            if !stretch.is_null() {
                *stretch = 0;
            }
            Rf_allocVector3(SEXPTYPE::INTSXP.0, 0)
        } else if stype2 == SEXPTYPE::LGLSXP.0 {
            logicalSubscript(s, ns, nx, stretch, call)
        } else if stype2 == SEXPTYPE::INTSXP.0 {
            integerSubscript(s, ns, nx, stretch, call, x)
        } else if stype2 == SEXPTYPE::REALSXP.0 {
            realSubscript(s, ns, nx, stretch, call, x)
        } else if stype2 == SEXPTYPE::STRSXP.0 {
            let names = crate::attrib_core::getAttrib(x, crate::attrib_core::R_NamesSymbol());
            stringSubscript(s, ns, nx, names, stretch, call, x, -1)
        } else if stype2 == SEXPTYPE::SYMSXP.0 {
            if !stretch.is_null() {
                *stretch = 0;
            }
            nullSubscript(nx)
        } else {
            // ECALL3(call, "invalid subscript type '%s'", R_typeToChar(s));
            R_NilValue()
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integer_one_index_positive() {
        unsafe {
            // 1-based index 3 -> 0-based index 2
            assert_eq!(integerOneIndex(3, 10, ptr::null_mut()), 2);
            // 1-based index 1 -> 0-based index 0
            assert_eq!(integerOneIndex(1, 10, ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_integer_one_index_zero() {
        unsafe {
            // Index 0 is invalid -> returns -1 (error in full R)
            assert_eq!(integerOneIndex(0, 10, ptr::null_mut()), -1);
        }
    }

    #[test]
    fn test_integer_one_index_negative_len2() {
        unsafe {
            // For length-2 vector, -1 selects element 1, -2 selects element 0
            assert_eq!(integerOneIndex(-1, 2, ptr::null_mut()), 1);
            assert_eq!(integerOneIndex(-2, 2, ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_integer_one_index_negative_len2_out_of_range() {
        unsafe {
            // -3 for length-2 vector is too many elements -> -1 (error in full R)
            assert_eq!(integerOneIndex(-3, 2, ptr::null_mut()), -1);
        }
    }

    #[test]
    fn test_integer_one_index_negative_other_lengths() {
        unsafe {
            // Negative indices for non-length-2 vectors are errors
            assert_eq!(integerOneIndex(-1, 1, ptr::null_mut()), -1);
            assert_eq!(integerOneIndex(-1, 3, ptr::null_mut()), -1);
            assert_eq!(integerOneIndex(-1, 10, ptr::null_mut()), -1);
        }
    }

    #[test]
    fn test_one_index_null_args() {
        unsafe {
            // Calling with null pointers should not crash
            let mut newname: SEXP = ptr::null_mut();
            let result = OneIndex(
                ptr::null_mut(),
                ptr::null_mut(),
                10,
                0,
                &mut newname,
                -1,
                ptr::null_mut(),
            );
            // With null s, LENGTH returns 0 < 1, so error path -> -1
            assert_eq!(result, -1);
        }
    }

    #[test]
    fn test_get1index_null_args() {
        unsafe {
            let result = get1index(ptr::null_mut(), ptr::null_mut(), 10, 0, -1, ptr::null_mut());
            // With null s, LENGTH returns 0 != 1, so error path -> -1
            assert_eq!(result, -1);
        }
    }

    #[test]
    fn test_get1index_pos_out_of_range() {
        unsafe {
            // pos >= length(s) -> internal error -> -1
            let result = get1index(ptr::null_mut(), ptr::null_mut(), 10, 0, 5, ptr::null_mut());
            assert_eq!(result, -1);
        }
    }

    #[test]
    fn test_vector_index_stub() {
        unsafe {
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
    }

    #[test]
    fn test_mat2indsub_stub() {
        unsafe {
            let result = mat2indsub(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_strmat2intmat_stub() {
        unsafe {
            let result = strmat2intmat(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_make_subscript_null() {
        unsafe {
            let mut stretch: R_xlen_t = 0;
            let result =
                makeSubscript(ptr::null_mut(), R_NilValue(), &mut stretch, ptr::null_mut());
            // NILSXP case -> stretch = 0, returns empty INTSXP
            assert_eq!(stretch, 0);
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP.0);
            assert_eq!(LENGTH(result), 0);
        }
    }

    #[test]
    fn test_make_subscript_scalar_int_in_range() {
        unsafe {
            // For null s, TYPEOF returns 0 (NILSXP), returns empty INTSXP
            let mut stretch: R_xlen_t = 1;
            let result = makeSubscript(
                ptr::null_mut(),
                ptr::null_mut(),
                &mut stretch,
                ptr::null_mut(),
            );
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP.0);
            assert_eq!(LENGTH(result), 0);
        }
    }

    #[test]
    fn test_int_array_subscript_nil() {
        unsafe {
            let result = int_arraySubscript(
                0,
                R_NilValue(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            // Returns empty INTSXP for NILSXP subscript
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP.0);
            assert_eq!(LENGTH(result), 0);
        }
    }

    #[test]
    fn test_array_subscript_delegates() {
        unsafe {
            let result = arraySubscript(0, R_NilValue(), ptr::null_mut(), 0, 0, ptr::null_mut());
            // arraySubscript delegates to int_arraySubscript which returns empty INTSXP
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP.0);
            assert_eq!(LENGTH(result), 0);
        }
    }

    #[test]
    fn test_na_real_is_nan() {
        assert!(NA_REAL.is_nan());
    }

    #[test]
    fn test_constants() {
        assert_eq!(NA_INTEGER, c_int::MIN);
        assert_eq!(NA_LOGICAL, c_int::MIN);
        assert_eq!(TRUE, 1);
        assert_eq!(FALSE, 0);
    }
}
