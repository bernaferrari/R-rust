//! Hierarchical clustering utilities: cutree
//! Port of r-source/src/library/stats/src/hclust-utils.c

use crate::sexp::accessors::{INTEGER_ELT, REAL_ELT, SET_INTEGER_ELT, SET_REAL_ELT};
use std::os::raw::c_int;
use std::slice;

use crate::main::array::allocMatrix;
use crate::main::coerce::coerceVector;
use crate::main::util_main::nrows;
use crate::sexp::accessors::{INTEGER, LENGTH};
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::protect::protect as protect_sexp;

pub fn cutree(merge: SEXP, which: SEXP) -> SEXP {
    let merge = coerceVector(merge, SEXPTYPE::INTSXP.as_c_int());
    let _merge_guard = protect_sexp(merge);
    let i_merge_len = LENGTH(merge);
    let i_merge = unsafe { slice::from_raw_parts(INTEGER(merge), i_merge_len as usize) };

    let which = coerceVector(which, SEXPTYPE::INTSXP.as_c_int());
    let _which_guard = protect_sexp(which);
    let which_len = LENGTH(which);
    let i_which = unsafe { slice::from_raw_parts(INTEGER(which), which_len as usize) };

    let n = unsafe { nrows(merge as *const std::ffi::c_void) + 1 };

    // Using 1-based indices
    let mut sing = vec![true; (n + 1) as usize];
    let mut m_nr = vec![0i32; (n + 1) as usize];
    let mut z = vec![0i32; (n + 1) as usize];

    let ans = unsafe { allocMatrix(SEXPTYPE::INTSXP.into(), n, which_len) };
    let _ans_guard = protect_sexp(ans);
    let i_ans = unsafe { slice::from_raw_parts_mut(INTEGER(ans), (n * which_len) as usize) };

    let mut k: c_int = 1;
    while k <= n {
        sing[k as usize] = true;
        m_nr[k as usize] = 0;
        k += 1;
    }

    let mut k: c_int = 1;
    while k < n {
        let mut m1 = i_merge[(k - 1) as usize];
        let mut m2 = i_merge[(n - 1 + k - 1) as usize];

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
        while j < which_len {
            if i_which[j as usize] == n - k {
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
                            i_ans[m1_idx as usize] = nclust;
                        } else {
                            if z[m_nr[l as usize] as usize] == 0 {
                                nclust += 1;
                                z[m_nr[l as usize] as usize] = nclust;
                            }
                            i_ans[m1_idx as usize] = z[m_nr[l as usize] as usize];
                        }
                        l += 1;
                        m1_idx += 1;
                    }
                } else {
                    let mut l: c_int = 1;
                    let mut m1_idx = j * n;
                    let mut m2_idx = mm;
                    while l <= n {
                        i_ans[m1_idx as usize] = i_ans[m2_idx as usize];
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
    while j < which_len {
        if i_which[j as usize] == n {
            let mut l: c_int = 1;
            let mut m1 = j * n;
            while l <= n {
                i_ans[m1 as usize] = l;
                l += 1;
                m1 += 1;
            }
        }
        j += 1;
    }

    ans
}
pub unsafe extern "C-unwind" fn c_cutree(merge: SEXP, which: SEXP) -> SEXP {
    cutree(merge, which)
}

fn list_elt(list: SEXP, name: &str) -> SEXP {
    use crate::sexp::accessors::{STRING_ELT, TYPEOF, VECTOR_ELT, XLENGTH};
    use crate::sexp::globals::R_NilValue;
    if list.is_null() || list == R_NilValue() || TYPEOF(list) != SEXPTYPE::VECSXP {
        return R_NilValue();
    }
    let names =
        crate::sexp::attrib_core::getAttrib(list, crate::sexp::attrib_core::R_NamesSymbol());
    if names.is_null() || names == R_NilValue() || TYPEOF(names) != SEXPTYPE::STRSXP {
        return R_NilValue();
    }
    for i in 0..XLENGTH(names) {
        let s = STRING_ELT(names, i);
        if s.is_null() {
            continue;
        }
        let raw = s;
        if raw.is_null() {
            continue;
        }
        if crate::library::stats::checked::char_string(raw) == name {
            return VECTOR_ELT(list, i);
        }
    }
    R_NilValue()
}

/// GNU `cutree(tree, k)` / `cutree(tree, h)`.
pub fn do_cutree(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    use crate::sexp::accessors::CHAR;
    use crate::sexp::accessors::PRINTNAME;
    use crate::sexp::accessors::{CAR, CDR, INTEGER, REAL, SET_VECTOR_ELT, TAG, TYPEOF, XLENGTH};
    use crate::sexp::constructors::{Rf_ScalarInteger, Rf_allocVector3};
    use crate::sexp::ffi::NA_REAL;
    use crate::sexp::globals::R_NilValue;

    let mut tree = R_NilValue();
    let mut k = NA_REAL;
    let mut h = NA_REAL;
    let mut have_k = false;
    let mut have_h = false;
    let mut cell = args;
    let mut pos = 0;
    while !cell.is_null() && cell != R_NilValue() {
        let v = CAR(cell);
        let tag = TAG(cell);
        let name = if !tag.is_null() && tag != R_NilValue() {
            crate::library::stats::checked::char_string(PRINTNAME(tag))
        } else {
            String::new()
        };
        match name.as_str() {
            "tree" => tree = v,
            "k" => {
                if !v.is_null() && v != R_NilValue() {
                    k = if TYPEOF(v) == SEXPTYPE::INTSXP && XLENGTH(v) > 0 {
                        INTEGER_ELT(v, 0) as f64
                    } else if TYPEOF(v) == SEXPTYPE::REALSXP && XLENGTH(v) > 0 {
                        REAL_ELT(v, 0)
                    } else {
                        NA_REAL
                    };
                    have_k = k.is_finite();
                }
            }
            "h" => {
                if !v.is_null() && v != R_NilValue() {
                    h = if TYPEOF(v) == SEXPTYPE::REALSXP && XLENGTH(v) > 0 {
                        REAL_ELT(v, 0)
                    } else if TYPEOF(v) == SEXPTYPE::INTSXP && XLENGTH(v) > 0 {
                        INTEGER_ELT(v, 0) as f64
                    } else {
                        NA_REAL
                    };
                    have_h = h.is_finite();
                }
            }
            _ if name.is_empty() => {
                if pos == 0 {
                    tree = v;
                } else if pos == 1 && !have_k && !have_h {
                    if !v.is_null() && v != R_NilValue() {
                        k = if TYPEOF(v) == SEXPTYPE::INTSXP && XLENGTH(v) > 0 {
                            INTEGER_ELT(v, 0) as f64
                        } else if TYPEOF(v) == SEXPTYPE::REALSXP && XLENGTH(v) > 0 {
                            REAL_ELT(v, 0)
                        } else {
                            NA_REAL
                        };
                        have_k = k.is_finite();
                    }
                }
                pos += 1;
            }
            _ => {}
        }
        cell = CDR(cell);
    }
    let merge = list_elt(tree, "merge");
    if merge.is_null() || merge == R_NilValue() {
        return R_NilValue();
    }
    let n = crate::library::stats::checked::nrows(merge) + 1;
    let mut which_k = if have_k { k.round() as i32 } else { 0 };
    if !have_k && have_h {
        let height = list_elt(tree, "height");
        let nmerge = n - 1;
        let mut first = nmerge + 1; // Inf
        if !height.is_null() && height != R_NilValue() {
            for i in 0..nmerge {
                let hi = if TYPEOF(height) == SEXPTYPE::REALSXP {
                    REAL_ELT(height, i as i32)
                } else {
                    INTEGER_ELT(height, i as i32) as f64
                };
                if hi > h {
                    first = i + 1;
                    break;
                }
            }
        }
        which_k = n + 1 - first;
    }
    if which_k < 1 {
        which_k = 1;
    }
    if which_k > n {
        which_k = n;
    }
    let ksexp = Rf_ScalarInteger(which_k);
    let _ks = protect_sexp(ksexp);
    let ans = cutree(merge, ksexp);
    let _ans = protect_sexp(ans);
    let out = Rf_allocVector3(SEXPTYPE::INTSXP, n as i64);
    let _o = protect_sexp(out);
    for i in 0..n {
        SET_INTEGER_ELT(out, i as i32, INTEGER_ELT(ans, i as i32));
    }
    let _ = SET_VECTOR_ELT;
    out
}

pub unsafe extern "C-unwind" fn c_mono_fc_m(m: SEXP, sx: SEXP) -> SEXP {
    let n = crate::sexp::accessors::XLENGTH(m);
    if n < 2 {
        crate::main::errors::Rf_error(c"length(m) must be at least two".as_ptr());
    }
    if crate::sexp::accessors::TYPEOF(sx) != SEXPTYPE::REALSXP
        || crate::sexp::accessors::XLENGTH(sx) != n - 1
    {
        crate::main::errors::Rf_error(
            c"Argument Sx must be numeric vector one shorter than m[]".as_ptr(),
        );
    }
    let val = crate::mainutils::seq::Rf_duplicate(m);
    let _g = protect_sexp(val);
    for k in 0..(n - 1) as usize {
        let sk = REAL_ELT(sx, k as i32);
        if sk == 0.0 {
            SET_REAL_ELT(val, k as i32, 0.0);
            SET_REAL_ELT(val, (k + 1) as i32, 0.0);
        } else {
            let alpha = REAL_ELT(val, k as i32) / sk;
            let beta = REAL_ELT(val, (k + 1) as i32) / sk;
            let a2b3 = 2.0 * alpha + beta - 3.0;
            let ab23 = alpha + 2.0 * beta - 3.0;
            if a2b3 > 0.0 && ab23 > 0.0 && alpha * (a2b3 + ab23) < a2b3 * a2b3 {
                let tau_s = 3.0 * sk / (alpha * alpha + beta * beta).sqrt();
                SET_REAL_ELT(val, k as i32, tau_s * alpha);
                SET_REAL_ELT(val, (k + 1) as i32, tau_s * beta);
            }
        }
    }
    val
}
