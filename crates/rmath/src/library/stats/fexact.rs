/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1995--2025  The R Core Team
 *
 *  Ported to Rust from r-source/src/library/stats/src/fexact.c
 */

/*
 *  Based on ACM TOMS643 (1993)
 *
 *  Main ref.:  Mehta & Patel (1986) ALGORITHM 643 FEXACT: FORTRAN ... Fisher's Exact Test .... ACM TOMS
 *              >>>  ../man/fisher.test.Rd  for *all* references
 */

/*
  Fisher's exact test for contingency tables -- usage see below

  fexact.f -- translated by f2c (version 19971204).
  Run through a slightly modified version of MM's f2c-clean.
  Heavily hand-edited by KH and MM.
*/

use std::io::Write;
use std::os::raw::{c_double, c_int};

use crate::main::coerce::coerceVector;
use crate::main::errors::REprintf;
use crate::main::sort::isort_slice;
use crate::nmath::dist::gamma::pgamma;
use crate::sexp::accessors::*;
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::*;
use crate::sexp::memory_ext::R_alloc;
use crate::sexp::protect::protect;

// ---- internal helper functions (not exported) ----

fn prterr(icode: c_int, mes: &str) -> ! {
    let msg = format!("FEXACT error {}.\n{}", icode, mes);
    crate::mainutils::errors::implicit_errorcall_str(
        crate::mainutils::errors::R_getCurrentCall(),
        &msg,
    )
}

fn iwork(iwkmax: i64, iwkpt: &mut i64, number: i64, double: bool) -> usize {
    // Charge the alignment cell so a double and a following integer do not
    // overlap when the cursor was odd.
    let start = if double { (*iwkpt + 1) & !1 } else { *iwkpt };
    let end = start + if double { 2 * number } else { number };
    if end > iwkmax {
        prterr(40, "Out of workspace.");
    }
    *iwkpt = end;
    (if double { start / 2 } else { start }) as usize
}

fn fact_term(fact: &[c_double], idx: c_int, site: &str) -> c_double {
    if idx < 0 {
        prterr(2, &format!("negative count {idx} at {site}"));
    }
    fact[idx as usize]
}

