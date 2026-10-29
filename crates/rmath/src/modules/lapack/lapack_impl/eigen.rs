use super::*;

/// La_rs - real symmetric eigenvalues/eigenvectors.
///
/// Port of: static SEXP La_rs(SEXP x, SEXP only_values)
pub unsafe fn La_rs(x: SEXP, only_values: SEXP) -> SEXP {
    unsafe {
        let dim = getAttrib(x, R_DimSymbol());
        if dim.is_null() || dim == R_NilValue() {
            Rf_error(b"'x' must be a matrix\0".as_ptr() as *const c_char);
        }

        let n = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
        let n2 = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;
        if n != n2 {
            Rf_error(b"'x' must be a square numeric matrix\0".as_ptr() as *const c_char);
        }

        let ov = asLogical(only_values);
        if ov == NA_INTEGER {
            Rf_error(b"invalid 'only.values' argument\0".as_ptr() as *const c_char);
        }

        let jobv = if ov != 0 { b'N' } else { b'V' };
        let uplo = b'L';
        let range = b'A';

        // Work on a copy of x
        let rx: *mut f64;
        let mut x = x;
        let mut guards = Vec::new();
        if TYPEOF(x) != 14 {
            x = coerceVector(x, REALSXP_C);
            guards.push(protect(x));
            rx = REAL(x);
        } else {
            rx = R_alloc((n as usize) * (n as usize), std::mem::size_of::<f64>()) as *mut f64;
            ptr::copy_nonoverlapping(REAL(x), rx, (n as usize) * (n as usize));
        }
        guards.push(protect(x));

        let values = Rf_allocVector(REALSXP_C, n as c_int);
        guards.push(protect(values));
        let rvalues = REAL(values);

        let mut z = R_NilValue();
        let mut rz: *mut f64 = ptr::null_mut();
        if ov == 0 {
            z = Rf_allocVector(REALSXP_C, (n as c_int) * (n as c_int));
            guards.push(protect(z));
            rz = REAL(z);
        }

        let isuppz = R_alloc(2 * n as usize, std::mem::size_of::<c_int>()) as *mut c_int;

        // Query optimal work sizes
        let mut tmp: f64 = 0.0;
        let mut itmp: c_int = 0;
        let mut lwork: c_int = -1;
        let mut liwork: c_int = -1;
        let mut m: c_int = 0;
        let mut info: c_int = 0;

        super::backend::dsyevr_(
            &jobv, &range, &uplo, &n, rx, &n, &0.0f64, &0.0f64, &0, &0, &0.0f64, &mut m, rvalues,
            rz, &n, isuppz, &mut tmp, &lwork, &mut itmp, &liwork, &mut info,
        );

        if info != 0 {
            Rf_error(b"error code from Lapack routine 'dsyevr'\0".as_ptr() as *const c_char);
        }

        lwork = tmp as c_int;
        liwork = itmp;

        let work = R_alloc(lwork as usize, std::mem::size_of::<f64>()) as *mut f64;
        let iwork = R_alloc(liwork as usize, std::mem::size_of::<c_int>()) as *mut c_int;

        super::backend::dsyevr_(
            &jobv, &range, &uplo, &n, rx, &n, &0.0f64, &0.0f64, &0, &0, &0.0f64, &mut m, rvalues,
            rz, &n, isuppz, work, &lwork, iwork, &liwork, &mut info,
        );

        if info != 0 {
            Rf_error(b"error code from Lapack routine 'dsyevr'\0".as_ptr() as *const c_char);
        }

        let ret;
        let nm;
        if ov == 0 {
            ret = Rf_allocVector(VECSXP_C, 2);
            guards.push(protect(ret));
            nm = Rf_allocVector(STRSXP_C, 2);
            guards.push(protect(nm));
            SET_STRING_ELT(nm, 1, Rf_mkChar(b"vectors\0".as_ptr() as *const c_char));
            SET_VECTOR_ELT(ret, 1, z);
        } else {
            ret = Rf_allocVector(VECSXP_C, 1);
            guards.push(protect(ret));
            nm = Rf_allocVector(STRSXP_C, 1);
            guards.push(protect(nm));
        }
        SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
        setAttrib(ret, R_NamesSymbol(), nm);
        SET_VECTOR_ELT(ret, 0, values);

        ret
    }
}

