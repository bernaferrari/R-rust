#![forbid(unsafe_code)]
//! GNU's heapsort-with-permutation, canonical in the pure world. The heapsort
//! (not a stable sort) is load-bearing: probability-sampling stream parity
//! depends on its exact permutation for tied probabilities.

use core::ffi::c_int;

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
