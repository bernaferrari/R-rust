use super::*;

/// La_chol - real Cholesky decomposition.
///
/// Port of: static SEXP La_chol(SEXP a, SEXP pivot, SEXP stol)
pub unsafe fn La_chol(a: SEXP, pivot: SEXP, stol: SEXP) -> SEXP {
    unsafe {
        let piv = asLogical(pivot);
        if piv == NA_INTEGER {
            Rf_error(b"invalid 'pivot' argument\0".as_ptr() as *const c_char);
        }

        let tol = asReal(stol);

        if TYPEOF(a) != REALSXP_C {
            crate::sexp::context::r_error("'a' must be a numeric matrix");
        }
        let _input_guard = protect(a);
        let dim = getAttrib(a, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'a' must be a matrix");
        }

        let n = INTEGER(dim).add(0).read();
        let n2 = INTEGER(dim).add(1).read();
        if n < 0 || n2 < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        if n != n2 {
            crate::sexp::context::r_error("'a' must be a square matrix");
        }

        let Some(len) = (n as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(a) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let work_len = if piv != 0 {
            (n as usize)
                .checked_mul(2)
                .unwrap_or_else(|| crate::sexp::context::r_error("matrix dimensions are too large"))
        } else {
            0
        };
        let Some(scratch_bytes) = len
            .checked_mul(std::mem::size_of::<f64>())
            .and_then(|bytes| bytes.checked_add(work_len.checked_mul(std::mem::size_of::<f64>())?))
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native Cholesky workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut a_copy = vec![0.0f64; len];
        if len != 0 {
            ptr::copy_nonoverlapping(REAL(a), a_copy.as_mut_ptr(), len);
        }

        let mut info: c_int = 0;
        let uplo = b'U';

        if piv != 0 {
            // pivoted Cholesky: dpstrf
            let piv_arr = R_alloc(n as usize, std::mem::size_of::<c_int>()) as *mut c_int;
            for i in 0..n as usize {
                *piv_arr.add(i) = i as c_int + 1;
            }
            let mut rank: c_int = 0;
            let mut work = vec![0.0f64; work_len];
            let uplo_s = c"U";

            super::backend::dpstrf_(
                uplo_s.as_ptr() as *const u8,
                &n,
                a_copy.as_mut_ptr(),
                &n,
                piv_arr,
                &mut rank,
                &tol,
                work.as_mut_ptr(),
                &mut info,
            );

            if info != 0 {
                let msg = std::ffi::CString::new(
                    "the matrix is either rank-deficient or not positive definite",
                )
                .unwrap();
                crate::mainutils::errors::warningcall(R_NilValue(), msg.as_ptr());
                if rank <= 0 {
                    rank = 0;
                }
            }

            let nn = n as usize;
            for j in rank as usize..nn {
                for i in rank as usize..=j {
                    a_copy[i + nn * j] = 0.0;
                }
            }
            for j in 0..nn {
                for i in (j + 1)..nn {
                    a_copy[i + j * nn] = 0.0;
                }
            }
            let ans = Rf_allocVector(REALSXP_C, len as c_int);
            let _ans = protect(ans);
            if len != 0 {
                ptr::copy_nonoverlapping(a_copy.as_ptr(), REAL(ans), len);
            }
            let dims = Rf_allocVector(INTSXP_C, 2);
            let _d = protect(dims);
            *INTEGER(dims) = n;
            *INTEGER(dims).add(1) = n;
            setAttrib(ans, R_DimSymbol(), dims);
            let pivot_s = Rf_allocVector(INTSXP_C, n as c_int);
            let _ps = protect(pivot_s);
            for i in 0..nn {
                *INTEGER(pivot_s).add(i) = *piv_arr.add(i);
            }
            setAttrib(
                ans,
                crate::sexp::symbol::Rf_install(c"pivot".as_ptr()),
                pivot_s,
            );
            setAttrib(
                ans,
                crate::sexp::symbol::Rf_install(c"rank".as_ptr()),
                Rf_ScalarInteger(rank),
            );
            ans
        } else {
            super::backend::dpotrf_(&uplo, &n, a_copy.as_mut_ptr(), &n, &mut info);
            if info > 0 {
                crate::sexp::context::r_error(format!(
                    "the leading minor of order {info} is not positive"
                ));
            }
            if info != 0 {
                crate::sexp::context::r_error("error code from Lapack routine 'dpotrf'");
            }
            for j in 0..n as usize {
                for i in (j + 1)..n as usize {
                    a_copy[i + j * n as usize] = 0.0;
                }
            }
            let ans = Rf_allocVector(REALSXP_C, len as c_int);
            if len != 0 {
                ptr::copy_nonoverlapping(a_copy.as_ptr(), REAL(ans), len);
            }
            ans
        }
    }
}

/// La_chol2inv - real inverse from Cholesky factor.
///
/// Port of: static SEXP La_chol2inv(SEXP a, SEXP size, SEXP diag_only)
pub unsafe fn La_chol2inv(a: SEXP, size: SEXP, diag_only: SEXP) -> SEXP {
    unsafe {
        let only_diag = if diag_only.is_null() || diag_only == R_NilValue() {
            0
        } else {
            asLogical(diag_only)
        };
        if only_diag == NA_INTEGER {
            crate::sexp::context::r_error("invalid 'diag.only' argument");
        }
        let n = asInteger(size);
        if n == NA_INTEGER || n <= 0 {
            Rf_error(b"'size' must be a positive integer\0".as_ptr() as *const c_char);
        }

        if TYPEOF(a) != REALSXP_C {
            crate::sexp::context::r_error("'a' must be a numeric matrix");
        }
        let _input_guard = protect(a);

        let dim = getAttrib(a, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'a' must be a matrix");
        }

        let m = INTEGER(dim).add(0).read();
        let p = INTEGER(dim).add(1).read();
        if m < 0 || p < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        // GNU: size may be rank < ncol; use the leading size×size of R.
        if n > p {
            crate::sexp::context::r_error(format!("'size' cannot exceed ncol(x) = {p}"));
        }
        if n > m {
            crate::sexp::context::r_error(format!("'size' cannot exceed nrow(x) = {m}"));
        }

        let Some(src_len) = (m as usize).checked_mul(p as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if src_len > c_int::MAX as usize || XLENGTH(a) as usize != src_len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let Some(len) = (n as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let Some(scratch_bytes) = len.checked_mul(std::mem::size_of::<f64>()) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native Cholesky inverse workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut a_copy = vec![0.0f64; len];
        let lda = m as usize;
        let sz = n as usize;
        for j in 0..sz {
            for i in 0..=j {
                a_copy[i + j * sz] = *REAL(a).add(i + j * lda);
            }
        }

        let mut info: c_int = 0;
        let uplo = b'U';

        super::backend::dpotri_(&uplo, &n, a_copy.as_mut_ptr(), &n, &mut info);

        if info != 0 {
            Rf_error(b"error code from Lapack routine 'dpotri'\0".as_ptr() as *const c_char);
        }

        // diag.only is the diagonal of inv(R'R). This backend's dpotri
        // writes that diagonal as the squared row norms of R^{-1}.
        if only_diag != 0 {
            let diag = Rf_allocVector(REALSXP_C, n);
            for i in 0..sz {
                *REAL(diag).add(i) = a_copy[i + i * sz];
            }
            return diag;
        }

        // Copy upper triangle to lower (dpotri U writes i<=j only).
        for j in 1..sz {
            for i in 0..j {
                a_copy[j + i * sz] = a_copy[i + j * sz];
            }
        }

        let ans = crate::mainutils::array::allocMatrix(REALSXP_C, n, n);
        if len != 0 {
            ptr::copy_nonoverlapping(a_copy.as_ptr(), REAL(ans), len);
        }
        ans
    }
}
