use core::ffi::c_int;

use crate::nmath::rng::unif_rand;

/// Safe slice contract used by the typed stats C-callable bridge. The ABI
/// wrapper validates all scalar-derived extents before it constructs slices.
pub(crate) fn rcont2_slices(
    nrow: c_int,
    ncol: c_int,
    nrowt: &[c_int],
    ncolt: &[c_int],
    ntotal: c_int,
    fact: &[f64],
    jwork: &mut [c_int],
    matrix: &mut [c_int],
) -> Result<(), &'static str> {
    if nrow < 2 || ncol < 2 || ntotal < 0 {
        return Err("rcont2 dimensions must be at least 2 and total non-negative");
    }
    let (nr, nc, total) = (nrow as usize, ncol as usize, ntotal as usize);
    let cells = nr.checked_mul(nc).ok_or("rcont2 table size overflow")?;
    let facts = total
        .checked_add(1)
        .ok_or("rcont2 factorial size overflow")?;
    if nrowt.len() != nr
        || ncolt.len() != nc
        || fact.len() < facts
        || jwork.len() < nc.saturating_sub(1)
        || matrix.len() < cells
        || cells > c_int::MAX as usize
        || cells > isize::MAX as usize / std::mem::size_of::<c_int>()
        || facts > isize::MAX as usize / std::mem::size_of::<f64>()
    {
        return Err("rcont2 buffer lengths do not match dimensions");
    }
    let valid_margins = |margins: &[c_int]| {
        margins.iter().all(|&x| x >= 0)
            && margins.iter().map(|&x| x as i64).sum::<i64>() == ntotal as i64
    };
    if !valid_margins(nrowt) || !valid_margins(ncolt) {
        return Err("rcont2 margins must be non-negative and sum to total");
    }
    if fact[..facts].iter().any(|value| !value.is_finite()) {
        return Err("rcont2 factorial values must be finite");
    }
    // The slice extents and marginal invariants above establish the legacy
    // pointer kernel's bounds and arithmetic preconditions.
    unsafe {
        rcont2(
            nrow,
            ncol,
            nrowt.as_ptr(),
            ncolt.as_ptr(),
            ntotal,
            fact.as_ptr(),
            jwork.as_mut_ptr(),
            matrix.as_mut_ptr(),
        );
    }
    Ok(())
}

/// Exact public stats C-callable ABI from stats_package.h / stats_stubs.h.
/// `.C` is not a valid caller for this scalar-value signature.
///
/// # Safety
/// The caller must provide aligned, valid arrays with lengths `nrow`, `ncol`,
/// `ntotal + 1`, `ncol - 1`, and `nrow * ncol` respectively. The arrays must
/// satisfy the public `S_rcont2` contract and may not alias in ways that cause
/// mutable references to overlap. The active R runtime must be initialized
/// and have RNG state acquired, as required by the GNU callable contract.
pub(crate) unsafe extern "C" fn rcont2_c_callable(
    nrow: c_int,
    ncol: c_int,
    nrowt: *const c_int,
    ncolt: *const c_int,
    ntotal: c_int,
    fact: *const f64,
    jwork: *mut c_int,
    matrix: *mut c_int,
) {
    if nrow < 2
        || ncol < 2
        || ntotal < 0
        || nrowt.is_null()
        || ncolt.is_null()
        || fact.is_null()
        || jwork.is_null()
        || matrix.is_null()
    {
        return;
    }
    let (nr, nc, total) = (nrow as usize, ncol as usize, ntotal as usize);
    let Some(cells) = nr.checked_mul(nc) else {
        return;
    };
    let Some(fact_len) = total.checked_add(1) else {
        return;
    };
    if cells > c_int::MAX as usize
        || cells > isize::MAX as usize / std::mem::size_of::<c_int>()
        || fact_len > isize::MAX as usize / std::mem::size_of::<f64>()
    {
        return;
    }
    // C-callable contract requires arrays sized by the scalar dimensions and
    // total; this boundary checks the scalar invariants before slice creation.
    let rows = unsafe { std::slice::from_raw_parts(nrowt, nr) };
    let cols = unsafe { std::slice::from_raw_parts(ncolt, nc) };
    let facts = unsafe { std::slice::from_raw_parts(fact, fact_len) };
    let work = unsafe { std::slice::from_raw_parts_mut(jwork, nc.saturating_sub(1)) };
    let table = unsafe { std::slice::from_raw_parts_mut(matrix, cells) };
    let _ = rcont2_slices(nrow, ncol, rows, cols, ntotal, facts, work, table);
}

/// Algorithm AS 159 Applied Statistics (1981), vol. 30, no. 1
/// Generate random two-way table with given marginal totals.
///
/// Heavily pretty edited by Martin Maechler, Dec 2003.
/// Use double precision for integer multiplication (against overflow).
///
/// Translated from R's C source: r-source/src/library/stats/src/rcont.c
pub unsafe fn rcont2(
    nrow: c_int,
    ncol: c_int,
    nrowt: *const c_int,
    ncolt: *const c_int,
    ntotal: c_int,
    fact: *const f64,
    jwork: *mut c_int,
    matrix: *mut c_int,
) {
    unsafe {
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
    unsafe {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    type Rcont2Callable = unsafe extern "C" fn(
        c_int,
        c_int,
        *const c_int,
        *const c_int,
        c_int,
        *const f64,
        *mut c_int,
        *mut c_int,
    );

    #[test]
    fn stats_registers_rcont2_with_its_typed_scalar_value_abi() {
        let mut session = crate::sexp::RSession::new_without_default_packages();
        session.with_protected(|| unsafe {
            let package = CString::new("stats").unwrap();
            let name = CString::new("rcont2").unwrap();
            let erased =
                crate::mainutils::rdynload::R_GetCCallable(package.as_ptr(), name.as_ptr())
                    .expect("stats must register the rcont2 C-callable");
            let callable: Rcont2Callable = std::mem::transmute(erased);

            // S_rcont2's public signature uses scalar dimensions/total and
            // array pointers. Exercise a valid 2x2 call with the session RNG.
            let row = [3, 5];
            let col = [2, 6];
            let fact = [
                0.0,
                0.0,
                std::f64::consts::LN_2,
                1.791759469228055,
                3.1780538303479458,
                4.787491742782046,
                6.579251212010101,
                8.525161361065415,
                10.60460290274525,
            ];
            let mut work = [0; 1];
            let mut matrix = [0; 4];
            crate::main::random::GetRNGstate();
            callable(
                2,
                2,
                row.as_ptr(),
                col.as_ptr(),
                8,
                fact.as_ptr(),
                work.as_mut_ptr(),
                matrix.as_mut_ptr(),
            );
            crate::main::random::PutRNGstate();
            assert_eq!(matrix[0] + matrix[2], row[0]);
            assert_eq!(matrix[1] + matrix[3], row[1]);
            assert_eq!(matrix[0] + matrix[1], col[0]);
            assert_eq!(matrix[2] + matrix[3], col[1]);
        });
        session.close();
    }

    #[test]
    fn slice_contract_rejects_invalid_margins_before_kernel_entry() {
        let mut work = [0; 1];
        let mut table = [0; 4];
        let fact = [0.0; 5];
        assert!(rcont2_slices(2, 2, &[1, 1], &[2, 1], 4, &fact, &mut work, &mut table).is_err());
    }
}
