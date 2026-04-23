/*
 * Ported from R's Fortran B-spline routines:
 *   - bsplvd.f / bsplvb.f : B-spline value and derivative evaluation
 *   - bvalue.f            : Evaluate B-spline at a point
 *   - sinerp.f            : Inner products between columns of L^{-1}
 *
 * Originally from the GAMFIT package by Hastie and Tibshirani.
 * Translated by f2c, cleaned up and extended by Martin Maechler.
 */

use std::os::raw::c_int;

// ---------------------------------------------------------------------------
// bsplvb: B-spline recurrence relation
// ---------------------------------------------------------------------------

/// Calculate the value of all possibly nonzero B-splines at x.
///
/// Uses the recurrence relation to generate B-spline values of increasing order.
///
/// # Arguments (0-indexed in Rust, 1-indexed in Fortran)
/// - `t`: knot sequence, length >= left + jout
/// - `jhigh`: target order
/// - `index`: 1 = start from scratch, 2 = continue from previous call
/// - `x`: evaluation point
/// - `left`: interval index such that t[left] <= x <= t[left+1] (0-indexed)
/// - `biatx`: output array of length jhigh
///
/// # Safety
/// `t` and `biatx` must be valid pointers to arrays of sufficient length.
pub unsafe fn bsplvb(
    t: *const f64,
    _lent: c_int,
    jhigh: c_int,
    index: c_int,
    x: f64,
    left: c_int,
    biatx: *mut f64,
) {
    const JMAX: usize = 20;
    let mut deltal: [f64; JMAX] = [0.0; JMAX];
    let mut deltar: [f64; JMAX] = [0.0; JMAX];
    // `j` persists across calls when index==2 (static in Fortran)
    thread_local! {
        static BSPLVB_J: std::cell::Cell<c_int> = std::cell::Cell::new(1);
    }

    BSPLVB_J.with(|j_cell| {
        let mut j = j_cell.get();

        if index != 2 {
            j = 1;
            *biatx.add(0) = 1.0;
            if j >= jhigh {
                return;
            }
        }

        loop {
            let jp1 = j + 1;
            let left_u = left as usize;
            let j_u = j as usize;
            let jp1_u = jp1 as usize;

            if jp1_u <= JMAX {
                deltar[j_u] = *t.add(left_u + jp1_u) - x;
                deltal[j_u] = x - *t.add(left_u + 1 - jp1_u);
            }

            let mut saved = 0.0f64;
            for i in 0..j_u {
                let denom = deltar[i] + deltal[jp1_u - 1 - i];
                let term = *biatx.add(i) / denom;
                *biatx.add(i) = saved + deltar[i] * term;
                saved = deltal[jp1_u - 1 - i] * term;
            }
            *biatx.add(jp1_u) = saved;

            j = jp1;
            if j < jhigh {
                // continue
            } else {
                break;
            }
        }

        j_cell.set(j);
    });
}

// ---------------------------------------------------------------------------
// bsplvd: B-spline values and derivatives
// ---------------------------------------------------------------------------

