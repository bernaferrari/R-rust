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
use crate::main::sort::R_isort;
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

unsafe fn isort_wrapper(n: c_int, ix: *mut c_int) {
    unsafe {
        R_isort(ix, n);
    }
}

unsafe fn fact_term(fact: *const c_double, idx: c_int, site: &str) -> c_double {
    unsafe {
        if idx < 0 {
            prterr(2, &format!("negative count {idx} at {site}"));
        }
        *fact.add(idx as usize)
    }
}

unsafe fn f2xact(
    nrow: c_int,
    ncol: c_int,
    table: &[c_int],
    expect: c_double,
    percnt: c_double,
    emin: c_double,
    pre: &mut c_double,
    fact: *mut c_double,
    ico: *mut c_int,
    iro: *mut c_int,
    kyy: *mut c_int,
    idif: *mut c_int,
    irn: *mut c_int,
    key: &mut [c_int],
    ldkey: c_int,
    ipoin: &mut [c_int],
    stp: &mut [c_double],
    ldstp: c_int,
    ifrq: &mut [c_int],
    lp: *mut c_double,
    sp: *mut c_double,
    tm: *mut c_double,
    key2: *mut c_int,
    iwk: *mut c_int,
    rwk: *mut c_double,
    n2_stack: c_int,
) {
    unsafe {
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

        let i31: c_int;
        let i32: c_int;
        let i33: c_int;
        let i34: c_int;
        let i35: c_int;
        let i36: c_int;
        let i37: c_int;
        let i38: c_int;
        let i39: c_int;
        let i41: c_int;
        let i42: c_int;
        let i43: c_int;
        let i44: c_int;
        let i45: c_int;
        let i46: c_int;
        let i47: c_int;
        let i48: c_int;
        let i310: c_int;
        let i311: c_int;

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

        // GNU decrements these pointers once so later 1-based indexes
        // land inside the allocation. table stays a 0-based slice.
        let ico = ico.sub(1);
        let iro = iro.sub(1);
        let kyy = kyy.sub(1);
        let idif = idif.sub(1);
        let irn = irn.sub(1);
        let lp = lp.sub(1);
        let sp = sp.sub(1);
        let tm = tm.sub(1);
        let key2 = key2.sub(1);
        let iwk = iwk.sub(1);
        let rwk = rwk.sub(1);

        // Initialize KEY array (1-based)
        key.fill(-9999);
        for idx in 1..=(ldkey << 1) as usize {
            *key2.add(idx) = -9999;
        }

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
            *iro.add(idx) = 0;
            for jdx in 1..=ncol as usize {
                let tval = table[(idx - 1) + (jdx - 1) * nrow as usize];
                *iro.add(idx) += tval;
            }
            ntot += *iro.add(idx);
        }

        // Column marginals (1-based)
        for idx in 1..=ncol as usize {
            *ico.add(idx) = 0;
            for jdx in 1..=nrow as usize {
                *ico.add(idx) += table[(jdx - 1) + (idx - 1) * nrow as usize];
            }
        }

        // Sort marginals
        isort_wrapper(nrow, iro.add(1));
        isort_wrapper(ncol, ico.add(1));

        // Swap marginals if necessary: ico[1:nco] & iro[1:nro]
        if nr_gt_nc {
            nro = ncol;
            for idx in 1..=nco as usize {
                ii = *iro.add(idx);
                if (idx as c_int) <= nro {
                    *iro.add(idx) = *ico.add(idx);
                }
                *ico.add(idx) = ii;
            }
        } else {
            nro = nrow;
        }

        // Get multipliers for stack (1-based)
        *kyy.add(1) = 1;
        let mut idx: usize = 1;
        while idx < nro as usize {
            if *iro.add(idx) < imax / *kyy.add(idx) {
                *kyy.add(idx + 1) = *kyy.add(idx) * (*iro.add(idx) + 1);
            } else {
                prterr(5, ch_err_5);
            }
            idx += 1;
        }

        // Check for Maximum product
        if *iro.add(nro as usize) + 1 > imax / *kyy.add(nro as usize) {
            prterr(501, ch_err_5);
        }

        // Compute log factorials (0-based for fact)
        *fact.add(0) = 0.;
        *fact.add(1) = 0.;
        if ntot >= 2 {
            *fact.add(2) = (2.0f64).ln();
        }
        let mut fi: usize = 3;
        while fi <= ntot as usize {
            *fact.add(fi) = *fact.add(fi - 1) + (fi as c_double).ln();
            let fj = fi + 1;
            if fj <= ntot as usize {
                *fact.add(fj) =
                    *fact.add(fi) + *fact.add(2) + *fact.add(fj / 2) - *fact.add(fj / 2 - 1);
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
                    dd += *fact.add(table[(jdx - 1) + (idx - 1) * nrow as usize] as usize);
                    ntot += table[(jdx - 1) + (idx - 1) * nrow as usize];
                }
            } else {
                let mut ii_us: usize = (jdx - 1) * nrow as usize;
                for idx in 1..=nro as usize {
                    dd += *fact.add(table[ii_us] as usize);
                    ntot += table[ii_us];
                    ii_us += 1;
                }
            }
            obs += *fact.add(*ico.add(jdx) as usize) - dd;
        }

        // Denominator of observed table: DRO
        dro = f9xact(nro, ntot, iro.add(1), fact);
        *pre = 0.;
        itop = 0;

        // Initialize pointers for workspace
        // f3xact
        i31 = 1;
        i32 = i31 + nco;
        i33 = i32 + nco;
        i34 = i33 + nco;
        i35 = i34 + nco;
        i36 = i35 + nco;
        i37 = i36 + nco;
        i38 = i37 + nco;
        i39 = i38 + 2 * n2_stack;
        i310 = 1;
        i311 = 1 + 2 * n2_stack;
        // f4xact
        i = nrow + ncol + 1;
        i41 = 1;
        i42 = i41 + i;
        i43 = i42 + i;
        i44 = i43 + i;
        i45 = i44 + i;
        i46 = i45 + i;
        i47 = i46 + i * nco;
        i48 = 1;

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
            n = *ico.add(kb as usize);
            kd = nro + 1;
            kmax = nro;
            // IDIF is the difference in going to the daughter
            for idx in 1..=nro as usize {
                *idif.add(idx) = 0;
            }

            // Generate the first daughter
            loop {
                kd -= 1;
                ntot = std::cmp::min(n, *iro.add(kd as usize));
                *idif.add(kd as usize) = ntot;
                if *idif.add(kmax as usize) == 0 {
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
                        std::slice::from_raw_parts_mut(iro.add(1), nro as usize),
                        std::slice::from_raw_parts(kyy.add(1), nro as usize),
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
                    for idx in 1..=(ldkey << 1) as usize {
                        *key2.add(idx) = -9999;
                    }
                    if !(k >= 2) {
                        break;
                    }
                }
                return;
            }

            k1 = k - 1;
            n = *ico.add(kb as usize);
            ntot = 0;
            for idx in (kb as usize + 1)..=nco as usize {
                ntot += *ico.add(idx);
            }

            // L150:
            'l150: loop {
                // Arc to daughter length=ICO[KB]
                for idx in 1..=nro as usize {
                    *irn.add(idx) = *iro.add(idx) - *idif.add(idx);
                }

                if k1 > 1 {
                    // Sort irn
                    if nro == 2 {
                        if *irn.add(1) > *irn.add(2) {
                            ii = *irn.add(1);
                            *irn.add(1) = *irn.add(2);
                            *irn.add(2) = ii;
                        }
                    } else {
                        isort_wrapper(nro, irn.add(1));
                    }

                    // Adjust start for zero
                    let mut found = false;
                    for idx in 1..=nro as usize {
                        if *irn.add(idx) != 0 {
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
                ddf = f9xact(nro, n, idif.add(1), fact);
                drn = f9xact(nro2, ntot, irn.add(nrb as usize), fact) - dro + ddf;
                // Get hash value
                if k1 > 1 {
                    kval = *irn.add(1);
                    for idx in 2..=nro as usize {
                        kval += *irn.add(idx) * *kyy.add(idx);
                    }

                    // Get hash table entry
                    i = kval % (ldkey << 1) + 1;
                    // Search for unused location
                    let mut found_itp = false;
                    for itp_search in i as usize..=(ldkey << 1) as usize {
                        ii = *key2.add(itp_search);
                        if ii == kval {
                            itp = itp_search as c_int;
                            found_itp = true;
                            break;
                        } else if ii < 0 {
                            *key2.add(itp_search) = kval;
                            *lp.add(itp_search) = 1.;
                            *sp.add(itp_search) = 1.;
                            itp = itp_search as c_int;
                            found_itp = true;
                            break;
                        }
                    }

                    if !found_itp {
                        for itp_search in 1..i as usize {
                            ii = *key2.add(itp_search);
                            if ii == kval {
                                itp = itp_search as c_int;
                                found_itp = true;
                                break;
                            } else if ii < 0 {
                                *key2.add(itp_search) = kval;
                                *lp.add(itp_search) = 1.;
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
                        - fact_term(fact, *ico.add((kb + 1) as usize), "obs2-kb1")
                        - fact_term(fact, *ico.add((kb + 2) as usize), "obs2-kb2")
                        - ddf;
                    for idx in 3..=k1 as usize {
                        obs2 -= fact_term(fact, *ico.add((kb as usize + idx) as usize), "obs2");
                    }

                    if *lp.add(itp as usize) > 0. {
                        dspt = obs - obs2 - ddf;
                        // Compute longest path
                        *lp.add(itp as usize) = f3xact(
                            nro2,
                            irn.add(nrb as usize),
                            k1,
                            ico.add((kb + 1) as usize),
                            ntot,
                            fact,
                            iwk.add(i31 as usize),
                            iwk.add(i32 as usize),
                            iwk.add(i33 as usize),
                            iwk.add(i34 as usize),
                            iwk.add(i35 as usize),
                            iwk.add(i36 as usize),
                            iwk.add(i37 as usize),
                            iwk.add(i38 as usize),
                            iwk.add(i39 as usize),
                            rwk.add(i310 as usize),
                            rwk.add(i311 as usize),
                            tol,
                            n2_stack,
                        );
                        if *lp.add(itp as usize) > 0. {
                            REprintf(c"___ LP[itp=%d] = %g > 0\n");
                            *lp.add(itp as usize) = 0.;
                        }

                        // Compute shortest path -- using dspt as offset
                        *sp.add(itp as usize) = f4xact(
                            nro2,
                            irn.add(nrb as usize),
                            k1,
                            ico.add((kb + 1) as usize),
                            dspt,
                            fact,
                            iwk.add(i47 as usize),
                            iwk.add(i41 as usize),
                            iwk.add(i42 as usize),
                            iwk.add(i43 as usize),
                            iwk.add(i44 as usize),
                            iwk.add(i45 as usize),
                            iwk.add(i46 as usize),
                            rwk.add(i48 as usize),
                            tol,
                        );
                        if *sp.add(itp as usize) > 0. {
                            *sp.add(itp as usize) = 0.;
                        }

                        // Use chi-squared approximation?
                        if maybe_chisq
                            && ((*irn.add(nrb as usize) as c_double
                                * *ico.add((kb + 1) as usize) as c_double)
                                > ntot as c_double * emin)
                        {
                            let mut ncell: c_int = 0;
                            for idx_i in 0..nro2 as usize {
                                for idx_j in 1..=k1 as usize {
                                    if (*irn.add(nrb as usize + idx_i) as c_double
                                        * *ico.add((kb as usize + idx_j) as usize) as c_double)
                                        >= ntot as c_double * expect
                                    {
                                        ncell += 1;
                                    }
                                }
                            }

                            if (ncell as c_double) * 100. >= (k1 * nro2) as c_double * percnt {
                                tmp = 0.;
                                for idx_i in 0..nro2 as usize {
                                    tmp += fact_term(fact, *irn.add(nrb as usize + idx_i), "irn")
                                        - fact_term(
                                            fact,
                                            *irn.add(nrb as usize + idx_i) - 1,
                                            "irn-1",
                                        );
                                }
                                tmp *= (k1 - 1) as c_double;
                                for idx_j in 1..=k1 as usize {
                                    tmp += (nro2 - 1) as c_double
                                        * (*fact
                                            .add(*ico.add((kb as usize + idx_j) as usize) as usize)
                                            - *fact.add(
                                                (*ico.add((kb as usize + idx_j) as usize) - 1)
                                                    as usize,
                                            ));
                                }
                                df = ((nro2 - 1) * (k1 - 1)) as c_double;
                                tmp += df * 1.83787706640934548356065947281;
                                tmp -= ((nro2 * k1 - 1) as c_double)
                                    * (*fact.add(ntot as usize) - *fact.add((ntot - 1) as usize));
                                *tm.add(itp as usize) = (obs - dro) * -2. - tmp;
                            } else {
                                *tm.add(itp as usize) = -9876.;
                            }
                        } else {
                            *tm.add(itp as usize) = -9876.;
                        }
                    }
                    obs3 = obs2 - *lp.add(itp as usize);
                    obs2 -= *sp.add(itp as usize);
                    if *tm.add(itp as usize) == -9876. {
                        chisq = false;
                    } else {
                        chisq = true;
                        tmp = *tm.add(itp as usize);
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
                            let (counts_and_links, branches) =
                                ifrq.split_at_mut(4 * step_len);
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
                ok_f7 = f7xact(kmax, iro.add(1), idif.add(1), &mut kd, &mut ks);
                if ok_f7 {
                    continue 'l150;
                }

                // L310: Go get a new mother from stage K
                loop {
                    if !f6xact(
                        std::slice::from_raw_parts_mut(iro.add(1), nro as usize),
                        std::slice::from_raw_parts(kyy.add(1), nro as usize),
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
                    for idx in 1..=(ldkey << 1) as usize {
                        *key2.add(idx) = -9999;
                    }
                    if !(k >= 2) {
                        break;
                    }
                }
                return;
            }
        }
    }
}

unsafe fn f3xact(
    nrow: c_int,
    irow: *const c_int,
    ncol: c_int,
    icol: *const c_int,
    ntot: c_int,
    fact: *const c_double,
    ico: *mut c_int,
    iro: *mut c_int,
    it: *mut c_int,
    lb: *mut c_int,
    nr: *mut c_int,
    nt: *mut c_int,
    nu: *mut c_int,
    itc: *mut c_int,
    ist: *mut c_int,
    stv: *mut c_double,
    alen: *mut c_double,
    tol: c_double,
    ldst: c_int,
) -> c_double {
    unsafe {
        let irow = irow.sub(1);
        let icol = icol.sub(1);
        let ico = ico.sub(1);
        let iro = iro.sub(1);
        let it = it.sub(1);
        let lb = lb.sub(1);
        let nr = nr.sub(1);
        let nt = nt.sub(1);
        let nu = nu.sub(1);
        let itc = itc.sub(1);
        let ist = ist.sub(1);
        let stv = stv.sub(1);

        if nrow <= 1 {
            // nrow is 1
            let mut lp: c_double = 0.;
            if nrow > 0 {
                for idx in 1..=ncol as usize {
                    lp -= *fact.add(*icol.add(idx) as usize);
                }
            }
            return lp;
        }

        if ncol <= 1 {
            // ncol is 1
            let mut lp: c_double = 0.;
            if ncol > 0 {
                for idx in 1..=nrow as usize {
                    lp -= *fact.add(*irow.add(idx) as usize);
                }
            }
            return lp;
        }

        // 2 by 2 table
        if nrow * ncol == 4 {
            let n11 = (*irow.add(1) + 1) * (*icol.add(1) + 1) / (ntot + 2);
            let n12 = *irow.add(1) - n11;
            return -(*fact.add(n11 as usize)
                + *fact.add(n12 as usize)
                + *fact.add((*icol.add(1) - n11) as usize)
                + *fact.add((*icol.add(2) - n12) as usize));
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

        if *irow.add(nrow as usize) <= *irow.add(1) + ncol {
            xmin = f10act(
                nrow,
                irow.add(1),
                ncol,
                icol.add(1),
                &mut val,
                fact,
                lb.add(1),
                nu.add(1),
                nr.add(1),
            );
        } else {
            xmin = false;
        }
        if !xmin && *icol.add(ncol as usize) <= *icol.add(1) + nrow {
            xmin = f10act(
                ncol,
                icol.add(1),
                nrow,
                irow.add(1),
                &mut val,
                fact,
                lb.add(1),
                nu.add(1),
                nr.add(1),
            );
        }
        if xmin {
            return -val;
        }

        // Setup for dynamic programming
        for idx in 0..=ncol as usize {
            *alen.add(idx) = 0.;
        }
        for idx in 1..=2 * ldst as usize {
            *ist.add(idx) = -1;
        }

        nn = ntot;
        // Minimize ncol: nco = min(ncol, nrow); nro = max(nrow, ncol)
        let mut nro;
        let nco;
        if nrow >= ncol {
            nro = nrow;
            nco = ncol;
            *ico.add(1) = *icol.add(1);
            *nt.add(1) = nn - *ico.add(1);
            for idx in 2..=ncol as usize {
                *ico.add(idx) = *icol.add(idx);
                *nt.add(idx) = *nt.add(idx - 1) - *ico.add(idx);
            }
            for idx in 1..=nrow as usize {
                *iro.add(idx) = *irow.add(idx);
            }
        } else {
            nro = ncol;
            nco = nrow;
            *ico.add(1) = *irow.add(1);
            *nt.add(1) = nn - *ico.add(1);
            for idx in 2..=nrow as usize {
                *ico.add(idx) = *irow.add(idx);
                *nt.add(idx) = *nt.add(idx - 1) - *ico.add(idx);
            }
            for idx in 1..=ncol as usize {
                *iro.add(idx) = *icol.add(idx);
            }
        }

        let nc1s = nco - 1;
        let kyy = *ico.add(nco as usize) + 1;
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
            let nrt = *iro.add(irl as usize);
            let nct = *ico.add(1);
            *lb.add(1) = (((nrt as c_double + 1.) * (nct as c_double + 1.))
                / (nn as c_double + nr1 as c_double * nc1s as c_double + 1.)
                - tol) as c_int
                - 1;
            *nu.add(1) = (((nrt as c_double + nc1s as c_double)
                * (nct as c_double + nr1 as c_double))
                / (nn as c_double + nr1 as c_double + nc1s as c_double)
                - *lb.add(1) as c_double
                + 1.) as c_int;
            *nr.add(1) = nrt - *lb.add(1);

            // LoopNode: Generate a node
            'loop_node: loop {
                *nu.add(lev as usize) -= 1;
                if *nu.add(lev as usize) == 0 {
                    if lev == 1 {
                        // goto L200
                        break 'loop_node;
                    }
                    lev -= 1;
                    continue 'loop_node;
                }
                *lb.add(lev as usize) += 1;
                *nr.add(lev as usize) -= 1;

                loop {
                    *alen.add(lev as usize) = *alen.add((lev - 1) as usize)
                        + fact_term(fact, *lb.add(lev as usize), "lb");
                    if lev >= nc1s {
                        break;
                    }

                    let nn1 = *nt.add(lev as usize);
                    let nrt_val = *nr.add(lev as usize);
                    lev += 1;
                    let nc1 = nco - lev;
                    let nct_val = *ico.add(lev as usize);
                    *lb.add(lev as usize) = (((nrt_val as c_double + 1.)
                        * (nct_val as c_double + 1.))
                        / (nn1 as c_double + nr1 as c_double * nc1 as c_double + 1.)
                        - tol) as c_int;
                    *nu.add(lev as usize) = (((nrt_val as c_double + nc1 as c_double)
                        * (nct_val as c_double + nr1 as c_double))
                        / (nn1 as c_double + nr1 as c_double + nc1 as c_double)
                        - *lb.add(lev as usize) as c_double
                        + 1.) as c_int;
                    *nr.add(lev as usize) = nrt_val - *lb.add(lev as usize);
                }
                *alen.add(nco as usize) =
                    *alen.add(lev as usize) + fact_term(fact, *nr.add(lev as usize), "nr");
                *lb.add(nco as usize) = *nr.add(lev as usize);

                let v = val + *alen.add(nco as usize);

                if nro == 2 {
                    // Only 1 row left
                    let mut v2 = v
                        + fact_term(fact, *ico.add(1) - *lb.add(1), "ico1-lb1")
                        + fact_term(fact, *ico.add(2) - *lb.add(2), "ico2-lb2");
                    for idx in 3..=nco as usize {
                        v2 += fact_term(fact, *ico.add(idx) - *lb.add(idx), "ico-lb");
                    }
                    if vmn > v2 {
                        vmn = v2;
                    }
                } else if nro == 3 && nco == 2 {
                    // 3 rows and 2 columns
                    let nn1 = nn - *iro.add(irl as usize) + 2;
                    let ic1 = *ico.add(1) - *lb.add(1);
                    let ic2 = *ico.add(2) - *lb.add(2);
                    let n11 = (*iro.add((irl + 1) as usize) + 1) * (ic1 + 1) / nn1;
                    let n12 = *iro.add((irl + 1) as usize) - n11;
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
                        *it.add(idx) = std::cmp::max(*ico.add(idx) - *lb.add(idx), 0);
                    }

                    // Sort column marginals it[]
                    if nco == 2 {
                        if *it.add(1) > *it.add(2) {
                            ii = *it.add(1);
                            *it.add(1) = *it.add(2);
                            *it.add(2) = ii;
                        }
                    } else {
                        isort_wrapper(nco, it.add(1));
                    }

                    // Compute hash value
                    let dky = kyy as c_double;
                    let mut dkey = *it.add(1) as c_double * dky + *it.add(2) as c_double;
                    for idx in 3..=nco as usize {
                        dkey = *it.add(idx) as c_double + dkey * dky;
                    }
                    if dkey > c_int::MAX as c_double {
                        let msg = format!(
                            "FEXACT[f3xact()] error: hash key {:.0} > INT_MAX, kyy={}, it[i (= nco = {})]= {}.\n\
                         Rather set 'simulate.p.value=TRUE'",
                            dkey,
                            kyy,
                            nco,
                            *it.add(nco as usize)
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
                        if *ist.add(ii_s) < 0 {
                            // L180: Push onto stack
                            *ist.add(ii_s) = key;
                            *stv.add(ii_s) = v;
                            nst += 1;
                            let ii2 = nst as usize + ks as usize;
                            *itc.add(ii2) = itp_s as c_int;
                            found_push = true;
                            break;
                        } else if *ist.add(ii_s) == key {
                            // L190: Marginals already on stack
                            *stv.add(ii_s) = f64::min(v, *stv.add(ii_s));
                            found_push = true;
                            break;
                        }
                    }
                    if !found_push {
                        for (itp_s, ii_s) in (1usize..=ipn as usize - 1).zip((ks as usize + 1)..) {
                            if *ist.add(ii_s) < 0 {
                                *ist.add(ii_s) = key;
                                *stv.add(ii_s) = v;
                                nst += 1;
                                let ii2 = nst as usize + ks as usize;
                                *itc.add(ii2) = itp_s as c_int;
                                found_push = true;
                                break;
                            } else if *ist.add(ii_s) == key {
                                *stv.add(ii_s) = f64::min(v, *stv.add(ii_s));
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
                    itp = *itc.add((nitc + k) as usize) + k;
                    nitc -= 1;
                    val = *stv.add(itp as usize);
                    key = *ist.add(itp as usize);
                    *ist.add(itp as usize) = -1;
                    // Compute marginals
                    for idx in (2..=nco as usize).rev() {
                        *ico.add(idx) = key % kyy;
                        key /= kyy;
                    }
                    *ico.add(1) = key;
                    // Set up nt array
                    *nt.add(1) = nn - *ico.add(1);
                    for idx in 2..=nco as usize {
                        *nt.add(idx) = *nt.add(idx - 1) - *ico.add(idx);
                    }

                    // Test for optimality (L90)
                    if *iro.add(nro as usize) <= *iro.add(irl as usize) + nco {
                        xmin = f10act(
                            nro,
                            iro.add(irl as usize),
                            nco,
                            ico.add(1),
                            &mut val,
                            fact,
                            lb.add(1),
                            nu.add(1),
                            nr.add(1),
                        );
                    } else {
                        xmin = false;
                    }

                    if !xmin && *ico.add(nco as usize) <= *ico.add(1) + nro {
                        xmin = f10act(
                            nco,
                            ico.add(1),
                            nro,
                            iro.add(irl as usize),
                            &mut val,
                            fact,
                            lb.add(1),
                            nu.add(1),
                            nr.add(1),
                        );
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
                    nn -= *iro.add(irl as usize);
                    irl += 1;
                    nro -= 1;
                    continue 'l200;
                } else {
                    return -vmn;
                }
            }
        }
    }
}

unsafe fn f4xact(
    nrow: c_int,
    irow: *mut c_int,
    ncol: c_int,
    icol: *mut c_int,
    dspt: c_double,
    fact: *const c_double,
    icstk: *mut c_int,
    ncstk: *mut c_int,
    lstk: *mut c_int,
    mstk: *mut c_int,
    nstk: *mut c_int,
    nrstk: *mut c_int,
    irstk: *mut c_int,
    ystk: *mut c_double,
    tol: c_double,
) -> c_double {
    unsafe {
        // Take care of the easy cases first
        if nrow == 1 {
            let mut sp: c_double = 0.;
            for idx in 0..ncol as usize {
                sp -= *fact.add(*icol.add(idx) as usize);
            }
            return sp;
        }
        if ncol == 1 {
            let mut sp: c_double = 0.;
            for idx in 0..nrow as usize {
                sp -= *fact.add(*irow.add(idx) as usize);
            }
            return sp;
        }
        if nrow * ncol == 4 {
            if *irow.add(1) <= *icol.add(1) {
                return -(fact_term(fact, *irow.add(1), "f4-irow")
                    + fact_term(fact, *icol.add(1), "f4-icol")
                    + fact_term(fact, *icol.add(1) - *irow.add(1), "f4-diff"));
            } else {
                return -(fact_term(fact, *icol.add(1), "f4-icol")
                    + fact_term(fact, *irow.add(1), "f4-irow")
                    + fact_term(fact, *irow.add(1) - *icol.add(1), "f4-diff"));
            }
        }

        // GNU: irstk -= nrow + 1; icstk -= ncol + 1; then the stack decrements.
        // irow/icol stay unshifted: irow[nrow - i] is 0-based.
        let irstk = irstk.sub(nrow as usize + 1);
        let icstk = icstk.sub(ncol as usize + 1);
        let nrstk = nrstk.sub(1);
        let ncstk = ncstk.sub(1);
        let lstk = lstk.sub(1);
        let mstk = mstk.sub(1);
        let nstk = nstk.sub(1);
        let ystk = ystk.sub(1);

        let mut i: c_int;
        let mut j: c_int;
        let _k: c_int = 0; // unused
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

        for idx in 1..=nrow as usize {
            *irstk.add(idx + nrow as usize) = *irow.add(nrow as usize - idx);
        }

        for idx in 1..=ncol as usize {
            *icstk.add(idx + ncol as usize) = *icol.add(ncol as usize - idx);
        }

        nro = nrow;
        nco = ncol;
        *nrstk.add(1) = nro;
        *ncstk.add(1) = nco;
        *ystk.add(1) = 0.;
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
                // irstk[istk * nrow + 1] and icstk[istk * ncol + 1]
                ir1 = *irstk.add((istk * nrow + 1) as usize);
                ic1 = *icstk.add((istk * ncol + 1) as usize);
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

                irt = *irstk.add((i + istk * nrow) as usize);
                ict = *icstk.add((j + istk * ncol) as usize);
                y += fact_term(fact, std::cmp::min(irt, ict), "f4-min");
                if irt == ict {
                    nro -= 1;
                    nco -= 1;
                    f11act(
                        irstk.add((istk * nrow + 1) as usize),
                        i,
                        nro,
                        irstk.add(((istk + 1) * nrow + 1) as usize),
                    );
                    f11act(
                        icstk.add((istk * ncol + 1) as usize),
                        j,
                        nco,
                        icstk.add(((istk + 1) * ncol + 1) as usize),
                    );
                } else if irt > ict {
                    nco -= 1;
                    f11act(
                        icstk.add((istk * ncol + 1) as usize),
                        j,
                        nco,
                        icstk.add(((istk + 1) * ncol + 1) as usize),
                    );
                    f8xact(
                        irstk.add((istk * nrow + 1) as usize),
                        irt - ict,
                        i,
                        nro,
                        irstk.add(((istk + 1) * nrow + 1) as usize),
                    );
                } else {
                    nro -= 1;
                    f11act(
                        irstk.add((istk * nrow + 1) as usize),
                        i,
                        nro,
                        irstk.add(((istk + 1) * nrow + 1) as usize),
                    );
                    f8xact(
                        icstk.add((istk * ncol + 1) as usize),
                        ict - irt,
                        j,
                        nco,
                        icstk.add(((istk + 1) * ncol + 1) as usize),
                    );
                }

                if nro == 1 {
                    for idx in 1..=nco as usize {
                        y += *fact
                            .add(*icstk.add(idx + (istk as usize + 1) * ncol as usize) as usize);
                    }
                    // goto L90
                    break 'l60;
                }
                if nco == 1 {
                    for idx in 1..=nro as usize {
                        y += *fact
                            .add(*irstk.add(idx + (istk as usize + 1) * nrow as usize) as usize);
                    }
                    // goto L90
                    break 'l60;
                }

                *lstk.add(istk as usize) = l;
                *mstk.add(istk as usize) = m;
                *nstk.add(istk as usize) = n;
                istk += 1;
                *nrstk.add(istk as usize) = nro;
                *ncstk.add(istk as usize) = nco;
                *ystk.add(istk as usize) = y;
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
                l = *lstk.add(istk as usize) + 1;

                // L110:
                'l110: loop {
                    if l > *mstk.add(istk as usize) {
                        // no match found, continue L100
                        continue 'l100;
                    }

                    n = *nstk.add(istk as usize);
                    nro = *nrstk.add(istk as usize);
                    nco = *ncstk.add(istk as usize);
                    y = *ystk.add(istk as usize);
                    if n == 1 {
                        if *irstk.add((l + istk * nrow) as usize)
                            < *irstk.add((l - 1 + istk * nrow) as usize)
                        {
                            // goto L60
                            in_first_loop = false;
                            continue 'main;
                        }
                    } else if n == 2 {
                        if *icstk.add((l + istk * ncol) as usize)
                            < *icstk.add((l - 1 + istk * ncol) as usize)
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

unsafe fn f7xact(
    nrow: c_int,
    iro: *const c_int,
    idif: *mut c_int,
    k: *mut c_int,
    ks: *mut c_int,
) -> bool {
    unsafe {
        let iro = iro.sub(1);
        let idif = idif.sub(1);

        let mut m: c_int;
        let kk: c_int;
        let mm: c_int;

        // Find node which can be incremented, ks
        if *ks == 0 {
            loop {
                *ks += 1;
                if !(*idif.add(*ks as usize) == *iro.add(*ks as usize)) {
                    break;
                }
            }
        }

        // Find node to decrement (>ks)
        if *idif.add(*k as usize) > 0 && *k > *ks {
            *idif.add(*k as usize) -= 1;
            loop {
                *k -= 1;
                if !(*iro.add(*k as usize) == 0) {
                    break;
                }
            }

            let mut m_val = *k;

            // Find node to increment (>=ks)
            while *idif.add(m_val as usize) >= *iro.add(m_val as usize) {
                m_val -= 1;
            }
            *idif.add(m_val as usize) += 1;
            // Change ks
            if m_val == *ks && *idif.add(m_val as usize) == *iro.add(m_val as usize) {
                *ks = *k;
            }
        } else {
            // Loop:
            'f7_loop: loop {
                // Check for finish
                let mut found_kk = false;
                let mut kk_val: c_int = 0;
                for idx in (*k + 1)..=nrow {
                    if *idif.add(idx as usize) > 0 {
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
                    mm_val += *idif.add(idx);
                    *idif.add(idx) = 0;
                }
                *k = kk_val;

                loop {
                    *k -= 1;
                    m = std::cmp::min(mm_val, *iro.add(*k as usize));
                    *idif.add(*k as usize) = m;
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
                *idif.add(kk_val as usize) -= 1;
                *ks = 0;
                loop {
                    *ks += 1;
                    if *ks > *k {
                        return true;
                    }
                    if !(*idif.add(*ks as usize) >= *iro.add(*ks as usize)) {
                        break;
                    }
                }
                break 'f7_loop;
            }
        }
        true
    }
}

unsafe fn f8xact(irow: *const c_int, is: c_int, i1: c_int, izero: c_int, new: *mut c_int) {
    unsafe {
        let irow = irow.sub(1);
        let new = new.sub(1);

        let mut i: c_int = 1;

        while i < i1 {
            *new.add(i as usize) = *irow.add(i as usize);
            i += 1;
        }

        while i < izero {
            if is >= *irow.add((i + 1) as usize) {
                break;
            }
            *new.add(i as usize) = *irow.add((i + 1) as usize);
            i += 1;
        }

        *new.add(i as usize) = is;

        loop {
            i += 1;
            if i > izero {
                return;
            }
            *new.add(i as usize) = *irow.add(i as usize);
        }
    }
}

unsafe fn f9xact(n: c_int, ntot: c_int, ir: *const c_int, fact: *const c_double) -> c_double {
    unsafe {
        // ir is 0-based
        let mut d = *fact.add(ntot as usize);
        for idx in 0..n as usize {
            d -= *fact.add(*ir.add(idx) as usize);
        }
        d
    }
}

unsafe fn f10act(
    nrow: c_int,
    irow: *const c_int,
    ncol: c_int,
    icol: *const c_int,
    val: *mut c_double,
    fact: *const c_double,
    nd: *mut c_int,
    ne: *mut c_int,
    m: *mut c_int,
) -> bool {
    unsafe {
        // All arrays are 0-based

        for idx in 0..(nrow - 1) as usize {
            *nd.add(idx) = 0;
        }

        let mut is = *icol.add(0) / nrow;
        let mut ix = *icol.add(0) - nrow * is;
        *ne.add(0) = is;
        *m.add(0) = ix;
        if ix != 0 {
            *nd.add((ix - 1) as usize) += 1;
        }

        for idx in 1..ncol as usize {
            ix = *icol.add(idx) / nrow;
            *ne.add(idx) = ix;
            is += ix;
            ix = *icol.add(idx) - nrow * ix;
            *m.add(idx) = ix;
            if ix != 0 {
                *nd.add((ix - 1) as usize) += 1;
            }
        }

        if nrow >= 3 {
            for idx in (0..=(nrow - 3) as usize).rev() {
                *nd.add(idx) += *nd.add(idx + 1);
            }
        }

        ix = 0;
        for idx in (2..=nrow as usize).rev() {
            ix += is + *nd.add(nrow as usize - idx) - *irow.add((idx - 1) as usize);
            if ix < 0 {
                return false;
            }
        }

        for idx in 0..ncol as usize {
            ix = *ne.add(idx);
            is = *m.add(idx);
            *val += is as c_double * *fact.add((ix + 1) as usize)
                + (nrow - is) as c_double * *fact.add(ix as usize);
        }
        true
    }
}

unsafe fn f11act(irow: *const c_int, i1: c_int, i2: c_int, new: *mut c_int) {
    unsafe {
        // All arrays are 0-based
        let mut i: c_int = 0;
        while i < i1 - 1 {
            *new.add(i as usize) = *irow.add(i as usize);
            i += 1;
        }
        i = i1;
        while i <= i2 {
            *new.add((i - 1) as usize) = *irow.add(i as usize);
            i += 1;
        }
    }
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
    let ikh = (k * 5 + 2 * kk).max(nco * 7 + 4 * n2_stack);
    let iiwk = iwork(iwkmax, &mut iwkpt, ikh, false);
    let ikh = (nco + 1 + 2 * n2_stack).max(k);
    let irwk = iwork(iwkmax, &mut iwkpt, ikh, true);
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
    // aligned for their element types. The session retains the transient
    // allocation for this native call.
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
            dwrk.add(i1),
            iwrk.add(i2),
            iwrk.add(i3),
            iwrk.add(i3a),
            iwrk.add(i3b),
            iwrk.add(i3c),
            std::slice::from_raw_parts_mut(iwrk.add(i4), (2 * ldkey) as usize),
            ldkey as c_int,
            std::slice::from_raw_parts_mut(iwrk.add(i5), (2 * ldkey) as usize),
            std::slice::from_raw_parts_mut(dwrk.add(i6), (2 * ldstp) as usize),
            ldstp as c_int,
            std::slice::from_raw_parts_mut(iwrk.add(i7), (6 * ldstp) as usize),
            dwrk.add(i8),
            dwrk.add(i9),
            dwrk.add(i9a),
            iwrk.add(i10),
            iwrk.add(iiwk),
            dwrk.add(irwk),
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
        assert!(!super::f6xact(&mut rows, &[1, 2], &mut key, &mut last, &mut ipn));
        assert_eq!((last, ipn, rows), (1, 1, [1, 0]));
        assert!(!super::f6xact(&mut rows, &[1, 2], &mut key, &mut last, &mut ipn));
        assert_eq!((last, ipn, rows), (2, 2, [0, 1]));
        assert!(super::f6xact(&mut rows, &[1, 2], &mut key, &mut last, &mut ipn));
        assert_eq!(last, 0);
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
