use super::*;

/// La_dlange - real matrix norm.
///
/// Port of: static SEXP La_dlange(SEXP a, SEXP type_)
pub unsafe fn La_dlange(a: SEXP, type_: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(type_) != 16 {
            Rf_error(b"'type' must be a character string\0".as_ptr() as *const c_char);
        }
        if TYPEOF(a) != REALSXP_C {
            crate::sexp::context::r_error("'a' must be a numeric matrix");
        }
        let _input_guard = protect(a);
        let _type_guard = protect(type_);

        let typ_str = CStr::from_ptr(CHAR(STRING_ELT(type_, 0)))
            .to_str()
            .unwrap_or("O");
        let norm_c = La_norm_type(typ_str);

        let dim = getAttrib(a, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'a' must be a matrix");
        }

        let m = INTEGER(dim).add(0).read();
        let n = INTEGER(dim).add(1).read();
        if m < 0 || n < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len) = (m as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(a) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let work_len = if norm_c == b'I' || norm_c == b'O' {
            m as usize
        } else {
            0
        };
        let Some(scratch_bytes) = work_len.checked_mul(std::mem::size_of::<f64>()) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native matrix-norm workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();
        let mut work = vec![0.0f64; work_len];

        let anorm = super::backend::dlange_(&norm_c, &m, &n, REAL(a), &m, work.as_mut_ptr());

        let ans = Rf_allocVector(REALSXP_C, 1);
        *REAL(ans) = anorm;
        ans
    }
}

/// La_dgecon - real matrix condition number estimate.
///
/// Port of: static SEXP La_dgecon(SEXP a, SEXP norm)
pub unsafe fn La_dgecon(a: SEXP, norm: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(norm) != 16 || XLENGTH(norm) < 1 {
            Rf_error(b"'norm' must be a character string\0".as_ptr() as *const c_char);
        }
        if TYPEOF(a) != REALSXP_C {
            crate::sexp::context::r_error("'a' must be a numeric matrix");
        }
        let _input_guard = protect(a);
        let _norm_guard = protect(norm);

        let norm_str = CStr::from_ptr(CHAR(STRING_ELT(norm, 0)))
            .to_str()
            .unwrap_or("O");
        let norm_c = La_rcond_type(norm_str);

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
        if n == 0 {
            crate::sexp::context::r_error("error code -4 from Lapack routine 'dgetrf()'");
        }

        let work_norm_len = if norm_c == b'I' { n as usize } else { 0 };
        let Some(work_len) = (n as usize).checked_mul(4) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let Some(scratch_bytes) = len
            .checked_mul(std::mem::size_of::<f64>())
            .and_then(|bytes| {
                bytes.checked_add(work_norm_len.checked_mul(std::mem::size_of::<f64>())?)
            })
            .and_then(|bytes| bytes.checked_add(work_len.checked_mul(std::mem::size_of::<f64>())?))
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native condition-number workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        // Compute the norm of A
        let mut work_norm = vec![0.0f64; work_norm_len];
        let anorm = super::backend::dlange_(&norm_c, &n, &n, REAL(a), &n, work_norm.as_mut_ptr());

        if anorm == 0.0 {
            let ans = Rf_allocVector(REALSXP_C, 1);
            *REAL(ans) = 0.0;
            return ans;
        }

        // Work on a copy
        let mut a_copy = vec![0.0f64; len];
        if len != 0 {
            ptr::copy_nonoverlapping(REAL(a), a_copy.as_mut_ptr(), len);
        }

        let ipiv = R_alloc(n as usize, std::mem::size_of::<c_int>()) as *mut c_int;
        let mut info: c_int = 0;

        // LU factorization
        super::backend::dgetrf_(&n, &n, a_copy.as_mut_ptr(), &n, ipiv, &mut info);
        if info > 0 {
            let ans = Rf_allocVector(REALSXP_C, 1);
            *REAL(ans) = 0.0;
            return ans;
        }
        if info != 0 {
            Rf_error(b"error code from Lapack routine 'dgetrf'\0".as_ptr() as *const c_char);
        }

        let mut rcond: f64 = 0.0;
        let mut work = vec![0.0f64; work_len];
        let iwork = R_alloc(n as usize, std::mem::size_of::<c_int>()) as *mut c_int;

        super::backend::dgecon_(
            &norm_c,
            &n,
            a_copy.as_ptr(),
            &n,
            &anorm,
            &mut rcond,
            work.as_mut_ptr(),
            iwork,
            &mut info,
        );

        if info != 0 {
            Rf_error(b"error code from Lapack routine 'dgecon'\0".as_ptr() as *const c_char);
        }

        let ans = Rf_allocVector(REALSXP_C, 1);
        *REAL(ans) = rcond;
        ans
    }
}

