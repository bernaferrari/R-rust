//! mathstats tests — extracted verbatim from essentials/mathstats.rs.

use super::*;

#[cfg(test)]
mod trunk_r90451_tests {
    use super::{do_glm_control, do_lm_wfit};
    use crate::sexp::context::RError;
    use crate::sexp::session::RSession;

    fn r_error_message<F: FnOnce()>(body: F) -> String {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(body))
            .expect_err("expected an R error")
            .downcast::<RError>()
            .expect("R error")
            .message
    }

    #[test]
    fn trunk_r90451_glm_control_reports_tol_and_wtol() {
        let _session = RSession::new();
        unsafe {
            let ans = do_glm_control(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                crate::sexp::globals::R_NilValue(),
                std::ptr::null_mut(),
            );
            assert_eq!(crate::sexp::accessors::XLENGTH(ans), 5);
            let epsilon = *crate::sexp::accessors::REAL(crate::sexp::accessors::VECTOR_ELT(ans, 0));
            let tol = *crate::sexp::accessors::REAL(crate::sexp::accessors::VECTOR_ELT(ans, 3));
            let wtol = *crate::sexp::accessors::REAL(crate::sexp::accessors::VECTOR_ELT(ans, 4));
            assert!((epsilon - 1e-8).abs() < 1e-18);
            assert!((tol - 1e-11).abs() < 1e-20, "{tol}");
            assert_eq!(wtol, 0.0);

            let inf = crate::sexp::constructors::Rf_ScalarReal(f64::INFINITY);
            let args = crate::sexp::constructors::Rf_cons(inf, crate::sexp::globals::R_NilValue());
            let wide = do_glm_control(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );
            let wide_tol =
                *crate::sexp::accessors::REAL(crate::sexp::accessors::VECTOR_ELT(wide, 3));
            assert!((wide_tol - 1e-7).abs() < 1e-18, "{wide_tol}");

            let zero = crate::sexp::constructors::Rf_ScalarReal(0.0);
            let bad = crate::sexp::constructors::Rf_cons(zero, crate::sexp::globals::R_NilValue());
            let message = r_error_message(|| {
                do_glm_control(
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    bad,
                    std::ptr::null_mut(),
                );
            });
            assert!(
                message.contains("value of 'epsilon' must be > 0"),
                "{message}"
            );
        }
    }

    #[test]
    fn trunk_r90451_lm_wfit_drops_weights_below_wtol_times_sum() {
        let _session = RSession::new();
        unsafe {
            let x =
                crate::sexp::constructors::Rf_allocVector3(crate::sexp::ffi::SEXPTYPE::REALSXP, 6);
            for (i, value) in [1.0, 1.0, 1.0, 0.0, 1.0, 2.0].iter().enumerate() {
                *crate::sexp::accessors::REAL(x).add(i) = *value;
            }
            let dim =
                crate::sexp::constructors::Rf_allocVector3(crate::sexp::ffi::SEXPTYPE::INTSXP, 2);
            *crate::sexp::accessors::INTEGER(dim) = 3;
            *crate::sexp::accessors::INTEGER(dim).add(1) = 2;
            crate::sexp::attrib_core::setAttrib(x, crate::sexp::attrib_core::R_DimSymbol(), dim);
            let y =
                crate::sexp::constructors::Rf_allocVector3(crate::sexp::ffi::SEXPTYPE::REALSXP, 3);
            for (i, value) in [0.0, 1.0, 100.0].iter().enumerate() {
                *crate::sexp::accessors::REAL(y).add(i) = *value;
            }
            let w =
                crate::sexp::constructors::Rf_allocVector3(crate::sexp::ffi::SEXPTYPE::REALSXP, 3);
            for (i, value) in [1.0, 1.0, 1e-8].iter().enumerate() {
                *crate::sexp::accessors::REAL(w).add(i) = *value;
            }
            let wtol = crate::sexp::constructors::Rf_ScalarReal(0.1);
            let args = crate::sexp::constructors::Rf_cons(
                x,
                crate::sexp::constructors::Rf_cons(
                    y,
                    crate::sexp::constructors::Rf_cons(
                        w,
                        crate::sexp::constructors::Rf_cons(
                            wtol,
                            crate::sexp::globals::R_NilValue(),
                        ),
                    ),
                ),
            );
            let ans = do_lm_wfit(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );
            let coef = crate::sexp::accessors::VECTOR_ELT(ans, 0);
            let b0 = *crate::sexp::accessors::REAL(coef);
            let b1 = *crate::sexp::accessors::REAL(coef).add(1);
            assert!(b0.abs() < 1e-8, "{b0}");
            assert!((b1 - 1.0).abs() < 1e-8, "{b1}");

            let negative = crate::sexp::constructors::Rf_ScalarReal(-1.0);
            let bad = crate::sexp::constructors::Rf_cons(
                x,
                crate::sexp::constructors::Rf_cons(
                    y,
                    crate::sexp::constructors::Rf_cons(
                        w,
                        crate::sexp::constructors::Rf_cons(
                            negative,
                            crate::sexp::globals::R_NilValue(),
                        ),
                    ),
                ),
            );
            let message = r_error_message(|| {
                do_lm_wfit(
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    bad,
                    std::ptr::null_mut(),
                );
            });
            assert!(
                message.contains("value of 'wtol' must be >= 0"),
                "{message}"
            );
        }
    }

    #[test]
    fn lm_fit_three_columns_matches_gnu_qr() {
        // GNU R 4.6.1 lm.fit(cbind(1, 1:4, c(2,0,1,3)), c(1,3,3,6)).
        let _session = RSession::new();
        unsafe {
            let x = crate::mainutils::array::allocMatrix(
                crate::sexp::ffi::SEXPTYPE::REALSXP.as_c_int(),
                4,
                3,
            );
            let xr = crate::sexp::accessors::REAL(x);
            let cols = [
                [1.0, 1.0, 1.0, 1.0],
                [1.0, 2.0, 3.0, 4.0],
                [2.0, 0.0, 1.0, 3.0],
            ];
            for (j, col) in cols.iter().enumerate() {
                for (i, value) in col.iter().enumerate() {
                    *xr.add(i + j * 4) = *value;
                }
            }
            let y =
                crate::sexp::constructors::Rf_allocVector3(crate::sexp::ffi::SEXPTYPE::REALSXP, 4);
            let ys = [1.0, 3.0, 3.0, 6.0];
            for (i, value) in ys.iter().enumerate() {
                *crate::sexp::accessors::REAL(y).add(i) = *value;
            }
            let args = crate::sexp::constructors::Rf_cons(
                x,
                crate::sexp::constructors::Rf_cons(y, crate::sexp::globals::R_NilValue()),
            );
            let result = super::do_lm_fit(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                args,
                std::ptr::null_mut(),
            );
            assert!(
                !result.is_null() && result != crate::sexp::globals::R_NilValue(),
                "lm.fit returned NULL for a 3-column design"
            );
            let coef = crate::sexp::accessors::VECTOR_ELT(result, 0);
            let expected = [-0.5595238095_f64, 1.4523809524, 0.1190476190];
            for (i, want) in expected.iter().enumerate() {
                let got = *crate::sexp::accessors::REAL(coef).add(i);
                assert!((got - want).abs() < 1e-8, "coef {i}: {got} vs {want}");
            }
            let rank =
                *crate::sexp::accessors::INTEGER(crate::sexp::accessors::VECTOR_ELT(result, 3));
            assert_eq!(rank, 3);
            let resid = crate::sexp::accessors::VECTOR_ELT(result, 1);
            let expected_resid = [-0.1309523810_f64, 0.6547619048, -0.9166666667, 0.3928571429];
            for (i, want) in expected_resid.iter().enumerate() {
                let got = *crate::sexp::accessors::REAL(resid).add(i);
                assert!((got - want).abs() < 1e-8, "resid {i}: {got} vs {want}");
            }
        }
    }
}

#[cfg(test)]
#[path = "lm_intercept_tests.rs"]
mod lm_intercept_tests;

#[cfg(test)]
#[path = "char_to_raw_tests.rs"]
mod char_to_raw_tests;
#[cfg(test)]
#[path = "char_to_raw_warning_tests.rs"]
mod char_to_raw_warning_tests;
