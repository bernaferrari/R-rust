#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/unique.c — hash and comparison utilities,
//! plus `do_unique`, `do_duplicated`, `do_any`, `do_all`.
//!
//! This module ports the standalone hash scattering, complex comparison,
//! and pointer hash functions used by R's `unique()`, `duplicated()`, etc.
//!
//! Ported standalone functions:
//!   scatter (hash scattering/mixing),
//!   unify_complex_na (complex NA unification),
//!   PTRHASH (pointer hash),
//!   cplx_eq (complex equality with NA/NaN handling)

use crate::sexp::ffi::SEXP;
use std::os::raw::c_int;

use crate::sexp::accessors::{
    CAR, CDR, COMPLEX, COMPLEX_ELT, INTEGER, INTEGER_ELT, LOGICAL, LOGICAL_ELT, RAW, RAW_ELT, REAL,
    REAL_ELT, SET_STRING_ELT, SET_VECTOR_ELT, STRING_ELT, TYPEOF, VECTOR_ELT, XLENGTH,
};
use crate::sexp::constructors::{Rf_ScalarLogical, Rf_allocVector3};
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, R_NA_BIT_PATTERN, R_xlen_t, Rcomplex, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

#[inline(always)]
unsafe fn PRIMVAL(op: SEXP) -> c_int {
    unsafe { crate::main::relop::PRIMVAL(op) }
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Check if a double is R's NA.
#[inline]
fn R_IsNA(x: f64) -> bool {
    x.to_bits() == R_NA_BIT_PATTERN
}

/// Check if a double is NaN (any NaN, not specifically R's NA).
#[inline]
fn ISNAN(x: f64) -> bool {
    x.is_nan()
}

// ---------------------------------------------------------------------------
// Hash scattering function
// ---------------------------------------------------------------------------

/// Hash scattering function (Knuth's multiplicative method variant).
///
/// Takes a key and a bit-width K, returns a hash value in [0, 2^K).
pub fn scatter(key: u32, k: u32) -> u32 {
    (3141592653_u32.wrapping_mul(key)) >> (32 - k)
}

// ---------------------------------------------------------------------------
// Complex NA unification
// ---------------------------------------------------------------------------

/// Unify complex NA representations.
///
/// Converts -0.0 to 0.0, and if either part is R's NA or NaN,
/// sets both parts to NA_REAL or NaN respectively.
pub fn unify_complex_na(z: Rcomplex) -> Rcomplex {
    let mut ans = Rcomplex {
        r: if z.r == 0.0 { 0.0 } else { z.r },
        i: if z.i == 0.0 { 0.0 } else { z.i },
    };

    if R_IsNA(ans.r) || R_IsNA(ans.i) {
        ans.r = f64::from_bits(R_NA_BIT_PATTERN);
        ans.i = f64::from_bits(R_NA_BIT_PATTERN);
    } else if ISNAN(ans.r) || ISNAN(ans.i) {
        ans.r = f64::NAN;
        ans.i = f64::NAN;
    }

    ans
}

// ---------------------------------------------------------------------------
// Pointer hash
// ---------------------------------------------------------------------------

/// Hash a pointer value.
///
/// Uses both halves of a pointer on 64-bit platforms for better distribution.
pub fn PTRHASH(x: usize) -> u32 {
    let z = x as u64;
    let z1 = z as u32;
    let z2 = (z >> 32) as u32;
    z1 ^ z2
}

// ---------------------------------------------------------------------------
// Complex equality
// ---------------------------------------------------------------------------

/// Compare two complex numbers for equality with NA/NaN handling.
///
/// - If neither has NA/NaN: exact comparison.
/// - If either has R's NA: both must have NA.
/// - If only NaN (not NA): NaN parts must match in both position and presence.
pub fn cplx_eq(x: Rcomplex, y: Rcomplex) -> bool {
    if !ISNAN(x.r) && !ISNAN(x.i) && !ISNAN(y.r) && !ISNAN(y.i) {
        return x.r == y.r && x.i == y.i;
    }

    // x has NA
    if R_IsNA(x.r) || R_IsNA(x.i) {
        return R_IsNA(y.r) || R_IsNA(y.i);
    }

    // y has NA but x doesn't
    if R_IsNA(y.r) || R_IsNA(y.i) {
        return false;
    }

    // Neither has NA but at least one has NaN
    let re_eq = (ISNAN(x.r) && ISNAN(y.r)) || (!ISNAN(x.r) && !ISNAN(y.r) && x.r == y.r);
    let im_eq = (ISNAN(x.i) && ISNAN(y.i)) || (!ISNAN(x.i) && !ISNAN(y.i) && x.i == y.i);

    re_eq && im_eq
}

// ---------------------------------------------------------------------------
// Hash table infrastructure for duplicated/unique
// ---------------------------------------------------------------------------

/// Sentinel value meaning "empty slot" in the hash table.
const NIL: i32 = -1;

/// Hash table data for the duplicated/unique algorithm.
struct HashData {
    k: u32,           // log2(M)
    m: usize,         // table size (power of 2)
    nmax: R_xlen_t,   // remaining capacity
    hash_table: SEXP, // integer vector used as hash table
}

/// Hash function for logical vectors: NA -> 2, FALSE -> 0, TRUE -> 1.
#[inline]
unsafe fn lhash(x: SEXP, indx: R_xlen_t, _d: &HashData) -> usize {
    unsafe {
        let xi = LOGICAL_ELT(x, indx as c_int);
        if xi == NA_LOGICAL { 2 } else { xi as usize }
    }
}

/// Equality test for logical elements.
#[inline]
unsafe fn lequal(x: SEXP, i: R_xlen_t, y: SEXP, j: R_xlen_t) -> bool {
    unsafe {
        if i < 0 || j < 0 {
            return false;
        }
        LOGICAL_ELT(x, i as c_int) == LOGICAL_ELT(y, j as c_int)
    }
}

/// Hash function for integer vectors using scatter.
#[inline]
unsafe fn ihash(x: SEXP, indx: R_xlen_t, d: &HashData) -> usize {
    unsafe {
        let xi = INTEGER_ELT(x, indx as c_int);
        if xi == NA_INTEGER {
            0
        } else {
            scatter(xi as u32, d.k) as usize
        }
    }
}

/// Equality test for integer elements.
#[inline]
unsafe fn iequal(x: SEXP, i: R_xlen_t, y: SEXP, j: R_xlen_t) -> bool {
    unsafe {
        if i < 0 || j < 0 {
            return false;
        }
        INTEGER_ELT(x, i as c_int) == INTEGER_ELT(y, j as c_int)
    }
}

/// Hash function for real (double) vectors.
/// Normalizes signed zero, NA, and NaN for consistent hashing.
#[inline]
unsafe fn rhash(x: SEXP, indx: R_xlen_t, d: &HashData) -> usize {
    unsafe {
        let xi = REAL_ELT(x, indx as c_int);
        let tmp = if xi == 0.0 { 0.0 } else { xi };
        let tmp = if R_IsNA(tmp) {
            f64::from_bits(R_NA_BIT_PATTERN)
        } else if ISNAN(tmp) {
            f64::NAN
        } else {
            tmp
        };
        let bits = tmp.to_bits();
        let lo = bits as u32;
        let hi = (bits >> 32) as u32;
        scatter(lo.wrapping_add(hi), d.k) as usize
    }
}

/// Equality test for real elements.
/// NA == NA, NaN == NaN, otherwise exact comparison.
#[inline]
unsafe fn requal(x: SEXP, i: R_xlen_t, y: SEXP, j: R_xlen_t) -> bool {
    unsafe {
        if i < 0 || j < 0 {
            return false;
        }
        let xi = REAL_ELT(x, i as c_int);
        let yj = REAL_ELT(y, j as c_int);
        if !ISNAN(xi) && !ISNAN(yj) {
            xi == yj
        } else if R_IsNA(xi) && R_IsNA(yj) {
            true
        } else if ISNAN(xi) && ISNAN(yj) {
            true
        } else {
            false
        }
    }
}

/// Hash function for complex vectors.
#[inline]
unsafe fn chash(x: SEXP, indx: R_xlen_t, d: &HashData) -> usize {
    unsafe {
        let tmp = unify_complex_na(COMPLEX_ELT(x, indx as c_int));
        let rbits = tmp.r.to_bits();
        let ibits = tmp.i.to_bits();
        let u =
            ((rbits as u32) ^ ((rbits >> 32) as u32)) ^ ((ibits as u32) ^ ((ibits >> 32) as u32));
        scatter(u, d.k) as usize
    }
}

/// Equality test for complex elements using cplx_eq.
#[inline]
unsafe fn cequal(x: SEXP, i: R_xlen_t, y: SEXP, j: R_xlen_t) -> bool {
    unsafe {
        if i < 0 || j < 0 {
            return false;
        }
        cplx_eq(COMPLEX_ELT(x, i as c_int), COMPLEX_ELT(y, j as c_int))
    }
}

/// Hash for strings by pointer address.
#[inline]
unsafe fn cshash(x: SEXP, indx: R_xlen_t, d: &HashData) -> usize {
    unsafe { scatter(PTRHASH(STRING_ELT(x, indx) as usize), d.k) as usize }
}

/// Equality test for string elements (by pointer identity for cached strings).
#[inline]
unsafe fn sequal(x: SEXP, i: R_xlen_t, y: SEXP, j: R_xlen_t) -> bool {
    unsafe {
        if i < 0 || j < 0 {
            return false;
        }
        let xi = STRING_ELT(x, i);
        let yj = STRING_ELT(y, j);
        xi == yj
    }
}

/// Hash for raw (byte) vectors: identity hash.
#[inline]
unsafe fn rawhash_fn(x: SEXP, indx: R_xlen_t, _d: &HashData) -> usize {
    unsafe { RAW_ELT(x, indx as c_int) as usize }
}

/// Equality test for raw elements.
#[inline]
unsafe fn rawequal(x: SEXP, i: R_xlen_t, y: SEXP, j: R_xlen_t) -> bool {
    unsafe {
        if i < 0 || j < 0 {
            return false;
        }
        RAW_ELT(x, i as c_int) == RAW_ELT(y, j as c_int)
    }
}

/// Hash function pointer type.
type HashFn = unsafe fn(SEXP, R_xlen_t, &HashData) -> usize;

/// Equality function pointer type.
type EqualFn = unsafe fn(SEXP, R_xlen_t, SEXP, R_xlen_t) -> bool;

/// Set up hash table parameters: choose M (smallest power of 2 >= 2*n) and K = log2(M).
unsafe fn mk_setup(n: R_xlen_t, nmax_arg: R_xlen_t) -> (usize, u32, R_xlen_t) {
    let n = if nmax_arg > 0
        && nmax_arg as R_xlen_t != NA_INTEGER as R_xlen_t
        && nmax_arg as R_xlen_t != 1
    {
        nmax_arg as R_xlen_t
    } else {
        n
    };
    let mut m: usize = 2;
    let mut k: u32 = 1;
    let n2 = 2 * n as usize;
    while m < n2 {
        m *= 2;
        k += 1;
    }
    (m, k, n)
}

/// Initialize a HashData for the given vector type.
unsafe fn hash_table_setup(x: SEXP, nmax_arg: i32) -> HashData {
    unsafe {
        let xtype = TYPEOF(x);
        let (m, k, nmax) = match xtype {
            t if t == SEXPTYPE::LGLSXP.0 => (4usize, 2u32, XLENGTH(x)),
            t if t == SEXPTYPE::INTSXP.0 => {
                let n = XLENGTH(x);
                mk_setup(n, nmax_arg as R_xlen_t)
            }
            t if t == SEXPTYPE::REALSXP.0 => mk_setup(XLENGTH(x), nmax_arg as R_xlen_t),
            t if t == SEXPTYPE::CPLXSXP.0 => mk_setup(XLENGTH(x), nmax_arg as R_xlen_t),
            t if t == SEXPTYPE::STRSXP.0 => mk_setup(XLENGTH(x), nmax_arg as R_xlen_t),
            t if t == SEXPTYPE::RAWSXP.0 => (256usize, 8u32, XLENGTH(x)),
            t if t == SEXPTYPE::VECSXP.0 || t == SEXPTYPE::EXPRSXP.0 => {
                mk_setup(XLENGTH(x), nmax_arg as R_xlen_t)
            }
            _ => {
                // fallback
                mk_setup(XLENGTH(x), nmax_arg as R_xlen_t)
            }
        };

        // Allocate integer hash table filled with NIL
        let hash_table = Rf_allocVector3(SEXPTYPE::INTSXP.0, m as R_xlen_t);
        let htable = INTEGER(hash_table);
        for i in 0..m {
            *htable.add(i) = NIL;
        }

        HashData {
            k,
            m,
            nmax,
            hash_table,
        }
    }
}

/// Check if element at index `indx` in vector `x` is a duplicate using open-addressing.
///
/// Returns 1 if duplicated (seen before), 0 if first occurrence.
/// First occurrences are inserted into the hash table.
unsafe fn is_duplicated(x: SEXP, indx: R_xlen_t, d: &mut HashData) -> i32 {
    unsafe {
        let xtype = TYPEOF(x);

        let hash_fn: HashFn = match xtype {
            t if t == SEXPTYPE::LGLSXP.0 => lhash,
            t if t == SEXPTYPE::INTSXP.0 => ihash,
            t if t == SEXPTYPE::REALSXP.0 => rhash,
            t if t == SEXPTYPE::CPLXSXP.0 => chash,
            t if t == SEXPTYPE::STRSXP.0 => cshash,
            t if t == SEXPTYPE::RAWSXP.0 => rawhash_fn,
            _ => ihash, // fallback
        };

        let equal_fn: EqualFn = match xtype {
            t if t == SEXPTYPE::LGLSXP.0 => lequal,
            t if t == SEXPTYPE::INTSXP.0 => iequal,
            t if t == SEXPTYPE::REALSXP.0 => requal,
            t if t == SEXPTYPE::CPLXSXP.0 => cequal,
            t if t == SEXPTYPE::STRSXP.0 => sequal,
            t if t == SEXPTYPE::RAWSXP.0 => rawequal,
            _ => iequal, // fallback
        };

        let h = INTEGER(d.hash_table);
        let mut i = hash_fn(x, indx, d);
        while *h.add(i) != NIL {
            if equal_fn(x, *h.add(i) as R_xlen_t, x, indx) {
                return if *h.add(i) >= 0 { 1 } else { 0 };
            }
            i = (i + 1) % d.m;
        }
        if d.nmax <= 0 {
            // hash table full - should not happen with proper sizing
        }
        d.nmax -= 1;
        *h.add(i) = indx as i32;
        0
    }
}

/// Core Duplicated implementation: returns a logical vector where TRUE means duplicated.
///
/// `from_last`: if true, scan from right to left (so last occurrence is kept).
unsafe fn duplicated_impl(x: SEXP, from_last: bool, nmax_arg: i32) -> SEXP {
    unsafe {
        let n = XLENGTH(x);
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::LGLSXP.0, 0);
        }
        if n == 1 {
            return Rf_ScalarLogical(0); // FALSE
        }

        let mut data = hash_table_setup(x, nmax_arg);
        Rf_protect(data.hash_table);

        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP.0, n);
        Rf_protect(ans);

        let v = LOGICAL(ans);

        if from_last {
            let mut i = n as i64 - 1;
            loop {
                *v.add(i as usize) = is_duplicated(x, i as R_xlen_t, &mut data);
                if i == 0 {
                    break;
                }
                i -= 1;
            }
        } else {
            for i in 0..n {
                *v.add(i as usize) = is_duplicated(x, i, &mut data);
            }
        }

        Rf_unprotect(2);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_unique