/// La_dtrcon - real triangular condition number.
///
/// Port of: static SEXP La_dtrcon(SEXP a, SEXP norm)
pub unsafe fn La_dtrcon(a: SEXP, norm: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(norm) != 16 {
            Rf_error(b"'norm' must be a character string\0".as_ptr() as *const c_char);
        }
        if TYPEOF(a) != REALSXP_C {
            crate::sexp::context::r_error("'a' must be a numeric matrix");
        }
        let _input_guard = protect(a);
        let _norm_guard = protect(norm);

        let norm_str = CStr::from_ptr(CHAR(STRING_ELT(norm, 0)))
            .to_str()
            .unwrap_or("O");
        let norm_c = La_rcond_type(norm_str);

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
        let Some(work_len) = (n as usize).checked_mul(3) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let Some(scratch_bytes) = work_len.checked_mul(std::mem::size_of::<f64>()) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native triangular condition-number workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut rcond: f64 = 0.0;
        let mut work = vec![0.0f64; work_len];
        let iwork = R_alloc(n as usize, std::mem::size_of::<c_int>()) as *mut c_int;
        let mut info: c_int = 0;

        let uplo = b'U'; // Default upper
        let diag = b'N'; // Non-unit triangular

        super::backend::dtrcon_(
            &norm_c,
            &uplo,
            &diag,
            &n,
            REAL(a),
            &n,
            &mut rcond,
            work.as_mut_ptr(),
            iwork,
            &mut info,
        );

        if info != 0 {
            Rf_error(b"error code from Lapack routine 'dtrcon'\0".as_ptr() as *const c_char);
        }

        let ans = Rf_allocVector(REALSXP_C, 1);
        *REAL(ans) = rcond;
        ans
    }
}

/// La_dtrcon3 - real triangular condition number with explicit uplo.
///
/// Port of: static SEXP La_dtrcon3(SEXP a, SEXP norm, SEXP uplo)
pub unsafe fn La_dtrcon3(a: SEXP, norm: SEXP, uplo: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(norm) != 16 {
            Rf_error(b"'norm' must be a character string\0".as_ptr() as *const c_char);
        }
        if TYPEOF(uplo) != 16 {
            Rf_error(b"'uplo' must be a character string\0".as_ptr() as *const c_char);
        }
        if TYPEOF(a) != REALSXP_C {
            crate::sexp::context::r_error("'a' must be a numeric matrix");
        }
        let _input_guard = protect(a);
        let _norm_guard = protect(norm);
        let _uplo_guard = protect(uplo);

        let norm_str = CStr::from_ptr(CHAR(STRING_ELT(norm, 0)))
            .to_str()
            .unwrap_or("O");
        let norm_c = La_rcond_type(norm_str);
        let uplo_str = CStr::from_ptr(CHAR(STRING_ELT(uplo, 0)))
            .to_str()
            .unwrap_or("U");
        let uplo_c = La_valid_uplo(uplo_str);

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
        let Some(work_len) = (n as usize).checked_mul(3) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let Some(scratch_bytes) = work_len.checked_mul(std::mem::size_of::<f64>()) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native triangular condition-number workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut rcond: f64 = 0.0;
        let mut work = vec![0.0f64; work_len];
        let iwork = R_alloc(n as usize, std::mem::size_of::<c_int>()) as *mut c_int;
        let diag = b'N';
        let mut info: c_int = 0;

        super::backend::dtrcon_(
            &norm_c,
            &uplo_c,
            &diag,
            &n,
            REAL(a),
            &n,
            &mut rcond,
            work.as_mut_ptr(),
            iwork,
            &mut info,
        );

        if info != 0 {
            Rf_error(b"error code from Lapack routine 'dtrcon'\0".as_ptr() as *const c_char);
        }

        let ans = Rf_allocVector(REALSXP_C, 1);
        *REAL(ans) = rcond;
        ans
    }
}

