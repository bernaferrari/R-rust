
use core::ffi::c_int;

use crate::nmath::rng::unif_rand;

/// Algorithm AS 159 Applied Statistics (1981), vol. 30, no. 1
/// Generate random two-way table with given marginal totals.
///
/// Heavily pretty edited by Martin Maechler, Dec 2003.
/// Use double precision for integer multiplication (against overflow).
///
/// Translated from R's C source: r-source/src/library/stats/src/rcont.c
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rcont2(
    nrow: c_int,
    ncol: c_int,
    nrowt: *const c_int,
    ncolt: *const c_int,
    ntotal: c_int,
    fact: *const f64,
    jwork: *mut c_int,
    matrix: *mut c_int,
) {
    let nr_1 = nrow - 1;
    let nc_1 = ncol - 1;
    let mut ib: c_int = 0;

    /* Construct random matrix */
    for j in 0..nc_1 {
        *jwork.add(j as usize) = *ncolt.add(j as usize);
    }

    let mut jc = ntotal;
    for l in 0..nr_1 {
        /* -----  matrix[ l, * ] ----- */
        let mut ia = *nrowt.add(l as usize);
        let mut ic = jc;
        jc -= ia; /* = n_tot - sum(nr[0:l]) */

        for m in 0..nc_1 {
            let id = *jwork.add(m as usize);
            let ie = ic;
            let ii = (ie - ia) - id;
            ic -= id;
            ib = ie - ia;

            if ie == 0 {
                /* Row [l,] is full, fill rest with zero entries */
                for j in m..nc_1 {
                    *matrix.add((l + j * nrow) as usize) = 0;
                }
                ia = 0;
                break;
            }

            let nlm = rcont2_sample(ia, id, ie, ib, ic, ii, fact);

            *matrix.add((l + m * nrow) as usize) = nlm;
            ia -= nlm;
            *jwork.add(m as usize) -= nlm;
        }
        // Last column in row l
        *matrix.add((l + nc_1 * nrow) as usize) = ia;
    }

    /* Compute entries in last row of MATRIX */
    for m in 0..nc_1 {
        *matrix.add((nr_1 + m * nrow) as usize) = *jwork.add(m as usize);
    }

    *matrix.add((nr_1 + nc_1 * nrow) as usize) =
        ib - *matrix.add((nr_1 + (nc_1 - 1) * nrow) as usize);
}

/// Sample a single entry for the rcont2 algorithm using rejection sampling.
/// This is the inner loop of AS 159, corresponding to the `do { ... } while(1)` / `L160` block.
#[inline(never)]
unsafe fn rcont2_sample(
    ia: c_int,
    id: c_int,
    ie: c_int,
    ib: c_int,
    ic: c_int,
    ii: c_int,
    fact: *const f64,
) -> c_int {
    /* Generate pseudo-random number */
    let mut U = unif_rand();
    let mut nlm: c_int;

    loop {
        /* Outer Loop */

        /* Compute conditional expected value of MATRIX(L, M) */
        nlm = (ia as f64 * (id as f64 / ie as f64) + 0.5) as c_int;
        let mut x = (*fact.add(ia as usize)
            + *fact.add(ib as usize)
            + *fact.add(ic as usize)
            + *fact.add(id as usize)
            - *fact.add(ie as usize)
            - *fact.add(nlm as usize)
            - *fact.add((id - nlm) as usize)
            - *fact.add((ia - nlm) as usize)
            - *fact.add((ii + nlm) as usize))
        .exp();
        if x >= U {
            return nlm;
        }
        if x == 0.0 {
            /* Algorithm failure: exp underflow to 0.
            In R this calls error(), but we cannot call R's error from here.
            Return nlm (the conditional expected value) as a fallback. */
            return nlm;
        }

        let mut sumprb = x;
        let mut y = x;

        let mut nll = nlm;
        let mut lsp: bool;

        loop {
            /* Increment entry in row L, column M */
            let j_val = (id - nlm) as f64 * (ia - nlm) as f64;
            lsp = (nlm == ia) || (nlm == id);
            if !lsp {
                nlm += 1;
                x *= j_val / (nlm as f64 * (ii + nlm) as f64);
                sumprb += x;
                if sumprb >= U {
                    return nlm;
                }
            }

            let mut lsm: bool;
            loop {
                /* R_CheckUserInterrupt() is not available in this context;
                we skip it in the Rust port. */

                /* Decrement entry in row L, column M */
                let j_val2 = nll as f64 * (ii + nll) as f64;
                lsm = nll == 0;
                if !lsm {
                    nll -= 1;
                    y *= j_val2 / ((id - nll) as f64 * (ia - nll) as f64);
                    sumprb += y;
                    if sumprb >= U {
                        return nll;
                    }
                    /* else */
                    if !lsp {
                        break; /* to while (!lsp) */
                    }
                }
                if lsm {
                    break;
                }
            }

            if lsp {
                break;
            }
        }

        U = sumprb * unif_rand();
    } // 'Outer Loop'
}
