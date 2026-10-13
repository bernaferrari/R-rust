use super::*;

/// La_solve - real linear solve.
///
/// Port of: static SEXP La_solve(SEXP a, SEXP bin, SEXP tolin)
pub unsafe fn La_solve(a: SEXP, bin: SEXP, tolin: SEXP) -> SEXP {
    unsafe {
        let tol = asReal(tolin);

        if TYPEOF(a) != REALSXP_C {
            crate::sexp::context::r_error("'a' must be a numeric matrix");
        }
        if TYPEOF(bin) != REALSXP_C {
            crate::sexp::context::r_error("'b' must be a numeric matrix");
        }
        let _a_guard = protect(a);
        let _b_guard = protect(bin);

        let a_dim = getAttrib(a, R_DimSymbol());
        if a_dim.is_null() || TYPEOF(a_dim) != INTSXP_C || XLENGTH(a_dim) != 2 {
            crate::sexp::context::r_error("'a' must be a matrix");
        }

        let n = INTEGER(a_dim).add(0).read();
        let n2 = INTEGER(a_dim).add(1).read();
        if n < 0 || n2 < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        if n != n2 {
            crate::sexp::context::r_error("'a' must be a square matrix");
        }

        let b_dim = getAttrib(bin, R_DimSymbol());
        if b_dim.is_null() || TYPEOF(b_dim) != INTSXP_C || XLENGTH(b_dim) != 2 {
            crate::sexp::context::r_error("'b' must be a matrix");
        }

        let m_b = INTEGER(b_dim).add(0).read();
        let nrhs = INTEGER(b_dim).add(1).read();
        if m_b < 0 || nrhs < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        if m_b != n {
            crate::sexp::context::r_error("'b' must have same row dimension as 'a'");
        }

        let Some(len_a) = (n as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let Some(len_b) = (n as usize).checked_mul(nrhs as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_a > c_int::MAX as usize || XLENGTH(a) as usize != len_a {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }
        if len_b > c_int::MAX as usize || XLENGTH(bin) as usize != len_b {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let work_len = if tol > 0.0 {
            (n as usize)
                .checked_mul(4)
                .unwrap_or_else(|| crate::sexp::context::r_error("matrix dimensions are too large"))
        } else {
            0
        };
        let iwork_len = if tol > 0.0 { n as usize } else { 0 };
        let Some(scratch_bytes) = len_a
            .checked_mul(std::mem::size_of::<f64>())
            .and_then(|bytes| bytes.checked_add(len_b.checked_mul(std::mem::size_of::<f64>())?))
            .and_then(|bytes| bytes.checked_add(work_len.checked_mul(std::mem::size_of::<f64>())?))
            .and_then(|bytes| {
                bytes.checked_add(iwork_len.checked_mul(std::mem::size_of::<c_int>())?)
            })
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native solve workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut a_copy = vec![0.0f64; len_a];
        if len_a != 0 {
            ptr::copy_nonoverlapping(REAL(a), a_copy.as_mut_ptr(), len_a);
        }

        let mut b_copy = vec![0.0f64; len_b];
        if len_b != 0 {
            ptr::copy_nonoverlapping(REAL(bin), b_copy.as_mut_ptr(), len_b);
        }

        let anorm = {
            let n0 = (n as usize).max(1);
            let mut max_col = 0.0;
            let mut saw_nan = false;
            for col in a_copy.chunks(n0) {
                let mut sum = 0.0;
                for v in col {
                    if v.is_nan() {
                        saw_nan = true;
                    }
                    sum += v.abs();
                }
                if sum > max_col {
                    max_col = sum;
                }
            }
            if saw_nan { f64::NAN } else { max_col }
        };
        let ipiv = R_alloc(n as usize, std::mem::size_of::<c_int>()) as *mut c_int;
        let mut info: c_int = 0;

        super::backend::dgesv_(
            &n,
            &nrhs,
            a_copy.as_mut_ptr(),
            &n,
            ipiv,
            b_copy.as_mut_ptr(),
            &n,
            &mut info,
        );

        if info > 0 {
            Rf_error(b"singular matrix in 'solve'\0".as_ptr() as *const c_char);
        }
        if info != 0 {
            Rf_error(b"error code from Lapack routine 'dgesv'\0".as_ptr() as *const c_char);
        }

        if tol > 0.0 {
            let mut rcond = 0.0;
            let mut work = vec![0.0; work_len];
            let mut iwork = vec![0; iwork_len];
            super::backend::dgecon_(
                b"1".as_ptr(),
                &n,
                a_copy.as_ptr(),
                &n,
                &anorm,
                &mut rcond,
                work.as_mut_ptr(),
                iwork.as_mut_ptr(),
                &mut info,
            );
            // GNU: NaN rcond is not < tol (IEEE). All-NaN A must return NaN, not error.
            if rcond.is_finite() && rcond < tol {
                crate::sexp::context::r_error("system is computationally singular");
            }
        }
        let ans = Rf_allocVector(REALSXP_C, len_b as c_int);
        let _ans_guard = protect(ans);
        if len_b != 0 {
            ptr::copy_nonoverlapping(b_copy.as_ptr(), REAL(ans), len_b);
        }
        ans
    }
}

/// La_solve_cmplx - complex linear solve.
///
/// Port of: static SEXP La_solve_cmplx(SEXP a, SEXP bin, SEXP tolin)
pub unsafe fn La_solve_cmplx(a: SEXP, bin: SEXP, tolin: SEXP) -> SEXP {
    unsafe {
        let tol = asReal(tolin);

        if TYPEOF(a) != CPLXSXP_C {
            crate::sexp::context::r_error("'a' must be a complex matrix");
        }
        if TYPEOF(bin) != CPLXSXP_C {
            crate::sexp::context::r_error("'b' must be a complex matrix");
        }
        let _a_guard = protect(a);
        let _b_guard = protect(bin);

        let a_dim = getAttrib(a, R_DimSymbol());
        if a_dim.is_null() || TYPEOF(a_dim) != INTSXP_C || XLENGTH(a_dim) != 2 {
            crate::sexp::context::r_error("'a' must be a matrix");
        }

        let n = INTEGER(a_dim).add(0).read();
        let n2 = INTEGER(a_dim).add(1).read();
        if n < 0 || n2 < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        if n != n2 {
            crate::sexp::context::r_error("'a' must be a square matrix");
        }

        let b_dim = getAttrib(bin, R_DimSymbol());
        if b_dim.is_null() || TYPEOF(b_dim) != INTSXP_C || XLENGTH(b_dim) != 2 {
            crate::sexp::context::r_error("'b' must be a matrix");
        }

        let m_b = INTEGER(b_dim).add(0).read();
        let nrhs = INTEGER(b_dim).add(1).read();
        if m_b < 0 || nrhs < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        if m_b != n {
            crate::sexp::context::r_error("'b' must have same row dimension as 'a'");
        }

        let Some(len_a) = (n as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let Some(len_b) = (n as usize).checked_mul(nrhs as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_a > c_int::MAX as usize || XLENGTH(a) as usize != len_a {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }
        if len_b > c_int::MAX as usize || XLENGTH(bin) as usize != len_b {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let work_len = if tol > 0.0 {
            (n as usize)
                .checked_mul(2)
                .unwrap_or_else(|| crate::sexp::context::r_error("matrix dimensions are too large"))
        } else {
            0
        };
        let rwork_len = work_len;
        let Some(scratch_bytes) = len_a
            .checked_mul(std::mem::size_of::<LapRcomplex>())
            .and_then(|bytes| {
                bytes.checked_add(len_b.checked_mul(std::mem::size_of::<LapRcomplex>())?)
            })
            .and_then(|bytes| {
                bytes.checked_add(work_len.checked_mul(std::mem::size_of::<LapRcomplex>())?)
            })
            .and_then(|bytes| {
                bytes.checked_add(rwork_len.checked_mul(std::mem::size_of::<f64>())?)
            })
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native solve workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut a_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len_a];
        if len_a != 0 {
            ptr::copy_nonoverlapping(COMPLEX(a) as *const LapRcomplex, a_copy.as_mut_ptr(), len_a);
        }

        let mut b_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len_b];
        if len_b != 0 {
            ptr::copy_nonoverlapping(
                COMPLEX(bin) as *const LapRcomplex,
                b_copy.as_mut_ptr(),
                len_b,
            );
        }

        let anorm = {
            let n0 = (n as usize).max(1);
            let mut max_col = 0.0;
            let mut saw_nan = false;
            for col in a_copy.chunks(n0) {
                let mut sum = 0.0;
                for v in col {
                    if v.r.is_nan() || v.i.is_nan() {
                        saw_nan = true;
                    }
                    sum += v.r.hypot(v.i);
                }
                if sum > max_col {
                    max_col = sum;
                }
            }
            if saw_nan { f64::NAN } else { max_col }
        };
        let ipiv = R_alloc(n as usize, std::mem::size_of::<c_int>()) as *mut c_int;
        let mut info: c_int = 0;

        super::backend::zgesv_(
            &n,
            &nrhs,
            a_copy.as_mut_ptr(),
            &n,
            ipiv,
            b_copy.as_mut_ptr(),
            &n,
            &mut info,
        );

        if info > 0 {
            Rf_error(b"singular matrix in 'solve'\0".as_ptr() as *const c_char);
        }
        if info != 0 {
            Rf_error(b"error code from Lapack routine 'zgesv'\0".as_ptr() as *const c_char);
        }

        if tol > 0.0 {
            let mut rcond = 0.0;
            let mut work = vec![LapRcomplex::default(); work_len];
            let mut rwork = vec![0.0; rwork_len];
            super::backend::zgecon_(
                b"1".as_ptr(),
                &n,
                a_copy.as_ptr(),
                &n,
                &anorm,
                &mut rcond,
                work.as_mut_ptr(),
                rwork.as_mut_ptr(),
                &mut info,
            );
            if rcond.is_finite() && rcond < tol {
                crate::sexp::context::r_error("system is computationally singular");
            }
        }
        let ans = Rf_allocVector(CPLXSXP_C, len_b as c_int);
        let _ans_guard = protect(ans);
        if len_b != 0 {
            ptr::copy_nonoverlapping(b_copy.as_ptr(), COMPLEX(ans) as *mut LapRcomplex, len_b);
        }
        ans
    }
}
