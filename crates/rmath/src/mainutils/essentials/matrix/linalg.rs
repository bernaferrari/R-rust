//! Matrix linear algebra: crossprod, tcrossprod, det, solve, which() for arrays — extracted verbatim from the former single-file module.
use super::*;

// ---------------------------------------------------------------------------
// Matrix/linear algebra
// ---------------------------------------------------------------------------

/// R's `crossprod(x, y)` — computes t(x) %*% y.
/// If y is NULL, computes t(x) %*% x.
pub unsafe fn do_crossprod(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::array::do_matprod(call, op, args, rho) }
}

/// R's `tcrossprod(x, y)` — computes x %*% t(y).
/// If y is NULL, computes x %*% t(x).
pub unsafe fn do_tcrossprod(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::array::do_matprod(call, op, args, rho) }
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

/// GNU `chol(x)` — upper Cholesky factor.
pub unsafe fn do_chol(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let pivot = Rf_ScalarLogical(FALSE);
        let _p = protect(pivot);
        let tol = Rf_ScalarReal(-1.0);
        let _t = protect(tol);
        let ans = crate::modules::lapack::lapack_impl::La_chol(x, pivot, tol);
        let _a = protect(ans);
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        if !dim.is_null() && dim != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(ans, crate::sexp::attrib_core::R_DimSymbol(), dim);
        }
        ans
    }
}

/// GNU `chol2inv(x)` — inverse from an upper Cholesky factor.
pub unsafe fn do_chol2inv(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        let n = if !dim.is_null()
            && dim != R_NilValue()
            && TYPEOF(dim) == SEXPTYPE::INTSXP
            && XLENGTH(dim) >= 1
        {
            *INTEGER(dim)
        } else {
            return R_NilValue();
        };
        let size = Rf_ScalarInteger(n);
        let _s = protect(size);
        let ans = crate::modules::lapack::lapack_impl::La_chol2inv(x, size);
        let _a = protect(ans);
        if !dim.is_null() && dim != R_NilValue() {
            crate::sexp::attrib_core::setAttrib(ans, crate::sexp::attrib_core::R_DimSymbol(), dim);
        }
        ans
    }
}

/// GNU `norm(x, type)` — `La_dlange`. Default type `"O"`.
pub unsafe fn do_norm(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let typ = CAR(CDR(args));
        let typ = if typ.is_null() || typ == R_NilValue() || TYPEOF(typ) != SEXPTYPE::STRSXP {
            let s = Rf_mkString(c"O".as_ptr());
            let _s = protect(s);
            s
        } else {
            typ
        };
        crate::modules::lapack::lapack_impl::La_dlange(x, typ)
    }
}

/// GNU `rcond(x, norm)` — `La_dgecon`. Default norm `"O"`.
pub unsafe fn do_rcond(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let typ = CAR(CDR(args));
        let typ = if typ.is_null() || typ == R_NilValue() || TYPEOF(typ) != SEXPTYPE::STRSXP {
            let s = Rf_mkString(c"O".as_ptr());
            let _s = protect(s);
            s
        } else {
            typ
        };
        crate::modules::lapack::lapack_impl::La_dgecon(x, typ)
    }
}

/// GNU `kappa` — `1/rcond`, or `smax/smin` when `exact=TRUE`.
pub unsafe fn do_kappa(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let mut exact = false;
        let mut cell = CDR(args);
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let name = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let v = CAR(cell);
            if name == "exact"
                || (name.is_empty() && TYPEOF(v) == SEXPTYPE::LGLSXP)
            {
                if TYPEOF(v) == SEXPTYPE::LGLSXP && XLENGTH(v) > 0 {
                    exact = *LOGICAL(v) != 0;
                }
            }
            cell = CDR(cell);
        }
        if exact {
            let sv = do_svd(call, op, Rf_cons(x, R_NilValue()), rho);
            let _sv = protect(sv);
            let d = crate::sexp::attrib_core::getAttrib(
                sv,
                crate::sexp::attrib_core::R_NamesSymbol(),
            );
            let mut dvec = R_NilValue();
            if TYPEOF(sv) == SEXPTYPE::VECSXP && TYPEOF(d) == SEXPTYPE::STRSXP {
                for i in 0..XLENGTH(d) {
                    let raw = CHAR(STRING_ELT(d, i));
                    if !raw.is_null()
                        && std::ffi::CStr::from_ptr(raw).to_bytes() == b"d"
                    {
                        dvec = VECTOR_ELT(sv, i);
                        break;
                    }
                }
            }
            if TYPEOF(dvec) == SEXPTYPE::REALSXP && XLENGTH(dvec) > 0 {
                let first = *REAL(dvec);
                let last = *REAL(dvec).add((XLENGTH(dvec) as usize) - 1);
                return Rf_ScalarReal(if last == 0.0 {
                    f64::INFINITY
                } else {
                    first / last
                });
            }
        }
        let rc = do_rcond(call, op, Rf_cons(x, R_NilValue()), rho);
        if rc.is_null() || rc == R_NilValue() || TYPEOF(rc) != SEXPTYPE::REALSXP {
            return R_NilValue();
        }
        let v = *REAL(rc);
        Rf_ScalarReal(if v == 0.0 { f64::INFINITY } else { 1.0 / v })
    }
}