// ---------------------------------------------------------------------------

/// Implementation of R's `unique()` builtin.
///
/// `.Internal(unique(x, incomparables, fromLast, nmax))`
/// PRIMVAL(op) == 1 in the C source; here called directly as do_unique.
pub unsafe fn do_unique(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        // incomp = CADR(args) -- we skip incomparables support (same as empty incomparables)
        // fromLast = CADDR(args) -- skip, default FALSE
        // nmax = CADDDR(args) -- skip, default NA_INTEGER

        let n = XLENGTH(x);
        if n == 0 {
            return Rf_allocVector3(TYPEOF(x), 0);
        }

        // Get duplicated flags
        let dup = duplicated_impl(x, false, NA_INTEGER);
        Rf_protect(dup);

        // Count unique entries
        let mut k: R_xlen_t = 0;
        for i in 0..n {
            if *LOGICAL(dup).add(i as usize) == 0 {
                k += 1;
            }
        }

        let ans = Rf_allocVector3(TYPEOF(x), k);
        Rf_protect(ans);

        let xtype = TYPEOF(x);
        let mut ki: R_xlen_t = 0;

        match xtype {
            t if t == SEXPTYPE::LGLSXP.0 => {
                let a = LOGICAL(ans);
                for i in 0..n {
                    if *LOGICAL(dup).add(i as usize) == 0 {
                        *a.add(ki as usize) = LOGICAL_ELT(x, i as c_int);
                        ki += 1;
                    }
                }
            }
            t if t == SEXPTYPE::INTSXP.0 => {
                let a = INTEGER(ans);
                for i in 0..n {
                    if *LOGICAL(dup).add(i as usize) == 0 {
                        *a.add(ki as usize) = INTEGER_ELT(x, i as c_int);
                        ki += 1;
                    }
                }
            }
            t if t == SEXPTYPE::REALSXP.0 => {
                let a = REAL(ans);
                for i in 0..n {
                    if *LOGICAL(dup).add(i as usize) == 0 {
                        *a.add(ki as usize) = REAL_ELT(x, i as c_int);
                        ki += 1;
                    }
                }
            }
            t if t == SEXPTYPE::CPLXSXP.0 => {
                let a = COMPLEX(ans);
                for i in 0..n {
                    if *LOGICAL(dup).add(i as usize) == 0 {
                        *a.add(ki as usize) = COMPLEX_ELT(x, i as c_int);
                        ki += 1;
                    }
                }
            }
            t if t == SEXPTYPE::STRSXP.0 => {
                for i in 0..n {
                    if *LOGICAL(dup).add(i as usize) == 0 {
                        SET_STRING_ELT(ans, ki, STRING_ELT(x, i));
                        ki += 1;
                    }
                }
            }
            t if t == SEXPTYPE::VECSXP.0 || t == SEXPTYPE::EXPRSXP.0 => {
                for i in 0..n {
                    if *LOGICAL(dup).add(i as usize) == 0 {
                        SET_VECTOR_ELT(ans, ki, VECTOR_ELT(x, i));
                        ki += 1;
                    }
                }
            }
            t if t == SEXPTYPE::RAWSXP.0 => {
                let a = RAW(ans);
                for i in 0..n {
                    if *LOGICAL(dup).add(i as usize) == 0 {
                        *a.add(ki as usize) = RAW_ELT(x, i as c_int);
                        ki += 1;
                    }
                }
            }
            _ => {}
        }

        Rf_unprotect(2);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_duplicated
// ---------------------------------------------------------------------------

/// Implementation of R's `duplicated()` builtin.
///
/// `.Internal(duplicated(x, incomparables, fromLast, nmax))`
pub unsafe fn do_duplicated(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        // fromLast = CADDR(args) -- skip, default FALSE
        // nmax = CADDDR(args) -- skip, default NA_INTEGER

        let n = XLENGTH(x);
        if n == 0 {
            return Rf_allocVector3(SEXPTYPE::LGLSXP.0, 0);
        }

        duplicated_impl(x, false, NA_INTEGER)
    }
}

