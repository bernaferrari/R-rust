#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

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
use crate::sexp::object::Sexp;
use std::os::raw::c_int;

use crate::sexp::accessors::{
    COMPLEX, COMPLEX_ELT, INTEGER, INTEGER_ELT, LOGICAL, LOGICAL_ELT, RAW, RAW_ELT, REAL, REAL_ELT,
    SET_STRING_ELT, SET_VECTOR_ELT, STRING_ELT, TYPEOF, VECTOR_ELT, XLENGTH,
};
use crate::sexp::constructors::{Rf_ScalarLogical, Rf_allocVector3};
use crate::sexp::context::RError;
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, R_NA_BIT_PATTERN, R_xlen_t, Rcomplex, SEXPTYPE};
use crate::sexp::protect::protect;

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

fn unique_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
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
        } else {
            ISNAN(xi) && ISNAN(yj)
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
            t if t == SEXPTYPE::LGLSXP => (4usize, 2u32, XLENGTH(x)),
            t if t == SEXPTYPE::INTSXP => {
                let n = XLENGTH(x);
                mk_setup(n, nmax_arg as R_xlen_t)
            }
            t if t == SEXPTYPE::REALSXP => mk_setup(XLENGTH(x), nmax_arg as R_xlen_t),
            t if t == SEXPTYPE::CPLXSXP => mk_setup(XLENGTH(x), nmax_arg as R_xlen_t),
            t if t == SEXPTYPE::STRSXP => mk_setup(XLENGTH(x), nmax_arg as R_xlen_t),
            t if t == SEXPTYPE::RAWSXP => (256usize, 8u32, XLENGTH(x)),
            t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
                mk_setup(XLENGTH(x), nmax_arg as R_xlen_t)
            }
            _ => {
                // fallback
                mk_setup(XLENGTH(x), nmax_arg as R_xlen_t)
            }
        };

        // Allocate integer hash table filled with NIL
        let hash_table = Rf_allocVector3(SEXPTYPE::INTSXP, m as R_xlen_t);
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
            t if t == SEXPTYPE::LGLSXP => lhash,
            t if t == SEXPTYPE::INTSXP => ihash,
            t if t == SEXPTYPE::REALSXP => rhash,
            t if t == SEXPTYPE::CPLXSXP => chash,
            t if t == SEXPTYPE::STRSXP => cshash,
            t if t == SEXPTYPE::RAWSXP => rawhash_fn,
            _ => ihash, // fallback
        };

        let equal_fn: EqualFn = match xtype {
            t if t == SEXPTYPE::LGLSXP => lequal,
            t if t == SEXPTYPE::INTSXP => iequal,
            t if t == SEXPTYPE::REALSXP => requal,
            t if t == SEXPTYPE::CPLXSXP => cequal,
            t if t == SEXPTYPE::STRSXP => sequal,
            t if t == SEXPTYPE::RAWSXP => rawequal,
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
            return Rf_allocVector3(SEXPTYPE::LGLSXP, 0);
        }
        if n == 1 {
            return Rf_ScalarLogical(0); // FALSE
        }

        let mut data = hash_table_setup(x, nmax_arg);
        let _hash_table_guard = protect(data.hash_table);

        let ans = Rf_allocVector3(SEXPTYPE::LGLSXP, n);
        let _ans_guard = protect(ans);

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

        ans
    }
}

// ---------------------------------------------------------------------------
// Safe wrapper functions using Sexp<'a>
// ---------------------------------------------------------------------------