fn f2xact(
    nrow: c_int,
    ncol: c_int,
    table: &[c_int],
    expect: c_double,
    percnt: c_double,
    emin: c_double,
    pre: &mut c_double,
    fact: &mut [c_double],
    ico: &mut [c_int],
    iro: &mut [c_int],
    kyy: &mut [c_int],
    idif: &mut [c_int],
    irn: &mut [c_int],
    key: &mut [c_int],
    ldkey: c_int,
    ipoin: &mut [c_int],
    stp: &mut [c_double],
    ldstp: c_int,
    ifrq: &mut [c_int],
    lp: &mut [c_double],
    sp: &mut [c_double],
    tm: &mut [c_double],
    key2: &mut [c_int],
    iwk: &mut [c_int],
    rwk: &mut [c_double],
    n2_stack: c_int,
) {
    let imax = c_int::MAX;

    let tol: c_double = 3.45254e-7;

    let ch_err_5 = "The hash table key cannot be computed because the largest key\n\
                    is larger than the largest representable int.\n\
                    The algorithm cannot proceed.\n\
                    Reduce the workspace, consider using 'simulate.p.value=TRUE' or another algorithm.";

    let mut i: c_int = 0;
    let mut ii: c_int = 0;
    let mut j: c_int = 0;
    let mut k: c_int = 0;
    let mut n: c_int = 0;
    let mut ifreq: c_int = 0;
    let mut ikkey: c_int = 0;
    let mut ikstp: c_int = 0;
    let mut ikstp2: c_int = 0;
    let mut ipn: c_int = 0;
    let mut ipo: c_int = 0;
    let mut itop: c_int = 0;
    let mut itp: c_int = 0;
    let mut jkey: c_int = 0;
    let mut jstp: c_int = 0;
    let mut jstp2: c_int = 0;
    let mut k1: c_int = 0;
    let mut kb: c_int = 0;
    let mut kd: c_int = 0;
    let mut ks: c_int = 0;
    let mut kval: c_int = 0;
    let mut kmax: c_int = 0;
    let mut last: c_int = 0;
    let mut ntot: c_int = 0;
    let mut nco: c_int = 0;
    let mut nro: c_int = 0;
    let mut nro2: c_int = 0;
    let mut nrb: c_int = 0;

    let mut dspt: c_double = 0.;
    let mut df: c_double = 0.;
    let mut ddf: c_double = 0.;
    let mut drn: c_double = 0.;
    let mut dro: c_double = 0.;
    let mut obs: c_double = 0.;
    let mut obs2: c_double = 0.;
    let mut obs3: c_double = 0.;
    let mut pastp: c_double = 0.;
    let mut pv: c_double = 0.;
    let mut tmp: c_double = 0.;

    let mut ok_f7: bool;
    let nr_gt_nc: bool;
    let maybe_chisq: bool = expect > 0.;
    let mut chisq: bool = false;
    let mut psh: bool;

    // Initialize KEY array (1-based)
    key.fill(-9999);
    key2.fill(-9999);

    // Determine row and column marginals
    nr_gt_nc = nrow > ncol;
    if nr_gt_nc {
        nco = nrow;
    } else {
        nco = ncol;
    }

    // Compute row marginals and total (1-based)
    ntot = 0;
    for idx in 1..=nrow as usize {
        iro[idx - 1] = 0;
        for jdx in 1..=ncol as usize {
            let tval = table[(idx - 1) + (jdx - 1) * nrow as usize];
            iro[idx - 1] += tval;
        }
        ntot += iro[idx - 1];
    }

    // Column marginals (1-based)
    for idx in 1..=ncol as usize {
        ico[idx - 1] = 0;
        for jdx in 1..=nrow as usize {
            ico[idx - 1] += table[(jdx - 1) + (idx - 1) * nrow as usize];
        }
    }

    // Sort marginals
    isort_slice(&mut iro[..nrow as usize]);
    isort_slice(&mut ico[..ncol as usize]);

    // Swap marginals if necessary: ico[1:nco] & iro[1:nro]
    if nr_gt_nc {
        nro = ncol;
        for idx in 1..=nco as usize {
            ii = iro[idx - 1];
            if (idx as c_int) <= nro {
                iro[idx - 1] = ico[idx - 1];
            }
            ico[idx - 1] = ii;
        }
    } else {
        nro = nrow;
    }

    // Get multipliers for stack (1-based)
    kyy[0] = 1;
    let mut idx: usize = 1;
    while idx < nro as usize {
        if iro[idx - 1] < imax / kyy[idx - 1] {
            kyy[idx] = kyy[idx - 1] * (iro[idx - 1] + 1);
        } else {
            prterr(5, ch_err_5);
        }
        idx += 1;
    }

    // Check for Maximum product
    if iro[(nro - 1) as usize] + 1 > imax / kyy[(nro - 1) as usize] {
        prterr(501, ch_err_5);
    }

    // Compute log factorials (0-based for fact)
    fact[0] = 0.;
    fact[1] = 0.;
    if ntot >= 2 {
        fact[2] = (2.0f64).ln();
    }
    let mut fi: usize = 3;
    while fi <= ntot as usize {
        fact[fi] = fact[fi - 1] + (fi as c_double).ln();
        let fj = fi + 1;
        if fj <= ntot as usize {
            fact[fj] = fact[fi] + fact[2] + fact[fj / 2] - fact[fj / 2 - 1];
        }
        fi += 2;
    }

    // Compute obs := observed path length
    obs = tol;
    ntot = 0;
    for jdx in 1..=nco as usize {
        let mut dd: c_double = 0.;
        if nr_gt_nc {
            for idx in 1..=nro as usize {
                dd += fact[table[(jdx - 1) + (idx - 1) * nrow as usize] as usize];
                ntot += table[(jdx - 1) + (idx - 1) * nrow as usize];
            }
        } else {
            let mut ii_us: usize = (jdx - 1) * nrow as usize;
            for idx in 1..=nro as usize {
                dd += fact[table[ii_us] as usize];
                ntot += table[ii_us];
                ii_us += 1;
            }
        }
        obs += fact[ico[jdx - 1] as usize] - dd;
    }

    // Denominator of observed table: DRO
    dro = f9xact(ntot, &iro[..nro as usize], fact);
    *pre = 0.;
    itop = 0;

    // Initialize pointers
    k = nco;
    last = ldkey + 1;
    jkey = ldkey + 1;
    jstp = ldstp + 1;
    jstp2 = ldstp * 3 + 1;
    ikkey = 0;
    ikstp = 0;
    ikstp2 = ldstp << 1;
    ipo = 1;
    ipoin[0] = 1;
    stp[0] = 0.;
    ifrq[0] = 1;
    ifrq[ikstp2 as usize] = -1;

    // Outer_Loop:
    'outer_loop: loop {
        kb = nco - k + 1;
        ks = 0;
        n = ico[(kb - 1) as usize];
        kd = nro + 1;
        kmax = nro;
        // IDIF is the difference in going to the daughter
        idif.fill(0);

        // Generate the first daughter
        loop {
            kd -= 1;
            ntot = std::cmp::min(n, iro[(kd - 1) as usize]);
            idif[(kd - 1) as usize] = ntot;
            if idif[(kmax - 1) as usize] == 0 {
                kmax -= 1;
            }
            n -= ntot;
            if !(n > 0 && kd != 1) {
                break;
            }
        }

        if n != 0 {
            // goto L310
            // Go get a new mother from stage K
            loop {
                if !f6xact(
                    &mut iro[..nro as usize],
                    &kyy[..nro as usize],
                    &mut key[ikkey as usize..(ikkey + ldkey) as usize],
                    &mut last,
                    &mut ipo,
                ) {
                    // Update pointers -- continue outer loop
                    continue 'outer_loop;
                }
                // No additional nodes to process
                k -= 1;
                itop = 0;
                ikkey = jkey - 1;
                ikstp = jstp - 1;
                ikstp2 = jstp2 - 1;
                jkey = ldkey - jkey + 2;
                jstp = ldstp - jstp + 2;
                jstp2 = (ldstp << 1) + jstp;
                key2.fill(-9999);
                if !(k >= 2) {
                    break;
                }
            }
            return;
        }

        k1 = k - 1;
        n = ico[(kb - 1) as usize];
        ntot = 0;
        for idx in (kb as usize + 1)..=nco as usize {
            ntot += ico[idx - 1];
        }

        // L150:
        'l150: loop {
            // Arc to daughter length=ICO[KB]
            for idx in 1..=nro as usize {
                irn[idx - 1] = iro[idx - 1] - idif[idx - 1];
            }

            if k1 > 1 {
                // Sort irn
                if nro == 2 {
                    if irn[0] > irn[1] {
                        ii = irn[0];
                        irn[0] = irn[1];
                        irn[1] = ii;
                    }
                } else {
                    isort_slice(&mut irn[..nro as usize]);
                }

                // Adjust start for zero
                let mut found = false;
                for idx in 1..=nro as usize {
                    if irn[idx - 1] != 0 {
                        nrb = idx as c_int;
                        found = true;
                        break;
                    }
                }
                if !found {
                    nrb = (nro as usize + 1) as c_int;
                }
            } else {
                nrb = 1;
            }
            nro2 = nro - nrb + 1;

            // Some table values
            ddf = f9xact(n, idif, fact);
            drn = f9xact(ntot, &irn[(nrb - 1) as usize..], fact) - dro + ddf;
            // Get hash value
            if k1 > 1 {
                kval = irn[0];
                for idx in 2..=nro as usize {
                    kval += irn[idx - 1] * kyy[idx - 1];
                }

                // Get hash table entry
                i = kval % (ldkey << 1) + 1;
                // Search for unused location
                let mut found_itp = false;
                for itp_search in i as usize..=(ldkey << 1) as usize {
                    ii = key2[itp_search - 1];
                    if ii == kval {
                        itp = itp_search as c_int;
                        found_itp = true;
                        break;
                    } else if ii < 0 {
                        key2[itp_search - 1] = kval;
                        lp[itp_search - 1] = 1.;
                        sp[itp_search - 1] = 1.;
                        itp = itp_search as c_int;
                        found_itp = true;
                        break;
                    }
                }

                if !found_itp {
                    for itp_search in 1..i as usize {
                        ii = key2[itp_search - 1];
                        if ii == kval {
                            itp = itp_search as c_int;
                            found_itp = true;
                            break;
                        } else if ii < 0 {
                            key2[itp_search - 1] = kval;
                            lp[itp_search - 1] = 1.;
                            itp = itp_search as c_int;
                            found_itp = true;
                            break;
                        }
                    }
                }

                if !found_itp {
                    let msg = format!(
                        "FEXACT error 6.  LDKEY={} is too small for this problem,\n  (ii := key2[itp={}] = {}, ldstp={})\n\
                         Try increasing the size of the workspace and possibly 'mult'",
                        ldkey, itp, ii, ldstp
                    );
                    prterr(6, &msg);
                }
            }

            // L240:
            psh = true;
            let mut hash_slot = 0;
            // Recover pastp
            ipn = ipoin[(ipo + ikkey - 1) as usize];
            pastp = stp[(ipn + ikstp - 1) as usize];
            ifreq = ifrq[(ipn + ikstp - 1) as usize];
            // Compute shortest and longest path
            if k1 > 1 {
                obs2 = obs
                    - fact_term(fact, ico[kb as usize], "obs2-kb1")
                    - fact_term(fact, ico[(kb + 1) as usize], "obs2-kb2")
                    - ddf;
                for idx in 3..=k1 as usize {
                    obs2 -= fact_term(fact, ico[kb as usize + idx - 1], "obs2");
                }

                if lp[(itp - 1) as usize] > 0. {
                    dspt = obs - obs2 - ddf;
                    let rows = &irn[(nrb - 1) as usize..];
                    let cols = &ico[kb as usize..nco as usize];
                    let width = nco as usize;
                    let stack_len = 2 * n2_stack as usize;
                    let (col_work, scratch) = iwk.split_at_mut(width);
                    let (row_work, scratch) = scratch.split_at_mut(width);
                    let (it, scratch) = scratch.split_at_mut(width);
                    let (lb, scratch) = scratch.split_at_mut(width);
                    let (nr, scratch) = scratch.split_at_mut(width);
                    let (nt, scratch) = scratch.split_at_mut(width);
                    let (nu, scratch) = scratch.split_at_mut(width);
                    let (itc, scratch) = scratch.split_at_mut(stack_len);
                    let ist = &mut scratch[..stack_len];
                    let (stv, scratch) = rwk.split_at_mut(stack_len);
                    let alen = &mut scratch[..width + 1];
                    // Compute longest path
                    lp[(itp - 1) as usize] = f3xact(
                        rows, cols, ntot, fact, col_work, row_work, it, lb, nr, nt, nu, itc, ist,
                        stv, alen, tol, n2_stack,
                    );
                    if lp[(itp - 1) as usize] > 0. {
                        REprintf(c"___ LP[itp=%d] = %g > 0\n");
                        lp[(itp - 1) as usize] = 0.;
                    }

                    // Compute shortest path -- using dspt as offset
                    // Reuses the same reserved iwk/rwk space as the
                    // longest-path search; the earlier borrows end at its
                    // return.
                    let depth = (nrow + ncol + 1) as usize;
                    let stack_cells = depth * nco as usize;
                    let scratch = &mut iwk[..5 * depth + 2 * stack_cells];
                    let (ncstk, scratch) = scratch.split_at_mut(depth);
                    let (lstk, scratch) = scratch.split_at_mut(depth);
                    let (mstk, scratch) = scratch.split_at_mut(depth);
                    let (nstk, scratch) = scratch.split_at_mut(depth);
                    let (nrstk, scratch) = scratch.split_at_mut(depth);
                    let (irstk, icstk) = scratch.split_at_mut(stack_cells);
                    sp[(itp - 1) as usize] = f4xact(
                        rows,
                        cols,
                        dspt,
                        fact,
                        icstk,
                        ncstk,
                        lstk,
                        mstk,
                        nstk,
                        nrstk,
                        irstk,
                        &mut rwk[..depth],
                        tol,
                    );
                    if sp[(itp - 1) as usize] > 0. {
                        sp[(itp - 1) as usize] = 0.;
                    }

                    // Use chi-squared approximation?
                    if maybe_chisq
                        && rows.first().is_some_and(|&row| {
                            (row as c_double * cols[0] as c_double) > ntot as c_double * emin
                        })
                    {
                        let mut ncell: c_int = 0;
                        for &row in rows {
                            for &col in cols {
                                if (row as c_double * col as c_double) >= ntot as c_double * expect
                                {
                                    ncell += 1;
                                }
                            }
                        }

                        if (ncell as c_double) * 100. >= (k1 * nro2) as c_double * percnt {
                            tmp = 0.;
                            for &row in rows {
                                tmp +=
                                    fact_term(fact, row, "irn") - fact_term(fact, row - 1, "irn-1");
                            }
                            tmp *= (k1 - 1) as c_double;
                            for &col in cols {
                                tmp += (nro2 - 1) as c_double
                                    * (fact[col as usize] - fact_term(fact, col - 1, "ico-1"));
                            }
                            df = ((nro2 - 1) * (k1 - 1)) as c_double;
                            tmp += df * 1.83787706640934548356065947281;
                            tmp -= ((nro2 * k1 - 1) as c_double)
                                * (fact[ntot as usize] - fact[(ntot - 1) as usize]);
                            tm[(itp - 1) as usize] = (obs - dro) * -2. - tmp;
                        } else {
                            tm[(itp - 1) as usize] = -9876.;
                        }
                    } else {
                        tm[(itp - 1) as usize] = -9876.;
                    }
                }
                obs3 = obs2 - lp[(itp - 1) as usize];
                obs2 -= sp[(itp - 1) as usize];
                if tm[(itp - 1) as usize] == -9876. {
                    chisq = false;
                } else {
                    chisq = true;
                    tmp = tm[(itp - 1) as usize];
                }
            } else {
                obs2 = obs - drn - dro;
                obs3 = obs2;
            }

            // L300: Process node with new PASTP
            'l300: loop {
                if pastp <= obs3 {
                    // Update pre
                    *pre += ifreq as c_double * (pastp + drn).exp();
                } else if pastp < obs2 {
                    if chisq {
                        df = ((nro2 - 1) * (k1 - 1)) as c_double;
                        pv = pgamma(
                            f64::max(0., tmp + (pastp + drn) * 2.) / 2.,
                            df / 2.,
                            1.,
                            0,
                            1,
                        );
                        *pre += ifreq as c_double * (pastp + drn + pv).exp();
                    } else {
                        // Put daughter on queue
                        let step_len = ldstp as usize;
                        let step_start = (jstp - 1) as usize;
                        let hash_start = (jkey - 1) as usize;
                        // The branch arrays are shared by both stages; the counts
                        // and links alternate halves.
                        let (counts_and_links, branches) = ifrq.split_at_mut(4 * step_len);
                        let (counts, links) = counts_and_links.split_at_mut(2 * step_len);
                        let (right, left) = branches.split_at_mut(step_len);
                        f5xact(
                            pastp + ddf,
                            tol,
                            kval,
                            &mut key[hash_start..hash_start + ldkey as usize],
                            &mut ipoin[hash_start..hash_start + ldkey as usize],
                            &mut stp[step_start..step_start + step_len],
                            &mut counts[step_start..step_start + step_len],
                            &mut links[step_start..step_start + step_len],
                            right,
                            left,
                            ifreq,
                            &mut itop,
                            psh,
                            &mut hash_slot,
                        );
                        psh = false;
                    }
                }
                // Get next PASTP on chain
                ipn = ifrq[(ipn + ikstp2 - 1) as usize];
                if ipn > 0 {
                    pastp = stp[(ipn + ikstp - 1) as usize];
                    ifreq = ifrq[(ipn + ikstp - 1) as usize];
                    continue 'l300;
                }
                break;
            }

            // Generate a new daughter node
            ok_f7 = f7xact(kmax, &iro[..nro as usize], idif, &mut kd, &mut ks);
            if ok_f7 {
                continue 'l150;
            }

            // L310: Go get a new mother from stage K
            loop {
                if !f6xact(
                    &mut iro[..nro as usize],
                    &kyy[..nro as usize],
                    &mut key[ikkey as usize..(ikkey + ldkey) as usize],
                    &mut last,
                    &mut ipo,
                ) {
                    // Update pointers -- continue outer loop
                    continue 'outer_loop;
                }
                // No additional nodes to process
                k -= 1;
                itop = 0;
                ikkey = jkey - 1;
                ikstp = jstp - 1;
                ikstp2 = jstp2 - 1;
                jkey = ldkey - jkey + 2;
                jstp = ldstp - jstp + 2;
                jstp2 = (ldstp << 1) + jstp;
                key2.fill(-9999);
                if !(k >= 2) {
                    break;
                }
            }
            return;
        }
    }
}

