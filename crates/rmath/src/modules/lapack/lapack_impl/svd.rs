use super::*;

/// La_svd - real singular value decomposition.
///
/// Port of: static SEXP La_svd(SEXP jobu, SEXP x, SEXP s, SEXP u, SEXP vt)
pub unsafe fn La_svd(jobu: SEXP, x: SEXP, s: SEXP, u: SEXP, vt: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(jobu) != STRSXP_C || XLENGTH(jobu) < 1 {
            crate::sexp::context::r_error("'jobu' must be a character string");
        }
        if TYPEOF(x) != REALSXP_C {
            crate::sexp::context::r_error("'x' must be a numeric matrix");
        }
        if TYPEOF(s) != REALSXP_C {
            crate::sexp::context::r_error("'s' must be a numeric vector");
        }
        if TYPEOF(u) != REALSXP_C {
            crate::sexp::context::r_error("'u' must be a numeric matrix");
        }
        if TYPEOF(vt) != REALSXP_C {
            crate::sexp::context::r_error("'vt' must be a numeric matrix");
        }
        let _jobu_guard = protect(jobu);
        let _x_guard = protect(x);
        let _s_guard = protect(s);
        let _u_guard = protect(u);
        let _vt_guard = protect(vt);

        let dim = getAttrib(x, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'x' must be a matrix");
        }

        let n = INTEGER(dim).add(0).read();
        let p = INTEGER(dim).add(1).read();
        if n < 0 || p < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }

        let Some(len) = (n as usize).checked_mul(p as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(x) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let min_np = if n < p { n } else { p };
        if XLENGTH(s) as usize != min_np as usize {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let u_dims = getAttrib(u, R_DimSymbol());
        if u_dims.is_null() || TYPEOF(u_dims) != INTSXP_C || XLENGTH(u_dims) != 2 {
            crate::sexp::context::r_error("'u' must be a matrix");
        }
        let ldu = INTEGER(u_dims).add(0).read();
        let u_cols = INTEGER(u_dims).add(1).read();
        if ldu < 0 || u_cols < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_u) = (ldu as usize).checked_mul(u_cols as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_u > c_int::MAX as usize || XLENGTH(u) as usize != len_u {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let vt_dims = getAttrib(vt, R_DimSymbol());
        if vt_dims.is_null() || TYPEOF(vt_dims) != INTSXP_C || XLENGTH(vt_dims) != 2 {
            crate::sexp::context::r_error("'vt' must be a matrix");
        }
        let ldvt = INTEGER(vt_dims).add(0).read();
        let vt_cols = INTEGER(vt_dims).add(1).read();
        if ldvt < 0 || vt_cols < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_vt) = (ldvt as usize).checked_mul(vt_cols as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_vt > c_int::MAX as usize || XLENGTH(vt) as usize != len_vt {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let Some(scratch_bytes) = super::workspace_size::svd_scratch_bytes(len, min_np as usize)
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let iwork_len = (min_np as usize).saturating_mul(8);
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native SVD workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut x_copy = vec![0.0f64; len];
        if len != 0 {
            ptr::copy_nonoverlapping(REAL(x), x_copy.as_mut_ptr(), len);
        }
        let mut iwork = vec![0 as c_int; iwork_len];

        let ju = CHAR(STRING_ELT(jobu, 0)) as *const u8;
        let mut tmp: f64 = 0.0;
        let mut info: c_int = 0;
        let mut lwork: c_int = -1;

        super::backend::dgesdd_(
            ju,
            &n,
            &p,
            x_copy.as_mut_ptr(),
            &n,
            REAL(s),
            REAL(u),
            &ldu,
            REAL(vt),
            &ldvt,
            &mut tmp,
            &lwork,
            iwork.as_mut_ptr(),
            &mut info,
        );

        if info != 0 {
            crate::sexp::context::r_error("error code from Lapack routine 'dgesdd'");
        }

        if n > 0 && p > 0 {
            if !tmp.is_finite() || tmp < 1.0 || tmp > c_int::MAX as f64 {
                crate::sexp::context::r_error(
                    "invalid workspace size from Lapack routine 'dgesdd'",
                );
            }
            lwork = tmp as c_int;
            let work_bytes = (lwork as usize)
                .checked_mul(std::mem::size_of::<f64>())
                .unwrap_or_else(|| crate::sexp::context::r_error("invalid SVD workspace size"));
            let work_reservation = with_current_instance(|instance| {
                with_arena_in(instance, |arena| arena.try_reserve_transient(work_bytes))
            });
            if matches!(work_reservation, Some(None)) {
                crate::sexp::context::r_error(
                    "allocation failed: native SVD workspace exceeds resource limit",
                );
            }
            let _work_reservation = work_reservation.flatten();
            let mut work = vec![0.0f64; lwork as usize];

            super::backend::dgesdd_(
                ju,
                &n,
                &p,
                x_copy.as_mut_ptr(),
                &n,
                REAL(s),
                REAL(u),
                &ldu,
                REAL(vt),
                &ldvt,
                work.as_mut_ptr(),
                &lwork,
                iwork.as_mut_ptr(),
                &mut info,
            );

            if info != 0 {
                crate::sexp::context::r_error("error code from Lapack routine 'dgesdd'");
            }
        }

        // Build result list: list(d=s, u=u, vt=vt)
        let val = Rf_allocVector(VECSXP_C, 3);
        let _val_guard = protect(val);
        let nm = Rf_allocVector(STRSXP_C, 3);
        let _names_guard = protect(nm);
        SET_STRING_ELT(nm, 0, Rf_mkChar(b"d\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 1, Rf_mkChar(b"u\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 2, Rf_mkChar(b"vt\0".as_ptr() as *const c_char));
        setAttrib(val, R_NamesSymbol(), nm);
        SET_VECTOR_ELT(val, 0, s);
        SET_VECTOR_ELT(val, 1, u);
        SET_VECTOR_ELT(val, 2, vt);

        val
    }
}

/// La_svd_cmplx - complex singular value decomposition.
///
/// Port of: static SEXP La_svd_cmplx(SEXP jobu, SEXP x, SEXP s, SEXP u, SEXP v)
pub unsafe fn La_svd_cmplx(jobu: SEXP, x: SEXP, s: SEXP, u: SEXP, v: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(jobu) != STRSXP_C || XLENGTH(jobu) < 1 {
            crate::sexp::context::r_error("'jobu' must be a character string");
        }
        if TYPEOF(x) != CPLXSXP_C {
            crate::sexp::context::r_error("'x' must be a complex matrix");
        }
        if TYPEOF(s) != REALSXP_C {
            crate::sexp::context::r_error("'s' must be a numeric vector");
        }
        if TYPEOF(u) != CPLXSXP_C {
            crate::sexp::context::r_error("'u' must be a complex matrix");
        }
        if TYPEOF(v) != CPLXSXP_C {
            crate::sexp::context::r_error("'v' must be a complex matrix");
        }
        let _jobu_guard = protect(jobu);
        let _x_guard = protect(x);
        let _s_guard = protect(s);
        let _u_guard = protect(u);
        let _v_guard = protect(v);

        let dim = getAttrib(x, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'x' must be a matrix");
        }

        let n = INTEGER(dim).add(0).read();
        let p = INTEGER(dim).add(1).read();
        if n < 0 || p < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }

        let Some(len) = (n as usize).checked_mul(p as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(x) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let min_np = if n < p { n } else { p };
        if XLENGTH(s) as usize != min_np as usize {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let u_dims = getAttrib(u, R_DimSymbol());
        if u_dims.is_null() || TYPEOF(u_dims) != INTSXP_C || XLENGTH(u_dims) != 2 {
            crate::sexp::context::r_error("'u' must be a matrix");
        }
        let ldu = INTEGER(u_dims).add(0).read();
        let u_cols = INTEGER(u_dims).add(1).read();
        if ldu < 0 || u_cols < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_u) = (ldu as usize).checked_mul(u_cols as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_u > c_int::MAX as usize || XLENGTH(u) as usize != len_u {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let vt_dims = getAttrib(v, R_DimSymbol());
        if vt_dims.is_null() || TYPEOF(vt_dims) != INTSXP_C || XLENGTH(vt_dims) != 2 {
            crate::sexp::context::r_error("'v' must be a matrix");
        }
        let ldvt = INTEGER(vt_dims).add(0).read();
        let vt_cols = INTEGER(vt_dims).add(1).read();
        if ldvt < 0 || vt_cols < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_vt) = (ldvt as usize).checked_mul(vt_cols as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_vt > c_int::MAX as usize || XLENGTH(v) as usize != len_vt {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let ju = CHAR(STRING_ELT(jobu, 0)) as *const u8;

        let mn0 = min_np as usize;
        // Bound the backend's actual writes, accepting padded leading strides.
        // Overwrite mode stores one side in x_copy and ignores that output.
        let (required_u_cols, required_vt_rows) = match (*ju).to_ascii_uppercase() {
            b'A' => (Some(n), Some(p)),
            b'S' => (Some(min_np), Some(min_np)),
            b'O' if n >= p => (None, Some(p)),
            b'O' => (Some(n), None),
            b'N' => (None, None),
            _ => crate::sexp::context::r_error("error code from Lapack routine 'zgesdd'"),
        };
        if n > 0 && p > 0 {
            if required_u_cols.is_some_and(|columns| ldu < n || u_cols < columns) {
                crate::sexp::context::r_error(
                    "'u' is too small for the requested singular vectors",
                );
            }
            if required_vt_rows.is_some_and(|rows| ldvt < rows || vt_cols < p) {
                crate::sexp::context::r_error(
                    "'v' is too small for the requested singular vectors",
                );
            }
        }
        let mn1 = (if n > p { n } else { p }) as usize;
        let Some(scratch) = super::workspace_size::complex_svd_scratch(
            len,
            unsafe { *ju } == b'N',
            mn0,
            mn1,
            std::mem::size_of::<LapRcomplex>(),
        ) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch.bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native SVD workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut x_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len];
        if len != 0 {
            ptr::copy_nonoverlapping(COMPLEX(x) as *const LapRcomplex, x_copy.as_mut_ptr(), len);
        }
        let mut rwork = vec![0.0f64; scratch.rwork_len];
        let mut iwork = vec![0 as c_int; scratch.iwork_len];

        let mut tmp = LapRcomplex::default();
        let mut info: c_int = 0;
        let mut lwork: c_int = -1;

        super::backend::zgesdd_(
            ju,
            &n,
            &p,
            x_copy.as_mut_ptr(),
            &n,
            REAL(s),
            COMPLEX(u) as *mut LapRcomplex,
            &ldu,
            COMPLEX(v) as *mut LapRcomplex,
            &ldvt,
            &mut tmp,
            &lwork,
            rwork.as_mut_ptr(),
            iwork.as_mut_ptr(),
            &mut info,
        );

        if info != 0 {
            crate::sexp::context::r_error("error code from Lapack routine 'zgesdd'");
        }

        if n > 0 && p > 0 {
            if !tmp.r.is_finite() || tmp.r < 1.0 || tmp.r > c_int::MAX as f64 {
                crate::sexp::context::r_error(
                    "invalid workspace size from Lapack routine 'zgesdd'",
                );
            }
            lwork = tmp.r as c_int;
            let work_bytes = (lwork as usize)
                .checked_mul(std::mem::size_of::<LapRcomplex>())
                .unwrap_or_else(|| crate::sexp::context::r_error("invalid SVD workspace size"));
            let work_reservation = with_current_instance(|instance| {
                with_arena_in(instance, |arena| arena.try_reserve_transient(work_bytes))
            });
            if matches!(work_reservation, Some(None)) {
                crate::sexp::context::r_error(
                    "allocation failed: native SVD workspace exceeds resource limit",
                );
            }
            let _work_reservation = work_reservation.flatten();
            let mut work = vec![LapRcomplex::default(); lwork as usize];

            super::backend::zgesdd_(
                ju,
                &n,
                &p,
                x_copy.as_mut_ptr(),
                &n,
                REAL(s),
                COMPLEX(u) as *mut LapRcomplex,
                &ldu,
                COMPLEX(v) as *mut LapRcomplex,
                &ldvt,
                work.as_mut_ptr(),
                &lwork,
                rwork.as_mut_ptr(),
                iwork.as_mut_ptr(),
                &mut info,
            );

            if info != 0 {
                crate::sexp::context::r_error("error code from Lapack routine 'zgesdd'");
            }
        }

        let val = Rf_allocVector(VECSXP_C, 3);
        let _val_guard = protect(val);
        let nm = Rf_allocVector(STRSXP_C, 3);
        let _nm_guard = protect(nm);
        SET_STRING_ELT(nm, 0, Rf_mkChar(b"d\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 1, Rf_mkChar(b"u\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 2, Rf_mkChar(b"vt\0".as_ptr() as *const c_char));
        setAttrib(val, R_NamesSymbol(), nm);
        SET_VECTOR_ELT(val, 0, s);
        SET_VECTOR_ELT(val, 1, u);
        SET_VECTOR_ELT(val, 2, v);

        val
    }
}
