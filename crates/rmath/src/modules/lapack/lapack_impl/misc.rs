use super::*;

/// det_ge_real - real matrix determinant.
///
/// Port of: static SEXP det_ge_real(SEXP ain, SEXP logarithm)
pub fn det_ge_real(ain: SEXP, logarithm: SEXP) -> SEXP {
    unsafe {
        let ldet = asLogical(logarithm);
        if ldet == NA_INTEGER {
            Rf_error_cstr(c"invalid 'logarithm' argument");
        }
        let ain = if TYPEOF(ain) == REALSXP_C {
            ain
        } else if TYPEOF(ain) == INTSXP_C || TYPEOF(ain) == LGLSXP_C {
            let coerced = coerceVector(ain, REALSXP_C);
            let _c = protect(coerced);
            coerced
        } else {
            crate::sexp::context::r_error("'a' must be a numeric matrix");
        };
        let _a_guard = protect(ain);
        let _log_guard = protect(logarithm);

        let dim = getAttrib(ain, R_DimSymbol());
        if dim.is_null() || TYPEOF(dim) != INTSXP_C || XLENGTH(dim) != 2 {
            crate::sexp::context::r_error("'a' must be a numeric matrix");
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
        if len > c_int::MAX as usize || XLENGTH(ain) as usize != len {
            crate::sexp::context::r_error("invalid matrix dimensions or length");
        }

        let Some(scratch_bytes) = len.checked_mul(std::mem::size_of::<f64>()) else {
            crate::sexp::context::r_error("matrix dimensions are too large");
        };
        let scratch_reservation = with_current_instance(|instance| {
            with_arena_in(instance, |arena| arena.try_reserve_transient(scratch_bytes))
        });
        if matches!(scratch_reservation, Some(None)) {
            crate::sexp::context::r_error(
                "allocation failed: native determinant workspace exceeds resource limit",
            );
        }
        let _scratch_reservation = scratch_reservation.flatten();

        let mut a_copy = vec![0.0f64; len];
        if len != 0 {
            ptr::copy_nonoverlapping(REAL(ain), a_copy.as_mut_ptr(), len);
        }

        let mut ipiv_ws = vec![0 as c_int; n as usize];
        let ipiv = ipiv_ws.as_mut_ptr();
        let mut info: c_int = 0;
        super::backend::dgetrf_(&n, &n, a_copy.as_mut_ptr(), &n, ipiv, &mut info);
        if info < 0 {
            crate::sexp::context::r_error("error code from Lapack routine 'dgetrf'");
        }

        let use_log = ldet != 0;
        let mut sign: c_int = 1;
        let modulus = if info > 0 {
            if use_log { f64::NEG_INFINITY } else { 0.0 }
        } else {
            for i in 0..n as usize {
                if *ipiv.add(i) != (i as c_int + 1) {
                    sign = -sign;
                }
            }
            if use_log {
                let mut acc = 0.0;
                let n1 = n as usize + 1;
                for i in 0..n as usize {
                    let dii = a_copy[i * n1];
                    acc += dii.abs().ln();
                    if dii < 0.0 {
                        sign = -sign;
                    }
                }
                acc
            } else {
                let mut acc = 1.0;
                let n1 = n as usize + 1;
                for i in 0..n as usize {
                    acc *= a_copy[i * n1];
                }
                if acc < 0.0 {
                    sign = -sign;
                    -acc
                } else {
                    acc
                }
            }
        };

        let val = Rf_allocVector(VECSXP_C, 2);
        let _val = protect(val);
        let nm = Rf_allocVector(STRSXP_C, 2);
        let _nm = protect(nm);
        SET_STRING_ELT(nm, 0, crate::sexp::constructors::mk_char_str("modulus"));
        SET_STRING_ELT(nm, 1, crate::sexp::constructors::mk_char_str("sign"));
        setAttrib(val, R_NamesSymbol(), nm);
        let modulus_s = Rf_ScalarReal(modulus);
        let _ms = protect(modulus_s);
        setAttrib(
            modulus_s,
            crate::sexp::symbol::install_str("logarithm"),
            Rf_ScalarLogical(if use_log { 1 } else { 0 }),
        );
        SET_VECTOR_ELT(val, 0, modulus_s);
        SET_VECTOR_ELT(val, 1, Rf_ScalarInteger(sign));
        setAttrib(
            val,
            R_ClassSymbol(),
            Rf_ScalarString(crate::sexp::constructors::mk_char_str("det")),
        );
        val
    }
}

/// mod_do_lapack - main LAPACK dispatcher.
///
/// Port of: SEXP mod_do_lapack(SEXP call, SEXP op, SEXP args, SEXP env)
pub fn mod_do_lapack(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    {
        // This dispatches based on op symbol to the appropriate La_* function.
        // In R, this is done via the .Internal() mechanism.
        // For now, return nil as dispatch needs the full .Internal infrastructure.
        R_NilValue()
    }
}

/// R_init_lapack - LAPACK module initialization.
///
/// Port of: void R_init_lapack(DllInfo *dll)
pub fn R_init_lapack(_info: *mut std::ffi::c_void) {
    {
        // Registration would happen here in the full implementation
    }
}