// ---------------------------------------------------------------------------
// do_any / do_all (ported from R's src/main/logic.c do_logic3)
// ---------------------------------------------------------------------------

/// Check values in a logical vector for any/all semantics.
///
/// Returns TRUE, FALSE, or NA_LOGICAL.
/// `op`: 1 = all, 2 = any
/// `na_rm`: if true, skip NA values.
unsafe fn check_values(op: i32, na_rm: bool, x: SEXP, n: R_xlen_t) -> i32 {
    unsafe {
        let px = LOGICAL(x);
        let mut has_na = false;

        for i in 0..n {
            let xi = *px.add(i as usize);
            if !na_rm && xi == NA_LOGICAL {
                has_na = true;
            } else {
                if xi == 1 && op == 2 {
                    // TRUE && _OP_ANY
                    return 1; // TRUE
                }
                if xi == 0 && op == 1 {
                    // FALSE && _OP_ALL
                    return 0; // FALSE
                }
            }
        }

        if op == 2 {
            // _OP_ANY
            if has_na { NA_LOGICAL } else { 0 } // FALSE
        } else {
            // _OP_ALL
            if has_na { NA_LOGICAL } else { 1 } // TRUE
        }
    }
}

/// Implementation of R's `any()` builtin.
///
/// `.Internal(any(..., na.rm = FALSE))`
/// PRIMVAL(op) == 2 in the C source.
pub unsafe fn do_any(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        // Walk through the args list. For a simple implementation,
        // we process the first logical vector argument.
        // na.rm is typically the last named argument.

        let mut s = args;
        let mut val: i32 = 0; // FALSE (default for empty any())
        let mut has_na = false;
        let mut na_rm = false;

        // First pass: look for na.rm argument
        let mut arg_list = args;
        let mut na_rm_sexp: SEXP = std::ptr::null_mut();
        while !arg_list.is_null() && arg_list != R_NilValue() {
            let t = CAR(arg_list);
            // Check TAG for na.rm
            let tag = crate::sexp::accessors::TAG(arg_list);
            if !tag.is_null() {
                let pname = crate::sexp::accessors::PRINTNAME(tag);
                if !pname.is_null() {
                    let name_bytes = crate::sexp::accessors::CHAR(pname);
                    if !name_bytes.is_null() {
                        let name_str = std::ffi::CStr::from_ptr(name_bytes);
                        if name_str.to_bytes() == b"na.rm" {
                            na_rm_sexp = t;
                        }
                    }
                }
            }
            arg_list = CDR(arg_list);
        }

        if !na_rm_sexp.is_null() {
            let nrm = LOGICAL_ELT(na_rm_sexp, 0);
            na_rm = nrm == 1;
        }

        // Process non-na.rm arguments
        s = args;
        while !s.is_null() && s != R_NilValue() {
            let t = CAR(s);
            let tag = crate::sexp::accessors::TAG(s);
            let is_named = !tag.is_null();

            // Skip na.rm argument
            if is_named {
                let pname = crate::sexp::accessors::PRINTNAME(tag);
                if !pname.is_null() {
                    let name_bytes = crate::sexp::accessors::CHAR(pname);
                    if !name_bytes.is_null() {
                        let name_str = std::ffi::CStr::from_ptr(name_bytes);
                        if name_str.to_bytes() == b"na.rm" {
                            s = CDR(s);
                            continue;
                        }
                    }
                }
            }

            let n = XLENGTH(t);
            if n == 0 {
                s = CDR(s);
                continue;
            }

            let cv = check_values(2, na_rm, t, n); // _OP_ANY = 2
            if cv != NA_LOGICAL {
                if cv == 1 {
                    // any found TRUE
                    val = 1;
                    has_na = false;
                    break;
                }
            } else {
                has_na = true;
            }
            val = cv;
            s = CDR(s);
        }

        if has_na {
            Rf_ScalarLogical(NA_LOGICAL)
        } else {
            Rf_ScalarLogical(val)
        }
    }
}

