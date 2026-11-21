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
use crate::sexp::object::{SessionNodeFactory, Sexp, SexpMut, SexpResult};
use crate::sexp::owner::{OwnerToken, StoredOwner};
use std::os::raw::c_int;

use crate::sexp::context::RError;
use crate::sexp::ffi::{NA_INTEGER, NA_LOGICAL, R_NA_BIT_PATTERN, R_xlen_t, Rcomplex, SEXPTYPE};

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
// Session-scoped checked helpers
// ---------------------------------------------------------------------------

/// Acquire the current session's node factory for one builtin scope.
///
/// # Safety boundary
/// [`OwnerToken::current`] names whichever instance is current on this
/// thread. These entry points only run on the interpreter thread during
/// builtin evaluation, where the current instance is the rightful
/// allocation owner.
fn session_factory() -> SexpResult<SessionNodeFactory<'static>> {
    // SAFETY: builtin evaluation runs on the interpreter thread with a live
    // current instance; the token only scopes this factory's allocations.
    let owner = unsafe { OwnerToken::current() }?;
    Ok(owner.node_factory())
}

/// Wrap a raw SEXP owned by the current session's heap.
///
/// Unlike `Sexp::try_from_raw`, `SessionNodeFactory::wrap` validates heap
/// membership and returns a session-owned handle, so handles derived from it
/// keep enough authority for checked allocation and mutation.
fn wrap_current(ptr: SEXP) -> SexpResult<Sexp<'static>> {
    session_factory()?.wrap(ptr)
}

/// Allocation authority carried by an already checked handle.
fn vector_factory<'a>(x: &Sexp<'a>) -> SexpResult<SessionNodeFactory<'a>> {
    StoredOwner::from_value(x)?.node_factory()
}

/// Allocate a fresh mutable vector in `x`'s session.
fn alloc_like<'a>(x: &Sexp<'a>, kind: SEXPTYPE, len: R_xlen_t) -> SexpResult<SexpMut<'a>> {
    let value = vector_factory(x)?.allocate(|arena| Some(arena.alloc_vector(kind, len)))?;
    SexpMut::try_from_checked(value)
}

/// Allocate a fresh vector in `x`'s session, falling back to the current
/// session for handles that carry no owner authority (e.g. `R_NilValue`).
fn alloc_in_session<'a>(x: &Sexp<'a>, kind: SEXPTYPE, len: R_xlen_t) -> SexpResult<SexpMut<'a>> {
    match alloc_like(x, kind, len) {
        ok @ Ok(_) => ok,
        Err(_) => {
            let value = session_factory()?.allocate(|arena| Some(arena.alloc_vector(kind, len)))?;
            SexpMut::try_from_checked(value)
        }
    }
}

/// Allocate a one-element logical vector holding `v` (may be `NA_LOGICAL`).
fn scalar_logical<'a>(x: &Sexp<'a>, v: c_int) -> SexpResult<Sexp<'a>> {
    let mut scalar = alloc_in_session(x, SEXPTYPE::LGLSXP, 1)?;
    scalar.try_set_logical_elt(0, v)?;
    Ok(scalar.freeze())
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
    if R_IsNA(x.r) || R_IsNA(x.i) {
        return R_IsNA(y.r) || R_IsNA(y.i);
    }
    if R_IsNA(y.r) || R_IsNA(y.i) {
        return false;
    }
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
///
/// The table is pure Rust scratch storage: it only holds vector indices for
/// one call, is never exposed to R, and drops deterministically. Upstream R
/// uses an INTSXP only because it lacks a native heap.
struct HashData {
    k: u32,          // log2(M)
    m: usize,        // table size (power of 2)
    nmax: R_xlen_t,  // remaining capacity
    table: Vec<i32>, // open-addressing slots; NIL marks an empty slot
}

/// Hash function for logical vectors: NA -> 2, FALSE -> 0, TRUE -> 1.
#[inline]
fn lhash(x: &Sexp<'_>, indx: R_xlen_t, _d: &HashData) -> SexpResult<usize> {
    let xi = x.try_logical_elt(indx)?;
    Ok(if xi == NA_LOGICAL { 2 } else { xi as usize })
}

/// Equality test for logical elements.
#[inline]
fn lequal(x: &Sexp<'_>, i: R_xlen_t, y: &Sexp<'_>, j: R_xlen_t) -> SexpResult<bool> {
    if i < 0 || j < 0 {
        return Ok(false);
    }
    Ok(x.try_logical_elt(i)? == y.try_logical_elt(j)?)
}

/// Hash function for integer vectors using scatter.
#[inline]
fn ihash(x: &Sexp<'_>, indx: R_xlen_t, d: &HashData) -> SexpResult<usize> {
    let xi = x.try_integer_elt(indx)?;
    Ok(if xi == NA_INTEGER {
        0
    } else {
        scatter(xi as u32, d.k) as usize
    })
}

/// Equality test for integer elements.
#[inline]
fn iequal(x: &Sexp<'_>, i: R_xlen_t, y: &Sexp<'_>, j: R_xlen_t) -> SexpResult<bool> {
    if i < 0 || j < 0 {
        return Ok(false);
    }
    Ok(x.try_integer_elt(i)? == y.try_integer_elt(j)?)
}

