/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 1997-2025  The R Core Team
 *
 *  Ported to Rust from r-source/src/library/grDevices/src/chull.c
 *
 *  Convex hull algorithm based on ACM TOMS algorithm 523 by W. F. Eddy.
 */

use crate::mainutils::coerce::coerceVector;
use crate::mainutils::util_main::nrows;
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::*;
use crate::sexp::globals::*;
use crate::sexp::protect::*;
use std::os::raw::{c_double, c_int};

/// split() partitions m points by the line joining points ii and jj.
unsafe fn split(
    n: c_int,
    x: *const c_double,
    m: c_int,
    in_: *const c_int,
    ii: c_int,
    jj: c_int,
    s: c_int,
    iabv: *mut c_int,
    na: *mut c_int,
    maxa: *mut c_int,
    ibel: *mut c_int,
    nb: *mut c_int,
    maxb: *mut c_int,
) {
    unsafe {
        // Note: x is stored as (2, n) matrix — x[k] = x-coordinate, x[k+n] = y-coordinate
        // Using 0-based indexing (unlike C's Fortran-style 1-based)
        let xt = *x.add((ii - 1) as usize);
        let vert = *x.add((jj - 1) as usize) == xt;
        let d1 = *x.add((jj - 1 + n) as usize) - *x.add((ii - 1 + n) as usize);

        let (a, b, neg_dir): (f64, f64, bool);
        if vert {
            neg_dir = (s > 0 && d1 < 0.) || (s < 0 && d1 > 0.);
            a = 0.0;
            b = 0.0;
        } else {
            a = d1 / (*x.add((jj - 1) as usize) - xt);
            b = *x.add((ii - 1 + n) as usize) - a * xt;
            neg_dir = false;
        }

        let mut up = 0.0;
        let mut down = 0.0;
        *na = 0;
        *maxa = 0;
        *nb = 0;
        *maxb = 0;

        for i in 0..m as usize {
            let is = *in_.add(i) as usize; // 1-based subscript from in[]
            let z = if vert {
                if neg_dir {
                    xt - *x.add((is - 1) as usize)
                } else {
                    *x.add((is - 1) as usize) - xt
                }
            } else {
                *x.add(is - 1 + n as usize) - a * *x.add((is - 1) as usize) - b
            };

            if z > 0. {
                if s == -2 {
                    continue;
                }
                *iabv.add(*na as usize) = *in_.add(i);
                *na += 1;
                if z >= up {
                    up = z;
                    *maxa = *na;
                }
            } else if s != 2 && z < 0. {
                *ibel.add(*nb as usize) = *in_.add(i);
                *nb += 1;
                if z <= down {
                    down = z;
                    *maxb = *nb;
                }
            }
        }
    }
}

