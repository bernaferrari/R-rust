#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Port of R's src/main/sort.c — array sorting utilities.
//!
//! This module ports the standalone array-based sorting functions that operate
//! on basic C arrays (int, double, complex) without requiring SEXP.
//!
//! Ported standalone functions:
//!   R_isort, R_rsort, R_csort,
//!   rsort_with_index, revsort,
//!   iPsort, rPsort, cPsort,
//!   ccmp (complex comparison)
//!
//! Note: icmp and rcmp are already in qsort.rs; ccmp is defined here.

use std::os::raw::c_int;

use crate::sexp::ffi::{NA_INTEGER, Rcomplex};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Check if a double is NaN.
#[inline]
pub fn ISNAN(x: f64) -> bool {
    x.is_nan()
}

// ---------------------------------------------------------------------------
// Comparison functions (duplicated from qsort.rs for standalone use)
// ---------------------------------------------------------------------------

/// NA-aware integer comparison for sorting.
#[inline]
fn sort_icmp(x: c_int, y: c_int, nalast: bool) -> c_int {
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
        -1
    } else if x > y {
        1
    } else {
        0
    }
}

/// NA-aware double comparison for sorting.
#[inline]
fn sort_rcmp(x: f64, y: f64, nalast: bool) -> c_int {
    let nax = ISNAN(x);
    let nay = ISNAN(y);
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
        -1
    } else if x > y {
        1
    } else {
        0
    }
}

/// NA-aware complex comparison for sorting.
/// Compares by real part first, then imaginary part.
#[inline]
pub fn ccmp(x: Rcomplex, y: Rcomplex, nalast: bool) -> c_int {
    let nax = ISNAN(x.r);
    let nay = ISNAN(y.r);
    if nax && nay {
        return 0;
    }
    if nax {
        return if nalast { 1 } else { -1 };
    }
    if nay {
        return if nalast { -1 } else { 1 };
    }
    if x.r < y.r {
        return -1;
    }
    if x.r > y.r {
        return 1;
    }

    let nax = ISNAN(x.i);
    let nay = ISNAN(y.i);
    if nax && nay {
        return 0;
    }
    if nax {
        return if nalast { 1 } else { -1 };
    }
    if nay {
        return if nalast { -1 } else { 1 };
    }
    if x.i < y.i {
        return -1;
    }
    if x.i > y.i {
        return 1;
    }

    0
}

// ---------------------------------------------------------------------------
// R_isort — sort integer array (shell sort, NA last)
// ---------------------------------------------------------------------------

/// Sort an integer array in-place using Shell sort.
///
/// NA_INTEGER values sort last.
///
/// # Safety
/// `x` must point to at least `n` valid `c_int` values.
pub unsafe fn R_isort(x: *mut c_int, n: c_int) {
    unsafe {
        if n <= 1 {
            return;
        }

        let nalast = true;
        let mut h: c_int = 1;
        while h <= n / 9 {
            h = 3 * h + 1;
        }

        while h > 0 {
            let gap = h as isize;
            for i in gap..n as isize {
                let v = *x.add(i as usize);
                let mut j = i;
                while j >= gap && sort_icmp(*x.add((j - gap) as usize), v, nalast) > 0 {
                    *x.add(j as usize) = *x.add((j - gap) as usize);
                    j -= gap;
                }
                *x.add(j as usize) = v;
            }
            h /= 3;
        }
    }
}

// ---------------------------------------------------------------------------
// R_rsort — sort double array (shell sort, NA last)
// ---------------------------------------------------------------------------

/// Sort a double array in-place using Shell sort.
///
/// NaN values sort last.
///
/// # Safety
/// `x` must point to at least `n` valid `f64` values.
pub unsafe fn R_rsort(x: *mut f64, n: c_int) {
    unsafe {
        if n <= 1 {
            return;
        }

        let nalast = true;
        let mut h: c_int = 1;
        while h <= n / 9 {
            h = 3 * h + 1;
        }

        while h > 0 {
            let gap = h as isize;
            for i in gap..n as isize {
                let v = *x.add(i as usize);
                let mut j = i;
                while j >= gap && sort_rcmp(*x.add((j - gap) as usize), v, nalast) > 0 {
                    *x.add(j as usize) = *x.add((j - gap) as usize);
                    j -= gap;
                }
                *x.add(j as usize) = v;
            }
            h /= 3;
        }
    }
}

// ---------------------------------------------------------------------------
// R_csort — sort complex array (shell sort, NA last)
// ---------------------------------------------------------------------------

