#![allow(unsafe_op_in_unsafe_fn)] // legacy C-port unsafe boundary; see docs/unsafe-op-allowlist.tsv.
//! Hierarchical clustering utilities: cutree
//! Port of r-source/src/library/stats/src/hclust-utils.c

use std::os::raw::c_int;
use std::ptr;

use crate::main::array::allocMatrix;
use crate::main::coerce::coerceVector;
use crate::main::util_main::nrows;
use crate::sexp::accessors::{INTEGER, LENGTH};
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::protect::{Rf_protect, Rf_unprotect};

pub unsafe fn cutree(merge: SEXP, which: SEXP) -> SEXP {
    let merge = Rf_protect(coerceVector(merge, SEXPTYPE::INTSXP.as_c_int()));
    let i_merge = INTEGER(merge);

    let which = Rf_protect(coerceVector(which, SEXPTYPE::INTSXP.as_c_int()));
    let i_which = INTEGER(which);

    let n = nrows(merge as *const std::ffi::c_void) + 1;

    // Using 1-based indices
    let mut sing = vec![true; (n + 1) as usize];
    let mut m_nr = vec![0i32; (n + 1) as usize];
    let mut z = vec![0i32; (n + 1) as usize];

    let ans = Rf_protect(allocMatrix(SEXPTYPE::INTSXP.into(), n, LENGTH(which)));
    let i_ans = INTEGER(ans);

    let mut k: c_int = 1;
    while k <= n {
        sing[k as usize] = true;
        m_nr[k as usize] = 0;
        k += 1;
    }

    let mut k: c_int = 1;
    while k < n {
        let mut m1 = *i_merge.add((k - 1) as usize);
        let mut m2 = *i_merge.add((n - 1 + k - 1) as usize);

        if m1 < 0 && m2 < 0 {
            m_nr[(-m1) as usize] = k;
            m_nr[(-m2) as usize] = k;
            sing[(-m1) as usize] = false;
            sing[(-m2) as usize] = false;
        } else if m1 < 0 || m2 < 0 {
            let mut j: c_int;
            if m1 < 0 {
                j = -m1;
                m1 = m2;
            } else {
                j = -m2;
            }
            let mut l: c_int = 1;
            while l <= n {
                if m_nr[l as usize] == m1 {
                    m_nr[l as usize] = k;
                }
                l += 1;
            }
            m_nr[j as usize] = k;
            sing[j as usize] = false;
        } else {
            let mut l: c_int = 1;
            while l <= n {
                if m_nr[l as usize] == m1 || m_nr[l as usize] == m2 {
                    m_nr[l as usize] = k;
                }
                l += 1;
            }
        }

        let mut found_j = false;
        let mut mm: c_int = 0;
        let mut j: c_int = 0;
        while j < LENGTH(which) {
            if *i_which.add(j as usize) == n - k {
                if !found_j {
                    found_j = true;
                    let mut l: c_int = 1;
                    while l <= n {
                        z[l as usize] = 0;
                        l += 1;
                    }
                    let mut nclust: c_int = 0;
                    mm = j * n;
                    let mut l: c_int = 1;
                    let mut m1_idx = mm;
                    while l <= n {
                        if sing[l as usize] {
                            nclust += 1;
                            *i_ans.add(m1_idx as usize) = nclust;
                        } else {
                            if z[m_nr[l as usize] as usize] == 0 {
                                nclust += 1;
                                z[m_nr[l as usize] as usize] = nclust;
                            }
                            *i_ans.add(m1_idx as usize) = z[m_nr[l as usize] as usize];
                        }
                        l += 1;
                        m1_idx += 1;
                    }
                } else {
                    let mut l: c_int = 1;
                    let mut m1_idx = j * n;
                    let mut m2_idx = mm;
                    while l <= n {
                        *i_ans.add(m1_idx as usize) = *i_ans.add(m2_idx as usize);
                        l += 1;
                        m1_idx += 1;
                        m2_idx += 1;
                    }
                }
            }
            j += 1;
        }
        k += 1;
    }

    // Trivial case which[] = n:
    let mut j: c_int = 0;
    while j < LENGTH(which) {
        if *i_which.add(j as usize) == n {
            let mut l: c_int = 1;
            let mut m1 = j * n;
            while l <= n {
                *i_ans.add(m1 as usize) = l;
                l += 1;
                m1 += 1;
            }
        }
        j += 1;
    }

    Rf_unprotect(3);
    ans
}
