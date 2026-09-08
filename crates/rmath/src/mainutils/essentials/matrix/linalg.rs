//! Matrix linear algebra: crossprod, tcrossprod, det, solve, which() for arrays — extracted verbatim from the former single-file module.
use super::*;

// ---------------------------------------------------------------------------
// Matrix/linear algebra
// ---------------------------------------------------------------------------

/// R's `crossprod(x, y)` — computes t(x) %*% y.
/// If y is NULL, computes t(x) %*% x.
pub unsafe fn do_crossprod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::array::do_matprod_kind(
            crate::mainutils::array::MatProductKind::Cross,
            args,
            "crossprod",
        )
    }
}

/// R's `tcrossprod(x, y)` — computes x %*% t(y).
/// If y is NULL, computes x %*% t(x).
pub unsafe fn do_tcrossprod(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::array::do_matprod_kind(
            crate::mainutils::array::MatProductKind::TransposedCross,
            args,
            "tcrossprod",
        )
    }
}

/// R's `det(x)` — determinant of a square matrix (simplified via LU-like approach).
/// For a 2x2 matrix: det = a*d - b*c. For larger, uses LU decomposition concept.
pub unsafe fn do_det(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        if x.is_null() || x == R_NilValue() {
            return Rf_ScalarReal(NA_REAL);
        }

        let dim_attr = crate::sexp::attrib_core::getAttrib(x, Rf_install(c"dim".as_ptr()));
        if dim_attr.is_null() || TYPEOF(dim_attr) != SEXPTYPE::INTSXP || LENGTH(dim_attr) != 2 {
            return Rf_ScalarReal(NA_REAL);
        }
        let n = *INTEGER(dim_attr) as usize;
        let m = *INTEGER(dim_attr).add(1) as usize;
        if n != m || n == 0 {
            return Rf_ScalarReal(NA_REAL);
        }

        if TYPEOF(x) != SEXPTYPE::REALSXP {
            return Rf_ScalarReal(NA_REAL);
        }

        // Compute determinant using LU decomposition (without pivoting for simplicity)
        let src = REAL(x);
        // Copy matrix data
        let mut mat: Vec<f64> = Vec::with_capacity(n * n);
        for i in 0..n * n {
            mat.push(*src.add(i));
        }

        let mut det_val = 1.0_f64;
        for i in 0..n {
            // Find pivot
            let mut max_val = mat[i * n + i].abs();
            let mut max_row = i;
            for k in (i + 1)..n {
                let v = mat[k * n + i].abs();
                if v > max_val {
                    max_val = v;
                    max_row = k;
                }
            }
            if max_val == 0.0 {
                return Rf_ScalarReal(0.0);
            }
            // Swap rows
            if max_row != i {
                for j in 0..n {
                    let tmp = mat[i * n + j];
                    mat[i * n + j] = mat[max_row * n + j];
                    mat[max_row * n + j] = tmp;
                }
                det_val = -det_val;
            }
            det_val *= mat[i * n + i];
            // Eliminate
            let pivot = mat[i * n + i];
            for k in (i + 1)..n {
                let factor = mat[k * n + i] / pivot;
                mat[k * n + i] = 0.0;
                for j in (i + 1)..n {
                    mat[k * n + j] -= factor * mat[i * n + j];
                }
            }
        }

        Rf_ScalarReal(det_val)
    }
}

