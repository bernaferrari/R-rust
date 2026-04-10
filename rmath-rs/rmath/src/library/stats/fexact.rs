#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    unused_assignments,
    non_camel_case_types,
    unsafe_op_in_unsafe_fn
)]

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
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

// ---- internal helper functions (not exported) ----

unsafe fn prterr(icode: c_int, mes: &str) {
    let msg = format!("FEXACT error {}.\n{}", icode, mes);
    eprintln!("{}", msg);
    std::panic::panic_any(crate::sexp::context::RError { message: msg });
}

unsafe fn iwork(iwkmax: c_int, iwkpt: &mut c_int, number: c_int, itype: c_int) -> c_int {
    let i_real: c_int = 4;
    let i_int: c_int = 2;

    let mut i = *iwkpt;
    if itype == i_int || itype == 3 {
        *iwkpt += number;
    } else {
        // double
        if i % 2 != 0 {
            i += 1;
        }
        *iwkpt += number << 1;
        i /= 2;
    }
    if *iwkpt > iwkmax {
        prterr(40, "Out of workspace.");
    }
    i
}

unsafe fn isort_wrapper(n: c_int, ix: *mut c_int) {
    R_isort(ix, n);
}

unsafe fn f2xact(
    nrow: c_int,
    ncol: c_int,
    table: *const c_int,
    ldtabl: c_int,
    expect: c_double,
    percnt: c_double,
    emin: c_double,
    prt: *mut c_double,
    pre: *mut c_double,
    fact: *mut c_double,
    ico: *mut c_int,
    iro: *mut c_int,
    kyy: *mut c_int,
    idif: *mut c_int,
    irn: *mut c_int,
    key: *mut c_int,
    ldkey: c_int,
    ipoin: *mut c_int,
    stp: *mut c_double,
    ldstp: c_int,
    ifrq: *mut c_int,
    lp: *mut c_double,
    sp: *mut c_double,
    tm: *mut c_double,
    key2: *mut c_int,
    iwk: *mut c_int,
    rwk: *mut c_double,
    n2_stack: c_int,
) {
    let imax = c_int::MAX;

    let amiss: c_double = -12345.;

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
    let mut jstp3: c_int = 0;
    let mut jstp4: c_int = 0;
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

    // Parameter adjustments (1-based indexing): all arrays shifted by -1
    // C code does: table -= ldtabl + 1; --ico; --iro; etc.
    // We handle this by adjusting pointer offsets inline.

    if nrow > ldtabl {
        prterr(1, "NROW must be less than or equal to LDTABL.");
    }
    if ncol <= 1 {
        prterr(4, "NCOL must be at least 2");
    }

    // Initialize KEY array (1-based)
    for idx in 1..=(ldkey << 1) as usize {
        *key.add(idx) = -9999;
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
            // table[i + j * ldtabl] with 1-based: table[(i-1) + (j-1)*ldtabl] = table[0-based]
            let tval = *table.add((idx - 1) + (jdx - 1) * ldtabl as usize);
            if tval < 0 {
                prterr(2, "All elements of TABLE must be nonnegative.");
            }
            *iro.add(idx) += tval;
        }
        ntot += *iro.add(idx);
    }

    if ntot == 0 {
        prterr(
            3,
            "All elements of TABLE are zero.\n\
                   PRT and PRE are set to missing values.",
        );
        *pre = amiss;
        *prt = amiss;
        return;
    }

    // Column marginals (1-based)
    for idx in 1..=ncol as usize {
        *ico.add(idx) = 0;
        for jdx in 1..=nrow as usize {
            *ico.add(idx) += *table.add((jdx - 1) + (idx - 1) * ldtabl as usize);
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
        if *iro.add(idx) + 1 <= imax / *kyy.add(idx) {
            *kyy.add(idx + 1) = *kyy.add(idx) * (*iro.add(idx) + 1);
        } else {
            prterr(5, ch_err_5);
            return;
        }
        idx += 1;
    }

    // Check for Maximum product
    if *iro.add(nro as usize) + 1 > imax / *kyy.add(nro as usize) {
        prterr(501, ch_err_5);
        return;
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
                dd += *fact.add(*table.add((jdx - 1) + (idx - 1) * ldtabl as usize) as usize);
                ntot += *table.add((jdx - 1) + (idx - 1) * ldtabl as usize);
            }
        } else {
            let mut ii_us: usize = (jdx - 1) * ldtabl as usize + 1;
            for idx in 1..=nro as usize {
                dd += *fact.add(*table.add(ii_us) as usize);
                ntot += *table.add(ii_us);
                ii_us += 1;
            }
        }
        obs += *fact.add(*ico.add(jdx) as usize) - dd;
    }

    // Denominator of observed table: DRO
    dro = f9xact(nro, ntot, iro.add(1), fact);
    *prt = (obs - dro).exp();
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
    jstp3 = (ldstp << 2) + 1;
    jstp4 = ldstp * 5 + 1;
    ikkey = 0;
    ikstp = 0;
    ikstp2 = ldstp << 1;
    ipo = 1;
    *ipoin.add(1) = 1;
    *stp.add(1) = 0.;
    *ifrq.add(1) = 1;
    *ifrq.add((ikstp2 + 1) as usize) = -1;

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
                    nro,
                    iro.add(1),
                    kyy.add(1),
                    key.add((ikkey + 1) as usize),
                    ldkey,
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
            // Recover pastp
            ipn = *ipoin.add((ipo + ikkey) as usize);
            pastp = *stp.add((ipn + ikstp) as usize);
            ifreq = *ifrq.add((ipn + ikstp) as usize);
            // Compute shortest and longest path
            if k1 > 1 {
                obs2 = obs
                    - *fact.add(*ico.add((kb + 1) as usize) as usize)
                    - *fact.add(*ico.add((kb + 2) as usize) as usize)
                    - ddf;
                for idx in 3..=k1 as usize {
                    obs2 -= *fact.add(*ico.add((kb as usize + idx) as usize) as usize);
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
                        REprintf(
                            std::ffi::CStr::from_bytes_with_nul(b"___ LP[itp=%d] = %g > 0\n\0")
                                .unwrap_or_else(|_| {
                                    std::ffi::CStr::from_ptr(b"\0".as_ptr() as *const _)
                                })
                                .as_ptr(),
                        );
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
                        REprintf(
                            std::ffi::CStr::from_bytes_with_nul(b"___ SP[itp=%d] = %g > 0\n\0")
                                .unwrap_or_else(|_| {
                                    std::ffi::CStr::from_ptr(b"\0".as_ptr() as *const _)
                                })
                                .as_ptr(),
                        );
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
                                tmp += *fact.add(*irn.add(nrb as usize + idx_i) as usize)
                                    - *fact.add((*irn.add(nrb as usize + idx_i) - 1) as usize);
                            }
                            tmp *= (k1 - 1) as c_double;
                            for idx_j in 1..=k1 as usize {
                                tmp += (nro2 - 1) as c_double
                                    * (*fact
                                        .add(*ico.add((kb as usize + idx_j) as usize) as usize)
                                        - *fact.add(
                                            (*ico.add((kb as usize + idx_j) as usize) - 1) as usize,
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
                        f5xact(
                            pastp + ddf,
                            tol,
                            &mut kval,
                            key.add(jkey as usize),
                            ldkey,
                            ipoin.add(jkey as usize),
                            stp.add(jstp as usize),
                            ldstp,
                            ifrq.add(jstp as usize),
                            ifrq.add(jstp2 as usize),
                            ifrq.add(jstp3 as usize),
                            ifrq.add(jstp4 as usize),
                            ifreq,
                            &mut itop,
                            psh,
                        );
                        psh = false;
                    }
                }
                // Get next PASTP on chain
                ipn = *ifrq.add((ipn + ikstp2) as usize);
                if ipn > 0 {
                    pastp = *stp.add((ipn + ikstp) as usize);
                    ifreq = *ifrq.add((ipn + ikstp) as usize);
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
                    nro,
                    iro.add(1),
                    kyy.add(1),
                    key.add((ikkey + 1) as usize),
                    ldkey,
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
    // All arrays are 1-based in C (adjusted with --array)

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
    let nr1 = nro - 1;
    let mut vmn: c_double = 1e100;

    'new_node: loop {
        // LnewNode: Setup to generate new node
        lev = 1;
        let nrt = *iro.add(irl as usize);
        let nct = *ico.add(1);
        *lb.add(1) = (((nrt as c_double + 1.) * (nct as c_double + 1.))
            / (nn as c_double + nr1 as c_double * nc1s as c_double + 1.)
            - tol) as c_int
            - 1;
        *nu.add(1) = (((nrt as c_double + nc1s as c_double) * (nct as c_double + nr1 as c_double))
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
                *alen.add(lev as usize) =
                    *alen.add((lev - 1) as usize) + *fact.add(*lb.add(lev as usize) as usize);
                if lev >= nc1s {
                    break;
                }

                let nn1 = *nt.add(lev as usize);
                let nrt_val = *nr.add(lev as usize);
                lev += 1;
                let nc1 = nco - lev;
                let nct_val = *ico.add(lev as usize);
                *lb.add(lev as usize) = (((nrt_val as c_double + 1.) * (nct_val as c_double + 1.))
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
                *alen.add(lev as usize) + *fact.add(*nr.add(lev as usize) as usize);
            *lb.add(nco as usize) = *nr.add(lev as usize);

            let v = val + *alen.add(nco as usize);

            if nro == 2 {
                // Only 1 row left
                let mut v2 = v
                    + *fact.add((*ico.add(1) - *lb.add(1)) as usize)
                    + *fact.add((*ico.add(2) - *lb.add(2)) as usize);
                for idx in 3..=nco as usize {
                    v2 += *fact.add((*ico.add(idx) - *lb.add(idx)) as usize);
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
                    + *fact.add(n11 as usize)
                    + *fact.add(n12 as usize)
                    + *fact.add((ic1 - n11) as usize)
                    + *fact.add((ic2 - n12) as usize);
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
            return -(*fact.add(*irow.add(1) as usize)
                + *fact.add(*icol.add(1) as usize)
                + *fact.add((*icol.add(1) - *irow.add(1)) as usize));
        } else {
            return -(*fact.add(*icol.add(1) as usize)
                + *fact.add(*irow.add(1) as usize)
                + *fact.add((*irow.add(1) - *icol.add(1)) as usize));
        }
    }

    // Parameter adjustments: irstk -= nrow + 1; icstk -= ncol + 1; --nrstk; --ncstk; --lstk; --mstk; --nstk; --ystk;
    // We handle 1-based indexing by using adjusted pointers

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

    // initialization before loop (1-based)
    for idx in 1..=nrow as usize {
        // irstk[i + nrow] = irow[nrow - i]
        *irstk.add(idx + nrow as usize) = *irow.add((nrow - idx as c_int + 1) as usize);
    }

    for idx in 1..=ncol as usize {
        // icstk[j + ncol] = icol[ncol - j]
        *icstk.add(idx + ncol as usize) = *icol.add((ncol - idx as c_int + 1) as usize);
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
            y += *fact.add(std::cmp::min(irt, ict) as usize);
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
                    y += *fact.add(*icstk.add(idx + (istk as usize + 1) * ncol as usize) as usize);
                }
                // goto L90
                break 'l60;
            }
            if nco == 1 {
                for idx in 1..=nro as usize {
                    y += *fact.add(*irstk.add(idx + (istk as usize + 1) * nrow as usize) as usize);
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

unsafe fn f5xact(
    pastp: c_double,
    tol: c_double,
    kval: *mut c_int,
    key: *mut c_int,
    ldkey: c_int,
    ipoin: *mut c_int,
    stp: *mut c_double,
    ldstp: c_int,
    ifrq: *mut c_int,
    npoin: *mut c_int,
    nr: *mut c_int,
    nl: *mut c_int,
    ifreq: c_int,
    itop: *mut c_int,
    psh: bool,
) {
    // Static variables carried across calls (C uses static)
    thread_local! {
        static ITMP: std::cell::Cell<c_int> = std::cell::Cell::new(0);
        static IRD: std::cell::Cell<c_int> = std::cell::Cell::new(0);
        static IPN: std::cell::Cell<c_int> = std::cell::Cell::new(0);
        static ITP: std::cell::Cell<c_int> = std::cell::Cell::new(0);
    }

    // All arrays are 1-based

    if psh {
        // Convert KVAL to int in range 0, ..., LDKEY-1
        let ird = *kval % ldkey;
        let mut found = false;
        let mut itp_val: c_int = 0;

        // Search for an unused location
        for idx in ird..ldkey {
            if *key.add(idx as usize) == *kval {
                itp_val = idx;
                found = true;
                break;
            }
            if *key.add(idx as usize) < 0 {
                itp_val = idx;
                break;
            }
        }
        if !found {
            for idx in 0..ird {
                if *key.add(idx as usize) == *kval {
                    itp_val = idx;
                    found = true;
                    break;
                }
                if *key.add(idx as usize) < 0 {
                    itp_val = idx;
                    break;
                }
            }
        }

        if !found && *key.add(itp_val as usize) != *kval {
            // Return if KEY array is full
            let msg = format!(
                "FEXACT error 6 (f5xact).  LDKEY={} is too small for this problem: kval={}.\n\
                 Try increasing the size of the workspace.",
                ldkey, *kval
            );
            prterr(6, &msg);
        }

        // L30: Update KEY
        ITP.set(itp_val);
        *key.add(itp_val as usize) = *kval;
        *itop += 1;
        *ipoin.add(itp_val as usize) = *itop;
        // Return if STP array full
        if *itop > ldstp {
            let msg = format!(
                "FEXACT error 7(update key). LDSTP={} is too small for this problem,\n  (kval={}, itop-ldstp={}).\n\
                 Increase workspace or consider using 'simulate.p.value=TRUE'.",
                ldstp,
                *kval,
                *itop - ldstp
            );
            prterr(7, &msg);
        }
        // Update STP, etc.
        *npoin.add(*itop as usize) = -1;
        *nr.add(*itop as usize) = -1;
        *nl.add(*itop as usize) = -1;
        *stp.add(*itop as usize) = pastp;
        *ifrq.add(*itop as usize) = ifreq;
        return;
    }

    // L40: Find location, if any, of pastp
    let itp_val = ITP.get();
    let mut ipn = *ipoin.add(itp_val as usize);

    let test1 = pastp - tol;
    let test2 = pastp + tol;

    loop {
        if *stp.add(ipn as usize) < test1 {
            ipn = *nl.add(ipn as usize);
        } else if *stp.add(ipn as usize) > test2 {
            ipn = *nr.add(ipn as usize);
        } else {
            if c_int::MAX - *ifrq.add(ipn as usize) < ifreq {
                let msg = "integer overflow in exact computation";
                prterr(99, msg);
            }
            *ifrq.add(ipn as usize) += ifreq;
            return;
        }
        if !(ipn > 0) {
            break;
        }
    }

    // Return if STP array full
    *itop += 1;
    if *itop > ldstp {
        let ipn0 = *ipoin.add(itp_val as usize);
        let msg = format!(
            "FEXACT error 7(location). LDSTP={} is too small for this problem,\n  (pastp={}, ipn_0:=ipoin[itp={}]= {}, stp[ipn_0]={}).\n\
             Increase workspace or consider using 'simulate.p.value=TRUE'",
            ldstp,
            pastp,
            itp_val,
            ipn0,
            *stp.add(ipn0 as usize)
        );
        prterr(7, &msg);
    }

    // Find location to add value
    ipn = *ipoin.add(itp_val as usize);
    let mut itmp = ipn;

    // L60:
    loop {
        if *stp.add(ipn as usize) < test1 {
            itmp = ipn;
            ipn = *nl.add(ipn as usize);
            if ipn > 0 {
                continue;
            }
            // else
            *nl.add(itmp as usize) = *itop;
            break;
        } else if *stp.add(ipn as usize) > test2 {
            itmp = ipn;
            ipn = *nr.add(ipn as usize);
            if ipn > 0 {
                continue;
            }
            // else
            *nr.add(itmp as usize) = *itop;
            break;
        } else {
            break;
        }
    }
    // Update STP, etc.
    *npoin.add(*itop as usize) = *npoin.add(itmp as usize);
    *npoin.add(itmp as usize) = *itop;
    *stp.add(*itop as usize) = pastp;
    *ifrq.add(*itop as usize) = ifreq;
    *nl.add(*itop as usize) = -1;
    *nr.add(*itop as usize) = -1;
}

unsafe fn f6xact(
    nrow: c_int,
    irow: *mut c_int,
    kyy: *const c_int,
    key: *mut c_int,
    ldkey: c_int,
    last: *mut c_int,
    ipn: *mut c_int,
) -> bool {
    // key is 1-based

    // L10:
    'l10: loop {
        *last += 1;
        if *last <= ldkey {
            if *key.add(*last as usize) < 0 {
                continue 'l10;
            }

            // Get KVAL from the stack
            let mut kval = *key.add(*last as usize);
            *key.add(*last as usize) = -9999;
            for j in (1..nrow as usize).rev() {
                *irow.add(j) = kval / *kyy.add(j);
                kval -= *irow.add(j) * *kyy.add(j);
            }
            *irow.add(0) = kval;
            *ipn = *last;
            return false;
        } else {
            *last = 0;
            return true;
        }
    }
}

unsafe fn f7xact(
    nrow: c_int,
    iro: *const c_int,
    idif: *mut c_int,
    k: *mut c_int,
    ks: *mut c_int,
) -> bool {
    // idif and iro are 1-based

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
        }
    }
    true
}

unsafe fn f8xact(irow: *const c_int, is: c_int, i1: c_int, izero: c_int, new: *mut c_int) {
    // new and irow are 1-based

    let mut i: c_int = 1;

    while i < i1 {
        *new.add(i as usize) = *irow.add(i as usize);
        i += 1;
    }

    while i <= izero - 1 {
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

unsafe fn f9xact(n: c_int, ntot: c_int, ir: *const c_int, fact: *const c_double) -> c_double {
    // ir is 0-based
    let mut d = *fact.add(ntot as usize);
    for idx in 0..n as usize {
        d -= *fact.add(*ir.add(idx) as usize);
    }
    d
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

    for idx in (0..=(nrow - 3) as usize).rev() {
        *nd.add(idx) += *nd.add(idx + 1);
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

unsafe fn f11act(irow: *const c_int, i1: c_int, i2: c_int, new: *mut c_int) {
    // All arrays are 0-based
    let mut i: c_int = 0;
    while i < i1 - 1 {
        *new.add(i as usize) = *irow.add(i as usize);
        i += 1;
    }
    while i <= i2 {
        *new.add((i - 1) as usize) = *irow.add(i as usize);
        i += 1;
    }
}

// ---- Exported public functions ----

/// Fisher's exact test entry point.
///
/// # Safety
/// All pointer arguments must be valid.
pub unsafe fn fexact(
    nrow: c_int,
    ncol: c_int,
    table: *const c_int,
    ldtabl: c_int,
    expect: c_double,
    percnt: c_double,
    emin: c_double,
    prt: *mut c_double,
    pre: *mut c_double,
    workspace: c_int,
    mult: c_int,
) {
    let amiss: c_double = -12345.;

    let i_real: c_int = 4;
    let i_int_val: c_int = 2;

    let mut nco: c_int;
    let mut nro: c_int;
    let ntot: c_int;
    let mut numb: c_int;
    let mut iiwk: c_int;
    let mut irwk: c_int;

    let mut i: c_int;
    let j: c_int;
    let k: c_int;
    let kk: c_int;
    let ldkey: c_int;
    let ldstp: c_int;
    let i1: c_int;
    let i2: c_int;
    let i3: c_int;
    let i4: c_int;
    let i5: c_int;
    let i6: c_int;
    let i7: c_int;
    let i8: c_int;
    let i9: c_int;
    let i10: c_int;
    let i3a: c_int;
    let i3b: c_int;
    let i3c: c_int;
    let i9a: c_int;

    let iwkmax = 2 * (workspace / 2);
    let mut iwkpt: c_int = 0;
    let n2_stack = std::cmp::max(200, iwkmax / 1000);

    // Workspace Allocation: equiv = R_alloc(iwkmax / 2, sizeof(double))
    // equiv serves as dwrk (doubles), iwrk (ints), rwrk (floats) via pointer casts
    let equiv = R_alloc(std::mem::size_of::<c_double>(), (iwkmax / 2) as usize) as *mut c_double;
    // dwrk = equiv, iwrk = equiv as int*, rwrk = equiv as float*

    if nrow > ldtabl {
        prterr(1, "NROW must be less than or equal to LDTABL.");
    }

    let mut ntot_val: c_int = 0;
    for idx_i in 0..nrow as usize {
        for idx_j in 0..ncol as usize {
            if *table.add(idx_i + idx_j * ldtabl as usize) < 0 {
                prterr(2, "All elements of TABLE must be nonnegative.");
            }
            ntot_val += *table.add(idx_i + idx_j * ldtabl as usize);
        }
    }
    if ntot_val == 0 {
        prterr(
            3,
            "All elements of TABLE are zero.\n\
                   PRT and PRE are set to missing values.",
        );
        *pre = amiss;
        *prt = amiss;
        return;
    }

    // nco := max(nrow, ncol), nro := min(nrow, ncol)
    if ncol > nrow {
        nco = ncol;
        nro = nrow;
    } else {
        nco = nrow;
        nro = ncol;
    }
    k = nrow + ncol + 1;
    kk = k * nco;

    i1 = iwork(iwkmax, &mut iwkpt, ntot_val + 1, i_real);
    i2 = iwork(iwkmax, &mut iwkpt, nco, i_int_val);
    i3 = iwork(iwkmax, &mut iwkpt, nco, i_int_val);
    i3a = iwork(iwkmax, &mut iwkpt, nco, i_int_val);
    i3b = iwork(iwkmax, &mut iwkpt, nro, i_int_val);
    i3c = iwork(iwkmax, &mut iwkpt, nro, i_int_val);
    let mut ikh = std::cmp::max(k * 5 + (kk << 1), nco * 7 + 4 * n2_stack);
    iiwk = iwork(iwkmax, &mut iwkpt, ikh, i_int_val);
    ikh = std::cmp::max(nco + 1 + 2 * n2_stack, k);
    irwk = iwork(iwkmax, &mut iwkpt, ikh, i_real);

    // Double precision reals
    numb = 18 + 10 * mult;
    ldkey = (iwkmax - iwkpt) / numb - 1;
    if (mult as c_double) * (ldkey as c_double) > c_int::MAX as c_double {
        let msg = format!(
            "integer overflow would happen in 'mult * ldkey' = {}",
            (mult as c_double) * (ldkey as c_double)
        );
        prterr(99, &msg);
    }
    ldstp = mult * ldkey;
    i4 = iwork(iwkmax, &mut iwkpt, ldkey << 1, i_int_val);
    i5 = iwork(iwkmax, &mut iwkpt, ldkey << 1, i_int_val);
    i6 = iwork(iwkmax, &mut iwkpt, ldstp << 1, i_real);
    i7 = iwork(iwkmax, &mut iwkpt, ldstp * 6, i_int_val);
    i8 = iwork(iwkmax, &mut iwkpt, ldkey << 1, i_real);
    i9 = iwork(iwkmax, &mut iwkpt, ldkey << 1, i_real);
    i9a = iwork(iwkmax, &mut iwkpt, ldkey << 1, i_real);
    i10 = iwork(iwkmax, &mut iwkpt, ldkey << 1, i_int_val);

    // Call f2xact with workspace pointers
    // dwrk + i1 = equiv as double + i1
    // iwrk + i2 = equiv as int + i2 (iwrk and dwrk share the same memory, cast accordingly)
    let dwrk = equiv;
    let iwrk = equiv as *mut c_int;

    f2xact(
        nrow,
        ncol,
        table,
        ldtabl,
        expect,
        percnt,
        emin,
        prt,
        pre,
        dwrk.add(i1 as usize),
        iwrk.add(i2 as usize),
        iwrk.add(i3 as usize),
        iwrk.add(i3a as usize),
        iwrk.add(i3b as usize),
        iwrk.add(i3c as usize),
        iwrk.add(i4 as usize),
        ldkey,
        iwrk.add(i5 as usize),
        dwrk.add(i6 as usize),
        ldstp,
        iwrk.add(i7 as usize),
        dwrk.add(i8 as usize),
        dwrk.add(i9 as usize),
        dwrk.add(i9a as usize),
        iwrk.add(i10 as usize),
        iwrk.add(iiwk as usize),
        dwrk.add(irwk as usize),
        n2_stack,
    );
}

/// SEXP wrapper for Fisher's exact test.
///
/// # Safety
/// x, pars, work, smult must be valid SEXP pointers.
pub unsafe fn Fexact(x: SEXP, pars: SEXP, work: SEXP, smult: SEXP) -> SEXP {
    let nr = crate::main::util_main::nrows(x as *const std::ffi::c_void);
    let nc = crate::main::util_main::ncols(x as *const std::ffi::c_void);

    fn asInteger_local(s: SEXP) -> c_int {
        unsafe { crate::main::coerce::asInteger(s) }
    }

    let ws = asInteger_local(work);
    let mult = asInteger_local(smult);

    let pars = coerceVector(pars, SEXPTYPE::REALSXP.0);
    let pars = Rf_protect(pars);

    let mut p: c_double = 0.;
    let mut prt: c_double = 0.;
    let rp = REAL(pars);

    fexact(
        nr,
        nc,
        INTEGER(x),
        nr,
        *rp.add(0),
        *rp.add(1),
        *rp.add(2),
        &mut prt,
        &mut p,
        ws,
        mult,
    );

    Rf_unprotect(1);

    // ScalarReal: allocate a 1-element REALSXP
    let ans = Rf_allocVector(SEXPTYPE::REALSXP.0, 1);
    *REAL(ans).add(0) = p;
    ans
}