fn f3xact(
    irow: &[c_int],
    icol: &[c_int],
    ntot: c_int,
    fact: &[c_double],
    ico: &mut [c_int],
    iro: &mut [c_int],
    it: &mut [c_int],
    lb: &mut [c_int],
    nr: &mut [c_int],
    nt: &mut [c_int],
    nu: &mut [c_int],
    itc: &mut [c_int],
    ist: &mut [c_int],
    stv: &mut [c_double],
    alen: &mut [c_double],
    tol: c_double,
    ldst: c_int,
) -> c_double {
    let nrow = irow.len() as c_int;
    let ncol = icol.len() as c_int;

    if nrow <= 1 {
        // nrow is 1
        let mut lp: c_double = 0.;
        if nrow > 0 {
            for idx in 1..=ncol as usize {
                lp -= fact[icol[idx - 1] as usize];
            }
        }
        return lp;
    }

    if ncol <= 1 {
        // ncol is 1
        let mut lp: c_double = 0.;
        if ncol > 0 {
            for idx in 1..=nrow as usize {
                lp -= fact[irow[idx - 1] as usize];
            }
        }
        return lp;
    }

    // 2 by 2 table
    if nrow * ncol == 4 {
        let n11 = (irow[0] + 1) * (icol[0] + 1) / (ntot + 2);
        let n12 = irow[0] - n11;
        return -(fact[n11 as usize]
            + fact[n12 as usize]
            + fact[(icol[0] - n11) as usize]
            + fact[(icol[1] - n12) as usize]);
    }

    // ELSE: larger than 2 x 2

    // Test for optimal table
    let mut nst: c_int = 0;
    let mut nitc: c_int = 0;

    let mut i: c_int;
    let mut ii: c_int;
    let mut nn: c_int;
    let nco: c_int;
    let mut ipn: c_int;
    let mut key: c_int = 0;
    let mut itp: c_int = 0;
    let nro: c_int;
    let mut xmin: bool;

    let mut val: c_double = 0.;

    if irow[(nrow - 1) as usize] <= irow[0] + ncol {
        xmin = f10act(irow, icol, &mut val, fact, lb, nu, nr);
    } else {
        xmin = false;
    }
    if !xmin && icol[(ncol - 1) as usize] <= icol[0] + nrow {
        xmin = f10act(icol, irow, &mut val, fact, lb, nu, nr);
    }
    if xmin {
        return -val;
    }

    // Setup for dynamic programming
    alen[..=ncol as usize].fill(0.);
    ist.fill(-1);

    nn = ntot;
    // Minimize ncol: nco = min(ncol, nrow); nro = max(nrow, ncol)
    let mut nro;
    let nco;
    if nrow >= ncol {
        nro = nrow;
        nco = ncol;
        ico[0] = icol[0];
        nt[0] = nn - ico[0];
        for idx in 2..=ncol as usize {
            ico[idx - 1] = icol[idx - 1];
            nt[idx - 1] = nt[idx - 2] - ico[idx - 1];
        }
        for idx in 1..=nrow as usize {
            iro[idx - 1] = irow[idx - 1];
        }
    } else {
        nro = ncol;
        nco = nrow;
        ico[0] = irow[0];
        nt[0] = nn - ico[0];
        for idx in 2..=nrow as usize {
            ico[idx - 1] = irow[idx - 1];
            nt[idx - 1] = nt[idx - 2] - ico[idx - 1];
        }
        for idx in 1..=ncol as usize {
            iro[idx - 1] = icol[idx - 1];
        }
    }

    let nc1s = nco - 1;
    let kyy = ico[(nco - 1) as usize] + 1;
    // Initialize pointers
    let mut irl: c_int = 1;
    let mut ks: c_int = 0;
    let mut k: c_int = ldst;
    let mut lev: c_int;
    let mut nr1 = nro - 1;
    let mut vmn: c_double = 1e100;

    'new_node: loop {
        // GNU sets nr1 = nro - 1 at every LnewNode.
        nr1 = nro - 1;
        lev = 1;
        let nrt = iro[(irl - 1) as usize];
        let nct = ico[0];
        lb[0] = (((nrt as c_double + 1.) * (nct as c_double + 1.))
            / (nn as c_double + nr1 as c_double * nc1s as c_double + 1.)
            - tol) as c_int
            - 1;
        nu[0] = (((nrt as c_double + nc1s as c_double) * (nct as c_double + nr1 as c_double))
            / (nn as c_double + nr1 as c_double + nc1s as c_double)
            - lb[0] as c_double
            + 1.) as c_int;
        nr[0] = nrt - lb[0];

        // LoopNode: Generate a node
        'loop_node: loop {
            nu[(lev - 1) as usize] -= 1;
            if nu[(lev - 1) as usize] == 0 {
                if lev == 1 {
                    // goto L200
                    break 'loop_node;
                }
                lev -= 1;
                continue 'loop_node;
            }
            lb[(lev - 1) as usize] += 1;
            nr[(lev - 1) as usize] -= 1;

            loop {
                alen[lev as usize] =
                    alen[(lev - 1) as usize] + fact_term(fact, lb[(lev - 1) as usize], "lb");
                if lev >= nc1s {
                    break;
                }

                let nn1 = nt[(lev - 1) as usize];
                let nrt_val = nr[(lev - 1) as usize];
                lev += 1;
                let nc1 = nco - lev;
                let nct_val = ico[(lev - 1) as usize];
                lb[(lev - 1) as usize] = (((nrt_val as c_double + 1.) * (nct_val as c_double + 1.))
                    / (nn1 as c_double + nr1 as c_double * nc1 as c_double + 1.)
                    - tol) as c_int;
                nu[(lev - 1) as usize] = (((nrt_val as c_double + nc1 as c_double)
                    * (nct_val as c_double + nr1 as c_double))
                    / (nn1 as c_double + nr1 as c_double + nc1 as c_double)
                    - lb[(lev - 1) as usize] as c_double
                    + 1.) as c_int;
                nr[(lev - 1) as usize] = nrt_val - lb[(lev - 1) as usize];
            }
            alen[nco as usize] = alen[lev as usize] + fact_term(fact, nr[(lev - 1) as usize], "nr");
            lb[(nco - 1) as usize] = nr[(lev - 1) as usize];

            let v = val + alen[nco as usize];

            if nro == 2 {
                // Only 1 row left
                let mut v2 = v
                    + fact_term(fact, ico[0] - lb[0], "ico1-lb1")
                    + fact_term(fact, ico[1] - lb[1], "ico2-lb2");
                for idx in 3..=nco as usize {
                    v2 += fact_term(fact, ico[idx - 1] - lb[idx - 1], "ico-lb");
                }
                if vmn > v2 {
                    vmn = v2;
                }
            } else if nro == 3 && nco == 2 {
                // 3 rows and 2 columns
                let nn1 = nn - iro[(irl - 1) as usize] + 2;
                let ic1 = ico[0] - lb[0];
                let ic2 = ico[1] - lb[1];
                let n11 = (iro[irl as usize] + 1) * (ic1 + 1) / nn1;
                let n12 = iro[irl as usize] - n11;
                let v2 = v
                    + fact_term(fact, n11, "n11")
                    + fact_term(fact, n12, "n12")
                    + fact_term(fact, ic1 - n11, "ic1-n11")
                    + fact_term(fact, ic2 - n12, "ic2-n12");
                if vmn > v2 {
                    vmn = v2;
                }
            } else {
                // Column marginals are new node
                for idx in 1..=nco as usize {
                    it[idx - 1] = std::cmp::max(ico[idx - 1] - lb[idx - 1], 0);
                }

                // Sort column marginals it[]
                if nco == 2 {
                    if it[0] > it[1] {
                        ii = it[0];
                        it[0] = it[1];
                        it[1] = ii;
                    }
                } else {
                    isort_slice(&mut it[..nco as usize]);
                }

                // Compute hash value
                let dky = kyy as c_double;
                let mut dkey = it[0] as c_double * dky + it[1] as c_double;
                for idx in 3..=nco as usize {
                    dkey = it[idx - 1] as c_double + dkey * dky;
                }
                if dkey > c_int::MAX as c_double {
                    let msg = format!(
                        "FEXACT[f3xact()] error: hash key {:.0} > INT_MAX, kyy={}, it[i (= nco = {})]= {}.\n\
                     Rather set 'simulate.p.value=TRUE'",
                        dkey,
                        kyy,
                        nco,
                        it[(nco - 1) as usize]
                    );
                    prterr(30, &msg);
                } else {
                    key = dkey as c_int;
                }
                // Table index
                ipn = key % ldst + 1;
                // Find empty position
                let mut found_push = false;
                for (itp_s, ii_s) in
                    (ipn as usize..=ldst as usize).zip((ks as usize + ipn as usize)..)
                {
                    if ist[ii_s - 1] < 0 {
                        // L180: Push onto stack
                        ist[ii_s - 1] = key;
                        stv[ii_s - 1] = v;
                        nst += 1;
                        let ii2 = nst as usize + ks as usize;
                        itc[ii2 - 1] = itp_s as c_int;
                        found_push = true;
                        break;
                    } else if ist[ii_s - 1] == key {
                        // L190: Marginals already on stack
                        stv[ii_s - 1] = f64::min(v, stv[ii_s - 1]);
                        found_push = true;
                        break;
                    }
                }
                if !found_push {
                    for (itp_s, ii_s) in (1usize..=ipn as usize - 1).zip((ks as usize + 1)..) {
                        if ist[ii_s - 1] < 0 {
                            ist[ii_s - 1] = key;
                            stv[ii_s - 1] = v;
                            nst += 1;
                            let ii2 = nst as usize + ks as usize;
                            itc[ii2 - 1] = itp_s as c_int;
                            found_push = true;
                            break;
                        } else if ist[ii_s - 1] == key {
                            stv[ii_s - 1] = f64::min(v, stv[ii_s - 1]);
                            found_push = true;
                            break;
                        }
                    }
                }
                if !found_push {
                    let msg = format!(
                        "FEXACT error 30.  Stack length exceeded in f3xact,\n  (ldst={}, key={}, ipn={}, itp={}, ist[ii={}]={}).\n\
                     Increase workspace or consider using 'simulate.p.value=TRUE'",
                        ldst, key, ipn, itp, 0, 0
                    );
                    prterr(30, &msg);
                }
            }
            continue 'loop_node;
        }

        // L200: Pop item from stack
        'l200: loop {
            if nitc > 0 {
                // Stack index
                itp = itc[(nitc + k - 1) as usize] + k;
                nitc -= 1;
                val = stv[(itp - 1) as usize];
                key = ist[(itp - 1) as usize];
                ist[(itp - 1) as usize] = -1;
                // Compute marginals
                for idx in (2..=nco as usize).rev() {
                    ico[idx - 1] = key % kyy;
                    key /= kyy;
                }
                ico[0] = key;
                // Set up nt array
                nt[0] = nn - ico[0];
                for idx in 2..=nco as usize {
                    nt[idx - 1] = nt[idx - 2] - ico[idx - 1];
                }

                let remaining_rows = &iro[(irl - 1) as usize..(irl - 1 + nro) as usize];
                let remaining_cols = &ico[..nco as usize];
                // Test for optimality (L90)
                if iro[(nro - 1) as usize] <= iro[(irl - 1) as usize] + nco {
                    xmin = f10act(remaining_rows, remaining_cols, &mut val, fact, lb, nu, nr);
                } else {
                    xmin = false;
                }

                if !xmin && ico[(nco - 1) as usize] <= ico[0] + nro {
                    xmin = f10act(remaining_cols, remaining_rows, &mut val, fact, lb, nu, nr);
                }
                if xmin {
                    if vmn > val {
                        vmn = val;
                    }
                    continue 'l200;
                } else {
                    continue 'new_node;
                }
            } else if nro > 2 && nst > 0 {
                // Go to next level
                nitc = nst;
                nst = 0;
                k = ks;
                ks = ldst - ks;
                nn -= iro[(irl - 1) as usize];
                irl += 1;
                nro -= 1;
                continue 'l200;
            } else {
                return -vmn;
            }
        }
    }
}