/// Sort a complex array in-place using Shell sort.
///
/// Sorts by real part first, then imaginary part. NaN parts sort last.
///
/// # Safety
/// `x` must point to at least `n` valid `Rcomplex` values.
pub unsafe fn R_csort(x: *mut Rcomplex, n: c_int) {
    unsafe {
        if n <= 1 {
            return;
        }

        let nalast = true;
        let mut h: c_int = 1;
        while h <= n / 9 {
            h = 3 * h + 1;
        }

        while h > 0 {
            let gap = h as isize;
            for i in gap..n as isize {
                let v = *x.add(i as usize);
                let mut j = i;
                while j >= gap && ccmp(*x.add((j - gap) as usize), v, nalast) > 0 {
                    *x.add(j as usize) = *x.add((j - gap) as usize);
                    j -= gap;
                }
                *x.add(j as usize) = v;
            }
            h /= 3;
        }
    }
}

// ---------------------------------------------------------------------------
// rsort_with_index — sort double array with index tracking
// ---------------------------------------------------------------------------

/// Sort a double array in-place using Shell sort, tracking the permutation.
///
/// Both `x` and `indx` are reordered. NaN values sort last.
///
/// # Safety
/// `x` must point to at least `n` valid `f64` values.
/// `indx` must point to at least `n` valid `c_int` values.
#[unsafe(no_mangle)]
pub unsafe fn rsort_with_index(x: *mut f64, indx: *mut c_int, n: c_int) {
    unsafe {
        if n <= 1 {
            return;
        }

        let nalast = true;
        let mut h: c_int = 1;
        while h <= n / 9 {
            h = 3 * h + 1;
        }

        while h > 0 {
            let gap = h as isize;
            for i in gap..n as isize {
                let v = *x.add(i as usize);
                let iv = *indx.add(i as usize);
                let mut j = i;
                while j >= gap && sort_rcmp(*x.add((j - gap) as usize), v, nalast) > 0 {
                    *x.add(j as usize) = *x.add((j - gap) as usize);
                    *indx.add(j as usize) = *indx.add((j - gap) as usize);
                    j -= gap;
                }
                *x.add(j as usize) = v;
                *indx.add(j as usize) = iv;
            }
            h /= 3;
        }
    }
}

// ---------------------------------------------------------------------------
// revsort — reverse sort double array with index (heapsort, descending)
// ---------------------------------------------------------------------------