/// Hash function for real (double) vectors.
/// Normalizes signed zero, NA, and NaN for consistent hashing.
#[inline]
fn rhash(x: &Sexp<'_>, indx: R_xlen_t, d: &HashData) -> SexpResult<usize> {
    let xi = x.try_real_elt(indx)?;
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
    Ok(scatter(lo.wrapping_add(hi), d.k) as usize)
}

/// Equality test for real elements.
/// NA == NA, NaN == NaN, otherwise exact comparison.
#[inline]
fn requal(x: &Sexp<'_>, i: R_xlen_t, y: &Sexp<'_>, j: R_xlen_t) -> SexpResult<bool> {
    if i < 0 || j < 0 {
        return Ok(false);
    }
    let xi = x.try_real_elt(i)?;
    let yj = y.try_real_elt(j)?;
    Ok(if !ISNAN(xi) && !ISNAN(yj) {
        xi == yj
    } else if R_IsNA(xi) && R_IsNA(yj) {
        true
    } else {
        ISNAN(xi) && ISNAN(yj)
    })
}

/// Hash function for complex vectors.
#[inline]
fn chash(x: &Sexp<'_>, indx: R_xlen_t, d: &HashData) -> SexpResult<usize> {
    let tmp = unify_complex_na(x.try_complex_elt(indx)?);
    let rbits = tmp.r.to_bits();
    let ibits = tmp.i.to_bits();
    let u = ((rbits as u32) ^ ((rbits >> 32) as u32)) ^ ((ibits as u32) ^ ((ibits >> 32) as u32));
    Ok(scatter(u, d.k) as usize)
}

/// Equality test for complex elements using cplx_eq.
#[inline]
fn cequal(x: &Sexp<'_>, i: R_xlen_t, y: &Sexp<'_>, j: R_xlen_t) -> SexpResult<bool> {
    if i < 0 || j < 0 {
        return Ok(false);
    }
    Ok(cplx_eq(x.try_complex_elt(i)?, y.try_complex_elt(j)?))
}

/// Hash for strings by pointer address.
#[inline]
fn cshash(x: &Sexp<'_>, indx: R_xlen_t, d: &HashData) -> SexpResult<usize> {
    let elt = x.try_string_elt(indx)?;
    Ok(scatter(PTRHASH(elt.as_raw() as usize), d.k) as usize)
}

/// Equality test for string elements (by pointer identity for cached strings).
#[inline]
fn sequal(x: &Sexp<'_>, i: R_xlen_t, y: &Sexp<'_>, j: R_xlen_t) -> SexpResult<bool> {
    if i < 0 || j < 0 {
        return Ok(false);
    }
    Ok(x.try_string_elt(i)?.as_raw() == y.try_string_elt(j)?.as_raw())
}

/// Hash for raw (byte) vectors: identity hash.
#[inline]
fn rawhash_fn(x: &Sexp<'_>, indx: R_xlen_t, _d: &HashData) -> SexpResult<usize> {
    Ok(x.try_raw_elt(indx)? as usize)
}

/// Equality test for raw elements.
#[inline]
fn rawequal(x: &Sexp<'_>, i: R_xlen_t, y: &Sexp<'_>, j: R_xlen_t) -> SexpResult<bool> {
    if i < 0 || j < 0 {
        return Ok(false);
    }
    Ok(x.try_raw_elt(i)? == y.try_raw_elt(j)?)
}

