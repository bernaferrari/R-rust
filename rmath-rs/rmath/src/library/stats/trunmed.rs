
//! Port of R's Trunmed.c -- running median smoother using a double heap algorithm
//! (Haerdle & Steiger, 1995, DOI:10.2307/2986349).
//!
//! The implementation uses 1-based indexing for `window[]` and `nrlist[]`,
//! matching the original C code, to minimize translation errors.
//! `outlist[]` is 0-based as in the original.

/// Swap positions `l` and `r` in `window[]` and `nrlist[]`.
/// `l` and `r` are 1-based indices.
#[inline]
unsafe fn swap(l: i64, r: i64, window: &mut [f64], outlist: &mut [i64], nrlist: &mut [i64]) {
    let tmp = window[l as usize];
    window[l as usize] = window[r as usize];
    window[r as usize] = tmp;

    let nl = nrlist[l as usize];
    let nr = nrlist[r as usize];
    nrlist[l as usize] = nr;
    outlist[nr as usize] = l;
    nrlist[r as usize] = nl;
    outlist[nl as usize] = r;
}

/// Heap sift-up (max heap). Used only in `R_heapsort`.
/// `l` and `r` are 1-based indices.
unsafe fn siftup(mut l: i64, r: i64, window: &mut [f64], outlist: &mut [i64], nrlist: &mut [i64]) {
    let mut i = l;
    let nrold = nrlist[i as usize];
    let x = window[i as usize];
    loop {
        let j = 2 * i;
        if j > r {
            break;
        }
        let mut j = j;
        if j < r && window[j as usize] < window[(j + 1) as usize] {
            j += 1;
        }
        if x >= window[j as usize] {
            break;
        }
        window[i as usize] = window[j as usize];
        outlist[nrlist[j as usize] as usize] = i;
        nrlist[i as usize] = nrlist[j as usize];
        i = j;
    }
    window[i as usize] = x;
    outlist[nrold as usize] = i;
    nrlist[i as usize] = nrold;
}

/// Heap sort window[low..up] (1-based indices).
unsafe fn R_heapsort(
    low: i64,
    up: i64,
    window: &mut [f64],
    outlist: &mut [i64],
    nrlist: &mut [i64],
) {
    let mut l = (up / 2) + 1;
    let mut u = up;
    while l > low {
        l -= 1;
        siftup(l, u, window, outlist, nrlist);
    }
    while u > low {
        swap(l, u, window, outlist, nrlist);
        u -= 1;
        siftup(l, u, window, outlist, nrlist);
    }
}

/// Initialize the tree (double heap) structure.
///
/// `n` = data length, `k` = window size (odd), `k2 = (k-1)/2`.
/// `data` is 0-based, `window` and `nrlist` are 1-based, `outlist` is 0-based.
unsafe fn inittree(
    n: i64,
    k: i64,
    k2: i64,
    data: &[f64],
    window: &mut [f64],
    outlist: &mut [i64],
    nrlist: &mut [i64],
) {
    // Use 1-indexing for window, nrlist, outlist
    let k_usize = k as usize;
    for i in 1..=k_usize {
        window[i] = data[i - 1];
        nrlist[i] = i as i64;
        outlist[i] = i as i64;
    }

    // Sort window[1..k] = data[0..k-1] (only called here)
    R_heapsort(1, k, window, outlist, nrlist);

    let mut big = window[k_usize].abs();
    let w1_abs = window[1].abs();
    if big < w1_abs {
        big = w1_abs;
    }
    // big := max |X[1..k]| (or +BIG if data had NA/NaN, since NaN comparisons return false)

    // Shift sorted window right by k2
    for i in (1..=k_usize).rev() {
        window[i + k2 as usize] = window[i];
        nrlist[i + k2 as usize] = nrlist[i] - 1;
    }
    // outlist[0..k-1] := shift down by 1 and offset by k2
    for i in 0..k_usize {
        outlist[i] = outlist[i + 1] + k2;
    }

    // Maybe increase 'big' from the rest of the data
    for i in k..n {
        let d_abs = data[i as usize].abs();
        if big < d_abs {
            big = d_abs;
        }
    }

    // big == max(|data_i|, i = 0..n-1)
    big = 1.0 + 2.0 * big; // such that -big < data[] < +big

    let k2p1 = k2 + 1;
    // Fill sentinel values: -big on the left, +big on the right
    for i in 0..k2p1 as usize {
        window[i] = -big;
        window[k as usize + k2p1 as usize + i] = big;
    }
}