/// La_rg - real eigenvalues/eigenvectors (general, non-symmetric).
///
/// Port of: static SEXP La_rg(SEXP x, SEXP only_values)
pub unsafe fn La_rg(x: SEXP, only_values: SEXP) -> SEXP {
    unsafe {
        let dim = getAttrib(x, R_DimSymbol());
        if dim.is_null() || dim == R_NilValue() {
            Rf_error(b"'x' must be a matrix\0".as_ptr() as *const c_char);
        }

        let n = INTEGER(coerceVector(dim, INTSXP_C)).add(0).read() as i32;
        let n2 = INTEGER(coerceVector(dim, INTSXP_C)).add(1).read() as i32;
        if n != n2 {
            Rf_error(b"'x' must be a square numeric matrix\0".as_ptr() as *const c_char);
        }

        let ov = asLogical(only_values);
        if ov == NA_INTEGER {
            Rf_error(b"invalid 'only.values' argument\0".as_ptr() as *const c_char);
        }

        let jobvl = b'N';
        let jobvr = if ov != 0 { b'N' } else { b'V' };

        // Work on a copy of x
        let xvals: *mut f64;
        let mut x = x;
        if TYPEOF(x) != 14 {
            x = coerceVector(x, REALSXP_C);
            xvals = REAL(x);
        } else {
            xvals = R_alloc((n as usize) * (n as usize), std::mem::size_of::<f64>()) as *mut f64;
            ptr::copy_nonoverlapping(REAL(x), xvals, (n as usize) * (n as usize));
        }
        let _x_guard = protect(x);

        let wR = R_alloc(n as usize, std::mem::size_of::<f64>()) as *mut f64;
        let wI = R_alloc(n as usize, std::mem::size_of::<f64>()) as *mut f64;

        let mut right: *mut f64 = ptr::null_mut();
        if ov == 0 {
            right = R_alloc((n as usize) * (n as usize), std::mem::size_of::<f64>()) as *mut f64;
        }

        // Query optimal work size
        let mut tmp: f64 = 0.0;
        let mut lwork: c_int = -1;
        let mut info: c_int = 0;

        super::backend::dgeev_(
            &jobvl,
            &jobvr,
            &n,
            xvals,
            &n,
            wR,
            wI,
            ptr::null_mut(),
            &1,
            right,
            &n,
            &mut tmp,
            &lwork,
            &mut info,
        );

        if info != 0 {
            Rf_error(b"error code from Lapack routine 'dgeev'\0".as_ptr() as *const c_char);
        }

        lwork = tmp as c_int;
        let work = R_alloc(lwork as usize, std::mem::size_of::<f64>()) as *mut f64;

        super::backend::dgeev_(
            &jobvl,
            &jobvr,
            &n,
            xvals,
            &n,
            wR,
            wI,
            ptr::null_mut(),
            &1,
            right,
            &n,
            work,
            &lwork,
            &mut info,
        );

        if info != 0 {
            Rf_error(b"error code from Lapack routine 'dgeev'\0".as_ptr() as *const c_char);
        }

        // Build the result
        let ret;
        let nm;

        if ov == 0 {
            // Check if any eigenvalues are complex
            let has_complex = (0..n as usize).any(|i| *wI.add(i) != 0.0);

            if has_complex {
                // Build complex eigenvalue vector
                let values = Rf_allocVector(CPLXSXP_C, n as c_int);
                let _values_guard = protect(values);
                for i in 0..n as usize {
                    let c = COMPLEX(values).add(i);
                    (*c).r = *wR.add(i);
                    (*c).i = *wI.add(i);
                }

                // Allocate admitted output before borrowing any native input
                // or fresh payload; reconstruction needs no intermediate Vec.
                let z = complex_eigenvector_matrix(n, wI, right);
                let _z_guard = protect(z);

                ret = Rf_allocVector(VECSXP_C, 2);
                let _ret_guard = protect(ret);
                nm = Rf_allocVector(STRSXP_C, 2);
                let _nm_guard = protect(nm);
                SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
                SET_STRING_ELT(nm, 1, Rf_mkChar(b"vectors\0".as_ptr() as *const c_char));
                SET_VECTOR_ELT(ret, 0, values);
                SET_VECTOR_ELT(ret, 1, z);
                setAttrib(ret, R_NamesSymbol(), nm);

                return ret;
            } else {
                // All real eigenvalues
                let values = Rf_allocVector(REALSXP_C, n as c_int);
                let _values_guard = protect(values);
                ptr::copy_nonoverlapping(wR, REAL(values), n as usize);

                let z = Rf_allocVector(REALSXP_C, (n as c_int) * (n as c_int));
                let _z_guard = protect(z);
                ptr::copy_nonoverlapping(right, REAL(z), (n as usize) * (n as usize));

                ret = Rf_allocVector(VECSXP_C, 2);
                let _ret_guard = protect(ret);
                nm = Rf_allocVector(STRSXP_C, 2);
                let _nm_guard = protect(nm);
                SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
                SET_STRING_ELT(nm, 1, Rf_mkChar(b"vectors\0".as_ptr() as *const c_char));
                SET_VECTOR_ELT(ret, 0, values);
                SET_VECTOR_ELT(ret, 1, z);
                setAttrib(ret, R_NamesSymbol(), nm);

                return ret;
            }
        } else {
            // Only values
            let values = Rf_allocVector(REALSXP_C, n as c_int);
            let _values_guard = protect(values);
            ptr::copy_nonoverlapping(wR, REAL(values), n as usize);

            ret = Rf_allocVector(VECSXP_C, 1);
            let _ret_guard = protect(ret);
            nm = Rf_allocVector(STRSXP_C, 1);
            let _nm_guard = protect(nm);
            SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
            SET_VECTOR_ELT(ret, 0, values);
            setAttrib(ret, R_NamesSymbol(), nm);

            ret
        }
    }
}