/// Internal convex hull computation.
/// Ported from in_chull() — uses 1-based indexing internally.
unsafe fn in_chull(
    n: *mut c_int,
    x: *mut c_double,
    m: *mut c_int,
    in_: *mut c_int,
    ia: *mut c_int,
    ib: *mut c_int,
    ih: *mut c_int,
    nh: *mut c_int,
    il: *mut c_int,
) {
    unsafe {
        // All arrays use 1-based indexing (C/Fortran style)
        // x[k] = x-coordinate, x[k + *n] = y-coordinate

        let mut i: c_int;
        let mut j: c_int;
        let mut ilinh: c_int;
        let mut ma: c_int = 0;
        let mut mb: c_int = 0;
        let mut kn: c_int = 0;
        let mut mm: c_int = 0;
        let mut kx: c_int = 0;
        let mut mx: c_int = 0;
        let mut mp1: c_int = 0;
        let mut mbb: c_int = 0;
        let mut nia: c_int = 0;
        let mut nib: c_int = 0;
        let mut inh: c_int = 0;
        let mut min_: c_int = 0;
        let mut mxa: c_int = 0;
        let mut mxb: c_int = 0;
        let mut mxbb: c_int = 0;

        let nval = *n;
        let mut mval = *m;

        let x_dim1 = nval;

        // Macro: y(k) = x[k + x_dim1] (1-based)
        let y = |k: c_int| -> f64 { *x.add((k + x_dim1 - 1) as usize) };

        if mval == 1 {
            *nh = 2;
            *ih.add(1) = *in_.add(1);
            *il.add(1) = 1;
            return;
        }

        *il.add(1) = 2;
        *il.add(2) = 1;
        kn = *in_.add(1);
        kx = *in_.add(2);

        if mval == 2 {
            // L_2pts
            *ih.add(1) = kx;
            *ih.add(2) = kn;
            if *x.add((kn - 1) as usize) == *x.add((kx - 1) as usize) && y(kn) == y(kx) {
                *nh = 2;
            } else {
                *nh = 3;
            }
            *nh -= 1;
            // put results in order
            for i in 1..=*nh {
                *ia.add(i as usize) = *ih.add(i as usize);
            }
            j = *il.add(1);
            for i in 2..=*nh {
                *ih.add(i as usize) = *ia.add(j as usize);
                j = *il.add(j as usize);
            }
            return;
        }

        mp1 = mval + 1;
        min_ = 1;
        mx = 1;
        kx = *in_.add(1);
        let mut maxe = false;
        let mut mine = false;

        // Find two vertices of the convex hull
        for i in 2..=mval {
            j = *in_.add(i as usize);
            let d1 = *x.add((j - 1) as usize) - *x.add((kx - 1) as usize);
            if d1 < 0. {
                // do nothing
            } else if d1 == 0. {
                maxe = true;
            } else {
                maxe = false;
                mx = i;
                kx = j;
            }
            let d1 = *x.add((j - 1) as usize) - *x.add((kn - 1) as usize);
            if d1 < 0. {
                mine = false;
                min_ = i;
                kn = j;
            } else if d1 == 0. {
                mine = true;
            }
        }

        if kx == kn {
            // All points on a vertical line
            kx = *in_.add(1);
            kn = *in_.add(1);
            for i in 1..=mval {
                j = *in_.add(i as usize);
                if y(j) > y(kx) {
                    mx = i;
                    kx = j;
                }
                if y(j) < y(kn) {
                    min_ = i;
                    kn = j;
                }
            }
            if kx == kn {
                // Single point
                *nh = 2;
                *ih.add(1) = *in_.add(1);
                *il.add(1) = 1;
                return;
            }
            // Two points
            *ih.add(1) = kx;
            *ih.add(2) = kn;
            if *x.add((kn - 1) as usize) == *x.add((kx - 1) as usize) && y(kn) == y(kx) {
                *nh = 2;
            } else {
                *nh = 3;
            }
            *nh -= 1;
            for i in 1..=*nh {
                *ia.add(i as usize) = *ih.add(i as usize);
            }
            j = *il.add(1);
            for i in 2..=*nh {
                *ih.add(i as usize) = *ia.add(j as usize);
                j = *il.add(j as usize);
            }
            return;
        }

        if maxe || mine {
            if maxe {
                for i in 1..=mval {
                    j = *in_.add(i as usize);
                    if *x.add((j - 1) as usize) != *x.add((kx - 1) as usize) {
                        continue;
                    }
                    if y(j) <= y(kx) {
                        continue;
                    }
                    mx = i;
                    kx = j;
                }
            }
            if mine {
                for i in 1..=mval {
                    j = *in_.add(i as usize);
                    if *x.add((j - 1) as usize) != *x.add((kn - 1) as usize) {
                        continue;
                    }
                    if y(j) >= y(kn) {
                        continue;
                    }
                    min_ = i;
                    kn = j;
                }
            }
        }

        *ih.add(1) = kx;
        *ih.add(2) = kn;
        *nh = 3;
        inh = 1;
        nib = 1;
        ma = mval;
        *in_.add(mx as usize) = *in_.add(mval as usize);
        *in_.add(mval as usize) = kx;
        mm = mval - 2;
        if min_ == mval {
            min_ = mx;
        }
        *in_.add(min_ as usize) = *in_.add((mval - 1) as usize);
        *in_.add((mval - 1) as usize) = kn;

        // Begin partitioning
        split(
            nval,
            x,
            mm,
            in_,
            *ih.add(1),
            *ih.add(2),
            0,
            ia,
            &mut mb,
            &mut mxa,
            ib,
            ia.offset(ma as isize),
            &mut mxbb,
        );

        // Traverse LEFT HALF of the tree
        nib += *ia.offset(ma as isize);
        ma -= 1;

        'left_half: loop {
            if mxa != 0 {
                *il.add(*nh as usize) = *il.add(inh as usize);
                *il.add(inh as usize) = *nh;
                *ih.add(*nh as usize) = *ia.add(mxa as usize);
                *ia.add(mxa as usize) = *ia.add(mb as usize);
                mb -= 1;
                *nh += 1;
                if mb != 0 {
                    ilinh = *il.add(inh as usize);
                    split(
                        nval,
                        x,
                        mb,
                        ia,
                        *ih.add(inh as usize),
                        *ih.add(ilinh as usize),
                        1,
                        ia,
                        &mut mbb,
                        &mut mxa,
                        ib.offset(nib as isize),
                        ia.offset(ma as isize),
                        &mut mxb,
                    );
                    mb = mbb;
                    continue 'left_half;
                }
                inh = *il.add(inh as usize);
            }

            loop {
                inh = *il.add(inh as usize);
                ma += 1;
                nib -= *ia.offset(ma as isize);
                if ma >= mval {
                    break 'left_half;
                }
                if *ia.offset(ma as isize) != 0 {
                    break;
                }
            }
            ilinh = *il.add(inh as usize);
            split(
                nval,
                x,
                *ia.offset(ma as isize),
                ib.offset(nib as isize),
                *ih.add(inh as usize),
                *ih.add(ilinh as usize),
                2,
                ia,
                &mut mb,
                &mut mxa,
                ib.offset(nib as isize),
                &mut mbb,
                &mut mxb,
            );
            *ia.offset(ma as isize) = mbb;
        }

        // Traverse RIGHT HALF of the tree
        mxb = mxbb;
        ma = mval;
        mb = *ia.offset(mval as isize);
        nia = 1;
        *ia.offset(mval as isize) = 0;

        'right_half: loop {
            nia += *ia.offset(ma as isize);
            ma -= 1;

            if mxb != 0 {
                *il.add(*nh as usize) = *il.add(inh as usize);
                *il.add(inh as usize) = *nh;
                *ih.add(*nh as usize) = *ib.add(mxb as usize);
                *ib.add(mxb as usize) = *ib.add(mb as usize);
                mb -= 1;
                *nh += 1;
                if mb != 0 {
                    ilinh = *il.add(inh as usize);
                    split(
                        nval,
                        x,
                        mb,
                        ib.offset(nib as isize),
                        *ih.add(inh as usize),
                        *ih.add(ilinh as usize),
                        -1,
                        ia.offset(nia as isize),
                        ia.offset(ma as isize),
                        &mut mxa,
                        ib.offset(nib as isize),
                        &mut mbb,
                        &mut mxb,
                    );
                    mb = mbb;
                    continue 'right_half;
                }
                inh = *il.add(inh as usize);
            }

            loop {
                inh = *il.add(inh as usize);
                ma += 1;
                if ma == mp1 {
                    break 'right_half;
                }
                nia -= *ia.offset(ma as isize);
                if *ia.offset(ma as isize) != 0 {
                    break;
                }
            }
            ilinh = *il.add(inh as usize);
            split(
                nval,
                x,
                *ia.offset(ma as isize),
                ia.offset(nia as isize),
                *ih.add(inh as usize),
                *ih.add(ilinh as usize),
                -2,
                ia.offset(nia as isize),
                &mut mbb,
                &mut mxa,
                ib.offset(nib as isize),
                &mut mb,
                &mut mxb,
            );
        }

        // Finis: put results in order
        *nh -= 1;
        for i in 1..=*nh {
            *ia.add(i as usize) = *ih.add(i as usize);
        }
        j = *il.add(1);
        for i in 2..=*nh {
            *ih.add(i as usize) = *ia.add(j as usize);
            j = *il.add(j as usize);
        }
    }
}