/// Move element at virtual position `outvirt` to the root, inserting new data.
///
/// `outvirt` is a 0-based virtual position in the upper or lower heap.
/// The root of the double heap is at window[k] (1-based).
unsafe fn toroot(
    mut outvirt: i64,
    k: i64,
    nrnew: i64,
    outnext: i64,
    data: &[f64],
    window: &mut [f64],
    outlist: &mut [i64],
    nrlist: &mut [i64],
) {
    loop {
        let father = outvirt / 2;
        window[(outvirt + k) as usize] = window[(father + k) as usize];
        outlist[nrlist[(father + k) as usize] as usize] = outvirt + k;
        nrlist[(outvirt + k) as usize] = nrlist[(father + k) as usize];
        outvirt = father;
        if father == 0 {
            break;
        }
    }
    window[k as usize] = data[nrnew as usize];
    outlist[outnext as usize] = k;
    nrlist[k as usize] = outnext;
}

/// Sift down in the lower heap (max heap).
///
/// `outvirt` is a 0-based virtual position. Elements are at window[outvirt+k].
unsafe fn downtoleave(
    mut outvirt: i64,
    k: i64,
    window: &mut [f64],
    outlist: &mut [i64],
    nrlist: &mut [i64],
) {
    loop {
        let childl = outvirt * 2;
        let childr = childl - 1;
        let mut childl = childl;
        if window[(childl + k) as usize] < window[(childr + k) as usize] {
            childl = childr;
        }
        if window[(outvirt + k) as usize] >= window[(childl + k) as usize] {
            break;
        }
        swap(outvirt + k, childl + k, window, outlist, nrlist);
        outvirt = childl;
    }
}

/// Sift up in the upper heap (min heap).
///
/// `outvirt` is a 0-based virtual position. Elements are at window[outvirt+k].
unsafe fn uptoleave(
    mut outvirt: i64,
    k: i64,
    window: &mut [f64],
    outlist: &mut [i64],
    nrlist: &mut [i64],
) {
    loop {
        let childl = outvirt * 2;
        let childr = childl + 1;
        let mut childl = childl;
        if window[(childl + k) as usize] > window[(childr + k) as usize] {
            childl = childr;
        }
        if window[(outvirt + k) as usize] <= window[(childl + k) as usize] {
            break;
        }
        swap(outvirt + k, childl + k, window, outlist, nrlist);
        outvirt = childl;
    }
}

/// Upper-out, upper-in: element left upper heap, new element enters upper heap.
/// Sift up to restore min-heap property, then bubble up toward root.
unsafe fn upperoutupperin(
    mut outvirt: i64,
    k: i64,
    window: &mut [f64],
    outlist: &mut [i64],
    nrlist: &mut [i64],
) {
    uptoleave(outvirt, k, window, outlist, nrlist);
    let mut father = outvirt / 2;
    while window[(outvirt + k) as usize] < window[(father + k) as usize] {
        swap(outvirt + k, father + k, window, outlist, nrlist);
        outvirt = father;
        father = outvirt / 2;
    }
}

/// Upper-out, down-in: element left upper heap, new element enters lower heap.
/// Move to root, then swap root with lower heap max if needed.
unsafe fn upperoutdownin(
    outvirt: i64,
    k: i64,
    nrnew: i64,
    outnext: i64,
    data: &[f64],
    window: &mut [f64],
    outlist: &mut [i64],
    nrlist: &mut [i64],
) {
    toroot(outvirt, k, nrnew, outnext, data, window, outlist, nrlist);
    if window[k as usize] < window[(k - 1) as usize] {
        swap(k, k - 1, window, outlist, nrlist);
        downtoleave(-1, k, window, outlist, nrlist);
    }
}

/// Down-out, down-in: element left lower heap, new element enters lower heap.
/// Sift down to restore max-heap property, then bubble down away from root.
unsafe fn downoutdownin(
    mut outvirt: i64,
    k: i64,
    window: &mut [f64],
    outlist: &mut [i64],
    nrlist: &mut [i64],
) {
    downtoleave(outvirt, k, window, outlist, nrlist);
    let mut father = outvirt / 2;
    while window[(outvirt + k) as usize] > window[(father + k) as usize] {
        swap(outvirt + k, father + k, window, outlist, nrlist);
        outvirt = father;
        father = outvirt / 2;
    }
}

/// Down-out, upper-in: element left lower heap, new element enters upper heap.
/// Move to root, then swap root with upper heap min if needed.
unsafe fn downoutupperin(
    outvirt: i64,
    k: i64,
    nrnew: i64,
    outnext: i64,
    data: &[f64],
    window: &mut [f64],
    outlist: &mut [i64],
    nrlist: &mut [i64],
) {
    toroot(outvirt, k, nrnew, outnext, data, window, outlist, nrlist);
    if window[k as usize] > window[(k + 1) as usize] {
        swap(k, k + 1, window, outlist, nrlist);
        uptoleave(1, k, window, outlist, nrlist);
    }
}