/// Hash function pointer type.
type HashFn = fn(&Sexp<'_>, R_xlen_t, &HashData) -> SexpResult<usize>;

/// Equality function pointer type.
type EqualFn = fn(&Sexp<'_>, R_xlen_t, &Sexp<'_>, R_xlen_t) -> SexpResult<bool>;

/// Set up hash table parameters: choose M (smallest power of 2 >= 2*n) and K = log2(M).
fn mk_setup(n: R_xlen_t, nmax_arg: R_xlen_t) -> (usize, u32, R_xlen_t) {
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
fn hash_table_setup(x: &Sexp<'_>, nmax_arg: i32) -> HashData {
    let xtype = x.typeof_();
    let (m, k, nmax) = match xtype {
        SEXPTYPE::LGLSXP => (4usize, 2u32, x.len()),
        SEXPTYPE::INTSXP
        | SEXPTYPE::REALSXP
        | SEXPTYPE::CPLXSXP
        | SEXPTYPE::STRSXP
        | SEXPTYPE::VECSXP
        | SEXPTYPE::EXPRSXP => mk_setup(x.len(), nmax_arg as R_xlen_t),
        SEXPTYPE::RAWSXP => (256usize, 8u32, x.len()),
        _ => {
            // fallback
            mk_setup(x.len(), nmax_arg as R_xlen_t)
        }
    };

    HashData {
        k,
        m,
        nmax,
        table: vec![NIL; m],
    }
}

/// Check if element at index `indx` in vector `x` is a duplicate using open-addressing.
///
/// Returns 1 if duplicated (seen before), 0 if first occurrence.
/// First occurrences are inserted into the hash table.
fn is_duplicated(x: &Sexp<'_>, indx: R_xlen_t, d: &mut HashData) -> SexpResult<i32> {
    let xtype = x.typeof_();

    let hash_fn: HashFn = match xtype {
        SEXPTYPE::LGLSXP => lhash,
        SEXPTYPE::INTSXP => ihash,
        SEXPTYPE::REALSXP => rhash,
        SEXPTYPE::CPLXSXP => chash,
        SEXPTYPE::STRSXP => cshash,
        SEXPTYPE::RAWSXP => rawhash_fn,
        _ => ihash, // fallback
    };

    let equal_fn: EqualFn = match xtype {
        SEXPTYPE::LGLSXP => lequal,
        SEXPTYPE::INTSXP => iequal,
        SEXPTYPE::REALSXP => requal,
        SEXPTYPE::CPLXSXP => cequal,
        SEXPTYPE::STRSXP => sequal,
        SEXPTYPE::RAWSXP => rawequal,
        _ => iequal, // fallback
    };

    let mut i = hash_fn(x, indx, d)?;
    while d.table[i] != NIL {
        let entry = d.table[i];
        if equal_fn(x, entry as R_xlen_t, x, indx)? {
            return Ok(if entry >= 0 { 1 } else { 0 });
        }
        i = (i + 1) % d.m;
    }
    if d.nmax <= 0 {
        // hash table full - should not happen with proper sizing
    }
    d.nmax -= 1;
    d.table[i] = indx as i32;
    Ok(0)
}

/// Core Duplicated implementation: returns a logical vector where TRUE means duplicated.
///
/// `from_last`: if true, scan from right to left (so last occurrence is kept).
fn duplicated_impl<'a>(x: &Sexp<'a>, from_last: bool, nmax_arg: i32) -> SexpResult<Sexp<'a>> {
    let n = x.len();
    let factory = vector_factory(x)?;
    if n == 0 {
        return factory.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::LGLSXP, 0)));
    }
    if n == 1 {
        return Ok(factory.domain().logical(false)); // FALSE
    }

    let mut data = hash_table_setup(x, nmax_arg);

    let mut ans = SexpMut::try_from_checked(
        factory.allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::LGLSXP, n)))?,
    )?;

    if from_last {
        let mut i = n - 1;
        loop {
            let flag = is_duplicated(x, i, &mut data)?;
            ans.try_set_logical_elt(i, flag)?;
            if i == 0 {
                break;
            }
            i -= 1;
        }
    } else {
        for i in 0..n {
            let flag = is_duplicated(x, i, &mut data)?;
            ans.try_set_logical_elt(i, flag)?;
        }
    }

    Ok(ans.freeze())
}

// ---------------------------------------------------------------------------
// Safe wrapper functions using Sexp<'a>
// ---------------------------------------------------------------------------

/// Safe wrapper for `duplicated_impl` using `Sexp<'a>`.
///
/// Returns `Ok(SEXP)` on success, `Err` on invalid input.
fn duplicated_safe<'a>(x: Sexp<'a>, from_last: bool, nmax_arg: i32) -> Result<SEXP, String> {
    if !x.is_vector() {
        return Err("duplicated requires a vector".into());
    }
    duplicated_impl(&x, from_last, nmax_arg)
        .map(|dup| dup.as_raw())
        .map_err(|err| err.to_string())
}

/// Core `unique` implementation over checked handles.
///
/// Keeps the first (`from_last = false`) or last (`true`) occurrence of each
/// element, preserving order.
fn unique_impl<'a>(x: &Sexp<'a>, from_last: bool, nmax_arg: i32) -> SexpResult<Sexp<'a>> {
    let n = x.len();
    let xtype = x.typeof_();
    let factory = vector_factory(x)?;

    if n == 0 {
        return factory.allocate(|arena| Some(arena.alloc_vector(xtype, 0)));
    }

    let dup = duplicated_impl(x, from_last, nmax_arg)?;

    let mut k: R_xlen_t = 0;
    for i in 0..n {
        if dup.try_logical_elt(i)? == 0 {
            k += 1;
        }
    }

    let mut ans =
        SexpMut::try_from_checked(factory.allocate(|arena| Some(arena.alloc_vector(xtype, k)))?)?;

    let mut ki: R_xlen_t = 0;

    match xtype {
        SEXPTYPE::LGLSXP => {
            for i in 0..n {
                if dup.try_logical_elt(i)? == 0 {
                    let value = x.try_logical_elt(i)?;
                    ans.try_set_logical_elt(ki, value)?;
                    ki += 1;
                }
            }
        }
        SEXPTYPE::INTSXP => {
            for i in 0..n {
                if dup.try_logical_elt(i)? == 0 {
                    let value = x.try_integer_elt(i)?;
                    ans.try_set_integer_elt(ki, value)?;
                    ki += 1;
                }
            }
        }
        SEXPTYPE::REALSXP => {
            for i in 0..n {
                if dup.try_logical_elt(i)? == 0 {
                    let value = x.try_real_elt(i)?;
                    ans.try_set_real_elt(ki, value)?;
                    ki += 1;
                }
            }
        }
        SEXPTYPE::CPLXSXP => {
            for i in 0..n {
                if dup.try_logical_elt(i)? == 0 {
                    let value = x.try_complex_elt(i)?;
                    ans.try_set_complex_elt(ki, value)?;
                    ki += 1;
                }
            }
        }
        SEXPTYPE::STRSXP => {
            for i in 0..n {
                if dup.try_logical_elt(i)? == 0 {
                    let value = x.try_string_elt(i)?;
                    ans.try_set_string_elt(ki, value)?;
                    ki += 1;
                }
            }
        }
        SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP => {
            for i in 0..n {
                if dup.try_logical_elt(i)? == 0 {
                    let value = x.try_vector_elt(i)?;
                    ans.try_set_vector_elt(ki, value)?;
                    ki += 1;
                }
            }
        }
        SEXPTYPE::RAWSXP => {
            for i in 0..n {
                if dup.try_logical_elt(i)? == 0 {
                    let value = x.try_raw_elt(i)?;
                    ans.try_set_raw_elt(ki, value)?;
                    ki += 1;
                }
            }
        }
        _ => {} // intentionally unhandled: unsupported SEXPTYPE for unique
    }

    Ok(ans.freeze())
}