/// Compute the convex hull of a set of 2D points.
/// x is a two-column numeric matrix.
pub unsafe fn chull(x: SEXP) -> SEXP {
    unsafe {
        let n = nrows(x as *const std::ffi::c_void);
        if n <= 0 {
            return Rf_allocVector(SEXPTYPE::INTSXP, 0);
        }

        // Allocate work arrays using Vec::leak
        let mut in_vec: Vec<c_int> = (1..=n).collect();
        let mut ih_vec: Vec<c_int> = vec![0; (4 * n) as usize];
        let mut ia_vec: Vec<c_int> = vec![0; (4 * n) as usize];
        let mut ib_vec: Vec<c_int> = vec![0; (4 * n) as usize];
        let mut il_vec: Vec<c_int> = vec![0; (4 * n) as usize];

        let x = coerceVector(x, SEXPTYPE::REALSXP.into());
        let _x_guard = protect(x);
        let x_data = REAL(x) as *mut c_double;

        let mut n_mut = n;
        let mut m_mut = n;
        let mut nh: c_int = 0;

        in_chull(
            &mut n_mut,
            x_data,
            &mut m_mut,
            in_vec.as_mut_ptr(),
            ia_vec.as_mut_ptr().add(n as usize),
            ia_vec.as_mut_ptr().add(2 * n as usize),
            ih_vec.as_mut_ptr(),
            &mut nh,
            il_vec.as_mut_ptr().add(3 * n as usize),
        );

        let ans = Rf_allocVector(SEXPTYPE::INTSXP, nh);
        let _ans_guard = protect(ans);
        let ians = INTEGER(ans);
        for i in 0..nh as usize {
            // Reverse order to match C output
            *ians.add(i) = ih_vec.get(nh as usize - 1 - i).copied().unwrap_or(0);
        }

        ans
    }
}
