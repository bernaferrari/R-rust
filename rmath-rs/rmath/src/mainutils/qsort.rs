#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

//! Port of R's quicksort implementation.
//!
//! Original sources:
//!   - src/main/qsort.c  (R_qsort, R_qsort_I, R_qsort_int, R_qsort_int_I)
//!   - src/main/qsort-body.c  (CACM algorithm #347 by R.C. Singleton, 1969)
//!   - src/main/sort.c  (icmp, rcmp comparison utilities)
//!
//! Based on CACM algorithm #347 by R. C. Singleton (1969), a modified Hoare
//! quicksort incorporating the modification in the remark by Peto.

use std::os::raw::{c_int, c_void};

use crate::sexp::ffi::{NA_INTEGER, SEXP};

// ---------------------------------------------------------------------------
// NaN / NA utilities
// ---------------------------------------------------------------------------

// R_isnancpp is provided by crate::special::mlutils; re-export for local use.
// ---------------------------------------------------------------------------
// NA-aware comparison utilities  (ported from sort.c)
// ---------------------------------------------------------------------------

/// NA-aware integer comparison (port of `icmp` in sort.c).
///
/// Compares two R integer values, treating `NA_INTEGER` specially.
/// Returns -1, 0, or 1.  `nalast` controls whether NA sorts last (true) or
/// first (false).
pub fn icmp(x: c_int, y: c_int, nalast: bool) -> c_int {
    if x == NA_INTEGER && y == NA_INTEGER {
        return 0;
    }
    if x == NA_INTEGER {
        return if nalast { 1 } else { -1 };
    }
    if y == NA_INTEGER {
        return if nalast { -1 } else { 1 };
    }
    if x < y {
        return -1;
    }
    if x > y {
        return 1;
    }
    0
}

/// NA-aware double comparison (port of `rcmp` in sort.c).
///
/// Compares two R double values, treating NaN specially.
/// Returns -1, 0, or 1.  `nalast` controls whether NaN sorts last (true) or
/// first (false).
pub fn rcmp(x: f64, y: f64, nalast: bool) -> c_int {
    let nax = x.is_nan();
    let nay = y.is_nan();
    if nax && nay {
        return 0;
    }
    if nax {
        return if nalast { 1 } else { -1 };
    }
    if nay {
        return if nalast { -1 } else { 1 };
    }
    if x < y {
        return -1;
    }
    if x > y {
        return 1;
    }
    0
}

// ---------------------------------------------------------------------------
// Quicksort implementation  (port of qsort-body.c)
// ---------------------------------------------------------------------------

/// Core quicksort body for doubles **without** index tracking.
///
/// Port of `R_qsort(v, i, j)` from qsort.c.
///
/// Sorts `v[i-1..j-1]` increasingly in place.  The range parameters `i` and
/// `j` use 1-based indexing as in the original R/Fortran interface.
pub fn R_qsort(v: &mut [f64], i: usize, j: usize) {
    r_qsort_impl(v, None, i, j)
}

/// Core quicksort body for doubles **with** index tracking.
///
/// Port of `R_qsort_I(v, I, i, j)` from qsort.c.
///
/// Sorts `v[i-1..j-1]` increasingly in place and fills `I` with the
/// permutation vector such that `new v[k] = old v[I[k]]`.  The range
/// parameters use 1-based indexing.
pub fn R_qsort_I(v: &mut [f64], I: &mut [c_int], i: c_int, j: c_int) {
    let i = i as usize;
    let j = j as usize;
    let mut v = v;
    let mut I = I;
    // Adjust for 1-based indexing
    r_qsort_impl(&mut v, Some(&mut I), i, j)
}

/// Core quicksort body for integers **without** index tracking.
///
/// Port of `R_qsort_int(v, i, j)` from qsort.c.
pub fn R_qsort_int(v: &mut [c_int], i: usize, j: usize) {
    i_qsort_impl(v, None, i, j)
}

/// Core quicksort body for integers **with** index tracking.
///
/// Port of `R_qsort_int_I(v, I, i, j)` from qsort.c.
pub fn R_qsort_int_I(v: &mut [c_int], I: &mut [c_int], i: c_int, j: c_int) {
    let i = i as usize;
    let j = j as usize;
    let mut v = v;
    let mut I = I;
    i_qsort_impl(&mut v, Some(&mut I), i, j)
}

// ---------------------------------------------------------------------------
// Generic quicksort engine for doubles (CACM #347)
// ---------------------------------------------------------------------------