/// GNU `forwardsolve(l, x)` — `backsolve(..., upper.tri=FALSE)`.
pub unsafe fn do_forwardsolve(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let l = CAR(args);
        let x = CAR(CDR(args));
        let ut = Rf_ScalarLogical(FALSE);
        let _u = protect(ut);
        let tail = Rf_cons(ut, R_NilValue());
        let _t = protect(tail);
        crate::sexp::accessors::SETTAG(tail, crate::sexp::symbol::Rf_install(c"upper.tri".as_ptr()));
        let packed = Rf_cons(l, Rf_cons(x, tail));
        let _p = protect(packed);
        crate::mainutils::array::do_backsolve(call, op, packed, rho)
    }
}

/// GNU `svd(x)` — singular values via `La_svd`.
pub unsafe fn do_svd(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let dim = crate::sexp::attrib_core::getAttrib(x, crate::sexp::attrib_core::R_DimSymbol());
        if dim.is_null()
            || dim == R_NilValue()
            || TYPEOF(dim) != SEXPTYPE::INTSXP
            || XLENGTH(dim) < 2
        {
            return R_NilValue();
        }
        let n = *INTEGER(dim) as i32;
        let p = *INTEGER(dim).add(1) as i32;
        if n <= 0 || p <= 0 {
            return R_NilValue();
        }
        let min_np = if n < p { n } else { p };
        let mut nu = min_np;
        let mut nv = min_np;
        let mut cell = CDR(args);
        let mut pos = 0;
        while !cell.is_null() && cell != R_NilValue() {
            let tag = TAG(cell);
            let named = if !tag.is_null() && tag != R_NilValue() {
                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(tag)))
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            let val = CAR(cell);
            let iv = if TYPEOF(val) == SEXPTYPE::INTSXP && XLENGTH(val) > 0 {
                *INTEGER(val)
            } else if TYPEOF(val) == SEXPTYPE::REALSXP && XLENGTH(val) > 0 {
                *REAL(val) as i32
            } else {
                -1
            };
            if named == "nu" || (named.is_empty() && pos == 0) {
                if iv >= 0 {
                    nu = iv.min(n);
                }
            } else if named == "nv" || (named.is_empty() && pos == 1) {
                if iv >= 0 {
                    nv = iv.min(p);
                }
            }
            if named.is_empty() {
                pos += 1;
            }
            cell = CDR(cell);
        }

        let jobu = Rf_mkString(c"A".as_ptr());
        let _j = protect(jobu);
        let s = Rf_allocVector3(SEXPTYPE::REALSXP, min_np as i64);
        let _s = protect(s);
        let u = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), n, n);
        let _u = protect(u);
        let vt = crate::mainutils::array::allocMatrix(SEXPTYPE::REALSXP.as_c_int(), p, p);
        let _vt = protect(vt);
        for i in 0..(n as usize * n as usize) {
            *REAL(u).add(i) = 0.0;
        }
        for i in 0..(p as usize * p as usize) {
            *REAL(vt).add(i) = 0.0;
        }
        let la = crate::modules::lapack::lapack_impl::La_svd(jobu, x, s, u, vt);
        let _la = protect(la);
        let v = crate::mainutils::array::do_transpose(call, op, Rf_cons(vt, R_NilValue()), rho);
        let _v = protect(v);
        // GNU svd() defaults nu=nv=min(n,p).
        let u_out = if nu != n {
            let thin = crate::mainutils::array::allocMatrix(
                SEXPTYPE::REALSXP.as_c_int(),
                n,
                nu,
            );
            let _th = protect(thin);
            let cols = (nu.min(n)) as usize;
            for j in 0..cols {
                for i in 0..n as usize {
                    *REAL(thin).add(j * n as usize + i) = *REAL(u).add(j * n as usize + i);
                }
            }
            thin
        } else {
            u
        };
        let v_out = if nv != p {
            let thin = crate::mainutils::array::allocMatrix(
                SEXPTYPE::REALSXP.as_c_int(),
                p,
                nv,
            );
            let _th = protect(thin);
            let cols = (nv.min(p)) as usize;
            for j in 0..cols {
                for i in 0..p as usize {
                    *REAL(thin).add(j * p as usize + i) = *REAL(v).add(j * p as usize + i);
                }
            }
            thin
        } else {
            v
        };

        let result = Rf_allocVector3(SEXPTYPE::VECSXP, 3);
        let _r = protect(result);
        SET_VECTOR_ELT(result, 0, s);
        SET_VECTOR_ELT(result, 1, u_out);
        SET_VECTOR_ELT(result, 2, v_out);
        crate::mainutils::essentials::set_string_names(
            result,
            &["d".to_string(), "u".to_string(), "v".to_string()],
        );
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