/// La_rs_cmplx - complex symmetric eigenvalues/eigenvectors.
///
/// Port of: static SEXP La_rs_cmplx(SEXP xin, SEXP only_values)
pub unsafe fn La_rs_cmplx(xin: SEXP, only_values: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(xin) != CPLXSXP_C {
            crate::sexp::context::r_error("'x' must be a complex matrix");
        }
        let _input_guard = protect(xin);
        let _ov_guard = protect(only_values);
        let dim = getAttrib(xin, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'x' must be a matrix");
        }

        let n = INTEGER(dim).add(0).read();
        let n2 = INTEGER(dim).add(1).read();
        if n < 0 || n2 < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        if n != n2 {
            crate::sexp::context::r_error("'x' must be a square numeric matrix");
        }
        let Some(len) = (n as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(xin) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let ov = asLogical(only_values);
        if ov == NA_INTEGER {
            Rf_error(b"invalid 'only.values' argument\0".as_ptr() as *const c_char);
        }

        let jobv = if ov != 0 { b'N' } else { b'V' };
        let uplo = b'U';

        let Some(rwork_len) = (n as usize).checked_mul(3) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let Some(scratch_bytes) = len
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
                "allocation failed: native complex eigen workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut a_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len];
        if len != 0 {
            ptr::copy_nonoverlapping(COMPLEX(xin) as *const LapRcomplex, a_copy.as_mut_ptr(), len);
        }

        let values = Rf_allocVector(REALSXP_C, n as c_int);
        let _values_guard = protect(values);

        // Query optimal work size
        let mut tmp = LapRcomplex::default();
        let mut rwork = vec![0.0f64; rwork_len];
        let mut lwork: c_int = -1;
        let mut info: c_int = 0;

        super::backend::zheev_(
            &jobv,
            &uplo,
            &n,
            a_copy.as_mut_ptr(),
            &n,
            REAL(values),
            &mut tmp,
            &lwork,
            rwork.as_mut_ptr(),
            &mut info,
        );

        if info != 0 {
            Rf_error(b"error code from Lapack routine 'zheev'\0".as_ptr() as *const c_char);
        }

        lwork = tmp.r as c_int;
        let work = R_alloc(lwork as usize, std::mem::size_of::<LapRcomplex>()) as *mut LapRcomplex;

        super::backend::zheev_(
            &jobv,
            &uplo,
            &n,
            a_copy.as_mut_ptr(),
            &n,
            REAL(values),
            work,
            &lwork,
            rwork.as_mut_ptr(),
            &mut info,
        );

        if info != 0 {
            Rf_error(b"error code from Lapack routine 'zheev'\0".as_ptr() as *const c_char);
        }

        let ret;
        let nm;
        let _ret_guard;
        let _nm_guard;
        if ov == 0 {
            let z = Rf_allocVector(CPLXSXP_C, len as c_int);
            let _z_guard = protect(z);
            ptr::copy_nonoverlapping(a_copy.as_ptr(), COMPLEX(z) as *mut LapRcomplex, len);

            ret = Rf_allocVector(VECSXP_C, 2);
            _ret_guard = protect(ret);
            nm = Rf_allocVector(STRSXP_C, 2);
            _nm_guard = protect(nm);
            SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
            SET_STRING_ELT(nm, 1, Rf_mkChar(b"vectors\0".as_ptr() as *const c_char));
            SET_VECTOR_ELT(ret, 0, values);
            SET_VECTOR_ELT(ret, 1, z);
        } else {
            ret = Rf_allocVector(VECSXP_C, 1);
            _ret_guard = protect(ret);
            nm = Rf_allocVector(STRSXP_C, 1);
            _nm_guard = protect(nm);
            SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
            SET_VECTOR_ELT(ret, 0, values);
        }

        setAttrib(ret, R_NamesSymbol(), nm);
        ret
    }
}

