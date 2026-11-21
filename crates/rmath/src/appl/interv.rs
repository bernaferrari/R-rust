// Ported from R's appl/interv.c
//
// The interv() function was originally Fortran in ../library/modreg/src/bvalue.f
// and part of Hastie and Tibshirani's public domain GAMFIT package.
// Translated by f2c, cleaned up and extended by Martin Maechler.

/// Find the interval that `x` falls into within the sorted slice `xt`.
///
/// Returns `(i, mflag)` where `i` is the index such that
/// `xt[i] <= x < xt[i+1]` (0-indexed) and `mflag` is:
/// - `-1` if x < xt[0]
/// - `0` if xt[i] <= x < xt[i+1] (normal case)
/// - `1` if x >= xt[n-1]
pub fn findInterval(
    xt: &[f64],
    x: f64,
    rightmost_closed: bool,
    all_inside: bool,
    ilo: std::os::raw::c_int,
) -> (std::os::raw::c_int, std::os::raw::c_int) {
    findInterval2(xt, x, rightmost_closed, all_inside, false, ilo)
}

/// Extended version of findInterval with `left_open` option.
///
/// When `left_open` is true, uses intervals (s, t] instead of [s, t).
///
/// Returns `(i, mflag)` with the same contract as [`findInterval`].
pub fn findInterval2(
    xt: &[f64],
    x: f64,
    rightmost_closed: bool,
    all_inside: bool,
    left_open: bool,
    ilo: std::os::raw::c_int,
) -> (std::os::raw::c_int, std::os::raw::c_int) {
    let n = xt.len() as i32;
    if n == 0 {
        return (0, 0);
    }

    // Helper macros (as closures for idiomatic Rust)
    // C uses 1-based indexing; we use 0-based
    let x_grtr = |xt_v: f64| -> bool {
        if left_open {
            x > xt_v
        } else {
            x >= xt_v || x > xt_v
        }
    };
    let x_smlr = |xt_v: f64| -> bool { if left_open { x <= xt_v } else { x < xt_v } };

    let mut ilo = ilo;
    let mut ihi: i32;

    if ilo <= 0 {
        if x_smlr(xt[0]) {
            return (
                if all_inside || (rightmost_closed && x == xt[0]) {
                    1
                } else {
                    0
                },
                -1,
            );
        }
        ilo = 1;
    }
    ihi = ilo + 1;
    if ihi >= n {
        if x_grtr(xt[(n - 1) as usize]) {
            return (
                if all_inside || (rightmost_closed && x == xt[(n - 1) as usize]) {
                    n - 1
                } else {
                    n
                },
                1,
            );
        }
        if n <= 1 {
            // x < xt[0]
            return (
                if all_inside || (rightmost_closed && x == xt[0]) {
                    1
                } else {
                    0
                },
                -1,
            );
        }
        ilo = n - 1;
        ihi = n;
    }

    // ilo and ihi are 0-indexed here
    if x_smlr(xt[ihi as usize]) {
        if x_grtr(xt[ilo as usize]) {
            // lucky: same interval as last time
            return (ilo, 0);
        }
        // x < xt[ilo]: decrease ilo to capture x
        let mut istep: i32 = 1;
        loop {
            ihi = ilo;
            ilo = ihi - istep;
            if ilo <= 0 {
                break;
            }
            if !left_open && x >= xt[ilo as usize] {
                break;
            }
            if left_open && x > xt[ilo as usize] {
                break;
            }
            istep *= 2;
        }
        ilo = 0;
        if x_smlr(xt[0]) {
            return (
                if all_inside || (rightmost_closed && x == xt[0]) {
                    1
                } else {
                    0
                },
                -1,
            );
        }
    } else {
        // x >= xt[ihi]: increase ihi to capture x
        let mut istep: i32 = 1;
        loop {
            ilo = ihi;
            ihi = ilo + istep;
            if ihi >= n {
                break;
            }
            if !left_open && x < xt[ihi as usize] {
                break;
            }
            if left_open && x <= xt[ihi as usize] {
                break;
            }
            istep *= 2;
        }
        if x_grtr(xt[(n - 1) as usize]) {
            return (
                if all_inside || (rightmost_closed && x == xt[(n - 1) as usize]) {
                    n - 1
                } else {
                    n
                },
                1,
            );
        }
        ihi = n;
    }

    // Narrow the interval using bisection
    if !left_open {
        loop {
            let middle = (ilo + ihi) / 2;
            if middle == ilo {
                return (ilo, 0);
            }
            if x >= xt[middle as usize] {
                ilo = middle;
            } else {
                ihi = middle;
            }
        }
    } else {
        loop {
            let middle = (ilo + ihi) / 2;
            if middle == ilo {
                return (ilo, 0);
            }
            if x > xt[middle as usize] {
                ilo = middle;
            } else {
                ihi = middle;
            }
        }
    }
}

/// Fortran-compatible entry point (F77_SUB(interv)).
/// Maps to findInterval with int parameters instead of bool.
///
/// # Safety
///
/// `n`, `x`, `rightmost_closed`, `all_inside`, `ilo` must be valid for one read and `mflag` for one
/// write; `xt` must be valid for reads of `*n` doubles.
pub unsafe extern "C" fn F77_SUB_interv(
    xt: *const f64,
    n: *mut std::os::raw::c_int,
    x: *mut f64,
    rightmost_closed: *mut std::os::raw::c_int,
    all_inside: *mut std::os::raw::c_int,
    ilo: *mut std::os::raw::c_int,
    mflag: *mut std::os::raw::c_int,
) -> std::os::raw::c_int {
    // SAFETY: the caller guarantees `*n` readable doubles behind `xt` and one
    // valid cell behind every scalar pointer (see `# Safety`).
    unsafe {
        let (index, flag) = findInterval(
            std::slice::from_raw_parts(xt, *n as usize),
            *x,
            *rightmost_closed != 0,
            *all_inside != 0,
            *ilo,
        );
        *mflag = flag;
        index
    }
}