fn f4xact(
    irow: &[c_int],
    icol: &[c_int],
    dspt: c_double,
    fact: &[c_double],
    icstk: &mut [c_int],
    ncstk: &mut [c_int],
    lstk: &mut [c_int],
    mstk: &mut [c_int],
    nstk: &mut [c_int],
    nrstk: &mut [c_int],
    irstk: &mut [c_int],
    ystk: &mut [c_double],
    tol: c_double,
) -> c_double {
    let nrow = irow.len() as c_int;
    let ncol = icol.len() as c_int;
    if nrow == 0 || ncol == 0 {
        return 0.0;
    }
    // Take care of the easy cases first
    if nrow == 1 {
        let mut sp: c_double = 0.;
        for idx in 0..ncol as usize {
            sp -= fact[icol[idx] as usize];
        }
        return sp;
    }
    if ncol == 1 {
        let mut sp: c_double = 0.;
        for idx in 0..nrow as usize {
            sp -= fact[irow[idx] as usize];
        }
        return sp;
    }
    if nrow * ncol == 4 {
        if irow[1] <= icol[1] {
            return -(fact_term(fact, irow[1], "f4-irow")
                + fact_term(fact, icol[1], "f4-icol")
                + fact_term(fact, icol[1] - irow[1], "f4-diff"));
        } else {
            return -(fact_term(fact, icol[1], "f4-icol")
                + fact_term(fact, irow[1], "f4-irow")
                + fact_term(fact, irow[1] - icol[1], "f4-diff"));
        }
    }

    let mut i: c_int;
    let mut j: c_int;
    let mut l: c_int;
    let mut m: c_int = 0;
    let mut n: c_int = 0;
    let mut ic1: c_int;
    let mut ir1: c_int;
    let mut ict: c_int;
    let mut irt: c_int;
    let mut istk: c_int;
    let mut nco: c_int;
    let mut nro: c_int;
    let mut y: c_double;
    let mut amx: c_double;
    let mut sp: c_double;

    for (dst, &src) in irstk[..nrow as usize].iter_mut().zip(irow.iter().rev()) {
        *dst = src;
    }
    for (dst, &src) in icstk[..ncol as usize].iter_mut().zip(icol.iter().rev()) {
        *dst = src;
    }

    nro = nrow;
    nco = ncol;
    nrstk[0] = nro;
    ncstk[0] = nco;
    ystk[0] = 0.;
    y = 0.;
    istk = 1;
    l = 1;
    amx = 0.;
    sp = dspt;

    // In the C code, the first do-while loop and the L100 do-while loop
    // share goto L60 from inside L110. We restructure using a single
    // encompassing loop with state flags.

    let mut in_first_loop = true; // true = first loop, false = L100 loop

    // Combined loop: both the first "do {} while(1)" and "L100: do {} while(1)"
    // share the L60 code via goto. We use a state flag.
    'main: loop {
        if in_first_loop {
            // irstk[(istk - 1) * nrow] and icstk[(istk - 1) * ncol]
            ir1 = irstk[((istk - 1) * nrow) as usize];
            ic1 = icstk[((istk - 1) * ncol) as usize];
            if ir1 > ic1 {
                if nro >= nco {
                    m = nco - 1;
                    n = 2;
                } else {
                    m = nro;
                    n = 1;
                }
            } else if ir1 < ic1 {
                if nro <= nco {
                    m = nro - 1;
                    n = 1;
                } else {
                    m = nco;
                    n = 2;
                }
            } else {
                if nro <= nco {
                    m = nro - 1;
                    n = 1;
                } else {
                    m = nco - 1;
                    n = 2;
                }
            }
        }

        // L60:
        'l60: loop {
            if n == 1 {
                i = l;
                j = 1;
            } else {
                i = 1;
                j = l;
            }

            irt = irstk[((istk - 1) * nrow + i - 1) as usize];
            ict = icstk[((istk - 1) * ncol + j - 1) as usize];
            y += fact_term(fact, std::cmp::min(irt, ict), "f4-min");

            // Split the stack into the current and next frames so each
            // helper borrows disjoint rows/columns.
            let row_width = nrow as usize;
            let col_width = ncol as usize;
            let row_next = istk as usize * row_width;
            let col_next = istk as usize * col_width;
            let (row_frames, row_tail) = irstk.split_at_mut(row_next);
            let (col_frames, col_tail) = icstk.split_at_mut(col_next);
            let rows = &row_frames[row_next - row_width..];
            let cols = &col_frames[col_next - col_width..];
            let next_rows = &mut row_tail[..row_width];
            let next_cols = &mut col_tail[..col_width];

            if irt == ict {
                nro -= 1;
                nco -= 1;
                f11act(rows, i, nro, next_rows);
                f11act(cols, j, nco, next_cols);
            } else if irt > ict {
                nco -= 1;
                f11act(cols, j, nco, next_cols);
                f8xact(rows, irt - ict, i, nro, next_rows);
            } else {
                nro -= 1;
                f11act(rows, i, nro, next_rows);
                f8xact(cols, ict - irt, j, nco, next_cols);
            }

            if nro == 1 {
                for &count in &next_cols[..nco as usize] {
                    y += fact[count as usize];
                }
                // goto L90
                break 'l60;
            }
            if nco == 1 {
                for &count in &next_rows[..nro as usize] {
                    y += fact[count as usize];
                }
                // goto L90
                break 'l60;
            }

            lstk[(istk - 1) as usize] = l;
            mstk[(istk - 1) as usize] = m;
            nstk[(istk - 1) as usize] = n;
            istk += 1;
            nrstk[(istk - 1) as usize] = nro;
            ncstk[(istk - 1) as usize] = nco;
            ystk[(istk - 1) as usize] = y;
            l = 1;
            continue 'main;
        }

        // L90:
        if y > amx {
            amx = y;
            if sp - amx <= tol {
                return -dspt;
            }
        }

        // L100:
        'l100: loop {
            istk -= 1;
            if istk == 0 {
                sp -= amx;
                if sp - amx <= tol {
                    return -dspt;
                } else {
                    return sp - dspt;
                }
            }
            l = lstk[(istk - 1) as usize] + 1;

            // L110:
            'l110: loop {
                if l > mstk[(istk - 1) as usize] {
                    // no match found, continue L100
                    continue 'l100;
                }

                n = nstk[(istk - 1) as usize];
                nro = nrstk[(istk - 1) as usize];
                nco = ncstk[(istk - 1) as usize];
                y = ystk[(istk - 1) as usize];
                if n == 1 {
                    if irstk[((istk - 1) * nrow + l - 1) as usize]
                        < irstk[((istk - 1) * nrow + l - 2) as usize]
                    {
                        // goto L60
                        in_first_loop = false;
                        continue 'main;
                    }
                } else if n == 2 {
                    if icstk[((istk - 1) * ncol + l - 1) as usize]
                        < icstk[((istk - 1) * ncol + l - 2) as usize]
                    {
                        // goto L60
                        in_first_loop = false;
                        continue 'main;
                    }
                }
                l += 1;
                continue 'l110;
            }
        }
    }
}