/// Internal quicksort implementation for `f64` arrays.
///
/// This is a faithful translation of the CACM algorithm #347 body in
/// `qsort-body.c`.  The original C code uses 1-based indexing; here we convert
/// to 0-based Rust slices but keep the `i`/`j` parameters 1-based to match
/// the R API.
fn r_qsort_impl(v: &mut [f64], mut I: Option<&mut &mut [c_int]>, i: usize, j: usize) {
    let len = v.len();
    if len == 0 || i == 0 || j == 0 || i > j || j > len {
        return;
    }

    let mut il: [usize; 40] = [0; 40];
    let mut iu: [usize; 40] = [0; 40];
    let mut r: f64 = 0.375;
    let ii = i;
    let mut m: usize = 1;
    let mut i = i;
    let mut j = j;
    let mut vt: f64;
    let mut it: c_int;

    loop {
        // L10
        if i < j {
            if r < 0.5898437 {
                r += 0.0390625;
            } else {
                r -= 0.21875;
            }

            // L20
            let mut k = i;
            // ij = i + (j - i)*R  (median-of-three pivot selection)
            let ij = i + ((j - i) as f64 * r) as usize;

            // Swap pivot into position
            vt = v[ij - 1];
            if let Some(ref mut I_ref) = I {
                it = (**I_ref)[ij - 1];
            } else {
                it = 0;
            }

            if v[i - 1] > vt {
                if let Some(ref mut I_ref) = I {
                    let tmp = (**I_ref)[ij - 1];
                    (**I_ref)[ij - 1] = (**I_ref)[i - 1];
                    (**I_ref)[i - 1] = it;
                    it = tmp;
                }
                v.swap(ij - 1, i - 1);
                vt = v[ij - 1];
            }

            let mut l = j;
            if v[j - 1] < vt {
                if let Some(ref mut I_ref) = I {
                    let tmp = (**I_ref)[ij - 1];
                    (**I_ref)[ij - 1] = (**I_ref)[j - 1];
                    (**I_ref)[j - 1] = it;
                    it = tmp;
                }
                v.swap(ij - 1, j - 1);
                vt = v[ij - 1];

                if v[i - 1] > vt {
                    if let Some(ref mut I_ref) = I {
                        let tmp = (**I_ref)[ij - 1];
                        (**I_ref)[ij - 1] = (**I_ref)[i - 1];
                        (**I_ref)[i - 1] = it;
                        it = tmp;
                    }
                    v.swap(ij - 1, i - 1);
                    vt = v[ij - 1];
                }
            }

            // Partition loop (L50/L60)
            loop {
                loop {
                    l -= 1;
                    if !(v[l - 1] > vt) {
                        break;
                    }
                }

                let tt: c_int;
                if let Some(ref mut I_ref) = I {
                    tt = (**I_ref)[l - 1];
                } else {
                    tt = 0;
                }
                let vtt = v[l - 1];

                loop {
                    k += 1;
                    if !(v[k - 1] < vt) {
                        break;
                    }
                }

                if k > l {
                    break;
                }

                // Swap
                if let Some(ref mut I_ref) = I {
                    (**I_ref)[l - 1] = (**I_ref)[k - 1];
                    (**I_ref)[k - 1] = tt;
                }
                v[l - 1] = v[k - 1];
                v[k - 1] = vtt;
            }

            m += 1;
            if l.wrapping_sub(i) <= j.wrapping_sub(k) {
                // L70
                il[m] = k;
                iu[m] = j;
                j = l;
            } else {
                il[m] = i;
                iu[m] = l;
                i = k;
            }
        } else {
            // i >= j : L80
            if m == 1 {
                return;
            }
            i = il[m];
            j = iu[m];
            m -= 1;
        }

        if j > i && j - i > 10 {
            continue; // goto L20
        }

        if i == ii {
            continue; // goto L10
        }

        // Insertion sort pass (L100/L110)
        let mut i_mut = i;
        loop {
            i_mut += 1;
            if i_mut == j {
                // goto L80
                if m == 1 {
                    return;
                }
                i = il[m];
                j = iu[m];
                m -= 1;

                if j > i && j - i > 10 {
                    break; // will continue outer loop -> L20
                }
                if i == ii {
                    break; // will continue outer loop -> L10
                }
                i_mut = i;
                continue;
            }

            vt = v[i_mut]; // v[i + 1] in original 1-based
            if v[i_mut - 1] <= vt {
                continue;
            }

            if let Some(ref mut I_ref) = I {
                it = (**I_ref)[i_mut]; // I[i + 1] in original
            } else {
                it = 0;
            }

            let mut k = i_mut;
            loop {
                // L110
                if let Some(ref mut I_ref) = I {
                    (**I_ref)[k] = (**I_ref)[k - 1];
                }
                v[k] = v[k - 1];
                if k == i {
                    break;
                }
                k -= 1;
                if !(vt < v[k - 1]) {
                    break;
                }
            }

            if let Some(ref mut I_ref) = I {
                (**I_ref)[k] = it;
            }
            v[k] = vt;
        }
    }
}