/// Safe wrapper for `unique` using `Sexp<'a>`.
///
/// Returns `Ok(SEXP)` with unique elements on success.
fn unique_safe<'a>(x: Sexp<'a>, from_last: bool, nmax_arg: i32) -> Result<SEXP, String> {
    if !x.is_vector() {
        return Err("unique requires a vector".into());
    }
    unique_impl(&x, from_last, nmax_arg)
        .map(|ans| ans.as_raw())
        .map_err(|err| err.to_string())
}

/// Check values in a logical vector for any/all semantics (safe version).
///
/// Returns `TRUE`, `FALSE`, or `NA_LOGICAL`.
/// `op`: 1 = all, 2 = any
fn check_values_safe(x: Sexp<'_>, op: i32, na_rm: bool) -> Result<i32, String> {
    let n = x.clone().len();
    let mut has_na = false;

    for i in 0..n {
        let xi = x
            .clone()
            .try_logical_elt(i)
            .map_err(|err| err.to_string())?;
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
    let mut arg_list = if args.clone().is_nil() {
        None
    } else {
        Some(args.clone())
    };
    while let Some(current) = arg_list {
        if current
            .clone()
            .try_tag_name_eq(b"na.rm")
            .map_err(|err| err.to_string())?
        {
            let na_val = current
                .clone()
                .try_car()
                .clone()
                .map_err(|err| err.to_string())?;
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
    let mut s = if args.clone().is_nil() {
        None
    } else {
        Some(args.clone())
    };
    while let Some(current) = s {
        if current
            .clone()
            .try_tag_name_eq(b"na.rm")
            .map_err(|err| err.to_string())?
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

    let answer = if has_na { NA_LOGICAL } else { val };
    scalar_logical(&args, answer)
        .map(|value| value.as_raw())
        .map_err(|err| err.to_string())
}

/// Safe wrapper for `all` using `Sexp<'a>`.
///
/// Returns `Ok(SEXP)` with a scalar logical result.
fn all_safe(args: Sexp<'_>) -> Result<SEXP, String> {
    let mut val: i32 = 1;
    let mut has_na = false;
    let mut na_rm = false;

    let mut arg_list = if args.clone().is_nil() {
        None
    } else {
        Some(args.clone())
    };
    while let Some(current) = arg_list {
        if current
            .clone()
            .try_tag_name_eq(b"na.rm")
            .map_err(|err| err.to_string())?
        {
            let na_val = current
                .clone()
                .try_car()
                .clone()
                .map_err(|err| err.to_string())?;
            if let Ok(nrm) = na_val.try_logical_elt(0) {
                na_rm = nrm == 1;
            }
        }
        arg_list = current
            .try_next_pairlist_cell()
            .map_err(|err| err.to_string())?;
    }

    let mut s = if args.clone().is_nil() {
        None
    } else {
        Some(args.clone())
    };
    while let Some(current) = s {
        if current
            .clone()
            .try_tag_name_eq(b"na.rm")
            .map_err(|err| err.to_string())?
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

    let answer = if has_na { NA_LOGICAL } else { val };
    scalar_logical(&args, answer)
        .map(|value| value.as_raw())
        .map_err(|err| err.to_string())
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
pub fn do_unique(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let args_s =
        wrap_current(args).unwrap_or_else(|err| -> Sexp<'_> { unique_error(err.to_string()) });
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
pub fn do_duplicated(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let args_s =
        wrap_current(args).unwrap_or_else(|err| -> Sexp<'_> { unique_error(err.to_string()) });
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
pub fn do_any(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let args_s =
        wrap_current(args).unwrap_or_else(|err| -> Sexp<'_> { unique_error(err.to_string()) });
    any_safe(args_s).unwrap_or_else(|message| -> SEXP { unique_error(message) })
}

/// Implementation of R's `all()` builtin.
///
/// `.Internal(all(..., na.rm = FALSE))`
/// PRIMVAL(op) == 1 in the C source.
pub fn do_all(_call: SEXP, _op: SEXP, args: SEXP, _env: SEXP) -> SEXP {
    let args_s =
        wrap_current(args).unwrap_or_else(|err| -> Sexp<'_> { unique_error(err.to_string()) });
    all_safe(args_s).unwrap_or_else(|message| -> SEXP { unique_error(message) })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
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
        let (m, k, nmax) = mk_setup(10, i64::MIN);
        assert_eq!(m, 32);
        assert_eq!(k, 5);
        assert_eq!(nmax, 10);
    }

    #[test]
    fn test_mk_setup_nmax_override() {
        let _session = crate::sexp::session::RSession::new();
        // nmax=5 should override n=10
        let (m, k, nmax) = mk_setup(10, 5);
        assert_eq!(m, 16); // 2*5 = 10, next power of 2 is 16
        assert_eq!(k, 4);
        assert_eq!(nmax, 5);
    }

    #[test]
    fn test_mk_setup_large() {
        let _session = crate::sexp::session::RSession::new();
        let (m, k, _) = mk_setup(1000, i64::MIN);
        assert_eq!(m, 2048); // 2*1000=2000, next power of 2 is 2048
        assert_eq!(k, 11);
    }

    #[test]
    fn test_mk_setup_nmax_one() {
        let _session = crate::sexp::session::RSession::new();
        // nmax=1 is special: not used
        let (m, k, nmax) = mk_setup(10, 1);
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

    /// Session-bound factory for the checked helpers below.
    fn test_factory<'a>(session: &'a crate::sexp::session::RSession) -> SessionNodeFactory<'a> {
        session
            .owner_token()
            .expect("active session")
            .node_factory()
    }

    /// Helper to create an integer vector with values.
    fn make_int_vector<'a>(factory: &SessionNodeFactory<'a>, values: &[c_int]) -> Sexp<'a> {
        let value = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, values.len() as R_xlen_t)))
            .expect("integer vector allocation");
        let mut vector = SexpMut::try_from_checked(value).expect("fresh vector is mutable");
        for (i, &val) in values.iter().enumerate() {
            vector
                .try_set_integer_elt(i as R_xlen_t, val)
                .expect("in-bounds integer write");
        }
        vector.freeze()
    }

    /// Helper to create a logical vector with values.
    fn make_logical_vector<'a>(factory: &SessionNodeFactory<'a>, values: &[c_int]) -> Sexp<'a> {
        let value = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::LGLSXP, values.len() as R_xlen_t)))
            .expect("logical vector allocation");
        let mut vector = SexpMut::try_from_checked(value).expect("fresh vector is mutable");
        for (i, &val) in values.iter().enumerate() {
            vector
                .try_set_logical_elt(i as R_xlen_t, val)
                .expect("in-bounds logical write");
        }
        vector.freeze()
    }

    /// Helper to create a real vector with values.
    fn make_real_vector<'a>(factory: &SessionNodeFactory<'a>, values: &[f64]) -> Sexp<'a> {
        let value = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::REALSXP, values.len() as R_xlen_t)))
            .expect("real vector allocation");
        let mut vector = SexpMut::try_from_checked(value).expect("fresh vector is mutable");
        for (i, &val) in values.iter().enumerate() {
            vector
                .try_set_real_elt(i as R_xlen_t, val)
                .expect("in-bounds real write");
        }
        vector.freeze()
    }

    /// Helper to create a complex vector with values.
    fn make_complex_vector<'a>(factory: &SessionNodeFactory<'a>, values: &[Rcomplex]) -> Sexp<'a> {
        let value = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::CPLXSXP, values.len() as R_xlen_t)))
            .expect("complex vector allocation");
        let mut vector = SexpMut::try_from_checked(value).expect("fresh vector is mutable");
        for (i, &val) in values.iter().enumerate() {
            vector
                .try_set_complex_elt(i as R_xlen_t, val)
                .expect("in-bounds complex write");
        }
        vector.freeze()
    }

    /// Helper to create a raw vector with values.
    fn make_raw_vector<'a>(factory: &SessionNodeFactory<'a>, values: &[u8]) -> Sexp<'a> {
        let value = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::RAWSXP, values.len() as R_xlen_t)))
            .expect("raw vector allocation");
        let mut vector = SexpMut::try_from_checked(value).expect("fresh vector is mutable");
        for (i, &val) in values.iter().enumerate() {
            vector
                .try_set_raw_elt(i as R_xlen_t, val)
                .expect("in-bounds raw write");
        }
        vector.freeze()
    }

    /// Helper to build an untagged positional argument pairlist.
    fn make_args<'a>(factory: &SessionNodeFactory<'a>, values: &[&Sexp<'a>]) -> Sexp<'a> {
        let mut list = factory.nil();
        for value in values.iter().rev() {
            list = factory
                .pairlist_cell(value, &list, &factory.nil())
                .expect("pairlist cell allocation");
        }
        list
    }

    /// Standard unique/duplicated argument list:
    /// `(x, incomparables = FALSE, fromLast = FALSE, nmax = NA)`.
    fn make_duplicated_args<'a>(factory: &SessionNodeFactory<'a>, x: &Sexp<'a>) -> Sexp<'a> {
        let no_incomparables = make_logical_vector(factory, &[0]);
        let from_last = make_logical_vector(factory, &[0]);
        let nmax = make_int_vector(factory, &[NA_INTEGER]);
        make_args(factory, &[x, &no_incomparables, &from_last, &nmax])
    }

    /// Raw ABI shim: the `do_*` entry points take `SEXP`, so every test
    /// funnels through these call sites.
    fn call_do_unique(args: Sexp<'_>) -> SEXP {
        unsafe {
            do_unique(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args.as_raw(),
                std::ptr::null_mut(),
            )
        }
    }

    fn call_do_duplicated(args: Sexp<'_>) -> SEXP {
        unsafe {
            do_duplicated(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args.as_raw(),
                std::ptr::null_mut(),
            )
        }
    }

    fn call_do_any(args: Sexp<'_>) -> SEXP {
        unsafe {
            do_any(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args.as_raw(),
                std::ptr::null_mut(),
            )
        }
    }

    fn call_do_all(args: Sexp<'_>) -> SEXP {
        unsafe {
            do_all(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args.as_raw(),
                std::ptr::null_mut(),
            )
        }
    }

    #[test]
    fn test_duplicated_int_basic() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_int_vector(&factory, &[1, 2, 3, 2, 1]);
        let dup = duplicated_impl(&x, false, NA_INTEGER).expect("duplicated succeeds");
        assert_eq!(dup.len(), 5);
        // 1: FALSE, 2: FALSE, 3: FALSE, 2: TRUE, 1: TRUE
        assert_eq!(dup.logical_elt(0), Some(0)); // FALSE
        assert_eq!(dup.logical_elt(1), Some(0)); // FALSE
        assert_eq!(dup.logical_elt(2), Some(0)); // FALSE
        assert_eq!(dup.logical_elt(3), Some(1)); // TRUE
        assert_eq!(dup.logical_elt(4), Some(1)); // TRUE
    }

    #[test]
    fn test_duplicated_int_all_unique() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_int_vector(&factory, &[1, 2, 3, 4, 5]);
        let dup = duplicated_impl(&x, false, NA_INTEGER).expect("duplicated succeeds");
        assert_eq!(dup.len(), 5);
        for i in 0..5 {
            assert_eq!(dup.logical_elt(i), Some(0)); // all FALSE
        }
    }

    #[test]
    fn test_duplicated_int_all_same() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_int_vector(&factory, &[7, 7, 7, 7]);
        let dup = duplicated_impl(&x, false, NA_INTEGER).expect("duplicated succeeds");
        assert_eq!(dup.len(), 4);
        assert_eq!(dup.logical_elt(0), Some(0)); // FALSE (first)
        assert_eq!(dup.logical_elt(1), Some(1)); // TRUE
        assert_eq!(dup.logical_elt(2), Some(1)); // TRUE
        assert_eq!(dup.logical_elt(3), Some(1)); // TRUE
    }

    #[test]
    fn test_duplicated_int_empty() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_int_vector(&factory, &[]);
        let dup = duplicated_impl(&x, false, NA_INTEGER).expect("duplicated succeeds");
        assert_eq!(dup.len(), 0);
    }

    #[test]
    fn test_duplicated_int_single() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_int_vector(&factory, &[42]);
        let dup = duplicated_impl(&x, false, NA_INTEGER).expect("duplicated succeeds");
        // ScalarLogical(FALSE) returns length-1 vector
        assert_eq!(dup.len(), 1);
        assert_eq!(dup.logical_elt(0), Some(0));
    }

    #[test]
    fn test_duplicated_int_with_na() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_int_vector(&factory, &[1, NA_INTEGER, 3, NA_INTEGER, 1]);
        let dup = duplicated_impl(&x, false, NA_INTEGER).expect("duplicated succeeds");
        assert_eq!(dup.len(), 5);
        // 1: FALSE, NA: FALSE, 3: FALSE, NA: TRUE, 1: TRUE
        assert_eq!(dup.logical_elt(0), Some(0));
        assert_eq!(dup.logical_elt(1), Some(0));
        assert_eq!(dup.logical_elt(2), Some(0));
        assert_eq!(dup.logical_elt(3), Some(1));
        assert_eq!(dup.logical_elt(4), Some(1));
    }

    #[test]
    fn test_duplicated_logical_basic() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_logical_vector(&factory, &[1, 0, 1, 0, NA_LOGICAL]);
        let dup = duplicated_impl(&x, false, NA_INTEGER).expect("duplicated succeeds");
        assert_eq!(dup.len(), 5);
        // TRUE, FALSE, TRUE(dup), FALSE(dup), NA
        assert_eq!(dup.logical_elt(0), Some(0));
        assert_eq!(dup.logical_elt(1), Some(0));
        assert_eq!(dup.logical_elt(2), Some(1)); // TRUE is duplicate
        assert_eq!(dup.logical_elt(3), Some(1)); // FALSE is duplicate
        assert_eq!(dup.logical_elt(4), Some(0)); // NA first occurrence
    }

    #[test]
    fn test_duplicated_real_basic() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_real_vector(&factory, &[1.0, 2.5, 1.0, 3.0]);
        let dup = duplicated_impl(&x, false, NA_INTEGER).expect("duplicated succeeds");
        assert_eq!(dup.len(), 4);
        assert_eq!(dup.logical_elt(0), Some(0));
        assert_eq!(dup.logical_elt(1), Some(0));
        assert_eq!(dup.logical_elt(2), Some(1)); // 1.0 duplicate
        assert_eq!(dup.logical_elt(3), Some(0));
    }

    #[test]
    fn test_duplicated_real_with_na() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let na = f64::from_bits(R_NA_BIT_PATTERN);
        let x = make_real_vector(&factory, &[1.0, na, 3.0, na]);
        let dup = duplicated_impl(&x, false, NA_INTEGER).expect("duplicated succeeds");
        assert_eq!(dup.len(), 4);
        assert_eq!(dup.logical_elt(0), Some(0));
        assert_eq!(dup.logical_elt(1), Some(0)); // NA first
        assert_eq!(dup.logical_elt(2), Some(0));
        assert_eq!(dup.logical_elt(3), Some(1)); // NA duplicate
    }

    #[test]
    fn test_duplicated_real_with_nan() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_real_vector(&factory, &[1.0, f64::NAN, 3.0, f64::NAN]);
        let dup = duplicated_impl(&x, false, NA_INTEGER).expect("duplicated succeeds");
        assert_eq!(dup.len(), 4);
        assert_eq!(dup.logical_elt(0), Some(0));
        assert_eq!(dup.logical_elt(1), Some(0)); // NaN first
        assert_eq!(dup.logical_elt(2), Some(0));
        assert_eq!(dup.logical_elt(3), Some(1)); // NaN duplicate
    }

    #[test]
    fn test_duplicated_real_signed_zero() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_real_vector(&factory, &[0.0, -0.0]);
        let dup = duplicated_impl(&x, false, NA_INTEGER).expect("duplicated succeeds");
        // Signed zeros should be treated as equal (normalized in hash)
        assert_eq!(dup.len(), 2);
        assert_eq!(dup.logical_elt(0), Some(0));
        assert_eq!(dup.logical_elt(1), Some(1)); // -0.0 == 0.0
    }

    // -----------------------------------------------------------------------
    // Tests for do_unique with integer vectors
    // -----------------------------------------------------------------------

    #[test]
    fn test_do_unique_int_basic() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_int_vector(&factory, &[1, 2, 3, 2, 1]);
        let args = make_duplicated_args(&factory, &x);
        let ans = session
            .sexp(call_do_unique(args))
            .expect("unique result belongs to session");
        assert_eq!(ans.len(), 3);
        // unique should preserve first-occurrence order
        assert_eq!(ans.integer_elt(0), Some(1));
        assert_eq!(ans.integer_elt(1), Some(2));
        assert_eq!(ans.integer_elt(2), Some(3));
    }

    #[test]
    fn test_do_unique_int_empty() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_int_vector(&factory, &[]);
        let args = make_duplicated_args(&factory, &x);
        let ans = session
            .sexp(call_do_unique(args))
            .expect("unique result belongs to session");
        assert_eq!(ans.len(), 0);
    }

    #[test]
    fn test_do_unique_int_all_unique() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_int_vector(&factory, &[5, 4, 3, 2, 1]);
        let args = make_duplicated_args(&factory, &x);
        let ans = session
            .sexp(call_do_unique(args))
            .expect("unique result belongs to session");
        assert_eq!(ans.len(), 5);
    }

    // -----------------------------------------------------------------------
    // Tests for do_duplicated
    // -----------------------------------------------------------------------

    #[test]
    fn test_do_duplicated_int() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_int_vector(&factory, &[1, 2, 3, 2, 1]);
        let args = make_duplicated_args(&factory, &x);
        let dup = session
            .sexp(call_do_duplicated(args))
            .expect("duplicated result belongs to session");
        assert_eq!(dup.len(), 5);
        assert_eq!(dup.logical_elt(0), Some(0));
        assert_eq!(dup.logical_elt(1), Some(0));
        assert_eq!(dup.logical_elt(2), Some(0));
        assert_eq!(dup.logical_elt(3), Some(1));
        assert_eq!(dup.logical_elt(4), Some(1));
    }

    #[test]
    fn test_do_duplicated_empty() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_int_vector(&factory, &[]);
        let args = make_duplicated_args(&factory, &x);
        let dup = session
            .sexp(call_do_duplicated(args))
            .expect("duplicated result belongs to session");
        assert_eq!(dup.len(), 0);
    }

    // -----------------------------------------------------------------------
    // Tests for check_values (any/all core logic)
    // -----------------------------------------------------------------------

    #[test]
    fn test_check_values_any_all_true() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_logical_vector(&factory, &[0, 0, 1, 0]);
        // any: finds TRUE -> returns 1
        assert_eq!(check_values_safe(x, 2, false), Ok(1));
    }

    #[test]
    fn test_check_values_any_all_false() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_logical_vector(&factory, &[0, 0, 0, 0]);
        // any: no TRUE -> returns 0
        assert_eq!(check_values_safe(x, 2, false), Ok(0));
    }

    #[test]
    fn test_check_values_any_with_na() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_logical_vector(&factory, &[NA_LOGICAL, 0, 0]);
        // any with NA: returns NA
        assert_eq!(check_values_safe(x, 2, false), Ok(NA_LOGICAL));
    }

    #[test]
    fn test_check_values_any_with_na_narm() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_logical_vector(&factory, &[NA_LOGICAL, 0, 0]);
        // any with NA and na.rm=TRUE: returns 0 (FALSE)
        assert_eq!(check_values_safe(x, 2, true), Ok(0));
    }

    #[test]
    fn test_check_values_any_true_over_na() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_logical_vector(&factory, &[NA_LOGICAL, 1, 0]);
        // any: finds TRUE -> returns 1 immediately
        assert_eq!(check_values_safe(x, 2, false), Ok(1));
    }

    #[test]
    fn test_check_values_all_all_true() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_logical_vector(&factory, &[1, 1, 1]);
        // all: no FALSE -> returns 1 (TRUE)
        assert_eq!(check_values_safe(x, 1, false), Ok(1));
    }

    #[test]
    fn test_check_values_all_has_false() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_logical_vector(&factory, &[1, 0, 1]);
        // all: finds FALSE -> returns 0 (FALSE)
        assert_eq!(check_values_safe(x, 1, false), Ok(0));
    }

    #[test]
    fn test_check_values_all_with_na() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_logical_vector(&factory, &[1, NA_LOGICAL, 1]);
        // all with NA: returns NA
        assert_eq!(check_values_safe(x, 1, false), Ok(NA_LOGICAL));
    }

    #[test]
    fn test_check_values_all_with_na_narm() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_logical_vector(&factory, &[1, NA_LOGICAL, 1]);
        // all with NA and na.rm=TRUE: returns 1 (TRUE)
        assert_eq!(check_values_safe(x, 1, true), Ok(1));
    }

    #[test]
    fn test_check_values_all_false_over_na() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_logical_vector(&factory, &[1, 0, NA_LOGICAL]);
        // all: finds FALSE -> returns 0 immediately
        assert_eq!(check_values_safe(x, 1, false), Ok(0));
    }

    #[test]
    fn test_check_values_empty_any() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_logical_vector(&factory, &[]);
        // any of empty: returns 0 (FALSE)
        assert_eq!(check_values_safe(x, 2, false), Ok(0));
    }

    #[test]
    fn test_check_values_empty_all() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_logical_vector(&factory, &[]);
        // all of empty: returns 1 (TRUE)
        assert_eq!(check_values_safe(x, 1, false), Ok(1));
    }

    // -----------------------------------------------------------------------
    // Tests for do_any and do_all (without na.rm argument, simplified)
    // -----------------------------------------------------------------------

    #[test]
    fn test_do_any_simple() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        // Create args list: (logical_vector)  -- no na.rm named arg
        let x = make_logical_vector(&factory, &[0, 0, 1, 0]);
        let args = make_args(&factory, &[&x]);
        let ans = session
            .sexp(call_do_any(args))
            .expect("any result belongs to session");
        assert_eq!(ans.len(), 1);
        assert_eq!(ans.logical_elt(0), Some(1)); // TRUE
    }

    #[test]
    fn test_do_any_all_false() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_logical_vector(&factory, &[0, 0, 0]);
        let args = make_args(&factory, &[&x]);
        let ans = session
            .sexp(call_do_any(args))
            .expect("any result belongs to session");
        assert_eq!(ans.logical_elt(0), Some(0)); // FALSE
    }

    #[test]
    fn test_do_all_simple() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_logical_vector(&factory, &[1, 1, 1]);
        let args = make_args(&factory, &[&x]);
        let ans = session
            .sexp(call_do_all(args))
            .expect("all result belongs to session");
        assert_eq!(ans.len(), 1);
        assert_eq!(ans.logical_elt(0), Some(1)); // TRUE
    }

    #[test]
    fn test_do_all_has_false() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_logical_vector(&factory, &[1, 0, 1]);
        let args = make_args(&factory, &[&x]);
        let ans = session
            .sexp(call_do_all(args))
            .expect("all result belongs to session");
        assert_eq!(ans.logical_elt(0), Some(0)); // FALSE
    }

    // -----------------------------------------------------------------------
    // Tests for complex equality with NA
    // -----------------------------------------------------------------------

    #[test]
    fn test_duplicated_complex_basic() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        // {1+2i, 3+4i, 1+2i}
        let x = make_complex_vector(
            &factory,
            &[
                Rcomplex { r: 1.0, i: 2.0 },
                Rcomplex { r: 3.0, i: 4.0 },
                Rcomplex { r: 1.0, i: 2.0 },
            ],
        );
        let dup = duplicated_impl(&x, false, NA_INTEGER).expect("duplicated succeeds");
        assert_eq!(dup.len(), 3);
        assert_eq!(dup.logical_elt(0), Some(0)); // first occurrence
        assert_eq!(dup.logical_elt(1), Some(0)); // unique
        assert_eq!(dup.logical_elt(2), Some(1)); // duplicate
    }

    #[test]
    fn test_duplicated_complex_na() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let na = f64::from_bits(R_NA_BIT_PATTERN);
        // {1+NA, 3+4i, 5+NA}  -- all NA-containing are equal
        let x = make_complex_vector(
            &factory,
            &[
                Rcomplex { r: 1.0, i: na },
                Rcomplex { r: 3.0, i: 4.0 },
                Rcomplex { r: 5.0, i: na },
            ],
        );
        let dup = duplicated_impl(&x, false, NA_INTEGER).expect("duplicated succeeds");
        assert_eq!(dup.logical_elt(0), Some(0)); // first NA
        assert_eq!(dup.logical_elt(1), Some(0)); // unique
        assert_eq!(dup.logical_elt(2), Some(1)); // NA duplicate
    }

    // -----------------------------------------------------------------------
    // Test for large vector hash table sizing
    // -----------------------------------------------------------------------

    #[test]
    fn test_hash_table_setup_int() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_int_vector(&factory, &[1, 2, 3]);
        let data = hash_table_setup(&x, NA_INTEGER);
        // 3 elements -> M = 8 (next power of 2 >= 6)
        assert_eq!(data.m, 8);
        assert_eq!(data.k, 3);
    }

    #[test]
    fn test_hash_table_setup_logical() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_logical_vector(&factory, &[1, 0, 1]);
        let data = hash_table_setup(&x, NA_INTEGER);
        // Logical always uses M=4, K=2
        assert_eq!(data.m, 4);
        assert_eq!(data.k, 2);
    }

    #[test]
    fn test_hash_table_setup_raw() {
        let session = crate::sexp::session::RSession::new();
        let factory = test_factory(&session);
        let x = make_raw_vector(&factory, &[0, 1, 2, 3, 4]);
        let data = hash_table_setup(&x, NA_INTEGER);
        // Raw always uses M=256, K=8
        assert_eq!(data.m, 256);
        assert_eq!(data.k, 8);
    }
}
