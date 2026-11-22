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
// Shared kernels
// ---------------------------------------------------------------------------

/// Shell sort of `x` (NA last via `compare`), optionally carrying `indices`
/// along with each element move.
fn shell_sort<T: Copy>(
    x: &mut [T],
    mut indices: Option<&mut [c_int]>,
    compare: impl Fn(T, T, bool) -> c_int,
) {
    let mut gap = 1;
    while gap <= x.len() / 9 {
        gap = 3 * gap + 1;
    }
    while gap > 0 {
        for i in gap..x.len() {
            let value = x[i];
            let index = indices.as_deref().map(|idx| idx[i]);
            let mut j = i;
            while j >= gap && compare(x[j - gap], value, true) > 0 {
                x[j] = x[j - gap];
                if let Some(idx) = indices.as_deref_mut() {
                    idx[j] = idx[j - gap];
                }
                j -= gap;
            }
            x[j] = value;
            if let (Some(idx), Some(index)) = (indices.as_deref_mut(), index) {
                idx[j] = index;
            }
        }
        gap /= 3;
    }
}

// ---------------------------------------------------------------------------
// R_isort — sort integer array (shell sort, NA last)
// ---------------------------------------------------------------------------

/// Sort an integer array in-place using Shell sort.
///
/// NA_INTEGER values sort last.
///
/// # Safety
/// `x` must point to at least `n` valid, exclusively writable `c_int` values
/// for the duration of the call.
pub unsafe fn R_isort(x: *mut c_int, n: c_int) {
    if n <= 1 {
        return;
    }
    // SAFETY: the caller supplies n initialized elements and exclusive access.
    isort_slice(unsafe { std::slice::from_raw_parts_mut(x, n as usize) });
}

// ---------------------------------------------------------------------------
// R_rsort — sort double array (shell sort, NA last)
// ---------------------------------------------------------------------------

/// Sort a double array in-place using Shell sort.
///
/// NaN values sort last.
///
/// # Safety
/// `x` must point to at least `n` valid, exclusively writable `f64` values
/// for the duration of the call.
pub unsafe fn R_rsort(x: *mut f64, n: c_int) {
    if n <= 1 {
        return;
    }
    // SAFETY: the caller supplies n initialized elements and exclusive access.
    rsort_slice(unsafe { std::slice::from_raw_parts_mut(x, n as usize) });
}

// ---------------------------------------------------------------------------
// R_csort — sort complex array (shell sort, NA last)
// ---------------------------------------------------------------------------

/// Sort a complex array in-place using Shell sort.
///
/// Sorts by real part first, then imaginary part. NaN parts sort last.
///
/// # Safety
/// `x` must point to at least `n` valid, exclusively writable `Rcomplex`
/// values for the duration of the call.
pub unsafe fn R_csort(x: *mut Rcomplex, n: c_int) {
    if n <= 1 {
        return;
    }
    // SAFETY: the caller supplies n initialized elements and exclusive access.
    csort_slice(unsafe { std::slice::from_raw_parts_mut(x, n as usize) });
}

// ---------------------------------------------------------------------------
// rsort_with_index — sort double array with index tracking
// ---------------------------------------------------------------------------

/// Sort a double array in-place using Shell sort, tracking the permutation.
///
/// Both `x` and `indx` are reordered. NaN values sort last.
///
/// # Safety
/// `x` must point to at least `n` valid, exclusively writable `f64` values
/// and `indx` to at least `n` valid, exclusively writable `c_int` values;
/// the two buffers must not overlap and must stay live for the whole call.
pub unsafe fn rsort_with_index(x: *mut f64, indx: *mut c_int, n: c_int) {
    if n <= 1 {
        return;
    }
    // SAFETY: both callers supply n initialized elements per buffer, aligned
    // one-to-one, live for the call, with disjoint exclusive write access.
    let (values, indices) = unsafe {
        (
            std::slice::from_raw_parts_mut(x, n as usize),
            std::slice::from_raw_parts_mut(indx, n as usize),
        )
    };
    shell_sort(values, Some(indices), sort_rcmp);
}

// ---------------------------------------------------------------------------
// revsort — reverse sort double array with index (heapsort, descending)
// ---------------------------------------------------------------------------

/// Stock revsort (r-source/src/main/sort.c:359): descending HEAPSORT of
/// `p` with `perm` carried along. The heapsort (not a stable sort) is
/// load-bearing: ProbSampleReplace/ProbSampleNoReplace stream parity
/// depends on its exact permutation for tied probabilities.
pub(crate) fn revsort_with_perm(p: &mut [f64], perm: &mut [c_int], n: usize) {
    if n <= 1 {
        return;
    }
    // Indices kept 1-based like the C (`a--; ib--;`), addressed via -1.
    let mut l = (n >> 1) + 1;
    let mut ir = n;
    loop {
        let ra;
        let ii;
        if l > 1 {
            l -= 1;
            ra = p[l - 1];
            ii = perm[l - 1];
        } else {
            ra = p[ir - 1];
            ii = perm[ir - 1];
            p[ir - 1] = p[0];
            perm[ir - 1] = perm[0];
            ir -= 1;
            if ir == 1 {
                p[0] = ra;
                perm[0] = ii;
                return;
            }
        }
        let mut i = l;
        let mut j = l << 1;
        while j <= ir {
            if j < ir && p[j - 1] > p[j] {
                j += 1;
            }
            if ra > p[j - 1] {
                p[i - 1] = p[j - 1];
                perm[i - 1] = perm[j - 1];
                i = j;
                j += i;
            } else {
                j = ir + 1;
            }
        }
        p[i - 1] = ra;
        perm[i - 1] = ii;
    }
}

