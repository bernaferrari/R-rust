use super::*;

/// La_qr - real QR decomposition.
///
/// Port of: static SEXP La_qr(SEXP ain)
pub unsafe fn La_qr(ain: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(ain) != REALSXP_C {
            crate::sexp::context::r_error("'a' must be a numeric matrix");
        }
        let _input_guard = protect(ain);
        let dim = getAttrib(ain, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'a' must be a matrix");
        }

        let m = INTEGER(dim).add(0).read();
        let n = INTEGER(dim).add(1).read();
        if m < 0 || n < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let min_mn = if m < n { m } else { n };

        let Some(len) = (m as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(ain) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }
        let lda = m.max(1);
        let Some(scratch_bytes) = super::workspace_size::qr_scratch_bytes(m as usize, n as usize)
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native QR workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut a_copy = vec![0.0f64; len];
        if len != 0 {
            ptr::copy_nonoverlapping(REAL(ain), a_copy.as_mut_ptr(), len);
        }
        let mut jpvt = vec![0 as c_int; n as usize];
        let mut tau = vec![0.0f64; min_mn as usize];

        let mut tmp: f64 = 0.0;
        let mut lwork: c_int = -1;
        let mut info: c_int = 0;
        super::backend::dgeqp3_(
            &m,
            &n,
            a_copy.as_mut_ptr(),
            &lda,
            jpvt.as_mut_ptr(),
            tau.as_mut_ptr(),
            &mut tmp,
            &lwork,
            &mut info,
        );
        if info != 0 {
            crate::sexp::context::r_error("error code from Lapack routine 'dgeqp3'");
        }
        if !tmp.is_finite() || tmp < 1.0 || tmp > c_int::MAX as f64 {
            crate::sexp::context::r_error("invalid workspace size from Lapack routine 'dgeqp3'");
        }
        lwork = tmp as c_int;
        let work_bytes = (lwork as usize)
            .checked_mul(std::mem::size_of::<f64>())
            .unwrap_or_else(|| crate::sexp::context::r_error("invalid QR workspace size"));
        let work_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(work_bytes))
        });
        if matches!(work_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native QR workspace exceeds resource limit",
            );
        }
        let _work_reservation = work_reservation.flatten();
        let mut work = vec![0.0f64; lwork as usize];

        super::backend::dgeqp3_(
            &m,
            &n,
            a_copy.as_mut_ptr(),
            &lda,
            jpvt.as_mut_ptr(),
            tau.as_mut_ptr(),
            work.as_mut_ptr(),
            &lwork,
            &mut info,
        );
        if info != 0 {
            crate::sexp::context::r_error("error code from Lapack routine 'dgeqp3'");
        }

        let qr = Rf_allocVector(REALSXP_C, len as c_int);
        let _qr_guard = protect(qr);
        if len != 0 {
            ptr::copy_nonoverlapping(a_copy.as_ptr(), REAL(qr), len);
        }

        let qraux = Rf_allocVector(REALSXP_C, min_mn as c_int);
        let _qraux_guard = protect(qraux);
        for i in 0..min_mn as usize {
            *REAL(qraux).add(i) = tau[i];
        }

        let pivot = Rf_allocVector(INTSXP_C, n as c_int);
        let _pivot_guard = protect(pivot);
        for i in 0..n as usize {
            *INTEGER(pivot).add(i) = jpvt[i];
        }

        let ret = Rf_allocVector(VECSXP_C, 4);
        let _ret_guard = protect(ret);
        let nm = Rf_allocVector(STRSXP_C, 4);
        let _nm_guard = protect(nm);
        SET_STRING_ELT(nm, 0, Rf_mkChar(b"qr\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 1, Rf_mkChar(b"rank\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 2, Rf_mkChar(b"qraux\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 3, Rf_mkChar(b"pivot\0".as_ptr() as *const c_char));
        SET_VECTOR_ELT(ret, 0, qr);
        SET_VECTOR_ELT(ret, 1, Rf_ScalarInteger(min_mn));
        SET_VECTOR_ELT(ret, 2, qraux);
        SET_VECTOR_ELT(ret, 3, pivot);
        setAttrib(ret, R_NamesSymbol(), nm);

        ret
    }
}

/// La_qr_cmplx - complex QR decomposition.
///
/// Port of: static SEXP La_qr_cmplx(SEXP ain)
pub unsafe fn La_qr_cmplx(ain: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(ain) != CPLXSXP_C {
            crate::sexp::context::r_error("'a' must be a complex matrix");
        }
        let _input_guard = protect(ain);
        let dim = getAttrib(ain, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'a' must be a matrix");
        }

        let m = INTEGER(dim).add(0).read();
        let n = INTEGER(dim).add(1).read();
        if m < 0 || n < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let min_mn = if m < n { m } else { n };

        let Some(len) = (m as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(ain) as usize != len {
            crate::sexp::context::r_error("invalid complex matrix dimensions or length");
        }
        let Some(rwork_len) = (n as usize).checked_mul(2) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let lda = m.max(1);
        let Some(scratch_bytes) = len
            .checked_mul(std::mem::size_of::<LapRcomplex>())
            .and_then(|bytes| bytes.checked_add(rwork_len.checked_mul(std::mem::size_of::<f64>())?))
            .and_then(|bytes| {
                bytes.checked_add((n as usize).checked_mul(std::mem::size_of::<c_int>())?)
            })
            .and_then(|bytes| {
                bytes
                    .checked_add((min_mn as usize).checked_mul(std::mem::size_of::<LapRcomplex>())?)
            })
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native QR workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut a_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len];
        if len != 0 {
            ptr::copy_nonoverlapping(COMPLEX(ain) as *const LapRcomplex, a_copy.as_mut_ptr(), len);
        }

        let mut jpvt = vec![0 as c_int; n as usize];
        let mut tau = vec![LapRcomplex::default(); min_mn as usize];

        // Query optimal work size
        let mut tmp = LapRcomplex::default();
        let mut lwork: c_int = -1;
        let mut rwork = vec![0.0f64; rwork_len];
        let mut info: c_int = 0;

        super::backend::zgeqp3_(
            &m,
            &n,
            a_copy.as_mut_ptr(),
            &lda,
            jpvt.as_mut_ptr(),
            tau.as_mut_ptr(),
            &mut tmp,
            &lwork,
            rwork.as_mut_ptr(),
            &mut info,
        );

        if info != 0 {
            crate::sexp::context::r_error(format!(
                "error code {info} from Lapack routine 'zgeqp3'"
            ));
        }

        if !tmp.r.is_finite() || tmp.r < 1.0 || tmp.r > c_int::MAX as f64 {
            crate::sexp::context::r_error("invalid workspace size from Lapack routine 'zgeqp3'");
        }
        lwork = tmp.r as c_int;
        let work_bytes = (lwork as usize)
            .checked_mul(std::mem::size_of::<LapRcomplex>())
            .unwrap_or_else(|| crate::sexp::context::r_error("invalid QR workspace size"));
        let work_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(work_bytes))
        });
        if matches!(work_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native QR workspace exceeds resource limit",
            );
        }
        let _work_reservation = work_reservation.flatten();
        let mut work = vec![LapRcomplex::default(); lwork as usize];

        super::backend::zgeqp3_(
            &m,
            &n,
            a_copy.as_mut_ptr(),
            &lda,
            jpvt.as_mut_ptr(),
            tau.as_mut_ptr(),
            work.as_mut_ptr(),
            &lwork,
            rwork.as_mut_ptr(),
            &mut info,
        );

        if info != 0 {
            crate::sexp::context::r_error(format!(
                "error code {info} from Lapack routine 'zgeqp3'"
            ));
        }

        let qr = Rf_allocVector(CPLXSXP_C, len as c_int);
        let _qr_guard = protect(qr);
        if len != 0 {
            ptr::copy_nonoverlapping(a_copy.as_ptr(), COMPLEX(qr) as *mut LapRcomplex, len);
        }

        let qr_dim = Rf_allocVector(INTSXP_C, 2);
        let _qr_dim_guard = protect(qr_dim);
        *INTEGER(qr_dim) = m;
        *INTEGER(qr_dim).add(1) = n;
        setAttrib(qr, R_DimSymbol(), qr_dim);

        let qraux = Rf_allocVector(CPLXSXP_C, min_mn as c_int);
        let _qraux_guard = protect(qraux);
        for i in 0..min_mn as usize {
            *COMPLEX(qraux).add(i) = {
                // SAFETY: LapRcomplex and Rcomplex have identical layouts: #[repr(C)] struct { r: f64, i: f64 }
                std::mem::transmute::<LapRcomplex, Rcomplex>(tau[i])
            };
        }

        let pivot = Rf_allocVector(INTSXP_C, n as c_int);
        let _pivot_guard = protect(pivot);
        for i in 0..n as usize {
            *INTEGER(pivot).add(i) = jpvt[i];
        }

        // GNU pivots the input's column dimnames onto the factored matrix.
        let adn = getAttrib(ain, R_DimNamesSymbol());
        if adn != R_NilValue() && TYPEOF(adn) == VECSXP_C && XLENGTH(adn) == 2 {
            let adn2 = crate::mainutils::duplicate::Rf_duplicate(adn);
            let _adn2_guard = protect(adn2);
            let cn = VECTOR_ELT(adn, 1);
            let cn2 = VECTOR_ELT(adn2, 1);
            if cn != R_NilValue()
                && TYPEOF(cn) == STRSXP_C
                && TYPEOF(cn2) == STRSXP_C
                && (XLENGTH(cn) as usize) == n as usize
                && (XLENGTH(cn2) as usize) == n as usize
            {
                for j in 0..n as usize {
                    let source = *INTEGER(pivot).add(j);
                    if source >= 1 && (source as usize) <= n as usize {
                        SET_STRING_ELT(cn2, j as i64, STRING_ELT(cn, (source - 1) as i64));
                    }
                }
            }
            setAttrib(qr, R_DimNamesSymbol(), adn2);
        }
        let ret = Rf_allocVector(VECSXP_C, 4);
        let _ret_guard = protect(ret);
        let nm = Rf_allocVector(STRSXP_C, 4);
        let _nm_guard = protect(nm);
        SET_STRING_ELT(nm, 0, Rf_mkChar(b"qr\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 1, Rf_mkChar(b"rank\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 2, Rf_mkChar(b"qraux\0".as_ptr() as *const c_char));
        SET_STRING_ELT(nm, 3, Rf_mkChar(b"pivot\0".as_ptr() as *const c_char));
        SET_VECTOR_ELT(ret, 0, qr);
        SET_VECTOR_ELT(ret, 1, Rf_ScalarInteger(min_mn));
        SET_VECTOR_ELT(ret, 2, qraux);
        SET_VECTOR_ELT(ret, 3, pivot);
        setAttrib(ret, R_NamesSymbol(), nm);

        ret
    }
}