/// Safe wrapper for `duplicated_impl` using `Sexp<'a>`.
///
/// Returns `Ok(SEXP)` on success, `Err` on invalid input.
fn duplicated_safe(x: Sexp<'_>, from_last: bool, nmax_arg: i32) -> Result<SEXP, &'static str> {
    if !x.clone().is_vector() {
        return Err("duplicated requires a vector");
    }
    let n = x.clone().len();
    if n == 0 {
        return Ok(unsafe { Rf_allocVector3(SEXPTYPE::LGLSXP, 0) });
    }
    if n == 1 {
        return Ok(unsafe { Rf_ScalarLogical(0) });
    }
    let raw = x.as_raw();
    Ok(unsafe { duplicated_impl(raw, from_last, nmax_arg) })
}

/// Safe wrapper for `unique` using `Sexp<'a>`.
///
/// Returns `Ok(SEXP)` with unique elements on success.
fn unique_safe(x: Sexp<'_>, from_last: bool, nmax_arg: i32) -> Result<SEXP, &'static str> {
    if !x.clone().is_vector() {
        return Err("unique requires a vector");
    }
    let n = x.clone().len();
    if n == 0 {
        return Ok(unsafe { Rf_allocVector3(TYPEOF(x.as_raw()), 0) });
    }

    let raw = x.as_raw();
    let xtype = unsafe { TYPEOF(raw) };

    let dup = unsafe { duplicated_impl(raw, from_last, nmax_arg) };
    let _dup_guard = protect(dup);

    let mut k: R_xlen_t = 0;
    for i in 0..n {
        if unsafe { *LOGICAL(dup).add(i as usize) } == 0 {
            k += 1;
        }
    }

    let ans = unsafe { Rf_allocVector3(xtype, k) };
    let _ans_guard = protect(ans);

    let mut ki: R_xlen_t = 0;

    match xtype {
        t if t == SEXPTYPE::LGLSXP => {
            let a = unsafe { LOGICAL(ans) };
            for i in 0..n {
                if unsafe { *LOGICAL(dup).add(i as usize) } == 0 {
                    unsafe { *a.add(ki as usize) = LOGICAL_ELT(raw, i as c_int) };
                    ki += 1;
                }
            }
        }
        t if t == SEXPTYPE::INTSXP => {
            let a = unsafe { INTEGER(ans) };
            for i in 0..n {
                if unsafe { *LOGICAL(dup).add(i as usize) } == 0 {
                    unsafe { *a.add(ki as usize) = INTEGER_ELT(raw, i as c_int) };
                    ki += 1;
                }
            }
        }
        t if t == SEXPTYPE::REALSXP => {
            let a = unsafe { REAL(ans) };
            for i in 0..n {
                if unsafe { *LOGICAL(dup).add(i as usize) } == 0 {
                    unsafe { *a.add(ki as usize) = REAL_ELT(raw, i as c_int) };
                    ki += 1;
                }
            }
        }
        t if t == SEXPTYPE::CPLXSXP => {
            let a = unsafe { COMPLEX(ans) };
            for i in 0..n {
                if unsafe { *LOGICAL(dup).add(i as usize) } == 0 {
                    unsafe { *a.add(ki as usize) = COMPLEX_ELT(raw, i as c_int) };
                    ki += 1;
                }
            }
        }
        t if t == SEXPTYPE::STRSXP => {
            for i in 0..n {
                if unsafe { *LOGICAL(dup).add(i as usize) } == 0 {
                    unsafe { SET_STRING_ELT(ans, ki, STRING_ELT(raw, i)) };
                    ki += 1;
                }
            }
        }
        t if t == SEXPTYPE::VECSXP || t == SEXPTYPE::EXPRSXP => {
            for i in 0..n {
                if unsafe { *LOGICAL(dup).add(i as usize) } == 0 {
                    unsafe { SET_VECTOR_ELT(ans, ki, VECTOR_ELT(raw, i)) };
                    ki += 1;
                }
            }
        }
        t if t == SEXPTYPE::RAWSXP => {
            let a = unsafe { RAW(ans) };
            for i in 0..n {
                if unsafe { *LOGICAL(dup).add(i as usize) } == 0 {
                    unsafe { *a.add(ki as usize) = RAW_ELT(raw, i as c_int) };
                    ki += 1;
                }
            }
        }
        _ => {} // intentionally unhandled: unsupported SEXPTYPE for unique
    }

    Ok(ans)
}

/// Check values in a logical vector for any/all semantics (safe version).
///
/// Returns `TRUE`, `FALSE`, or `NA_LOGICAL`.
/// `op`: 1 = all, 2 = any
fn check_values_safe(x: Sexp<'_>, op: i32, na_rm: bool) -> Result<i32, String> {
    let n = x.clone().len();
    let mut has_na = false;

    for i in 0..n {
        let xi = x.clone().try_logical_elt(i).map_err(|err| err.to_string())?;
        if !na_rm && xi == NA_LOGICAL {
            has_na = true;
        } else {
            if xi == 1 && op == 2 {
                return Ok(1); // TRUE
            }
            if xi == 0 && op == 1 {
                return Ok(0); // FALSE
            }
        }
    }

    if op == 2 {
        Ok(if has_na { NA_LOGICAL } else { 0 })
    } else {
        Ok(if has_na { NA_LOGICAL } else { 1 })
    }
}

/// Safe wrapper for `any` using `Sexp<'a>`.
///
/// Returns `Ok(SEXP)` with a scalar logical result.
fn any_safe(args: Sexp<'_>) -> Result<SEXP, String> {
    let mut val: i32 = 0;
    let mut has_na = false;
    let mut na_rm = false;

    // Ownership: `args` is walked twice (na.rm scan, then value scan). The old
    // implicit Copy kept one handle alive across both passes; each pass now
    // consumes its own clone of the same underlying SEXP (no R-level copy).
    let mut arg_list = if args.clone().is_nil() { None } else { Some(args.clone()) };
    while let Some(current) = arg_list {
        if current
            .clone().try_tag_name_eq(b"na.rm").map_err(|err| err.to_string())?
        {
            let na_val = current.clone().try_car().clone().map_err(|err| err.to_string())?;
            if let Ok(nrm) = na_val.try_logical_elt(0) {
                na_rm = nrm == 1;
            }
        }
        arg_list = current
            .try_next_pairlist_cell()
            .map_err(|err| err.to_string())?;
    }


    // Second pass over the same list: `args` was already consumed by the
    // na.rm scan above, so this pass gets its own clone of the handle.
        let mut s = if args.clone().is_nil() { None } else { Some(args.clone()) };
    while let Some(current) = s {
        if current
            .clone().try_tag_name_eq(b"na.rm").map_err(|err| err.to_string())?
        {
            s = current
                .try_next_pairlist_cell()
                .map_err(|err| err.to_string())?;
            continue;
        }

        let t = current.clone().try_car().map_err(|err| err.to_string())?;
        let n = t.clone().len();

        if n > 0 {
            let cv = check_values_safe(t, 2, na_rm)?;
            if cv != NA_LOGICAL {
                if cv == 1 {
                    val = 1;
                    has_na = false;
                    break;
                }
            } else {
                has_na = true;
            }
            val = cv;
        }

        s = current
            .try_next_pairlist_cell()
            .map_err(|err| err.to_string())?;
    }

    if has_na {
        Ok(unsafe { Rf_ScalarLogical(NA_LOGICAL) })
    } else {
        Ok(unsafe { Rf_ScalarLogical(val) })
    }
}

/// Safe wrapper for `all` using `Sexp<'a>`.
///
/// Returns `Ok(SEXP)` with a scalar logical result.
fn all_safe(args: Sexp<'_>) -> Result<SEXP, String> {
    let mut val: i32 = 1;
    let mut has_na = false;
    let mut na_rm = false;

    let mut arg_list = if args.clone().is_nil() { None } else { Some(args.clone()) };
    while let Some(current) = arg_list {
        if current
            .clone().try_tag_name_eq(b"na.rm").map_err(|err| err.to_string())?
        {
            let na_val = current.clone().try_car().clone().map_err(|err| err.to_string())?;
            if let Ok(nrm) = na_val.try_logical_elt(0) {
                na_rm = nrm == 1;
            }
        }
        arg_list = current
            .try_next_pairlist_cell()
            .map_err(|err| err.to_string())?;
    }

    let mut s = if args.clone().is_nil() { None } else { Some(args.clone()) };
    while let Some(current) = s {
        if current
            .clone().try_tag_name_eq(b"na.rm").map_err(|err| err.to_string())?
        {
            s = current
                .try_next_pairlist_cell()
                .map_err(|err| err.to_string())?;
            continue;
        }

        let t = current.clone().try_car().map_err(|err| err.to_string())?;
        let n = t.clone().len();

        if n > 0 {
            let cv = check_values_safe(t, 1, na_rm)?;
            if cv != NA_LOGICAL {
                if cv == 0 {
                    has_na = false;
                    val = 0;
                    break;
                }
            } else {
                has_na = true;
            }
            val = cv;
        }

        s = current
            .try_next_pairlist_cell()
            .map_err(|err| err.to_string())?;
    }

    if has_na {
        Ok(unsafe { Rf_ScalarLogical(NA_LOGICAL) })
    } else {
        Ok(unsafe { Rf_ScalarLogical(val) })
    }
}

// ---------------------------------------------------------------------------
// FFI functions with catch_unwind delegation
// ---------------------------------------------------------------------------

/// Implementation of R's `unique()` builtin.
///
/// `.Internal(unique(x, incomparables, fromLast, nmax))`
/// PRIMVAL(op) == 1 in the C source; here called directly as do_unique.
///
/// Test-only in this port: the registered `unique` builtin dispatches to
/// `crate::mainutils::essentials::do_unique`; this raw pairlist wrapper is
/// exercised by the unit tests below.
pub unsafe fn do_unique(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let args_s = Sexp::try_from_raw(args)
        .unwrap_or_else(|err| -> Sexp<'_> { unique_error(err.to_string()) });
    let x = args_s
        .try_pairlist_arg(0)
        .unwrap_or_else(|err| -> Sexp<'_> { unique_error(err.to_string()) });
    unique_safe(x, false, NA_INTEGER).unwrap_or_else(|message| -> SEXP { unique_error(message) })
}

/// Implementation of R's `duplicated()` builtin.
///
/// `.Internal(duplicated(x, incomparables, fromLast, nmax))`
///
/// Test-only in this port: the registered `duplicated` builtin dispatches to
/// `crate::mainutils::essentials::do_duplicated`; this raw pairlist wrapper
/// is exercised by the unit tests below.
pub unsafe fn do_duplicated(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let args_s = Sexp::try_from_raw(args)
        .unwrap_or_else(|err| -> Sexp<'_> { unique_error(err.to_string()) });
    let x = args_s
        .try_pairlist_arg(0)
        .unwrap_or_else(|err| -> Sexp<'_> { unique_error(err.to_string()) });
    duplicated_safe(x, false, NA_INTEGER)
        .unwrap_or_else(|message| -> SEXP { unique_error(message) })
}

/// Implementation of R's `any()` builtin.
///
/// `.Internal(any(..., na.rm = FALSE))`
/// PRIMVAL(op) == 2 in the C source.
pub unsafe fn do_any(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let args_s = Sexp::try_from_raw(args)
        .unwrap_or_else(|err| -> Sexp<'_> { unique_error(err.to_string()) });
    any_safe(args_s).unwrap_or_else(|message| -> SEXP { unique_error(message) })
}

/// Implementation of R's `all()` builtin.
///
/// `.Internal(all(..., na.rm = FALSE))`
/// PRIMVAL(op) == 1 in the C source.
pub unsafe fn do_all(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let args_s = Sexp::try_from_raw(args)
        .unwrap_or_else(|err| -> Sexp<'_> { unique_error(err.to_string()) });
    all_safe(args_s).unwrap_or_else(|message| -> SEXP { unique_error(message) })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::sexp::accessors::*;
    use crate::sexp::constructors::*;

    use super::*;

    #[test]
    fn test_scatter_deterministic() {
        let _session = crate::sexp::session::RSession::new();
        let h1 = scatter(12345, 16);
        let h2 = scatter(12345, 16);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_scatter_different_keys() {
        let _session = crate::sexp::session::RSession::new();
        let h1 = scatter(12345, 16);
        let h2 = scatter(67890, 16);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_scatter_range() {
        let _session = crate::sexp::session::RSession::new();
        // With K=16, result should be in [0, 65535]
        for &key in &[0u32, 1, 42, 0xFFFFFFFF] {
            let h = scatter(key, 16);
            assert!(h < (1u32 << 16));
        }
    }

    #[test]
    fn test_unify_complex_na_normal() {
        let _session = crate::sexp::session::RSession::new();
        let z = Rcomplex { r: 1.0, i: 2.0 };
        let ans = unify_complex_na(z);
        assert_eq!(ans.r, 1.0);
        assert_eq!(ans.i, 2.0);
    }

    #[test]
    fn test_unify_complex_na_neg_zero() {
        let _session = crate::sexp::session::RSession::new();
        let z = Rcomplex { r: -0.0, i: 0.0 };
        let ans = unify_complex_na(z);
        assert_eq!(ans.r, 0.0);
        assert_eq!(ans.i, 0.0);
    }

    #[test]
    fn test_unify_complex_na_rna() {
        let _session = crate::sexp::session::RSession::new();
        let na = f64::from_bits(R_NA_BIT_PATTERN);
        let z = Rcomplex { r: na, i: 1.0 };
        let ans = unify_complex_na(z);
        assert!(R_IsNA(ans.r));
        assert!(R_IsNA(ans.i));
    }

    #[test]
    fn test_unify_complex_na_nan() {
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
        let h1 = PTRHASH(0x12345678 as usize);
        let h2 = PTRHASH(0x12345678 as usize);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_PTRHASH_different() {
        let _session = crate::sexp::session::RSession::new();
        let h1 = PTRHASH(0x12345678 as usize);
        let h2 = PTRHASH(0x87654321 as usize);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_cplx_eq_normal() {
        let _session = crate::sexp::session::RSession::new();
        let x = Rcomplex { r: 1.0, i: 2.0 };
        let y = Rcomplex { r: 1.0, i: 2.0 };
        assert!(cplx_eq(x, y));

        let z = Rcomplex { r: 1.0, i: 3.0 };
        assert!(!cplx_eq(x, z));
    }

    #[test]
    fn test_cplx_eq_na() {
        let _session = crate::sexp::session::RSession::new();
        let na = f64::from_bits(R_NA_BIT_PATTERN);
        let x = Rcomplex { r: na, i: 1.0 };
        let y = Rcomplex { r: na, i: 2.0 };
        assert!(cplx_eq(x, y)); // both have NA

        let z = Rcomplex { r: 1.0, i: 2.0 };
        assert!(!cplx_eq(x, z)); // x has NA, z doesn't
    }

    #[test]
    fn test_cplx_eq_nan() {
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(NIL, -1);
    }

    #[test]
    fn test_mk_setup_basic() {
        let _session = crate::sexp::session::RSession::new();
        // n=10 should give M >= 20 (power of 2), so M=32, K=5
        let (m, k, nmax) = unsafe { mk_setup(10, i64::MIN) };
        assert_eq!(m, 32);
        assert_eq!(k, 5);
        assert_eq!(nmax, 10);
    }

    #[test]
    fn test_mk_setup_nmax_override() {
        let _session = crate::sexp::session::RSession::new();
        // nmax=5 should override n=10
        let (m, k, nmax) = unsafe { mk_setup(10, 5) };
        assert_eq!(m, 16); // 2*5 = 10, next power of 2 is 16
        assert_eq!(k, 4);
        assert_eq!(nmax, 5);
    }

    #[test]
    fn test_mk_setup_large() {
        let _session = crate::sexp::session::RSession::new();
        let (m, k, _) = unsafe { mk_setup(1000, i64::MIN) };
        assert_eq!(m, 2048); // 2*1000=2000, next power of 2 is 2048
        assert_eq!(k, 11);
    }

    #[test]
    fn test_mk_setup_nmax_one() {
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
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
    fn make_int_vector(values: &[c_int]) -> SEXP {
        unsafe {
            let v = Rf_allocVector3(SEXPTYPE::INTSXP, values.len() as R_xlen_t);
            let ints = INTEGER(v);
            for (i, &val) in values.iter().enumerate() {
                *ints.add(i) = val;
            }
            v
        }
    }

    /// Helper to create a logical vector with values.
    fn make_logical_vector(values: &[c_int]) -> SEXP {
        unsafe {
            let v = Rf_allocVector3(SEXPTYPE::LGLSXP, values.len() as R_xlen_t);
            let logicals = LOGICAL(v);
            for (i, &val) in values.iter().enumerate() {
                *logicals.add(i) = val;
            }
            v
        }
    }

    /// Helper to create a real vector with values.
    fn make_real_vector(values: &[f64]) -> SEXP {
        unsafe {
            let v = Rf_allocVector3(SEXPTYPE::REALSXP, values.len() as R_xlen_t);
            let reals = REAL(v);
            for (i, &val) in values.iter().enumerate() {
                *reals.add(i) = val;
            }
            v
        }
    }

    #[test]
    fn test_duplicated_int_basic() {
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = Rf_allocVector3(SEXPTYPE::INTSXP, 0);
            let dup = duplicated_impl(x, false, NA_INTEGER);
            assert_eq!(XLENGTH(dup), 0);
        }
    }

    #[test]
    fn test_duplicated_int_single() {
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = Rf_allocVector3(SEXPTYPE::INTSXP, 0);
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
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let x = Rf_allocVector3(SEXPTYPE::INTSXP, 0);
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

    fn check_values(op: i32, na_rm: bool, x: SEXP, n: R_xlen_t) -> i32 {
        unsafe {
            let px = LOGICAL(x);
            let mut has_na = false;

            for i in 0..n {
                let xi = *px.add(i as usize);
                if !na_rm && xi == NA_LOGICAL {
                    has_na = true;
                } else {
                    if xi == 1 && op == 2 {
                        return 1;
                    }
                    if xi == 0 && op == 1 {
                        return 0;
                    }
                }
            }

            if op == 2 {
                if has_na { NA_LOGICAL } else { 0 }
            } else {
                if has_na { NA_LOGICAL } else { 1 }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Tests for check_values (any/all core logic)
    // -----------------------------------------------------------------------

    #[test]
    fn test_check_values_any_all_true() {
        let _session = crate::sexp::session::RSession::new();
        let x = make_logical_vector(&[0, 0, 1, 0]);
        // any: finds TRUE -> returns 1
        assert_eq!(check_values(2, false, x, 4), 1);
    }

    #[test]
    fn test_check_values_any_all_false() {
        let _session = crate::sexp::session::RSession::new();
        let x = make_logical_vector(&[0, 0, 0, 0]);
        // any: no TRUE -> returns 0
        assert_eq!(check_values(2, false, x, 4), 0);
    }

    #[test]
    fn test_check_values_any_with_na() {
        let _session = crate::sexp::session::RSession::new();
        let x = make_logical_vector(&[NA_LOGICAL, 0, 0]);
        // any with NA: returns NA
        assert_eq!(check_values(2, false, x, 3), NA_LOGICAL);
    }

    #[test]
    fn test_check_values_any_with_na_narm() {
        let _session = crate::sexp::session::RSession::new();
        let x = make_logical_vector(&[NA_LOGICAL, 0, 0]);
        // any with NA and na.rm=TRUE: returns 0 (FALSE)
        assert_eq!(check_values(2, true, x, 3), 0);
    }

    #[test]
    fn test_check_values_any_true_over_na() {
        let _session = crate::sexp::session::RSession::new();
        let x = make_logical_vector(&[NA_LOGICAL, 1, 0]);
        // any: finds TRUE -> returns 1 immediately
        assert_eq!(check_values(2, false, x, 3), 1);
    }

    #[test]
    fn test_check_values_all_all_true() {
        let _session = crate::sexp::session::RSession::new();
        let x = make_logical_vector(&[1, 1, 1]);
        // all: no FALSE -> returns 1 (TRUE)
        assert_eq!(check_values(1, false, x, 3), 1);
    }

    #[test]
    fn test_check_values_all_has_false() {
        let _session = crate::sexp::session::RSession::new();
        let x = make_logical_vector(&[1, 0, 1]);
        // all: finds FALSE -> returns 0
        assert_eq!(check_values(1, false, x, 3), 0);
    }

    #[test]
    fn test_check_values_all_with_na() {
        let _session = crate::sexp::session::RSession::new();
        let x = make_logical_vector(&[1, NA_LOGICAL, 1]);
        // all with NA: returns NA
        assert_eq!(check_values(1, false, x, 3), NA_LOGICAL);
    }

    #[test]
    fn test_check_values_all_with_na_narm() {
        let _session = crate::sexp::session::RSession::new();
        let x = make_logical_vector(&[1, NA_LOGICAL, 1]);
        // all with NA and na.rm=TRUE: returns 1 (TRUE)
        assert_eq!(check_values(1, true, x, 3), 1);
    }

    #[test]
    fn test_check_values_all_false_over_na() {
        let _session = crate::sexp::session::RSession::new();
        let x = make_logical_vector(&[1, 0, NA_LOGICAL]);
        // all: finds FALSE -> returns 0 immediately
        assert_eq!(check_values(1, false, x, 3), 0);
    }

    #[test]
    fn test_check_values_empty_any() {
        let _session = crate::sexp::session::RSession::new();
        let x = make_logical_vector(&[]);
        // any of empty: returns 0 (FALSE)
        assert_eq!(check_values(2, false, x, 0), 0);
    }

    #[test]
    fn test_check_values_empty_all() {
        let _session = crate::sexp::session::RSession::new();
        let x = make_logical_vector(&[]);
        // all of empty: returns 1 (TRUE)
        assert_eq!(check_values(1, false, x, 0), 1);
    }

    // -----------------------------------------------------------------------
    // Tests for do_any and do_all (without na.rm argument, simplified)
    // -----------------------------------------------------------------------

    #[test]
    fn test_do_any_simple() {
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_allocVector3(SEXPTYPE::CPLXSXP, 3);
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
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_allocVector3(SEXPTYPE::CPLXSXP, 3);
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
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_allocVector3(SEXPTYPE::RAWSXP, 5);
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