/// La_rg_cmplx - complex eigenvalues/eigenvectors.
///
/// Port of: static SEXP La_rg_cmplx(SEXP x, SEXP only_values)
pub unsafe fn La_rg_cmplx(x: SEXP, only_values: SEXP) -> SEXP {
    unsafe {
        if TYPEOF(x) != CPLXSXP_C {
            crate::sexp::context::r_error("'x' must be a complex matrix");
        }
        let _input_guard = protect(x);
        let _ov_guard = protect(only_values);
        let dim = getAttrib(x, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'x' must be a matrix");
        }

        let n = INTEGER(dim).add(0).read();
        let n2 = INTEGER(dim).add(1).read();
        if n < 0 || n2 < 0 {
            crate::sexp::context::r_error("invalid matrix dimensions");
        }
        if n != n2 {
            crate::sexp::context::r_error("'x' must be a square numeric matrix");
        }
        let Some(len) = (n as usize).checked_mul(n as usize) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        if len > c_int::MAX as usize || XLENGTH(x) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let ov = asLogical(only_values);
        if ov == NA_INTEGER {
            Rf_error(b"invalid 'only.values' argument\0".as_ptr() as *const c_char);
        }

        let jobvl = b'N';
        let jobvr = if ov != 0 { b'N' } else { b'V' };

        let Some(rwork_len) = (n as usize).checked_mul(2) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let Some(scratch_bytes) = len
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
                "allocation failed: native complex eigen workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut a_copy: Vec<LapRcomplex> = vec![LapRcomplex::default(); len];
        if len != 0 {
            ptr::copy_nonoverlapping(COMPLEX(x) as *const LapRcomplex, a_copy.as_mut_ptr(), len);
        }

        let values = Rf_allocVector(CPLXSXP_C, n as c_int);
        let _values_guard = protect(values);

        let mut vr: *mut LapRcomplex = ptr::null_mut();
        if ov == 0 {
            vr = R_alloc(len, std::mem::size_of::<LapRcomplex>()) as *mut LapRcomplex;
        }

        // Query optimal work size
        let mut tmp = LapRcomplex::default();
        let mut rwork = vec![0.0f64; rwork_len];
        let mut lwork: c_int = -1;
        let mut info: c_int = 0;

        super::backend::zgeev_(
            &jobvl,
            &jobvr,
            &n,
            a_copy.as_mut_ptr(),
            &n,
            COMPLEX(values) as *mut LapRcomplex,
            ptr::null_mut(),
            &1,
            vr,
            &n,
            &mut tmp,
            &lwork,
            rwork.as_mut_ptr(),
            &mut info,
        );

        if info != 0 {
            Rf_error(b"error code from Lapack routine 'zgeev'\0".as_ptr() as *const c_char);
        }

        lwork = tmp.r as c_int;
        let work = R_alloc(lwork as usize, std::mem::size_of::<LapRcomplex>()) as *mut LapRcomplex;

        super::backend::zgeev_(
            &jobvl,
            &jobvr,
            &n,
            a_copy.as_mut_ptr(),
            &n,
            COMPLEX(values) as *mut LapRcomplex,
            ptr::null_mut(),
            &1,
            vr,
            &n,
            work,
            &lwork,
            rwork.as_mut_ptr(),
            &mut info,
        );

        if info != 0 {
            Rf_error(b"error code from Lapack routine 'zgeev'\0".as_ptr() as *const c_char);
        }

        let ret;
        let nm;
        let _ret_guard;
        let _nm_guard;
        if ov == 0 {
            let z = Rf_allocVector(CPLXSXP_C, len as c_int);
            let _z_guard = protect(z);
            ptr::copy_nonoverlapping(vr, COMPLEX(z) as *mut LapRcomplex, len);

            ret = Rf_allocVector(VECSXP_C, 2);
            _ret_guard = protect(ret);
            nm = Rf_allocVector(STRSXP_C, 2);
            _nm_guard = protect(nm);
            SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
            SET_STRING_ELT(nm, 1, Rf_mkChar(b"vectors\0".as_ptr() as *const c_char));
            SET_VECTOR_ELT(ret, 0, values);
            SET_VECTOR_ELT(ret, 1, z);
        } else {
            ret = Rf_allocVector(VECSXP_C, 1);
            _ret_guard = protect(ret);
            nm = Rf_allocVector(STRSXP_C, 1);
            _nm_guard = protect(nm);
            SET_STRING_ELT(nm, 0, Rf_mkChar(b"values\0".as_ptr() as *const c_char));
            SET_VECTOR_ELT(ret, 0, values);
        }

        setAttrib(ret, R_NamesSymbol(), nm);
        ret
    }
}