/// Sort a double array into descending order using heapsort.
///
/// Reorders `a` in-place and tracks the permutation in `ib`.
/// If `ib` initially contains 1..n, it will contain the permutation afterward.
///
/// Uses 1-based indexing internally (R convention).
///
/// # Safety
/// `a` must point to at least `n` valid, exclusively writable `f64` values
/// and `ib` to at least `n` valid, exclusively writable `c_int` values;
/// the two buffers must not overlap and must stay live for the whole call.
pub unsafe fn revsort(a: *mut f64, ib: *mut c_int, n: c_int) {
    if n <= 1 {
        return;
    }
    // SAFETY: both buffers supply n initialized elements, aligned one-to-one,
    // live for the call, with disjoint exclusive write access.
    let (values, indices) = unsafe {
        (
            std::slice::from_raw_parts_mut(a, n as usize),
            std::slice::from_raw_parts_mut(ib, n as usize),
        )
    };
    revsort_with_perm(values, indices, n as usize);
}

// ---------------------------------------------------------------------------
// Partial sort functions (find k-th smallest element)
// ---------------------------------------------------------------------------

/// Partition `x` around its `k`-th smallest element (quickselect).
fn partial_sort<T: Copy>(x: &mut [T], k: usize, compare: impl Fn(T, T, bool) -> c_int) {
    if k >= x.len() {
        return;
    }
    let mut lo = 0isize;
    let mut hi = (x.len() - 1) as isize;
    let k = k as isize;
    while lo < hi {
        let value = x[k as usize];
        let mut i = lo;
        let mut j = hi;
        while i <= j {
            while compare(x[i as usize], value, true) < 0 {
                i += 1;
            }
            while compare(value, x[j as usize], true) < 0 {
                j -= 1;
            }
            if i <= j {
                x.swap(i as usize, j as usize);
                i += 1;
                j -= 1;
            }
        }
        if j < k { lo = i; }
        if k < i { hi = j; }
    }
}

/// Partial sort for integer arrays — partition around k-th smallest.
///
/// After calling, `x[k]` contains the k-th smallest element (0-based).
/// Elements before k are <= x[k], elements after are >= x[k].
///
/// # Safety
/// `x` must point to at least `n` valid, exclusively writable `c_int` values
/// for the duration of the call.
pub unsafe fn iPsort(x: *mut c_int, n: c_int, k: c_int) {
    if n <= 1 || !(0..n).contains(&k) {
        return;
    }
    // SAFETY: the caller supplies n initialized elements and exclusive access.
    partial_sort(unsafe { std::slice::from_raw_parts_mut(x, n as usize) }, k as usize, sort_icmp);
}

/// Partial sort for double arrays — partition around k-th smallest.
///
/// After calling, `x[k]` contains the k-th smallest element (0-based).
/// NaN values sort last.
///
/// # Safety
/// `x` must point to at least `n` valid, exclusively writable `f64` values
/// for the duration of the call.
pub unsafe fn rPsort(x: *mut f64, n: c_int, k: c_int) {
    if n <= 1 || !(0..n).contains(&k) {
        return;
    }
    // SAFETY: the caller supplies n initialized elements and exclusive access.
    partial_sort(unsafe { std::slice::from_raw_parts_mut(x, n as usize) }, k as usize, sort_rcmp);
}

/// Partial sort for complex arrays — partition around k-th smallest.
///
/// After calling, `x[k]` contains the k-th smallest element (0-based).
/// NaN parts sort last.
///
/// # Safety
/// `x` must point to at least `n` valid, exclusively writable `Rcomplex`
/// values for the duration of the call.
pub unsafe fn cPsort(x: *mut Rcomplex, n: c_int, k: c_int) {
    if n <= 1 || !(0..n).contains(&k) {
        return;
    }
    // SAFETY: the caller supplies n initialized elements and exclusive access.
    partial_sort(unsafe { std::slice::from_raw_parts_mut(x, n as usize) }, k as usize, ccmp);
}

// ---------------------------------------------------------------------------
// Rust slice APIs
// ---------------------------------------------------------------------------

/// Sort an integer slice in-place (NA_INTEGER last).
pub fn isort_slice(x: &mut [c_int]) {
    shell_sort(x, None, sort_icmp);
}

/// Sort a double slice in-place (NaN last).
pub fn rsort_slice(x: &mut [f64]) {
    shell_sort(x, None, sort_rcmp);
}

/// Sort a complex slice in-place (NaN parts last).
pub fn csort_slice(x: &mut [Rcomplex]) {
    shell_sort(x, None, ccmp);
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
        assert_eq!(ib, vec![2, 5, 3, 4, 1]);
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
