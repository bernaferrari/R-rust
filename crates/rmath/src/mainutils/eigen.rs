//! GNU `eigen()` wrapper over existing La_rs / La_rg kernels.

#![allow(non_snake_case)]

use std::os::raw::c_int;

use crate::attrib_core::{R_ClassSymbol, R_DimSymbol, getAttrib, setAttrib};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::protect::protect;

fn eigen_error(message: &str) -> ! {
    crate::mainutils::errors::errorcall_str(
        unsafe { crate::mainutils::errors::R_getCurrentCall() },
        message,
    )
}

unsafe fn is_symmetric_real(x: SEXP, n: usize) -> bool {
    unsafe {
        let p = REAL(x);
        let tol = 100.0 * f64::EPSILON;
        for j in 0..n {
            for i in 0..j {
                let a = *p.add(i + j * n);
                let b = *p.add(j + i * n);
                let scale = 1.0 + a.abs().max(b.abs());
                if (a - b).abs() > tol * scale {
                    return false;
                }
            }
        }
        true
    }
}

/// GNU `eigen(x, symmetric, only.values = FALSE)`.
pub unsafe fn do_eigen(_call: SEXP, _op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x0 = CAR(args);
        let mut only_values = FALSE;
        let mut symmetric_arg: Option<bool> = None;
        let mut cell = CDR(args);
        let mut positional = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let value = CAR(cell);
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                let pname = PRINTNAME(tag);
                if !pname.is_null() {
                    Some(
                        std::ffi::CStr::from_ptr(CHAR(pname))
                            .to_string_lossy()
                            .into_owned(),
                    )
                } else {
                    None
                }
            } else {
                None
            };
            let slot = match name.as_deref() {
                Some("symmetric") => 0,
                Some("only.values") => 1,
                _ => {
                    let s = positional;
                    positional += 1;
                    s
                }
            };
            match slot {
                0 => {
                    let flag = crate::main::coerce::asLogical(value);
                    if flag != crate::sexp::ffi::NA_LOGICAL {
                        symmetric_arg = Some(flag != 0);
                    }
                }
                1 => {
                    only_values = crate::main::coerce::asLogical(value);
                }
                _ => {}
            }
            cell = CDR(cell);
        }

        let dim0 = getAttrib(x0, R_DimSymbol());
        let x = if !dim0.is_null()
            && dim0 != R_NilValue()
            && TYPEOF(dim0) == SEXPTYPE::INTSXP
            && XLENGTH(dim0) == 2
        {
            x0
        } else {
            let mat_args = Rf_cons(x0, R_NilValue());
            let _mat_args = protect(mat_args);
            crate::mainutils::essentials::do_as_matrix(_call, _op, mat_args, rho)
        };
        let _x = protect(x);
        let dim = getAttrib(x, R_DimSymbol());
        if dim.is_null() || dim == R_NilValue() || TYPEOF(dim) != SEXPTYPE::INTSXP {
            eigen_error("non-square matrix in 'eigen'");
        }
        let nr = *INTEGER(dim);
        let nc = *INTEGER(dim).add(1);
        if nr != nc {
            eigen_error("non-square matrix in 'eigen'");
        }
        if nr == 0 {
            eigen_error("0 x 0 matrix");
        }
        let n = nr as usize;
        if TYPEOF(x) == SEXPTYPE::CPLXSXP {
            let p = COMPLEX(x);
            for i in 0..n * n {
                let z = *p.add(i);
                if !z.r.is_finite() || !z.i.is_finite() {
                    eigen_error("infinite or missing values in 'x'");
                }
            }
        } else {
            let xr = if TYPEOF(x) == SEXPTYPE::REALSXP {
                x
            } else {
                crate::main::coerce::coerceVector(x, SEXPTYPE::REALSXP.as_c_int())
            };
            let _xr = protect(xr);
            let p = REAL(xr);
            for i in 0..n * n {
                if !(*p.add(i)).is_finite() {
                    eigen_error("infinite or missing values in 'x'");
                }
            }
        }

        let only = Rf_ScalarLogical(only_values);
        let _only = protect(only);
        let complex = TYPEOF(x) == SEXPTYPE::CPLXSXP;
        let symmetric = match symmetric_arg {
            Some(flag) => flag,
            None => {
                if complex {
                    false
                } else {
                    let xr = if TYPEOF(x) == SEXPTYPE::REALSXP {
                        x
                    } else {
                        crate::main::coerce::coerceVector(x, SEXPTYPE::REALSXP.as_c_int())
                    };
                    is_symmetric_real(xr, n)
                }
            }
        };

        let z = if complex {
            if symmetric {
                crate::modules::lapack::lapack_impl::La_rs_cmplx(x, only)
            } else {
                crate::modules::lapack::lapack_impl::La_rg_cmplx(x, only)
            }
        } else {
            let xr = if TYPEOF(x) == SEXPTYPE::REALSXP {
                x
            } else {
                crate::main::coerce::coerceVector(x, SEXPTYPE::REALSXP.as_c_int())
            };
            let _xr = protect(xr);
            if symmetric {
                crate::modules::lapack::lapack_impl::La_rs(xr, only)
            } else {
                crate::modules::lapack::lapack_impl::La_rg(xr, only)
            }
        };
        let _z = protect(z);
        let values = VECTOR_ELT(z, 0);
        let vectors = if only_values != 0 || XLENGTH(z) < 2 {
            R_NilValue()
        } else {
            VECTOR_ELT(z, 1)
        };

        let mut order: Vec<usize> = (0..n).collect();
        if symmetric {
            order.reverse();
        } else if TYPEOF(values) == SEXPTYPE::REALSXP {
            let p = REAL(values);
            order.sort_by(|&i, &j| {
                let a = (*p.add(i)).abs();
                let b = (*p.add(j)).abs();
                b.partial_cmp(&a).unwrap_or(std::cmp::Ordering::Equal)
            });
        } else if TYPEOF(values) == SEXPTYPE::CPLXSXP {
            let p = COMPLEX(values);
            order.sort_by(|&i, &j| {
                let zi = *p.add(i);
                let zj = *p.add(j);
                let a = (zi.r * zi.r + zi.i * zi.i).sqrt();
                let b = (zj.r * zj.r + zj.i * zj.i).sqrt();
                b.partial_cmp(&a).unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        let out_values = Rf_allocVector3(TYPEOF(values), n as i64);
        let _ov = protect(out_values);
        if TYPEOF(values) == SEXPTYPE::REALSXP {
            for (dst, &src) in order.iter().enumerate() {
                *REAL(out_values).add(dst) = *REAL(values).add(src);
            }
        } else if TYPEOF(values) == SEXPTYPE::CPLXSXP {
            for (dst, &src) in order.iter().enumerate() {
                *COMPLEX(out_values).add(dst) = *COMPLEX(values).add(src);
            }
        }

        let out_vectors = if only_values != 0 || vectors == R_NilValue() {
            R_NilValue()
        } else {
            let nv = Rf_allocVector3(TYPEOF(vectors), (n * n) as i64);
            let _nv = protect(nv);
            let dimv = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
            *INTEGER(dimv) = n as c_int;
            *INTEGER(dimv).add(1) = n as c_int;
            setAttrib(nv, R_DimSymbol(), dimv);
            if TYPEOF(vectors) == SEXPTYPE::REALSXP {
                for (dst, &src) in order.iter().enumerate() {
                    for row in 0..n {
                        *REAL(nv).add(row + dst * n) = *REAL(vectors).add(row + src * n);
                    }
                }
            } else if TYPEOF(vectors) == SEXPTYPE::CPLXSXP {
                for (dst, &src) in order.iter().enumerate() {
                    for row in 0..n {
                        *COMPLEX(nv).add(row + dst * n) = *COMPLEX(vectors).add(row + src * n);
                    }
                }
            }
            nv
        };

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
        let _result = protect(result);
        SET_VECTOR_ELT(result, 0, out_values);
        SET_VECTOR_ELT(result, 1, out_vectors);
        let names = Rf_allocVector3(SEXPTYPE::STRSXP, 2);
        SET_STRING_ELT(names, 0, Rf_mkChar(c"values".as_ptr()));
        SET_STRING_ELT(names, 1, Rf_mkChar(c"vectors".as_ptr()));
        setAttrib(result, crate::attrib_core::R_NamesSymbol(), names);
        if only_values == 0 {
            let class = Rf_mkString(c"eigen".as_ptr());
            setAttrib(result, R_ClassSymbol(), class);
        }
        result
    }
}
