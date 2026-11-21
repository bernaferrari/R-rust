/* --- Isotonic regression ---
 * code simplified from VR_mds_fn() which is part of MASS.c,
 * Copyright (C) 1995  Brian Ripley
 * ---
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2003-2024	The R Core Team
 *  Copyright (C) 2003-2023	The R Foundation
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program; if not, a copy is available at
 *  https://www.R-project.org/Licenses/
 */

//! Isotonic regression
//! Port of r-source/src/library/stats/src/isoreg.c

use std::os::raw::{c_double, c_int};
use std::ptr;

use crate::main::errors::{Rf_error, Rf_error_cstr};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{R_FINITE, R_xlen_t, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

const R_PosInf: c_double = f64::INFINITY;

fn error(msg: &str) {
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    Rf_error_cstr(&c_msg);
}

/// Create a named list (simplified mkNamed for known patterns)
fn mkNamed(sexptype: c_int, names: &[&str]) -> SEXP {
    // SAFETY: the arena lend only builds fresh, not-yet-published nodes and
    // never reenters R; the fresh vector's attribute slot is linked to the
    // completed names pairlist before the lend ends.
    unsafe {
        let len = names.len() as c_int;
        let names_symbol = crate::attrib_core::R_NamesSymbol();
        let ans = crate::sexp::memory::with_arena(|arena| {
            let ans = arena.alloc_vector(SEXPTYPE(sexptype), len as R_xlen_t);
            let nm = arena.alloc_vector(SEXPTYPE::STRSXP, len as R_xlen_t);
            if ans.is_null() || nm.is_null() {
                return ptr::null_mut();
            }
            for (i, name) in names.iter().enumerate() {
                // Preserve the former CString fallback for an interior NUL.
                let bytes = if name.as_bytes().contains(&0) {
                    &[]
                } else {
                    name.as_bytes()
                };
                let value = arena.alloc_charsxp(bytes);
                if value.is_null() {
                    return ptr::null_mut();
                }
                if arena.set_reference_element(nm, i, value).is_none() {
                    return ptr::null_mut();
                }
            }
            let attribute = arena.cons(nm, R_NilValue(), names_symbol);
            if attribute.is_null() {
                return ptr::null_mut();
            }
            // Fresh vectors have no existing attributes or old-generation
            // edges. Publish the complete owned graph before ending the lend.
            (*ans).attrib = arena
                .link_from_projection(attribute)
                .expect("fresh names attribute");
            ans
        });
        if ans.is_null() {
            error("could not allocate named regression result");
        }
        ans
    }
}

/// Resize a vector to a shorter length (simplified xlengthgets)
fn xlengthgets(x: SEXP, new_len: R_xlen_t) -> SEXP {
    let old_len = XLENGTH(x);
    if old_len <= new_len {
        return x;
    }
    let old_type = TYPEOF(x);
    let ans = Rf_allocVector(old_type, new_len as c_int);
    if old_type == SEXPTYPE::REALSXP {
        for i in 0..new_len as usize {
            SET_REAL_ELT(ans, i as i32, REAL_ELT(x, i as i32));
        }
    } else if old_type == SEXPTYPE::INTSXP {
        for i in 0..new_len as usize {
            SET_INTEGER_ELT(ans, i as i32, INTEGER_ELT(x, i as i32));
        }
    }
    // Copy attributes
    let nm_attr = crate::attrib_core::getAttrib(x, crate::attrib_core::R_NamesSymbol());
    if Rf_isNull(nm_attr) == 0 {
        crate::attrib_core::setAttrib(ans, crate::attrib_core::R_NamesSymbol(), nm_attr);
    }
    ans
}

// ---------------------------------------------------------------------------
// isoreg: isotonic regression
// ---------------------------------------------------------------------------

pub fn isoreg(y: SEXP) -> SEXP {
    let n = XLENGTH(y);

    let anms: [&str; 5] = ["y", "yc", "yf", "iKnots", ""];
    let ans = mkNamed(SEXPTYPE::VECSXP.into(), &anms);
    let _ans_guard = protect(ans);

    let yc = Rf_allocVector(SEXPTYPE::REALSXP, (n + 1) as c_int);
    let yf = Rf_allocVector(SEXPTYPE::REALSXP, n as c_int);
    let iKnots = Rf_allocVector(SEXPTYPE::INTSXP, n as c_int);

    SET_VECTOR_ELT(ans, 0, y);
    SET_VECTOR_ELT(ans, 1, yc);
    SET_VECTOR_ELT(ans, 2, yf);
    SET_VECTOR_ELT(ans, 3, iKnots);

    if n == 0 {
        return ans;
    }

    // yc := cumsum(0, y)
    SET_REAL_ELT(yc, 0, 0.0);
    let mut tmp: c_double = 0.0;
    for i in 0..n as usize {
        tmp += REAL_ELT(y, i as i32);
        SET_REAL_ELT(yc, (i + 1) as i32, tmp);
    }

    if !R_FINITE(REAL_ELT(yc, n as i32)) {
        error(&format!(
            "non-finite sum(y) == {} is not allowed",
            REAL_ELT(yc, n as i32)
        ));
        return R_NilValue();
    }

    let mut known: R_xlen_t = 0;
    let mut ip: R_xlen_t = 0;
    let mut n_ip: R_xlen_t = 0;

    loop {
        let mut slope: c_double = R_PosInf;
        let mut i: R_xlen_t = known + 1;
        while i <= n {
            tmp = (REAL_ELT(yc, i as i32) - REAL_ELT(yc, known as i32)) / (i - known) as c_double;
            if tmp < slope {
                slope = tmp;
                ip = i;
            }
            i += 1;
        }

        SET_INTEGER_ELT(iKnots, n_ip as i32, ip as c_int);
        n_ip += 1;

        let mut i: R_xlen_t = known;
        while i < ip {
            SET_REAL_ELT(
                yf,
                i as i32,
                (REAL_ELT(yc, ip as i32) - REAL_ELT(yc, known as i32)) / (ip - known) as c_double,
            );
            i += 1;
        }

        known = ip;
        if known >= n {
            break;
        }
    }

    if n_ip < n {
        let trimmed = xlengthgets(iKnots, n_ip);
        SET_VECTOR_ELT(ans, 3, trimmed);
    }

    ans
}

/// GNU `isoreg(y)` for a single numeric series.
pub fn do_isoreg(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    let y0 = CAR(args);
    let n = XLENGTH(y0);
    let yd = Rf_allocVector3(SEXPTYPE::REALSXP, n);
    let _yd = protect(yd);
    for i in 0..n {
        SET_REAL_ELT(
            yd,
            i as i32,
            if TYPEOF(y0) == SEXPTYPE::REALSXP {
                REAL_ELT(y0, i as i32)
            } else {
                INTEGER_ELT(y0, i as i32) as f64
            },
        );
    }
    let z = isoreg(yd);
    let _z = protect(z);
    crate::sexp::attrib_core::setAttrib(
        z,
        crate::sexp::attrib_core::R_ClassSymbol(),
        crate::library::stats::checked::mk_string("isoreg"),
    );
    z
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_results_keep_every_name_live_across_reentrant_collection() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            crate::sexp::gengc::register_gc_callback(Box::new(|_| {
                crate::sexp::gengc::full_gc();
            }));
            crate::mainutils::memory_main::R_gc_torture(1, 1, 0);
            let result = mkNamed(SEXPTYPE::VECSXP.0, &["x", "y", "yf", "iKnots"]);
            crate::mainutils::memory_main::R_gc_torture(0, 0, 0);
            let result_root = protect(result);
            let names = crate::attrib_core::getAttrib(result, crate::attrib_core::R_NamesSymbol());
            assert!(crate::sexp::memory::checked_projection(names).is_some());
            assert_eq!(XLENGTH(names), 4);
            for (index, expected) in ["x", "y", "yf", "iKnots"].into_iter().enumerate() {
                let name = STRING_ELT(names, index as R_xlen_t);
                assert!(crate::sexp::memory::checked_projection(name).is_some());
                assert_eq!(
                    &crate::sexp::accessors::char_cstr_bytes(name)[..],
                    expected.as_bytes()
                );
            }
            drop(result_root);
        });
    }
}