/// Solve through the selected LAPACK adapter using R's column-major layout.
pub unsafe fn do_solve(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Exact/partial names bind before positional arguments; NULL is an
        // actual value, distinct from an omitted right-hand side.
        let mut matched = [None; 3];
        let formals = ["a", "b", "tol"];
        let mut positional = Vec::new();
        let mut cell = args;
        while !cell.is_null() && cell != R_NilValue() {
            if let Some(name) = crate::mainutils::essentials::tag_name(cell) {
                if let Some(index) = formals.iter().position(|formal| formal.starts_with(&name)) {
                    if matched[index].replace(CAR(cell)).is_some() {
                        base_error(format!(
                            "formal argument '{}' matched by multiple actual arguments",
                            formals[index]
                        ));
                    }
                }
            } else {
                positional.push(CAR(cell));
            }
            cell = CDR(cell);
        }
        for value in positional {
            if let Some(slot) = matched.iter_mut().find(|slot| slot.is_none()) {
                *slot = Some(value);
            }
        }
        let a = matched[0]
            .unwrap_or_else(|| base_error("argument 'a' is missing, with no default".to_owned()));
        let b = matched[1].unwrap_or_else(|| R_NilValue());
        let tol_arg = matched[2].unwrap_or_else(|| R_NilValue());
        let dim_sym = crate::sexp::attrib_core::R_DimSymbol();
        let dims = crate::sexp::attrib_core::getAttrib(a, dim_sym);
        if dims == R_NilValue() || XLENGTH(dims) != 2 {
            base_error("'a' must be a numeric matrix".to_owned());
        }
        let n = INTEGER_ELT(dims, 0);
        if n == 0 {
            base_error("'a' is 0-diml".to_owned());
        }
        if n != INTEGER_ELT(dims, 1) {
            base_error("'a' must be a square matrix".to_owned());
        }
        if !matches!(
            SEXPTYPE(TYPEOF(a)),
            SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP | SEXPTYPE::REALSXP | SEXPTYPE::CPLXSXP
        ) {
            base_error("'a' must be a numeric matrix".to_owned());
        }
        let inverse = matched[1].is_none() || b == crate::sexp::globals::R_MissingArg();
        let bdims = crate::sexp::attrib_core::getAttrib(b, dim_sym);
        let matrix_result = inverse || bdims != R_NilValue();
        let nrhs = if inverse {
            n
        } else if bdims != R_NilValue() {
            if XLENGTH(bdims) != 2 || INTEGER_ELT(bdims, 0) != n {
                base_error("'b' must have same row dimension as 'a'".to_owned());
            }
            INTEGER_ELT(bdims, 1)
        } else {
            if XLENGTH(b) != n as i64 {
                base_error("'b' must be compatible with 'a'".to_owned());
            }
            1
        };
        let complex = TYPEOF(a) == SEXPTYPE::CPLXSXP || TYPEOF(b) == SEXPTYPE::CPLXSXP;
        let ty = if complex {
            SEXPTYPE::CPLXSXP
        } else {
            SEXPTYPE::REALSXP
        };
        let aa = crate::mainutils::coerce::coerceVector(a, ty.as_c_int());
        let _aa = protect(aa);
        crate::sexp::attrib_core::setAttrib(aa, dim_sym, dims);
        let bb = if inverse {
            let v = Rf_allocVector3(ty, n as i64 * n as i64);
            for i in 0..n as usize {
                if complex {
                    (*COMPLEX(v).add(i + i * n as usize)).r = 1.0;
                } else {
                    *REAL(v).add(i + i * n as usize) = 1.0;
                }
            }
            v
        } else {
            if !matches!(
                SEXPTYPE(TYPEOF(b)),
                SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP | SEXPTYPE::REALSXP | SEXPTYPE::CPLXSXP
            ) {
                base_error("'b' must be numeric".to_owned());
            }
            let converted = crate::mainutils::coerce::coerceVector(b, ty.as_c_int());
            let _converted = protect(converted);
            crate::mainutils::duplicate::duplicate(converted)
        };
        let _bb = protect(bb);
        let out_dims = Rf_allocVector3(SEXPTYPE::INTSXP, 2);
        let _dims = protect(out_dims);
        *INTEGER(out_dims) = n;
        *INTEGER(out_dims).add(1) = nrhs;
        crate::sexp::attrib_core::setAttrib(bb, dim_sym, out_dims);
        let tol = if tol_arg == R_NilValue() {
            Rf_ScalarReal(f64::EPSILON)
        } else {
            tol_arg
        };
        let _tol = protect(tol);
        let result = if complex {
            crate::modules::lapack::lapack_impl::La_solve_cmplx(aa, bb, tol)
        } else {
            crate::modules::lapack::lapack_impl::La_solve(aa, bb, tol)
        };
        let _result = protect(result);
        if matrix_result {
            crate::sexp::attrib_core::setAttrib(result, dim_sym, out_dims);
        }
        // Row labels of the solution correspond to columns of A.
        let dn = crate::sexp::attrib_core::R_DimNamesSymbol();
        let adn = crate::sexp::attrib_core::getAttrib(a, dn);
        let bdn = crate::sexp::attrib_core::getAttrib(b, dn);
        let rows = if adn != R_NilValue() {
            VECTOR_ELT(adn, 1)
        } else {
            R_NilValue()
        };
        let cols = if inverse && adn != R_NilValue() {
            VECTOR_ELT(adn, 0)
        } else if bdn != R_NilValue() {
            VECTOR_ELT(bdn, 1)
        } else {
            R_NilValue()
        };
        if matrix_result && (rows != R_NilValue() || cols != R_NilValue()) {
            let names = Rf_allocVector3(SEXPTYPE::VECSXP, 2);
            let _names = protect(names);
            SET_VECTOR_ELT(names, 0, rows);
            SET_VECTOR_ELT(names, 1, cols);
            crate::sexp::attrib_core::setAttrib(result, dn, names);
        } else if !matrix_result && rows != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_NamesSymbol(),
                rows,
            );
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Matrix helpers
// ---------------------------------------------------------------------------

/// R's `which(x)` variant for arrays — returns 1-based row-major indices where x is TRUE.
pub unsafe fn do_which_array(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        // Same as do_which for now — array-aware which is equivalent for logical vectors
        do_which(_call, _op, args, _rho)
    }
}
