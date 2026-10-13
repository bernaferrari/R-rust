use super::*;

#[test]
fn environment_dollar_forces_delayed_binding_once_and_matches_bracket() {
    let mut session = crate::sexp::session::RSession::new_with_path_policy(
        crate::mainutils::paths::RuntimePathPolicy::new(Vec::new(), "/tmp"),
    );
    let (result, output, _) = session.eval_code_with_output_capture(
            "e <- new.env(); n <- 0L; delayedAssign('f', { n <<- n + 1L; function() 42L }, assign.env=e); stopifnot(identical(e$f(),42L), identical(e[['f']],e$f), identical(n,1L)); delayedAssign('bad',stop('binding failed'),assign.env=e); stopifnot(identical(tryCatch(e$bad,error=conditionMessage),'binding failed'),is.null(e$absent)); TRUE",
        );
    assert_eq!(
        result
            .unwrap_or_else(|error| panic!("{}: {output:?}", error.message))
            .logical_elt(0),
        Some(1)
    );
}

#[test]
fn test_scalar_index_null() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        assert_eq!(scalarIndex(ptr::null_mut()), -1);
    }
}

#[test]
fn test_scalar_index_nil() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        assert_eq!(scalarIndex(R_NilValue()), -1);
    }
}

#[test]
fn test_extract_exact_arg_nil() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let result = ExtractExactArg(R_NilValue());
        assert_eq!(result, 1);
    }
}

#[test]
fn test_r_dispatch_or_eval_sp_no_dispatch() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let mut ans: SEXP = ptr::null_mut();
        let result = R_DispatchOrEvalSP(
            ptr::null_mut(),
            ptr::null_mut(),
            b"[\0".as_ptr() as *const c_char,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut ans,
        );
        assert_eq!(result, 0);
    }
}

#[test]
fn test_extract_arg_empty_list() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let result = ExtractArg(R_NilValue(), R_NilValue());
        assert_eq!(result, R_NilValue());
    }
}

#[test]
fn test_extract_drop_arg_nil() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let mut drop: c_int = 0;
        ExtractDropArg(R_NilValue(), &mut drop);
        assert_eq!(drop, 1); /* default when not found */
    }
}

#[test]
fn test_as_logical_nil() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        assert_eq!(asLogical(R_NilValue()), NA_LOGICAL);
    }
}

#[test]
fn test_nthcdr_null() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let result = nthcdr(R_NilValue(), 5);
        assert_eq!(result, R_NilValue());
    }
}

#[test]
fn test_pstrmatch_null_target() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let result = pstrmatch(R_NilValue(), R_NilValue(), 3);
        assert_eq!(result, pmatch::NO_MATCH);
    }
}

#[test]
fn test_type_checking_helpers() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        /* NILSXP */
        assert!(isNull(R_NilValue()));
        assert!(!isVector(R_NilValue()));
        assert!(!isPairList(R_NilValue()));
        assert!(!isVectorList(R_NilValue()));
        assert!(!isEnvironment(R_NilValue()));
        assert!(!isSymbol(R_NilValue()));
    }
}

#[test]
fn test_vector_subset_missing_arg() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        /* When x is a simple integer vector and s is missing, should duplicate */
        let x = Rf_allocVector(SEXPTYPE::INTSXP, 3);
        if !x.is_null() {
            for i in 0..3 {
                *INTEGER(x).add(i) = ((i + 1) * 10) as c_int;
            }
            /* Missing arg: create a symbol with empty name */
            let missing = Rf_install(c"".as_ptr());
            let result = VectorSubset(x, missing, R_NilValue());
            /* Should return a duplicate (same length) */
            assert!(!result.is_null());
            assert_eq!(LENGTH(result), LENGTH(x));
        }
    }
}

#[test]
fn test_extract_subset_null() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let result = ExtractSubset(R_NilValue(), R_NilValue(), R_NilValue());
        assert_eq!(result, R_NilValue());
    }
}

#[test]
fn extract_subset_uses_na_string_for_missing_character_elements() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let values = Rf_allocVector(SEXPTYPE::STRSXP, 1);
        SET_STRING_ELT(values, 0, Rf_mkChar(c"present".as_ptr()));
        let indices = Rf_allocVector(SEXPTYPE::INTSXP, 1);
        *INTEGER(indices) = 2;

        let result = ExtractSubset(values, indices, R_NilValue());

        assert_eq!(STRING_ELT(result, 0), crate::sexp::globals::R_NaString());
    }
}

#[test]
fn test_find_a_sub_index() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        /* Simple 2D case: subs = [[1,2], [3,4]], indx = [0, 0], offset = [1, 3] */
        let sub0: Vec<c_int> = vec![1, 2];
        let sub1: Vec<c_int> = vec![3, 4];
        let subs: Vec<*const c_int> = vec![sub0.as_ptr(), sub1.as_ptr()];
        let indx: Vec<c_int> = vec![0, 0];
        let offset: Vec<R_xlen_t> = vec![1, 3];
        let result = findASubIndex(
            2,
            subs.as_ptr(),
            indx.as_ptr(),
            ptr::null(),
            offset.as_ptr(),
            R_NilValue(),
        );
        /* (1-1)*1 + (3-1)*3 = 0 + 6 = 6 */
        assert_eq!(result, 6);
    }
}

#[test]
fn test_find_a_sub_index_na() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let sub0: Vec<c_int> = vec![1, NA_INTEGER];
        let sub1: Vec<c_int> = vec![3, 4];
        let subs: Vec<*const c_int> = vec![sub0.as_ptr(), sub1.as_ptr()];
        let indx: Vec<c_int> = vec![1, 0]; /* index 1 into sub0 = NA_INTEGER */
        let offset: Vec<R_xlen_t> = vec![1, 3];
        let result = findASubIndex(
            2,
            subs.as_ptr(),
            indx.as_ptr(),
            ptr::null(),
            offset.as_ptr(),
            R_NilValue(),
        );
        assert_eq!(result, NA_INTEGER as R_xlen_t);
    }
}

#[test]
fn test_r_finite() {
    let _session = crate::sexp::session::RSession::new();
    assert!(R_FINITE(1.0));
    assert!(R_FINITE(-1.0));
    assert!(R_FINITE(0.0));
    assert!(!R_FINITE(f64::INFINITY));
    assert!(!R_FINITE(f64::NEG_INFINITY));
    assert!(!R_FINITE(f64::NAN));
}