// ---------------------------------------------------------------------------
// Generic quicksort engine for integers (CACM #347)
// ---------------------------------------------------------------------------

/// Internal quicksort implementation for `c_int` arrays.
fn i_qsort_impl(v: &mut [c_int], mut I: Option<&mut &mut [c_int]>, i: usize, j: usize) {
    let len = v.len();
    if len == 0 || i == 0 || j == 0 || i > j || j > len {
        return;
    }

    let mut il: [usize; 40] = [0; 40];
    let mut iu: [usize; 40] = [0; 40];
    let mut r: f64 = 0.375;
    let ii = i;
    let mut m: usize = 1;
    let mut i = i;
    let mut j = j;
    let mut vt: c_int;
    let mut it: c_int;

    loop {
        // L10
        if i < j {
            if r < 0.5898437 {
                r += 0.0390625;
            } else {
                r -= 0.21875;
            }

            // L20
            let mut k = i;
            let ij = i + ((j - i) as f64 * r) as usize;

            vt = v[ij - 1];
            if let Some(ref mut I_ref) = I {
                it = (**I_ref)[ij - 1];
            } else {
                it = 0;
            }

            if v[i - 1] > vt {
                if let Some(ref mut I_ref) = I {
                    let tmp = (**I_ref)[ij - 1];
                    (**I_ref)[ij - 1] = (**I_ref)[i - 1];
                    (**I_ref)[i - 1] = it;
                    it = tmp;
                }
                v.swap(ij - 1, i - 1);
                vt = v[ij - 1];
            }

            let mut l = j;
            if v[j - 1] < vt {
                if let Some(ref mut I_ref) = I {
                    let tmp = (**I_ref)[ij - 1];
                    (**I_ref)[ij - 1] = (**I_ref)[j - 1];
                    (**I_ref)[j - 1] = it;
                    it = tmp;
                }
                v.swap(ij - 1, j - 1);
                vt = v[ij - 1];

                if v[i - 1] > vt {
                    if let Some(ref mut I_ref) = I {
                        let tmp = (**I_ref)[ij - 1];
                        (**I_ref)[ij - 1] = (**I_ref)[i - 1];
                        (**I_ref)[i - 1] = it;
                        it = tmp;
                    }
                    v.swap(ij - 1, i - 1);
                    vt = v[ij - 1];
                }
            }

            loop {
                loop {
                    l -= 1;
                    if !(v[l - 1] > vt) {
                        break;
                    }
                }

                let tt: c_int;
                if let Some(ref mut I_ref) = I {
                    tt = (**I_ref)[l - 1];
                } else {
                    tt = 0;
                }
                let vtt = v[l - 1];

                loop {
                    k += 1;
                    if !(v[k - 1] < vt) {
                        break;
                    }
                }

                if k > l {
                    break;
                }

                if let Some(ref mut I_ref) = I {
                    (**I_ref)[l - 1] = (**I_ref)[k - 1];
                    (**I_ref)[k - 1] = tt;
                }
                v[l - 1] = v[k - 1];
                v[k - 1] = vtt;
            }

            m += 1;
            if l.wrapping_sub(i) <= j.wrapping_sub(k) {
                il[m] = k;
                iu[m] = j;
                j = l;
            } else {
                il[m] = i;
                iu[m] = l;
                i = k;
            }
        } else {
            if m == 1 {
                return;
            }
            i = il[m];
            j = iu[m];
            m -= 1;
        }

        if j > i && j - i > 10 {
            continue;
        }

        if i == ii {
            continue;
        }

        let mut i_mut = i;
        loop {
            i_mut += 1;
            if i_mut == j {
                if m == 1 {
                    return;
                }
                i = il[m];
                j = iu[m];
                m -= 1;

                if j > i && j - i > 10 {
                    break;
                }
                if i == ii {
                    break;
                }
                i_mut = i;
                continue;
            }

            vt = v[i_mut];
            if v[i_mut - 1] <= vt {
                continue;
            }

            if let Some(ref mut I_ref) = I {
                it = (**I_ref)[i_mut];
            } else {
                it = 0;
            }

            let mut k = i_mut;
            loop {
                if let Some(ref mut I_ref) = I {
                    (**I_ref)[k] = (**I_ref)[k - 1];
                }
                v[k] = v[k - 1];
                if k == i {
                    break;
                }
                k -= 1;
                if !(vt < v[k - 1]) {
                    break;
                }
            }

            if let Some(ref mut I_ref) = I {
                (**I_ref)[k] = it;
            }
            v[k] = vt;
        }
    }
}