/// La_zlange - complex matrix norm.
///
/// Port of: static SEXP La_zlange(SEXP a, SEXP type_)
pub unsafe fn La_zlange(a: SEXP, type_: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(type_) != 16 {
            Rf_error(b"'type' must be a character string\0".as_ptr() as *const c_char);
        }
        if TYPEOF(a) != CPLXSXP_C {
            crate::sexp::context::r_error("'a' must be a complex matrix");
        }
        let _input_guard = protect(a);
        let _type_guard = protect(type_);

        let typ_str = CStr::from_ptr(CHAR(STRING_ELT(type_, 0)))
            .to_str()
            .unwrap_or("O");
        let norm_c = La_norm_type(typ_str);

        let dim = getAttrib(a, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'a' must be a matrix");
        }

        let m = INTEGER(dim).add(0).read();
        let n = INTEGER(dim).add(1).read();
        if m < 0 || n < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len) = (m as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(a) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let work_len = if norm_c == b'I' || norm_c == b'O' {
            m as usize
        } else {
            0
        };
        let Some(scratch_bytes) = work_len.checked_mul(std::mem::size_of::<f64>()) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native complex matrix-norm workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();
        let mut work = vec![0.0f64; work_len];

        let a_ptr = COMPLEX(a) as *const LapRcomplex;
        let anorm = super::backend::zlange_(&norm_c, &m, &n, a_ptr, &m, work.as_mut_ptr());

        let ans = Rf_allocVector(REALSXP_C, 1);
        *REAL(ans) = anorm;
        ans
    }
}

/// La_zgecon - complex matrix condition number estimate.
///
/// Port of: static SEXP La_zgecon(SEXP a, SEXP norm)
pub unsafe fn La_zgecon(a: SEXP, norm: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(norm) != 16 || XLENGTH(norm) < 1 {
            Rf_error(b"'norm' must be a character string\0".as_ptr() as *const c_char);
        }
        if TYPEOF(a) != CPLXSXP_C {
            crate::sexp::context::r_error("'a' must be a complex matrix");
        }
        let _input_guard = protect(a);
        let _norm_guard = protect(norm);

        let norm_str = CStr::from_ptr(CHAR(STRING_ELT(norm, 0)))
            .to_str()
            .unwrap_or("O");
        let norm_c = La_rcond_type(norm_str);

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
        if n == 0 {
            crate::sexp::context::r_error("error code -4 from Lapack routine 'zgetrf()'");
        }

        let work_norm_len = if norm_c == b'I' { n as usize } else { 0 };
        let Some(work_len) = (n as usize).checked_mul(2) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let Some(scratch_bytes) = len
            .checked_mul(std::mem::size_of::<LapRcomplex>())
            .and_then(|bytes| {
                bytes.checked_add(work_norm_len.checked_mul(std::mem::size_of::<f64>())?)
            })
            .and_then(|bytes| {
                bytes.checked_add(work_len.checked_mul(std::mem::size_of::<LapRcomplex>())?)
            })
            .and_then(|bytes| bytes.checked_add(work_len.checked_mul(std::mem::size_of::<f64>())?))
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native complex condition-number workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut work_norm = vec![0.0f64; work_norm_len];
        let anorm = super::backend::zlange_(
            &norm_c,
            &n,
            &n,
            COMPLEX(a) as *const LapRcomplex,
            &n,
            work_norm.as_mut_ptr(),
        );

        if anorm == 0.0 {
            let ans = Rf_allocVector(REALSXP_C, 1);
            *REAL(ans) = 0.0;
            return ans;
        }

        // Work on a copy
        let mut a_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len];
        if len != 0 {
            ptr::copy_nonoverlapping(COMPLEX(a) as *const LapRcomplex, a_copy.as_mut_ptr(), len);
        }

        let ipiv = R_alloc(n as usize, std::mem::size_of::<c_int>()) as *mut c_int;
        let mut info: c_int = 0;

        super::backend::zgetrf_(&n, &n, a_copy.as_mut_ptr(), &n, ipiv, &mut info);
        if info > 0 {
            let ans = Rf_allocVector(REALSXP_C, 1);
            *REAL(ans) = 0.0;
            return ans;
        }
        if info != 0 {
            Rf_error(b"error code from Lapack routine 'zgetrf'\0".as_ptr() as *const c_char);
        }

        let mut rcond: f64 = 0.0;
        let mut work = vec![LapRcomplex::default(); work_len];
        let mut rwork = vec![0.0f64; work_len];

        super::backend::zgecon_(
            &norm_c,
            &n,
            a_copy.as_ptr(),
            &n,
            &anorm,
            &mut rcond,
            work.as_mut_ptr(),
            rwork.as_mut_ptr(),
            &mut info,
        );

        if info != 0 {
            Rf_error(b"error code from Lapack routine 'zgecon'\0".as_ptr() as *const c_char);
        }

        let ans = Rf_allocVector(REALSXP_C, 1);
        *REAL(ans) = rcond;
        ans
    }
}