/// Sort a double array into descending order using heapsort.
///
/// Reorders `a` in-place and tracks the permutation in `ib`.
/// If `ib` initially contains 1..n, it will contain the permutation afterward.
///
/// Uses 1-based indexing internally (R convention).
///
/// # Safety
/// `a` must point to at least `n` valid `f64` values.
/// `ib` must point to at least `n` valid `c_int` values.
pub unsafe fn revsort(a: *mut f64, ib: *mut c_int, n: c_int) {
    unsafe {
        if n <= 1 {
            return;
        }

        // Convert to 1-based indexing (matches C: a--; ib--;)
        let a = a.sub(1);
        let ib = ib.sub(1);

        let mut l: c_int = (n >> 1) + 1;
        let mut ir: c_int = n;

        loop {
            if l > 1 {
                l -= 1;
                let ra = *a.add(l as usize);
                let ii = *ib.add(l as usize);

                let mut i = l;
                let mut j = l << 1;
                while j <= ir {
                    if j < ir && *a.add(j as usize) > *a.add((j + 1) as usize) {
                        j += 1;
                    }
                    if ra > *a.add(j as usize) {
                        *a.add(i as usize) = *a.add(j as usize);
                        *ib.add(i as usize) = *ib.add(j as usize);
                        i = j;
                        j += i;
                    } else {
                        j = ir + 1;
                    }
                }
                *a.add(i as usize) = ra;
                *ib.add(i as usize) = ii;
            } else {
                let ra = *a.add(ir as usize);
                let ii = *ib.add(ir as usize);
                *a.add(ir as usize) = *a.add(1);
                *ib.add(ir as usize) = *ib.add(1);

                ir -= 1;
                if ir == 1 {
                    *a.add(1) = ra;
                    *ib.add(1) = ii;
                    return;
                }

                let mut i: c_int = 1;
                let mut j: c_int = 2;
                while j <= ir {
                    if j < ir && *a.add(j as usize) > *a.add((j + 1) as usize) {
                        j += 1;
                    }
                    if ra > *a.add(j as usize) {
                        *a.add(i as usize) = *a.add(j as usize);
                        *ib.add(i as usize) = *ib.add(j as usize);
                        i = j;
                        j += i;
                    } else {
                        j = ir + 1;
                    }
                }
                *a.add(i as usize) = ra;
                *ib.add(i as usize) = ii;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Partial sort functions (find k-th smallest element)
// ---------------------------------------------------------------------------

/// Partial sort for integer arrays — partition around k-th smallest.
///
/// After calling, `x[k]` contains the k-th smallest element (0-based).
/// Elements before k are <= x[k], elements after are >= x[k].
///
/// # Safety
/// `x` must point to at least `n` valid `c_int` values.
pub unsafe fn iPsort(x: *mut c_int, n: c_int, k: c_int) {
    unsafe {
        let mut lo: i64 = 0;
        let mut hi: i64 = (n - 1) as i64;
        let kk = k as i64;
        let nalast = true;

        while lo < hi {
            let v = *x.add(kk as usize);
            let mut i = lo;
            let mut j = hi;
            while i <= j {
                while sort_icmp(*x.add(i as usize), v, nalast) < 0 {
                    i += 1;
                }
                while sort_icmp(v, *x.add(j as usize), nalast) < 0 {
                    j -= 1;
                }
                if i <= j {
                    let w = *x.add(i as usize);
                    *x.add(i as usize) = *x.add(j as usize);
                    *x.add(j as usize) = w;
                    i += 1;
                    j -= 1;
                }
            }
            if j < kk {
                lo = i;
            }
            if kk < i {
                hi = j;
            }
        }
    }
}

/// Partial sort for double arrays — partition around k-th smallest.
///
/// After calling, `x[k]` contains the k-th smallest element (0-based).
/// NaN values sort last.
///
/// # Safety
/// `x` must point to at least `n` valid `f64` values.
pub unsafe fn rPsort(x: *mut f64, n: c_int, k: c_int) {
    unsafe {
        let mut lo: i64 = 0;
        let mut hi: i64 = (n - 1) as i64;
        let kk = k as i64;
        let nalast = true;

        while lo < hi {
            let v = *x.add(kk as usize);
            let mut i = lo;
            let mut j = hi;
            while i <= j {
                while sort_rcmp(*x.add(i as usize), v, nalast) < 0 {
                    i += 1;
                }
                while sort_rcmp(v, *x.add(j as usize), nalast) < 0 {
                    j -= 1;
                }
                if i <= j {
                    let w = *x.add(i as usize);
                    *x.add(i as usize) = *x.add(j as usize);
                    *x.add(j as usize) = w;
                    i += 1;
                    j -= 1;
                }
            }
            if j < kk {
                lo = i;
            }
            if kk < i {
                hi = j;
            }
        }
    }
}

/// Partial sort for complex arrays — partition around k-th smallest.
///
/// After calling, `x[k]` contains the k-th smallest element (0-based).
/// NaN parts sort last.
///
/// # Safety
/// `x` must point to at least `n` valid `Rcomplex` values.
pub unsafe fn cPsort(x: *mut Rcomplex, n: c_int, k: c_int) {
    unsafe {
        let mut lo: i64 = 0;
        let mut hi: i64 = (n - 1) as i64;
        let kk = k as i64;
        let nalast = true;

        while lo < hi {
            let v = *x.add(kk as usize);
            let mut i = lo;
            let mut j = hi;
            while i <= j {
                while ccmp(*x.add(i as usize), v, nalast) < 0 {
                    i += 1;
                }
                while ccmp(v, *x.add(j as usize), nalast) < 0 {
                    j -= 1;
                }
                if i <= j {
                    let w = *x.add(i as usize);
                    *x.add(i as usize) = *x.add(j as usize);
                    *x.add(j as usize) = w;
                    i += 1;
                    j -= 1;
                }
            }
            if j < kk {
                lo = i;
            }
            if kk < i {
                hi = j;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Safe Rust wrappers for testing
// ---------------------------------------------------------------------------

/// Sort an integer slice in-place (NA_INTEGER last).
pub fn isort_slice(x: &mut [c_int]) {
    let n = x.len() as c_int;
    if n <= 0 {
        return;
    }
    unsafe {
        R_isort(x.as_mut_ptr(), n);
    }
}

/// Sort a double slice in-place (NaN last).
pub fn rsort_slice(x: &mut [f64]) {
    let n = x.len() as c_int;
    if n <= 0 {
        return;
    }
    unsafe {
        R_rsort(x.as_mut_ptr(), n);
    }
}

/// Sort a complex slice in-place (NaN parts last).
pub fn csort_slice(x: &mut [Rcomplex]) {
    let n = x.len() as c_int;
    if n <= 0 {
        return;
    }
    unsafe {
        R_csort(x.as_mut_ptr(), n);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_isort() {
        let mut x = vec![3, 1, 4, 1, 5, 9, 2, 6];
        isort_slice(&mut x);
        assert_eq!(x, vec![1, 1, 2, 3, 4, 5, 6, 9]);
    }

    #[test]
    fn test_isort_with_na() {
        let mut x = vec![3, NA_INTEGER, 1, NA_INTEGER, 5];
        isort_slice(&mut x);
        assert_eq!(x[0], 1);
        assert_eq!(x[1], 3);
        assert_eq!(x[2], 5);
        assert_eq!(x[3], NA_INTEGER);
        assert_eq!(x[4], NA_INTEGER);
    }

    #[test]
    fn test_rsort() {
        let mut x = vec![3.0, 1.0, 4.0, 1.0, 5.0, 9.0, 2.0, 6.0];
        rsort_slice(&mut x);
        assert_eq!(x, vec![1.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 9.0]);
    }

    #[test]
    fn test_rsort_with_nan() {
        let mut x = vec![3.0, f64::NAN, 1.0, f64::NAN, 5.0];
        rsort_slice(&mut x);
        assert_eq!(x[0], 1.0);
        assert_eq!(x[1], 3.0);
        assert_eq!(x[2], 5.0);
        assert!(x[3].is_nan());
        assert!(x[4].is_nan());
    }

    #[test]
    fn test_csort() {
        let mut x = vec![
            Rcomplex { r: 3.0, i: 1.0 },
            Rcomplex { r: 1.0, i: 2.0 },
            Rcomplex { r: 3.0, i: 0.0 },
        ];
        csort_slice(&mut x);
        assert_eq!(x[0].r, 1.0);
        assert_eq!(x[1].r, 3.0);
        assert_eq!(x[1].i, 0.0); // smaller imaginary part first
        assert_eq!(x[2].r, 3.0);
        assert_eq!(x[2].i, 1.0);
    }

    #[test]
    fn test_rsort_with_index() {
        let mut x = vec![3.0, 1.0, 4.0, 1.0, 5.0];
        // Initialize idx to original positions (1-based, per R convention)
        let mut idx = vec![1i32, 2, 3, 4, 5];
        unsafe {
            rsort_with_index(x.as_mut_ptr(), idx.as_mut_ptr(), 5);
        }
        assert_eq!(x, vec![1.0, 1.0, 3.0, 4.0, 5.0]);
        // idx should contain the original 1-based positions of sorted elements
        let sorted: Vec<f64> = idx
            .iter()
            .map(|&i| [3.0, 1.0, 4.0, 1.0, 5.0][(i - 1) as usize])
            .collect();
        assert_eq!(sorted, vec![1.0, 1.0, 3.0, 4.0, 5.0]);
    }

    #[test]
    fn test_revsort() {
        let mut a = vec![1.0, 5.0, 3.0, 2.0, 4.0];
        let mut ib = vec![1i32, 2, 3, 4, 5];
        unsafe {
            revsort(a.as_mut_ptr(), ib.as_mut_ptr(), 5);
        }
        assert_eq!(a, vec![5.0, 4.0, 3.0, 2.0, 1.0]);
    }

    #[test]
    fn test_rPsort() {
        let mut x = vec![5.0, 3.0, 1.0, 4.0, 2.0];
        unsafe {
            rPsort(x.as_mut_ptr(), 5, 2);
        }
        assert_eq!(x[2], 3.0); // 3rd smallest (0-based)
        // Everything before should be <=
        for i in 0..2 {
            assert!(x[i] <= 3.0);
        }
        // Everything after should be >=
        for i in 3..5 {
            assert!(x[i] >= 3.0);
        }
    }

    #[test]
    fn test_iPsort() {
        let mut x = vec![5, 3, 1, 4, 2];
        unsafe {
            iPsort(x.as_mut_ptr(), 5, 2);
        }
        assert_eq!(x[2], 3); // 3rd smallest (0-based)
    }

    #[test]
    fn test_cPsort() {
        let mut x = vec![
            Rcomplex { r: 5.0, i: 0.0 },
            Rcomplex { r: 3.0, i: 0.0 },
            Rcomplex { r: 1.0, i: 0.0 },
            Rcomplex { r: 4.0, i: 0.0 },
            Rcomplex { r: 2.0, i: 0.0 },
        ];
        unsafe {
            cPsort(x.as_mut_ptr(), 5, 2);
        }
        assert_eq!(x[2].r, 3.0); // 3rd smallest (0-based)
    }

    #[test]
    fn test_empty_arrays() {
        let mut x: Vec<c_int> = vec![];
        isort_slice(&mut x);
        let mut x: Vec<f64> = vec![];
        rsort_slice(&mut x);
    }

    #[test]
    fn test_single_element() {
        let mut x = vec![42i32];
        isort_slice(&mut x);
        assert_eq!(x[0], 42);
    }
}
