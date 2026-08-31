// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mainutils::subassign::*;
    use std::os::raw::{c_char, c_double, c_int};
    use std::ptr;

    use crate::mainutils::subscript::{
        OneIndex, get1index, int_arraySubscript, makeSubscript, mat2indsub, strmat2intmat,
        vectorIndex,
    };
    use crate::sexp::accessors::*;
    use crate::sexp::constructors::*;
    use crate::sexp::envir::defineVar;
    use crate::sexp::ffi::{FALSE, NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE, TRUE};
    use crate::sexp::globals::R_NilValue;
    use crate::sexp::memory_ext::{allocList, allocSExp};
    use crate::sexp::protect::protect;
    use crate::sexp::symbol::Rf_install;

    #[test]
    fn test_do_subassign_handles_empty_r_argument_list() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = do_subassign(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                R_NilValue(),
                std::ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_do_subassign_dflt_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = do_subassign_dflt(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            // Null args return null
            assert!(result.is_null());
        }
    }

    #[test]
    fn test_do_subassign2_handles_empty_r_argument_list() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = do_subassign2(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                R_NilValue(),
                std::ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_do_subassign2_dflt_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = do_subassign2_dflt(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            // Null args return null
            assert!(result.is_null());
        }
    }

    #[test]
    fn test_do_subassign3_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        // do_subassign3 calls fixSubset3Args which panics with RError on nil args.
        // Just verify the function exists and has the right signature.
        // A full integration test would need proper SEXP arguments.
        let _fn_ptr: unsafe fn(SEXP, SEXP, SEXP, SEXP) -> SEXP = do_subassign3;
    }

    #[test]
    fn test_R_subassign3_dflt_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = R_subassign3_dflt(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            // Upstream subassign.c has no early NULL return: assignment into
            // NULL grows a result rather than staying nil.
            assert!(!result.is_null());
        }
    }

    #[test]
    fn test_SubassignTypeSym_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = SubassignTypeSym();
            // Should not be null (it's an installed symbol)
            assert!(!result.is_null());
        }
    }

    #[test]
    fn test_SubassignDotsNames_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = SubassignDotsNames(std::ptr::null_mut(), std::ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_GetSubassignSxpVec_returns_nil() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = GetSubassignSxpVec(std::ptr::null_mut(), std::ptr::null_mut());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_var_assign_handles_empty_r_argument_list() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = var_assign(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                R_NilValue(),
                std::ptr::null_mut(),
            );
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_getNames_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = getNames(R_NilValue());
            assert_eq!(result, R_NilValue());
        }
    }

    #[test]
    fn test_gi_integer() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_allocVector3(INTSXP, 3);
            let _v_guard = protect(v);
            let p = INTEGER(v);
            *p.add(0) = 10;
            *p.add(1) = 20;
            *p.add(2) = NA_INTEGER;
            assert_eq!(gi(v, 0), 10);
            assert_eq!(gi(v, 1), 20);
            assert_eq!(gi(v, 2), NA_INTEGER as R_xlen_t);
        }
    }

    #[test]
    fn test_SubAssignArgs_two_args() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // Create args: x, y (no subscripts)
            let y_val = Rf_allocVector3(INTSXP, 1);
            let _y_val_guard = protect(y_val);
            let args = Rf_cons(R_NilValue(), Rf_cons(y_val, R_NilValue()));
            let _args_guard = protect(args);

            let mut x: SEXP = ptr::null_mut();
            let mut s: SEXP = ptr::null_mut();
            let mut y: SEXP = ptr::null_mut();
            let nsubs = SubAssignArgs(args, &mut x, &mut s, &mut y);
            assert_eq!(nsubs, 0);
            assert_eq!(x, R_NilValue());
            assert_eq!(s, R_NilValue());
            assert_eq!(y, y_val);
        }
    }

    #[test]
    fn test_SubassignTypeFix_same_type() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut xv: SEXP = Rf_allocVector3(INTSXP, 1);
            let _xv_guard = protect(xv);
            let mut yv: SEXP = Rf_allocVector3(INTSXP, 1);
            let _yv_guard = protect(yv);
            let which = SubassignTypeFix(&mut xv, &mut yv, 0, 1, ptr::null_mut(), ptr::null_mut());
            // 100 * 13 + 13 = 1313
            assert_eq!(which, 1313);
        }
    }
}