/// La_ztrcon - complex triangular condition number.
///
/// Port of: static SEXP La_ztrcon(SEXP a, SEXP norm)
pub unsafe fn La_ztrcon(a: SEXP, norm: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(norm) != 16 {
            Rf_error(b"'norm' must be a character string\0".as_ptr() as *const c_char);
        }
        if TYPEOF(a) != CPLXSXP_C {
            crate::sexp::context::r_error("'a' must be a complex matrix");
        }
        let _input_guard = protect(a);
        let _norm_guard = protect(norm);

        let norm_str = CStr::from_ptr(CHAR(STRING_ELT(norm, 0)))
            .to_str()
            .unwrap_or("O");
        let norm_c = La_rcond_type(norm_str);

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
        let Some(work_len) = (n as usize).checked_mul(2) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let rwork_len = n as usize;
        let Some(scratch_bytes) = work_len
            .checked_mul(std::mem::size_of::<LapRcomplex>())
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
                "allocation failed: native complex triangular condition-number workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut rcond: f64 = 0.0;
        let mut work = vec![LapRcomplex::default(); work_len];
        let mut rwork = vec![0.0f64; rwork_len];
        let uplo = b'U';
        let diag = b'N';
        let mut info: c_int = 0;

        super::backend::ztrcon_(
            &norm_c,
            &uplo,
            &diag,
            &n,
            COMPLEX(a) as *const LapRcomplex,
            &n,
            &mut rcond,
            work.as_mut_ptr(),
            rwork.as_mut_ptr(),
            &mut info,
        );

        if info != 0 {
            Rf_error(b"error code from Lapack routine 'ztrcon'\0".as_ptr() as *const c_char);
        }

        let ans = Rf_allocVector(REALSXP_C, 1);
        *REAL(ans) = rcond;
        ans
    }
}

/// La_ztrcon3 - complex triangular condition number with explicit uplo.
///
/// Port of: static SEXP La_ztrcon3(SEXP a, SEXP norm, SEXP uplo)
pub unsafe fn La_ztrcon3(a: SEXP, norm: SEXP, uplo: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(norm) != 16 {
            Rf_error(b"'norm' must be a character string\0".as_ptr() as *const c_char);
        }
        if TYPEOF(uplo) != 16 {
            Rf_error(b"'uplo' must be a character string\0".as_ptr() as *const c_char);
        }
        if TYPEOF(a) != CPLXSXP_C {
            crate::sexp::context::r_error("'a' must be a complex matrix");
        }
        let _input_guard = protect(a);
        let _norm_guard = protect(norm);
        let _uplo_guard = protect(uplo);

        let norm_str = CStr::from_ptr(CHAR(STRING_ELT(norm, 0)))
            .to_str()
            .unwrap_or("O");
        let norm_c = La_rcond_type(norm_str);
        let uplo_str = CStr::from_ptr(CHAR(STRING_ELT(uplo, 0)))
            .to_str()
            .unwrap_or("U");
        let uplo_c = La_valid_uplo(uplo_str);

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
        let Some(work_len) = (n as usize).checked_mul(2) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let rwork_len = n as usize;
        let Some(scratch_bytes) = work_len
            .checked_mul(std::mem::size_of::<LapRcomplex>())
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
                "allocation failed: native complex triangular condition-number workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut rcond: f64 = 0.0;
        let mut work = vec![LapRcomplex::default(); work_len];
        let mut rwork = vec![0.0f64; rwork_len];
        let diag = b'N';
        let mut info: c_int = 0;

        super::backend::ztrcon_(
            &norm_c,
            &uplo_c,
            &diag,
            &n,
            COMPLEX(a) as *const LapRcomplex,
            &n,
            &mut rcond,
            work.as_mut_ptr(),
            rwork.as_mut_ptr(),
            &mut info,
        );

        if info != 0 {
            Rf_error(b"error code from Lapack routine 'ztrcon'\0".as_ptr() as *const c_char);
        }

        let ans = Rf_allocVector(REALSXP_C, 1);
        *REAL(ans) = rcond;
        ans
    }
}