/// Calculate value and derivatives of all B-splines which do not vanish at x.
///
/// # Arguments
/// - `t`: knot array, length >= left + k
/// - `lent`: length of t
/// - `k`: order of B-splines
/// - `x`: evaluation point
/// - `left`: interval index (0-indexed; Fortran is 1-indexed)
/// - `a`: work array of size k*k
/// - `dbiatx`: output, k*nderiv; dbiatx[(m-1)*k + (i-1)] = (m-1)th derivative
/// - `nderiv`: max derivative order requested
///
/// # Safety
/// All pointers must be valid.
pub unsafe fn bsplvd(
    t: *const f64,
    lent: c_int,
    k: c_int,
    x: f64,
    left: c_int,
    a: *mut f64,
    dbiatx: *mut f64,
    nderiv: c_int,
) {
    let mhigh = if nderiv < 1 {
        1
    } else if nderiv < k {
        nderiv
    } else {
        k
    };
    let kp1 = k + 1;
    let k_u = k as usize;

    // Generate B-spline values of order kp1 - mhigh
    bsplvb(t, lent, kp1 - mhigh, 1, x, left, dbiatx);
    if mhigh == 1 {
        return;
    }

    // Generate B-spline values of higher orders and store in columns of dbiatx
    let mut ideriv = mhigh;
    let mut m = 2;
    while m <= mhigh {
        let mut jp1mid: usize = 1;
        let mut j = ideriv as usize;
        while j <= k_u {
            *dbiatx.add(j - 1 + (ideriv as usize - 1) * k_u) = *dbiatx.add(jp1mid - 1);
            jp1mid += 1;
            j += 1;
        }
        ideriv -= 1;
        bsplvb(t, lent, kp1 - ideriv, 2, x, left, dbiatx);
        m += 1;
    }

    // Initialize a: a(j,i) = 0 for j < i, a(i,i) = 1
    // Column-major storage: a[(j-1) + (i-1)*k]
    let mut jlow: usize = 1;
    let mut i = 1usize;
    while i <= k_u {
        let mut j = jlow;
        while j <= k_u {
            *a.add(j - 1 + (i - 1) * k_u) = 0.0;
            j += 1;
        }
        jlow = i;
        *a.add(i - 1 + (i - 1) * k_u) = 1.0;
        i += 1;
    }

    // Generate derivatives by differencing and combining with B-spline values
    let mut m = 2;
    while m <= mhigh {
        let kp1mm = kp1 - m;
        let fkp1mm = kp1mm as f64;
        let mut il = left as usize;
        let mut i = k_u;

        let mut _ldummy: usize = 1;
        while _ldummy <= kp1mm as usize {
            let factor = fkp1mm / (*t.add(il + kp1mm as usize) - *t.add(il));
            let mut j = 1usize;
            while j <= i {
                *a.add(i - 1 + (j - 1) * k_u) =
                    (*a.add(i - 1 + (j - 1) * k_u) - *a.add(i - 2 + (j - 1) * k_u)) * factor;
                j += 1;
            }
            if il == 0 {
                break;
            }
            il -= 1;
            if i == 0 {
                break;
            }
            i -= 1;
            _ldummy += 1;
        }

        // Combine b-coeffs with B-spline values
        i = 1;
        while i <= k_u {
            let mut sum = 0.0f64;
            let jlow = if i > m as usize { i } else { m as usize };
            let mut j = jlow;
            while j <= k_u {
                sum += *a.add(j - 1 + (i - 1) * k_u) * *dbiatx.add(j - 1 + (m as usize - 1) * k_u);
                j += 1;
            }
            *dbiatx.add(i - 1 + (m as usize - 1) * k_u) = sum;
            i += 1;
        }
        m += 1;
    }
}

// ---------------------------------------------------------------------------
// bvalue: Evaluate B-spline at a point
// ---------------------------------------------------------------------------

