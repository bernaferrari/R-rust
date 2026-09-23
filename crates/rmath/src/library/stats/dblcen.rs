//! Double Centering for Classical Multidimensional Scaling.
//! Port of r-source/src/library/stats/src/dblcen.c

use std::os::raw::c_double;
use std::slice;

use crate::main::util_main::nrows;
use crate::sexp::accessors::REAL;
use crate::sexp::ffi::SEXP;

fn double_centre_square(a: &mut [c_double], n: usize) {
    let n_f = n as c_double;

    for row in 0..n {
        let mut sum = 0.0;
        for col in 0..n {
            sum += a[row + col * n];
        }
        let mean = sum / n_f;
        for col in 0..n {
            a[row + col * n] -= mean;
        }
    }

    for col in 0..n {
        let column = &mut a[col * n..(col + 1) * n];
        let mean = column.iter().sum::<c_double>() / n_f;
        for value in column {
            *value -= mean;
        }
    }
}

/// DoubleCentre - double centering for classical MDS.
///
/// Takes a matrix SEXP, modifies in-place:
/// 1. Compute row means, subtract from each row
/// 2. Compute column means, subtract from each column
/// Returns the SEXP.
///
/// NB: this does not duplicate A.
///
/// # Safety
/// A must be a valid REALSXP matrix pointer.
pub unsafe fn DoubleCentre(A: SEXP) -> SEXP {
    let n = unsafe { nrows(A as *const std::ffi::c_void) };
    if n <= 0 {
        return A;
    }
    let n_s = n as usize;
    let len = n_s * n_s;
    let a = unsafe { slice::from_raw_parts_mut(REAL(A), len) };
    double_centre_square(a, n_s);

    A
}
pub unsafe extern "C-unwind" fn c_double_centre(a: SEXP) -> SEXP {
    unsafe { DoubleCentre(a) }
}

/// GNU `cmdscale(d, k)` classical MDS.
pub unsafe fn do_cmdscale(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        use crate::sexp::accessors::{
            CAR, CDR, INTEGER, REAL, TAG, TYPEOF, VECTOR_ELT, XLENGTH,
        };
        use crate::sexp::constructors::{Rf_ScalarInteger, Rf_allocVector3, Rf_cons};
        use crate::sexp::ffi::SEXPTYPE;
        use crate::sexp::globals::R_NilValue;
        use crate::sexp::protect::protect;
        use crate::sexp::symbol::Rf_install;
        let d = CAR(args);
        let mut k = 2;
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(
                    crate::sexp::accessors::PRINTNAME(tag),
                ))
                .to_string_lossy()
                .into_owned()
            } else {
                String::new()
            };
            if name == "k" {
                let v = CAR(cell);
                k = if TYPEOF(v) == SEXPTYPE::INTSXP {
                    *INTEGER(v)
                } else {
                    *REAL(v) as i32
                };
            }
            cell = CDR(cell);
        }
        let size = crate::sexp::attrib_core::getAttrib(d, Rf_install(c"Size".as_ptr()));
        let n = if !size.is_null() && size != R_NilValue() && TYPEOF(size) == SEXPTYPE::INTSXP {
            *INTEGER(size) as usize
        } else {
            0
        };
        if n < 2 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "distances must be result of 'dist' or a square matrix",
            );
        }
        if k < 1 || k as usize > n - 1 {
            crate::mainutils::errors::errorcall_str(
                crate::mainutils::errors::R_getCurrentCall(),
                "'k' must be in {1, 2, ..  n - 1}",
            );
        }
        let a = Rf_allocVector3(SEXPTYPE::REALSXP, (n * n) as i64);
        let _a = protect(a);
        let dim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        *INTEGER(dim) = n as i32;
        *INTEGER(dim).add(1) = n as i32;
        crate::sexp::attrib_core::setAttrib(a, crate::sexp::attrib_core::R_DimSymbol(), dim);
        for i in 0..n * n {
            *REAL(a).add(i) = 0.0;
        }
        let mut t = 0usize;
        for i in 0..n {
            for j in (i + 1)..n {
                let v = *REAL(d).add(t);
                let s = v * v;
                *REAL(a).add(i + j * n) = s;
                *REAL(a).add(j + i * n) = s;
                t += 1;
            }
        }
        DoubleCentre(a);
        for i in 0..n * n {
            *REAL(a).add(i) *= -0.5;
        }
        let only = Rf_cons(a, R_NilValue());
        let _only = protect(only);
        let ev = crate::mainutils::eigen::do_eigen(_call, _op, only, rho);
        let _ev = protect(ev);
        let values = VECTOR_ELT(ev, 0);
        let vectors = VECTOR_ELT(ev, 1);
        let points = Rf_allocVector3(SEXPTYPE::REALSXP, (n * k as usize) as i64);
        let _p = protect(points);
        let pdim = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        *INTEGER(pdim) = n as i32;
        *INTEGER(pdim).add(1) = k;
        crate::sexp::attrib_core::setAttrib(points, crate::sexp::attrib_core::R_DimSymbol(), pdim);
        for j in 0..k as usize {
            let lam = if TYPEOF(values) == SEXPTYPE::REALSXP {
                *REAL(values).add(j)
            } else {
                0.0
            };
            let scale = if lam > 0.0 { lam.sqrt() } else { 0.0 };
            let mut sign = 1.0;
            if n > 0 {
                let v0 = if TYPEOF(vectors) == SEXPTYPE::REALSXP {
                    *REAL(vectors).add(j * n)
                } else {
                    0.0
                };
                if v0 < 0.0 {
                    sign = -1.0;
                }
            }
            for i in 0..n {
                let v = if TYPEOF(vectors) == SEXPTYPE::REALSXP {
                    *REAL(vectors).add(i + j * n)
                } else {
                    0.0
                };
                *REAL(points).add(i + j * n) = sign * scale * v;
            }
        }
        points
    }
}