/// Allocate the final complex matrix before projecting either input buffer.
/// # Safety
/// For a nonnegative, representable n-by-n matrix, imaginary and vecs point to
/// n and n*n initialized f64 elements respectively. These caller-owned native
/// buffers remain live across allocation/GC; they are not runtime field loans.
unsafe fn complex_eigenvector_matrix(n: i32, imaginary: *const f64, vecs: *const f64) -> SEXP {
    unsafe {
        let n = usize::try_from(n)
            .unwrap_or_else(|_| crate::sexp::context::r_error("invalid matrix dimensions"));
        let len = n
            .checked_mul(n)
            .and_then(|len| c_int::try_from(len).ok())
            .unwrap_or_else(|| crate::sexp::context::r_error("matrix dimensions are too large"));
        let result = Rf_allocVector(CPLXSXP_C, len);
        if result.is_null() {
            crate::sexp::context::r_error("cannot allocate complex eigenvector matrix");
        }
        let _result_guard = protect(result);
        let imaginary = std::slice::from_raw_parts(imaginary, n);
        let vecs = std::slice::from_raw_parts(vecs, len as usize);
        let output = std::slice::from_raw_parts_mut(COMPLEX(result), len as usize);
        unscramble_into(imaginary, n, vecs, output)
            .unwrap_or_else(|| crate::sexp::context::r_error("invalid packed eigenvector data"));
        result
    }
}

#[cfg(test)]
#[path = "../real_rg_tests.rs"]
mod real_rg_tests;