/// Evaluate the jderiv-th derivative of a spline from its B-representation.
///
/// # Arguments (0-indexed)
/// - `t`: knot sequence, length n + k
/// - `bcoef`: B-coefficient sequence, length n
/// - `n`: length of bcoef
/// - `k`: order of spline
/// - `x`: evaluation point
/// - `jderiv`: derivative order (0 = value)
///
/// # Safety
/// `t` must be valid for n+k elements, `bcoef` for n elements.
pub unsafe fn bvalue(
    t: *const f64,
    bcoef: *const f64,
    n: c_int,
    k: c_int,
    x: f64,
    jderiv: c_int,
) -> f64 {
    const KMAX: usize = 20;
    let mut aj: [f64; KMAX] = [0.0; KMAX];
    let mut dm: [f64; KMAX] = [0.0; KMAX];
    let mut dp: [f64; KMAX] = [0.0; KMAX];

    if jderiv >= k {
        return 0.0;
    }

    let n = n as usize;
    let k = k as usize;
    let jderiv = jderiv as usize;
    let nplusk = n + k;

    // Find interval containing x using interv (0-indexed result)
    let mut i: usize;
    if x != *t.add(n) || *t.add(n) != *t.add(nplusk - 1) {
        let mut mflag: c_int = 0;
        let i_ret = crate::appl::interv::findInterval(t, nplusk as c_int, x, 0, 0, 1, &mut mflag);
        if mflag != 0 {
            return 0.0;
        }
        // findInterval returns 0-indexed; bvalue internally uses 1-indexed
        i = i_ret as usize + 1;
    } else {
        i = n;
    }

    // k = 1: bvalue = bcoef(i)
    let km1 = k - 1;
    if km1 == 0 {
        return *bcoef.add(i - 1);
    }

    // Store k B-spline coefficients and compute dm, dp
    let mut jcmin: usize = 1;
    let imk = i as isize - k as isize;

    if imk >= 0 {
        let imk = imk as usize;
        for j in 1..=km1 {
            dm[j - 1] = x - *t.add(i + 1 - j - 1);
        }
    } else {
        let imk_abs = (-imk) as usize;
        jcmin = 1 + imk_abs;
        for j in 1..=i {
            dm[j - 1] = x - *t.add(i + 1 - j - 1);
        }
        for j in i..=km1 {
            aj[k - j - 1] = 0.0;
            dm[j - 1] = dm[i - 1];
        }
    }

    let mut jcmax: usize = k;
    let nmi = (n as isize) - (i as isize);
    if nmi >= 0 {
        for j in 1..=km1 {
            dp[j - 1] = *t.add(i + j - 1) - x;
        }
    } else {
        let nmi_abs = (-nmi) as usize;
        jcmax = k - nmi_abs;
        for j in 1..=jcmax {
            dp[j - 1] = *t.add(i + j - 1) - x;
        }
        for j in jcmax..=km1 {
            aj[j] = 0.0;
            dp[j - 1] = dp[jcmax - 1];
        }
    }

    // Copy B-spline coefficients
    let imk_idx = if imk >= 0 { imk as usize } else { 0 };
    for jc in jcmin..=jcmax {
        aj[jc - 1] = *bcoef.add(imk_idx + jc - 1);
    }

    // Difference the coefficients jderiv times
    if jderiv >= 1 {
        for _j in 1..=jderiv {
            let kmj = k - _j;
            let fkmj = kmj as f64;
            let mut ilo = kmj;
            for jj in 1..=kmj {
                aj[jj - 1] = ((aj[jj] - aj[jj - 1]) / (dm[ilo - 1] + dp[jj - 1])) * fkmj;
                if ilo == 0 {
                    break;
                }
                ilo -= 1;
            }
        }
    }

    // Compute value at x
    if jderiv != km1 {
        for j in (jderiv + 1)..=km1 {
            let kmj = k - j;
            let mut ilo = kmj;
            for jj in 1..=kmj {
                aj[jj - 1] =
                    (aj[jj] * dm[ilo - 1] + aj[jj - 1] * dp[jj - 1]) / (dm[ilo - 1] + dp[jj - 1]);
                if ilo == 0 {
                    break;
                }
                ilo -= 1;
            }
        }
    }

    aj[0]
}

// ---------------------------------------------------------------------------
// sinerp: Inner products between columns of L^{-1}
// ---------------------------------------------------------------------------