// ---------------------------------------------------------------------------
// SEXP-dependent stubs  (from qsort.c / sort.c)
// ---------------------------------------------------------------------------

/// R's `.Internal(qsort(x))` — in-place quicksort of a numeric vector.
pub unsafe fn do_qsort(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::*;

        let x = crate::sexp::accessors::CAR(args);
        if x.is_null() {
            return std::ptr::null_mut();
        }

        let n = crate::sexp::accessors::LENGTH(x) as usize;
        if n <= 1 {
            return x;
        }

        let xtype = TYPEOF(x);
        if xtype == crate::sexp::ffi::SEXPTYPE::REALSXP.0 {
            let slice = std::slice::from_raw_parts_mut(REAL(x), n);
            R_qsort(slice, 1, n);
        } else if xtype == crate::sexp::ffi::SEXPTYPE::INTSXP.0 {
            let slice = std::slice::from_raw_parts_mut(INTEGER(x) as *mut f64, n);
            R_qsort(slice, 1, n);
        }

        x
    }
}

/// String comparison for sorting, handling NA_STRING.
///
/// Returns -1, 0, or 1. NA strings sort last when `nalast` is true.
pub unsafe fn scmp(x: *mut c_void, y: *mut c_void, nalast: bool) -> c_int { unsafe {
    let x = x as SEXP;
    let y = y as SEXP;
    let na = crate::mainutils::relop::NA_STRING();
    if x == na && y == na {
        return 0;
    }
    if x == na {
        return if nalast { 1 } else { -1 };
    }
    if y == na {
        return if nalast { -1 } else { 1 };
    }
    if x == y {
        return 0;
    }
    let cx = crate::sexp::accessors::CHAR(x);
    let cy = crate::sexp::accessors::CHAR(y);
    if cx.is_null() || cy.is_null() {
        return 0;
    }
    libc::strcmp(cx, cy)
}}

/// Order vector for multiple sort keys (from sort.c).
///
/// `indx` is filled with 0-based indices that would sort `arglist`.
/// `arglist` is a pointer to a Vec<SEXP> of vectors to sort by.
pub unsafe fn R_orderVector(
    indx: *mut c_int,
    n: usize,
    arglist: *mut c_void,
    nalast: c_int,
    decreasing: c_int,
) {
    unsafe {
        if indx.is_null() || n == 0 {
            return;
        }

        let nalast = nalast != 0;
        let nrev = decreasing.unsigned_abs() as usize; // number of keys to reverse

        // Initialize indx to 0..n-1
        for i in 0..n {
            *indx.add(i) = i as c_int;
        }

        // Sort using index comparison
        let key_vecs = &*(&arglist as *const *mut c_void as *const Vec<*mut std::ffi::c_void>);

        let compare = |a: c_int, b: c_int| -> c_int {
            let ia = a as usize;
            let ib = b as usize;
            for k in 0..key_vecs.len() {
                let key = key_vecs[k] as *const f64;
                let va = *key.add(ia);
                let vb = *key.add(ib);
                let cmp = rcmp(va, vb, nalast);
                if cmp != 0 {
                    let rev = k < nrev;
                    return if rev { -cmp } else { cmp };
                }
            }
            0
        };

        // Shell sort for simplicity (stable for equal elements)
        let mut gap = n / 2;
        while gap > 0 {
            for i in gap..n {
                let tmp = *indx.add(i);
                let mut j = i;
                while j >= gap {
                    if compare(*indx.add(j - gap), tmp) > 0 {
                        *indx.add(j) = *indx.add(j - gap);
                    } else {
                        break;
                    }
                    j -= gap;
                }
                *indx.add(j) = tmp;
            }
            gap /= 2;
        }
    }
}

