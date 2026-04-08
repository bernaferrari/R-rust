//! Port of R's quicksort from src/main/qsort.c and src/main/qsort-body.c
//!
//! Based on CACM algorithm #347 by R. C. Singleton (1969),
//! a modified Hoare quicksort with Peto's modification.
//!
//! This is a 1-indexed quicksort that optionally produces an index vector.

/// Sorts v[0..n] increasingly using 0-based indexing.
/// Puts into I the permutation vector: new v[k] = old v[I[k]].
/// Only elements [i..j) (0-based) are considered.
///
/// Ported from R's R_qsort_I (src/main/qsort.c).
pub unsafe fn R_qsort_I(v: *mut f64, i: *mut i32, ii: i32, jj: i32) {
    unsafe {
        if ii >= jj {
            return;
        }
        qsort_with_index_impl(
            std::slice::from_raw_parts_mut(v, jj as usize),
            std::slice::from_raw_parts_mut(i, jj as usize),
            ii as usize,
            jj as usize,
        );
    }
}

/// Sorts v[0..n] (integers) increasingly using 0-based indexing.
/// Puts into I the permutation vector.
///
/// Ported from R's R_qsort_int_I.
pub unsafe fn R_qsort_int_I(v: *mut i32, i: *mut i32, ii: i32, jj: i32) {
    unsafe {
        if ii >= jj {
            return;
        }
        qsort_with_index_impl(
            std::slice::from_raw_parts_mut(v, jj as usize),
            std::slice::from_raw_parts_mut(i, jj as usize),
            ii as usize,
            jj as usize,
        );
    }
}

/// Sorts v[0..n] increasingly without index.
///
/// Ported from R's R_qsort.
pub unsafe fn R_qsort(v: *mut f64, ii: usize, jj: usize) {
    unsafe {
        if ii >= jj {
            return;
        }
        qsort_impl(std::slice::from_raw_parts_mut(v, jj), ii, jj);
    }
}

/// Sorts v[0..n] (integers) increasingly without index.
///
/// Ported from R's R_qsort_int.
pub unsafe fn R_qsort_int(v: *mut i32, ii: usize, jj: usize) {
    unsafe {
        if ii >= jj {
            return;
        }
        qsort_impl(std::slice::from_raw_parts_mut(v, jj), ii, jj);
    }
}

/// Generic quicksort with index vector.
///
/// Uses 1-based indexing internally (like the original C code).
fn qsort_with_index_impl<T: PartialOrd + Copy>(v: &mut [T], i: &mut [i32], lo: usize, hi: usize) {
    let n = v.len();
    if n < 2 {
        return;
    }
    // Use 1-based indexing by padding with a dummy element
    // Actually, the C code uses `--v` to shift to 1-based.
    // We'll work with 0-based throughout but adapt the algorithm.
    quicksort_with_index(v, i, lo, hi);
}