fn f5xact(
    pastp: c_double,
    tol: c_double,
    kval: c_int,
    key: &mut [c_int],
    ipoin: &mut [c_int],
    stp: &mut [c_double],
    ifrq: &mut [c_int],
    npoin: &mut [c_int],
    nr: &mut [c_int],
    nl: &mut [c_int],
    ifreq: c_int,
    itop: &mut c_int,
    psh: bool,
    hash_slot: &mut c_int,
) {
    let ldkey = key.len() as c_int;
    let ldstp = stp.len() as c_int;

    if psh {
        // Convert KVAL to int in range 0, ..., LDKEY-1
        let ird = kval % ldkey;
        let mut found = false;
        let mut saw_empty = false;
        let mut itp_val: c_int = 0;
        for idx in ird..ldkey {
            if key[idx as usize] == kval {
                itp_val = idx;
                found = true;
                break;
            }
            if key[idx as usize] < 0 {
                itp_val = idx;
                saw_empty = true;
                break;
            }
        }
        if !found && !saw_empty {
            for idx in 0..ird {
                if key[idx as usize] == kval {
                    itp_val = idx;
                    found = true;
                    break;
                }
                if key[idx as usize] < 0 {
                    itp_val = idx;
                    saw_empty = true;
                    break;
                }
            }
        }

        if !found && !saw_empty {
            // Return if KEY array is full
            let msg = format!(
                "FEXACT error 6 (f5xact).  LDKEY={} is too small for this problem: kval={}.\n\
             Try increasing the size of the workspace.",
                ldkey, kval
            );
            prterr(6, &msg);
        }

        *hash_slot = itp_val;
        // Empty slot: insert and return. A hit falls through to L40.
        if saw_empty {
            key[itp_val as usize] = kval;
            *itop += 1;
            ipoin[itp_val as usize] = *itop;
            if *itop > ldstp {
                let msg = format!(
                    "FEXACT error 7(update key). LDSTP={} is too small for this problem,\n  (kval={}, itop-ldstp={}).\n\
             Increase workspace or consider using 'simulate.p.value=TRUE'.",
                    ldstp,
                    kval,
                    *itop - ldstp
                );
                prterr(7, &msg);
            }
            npoin[(*itop - 1) as usize] = -1;
            nr[(*itop - 1) as usize] = -1;
            nl[(*itop - 1) as usize] = -1;
            stp[(*itop - 1) as usize] = pastp;
            ifrq[(*itop - 1) as usize] = ifreq;
            return;
        }
    }
    let itp_val = *hash_slot;

    let mut ipn = ipoin[itp_val as usize];

    let test1 = pastp - tol;
    let test2 = pastp + tol;

    loop {
        if ipn <= 0 {
            break;
        }
        if stp[(ipn - 1) as usize] < test1 {
            ipn = nl[(ipn - 1) as usize];
        } else if stp[(ipn - 1) as usize] > test2 {
            ipn = nr[(ipn - 1) as usize];
        } else {
            if c_int::MAX - ifrq[(ipn - 1) as usize] < ifreq {
                let msg = "integer overflow in exact computation";
                prterr(99, msg);
            }
            ifrq[(ipn - 1) as usize] += ifreq;
            return;
        }
        if !(ipn > 0) {
            break;
        }
    }

    // Return if STP array full
    *itop += 1;
    if *itop > ldstp {
        let ipn0 = ipoin[itp_val as usize];
        let initial_step = usize::try_from(ipn0)
            .ok()
            .and_then(|index| index.checked_sub(1))
            .and_then(|index| stp.get(index))
            .copied()
            .unwrap_or(f64::NAN);
        let msg = format!(
            "FEXACT error 7(location). LDSTP={} is too small for this problem,\n  (pastp={}, ipn_0:=ipoin[itp={}]= {}, stp[ipn_0]={}).\n\
         Increase workspace or consider using 'simulate.p.value=TRUE'",
            ldstp, pastp, itp_val, ipn0, initial_step
        );
        prterr(7, &msg);
    }

    // Find location to add value
    ipn = ipoin[itp_val as usize];
    let mut itmp = ipn;

    // L60:
    loop {
        if stp[(ipn - 1) as usize] < test1 {
            itmp = ipn;
            ipn = nl[(ipn - 1) as usize];
            if ipn > 0 {
                continue;
            }
            // else
            nl[(itmp - 1) as usize] = *itop;
            break;
        } else if stp[(ipn - 1) as usize] > test2 {
            itmp = ipn;
            ipn = nr[(ipn - 1) as usize];
            if ipn > 0 {
                continue;
            }
            // else
            nr[(itmp - 1) as usize] = *itop;
            break;
        } else {
            break;
        }
    }
    // Update STP, etc.
    npoin[(*itop - 1) as usize] = npoin[(itmp - 1) as usize];
    npoin[(itmp - 1) as usize] = *itop;
    stp[(*itop - 1) as usize] = pastp;
    ifrq[(*itop - 1) as usize] = ifreq;
    nl[(*itop - 1) as usize] = -1;
    nr[(*itop - 1) as usize] = -1;
}

