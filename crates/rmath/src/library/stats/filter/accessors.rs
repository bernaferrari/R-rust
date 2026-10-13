//! Default model accessors: coef/fitted/resid/deviance/df.residual/nobs/weights

use super::*;

/// GNU `coef(object)` — `$coefficients`, else `$coef`.
pub unsafe fn do_coef(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let x = CAR(args);
        let v = named_list_elt(x, "coefficients");
        if !v.is_null() && v != R_NilValue() {
            return v;
        }
        named_list_elt(x, "coef")
    }
}

/// GNU default `fitted(object)`.
pub unsafe fn do_fitted(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { named_list_elt(CAR(args), "fitted.values") }
}

/// GNU default `resid`/`residuals`.
pub unsafe fn do_resid(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { named_list_elt(CAR(args), "residuals") }
}

/// GNU default `deviance(object)`.
pub unsafe fn do_deviance(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { named_list_elt(CAR(args), "deviance") }
}

/// GNU default `df.residual(object)`.
pub unsafe fn do_df_residual(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { named_list_elt(CAR(args), "df.residual") }
}

/// GNU `nobs` — `$nobs`/`$n.obs`, else `nobs.lm`: `sum(weights != 0)` or `NROW(residuals)`.
pub unsafe fn do_nobs(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe {
        let mut ans = R_NilValue();
        if crate::eval::dispatch::DispatchOrEval(
            _call,
            _op,
            c"nobs".as_ptr(),
            args,
            _rho,
            &mut ans,
            0,
            1,
        ) != 0
        {
            return ans;
        }
        let x = CAR(args);
        let n = named_list_elt(x, "nobs");
        if !n.is_null() && n != R_NilValue() {
            return n;
        }
        let n = named_list_elt(x, "n.obs");
        if !n.is_null() && n != R_NilValue() {
            return n;
        }
        let w = named_list_elt(x, "weights");
        if !w.is_null() && w != R_NilValue() {
            let mut nz = 0i32;
            let nw = XLENGTH(w);
            if TYPEOF(w) == SEXPTYPE::REALSXP {
                for i in 0..nw {
                    let v = *REAL(w).add(i as usize);
                    if v != 0.0 && !v.is_nan() {
                        nz += 1;
                    }
                }
            } else if TYPEOF(w) == SEXPTYPE::INTSXP || TYPEOF(w) == SEXPTYPE::LGLSXP {
                for i in 0..nw {
                    let v = *INTEGER(w).add(i as usize);
                    if v != 0 && v != NA_INTEGER {
                        nz += 1;
                    }
                }
            }
            return Rf_ScalarInteger(nz);
        }
        let r = named_list_elt(x, "residuals");
        if !r.is_null() && r != R_NilValue() {
            return Rf_ScalarInteger(XLENGTH(r) as i32);
        }
        R_NilValue()
    }
}

/// GNU default `weights(object)`.
pub unsafe fn do_weights(_call: SEXP, _op: SEXP, args: SEXP, _rho: SEXP) -> SEXP {
    unsafe { named_list_elt(CAR(args), "weights") }
}