fn quicksort_with_index<T: PartialOrd + Copy>(v: &mut [T], i: &mut [i32], lo: usize, hi: usize) {
    let mut il: [usize; 40] = [0; 40];
    let mut iu: [usize; 40] = [0; 40];
    let mut r: f64 = 0.375;
    let mut m: usize = 1;
    let saved_lo = lo;

    let mut ii = lo;
    let mut jj = hi;

    // Main loop (L10 in C)
    loop {
        if ii < jj {
            if r < 0.5898437 {
                r += 0.0390625;
            } else {
                r -= 0.21875;
            }

            // L20: partition
            loop {
                let mut k = ii;
                let ij = ii + ((jj as f64 - ii as f64) * r) as usize;
                let mut it = i[ij];
                let mut vt = v[ij];

                if v[ii] > vt {
                    i[ij] = i[ii];
                    i[ii] = it;
                    let _ = i[ij];
                    v[ij] = v[ii];
                    v[ii] = vt;
                    vt = v[ij];
                }

                let mut l = jj;
                if v[jj] < vt {
                    i[ij] = i[jj];
                    i[jj] = it;
                    it = i[ij];
                    v[ij] = v[jj];
                    v[jj] = vt;
                    vt = v[ij];

                    if v[ii] > vt {
                        i[ij] = i[ii];
                        i[ii] = it;
                        let _ = i[ij];
                        v[ij] = v[ii];
                        v[ii] = vt;
                        vt = v[ij];
                    }
                }

                // L50: partition loop
                loop {
                    loop {
                        l -= 1;
                        if !(v[l] > vt) {
                            break;
                        }
                    }
                    let tt = i[l];
                    let vtt = v[l];

                    loop {
                        k += 1;
                        if !(v[k] < vt) {
                            break;
                        }
                    }

                    if k > l {
                        break;
                    }

                    // Swap
                    i[l] = i[k];
                    i[k] = tt;
                    v[l] = v[k];
                    v[k] = vtt;
                }

                m += 1;
                if l <= ii || jj - k < jj - l {
                    // L70
                    il[m] = k;
                    iu[m] = jj;
                    jj = l;
                } else {
                    il[m] = ii;
                    iu[m] = l;
                    ii = k;
                }

                // Check if subarray is small enough for insertion sort
                if jj - ii <= 10 {
                    break;
                }
            }

            // L80
            if m == 1 {
                break;
            }
            ii = il[m];
            jj = iu[m];
            m -= 1;
        } else {
            // L80
            if m == 1 {
                break;
            }
            ii = il[m];
            jj = iu[m];
            m -= 1;
        }

        // Insertion sort for small arrays (L100)
        if jj - ii <= 10 {
            if ii == saved_lo {
                break;
            }
        }

        // Insertion sort (L100-L110)
        let mut idx = ii;
        loop {
            idx += 1;
            if idx == jj {
                break;
            }
            let it = i[idx];
            let vt = v[idx];
            if v[idx - 1] <= vt {
                continue;
            }
            let mut k = idx;
            loop {
                i[k] = i[k - 1];
                v[k] = v[k - 1];
                k -= 1;
                if k == ii || !(vt < v[k - 1]) {
                    break;
                }
            }
            i[k] = it;
            v[k] = vt;
        }
    }
}

/// Generic quicksort without index vector.
fn qsort_impl<T: PartialOrd + Copy>(v: &mut [T], lo: usize, hi: usize) {
    let n = v.len();
    if n < 2 {
        return;
    }
    quicksort_no_index(v, lo, hi);
}

fn quicksort_no_index<T: PartialOrd + Copy>(v: &mut [T], lo: usize, hi: usize) {
    let mut il: [usize; 40] = [0; 40];
    let mut iu: [usize; 40] = [0; 40];
    let mut r: f64 = 0.375;
    let mut m: usize = 1;
    let saved_lo = lo;

    let mut ii = lo;
    let mut jj = hi;

    loop {
        if ii < jj {
            if r < 0.5898437 {
                r += 0.0390625;
            } else {
                r -= 0.21875;
            }

            loop {
                let mut k = ii;
                let ij = ii + ((jj as f64 - ii as f64) * r) as usize;
                let mut vt = v[ij];

                if v[ii] > vt {
                    v[ij] = v[ii];
                    v[ii] = vt;
                    vt = v[ij];
                }

                let mut l = jj;
                if v[jj] < vt {
                    v[ij] = v[jj];
                    v[jj] = vt;
                    vt = v[ij];

                    if v[ii] > vt {
                        v[ij] = v[ii];
                        v[ii] = vt;
                        vt = v[ij];
                    }
                }

                loop {
                    loop {
                        l -= 1;
                        if !(v[l] > vt) {
                            break;
                        }
                    }
                    let vtt = v[l];

                    loop {
                        k += 1;
                        if !(v[k] < vt) {
                            break;
                        }
                    }

                    if k > l {
                        break;
                    }

                    v[l] = v[k];
                    v[k] = vtt;
                }

                m += 1;
                if l <= ii || jj - k < jj - l {
                    il[m] = k;
                    iu[m] = jj;
                    jj = l;
                } else {
                    il[m] = ii;
                    iu[m] = l;
                    ii = k;
                }

                if jj - ii <= 10 {
                    break;
                }
            }

            if m == 1 {
                break;
            }
            ii = il[m];
            jj = iu[m];
            m -= 1;
        } else {
            if m == 1 {
                break;
            }
            ii = il[m];
            jj = iu[m];
            m -= 1;
        }

        // Insertion sort
        if jj - ii <= 10 {
            if ii == saved_lo {
                break;
            }
        }

        let mut idx = ii;
        loop {
            idx += 1;
            if idx == jj {
                break;
            }
            let vt = v[idx];
            if v[idx - 1] <= vt {
                continue;
            }
            let mut k = idx;
            loop {
                v[k] = v[k - 1];
                k -= 1;
                if k == ii || !(vt < v[k - 1]) {
                    break;
                }
            }
            v[k] = vt;
        }
    }
}