fn f6xact(
    irow: &mut [c_int],
    kyy: &[c_int],
    key: &mut [c_int],
    last: &mut c_int,
    ipn: &mut c_int,
) -> bool {
    while (*last as usize) < key.len() {
        let slot = &mut key[*last as usize];
        *last += 1;
        if *slot < 0 {
            continue;
        }
        let mut kval = std::mem::replace(slot, -9999);
        for j in (1..irow.len()).rev() {
            irow[j] = kval / kyy[j];
            kval -= irow[j] * kyy[j];
        }
        irow[0] = kval;
        *ipn = *last;
        return false;
    }
    *last = 0;
    true
}

fn f7xact(nrow: c_int, iro: &[c_int], idif: &mut [c_int], k: &mut c_int, ks: &mut c_int) -> bool {
    let mut m: c_int;
    let kk: c_int;
    let mm: c_int;

    // Find node which can be incremented, ks
    if *ks == 0 {
        // When all row totals are assigned, there is no other daughter.
        let Some(next) = idif
            .iter()
            .zip(iro)
            .position(|(count, total)| count < total)
        else {
            return false;
        };
        *ks = (next + 1) as c_int;
    }

    // Find node to decrement (>ks)
    if *k > *ks && idif[(*k - 1) as usize] > 0 {
        idif[(*k - 1) as usize] -= 1;
        loop {
            *k -= 1;
            if iro[(*k - 1) as usize] != 0 {
                break;
            }
        }

        let mut m_val = *k;

        // Find node to increment (>=ks)
        while idif[(m_val - 1) as usize] >= iro[(m_val - 1) as usize] {
            m_val -= 1;
        }
        idif[(m_val - 1) as usize] += 1;
        // Change ks
        if m_val == *ks && idif[(m_val - 1) as usize] == iro[(m_val - 1) as usize] {
            *ks = *k;
        }
    } else {
        // Loop:
        'f7_loop: loop {
            // Check for finish
            let mut found_kk = false;
            let mut kk_val: c_int = 0;
            for idx in (*k + 1)..=nrow {
                if idif[(idx - 1) as usize] > 0 {
                    kk_val = idx;
                    found_kk = true;
                    break;
                }
            }
            if !found_kk {
                return false;
            }

            // L70: Reallocate counts
            let mut mm_val: c_int = 1;
            for idx in 1..=*k as usize {
                mm_val += idif[idx - 1];
                idif[idx - 1] = 0;
            }
            *k = kk_val;

            loop {
                *k -= 1;
                m = std::cmp::min(mm_val, iro[(*k - 1) as usize]);
                idif[(*k - 1) as usize] = m;
                mm_val -= m;
                if !(mm_val > 0 && *k != 1) {
                    break;
                }
            }

            // Check that all counts reallocated
            if mm_val > 0 {
                if kk_val != nrow {
                    *k = kk_val;
                    continue 'f7_loop;
                }
                return false;
            }
            // Get ks
            idif[(kk_val - 1) as usize] -= 1;
            *ks = 0;
            loop {
                *ks += 1;
                if *ks > *k {
                    return true;
                }
                if !(idif[(*ks - 1) as usize] >= iro[(*ks - 1) as usize]) {
                    break;
                }
            }
            break 'f7_loop;
        }
    }
    true
}

fn f8xact(irow: &[c_int], is: c_int, i1: c_int, izero: c_int, new: &mut [c_int]) {
    let mut i = (i1 - 1) as usize;
    let end = izero as usize;
    new[..i].copy_from_slice(&irow[..i]);
    while i + 1 < end && is < irow[i + 1] {
        new[i] = irow[i + 1];
        i += 1;
    }
    new[i] = is;
    new[i + 1..end].copy_from_slice(&irow[i + 1..end]);
}

fn f9xact(ntot: c_int, ir: &[c_int], fact: &[c_double]) -> c_double {
    let mut d = fact[ntot as usize];
    for &count in ir {
        d -= fact[count as usize];
    }
    d
}