/// Compute inner products between columns of L^{-1} where L = abd is a banded
/// matrix with 3 subdiagonals.
///
/// # Arguments
/// - `abd`: banded matrix, dimension (ld4, nk) — column-major
/// - `ld4`: leading dimension of abd (typically 4)
/// - `nk`: number of columns
/// - `p1ip`: output array, dimension (ld4, nk) — column-major
/// - `p2ip`: output array, dimension (ldnk, nk) — column-major
/// - `ldnk`: leading dimension of p2ip
/// - `flag`: 0 = skip pass 2, nonzero = compute pass 2
///
/// # Safety
/// All pointers must be valid.
pub unsafe fn sinerp(
    abd: *const f64,
    ld4: c_int,
    nk: c_int,
    p1ip: *mut f64,
    p2ip: *mut f64,
    ldnk: c_int,
    flag: c_int,
) {
    let ld4 = ld4 as usize;
    let nk = nk as usize;
    let ldnk = ldnk as usize;

    let mut wjm3 = [0.0f64; 3];
    let mut wjm2 = [0.0f64; 2];
    let mut wjm1 = [0.0f64; 1];

    // Pass 1: compute p1ip
    for ii in 0..nk {
        // j is 1-based Fortran index
        let j = nk - ii;
        let j0 = j - 1; // 0-based column index
        // abd(4,j) = abd[3 + j0*ld4] (row 3 = 4th row 0-based)
        let c0 = 1.0 / *abd.add(3 + j0 * ld4);

        let (c1, c2, c3) = if j0 + 3 < nk {
            // j <= nk-3 in Fortran
            (
                *abd.add(0 + (j0 + 3) * ld4) * c0,
                *abd.add(1 + (j0 + 2) * ld4) * c0,
                *abd.add(2 + (j0 + 1) * ld4) * c0,
            )
        } else if j0 + 2 == nk {
            // j == nk-2
            (
                0.0,
                *abd.add(1 + (j0 + 2) * ld4) * c0,
                *abd.add(2 + (j0 + 1) * ld4) * c0,
            )
        } else if j0 + 1 == nk {
            // j == nk-1
            (0.0, 0.0, *abd.add(2 + (j0 + 1) * ld4) * c0)
        } else {
            // j == nk
            (0.0, 0.0, 0.0)
        };

        // p1ip(1,j) through p1ip(4,j): rows 0-3 of column j0
        *p1ip.add(0 + j0 * ld4) = -(c1 * wjm3[0] + c2 * wjm3[1] + c3 * wjm3[2]);
        *p1ip.add(1 + j0 * ld4) = -(c1 * wjm3[1] + c2 * wjm2[0] + c3 * wjm2[1]);
        *p1ip.add(2 + j0 * ld4) = -(c1 * wjm3[2] + c2 * wjm2[1] + c3 * wjm1[0]);
        *p1ip.add(3 + j0 * ld4) = c0 * c0
            + c1 * c1 * wjm3[0]
            + 2.0 * c1 * c2 * wjm3[1]
            + 2.0 * c1 * c3 * wjm3[2]
            + c2 * c2 * wjm2[0]
            + 2.0 * c2 * c3 * wjm2[1]
            + c3 * c3 * wjm1[0];

        wjm3[0] = wjm2[0];
        wjm3[1] = wjm2[1];
        wjm3[2] = *p1ip.add(1 + j0 * ld4);
        wjm2[0] = wjm1[0];
        wjm2[1] = *p1ip.add(2 + j0 * ld4);
        wjm1[0] = *p1ip.add(3 + j0 * ld4);
    }

    // Pass 2: compute p2ip (only if flag != 0; R always calls with flag=0)
    if flag != 0 {
        for ii in 0..nk {
            let j = nk - ii; // 1-based
            let j0 = j - 1; // 0-based
            for k in 1..=4usize {
                if j0 + k > nk {
                    break;
                }
                *p2ip.add(j0 + (j0 + k - 1) * ldnk) = *p1ip.add(4 - k + j0 * ld4);
            }
        }

        for ii in 0..nk {
            let j = nk - ii; // 1-based
            let j0 = j - 1; // 0-based
            if j0 >= 4 {
                let mut k = j0 - 4;
                loop {
                    let c0 = 1.0 / *abd.add(3 + k * ld4);
                    let c1 = *abd.add(0 + (k + 3) * ld4) * c0;
                    let c2 = *abd.add(1 + (k + 2) * ld4) * c0;
                    let c3 = *abd.add(2 + (k + 1) * ld4) * c0;
                    *p2ip.add(k + j0 * ldnk) = -(c1 * *p2ip.add(k + 3 + j0 * ldnk)
                        + c2 * *p2ip.add(k + 2 + j0 * ldnk)
                        + c3 * *p2ip.add(k + 1 + j0 * ldnk));
                    if k == 0 {
                        break;
                    }
                    k -= 1;
                }
            }
        }
    }
}
