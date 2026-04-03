// Ported from R's appl/interv.c
//
// The interv() function was originally Fortran in ../library/modreg/src/bvalue.f
// and part of Hastie and Tibshirani's public domain GAMFIT package.
// Translated by f2c, cleaned up and extended by Martin Maechler.

/// Find the interval that `x` falls into within a sorted array `xt`.
///
/// Returns the index `i` such that `xt[i] <= x < xt[i+1]` (0-indexed).
/// Sets `mflag`:
/// - `-1` if x < xt[0]
/// - `0` if xt[i] <= x < xt[i+1] (normal case)
/// - `1` if x >= xt[n-1]
///
/// # Safety
/// `xt` must point to a valid sorted array of at least `n` doubles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn findInterval(
    xt: *const f64,
    n: std::os::raw::c_int,
    x: f64,
    rightmost_closed: std::os::raw::c_int,
    all_inside: std::os::raw::c_int,
    ilo: std::os::raw::c_int,
    mflag: *mut std::os::raw::c_int,
) -> std::os::raw::c_int {
    unsafe {
        findInterval2(
            xt,
            n,
            x,
            rightmost_closed != 0,
            all_inside != 0,
            false,
            ilo,
            mflag,
        )
    }
}

/// Extended version of findInterval with `left_open` option.
///
/// When `left_open` is true, uses intervals (s, t] instead of [s, t).
///
/// # Safety
/// `xt` must point to a valid sorted array of at least `n` doubles.
/// `mflag` must be a valid pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn findInterval2(
    xt: *const f64,
    n: std::os::raw::c_int,
    x: f64,
    rightmost_closed: bool,
    all_inside: bool,
    left_open: bool,
    ilo: std::os::raw::c_int,
    mflag: *mut std::os::raw::c_int,
) -> std::os::raw::c_int {
    unsafe {
        let n = n as i32;
        if n == 0 {
            *mflag = 0;
            return 0;
        }

        let xt = xt as *const f64; // already const, 0-indexed access

        // Helper macros (as closures for idiomatic Rust)
        // C uses 1-based indexing; we use 0-based
        let x_grtr = |xt_v: f64| -> bool {
            if left_open {
                x > xt_v
            } else {
                x >= xt_v || x > xt_v
            }
        };
        let x_smlr = |xt_v: f64| -> bool {
            if left_open {
                x <= xt_v
            } else {
                x < xt_v || x <= xt_v
            }
        };

        let mut ilo = ilo;
        let mut ihi: i32;

        if ilo <= 0 {
            if x_smlr(*xt.add(0)) {
                *mflag = -1;
                return if all_inside || (rightmost_closed && x == *xt.add(0)) {
                    1
                } else {
                    0
                };
            }
            ilo = 1;
        }
        ihi = ilo + 1;
        if ihi >= n {
            if x_grtr(*xt.add((n - 1) as usize)) {
                *mflag = 1;
                return if all_inside || (rightmost_closed && x == *xt.add((n - 1) as usize)) {
                    n - 1
                } else {
                    n
                };
            }
            if n <= 1 {
                // x < xt[0]
                *mflag = -1;
                return if all_inside || (rightmost_closed && x == *xt.add(0)) {
                    1
                } else {
                    0
                };
            }
            ilo = n - 1;
            ihi = n;
        }

        // ilo and ihi are 0-indexed here
        if x_smlr(*xt.add(ihi as usize)) {
            if x_grtr(*xt.add(ilo as usize)) {
                // lucky: same interval as last time
                *mflag = 0;
                return ilo;
            }
            // x < xt[ilo]: decrease ilo to capture x
            let mut istep: i32 = 1;
            loop {
                ihi = ilo;
                ilo = ihi - istep;
                if ilo <= 0 {
                    break;
                }
                if !left_open && x >= *xt.add(ilo as usize) {
                    break;
                }
                if left_open && x > *xt.add(ilo as usize) {
                    break;
                }
                istep *= 2;
            }
            ilo = 0;
            if x_smlr(*xt.add(0)) {
                *mflag = -1;
                return if all_inside || (rightmost_closed && x == *xt.add(0)) {
                    1
                } else {
                    0
                };
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
                if !left_open && x < *xt.add(ihi as usize) {
                    break;
                }
                if left_open && x <= *xt.add(ihi as usize) {
                    break;
                }
                istep *= 2;
            }
            if x_grtr(*xt.add((n - 1) as usize)) {
                *mflag = 1;
                return if all_inside || (rightmost_closed && x == *xt.add((n - 1) as usize)) {
                    n - 1
                } else {
                    n
                };
            }
            ihi = n;
        }

        // Narrow the interval using bisection
        if !left_open {
            loop {
                let middle = (ilo + ihi) / 2;
                if middle == ilo {
                    *mflag = 0;
                    return ilo;
                }
                if x >= *xt.add(middle as usize) {
                    ilo = middle;
                } else {
                    ihi = middle;
                }
            }
        } else {
            loop {
                let middle = (ilo + ihi) / 2;
                if middle == ilo {
                    *mflag = 0;
                    return ilo;
                }
                if x > *xt.add(middle as usize) {
                    ilo = middle;
                } else {
                    ihi = middle;
                }
            }
        }
    }
}

/// Fortran-compatible entry point (F77_SUB(interv)).
/// Maps to findInterval with int parameters instead of bool.
#[unsafe(no_mangle)]
pub extern "C" fn F77_SUB_interv(
    xt: *const f64,
    n: *mut std::os::raw::c_int,
    x: *mut f64,
    rightmost_closed: *mut std::os::raw::c_int,
    all_inside: *mut std::os::raw::c_int,
    ilo: *mut std::os::raw::c_int,
    mflag: *mut std::os::raw::c_int,
) -> std::os::raw::c_int {
    unsafe { findInterval(xt, *n, *x, *rightmost_closed, *all_inside, *ilo, mflag) }
}