fn f10act(
    irow: &[c_int],
    icol: &[c_int],
    val: &mut c_double,
    fact: &[c_double],
    nd: &mut [c_int],
    ne: &mut [c_int],
    m: &mut [c_int],
) -> bool {
    // All arrays are 0-based
    let nrow = irow.len() as c_int;
    let ncol = icol.len() as c_int;

    nd[..(nrow - 1) as usize].fill(0);

    let mut is = icol[0] / nrow;
    let mut ix = icol[0] - nrow * is;
    ne[0] = is;
    m[0] = ix;
    if ix != 0 {
        nd[(ix - 1) as usize] += 1;
    }

    for idx in 1..ncol as usize {
        ix = icol[idx] / nrow;
        ne[idx] = ix;
        is += ix;
        ix = icol[idx] - nrow * ix;
        m[idx] = ix;
        if ix != 0 {
            nd[(ix - 1) as usize] += 1;
        }
    }

    if nrow >= 3 {
        for idx in (0..=(nrow - 3) as usize).rev() {
            nd[idx] += nd[idx + 1];
        }
    }

    ix = 0;
    for idx in (2..=nrow as usize).rev() {
        ix += is + nd[nrow as usize - idx] - irow[idx - 1];
        if ix < 0 {
            return false;
        }
    }

    for idx in 0..ncol as usize {
        ix = ne[idx];
        is = m[idx];
        *val +=
            is as c_double * fact[(ix + 1) as usize] + (nrow - is) as c_double * fact[ix as usize];
    }
    true
}

fn f11act(irow: &[c_int], i1: c_int, i2: c_int, new: &mut [c_int]) {
    let removed = (i1 - 1) as usize;
    let end = i2 as usize;
    new[..removed].copy_from_slice(&irow[..removed]);
    new[removed..end].copy_from_slice(&irow[removed + 1..end + 1]);
}

// ---- internal entry ----

fn fexact(
    nrow: c_int,
    ncol: c_int,
    table: &[c_int],
    expect: c_double,
    percnt: c_double,
    emin: c_double,
    workspace: c_int,
    mult: c_int,
) -> c_double {
    if nrow < 0 || ncol < 0 || (nrow as usize).checked_mul(ncol as usize) != Some(table.len()) {
        prterr(1, "TABLE dimensions do not match its length.");
    }
    let mut total = 0_i64;
    for &count in table {
        if count < 0 {
            prterr(2, "All elements of TABLE must be nonnegative.");
        }
        total += i64::from(count);
        if total >= i64::from(c_int::MAX) {
            prterr(99, "integer overflow in exact computation");
        }
    }
    if total == 0 {
        prterr(
            3,
            "All elements of TABLE are zero.\nPRT and PRE are set to missing values.",
        );
    }
    if ncol <= 1 {
        prterr(4, "NCOL must be at least 2");
    }
    if mult <= 0 {
        prterr(40, "Out of workspace.");
    }
    let iwkmax = 2 * i64::from(workspace / 2);
    let mut iwkpt = 0;
    let n2_stack = std::cmp::max(200, iwkmax / 1000);
    let nco = i64::from(nrow.max(ncol));
    let nro = i64::from(nrow.min(ncol));
    let i1 = iwork(iwkmax, &mut iwkpt, total + 1, true);
    let i2 = iwork(iwkmax, &mut iwkpt, nco, false);
    let i3 = iwork(iwkmax, &mut iwkpt, nco, false);
    let i3a = iwork(iwkmax, &mut iwkpt, nco, false);
    let i3b = iwork(iwkmax, &mut iwkpt, nro, false);
    let i3c = iwork(iwkmax, &mut iwkpt, nro, false);
    let k = nco + nro + 1;
    let kk = k * nco;
    let int_scratch_len = (k * 5 + 2 * kk).max(nco * 7 + 4 * n2_stack);
    let iiwk = iwork(iwkmax, &mut iwkpt, int_scratch_len, false);
    let real_scratch_len = (nco + 1 + 2 * n2_stack).max(k);
    let irwk = iwork(iwkmax, &mut iwkpt, real_scratch_len, true);
    let numb = 18 + 10 * i64::from(mult);
    let ldkey = (iwkmax - iwkpt) / numb - 1;
    if ldkey <= 0 {
        prterr(
            6,
            &format!(
                "LDKEY={ldkey} is too small for this problem.\nTry increasing the size of the workspace and possibly 'mult'"
            ),
        );
    }
    let ldstp = i64::from(mult) * ldkey;
    if ldstp > i64::from(c_int::MAX) {
        prterr(
            99,
            &format!("integer overflow would happen in 'mult * ldkey' = {ldstp}"),
        );
    }
    let i4 = iwork(iwkmax, &mut iwkpt, ldkey * 2, false);
    let i5 = iwork(iwkmax, &mut iwkpt, ldkey * 2, false);
    let i6 = iwork(iwkmax, &mut iwkpt, ldstp * 2, true);
    let i7 = iwork(iwkmax, &mut iwkpt, ldstp * 6, false);
    let i8 = iwork(iwkmax, &mut iwkpt, ldkey * 2, true);
    let i9 = iwork(iwkmax, &mut iwkpt, ldkey * 2, true);
    let i9a = iwork(iwkmax, &mut iwkpt, ldkey * 2, true);
    let i10 = iwork(iwkmax, &mut iwkpt, ldkey * 2, false);

    let mut pre = 0.0;
    // SAFETY: iwork partitions the R_alloc'd block into disjoint regions
    // aligned for their element types and inside the original allocation;
    // both element types have valid zero representations. The session retains
    // the transient allocation for this native call.
    unsafe {
        let dwrk =
            R_alloc(std::mem::size_of::<c_double>(), (iwkmax / 2) as usize).cast::<c_double>();
        let iwrk = dwrk.cast::<c_int>();
        f2xact(
            nrow,
            ncol,
            table,
            expect,
            percnt,
            emin,
            &mut pre,
            std::slice::from_raw_parts_mut(dwrk.add(i1), (total + 1) as usize),
            std::slice::from_raw_parts_mut(iwrk.add(i2), nco as usize),
            std::slice::from_raw_parts_mut(iwrk.add(i3), nco as usize),
            std::slice::from_raw_parts_mut(iwrk.add(i3a), nco as usize),
            std::slice::from_raw_parts_mut(iwrk.add(i3b), nro as usize),
            std::slice::from_raw_parts_mut(iwrk.add(i3c), nro as usize),
            std::slice::from_raw_parts_mut(iwrk.add(i4), (2 * ldkey) as usize),
            ldkey as c_int,
            std::slice::from_raw_parts_mut(iwrk.add(i5), (2 * ldkey) as usize),
            std::slice::from_raw_parts_mut(dwrk.add(i6), (2 * ldstp) as usize),
            ldstp as c_int,
            std::slice::from_raw_parts_mut(iwrk.add(i7), (6 * ldstp) as usize),
            std::slice::from_raw_parts_mut(dwrk.add(i8), (2 * ldkey) as usize),
            std::slice::from_raw_parts_mut(dwrk.add(i9), (2 * ldkey) as usize),
            std::slice::from_raw_parts_mut(dwrk.add(i9a), (2 * ldkey) as usize),
            std::slice::from_raw_parts_mut(iwrk.add(i10), (2 * ldkey) as usize),
            std::slice::from_raw_parts_mut(iwrk.add(iiwk), int_scratch_len as usize),
            std::slice::from_raw_parts_mut(dwrk.add(irwk), real_scratch_len as usize),
            n2_stack as c_int,
        );
    }
    pre
}

/// SEXP wrapper for Fisher's exact test.
pub fn Fexact(x: SEXP, pars: SEXP, work: SEXP, smult: SEXP) -> SEXP {
    let nr = crate::library::stats::checked::nrows(x);
    let nc = crate::library::stats::checked::ncols(x);
    let ws = crate::main::coerce::asInteger(work);
    let mult = crate::main::coerce::asInteger(smult);

    let pars = coerceVector(pars, SEXPTYPE::REALSXP.as_c_int());
    let _pars_guard = protect(pars);

    let table = crate::library::stats::checked::int_vec(x);
    let (expect, percnt, emin) = (REAL_ELT(pars, 0), REAL_ELT(pars, 1), REAL_ELT(pars, 2));

    let p = fexact(nr, nc, &table, expect, percnt, emin, ws, mult);

    // ScalarReal: allocate a 1-element REALSXP
    let ans = Rf_allocVector(SEXPTYPE::REALSXP, 1);
    SET_REAL_ELT(ans, 0, p);
    ans
}