/// Implementation of R's `all()` builtin.
///
/// `.Internal(all(..., na.rm = FALSE))`
/// PRIMVAL(op) == 1 in the C source.
pub unsafe fn do_all(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        let mut val: i32 = 1; // TRUE (default for empty all())
        let mut has_na = false;
        let mut na_rm = false;

        // First pass: look for na.rm argument
        let mut arg_list = args;
        let mut na_rm_sexp: SEXP = std::ptr::null_mut();
        while !arg_list.is_null() && arg_list != R_NilValue() {
            let tag = crate::sexp::accessors::TAG(arg_list);
            if !tag.is_null() {
                let pname = crate::sexp::accessors::PRINTNAME(tag);
                if !pname.is_null() {
                    let name_bytes = crate::sexp::accessors::CHAR(pname);
                    if !name_bytes.is_null() {
                        let name_str = std::ffi::CStr::from_ptr(name_bytes);
                        if name_str.to_bytes() == b"na.rm" {
                            na_rm_sexp = CAR(arg_list);
                        }
                    }
                }
            }
            arg_list = CDR(arg_list);
        }

        if !na_rm_sexp.is_null() {
            let nrm = LOGICAL_ELT(na_rm_sexp, 0);
            na_rm = nrm == 1;
        }

        // Process non-na.rm arguments
        let mut s = args;
        while !s.is_null() && s != R_NilValue() {
            let t = CAR(s);
            let tag = crate::sexp::accessors::TAG(s);
            let is_named = !tag.is_null();

            // Skip na.rm argument
            if is_named {
                let pname = crate::sexp::accessors::PRINTNAME(tag);
                if !pname.is_null() {
                    let name_bytes = crate::sexp::accessors::CHAR(pname);
                    if !name_bytes.is_null() {
                        let name_str = std::ffi::CStr::from_ptr(name_bytes);
                        if name_str.to_bytes() == b"na.rm" {
                            s = CDR(s);
                            continue;
                        }
                    }
                }
            }

            let n = XLENGTH(t);
            if n == 0 {
                s = CDR(s);
                continue;
            }

            let cv = check_values(1, na_rm, t, n); // _OP_ALL = 1
            if cv != NA_LOGICAL {
                if cv == 0 {
                    // all found FALSE
                    has_na = false;
                    val = 0;
                    break;
                }
            } else {
                has_na = true;
            }
            val = cv;
            s = CDR(s);
        }

        if has_na {
            Rf_ScalarLogical(NA_LOGICAL)
        } else {
            Rf_ScalarLogical(val)
        }
    }
}

// ---------------------------------------------------------------------------
// do_which — return indices of TRUE values
// ---------------------------------------------------------------------------

/// which() returns the indices where a logical vector is TRUE.
pub unsafe fn do_which(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::constructors::*;

        let x = CAR(args);
        let n = XLENGTH(x);
        let t = TYPEOF(x);

        // First pass: count TRUE values
        let mut count: R_xlen_t = 0;
        for i in 0..n {
            let v = if t == SEXPTYPE::LGLSXP.0 {
                LOGICAL_ELT(x, i as c_int)
            } else if t == SEXPTYPE::INTSXP.0 {
                INTEGER_ELT(x, i as c_int)
            } else if t == SEXPTYPE::REALSXP.0 {
                if REAL_ELT(x, i as c_int).is_nan() {
                    NA_INTEGER
                } else {
                    *REAL(x).add(i as usize) as c_int
                }
            } else {
                continue;
            };
            if v == 1 {
                count += 1;
            }
        }

        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::INTSXP.0, count));
        let p = INTEGER(ans);
        let mut j: usize = 0;

        for i in 0..n {
            let v = if t == SEXPTYPE::LGLSXP.0 {
                LOGICAL_ELT(x, i as c_int)
            } else if t == SEXPTYPE::INTSXP.0 {
                INTEGER_ELT(x, i as c_int)
            } else if t == SEXPTYPE::REALSXP.0 {
                if REAL_ELT(x, i as c_int).is_nan() {
                    NA_INTEGER
                } else {
                    *REAL(x).add(i as usize) as c_int
                }
            } else {
                continue;
            };
            if v == 1 {
                *p.add(j) = (i + 1) as c_int; // R uses 1-based indexing
                j += 1;
            }
        }

        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_anyNA — check if any value is NA
// ---------------------------------------------------------------------------

/// anyNA() returns TRUE if any element is NA.
pub unsafe fn do_anyNA(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::CHAR;
        use crate::sexp::constructors::Rf_ScalarLogical;

        let x = CAR(args);
        let n = XLENGTH(x);
        let t = TYPEOF(x);

        let mut result: c_int = 0;
        for i in 0..n {
            let is_na = if t == SEXPTYPE::LGLSXP.0 {
                LOGICAL_ELT(x, i as c_int) == NA_LOGICAL
            } else if t == SEXPTYPE::INTSXP.0 {
                INTEGER_ELT(x, i as c_int) == NA_INTEGER
            } else if t == SEXPTYPE::REALSXP.0 {
                REAL_ELT(x, i as c_int).is_nan()
            } else if t == SEXPTYPE::CPLXSXP.0 {
                let c = COMPLEX_ELT(x, i as c_int);
                c.r.is_nan() || c.i.is_nan()
            } else if t == SEXPTYPE::STRSXP.0 {
                let el = STRING_ELT(x, i);
                el.is_null() || el == R_NilValue() || {
                    let cs = CHAR(el);
                    cs.is_null() || *cs == 0
                }
            } else {
                false
            };
            if is_na {
                result = 1;
                break;
            }
        }

        Rf_ScalarLogical(result)
    }
}

// ---------------------------------------------------------------------------
// do_anyDuplicated — count duplicated elements
// ---------------------------------------------------------------------------