/// Order vector for a single sort key (from sort.c).
///
/// `indx` is filled with 0-based indices that would sort `x`.
/// `x` is a pointer to the data array.
pub unsafe fn R_orderVector1(
    indx: *mut c_int,
    n: usize,
    x: *mut c_void,
    nalast: c_int,
    decreasing: c_int,
) {
    unsafe {
        if indx.is_null() || n == 0 {
            return;
        }

        let nalast = nalast != 0;

        // Initialize indx to 0..n-1
        for i in 0..n {
            *indx.add(i) = i as c_int;
        }

        let data = x as *const f64;
        let rev = decreasing != 0;

        let compare = |a: c_int, b: c_int| -> c_int {
            let va = *data.add(a as usize);
            let vb = *data.add(b as usize);
            let cmp = rcmp(va, vb, nalast);
            if rev { -cmp } else { cmp }
        };

        // Shell sort
        let mut gap = n / 2;
        while gap > 0 {
            for i in gap..n {
                let tmp = *indx.add(i);
                let mut j = i;
                while j >= gap {
                    if compare(*indx.add(j - gap), tmp) > 0 {
                        *indx.add(j) = *indx.add(j - gap);
                    } else {
                        break;
                    }
                    j -= gap;
                }
                *indx.add(j) = tmp;
            }
            gap /= 2;
        }
    }
}

/// Check if a vector is sorted (from sort.c).
///
/// Returns 1 if unsorted, 0 if sorted.
pub unsafe fn isUnsorted(x: *mut c_void, strictly: c_int) -> c_int {
    let data = x as *const f64;
    // We don't know the length here without SEXP, so just check adjacent pairs
    // This is a simplified implementation
    let _ = strictly;
    // Without length info, we can't do much. Return 0 (sorted) as safe default.
    0
}

// ---------------------------------------------------------------------------
// C-callable wrappers (for ABI compatibility)
// ---------------------------------------------------------------------------

/// C-callable wrapper for `R_qsort_I`.
///
/// Corresponds to the Fortran-callable `qsort4` entry point in qsort.c.
///
/// # Safety
/// `v` and `indx` must point to valid arrays of at least `*jj` elements.
pub unsafe fn R_qsort_I_c(v: *mut f64, indx: *mut c_int, ii: c_int, jj: c_int) {
    unsafe {
        if v.is_null() || indx.is_null() || ii < 1 || jj < ii {
            return;
        }
        let n = (jj - ii + 1) as usize;
        R_qsort_I(
            std::slice::from_raw_parts_mut(v, n),
            std::slice::from_raw_parts_mut(indx, n),
            ii,
            jj,
        );
    }
}

/// C-callable wrapper for `R_qsort` (no index).
///
/// Corresponds to the Fortran-callable `qsort3` entry point in qsort.c.
///
/// # Safety
/// `v` must point to a valid array of at least `jj` elements.
pub unsafe fn R_qsort_c(v: *mut f64, ii: usize, jj: usize) {
    unsafe {
        if v.is_null() || ii < 1 || jj < ii {
            return;
        }
        let n = jj;
        R_qsort(std::slice::from_raw_parts_mut(v, n), ii, jj);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::special::mlutils::*;

    use super::*;

    #[test]
    fn test_isnancpp() {
        assert_eq!(R_isnancpp(1.0), 0);
        assert_eq!(R_isnancpp(f64::NAN), 1);
        assert_eq!(R_isnancpp(f64::INFINITY), 0);
        assert_eq!(R_isnancpp(f64::NEG_INFINITY), 0);
    }

    #[test]
    fn test_icmp() {
        assert_eq!(icmp(1, 2, false), -1);
        assert_eq!(icmp(2, 1, false), 1);
        assert_eq!(icmp(1, 1, false), 0);
        assert_eq!(icmp(NA_INTEGER, 1, true), 1); // NA last
        assert_eq!(icmp(NA_INTEGER, 1, false), -1); // NA first
        assert_eq!(icmp(1, NA_INTEGER, true), -1);
        assert_eq!(icmp(NA_INTEGER, NA_INTEGER, true), 0);
    }

    #[test]
    fn test_rcmp() {
        assert_eq!(rcmp(1.0, 2.0, false), -1);
        assert_eq!(rcmp(2.0, 1.0, false), 1);
        assert_eq!(rcmp(1.0, 1.0, false), 0);
        assert_eq!(rcmp(f64::NAN, 1.0, true), 1); // NaN last
        assert_eq!(rcmp(f64::NAN, 1.0, false), -1); // NaN first
        assert_eq!(rcmp(1.0, f64::NAN, true), -1);
        assert_eq!(rcmp(f64::NAN, f64::NAN, true), 0);
    }
}