#[cfg(test)]
mod tests {
    use std::os::raw::c_int;

    #[test]
    fn fexact_matches_gnu_for_rectangular_tables() {
        use super::fexact;
        let session = crate::sexp::RSession::new_for_gc_tests();
        session.with_active(|| {
            assert!(
                (fexact(2, 3, &[4, 3, 1, 2, 5, 2], -1.0, 100.0, 0.0, 200_000, 30)
                    - 0.81103455368161226)
                    .abs()
                    < 1e-14
            );
            assert!(
                (fexact(3, 2, &[4, 1, 5, 3, 2, 2], -1.0, 100.0, 0.0, 200_000, 30)
                    - 0.81103455368161226)
                    .abs()
                    < 1e-14
            );
        });
    }

    #[test]
    fn fexact_matches_gnu_when_shortest_path_uses_stack() {
        use super::fexact;
        let session = crate::sexp::RSession::new_for_gc_tests();
        session.with_active(|| {
            // GNU R 4.6.2 oracle: exercises the general f4xact stack path.
            let p = fexact(
                3,
                4,
                &[3, 2, 1, 1, 4, 2, 1, 2, 3, 1, 2, 0],
                -1.0,
                100.0,
                0.0,
                12_000,
                30,
            );
            // Miri injects a few ULP of random error into each libm call, so
            // under Miri the enumeration cannot reproduce the GNU oracle to
            // 1e-14; native execution matches it bit-for-bit.
            let tolerance = if cfg!(miri) { 1e-9 } else { 1e-14 };
            assert!((p - 0.73720341015583335).abs() < tolerance);
            assert!(
                (fexact(
                    4,
                    3,
                    &[3, 2, 1, 1, 1, 4, 2, 2, 1, 2, 3, 0],
                    -1.0,
                    100.0,
                    0.0,
                    12_000,
                    30
                ) - 0.69331540961148608)
                    .abs()
                    < tolerance
            );
        });
    }

    #[test]
    fn fexact_longest_path_matches_enumerated_tables() {
        let mut fact = [0.0; 23];
        for n in 2..fact.len() {
            fact[n] = fact[n - 1] + (n as f64).ln();
        }
        let actual = super::f3xact(
            &[1, 6, 7, 8],
            &[1, 5, 7, 9],
            22,
            &fact,
            &mut [0; 4],
            &mut [0; 4],
            &mut [0; 4],
            &mut [0; 4],
            &mut [0; 4],
            &mut [0; 4],
            &mut [0; 4],
            &mut [0; 400],
            &mut [0; 400],
            &mut [0.0; 400],
            &mut [0.0; 5],
            3.45254e-7,
            200,
        );
        // Exhaustive enumeration gives a minimum product of cell factorials of 6912.
        assert!((actual - (-8.841014310483892)).abs() < 1e-12, "{actual}");
    }

    #[test]
    fn fexact_queue_probes_wrap_and_coalesces_steps() {
        let mut key = [-9999, 2];
        let mut ipoin = [0, 0];
        let mut stp = [0.0; 4];
        let mut ifrq = [0; 4];
        let mut npoin = [0; 4];
        let mut nr = [0; 4];
        let mut nl = [0; 4];
        let mut itop = 0;
        let mut hash_slot = 0;
        {
            let mut enqueue = |pastp: f64, ifreq: c_int, psh: bool| {
                super::f5xact(
                    pastp,
                    1e-7,
                    1,
                    &mut key,
                    &mut ipoin,
                    &mut stp,
                    &mut ifrq,
                    &mut npoin,
                    &mut nr,
                    &mut nl,
                    ifreq,
                    &mut itop,
                    psh,
                    &mut hash_slot,
                )
            };
            enqueue(10.0, 2, true);
            enqueue(10.0, 3, false);
            enqueue(9.0, 1, false);
            enqueue(11.0, 1, false);
        }
        assert_eq!(key, [1, 2]);
        assert_eq!(ipoin, [1, 0]);
        assert_eq!(itop, 3);
        assert_eq!(stp[..3], [10.0, 9.0, 11.0]);
        assert_eq!(ifrq[..3], [5, 1, 1]);
        // GNU stores the smaller-child link in nr and the larger-child link
        // in nl: the 9.0 node is index 2 under nr, the 11.0 node index 3
        // under nl.
        assert_eq!(nr[0], 2);
        assert_eq!(nl[0], 3);
        assert_eq!(npoin[..3], [3, -1, 2]);

        let (mut last, mut ipn) = (0, 0);
        let mut rows = [0, 0];
        assert!(!super::f6xact(
            &mut rows,
            &[1, 2],
            &mut key,
            &mut last,
            &mut ipn
        ));
        assert_eq!((last, ipn, rows), (1, 1, [1, 0]));
        assert!(!super::f6xact(
            &mut rows,
            &[1, 2],
            &mut key,
            &mut last,
            &mut ipn
        ));
        assert_eq!((last, ipn, rows), (2, 2, [0, 1]));
        assert!(super::f6xact(
            &mut rows,
            &[1, 2],
            &mut key,
            &mut last,
            &mut ipn
        ));
        assert_eq!(last, 0);
    }

    #[test]
    fn fexact_handles_empty_margins_and_saturated_daughters() {
        use super::fexact;
        let session = crate::sexp::RSession::new_for_gc_tests();
        session.with_active(|| {
            // GNU R 4.6.2 oracles: empty row/column margins exercise the
            // zero-marginal paths in the daughter search.
            for (table, oracle) in [
                (&[0, 0, 0, 0, 2, 3, 0, 4, 5][..], 1.0),
                (&[0, 0, 0, 1, 2, 3, 4, 1, 2][..], 0.47552447552447391),
                (&[0, 0, 0, 0, 0, 0, 1, 2, 3][..], 1.0),
                (&[0, 0, 1, 0, 0, 2, 0, 0, 3][..], 1.0),
            ] {
                for expect in [-1.0, 5.0] {
                    let actual = fexact(3, 3, table, expect, 80.0, 1.0, 12_000, 30);
                    assert!(
                        (actual - oracle).abs() < 1e-13,
                        "{table:?} expect={expect}: {actual}"
                    );
                }
            }
            let (mut k, mut ks) = (3, 0);
            assert!(!super::f7xact(
                3,
                &[0, 2, 3],
                &mut [0, 2, 3],
                &mut k,
                &mut ks
            ));
        });
    }

    #[test]
    fn fexact_hybrid_matches_gnu() {
        use super::fexact;
        let session = crate::sexp::RSession::new_for_gc_tests();
        session.with_active(|| {
            // GNU R 4.6.2 oracle: expect>0 exercises the chi-squared hybrid
            // path over the remaining irn/ico margins.
            let actual = fexact(
                3,
                3,
                &[12, 5, 7, 9, 8, 14, 4, 8, 10],
                5.0,
                80.0,
                1.0,
                200_000,
                30,
            );
            assert!((actual - 0.22517990347474948).abs() < 1e-12, "{actual}");
        });
    }

    #[test]
    fn fexact_rejects_invalid_layout_before_entering_kernel() {
        use super::fexact;
        let session = crate::sexp::RSession::new_for_gc_tests();
        session.with_active(|| {
            for (nr, nc, table, ws, mult, code) in [
                (2, 3, &[4, 3, 1, 2, 5][..], 200_000, 30, "FEXACT error 1."),
                (2, 3, &[4, 3, 1, 2, 5, 2][..], 2, 30, "FEXACT error 40."),
                (2, 3, &[4, 3, 1, 2, 5, 2][..], 1690, 30, "FEXACT error 6."),
                (
                    2,
                    2,
                    &[c_int::MAX, 1, 1, 1][..],
                    200_000,
                    30,
                    "FEXACT error 99.",
                ),
            ] {
                let result =
                    std::panic::catch_unwind(|| fexact(nr, nc, table, -1.0, 100.0, 0.0, ws, mult));
                let error = result
                    .unwrap_err()
                    .downcast::<crate::sexp::context::RError>()
                    .unwrap();
                assert!(error.message.starts_with(code));
            }
        });
    }
}