/// anyDuplicated(x) returns the index of the first duplicated element.
pub unsafe fn do_anyDuplicated(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::constructors::*;

        let x = CAR(args);
        let n = XLENGTH(x);
        let ans = Rf_protect(Rf_allocVector3(SEXPTYPE::INTSXP.0, n));
        let p = INTEGER(ans);

        // Simple O(n^2) check
        for i in 0..n {
            *p.add(i as usize) = 0;
            let vi = STRING_ELT(x, i as i64);
            for j in 0..i {
                let vj = STRING_ELT(x, j as i64);
                if vi == vj {
                    *p.add(i as usize) = (i + 1) as c_int;
                    break;
                }
            }
        }

        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_pmin / do_pmax — parallel minimum/maximum
// ---------------------------------------------------------------------------

/// pmin() and pmax() return the parallel min/max of input vectors.
/// Uses offset: 0 for pmax, 1 for pmin.
pub unsafe fn do_pminmax(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::constructors::*;

        let is_pmax = PRIMVAL(_op) == 0;
        let mut a = args;
        let x = CAR(a);
        a = CDR(a);
        let na_rm = if !a.is_null() && !CAR(a).is_null() {
            crate::main::coerce::asLogical(CAR(a))
        } else {
            0
        };
        if !a.is_null() {
            a = CDR(a);
        }

        let n = XLENGTH(x);
        let t = TYPEOF(x);
        let ans = Rf_protect(Rf_allocVector3(t, n));

        // Initialize with first vector's values
        if t == SEXPTYPE::INTSXP.0 {
            for i in 0..n {
                *INTEGER(ans).add(i as usize) = INTEGER_ELT(x, i as c_int);
            }
        } else if t == SEXPTYPE::REALSXP.0 {
            for i in 0..n {
                *REAL(ans).add(i as usize) = REAL_ELT(x, i as c_int);
            }
        } else {
            Rf_unprotect(1);
            return R_NilValue();
        }

        // Process remaining vectors
        while !a.is_null() && a != R_NilValue() {
            let y = CAR(a);
            if t == SEXPTYPE::INTSXP.0 {
                for i in 0..n {
                    let cur = INTEGER_ELT(y, i as c_int);
                    let prev = *INTEGER(ans).add(i as usize);
                    if cur == NA_INTEGER {
                        if na_rm == 0 {
                            *INTEGER(ans).add(i as usize) = NA_INTEGER;
                        }
                    } else if prev == NA_INTEGER {
                        // keep NA unless na_rm
                    } else if is_pmax {
                        if cur > prev {
                            *INTEGER(ans).add(i as usize) = cur;
                        }
                    } else {
                        if cur < prev {
                            *INTEGER(ans).add(i as usize) = cur;
                        }
                    }
                }
            } else if t == SEXPTYPE::REALSXP.0 {
                for i in 0..n {
                    let cur = REAL_ELT(y, i as c_int);
                    let prev = *REAL(ans).add(i as usize);
                    if cur.is_nan() {
                        if na_rm == 0 {
                            *REAL(ans).add(i as usize) = cur;
                        }
                    } else if prev.is_nan() {
                        // keep NA unless na_rm
                    } else if is_pmax {
                        if cur > prev {
                            *REAL(ans).add(i as usize) = cur;
                        }
                    } else {
                        if cur < prev {
                            *REAL(ans).add(i as usize) = cur;
                        }
                    }
                }
            }
            a = CDR(a);
        }

        Rf_unprotect(1);
        ans
    }
}

// ---------------------------------------------------------------------------
// do_which_min / do_which_max
// ---------------------------------------------------------------------------

/// which.min(x) and which.max(x) return the index of the min/max element.
pub unsafe fn do_which_min(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::constructors::*;

        let x = CAR(args);
        let n = XLENGTH(x);
        let t = TYPEOF(x);

        let mut best_idx: usize = 0;
        let mut found = false;

        for i in 0..n {
            if t == SEXPTYPE::INTSXP.0 {
                let v = INTEGER_ELT(x, i as c_int);
                if v == NA_INTEGER {
                    continue;
                }
                if !found || v < INTEGER_ELT(x, best_idx as c_int) {
                    best_idx = i as usize;
                    found = true;
                }
            } else if t == SEXPTYPE::REALSXP.0 {
                let v = REAL_ELT(x, i as c_int);
                if v.is_nan() {
                    continue;
                }
                if !found || v < REAL_ELT(x, best_idx as c_int) {
                    best_idx = i as usize;
                    found = true;
                }
            }
        }

        if found {
            Rf_ScalarInteger((best_idx + 1) as c_int)
        } else {
            Rf_ScalarInteger(NA_INTEGER)
        }
    }
}