/// The element that left the window was the root of the upper heap.
/// Swap root with upper min, then sift up.
unsafe fn wentoutone(k: i64, window: &mut [f64], outlist: &mut [i64], nrlist: &mut [i64]) {
    swap(k, k + 1, window, outlist, nrlist);
    uptoleave(1, k, window, outlist, nrlist);
}

/// The element that left the window was the root of the lower heap.
/// Swap root with lower max, then sift down.
unsafe fn wentouttwo(k: i64, window: &mut [f64], outlist: &mut [i64], nrlist: &mut [i64]) {
    swap(k, k - 1, window, outlist, nrlist);
    downtoleave(-1, k, window, outlist, nrlist);
}

/// Compute the running median of `data` with window size `k`.
///
/// `n` = length of data, `k` = odd window size, `k2 = (k-1)/2`.
/// `median[]` receives the output (same length as data).
/// `end_rule`: 0 = leave end values as original data, 1 = constant end values.
///
/// `window`, `outlist`, `nrlist` are pre-allocated work arrays.
unsafe fn runmedint(
    n: i64,
    k: i64,
    k2: i64,
    data: &[f64],
    median: &mut [f64],
    window: &mut [f64],
    outlist: &mut [i64],
    nrlist: &mut [i64],
    end_rule: i32,
) {
    let mut outnext: i64 = 0;

    if end_rule != 0 {
        // Constant end values
        let mut i: i64 = 0;
        while i <= k2 {
            median[i as usize] = window[k as usize];
            i += 1;
        }
    } else {
        // Leave original values at the beginning
        let mut i: i64 = 0;
        while i < k2 {
            median[i as usize] = data[i as usize];
            i += 1;
        }
        median[k2 as usize] = window[k as usize];
    }

    // Main loop: compute median[k2+1] .. median[n-k2-1]
    let mut i: i64 = k2 + 1;
    while i < n - k2 {
        let out = outlist[outnext as usize];
        let nrnew = i + k2;
        window[out as usize] = data[nrnew as usize];
        let outvirt = out - k;

        if out > k {
            // Element left from the upper heap
            if !data[nrnew as usize].is_nan() && data[nrnew as usize] >= window[k as usize] {
                upperoutupperin(outvirt, k, window, outlist, nrlist);
            } else {
                upperoutdownin(outvirt, k, nrnew, outnext, data, window, outlist, nrlist);
            }
        } else if out < k {
            // Element left from the lower heap
            if data[nrnew as usize].is_nan() || data[nrnew as usize] < window[k as usize] {
                downoutdownin(outvirt, k, window, outlist, nrlist);
            } else {
                downoutupperin(outvirt, k, nrnew, outnext, data, window, outlist, nrlist);
            }
        } else if window[k as usize] > window[(k + 1) as usize] {
            // Element at root went out, upper heap min needs promotion
            wentoutone(k, window, outlist, nrlist);
        } else if window[k as usize] < window[(k - 1) as usize] {
            // Element at root went out, lower heap max needs promotion
            wentouttwo(k, window, outlist, nrlist);
        }

        median[i as usize] = window[k as usize];
        outnext = (outnext + 1) % k;
        i += 1;
    }

    if end_rule != 0 {
        let mut i: i64 = n - k2;
        while i < n {
            median[i as usize] = window[k as usize];
            i += 1;
        }
    } else {
        let mut i: i64 = n - k2;
        while i < n {
            median[i as usize] = data[i as usize];
            i += 1;
        }
    }
}

/// Main entry point: compute running median of `x` with window size `k`.
///
/// This is the Rust port of `Trunmed()` from Trunmed.c.
/// It is called from the SEXP wrapper (Srunmed.c equivalent).
///
/// # Safety
/// - `x` and `median` must be valid slices of length `n`.
/// - `k` must be odd and `<= n`.
pub unsafe fn Trunmed(x: &[f64], median: &mut [f64], n: i64, k: i64, end_rule: i32) {
    let k2 = (k - 1) / 2; // k is always odd: k == 2*k2 + 1

    // Allocate work arrays (replaces R_alloc).
    // window[0..2k] and nrlist[0..2k] use 1-based indexing (index 0 is unused sentinel).
    // outlist[0..k] uses 0-based indexing.
    let mut window: Vec<f64> = vec![0.0; (2 * k + 1) as usize];
    let mut nrlist: Vec<i64> = vec![0; (2 * k + 1) as usize];
    let mut outlist: Vec<i64> = vec![0; (k + 1) as usize];

    inittree(n, k, k2, x, &mut window, &mut outlist, &mut nrlist);

    runmedint(
        n,
        k,
        k2,
        x,
        median,
        &mut window,
        &mut outlist,
        &mut nrlist,
        end_rule,
    );
}