/// qr_coef_real - real QR coefficients.
///
/// Port of: static SEXP qr_coef_real(SEXP q, SEXP bin)
pub unsafe fn qr_coef_real(q: SEXP, bin: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(q) != VECSXP_C || XLENGTH(q) < 1 {
            crate::sexp::context::r_error("'qr' must be a QR decomposition");
        }
        if TYPEOF(bin) != REALSXP_C {
            crate::sexp::context::r_error("'y' must be a numeric matrix");
        }
        let _q_guard = protect(q);
        let _b_guard = protect(bin);
        let qr = VECTOR_ELT(q, 0);
        if TYPEOF(qr) != REALSXP_C {
            crate::sexp::context::r_error("'qr$qr' must be a numeric matrix");
        }

        let dim = getAttrib(qr, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'qr$qr' must be a matrix");
        }
        let m = INTEGER(dim).add(0).read();
        let n = INTEGER(dim).add(1).read();
        if m < 0 || n < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_r) = (m as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_r > c_int::MAX as usize || XLENGTH(qr) as usize != len_r {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let b_dim = getAttrib(bin, R_DimSymbol());
        if b_dim.is_null() || TYPEOF(b_dim) != INTSXP_C || XLENGTH(b_dim) != 2 {
            crate::sexp::context::r_error("'y' must be a matrix");
        }
        let bm = INTEGER(b_dim).add(0).read();
        let nrhs = INTEGER(b_dim).add(1).read();
        if bm < 0 || nrhs < 0 || bm != m {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_b) = (m as usize).checked_mul(nrhs as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_b > c_int::MAX as usize || XLENGTH(bin) as usize != len_b {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let k = if m < n { m } else { n };

        let Some(scratch_bytes) = len_r
            .checked_mul(std::mem::size_of::<f64>())
            .and_then(|bytes| bytes.checked_add(len_b.checked_mul(std::mem::size_of::<f64>())?))
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native QR coefficient workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut r_copy = vec![0.0f64; len_r];
        if len_r != 0 {
            ptr::copy_nonoverlapping(REAL(qr), r_copy.as_mut_ptr(), len_r);
        }
        let mut b_copy = vec![0.0f64; len_b];
        if len_b != 0 {
            ptr::copy_nonoverlapping(REAL(bin), b_copy.as_mut_ptr(), len_b);
        }

        let mut info: c_int = 0;

        // Solve R^T x = b^T using dtrtrs (transpose)
        super::backend::dtrtrs_(
            b"U".as_ptr(),
            b"T".as_ptr(),
            b"N".as_ptr(),
            &k,
            &nrhs,
            r_copy.as_ptr(),
            &m,
            b_copy.as_mut_ptr(),
            &m,
            &mut info,
        );

        if info != 0 {
            Rf_error(b"error code from Lapack routine 'dtrtrs'\0".as_ptr() as *const c_char);
        }

        let ans = Rf_allocVector(REALSXP_C, ((k as usize) * (nrhs as usize)) as c_int);
        let _ans_guard = protect(ans);
        // Copy only the first k rows
        for j in 0..nrhs as usize {
            for i in 0..k as usize {
                *REAL(ans).add(i + j * k as usize) = b_copy[i + j * m as usize];
            }
        }

        ans
    }
}

/// qr_coef_cmplx - complex QR coefficients.
///
/// Port of: static SEXP qr_coef_cmplx(SEXP q, SEXP bin)
pub unsafe fn qr_coef_cmplx(q: SEXP, bin: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(q) != VECSXP_C || XLENGTH(q) < 1 {
            crate::sexp::context::r_error("'qr' must be a QR decomposition");
        }
        if TYPEOF(bin) != CPLXSXP_C {
            crate::sexp::context::r_error("'y' must be a complex matrix");
        }
        let _q_guard = protect(q);
        let _b_guard = protect(bin);
        let qr = VECTOR_ELT(q, 0);
        if TYPEOF(qr) != CPLXSXP_C {
            crate::sexp::context::r_error("'qr$qr' must be a complex matrix");
        }
        let _qr_guard = protect(qr);

        let dim = getAttrib(qr, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'qr$qr' must be a matrix");
        }
        let m = INTEGER(dim).add(0).read();
        let n = INTEGER(dim).add(1).read();
        if m < 0 || n < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_r) = (m as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_r > c_int::MAX as usize || XLENGTH(qr) as usize != len_r {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let b_dim = getAttrib(bin, R_DimSymbol());
        if b_dim.is_null() || TYPEOF(b_dim) != INTSXP_C || XLENGTH(b_dim) != 2 {
            crate::sexp::context::r_error("'y' must be a matrix");
        }
        let bm = INTEGER(b_dim).add(0).read();
        let nrhs = INTEGER(b_dim).add(1).read();
        if bm < 0 || nrhs < 0 || bm != m {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_b) = (m as usize).checked_mul(nrhs as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_b > c_int::MAX as usize || XLENGTH(bin) as usize != len_b {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        // GNU trusts the stored taus; validate them only after the visible
        // matrix arguments, right before zunmqr reads them.
        let qraux = VECTOR_ELT(q, 2);
        if TYPEOF(qraux) != CPLXSXP_C {
            crate::sexp::context::r_error("'qr$qraux' must be a complex vector");
        }
        let _qraux_guard = protect(qraux);
        let k = if m < n { m } else { n };
        if (XLENGTH(qraux) as usize) < k as usize {
            crate::sexp::context::r_error("invalid QR decomposition fields");
        }

        let Some(scratch_bytes) = len_r
            .checked_mul(std::mem::size_of::<LapRcomplex>())
            .and_then(|bytes| {
                bytes.checked_add(len_b.checked_mul(std::mem::size_of::<LapRcomplex>())?)
            })
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native QR coefficient workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut r_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len_r];
        if len_r != 0 {
            ptr::copy_nonoverlapping(
                COMPLEX(qr) as *const LapRcomplex,
                r_copy.as_mut_ptr(),
                len_r,
            );
        }
        let mut b_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len_b];
        if len_b != 0 {
            ptr::copy_nonoverlapping(
                COMPLEX(bin) as *const LapRcomplex,
                b_copy.as_mut_ptr(),
                len_b,
            );
        }

        // GNU applies Q^H with zunmqr first, then solves R X = B with
        // ztrtrs; B keeps its full n rows (the tail holds Q^H residuals).
        let mut info: c_int = 0;
        let mut tmp = LapRcomplex::default();
        let mut lwork: c_int = -1;
        super::backend::zunmqr_(
            b"L".as_ptr(),
            b"C".as_ptr(),
            &m,
            &nrhs,
            &k,
            r_copy.as_ptr(),
            &m,
            COMPLEX(qraux) as *const LapRcomplex,
            b_copy.as_mut_ptr(),
            &m,
            &mut tmp,
            &lwork,
            &mut info,
        );
        if info != 0 {
            crate::sexp::context::r_error(format!(
                "error code {info} from Lapack routine 'zunmqr'"
            ));
        }
        if !tmp.r.is_finite() || tmp.r < 1.0 || tmp.r > c_int::MAX as f64 {
            crate::sexp::context::r_error("invalid workspace size from Lapack routine 'zunmqr'");
        }
        lwork = tmp.r as c_int;
        let work = R_alloc(lwork as usize, std::mem::size_of::<LapRcomplex>()) as *mut LapRcomplex;
        super::backend::zunmqr_(
            b"L".as_ptr(),
            b"C".as_ptr(),
            &m,
            &nrhs,
            &k,
            r_copy.as_ptr(),
            &m,
            COMPLEX(qraux) as *const LapRcomplex,
            b_copy.as_mut_ptr(),
            &m,
            work,
            &lwork,
            &mut info,
        );
        if info != 0 {
            crate::sexp::context::r_error(format!(
                "error code {info} from Lapack routine 'zunmqr'"
            ));
        }

        super::backend::ztrtrs_(
            b"U".as_ptr(),
            b"N".as_ptr(),
            b"N".as_ptr(),
            &k,
            &nrhs,
            r_copy.as_ptr(),
            &m,
            b_copy.as_mut_ptr(),
            &m,
            &mut info,
        );
        if info != 0 {
            crate::sexp::context::r_error(format!(
                "error code {info} from Lapack routine 'ztrtrs'"
            ));
        }

        let ans = Rf_allocVector(CPLXSXP_C, len_b as c_int);
        let _ans_guard = protect(ans);
        if len_b != 0 {
            ptr::copy_nonoverlapping(b_copy.as_ptr(), COMPLEX(ans) as *mut LapRcomplex, len_b);
        }
        let out_dim = Rf_allocVector(INTSXP_C, 2);
        let _out_dim_guard = protect(out_dim);
        *INTEGER(out_dim) = m;
        *INTEGER(out_dim).add(1) = nrhs;
        setAttrib(ans, R_DimSymbol(), out_dim);
        let bin_dimnames = getAttrib(bin, R_DimNamesSymbol());
        if bin_dimnames != R_NilValue() {
            setAttrib(ans, R_DimNamesSymbol(), bin_dimnames);
        }
        ans
    }
}
/// qr_qy_real - real QR multiply Q*y.
///
/// Port of: static SEXP qr_qy_real(SEXP q, SEXP bin, SEXP trans)
pub unsafe fn qr_qy_real(q: SEXP, bin: SEXP, trans: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(q) != VECSXP_C || XLENGTH(q) < 3 {
            crate::sexp::context::r_error("'qr' must be a QR decomposition");
        }
        if TYPEOF(bin) != REALSXP_C {
            crate::sexp::context::r_error("'y' must be a numeric matrix");
        }
        let _q_guard = protect(q);
        let _b_guard = protect(bin);
        let _trans_guard = protect(trans);
        let qr = VECTOR_ELT(q, 0);
        let qraux = VECTOR_ELT(q, 2);
        if TYPEOF(qr) != REALSXP_C {
            crate::sexp::context::r_error("'qr$qr' must be a numeric matrix");
        }
        if TYPEOF(qraux) != REALSXP_C {
            crate::sexp::context::r_error("'qr$qraux' must be a numeric vector");
        }

        let dim = getAttrib(qr, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'qr$qr' must be a matrix");
        }
        let m = INTEGER(dim).add(0).read();
        let n = INTEGER(dim).add(1).read();
        if m < 0 || n < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_r) = (m as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_r > c_int::MAX as usize || XLENGTH(qr) as usize != len_r {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let b_dim = getAttrib(bin, R_DimSymbol());
        if b_dim.is_null() || TYPEOF(b_dim) != INTSXP_C || XLENGTH(b_dim) != 2 {
            crate::sexp::context::r_error("'y' must be a matrix");
        }
        let bm = INTEGER(b_dim).add(0).read();
        let nrhs = INTEGER(b_dim).add(1).read();
        if bm < 0 || nrhs < 0 || bm != m {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_b) = (bm as usize).checked_mul(nrhs as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_b > c_int::MAX as usize || XLENGTH(bin) as usize != len_b {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let tr = asLogical(trans);
        if tr == NA_INTEGER {
            Rf_error(b"invalid 'trans' argument\0".as_ptr() as *const c_char);
        }

        let k = if m < n { m } else { n };
        if (XLENGTH(qraux) as usize) < k as usize {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        // Work on copies
        let Some(scratch_bytes) = len_r
            .checked_mul(std::mem::size_of::<f64>())
            .and_then(|bytes| bytes.checked_add(len_b.checked_mul(std::mem::size_of::<f64>())?))
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native QR Qy workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut r_copy = vec![0.0f64; len_r];
        if len_r != 0 {
            ptr::copy_nonoverlapping(REAL(qr), r_copy.as_mut_ptr(), len_r);
        }
        let mut b_copy = vec![0.0f64; len_b];
        if len_b != 0 {
            ptr::copy_nonoverlapping(REAL(bin), b_copy.as_mut_ptr(), len_b);
        }

        // Query optimal work size
        let mut tmp: f64 = 0.0;
        let mut lwork: c_int = -1;
        let mut info: c_int = 0;
        let side = b'L';
        let ctrans = if tr != 0 { b'T' } else { b'N' };

        super::backend::dormqr_(
            &side,
            &ctrans,
            &m,
            &nrhs,
            &k,
            r_copy.as_ptr(),
            &m,
            REAL(qraux),
            b_copy.as_mut_ptr(),
            &m,
            &mut tmp,
            &lwork,
            &mut info,
        );

        if info != 0 {
            Rf_error(b"error code from Lapack routine 'dormqr'\0".as_ptr() as *const c_char);
        }

        lwork = tmp as c_int;
        let work = R_alloc(lwork as usize, std::mem::size_of::<f64>()) as *mut f64;

        super::backend::dormqr_(
            &side,
            &ctrans,
            &m,
            &nrhs,
            &k,
            r_copy.as_ptr(),
            &m,
            REAL(qraux),
            b_copy.as_mut_ptr(),
            &m,
            work,
            &lwork,
            &mut info,
        );

        if info != 0 {
            Rf_error(b"error code from Lapack routine 'dormqr'\0".as_ptr() as *const c_char);
        }

        let ans = Rf_allocVector(REALSXP_C, len_b as c_int);
        let _ans_guard = protect(ans);
        ptr::copy_nonoverlapping(b_copy.as_ptr(), REAL(ans), len_b);
        ans
    }
}

/// qr_qy_cmplx - complex QR multiply Q*y.
///
/// Port of: static SEXP qr_qy_cmplx(SEXP q, SEXP bin, SEXP trans)
pub unsafe fn qr_qy_cmplx(q: SEXP, bin: SEXP, trans: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(q) != VECSXP_C || XLENGTH(q) < 3 {
            crate::sexp::context::r_error("'qr' must be a QR decomposition");
        }
        if TYPEOF(bin) != CPLXSXP_C {
            crate::sexp::context::r_error("'y' must be a complex matrix");
        }
        let _q_guard = protect(q);
        let _b_guard = protect(bin);
        let _trans_guard = protect(trans);
        let qr = VECTOR_ELT(q, 0);
        let qraux = VECTOR_ELT(q, 2);
        if TYPEOF(qr) != CPLXSXP_C {
            crate::sexp::context::r_error("'qr$qr' must be a complex matrix");
        }
        if TYPEOF(qraux) != CPLXSXP_C {
            crate::sexp::context::r_error("'qr$qraux' must be a complex vector");
        }

        let dim = getAttrib(qr, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'qr$qr' must be a matrix");
        }
        let m = INTEGER(dim).add(0).read();
        let n = INTEGER(dim).add(1).read();
        if m < 0 || n < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_r) = (m as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_r > c_int::MAX as usize || XLENGTH(qr) as usize != len_r {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let b_dim = getAttrib(bin, R_DimSymbol());
        if b_dim.is_null() || TYPEOF(b_dim) != INTSXP_C || XLENGTH(b_dim) != 2 {
            crate::sexp::context::r_error("'y' must be a matrix");
        }
        let bm = INTEGER(b_dim).add(0).read();
        let nrhs = INTEGER(b_dim).add(1).read();
        if bm < 0 || nrhs < 0 || bm != m {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        let Some(len_b) = (bm as usize).checked_mul(nrhs as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len_b > c_int::MAX as usize || XLENGTH(bin) as usize != len_b {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let tr = asLogical(trans);
        if tr == NA_INTEGER {
            Rf_error(b"invalid 'trans' argument\0".as_ptr() as *const c_char);
        }

        let k = if m < n { m } else { n };
        if (XLENGTH(qraux) as usize) < k as usize {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let Some(scratch_bytes) = len_r
            .checked_mul(std::mem::size_of::<LapRcomplex>())
            .and_then(|bytes| {
                bytes.checked_add(len_b.checked_mul(std::mem::size_of::<LapRcomplex>())?)
            })
        else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native QR Qy workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut r_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len_r];
        if len_r != 0 {
            ptr::copy_nonoverlapping(
                COMPLEX(qr) as *const LapRcomplex,
                r_copy.as_mut_ptr(),
                len_r,
            );
        }
        let mut b_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len_b];
        if len_b != 0 {
            ptr::copy_nonoverlapping(
                COMPLEX(bin) as *const LapRcomplex,
                b_copy.as_mut_ptr(),
                len_b,
            );
        }

        // Query optimal work size
        let mut tmp = LapRcomplex::default();
        let mut lwork: c_int = -1;
        let mut info: c_int = 0;
        let side = b'L';
        let ctrans = if tr != 0 { b'C' } else { b'N' };

        super::backend::zunmqr_(
            &side,
            &ctrans,
            &m,
            &nrhs,
            &k,
            r_copy.as_ptr(),
            &m,
            COMPLEX(qraux) as *const LapRcomplex,
            b_copy.as_mut_ptr(),
            &m,
            &mut tmp,
            &lwork,
            &mut info,
        );

        if info != 0 {
            crate::sexp::context::r_error(format!(
                "error code {info} from Lapack routine 'zunmqr'"
            ));
        }

        lwork = tmp.r as c_int;
        let work = R_alloc(lwork as usize, std::mem::size_of::<LapRcomplex>()) as *mut LapRcomplex;

        super::backend::zunmqr_(
            &side,
            &ctrans,
            &m,
            &nrhs,
            &k,
            r_copy.as_ptr(),
            &m,
            COMPLEX(qraux) as *const LapRcomplex,
            b_copy.as_mut_ptr(),
            &m,
            work,
            &lwork,
            &mut info,
        );

        if info != 0 {
            crate::sexp::context::r_error(format!(
                "error code {info} from Lapack routine 'zunmqr'"
            ));
        }

        let ans = Rf_allocVector(CPLXSXP_C, len_b as c_int);
        let _ans_guard = protect(ans);
        ptr::copy_nonoverlapping(b_copy.as_ptr(), COMPLEX(ans) as *mut LapRcomplex, len_b);
        ans
    }
}