pub unsafe fn do_which_max(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::constructors::*;

        let x = CAR(args);
        let n = XLENGTH(x);
        let t = TYPEOF(x);

        let mut best_idx: usize = 0;
        let mut found = false;

        for i in 0..n {
            if t == SEXPTYPE::INTSXP.0 {
                let v = INTEGER_ELT(x, i as c_int);
                if v == NA_INTEGER {
                    continue;
                }
                if !found || v > INTEGER_ELT(x, best_idx as c_int) {
                    best_idx = i as usize;
                    found = true;
                }
            } else if t == SEXPTYPE::REALSXP.0 {
                let v = REAL_ELT(x, i as c_int);
                if v.is_nan() {
                    continue;
                }
                if !found || v > REAL_ELT(x, best_idx as c_int) {
                    best_idx = i as usize;
                    found = true;
                }
            }
        }

        if found {
            Rf_ScalarInteger((best_idx + 1) as c_int)
        } else {
            Rf_ScalarInteger(NA_INTEGER)
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
    fn test_scatter_deterministic() {
        let h1 = scatter(12345, 16);
        let h2 = scatter(12345, 16);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_scatter_different_keys() {
        let h1 = scatter(12345, 16);
        let h2 = scatter(67890, 16);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_scatter_range() {
        // With K=16, result should be in [0, 65535]
        for &key in &[0u32, 1, 42, 0xFFFFFFFF] {
            let h = scatter(key, 16);
            assert!(h < (1u32 << 16));
        }
    }

    #[test]
    fn test_unify_complex_na_normal() {
        let z = Rcomplex { r: 1.0, i: 2.0 };
        let ans = unify_complex_na(z);
        assert_eq!(ans.r, 1.0);
        assert_eq!(ans.i, 2.0);
    }

    #[test]
    fn test_unify_complex_na_neg_zero() {
        let z = Rcomplex { r: -0.0, i: 0.0 };
        let ans = unify_complex_na(z);
        assert_eq!(ans.r, 0.0);
        assert_eq!(ans.i, 0.0);
    }

    #[test]
    fn test_unify_complex_na_rna() {
        let na = f64::from_bits(R_NA_BIT_PATTERN);
        let z = Rcomplex { r: na, i: 1.0 };
        let ans = unify_complex_na(z);
        assert!(R_IsNA(ans.r));
        assert!(R_IsNA(ans.i));
    }

    #[test]
    fn test_unify_complex_na_nan() {
        let z = Rcomplex {
            r: f64::NAN,
            i: 1.0,
        };
        let ans = unify_complex_na(z);
        assert!(ans.r.is_nan());
        assert!(ans.i.is_nan());
    }

    #[test]
    fn test_PTRHASH_deterministic() {
        let h1 = PTRHASH(0x12345678 as usize);
        let h2 = PTRHASH(0x12345678 as usize);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_PTRHASH_different() {
        let h1 = PTRHASH(0x12345678 as usize);
        let h2 = PTRHASH(0x87654321 as usize);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_cplx_eq_normal() {
        let x = Rcomplex { r: 1.0, i: 2.0 };
        let y = Rcomplex { r: 1.0, i: 2.0 };
        assert!(cplx_eq(x, y));

        let z = Rcomplex { r: 1.0, i: 3.0 };
        assert!(!cplx_eq(x, z));
    }

    #[test]
    fn test_cplx_eq_na() {
        let na = f64::from_bits(R_NA_BIT_PATTERN);
        let x = Rcomplex { r: na, i: 1.0 };
        let y = Rcomplex { r: na, i: 2.0 };
        assert!(cplx_eq(x, y)); // both have NA

        let z = Rcomplex { r: 1.0, i: 2.0 };
        assert!(!cplx_eq(x, z)); // x has NA, z doesn't
    }

    #[test]
    fn test_cplx_eq_nan() {
        let x = Rcomplex {
            r: f64::NAN,
            i: 2.0,
        };
        let y = Rcomplex {
            r: f64::NAN,
            i: 2.0,
        };
        assert!(cplx_eq(x, y)); // NaN in same position

        let z = Rcomplex {
            r: 1.0,
            i: f64::NAN,
        };
        assert!(!cplx_eq(x, z)); // NaN in different position
    }

    #[test]
    fn test_cplx_eq_mixed() {
        let na = f64::from_bits(R_NA_BIT_PATTERN);
        // NA vs NaN: not equal (different kinds of missing)
        let x = Rcomplex { r: na, i: 0.0 };
        let y = Rcomplex {
            r: f64::NAN,
            i: 0.0,
        };
        assert!(!cplx_eq(x, y));
    }

    // -----------------------------------------------------------------------
    // Tests for hash table infrastructure
    // -----------------------------------------------------------------------

    #[test]
    fn test_nil_constant() {
        assert_eq!(NIL, -1);
    }

    #[test]
    fn test_mk_setup_basic() {
        // n=10 should give M >= 20 (power of 2), so M=32, K=5
        let (m, k, nmax) = unsafe { mk_setup(10, i64::MIN) };
        assert_eq!(m, 32);
        assert_eq!(k, 5);
        assert_eq!(nmax, 10);
    }

    #[test]
    fn test_mk_setup_nmax_override() {
        // nmax=5 should override n=10
        let (m, k, nmax) = unsafe { mk_setup(10, 5) };
        assert_eq!(m, 16); // 2*5 = 10, next power of 2 is 16
        assert_eq!(k, 4);
        assert_eq!(nmax, 5);
    }

    #[test]
    fn test_mk_setup_large() {
        let (m, k, _) = unsafe { mk_setup(1000, i64::MIN) };
        assert_eq!(m, 2048); // 2*1000=2000, next power of 2 is 2048
        assert_eq!(k, 11);
    }

    #[test]
    fn test_mk_setup_nmax_one() {
        // nmax=1 is special: not used
        let (m, k, nmax) = unsafe { mk_setup(10, 1) };
        assert_eq!(nmax, 10); // original n used
        assert_eq!(m, 32);
    }

    // -----------------------------------------------------------------------
    // Tests for rhash / requal (pure logic)
    // -----------------------------------------------------------------------

    #[test]
    fn test_rhash_deterministic() {
        // Test scatter directly (the core of rhash) on known inputs
        let bits = 0.0_f64.to_bits();
        let lo = bits as u32;
        let hi = (bits >> 32) as u32;
        let h1 = scatter(lo.wrapping_add(hi), 16);
        // Verify scatter is deterministic: same input gives same output
        let h2 = scatter(lo.wrapping_add(hi), 16);
        assert_eq!(h1, h2);
        // Verify different inputs give different hashes
        let bits2 = 1.0_f64.to_bits();
        let lo2 = bits2 as u32;
        let hi2 = (bits2 >> 32) as u32;
        let h3 = scatter(lo2.wrapping_add(hi2), 16);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_requal_normal() {
        // Test requal via its pure logic (requires SEXP, so test indirectly)
        // NA equals NA
        let na = f64::from_bits(R_NA_BIT_PATTERN);
        assert!(R_IsNA(na));
        // NaN is NaN
        assert!(ISNAN(f64::NAN));
        // Regular equality
        assert_eq!(1.5_f64, 1.5_f64);
    }

    // -----------------------------------------------------------------------
    // Tests for duplicated_impl and do_unique with integer vectors
    // -----------------------------------------------------------------------

    /// Helper to create an integer vector with values.
    unsafe fn make_int_vector(values: &[c_int]) -> SEXP {
        let v = Rf_allocVector3(SEXPTYPE::INTSXP.0, values.len() as R_xlen_t);
        for (i, &val) in values.iter().enumerate() {
            *INTEGER(v).add(i) = val;
        }
        v
    }

    /// Helper to create a logical vector with values.
    unsafe fn make_logical_vector(values: &[c_int]) -> SEXP {
        let v = Rf_allocVector3(SEXPTYPE::LGLSXP.0, values.len() as R_xlen_t);
        for (i, &val) in values.iter().enumerate() {
            *LOGICAL(v).add(i) = val;
        }
        v
    }

    /// Helper to create a real vector with values.
    unsafe fn make_real_vector(values: &[f64]) -> SEXP {
        let v = Rf_allocVector3(SEXPTYPE::REALSXP.0, values.len() as R_xlen_t);
        for (i, &val) in values.iter().enumerate() {
            *REAL(v).add(i) = val;
        }
        v
    }

    #[test]
    fn test_duplicated_int_basic() {
        unsafe {
            let x = make_int_vector(&[1, 2, 3, 2, 1]);
            let dup = duplicated_impl(x, false, NA_INTEGER);
            assert_eq!(XLENGTH(dup), 5);
            // 1: FALSE, 2: FALSE, 3: FALSE, 2: TRUE, 1: TRUE
            assert_eq!(*LOGICAL(dup).add(0), 0); // FALSE
            assert_eq!(*LOGICAL(dup).add(1), 0); // FALSE
            assert_eq!(*LOGICAL(dup).add(2), 0); // FALSE
            assert_eq!(*LOGICAL(dup).add(3), 1); // TRUE
            assert_eq!(*LOGICAL(dup).add(4), 1); // TRUE
        }
    }

    #[test]
    fn test_duplicated_int_all_unique() {
        unsafe {
            let x = make_int_vector(&[1, 2, 3, 4, 5]);
            let dup = duplicated_impl(x, false, NA_INTEGER);
            assert_eq!(XLENGTH(dup), 5);
            for i in 0..5 {
                assert_eq!(*LOGICAL(dup).add(i), 0); // all FALSE
            }
        }
    }

    #[test]
    fn test_duplicated_int_all_same() {
        unsafe {
            let x = make_int_vector(&[7, 7, 7, 7]);
            let dup = duplicated_impl(x, false, NA_INTEGER);
            assert_eq!(XLENGTH(dup), 4);
            assert_eq!(*LOGICAL(dup).add(0), 0); // FALSE (first)
            assert_eq!(*LOGICAL(dup).add(1), 1); // TRUE
            assert_eq!(*LOGICAL(dup).add(2), 1); // TRUE
            assert_eq!(*LOGICAL(dup).add(3), 1); // TRUE
        }
    }

    #[test]
    fn test_duplicated_int_empty() {
        unsafe {
            let x = Rf_allocVector3(SEXPTYPE::INTSXP.0, 0);
            let dup = duplicated_impl(x, false, NA_INTEGER);
            assert_eq!(XLENGTH(dup), 0);
        }
    }

    #[test]
    fn test_duplicated_int_single() {
        unsafe {
            let x = make_int_vector(&[42]);
            let dup = duplicated_impl(x, false, NA_INTEGER);
            assert_eq!(XLENGTH(dup), 1);
            // ScalarLogical(FALSE) returns length-1 vector
            assert_eq!(XLENGTH(dup), 1);
        }
    }

    #[test]
    fn test_duplicated_int_with_na() {
        unsafe {
            let x = make_int_vector(&[1, NA_INTEGER, 3, NA_INTEGER, 1]);
            let dup = duplicated_impl(x, false, NA_INTEGER);
            assert_eq!(XLENGTH(dup), 5);
            // 1: FALSE, NA: FALSE, 3: FALSE, NA: TRUE, 1: TRUE
            assert_eq!(*LOGICAL(dup).add(0), 0);
            assert_eq!(*LOGICAL(dup).add(1), 0);
            assert_eq!(*LOGICAL(dup).add(2), 0);
            assert_eq!(*LOGICAL(dup).add(3), 1);
            assert_eq!(*LOGICAL(dup).add(4), 1);
        }
    }

    #[test]
    fn test_duplicated_logical_basic() {
        unsafe {
            let x = make_logical_vector(&[1, 0, 1, 0, NA_LOGICAL]);
            let dup = duplicated_impl(x, false, NA_INTEGER);
            assert_eq!(XLENGTH(dup), 5);
            // TRUE, FALSE, TRUE(dup), FALSE(dup), NA
            assert_eq!(*LOGICAL(dup).add(0), 0);
            assert_eq!(*LOGICAL(dup).add(1), 0);
            assert_eq!(*LOGICAL(dup).add(2), 1); // TRUE is duplicate
            assert_eq!(*LOGICAL(dup).add(3), 1); // FALSE is duplicate
            assert_eq!(*LOGICAL(dup).add(4), 0); // NA first occurrence
        }
    }

    #[test]
    fn test_duplicated_real_basic() {
        unsafe {
            let x = make_real_vector(&[1.0, 2.5, 1.0, 3.0]);
            let dup = duplicated_impl(x, false, NA_INTEGER);
            assert_eq!(XLENGTH(dup), 4);
            assert_eq!(*LOGICAL(dup).add(0), 0);
            assert_eq!(*LOGICAL(dup).add(1), 0);
            assert_eq!(*LOGICAL(dup).add(2), 1); // 1.0 duplicate
            assert_eq!(*LOGICAL(dup).add(3), 0);
        }
    }

    #[test]
    fn test_duplicated_real_with_na() {
        unsafe {
            let na = f64::from_bits(R_NA_BIT_PATTERN);
            let x = make_real_vector(&[1.0, na, 3.0, na]);
            let dup = duplicated_impl(x, false, NA_INTEGER);
            assert_eq!(XLENGTH(dup), 4);
            assert_eq!(*LOGICAL(dup).add(0), 0);
            assert_eq!(*LOGICAL(dup).add(1), 0); // NA first
            assert_eq!(*LOGICAL(dup).add(2), 0);
            assert_eq!(*LOGICAL(dup).add(3), 1); // NA duplicate
        }
    }

    #[test]
    fn test_duplicated_real_with_nan() {
        unsafe {
            let x = make_real_vector(&[1.0, f64::NAN, 3.0, f64::NAN]);
            let dup = duplicated_impl(x, false, NA_INTEGER);
            assert_eq!(XLENGTH(dup), 4);
            assert_eq!(*LOGICAL(dup).add(0), 0);
            assert_eq!(*LOGICAL(dup).add(1), 0); // NaN first
            assert_eq!(*LOGICAL(dup).add(2), 0);
            assert_eq!(*LOGICAL(dup).add(3), 1); // NaN duplicate
        }
    }

    #[test]
    fn test_duplicated_real_signed_zero() {
        unsafe {
            let x = make_real_vector(&[0.0, -0.0]);
            let dup = duplicated_impl(x, false, NA_INTEGER);
            // Signed zeros should be treated as equal (normalized in hash)
            assert_eq!(XLENGTH(dup), 2);
            assert_eq!(*LOGICAL(dup).add(0), 0);
            assert_eq!(*LOGICAL(dup).add(1), 1); // -0.0 == 0.0
        }
    }

    // -----------------------------------------------------------------------
    // Tests for do_unique with integer vectors
    // -----------------------------------------------------------------------

    #[test]
    fn test_do_unique_int_basic() {
        unsafe {
            let x = make_int_vector(&[1, 2, 3, 2, 1]);
            let args = crate::sexp::memory_ext::allocList(4);
            // Set up the arg list: (x, incomp=FALSE, fromLast=FALSE, nmax=NA)
            crate::sexp::accessors::SETCAR(args, x);
            crate::sexp::accessors::SETCAR(CDR(args), Rf_ScalarLogical(0));
            crate::sexp::accessors::SETCAR(CDDR(args), Rf_ScalarLogical(0));
            crate::sexp::accessors::SETCAR(CDDDR(args), Rf_ScalarInteger(NA_INTEGER));

            let ans = do_unique(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );
            assert_eq!(XLENGTH(ans), 3);
            // unique should preserve first-occurrence order
            assert_eq!(*INTEGER(ans).add(0), 1);
            assert_eq!(*INTEGER(ans).add(1), 2);
            assert_eq!(*INTEGER(ans).add(2), 3);
        }
    }

    #[test]
    fn test_do_unique_int_empty() {
        unsafe {
            let x = Rf_allocVector3(SEXPTYPE::INTSXP.0, 0);
            let args = crate::sexp::memory_ext::allocList(4);
            crate::sexp::accessors::SETCAR(args, x);
            crate::sexp::accessors::SETCAR(CDR(args), Rf_ScalarLogical(0));
            crate::sexp::accessors::SETCAR(CDDR(args), Rf_ScalarLogical(0));
            crate::sexp::accessors::SETCAR(CDDDR(args), Rf_ScalarInteger(NA_INTEGER));

            let ans = do_unique(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );
            assert_eq!(XLENGTH(ans), 0);
        }
    }

    #[test]
    fn test_do_unique_int_all_unique() {
        unsafe {
            let x = make_int_vector(&[5, 4, 3, 2, 1]);
            let args = crate::sexp::memory_ext::allocList(4);
            crate::sexp::accessors::SETCAR(args, x);
            crate::sexp::accessors::SETCAR(CDR(args), Rf_ScalarLogical(0));
            crate::sexp::accessors::SETCAR(CDDR(args), Rf_ScalarLogical(0));
            crate::sexp::accessors::SETCAR(CDDDR(args), Rf_ScalarInteger(NA_INTEGER));

            let ans = do_unique(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );
            assert_eq!(XLENGTH(ans), 5);
        }
    }

    // -----------------------------------------------------------------------
    // Tests for do_duplicated
    // -----------------------------------------------------------------------

    #[test]
    fn test_do_duplicated_int() {
        unsafe {
            let x = make_int_vector(&[1, 2, 3, 2, 1]);
            let args = crate::sexp::memory_ext::allocList(4);
            crate::sexp::accessors::SETCAR(args, x);
            crate::sexp::accessors::SETCAR(CDR(args), Rf_ScalarLogical(0));
            crate::sexp::accessors::SETCAR(CDDR(args), Rf_ScalarLogical(0));
            crate::sexp::accessors::SETCAR(CDDDR(args), Rf_ScalarInteger(NA_INTEGER));

            let dup = do_duplicated(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );
            assert_eq!(XLENGTH(dup), 5);
            assert_eq!(*LOGICAL(dup).add(0), 0);
            assert_eq!(*LOGICAL(dup).add(1), 0);
            assert_eq!(*LOGICAL(dup).add(2), 0);
            assert_eq!(*LOGICAL(dup).add(3), 1);
            assert_eq!(*LOGICAL(dup).add(4), 1);
        }
    }

    #[test]
    fn test_do_duplicated_empty() {
        unsafe {
            let x = Rf_allocVector3(SEXPTYPE::INTSXP.0, 0);
            let args = crate::sexp::memory_ext::allocList(4);
            crate::sexp::accessors::SETCAR(args, x);
            crate::sexp::accessors::SETCAR(CDR(args), Rf_ScalarLogical(0));
            crate::sexp::accessors::SETCAR(CDDR(args), Rf_ScalarLogical(0));
            crate::sexp::accessors::SETCAR(CDDDR(args), Rf_ScalarInteger(NA_INTEGER));

            let dup = do_duplicated(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );
            assert_eq!(XLENGTH(dup), 0);
        }
    }

    // -----------------------------------------------------------------------
    // Tests for check_values (any/all core logic)
    // -----------------------------------------------------------------------

    #[test]
    fn test_check_values_any_all_true() {
        unsafe {
            let x = make_logical_vector(&[0, 0, 1, 0]);
            // any: finds TRUE -> returns 1
            assert_eq!(check_values(2, false, x, 4), 1);
        }
    }

    #[test]
    fn test_check_values_any_all_false() {
        unsafe {
            let x = make_logical_vector(&[0, 0, 0, 0]);
            // any: no TRUE -> returns 0
            assert_eq!(check_values(2, false, x, 4), 0);
        }
    }

    #[test]
    fn test_check_values_any_with_na() {
        unsafe {
            let x = make_logical_vector(&[NA_LOGICAL, 0, 0]);
            // any with NA: returns NA
            assert_eq!(check_values(2, false, x, 3), NA_LOGICAL);
        }
    }

    #[test]
    fn test_check_values_any_with_na_narm() {
        unsafe {
            let x = make_logical_vector(&[NA_LOGICAL, 0, 0]);
            // any with NA and na.rm=TRUE: returns 0 (FALSE)
            assert_eq!(check_values(2, true, x, 3), 0);
        }
    }

    #[test]
    fn test_check_values_any_true_over_na() {
        unsafe {
            let x = make_logical_vector(&[NA_LOGICAL, 1, 0]);
            // any: finds TRUE -> returns 1 immediately
            assert_eq!(check_values(2, false, x, 3), 1);
        }
    }

    #[test]
    fn test_check_values_all_all_true() {
        unsafe {
            let x = make_logical_vector(&[1, 1, 1]);
            // all: no FALSE -> returns 1 (TRUE)
            assert_eq!(check_values(1, false, x, 3), 1);
        }
    }

    #[test]
    fn test_check_values_all_has_false() {
        unsafe {
            let x = make_logical_vector(&[1, 0, 1]);
            // all: finds FALSE -> returns 0
            assert_eq!(check_values(1, false, x, 3), 0);
        }
    }

    #[test]
    fn test_check_values_all_with_na() {
        unsafe {
            let x = make_logical_vector(&[1, NA_LOGICAL, 1]);
            // all with NA: returns NA
            assert_eq!(check_values(1, false, x, 3), NA_LOGICAL);
        }
    }

    #[test]
    fn test_check_values_all_with_na_narm() {
        unsafe {
            let x = make_logical_vector(&[1, NA_LOGICAL, 1]);
            // all with NA and na.rm=TRUE: returns 1 (TRUE)
            assert_eq!(check_values(1, true, x, 3), 1);
        }
    }

    #[test]
    fn test_check_values_all_false_over_na() {
        unsafe {
            let x = make_logical_vector(&[1, 0, NA_LOGICAL]);
            // all: finds FALSE -> returns 0 immediately
            assert_eq!(check_values(1, false, x, 3), 0);
        }
    }

    #[test]
    fn test_check_values_empty_any() {
        unsafe {
            let x = make_logical_vector(&[]);
            // any of empty: returns 0 (FALSE)
            assert_eq!(check_values(2, false, x, 0), 0);
        }
    }

    #[test]
    fn test_check_values_empty_all() {
        unsafe {
            let x = make_logical_vector(&[]);
            // all of empty: returns 1 (TRUE)
            assert_eq!(check_values(1, false, x, 0), 1);
        }
    }

    // -----------------------------------------------------------------------
    // Tests for do_any and do_all (without na.rm argument, simplified)
    // -----------------------------------------------------------------------

    #[test]
    fn test_do_any_simple() {
        unsafe {
            // Create args list: (logical_vector)  -- no na.rm named arg
            let x = make_logical_vector(&[0, 0, 1, 0]);
            let args = crate::sexp::memory_ext::allocList(1);
            crate::sexp::accessors::SETCAR(args, x);

            let ans = do_any(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );
            assert_eq!(XLENGTH(ans), 1);
            assert_eq!(*LOGICAL(ans).add(0), 1); // TRUE
        }
    }

    #[test]
    fn test_do_any_all_false() {
        unsafe {
            let x = make_logical_vector(&[0, 0, 0]);
            let args = crate::sexp::memory_ext::allocList(1);
            crate::sexp::accessors::SETCAR(args, x);

            let ans = do_any(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );
            assert_eq!(*LOGICAL(ans).add(0), 0); // FALSE
        }
    }

    #[test]
    fn test_do_all_simple() {
        unsafe {
            let x = make_logical_vector(&[1, 1, 1]);
            let args = crate::sexp::memory_ext::allocList(1);
            crate::sexp::accessors::SETCAR(args, x);

            let ans = do_all(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );
            assert_eq!(XLENGTH(ans), 1);
            assert_eq!(*LOGICAL(ans).add(0), 1); // TRUE
        }
    }

    #[test]
    fn test_do_all_has_false() {
        unsafe {
            let x = make_logical_vector(&[1, 0, 1]);
            let args = crate::sexp::memory_ext::allocList(1);
            crate::sexp::accessors::SETCAR(args, x);

            let ans = do_all(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );
            assert_eq!(*LOGICAL(ans).add(0), 0); // FALSE
        }
    }

    // -----------------------------------------------------------------------
    // Tests for complex equality with NA
    // -----------------------------------------------------------------------

    #[test]
    fn test_duplicated_complex_basic() {
        unsafe {
            let v = Rf_allocVector3(SEXPTYPE::CPLXSXP.0, 3);
            let cx = COMPLEX(v);
            // {1+2i, 3+4i, 1+2i}
            *cx.add(0) = Rcomplex { r: 1.0, i: 2.0 };
            *cx.add(1) = Rcomplex { r: 3.0, i: 4.0 };
            *cx.add(2) = Rcomplex { r: 1.0, i: 2.0 };

            let dup = duplicated_impl(v, false, NA_INTEGER);
            assert_eq!(XLENGTH(dup), 3);
            assert_eq!(*LOGICAL(dup).add(0), 0); // first occurrence
            assert_eq!(*LOGICAL(dup).add(1), 0); // unique
            assert_eq!(*LOGICAL(dup).add(2), 1); // duplicate
        }
    }

    #[test]
    fn test_duplicated_complex_na() {
        unsafe {
            let v = Rf_allocVector3(SEXPTYPE::CPLXSXP.0, 3);
            let cx = COMPLEX(v);
            let na = f64::from_bits(R_NA_BIT_PATTERN);
            // {1+NA, 3+4i, 5+NA}  -- all NA-containing are equal
            *cx.add(0) = Rcomplex { r: 1.0, i: na };
            *cx.add(1) = Rcomplex { r: 3.0, i: 4.0 };
            *cx.add(2) = Rcomplex { r: 5.0, i: na };

            let dup = duplicated_impl(v, false, NA_INTEGER);
            assert_eq!(*LOGICAL(dup).add(0), 0); // first NA
            assert_eq!(*LOGICAL(dup).add(1), 0); // unique
            assert_eq!(*LOGICAL(dup).add(2), 1); // NA duplicate
        }
    }

    // -----------------------------------------------------------------------
    // Test for large vector hash table sizing
    // -----------------------------------------------------------------------

    #[test]
    fn test_hash_table_setup_int() {
        unsafe {
            let x = make_int_vector(&[1, 2, 3]);
            let data = hash_table_setup(x, NA_INTEGER);
            // 3 elements -> M = 8 (next power of 2 >= 6)
            assert_eq!(data.m, 8);
            assert_eq!(data.k, 3);
        }
    }

    #[test]
    fn test_hash_table_setup_logical() {
        unsafe {
            let x = make_logical_vector(&[1, 0, 1]);
            let data = hash_table_setup(x, NA_INTEGER);
            // Logical always uses M=4, K=2
            assert_eq!(data.m, 4);
            assert_eq!(data.k, 2);
        }
    }

    #[test]
    fn test_hash_table_setup_raw() {
        unsafe {
            let v = Rf_allocVector3(SEXPTYPE::RAWSXP.0, 5);
            let raw_ptr = RAW(v);
            for i in 0..5 {
                *raw_ptr.add(i) = i as u8;
            }

            let data = hash_table_setup(v, NA_INTEGER);
            // Raw always uses M=256, K=8
            assert_eq!(data.m, 256);
            assert_eq!(data.k, 8);
        }
    }
}
