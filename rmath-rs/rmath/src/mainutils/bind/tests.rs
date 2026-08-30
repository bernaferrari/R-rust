use super::*;
use crate::sexp::protect::{R_ProtectCount, protect_n};
use crate::sexp::session::RSession;

fn reset_protect_stack() {
    let n = R_ProtectCount();
    if n > 0 {
        drop(protect_n(n));
    }
}

struct ProtectStackGuard {
    _session: RSession,
}

impl ProtectStackGuard {
    fn new() -> Self {
        let session = RSession::new();
        reset_protect_stack();
        Self { _session: session }
    }
}

impl Drop for ProtectStackGuard {
    fn drop(&mut self) {
        reset_protect_stack();
    }
}

#[test]
fn test_bind_data_size() {
    let size = std::mem::size_of::<BindData>();
    assert!(size > 0);
    assert!(size >= std::mem::size_of::<c_int>() * 5);
}

#[test]
fn test_name_data_size() {
    let size = std::mem::size_of::<NameData>();
    assert!(size > 0);
    assert!(size >= std::mem::size_of::<c_int>() * 2);
}

#[test]
fn test_imax2() {
    assert_eq!(imax2(3, 5), 5);
    assert_eq!(imax2(7, 2), 7);
    assert_eq!(imax2(0, 0), 0);
    assert_eq!(imax2(-1, 1), 1);
}

#[test]
fn test_type2char_basic() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let s = std::ffi::CStr::from_ptr(type2char(0));
        assert_eq!(s.to_str().unwrap_or(""), "NULL");

        let s = std::ffi::CStr::from_ptr(type2char(10));
        assert_eq!(s.to_str().unwrap_or(""), "logical");

        let s = std::ffi::CStr::from_ptr(type2char(13));
        assert_eq!(s.to_str().unwrap_or(""), "integer");

        let s = std::ffi::CStr::from_ptr(type2char(14));
        assert_eq!(s.to_str().unwrap_or(""), "double");

        let s = std::ffi::CStr::from_ptr(type2char(16));
        assert_eq!(s.to_str().unwrap_or(""), "character");

        let s = std::ffi::CStr::from_ptr(type2char(19));
        assert_eq!(s.to_str().unwrap_or(""), "list");

        let s = std::ffi::CStr::from_ptr(type2char(24));
        assert_eq!(s.to_str().unwrap_or(""), "raw");
    }
}

#[test]
fn test_blank_string_is_session_local_on_same_thread() {
    let mut left = RSession::new();
    let mut right = RSession::new();

    let mut left_blank = ptr::null_mut();
    left.with_arena(|_| unsafe {
        left_blank = R_BlankString();
        assert!(!left_blank.is_null());
        assert_eq!(R_BlankString(), left_blank);
    })
    .unwrap();

    right
        .with_arena(|_| unsafe {
            let right_blank = R_BlankString();
            assert!(!right_blank.is_null());
            assert_eq!(R_BlankString(), right_blank);
            assert_ne!(right_blank, left_blank);
        })
        .unwrap();

    left.with_arena(|_| unsafe {
        assert_eq!(R_BlankString(), left_blank);
    })
    .unwrap();
}

#[test]
fn test_ans_flags_to_mode() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        // Raw
        assert_eq!(ans_flags_to_mode(1), SEXPTYPE::RAWSXP);
        // Logical
        assert_eq!(ans_flags_to_mode(2), SEXPTYPE::LGLSXP);
        // Integer
        assert_eq!(ans_flags_to_mode(16), SEXPTYPE::INTSXP);
        // Double
        assert_eq!(ans_flags_to_mode(32), SEXPTYPE::REALSXP);
        // Complex
        assert_eq!(ans_flags_to_mode(64), SEXPTYPE::CPLXSXP);
        // Character
        assert_eq!(ans_flags_to_mode(128), SEXPTYPE::STRSXP);
        // List
        assert_eq!(ans_flags_to_mode(256), SEXPTYPE::VECSXP);
        // Expression
        assert_eq!(ans_flags_to_mode(512), SEXPTYPE::EXPRSXP);
        // No flags
        assert_eq!(ans_flags_to_mode(0), SEXPTYPE::NILSXP);
        // Combined: integer + double -> double wins
        assert_eq!(ans_flags_to_mode(16 | 32), SEXPTYPE::REALSXP);
        // Combined: logical + integer -> integer wins
        assert_eq!(ans_flags_to_mode(2 | 16), SEXPTYPE::INTSXP);
    }
}

#[test]
fn test_do_c_null_args() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        // c() with no args should return NULL
        let result = do_c(
            ptr::null_mut(),
            ptr::null_mut(),
            R_NilValue(),
            ptr::null_mut(),
        );
        assert_eq!(result, R_NilValue());
    }
}

#[test]
fn test_do_c_dflt_null_args() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let result = do_c_dflt(
            ptr::null_mut(),
            ptr::null_mut(),
            R_NilValue(),
            ptr::null_mut(),
        );
        assert_eq!(result, R_NilValue());
    }
}

#[test]
fn test_do_bind_null_args() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        // do_bind with just deparse.level and no data should return NULL
        // args = (deparse.level=0)
        let dl = Rf_ScalarInteger(0);
        let _dl_guard = protect(dl);
        let args = Rf_cons(dl, R_NilValue());
        let _args_guard = protect(args);
        let result = do_bind(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        assert_eq!(result, R_NilValue());
    }
}

#[test]
fn test_do_cbind_null_args() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let dl = Rf_ScalarInteger(0);
        let _dl_guard = protect(dl);
        let args = Rf_cons(dl, R_NilValue());
        let _args_guard = protect(args);
        let result = do_cbind(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        assert_eq!(result, R_NilValue());
    }
}

#[test]
fn test_do_rbind_null_args() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let dl = Rf_ScalarInteger(0);
        let _dl_guard = protect(dl);
        let args = Rf_cons(dl, R_NilValue());
        let _args_guard = protect(args);
        let result = do_rbind(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        assert_eq!(result, R_NilValue());
    }
}

#[test]
fn test_itemname_null() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        assert_eq!(ItemName(ptr::null_mut(), 0), R_NilValue());
        assert_eq!(ItemName(R_NilValue(), 0), R_NilValue());
    }
}

#[test]
fn test_has_names_null() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        assert_eq!(HasNames(ptr::null_mut()), 0);
        assert_eq!(HasNames(R_NilValue()), 0);
    }
}

#[test]
fn test_do_unlist_null_args() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let x = R_NilValue();
        let recurse = Rf_ScalarLogical(TRUE);
        let _recurse_guard = protect(recurse);
        let usenames = Rf_ScalarLogical(TRUE);
        let _usenames_guard = protect(usenames);
        let tail = Rf_cons(usenames, R_NilValue());
        let _tail_guard = protect(tail);
        let middle = Rf_cons(recurse, tail);
        let _middle_guard = protect(middle);
        let args = Rf_cons(x, middle);
        let _args_guard = protect(args);
        let result = do_unlist(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        // unlist(NULL) should return NULL
        assert_eq!(result, R_NilValue());
    }
}

#[test]
fn test_is_vector_types() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        assert_eq!(isVector(ptr::null_mut()), 0);
    }
}

#[test]
fn test_is_list_types() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        assert_eq!(isList(ptr::null_mut()), 0);
    }
}

#[test]
fn test_is_new_list_null() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        assert_eq!(isNewList(ptr::null_mut()), false);
        assert_eq!(isNewList(R_NilValue()), false);
    }
}

#[test]
fn test_is_symbol_null() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        assert_eq!(isSymbol(ptr::null_mut()), false);
        assert_eq!(isSymbol(R_NilValue()), false);
    }
}

#[test]
fn test_is_matrix_null() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        assert_eq!(isMatrix(ptr::null_mut()), false);
        assert_eq!(isMatrix(R_NilValue()), false);
    }
}

// ---- New tests for real logic ----

#[test]
fn test_resolve_promise_null() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        assert_eq!(resolve_promise(ptr::null_mut()), ptr::null_mut());
        assert_eq!(resolve_promise(R_NilValue()), R_NilValue());
    }
}

#[test]
fn test_r_list_compact_basic() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        // Build a list with some NULL entries: (1, NULL, 2, NULL, 3)
        let v1 = Rf_ScalarInteger(1);
        let _v1_guard = protect(v1);
        let v2 = Rf_ScalarInteger(2);
        let _v2_guard = protect(v2);
        let v3 = Rf_ScalarInteger(3);
        let _v3_guard = protect(v3);
        let cell3 = Rf_cons(v3, R_NilValue());
        let _cell3_guard = protect(cell3);
        let cell_null2 = Rf_cons(R_NilValue(), cell3);
        let _cell_null2_guard = protect(cell_null2);
        let cell2 = Rf_cons(v2, cell_null2);
        let _cell2_guard = protect(cell2);
        let cell_null1 = Rf_cons(R_NilValue(), cell2);
        let _cell_null1_guard = protect(cell_null1);
        let lst = Rf_cons(v1, cell_null1);
        let _lst_guard = protect(lst);

        // With keep_initial=true, leading NULLs are kept
        // But non-leading NULLs are removed
        let compacted = R_listCompact(lst, true);
        // Walk: 1 -> NULL -> 2 -> NULL -> 3
        // Non-leading removal: 1 -> 2 -> 3
        assert!(!compacted.is_null());
        assert_eq!(TYPEOF(CAR(compacted)), INTSXP_I);
        assert_eq!(*INTEGER(CAR(compacted)), 1);
        let second = CDR(compacted);
        assert_eq!(TYPEOF(CAR(second)), INTSXP_I);
        assert_eq!(*INTEGER(CAR(second)), 2);
        let third = CDR(second);
        assert_eq!(TYPEOF(CAR(third)), INTSXP_I);
        assert_eq!(*INTEGER(CAR(third)), 3);
        assert_eq!(CDR(third), R_NilValue());
    }
}

#[test]
fn test_r_list_compact_no_nulls() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        // List with no NULLs: (1, 2, 3)
        let v1 = Rf_ScalarInteger(1);
        let _v1_guard = protect(v1);
        let v2 = Rf_ScalarInteger(2);
        let _v2_guard = protect(v2);
        let v3 = Rf_ScalarInteger(3);
        let _v3_guard = protect(v3);
        let tail2 = Rf_cons(v3, R_NilValue());
        let _tail2_guard = protect(tail2);
        let tail1 = Rf_cons(v2, tail2);
        let _tail1_guard = protect(tail1);
        let lst = Rf_cons(v1, tail1);
        let _lst_guard = protect(lst);

        let compacted = R_listCompact(lst, true);
        assert!(!compacted.is_null());
        assert_eq!(*INTEGER(CAR(compacted)), 1);
        assert_eq!(*INTEGER(CAR(CDR(compacted))), 2);
        assert_eq!(*INTEGER(CAR(CDR(CDR(compacted)))), 3);
        assert_eq!(CDR(CDR(CDR(compacted))), R_NilValue());
    }
}

#[test]
fn test_r_list_compact_all_nulls() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let tail = Rf_cons(R_NilValue(), R_NilValue());
        let _tail_guard = protect(tail);
        let lst = Rf_cons(R_NilValue(), tail);
        let _lst_guard = protect(lst);
        let compacted = R_listCompact(lst, false);
        // With keep_initial=false, all NULLs are removed -> R_NilValue
        assert_eq!(compacted, R_NilValue());
    }
}

#[test]
fn test_answertype_single_integer() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let v = Rf_ScalarInteger(42);
        let _v_guard = protect(v);
        let mut data = BindData {
            ans_flags: 0,
            ans_ptr: ptr::null_mut(),
            ans_length: 0,
            ans_names: ptr::null_mut(),
            ans_nnames: 0,
        };
        AnswerType(v, false, false, &mut data, ptr::null_mut());
        assert_eq!(data.ans_flags & 16, 16); // INTSXP flag
        assert_eq!(data.ans_length, 1);
    }
}

#[test]
fn test_answertype_single_real() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let v = Rf_ScalarReal(3.14);
        let _v_guard = protect(v);
        let mut data = BindData {
            ans_flags: 0,
            ans_ptr: ptr::null_mut(),
            ans_length: 0,
            ans_names: ptr::null_mut(),
            ans_nnames: 0,
        };
        AnswerType(v, false, false, &mut data, ptr::null_mut());
        assert_eq!(data.ans_flags & 32, 32); // REALSXP flag
        assert_eq!(data.ans_length, 1);
    }
}

#[test]
fn test_answertype_single_logical() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let v = Rf_ScalarLogical(TRUE);
        let _v_guard = protect(v);
        let mut data = BindData {
            ans_flags: 0,
            ans_ptr: ptr::null_mut(),
            ans_length: 0,
            ans_names: ptr::null_mut(),
            ans_nnames: 0,
        };
        AnswerType(v, false, false, &mut data, ptr::null_mut());
        assert_eq!(data.ans_flags & 2, 2); // LGLSXP flag
        assert_eq!(data.ans_length, 1);
    }
}

#[test]
fn test_answertype_null_dropped() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let mut data = BindData {
            ans_flags: 0,
            ans_ptr: ptr::null_mut(),
            ans_length: 0,
            ans_names: ptr::null_mut(),
            ans_nnames: 0,
        };
        AnswerType(R_NilValue(), false, false, &mut data, ptr::null_mut());
        assert_eq!(data.ans_flags, 0);
        assert_eq!(data.ans_length, 0);
    }
}

#[test]
fn test_answertype_combined_types() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let v_int = Rf_ScalarInteger(1);
        let _v_int_guard = protect(v_int);
        let v_real = Rf_ScalarReal(2.0);
        let _v_real_guard = protect(v_real);
        let mut data = BindData {
            ans_flags: 0,
            ans_ptr: ptr::null_mut(),
            ans_length: 0,
            ans_names: ptr::null_mut(),
            ans_nnames: 0,
        };
        AnswerType(v_int, false, false, &mut data, ptr::null_mut());
        AnswerType(v_real, false, false, &mut data, ptr::null_mut());
        // Both INTSXP (16) and REALSXP (32) flags set
        assert_eq!(data.ans_flags & 16, 16);
        assert_eq!(data.ans_flags & 32, 32);
        assert_eq!(data.ans_length, 2);
        // Mode should be REALSXP (higher priority)
        assert_eq!(ans_flags_to_mode(data.ans_flags), SEXPTYPE::REALSXP);
    }
}

#[test]
fn test_answertype_vector_length() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        // Create a length-3 integer vector
        let v = Rf_allocVector3(INTSXP_I, 3);
        let _v_guard = protect(v);
        for i in 0..3 {
            *INTEGER(v).add(i) = (i + 1) as c_int;
        }
        let mut data = BindData {
            ans_flags: 0,
            ans_ptr: ptr::null_mut(),
            ans_length: 0,
            ans_names: ptr::null_mut(),
            ans_nnames: 0,
        };
        AnswerType(v, false, false, &mut data, ptr::null_mut());
        assert_eq!(data.ans_flags & 16, 16);
        assert_eq!(data.ans_length, 3);
    }
}

#[test]
fn test_do_c_dflt_single_integer() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let v = Rf_ScalarInteger(42);
        let _v_guard = protect(v);
        let args = Rf_cons(v, R_NilValue());
        let _args_guard = protect(args);
        let result = do_c_dflt(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        assert!(!result.is_null());
        assert_eq!(TYPEOF(result), INTSXP_I);
        assert_eq!(XLENGTH(result), 1);
        assert_eq!(*INTEGER(result), 42);
    }
}

#[test]
fn test_do_c_dflt_two_integers() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let v1 = Rf_ScalarInteger(1);
        let _v1_guard = protect(v1);
        let v2 = Rf_ScalarInteger(2);
        let _v2_guard = protect(v2);
        let tail = Rf_cons(v2, R_NilValue());
        let _tail_guard = protect(tail);
        let args = Rf_cons(v1, tail);
        let _args_guard = protect(args);
        let result = do_c_dflt(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        assert_eq!(TYPEOF(result), INTSXP_I);
        assert_eq!(XLENGTH(result), 2);
        assert_eq!(*INTEGER(result), 1);
        assert_eq!(*INTEGER(result).add(1), 2);
    }
}

#[test]
fn test_do_c_dflt_int_and_real() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let v_int = Rf_ScalarInteger(1);
        let _v_int_guard = protect(v_int);
        let v_real = Rf_ScalarReal(2.5);
        let _v_real_guard = protect(v_real);
        let tail = Rf_cons(v_real, R_NilValue());
        let _tail_guard = protect(tail);
        let args = Rf_cons(v_int, tail);
        let _args_guard = protect(args);
        let result = do_c_dflt(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        // integer + real -> real (coercion)
        assert_eq!(TYPEOF(result), REALSXP_I);
        assert_eq!(XLENGTH(result), 2);
        assert_eq!(*REAL(result), 1.0);
        assert_eq!(*REAL(result).add(1), 2.5);
    }
}

#[test]
fn test_do_c_dflt_with_null() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let v1 = Rf_ScalarInteger(1);
        let _v1_guard = protect(v1);
        let v_null = R_NilValue();
        let v2 = Rf_ScalarInteger(3);
        let _v2_guard = protect(v2);
        // (1, NULL, 3) -> NULLs are dropped -> c(1, 3)
        let tail2 = Rf_cons(v2, R_NilValue());
        let _tail2_guard = protect(tail2);
        let tail1 = Rf_cons(v_null, tail2);
        let _tail1_guard = protect(tail1);
        let args = Rf_cons(v1, tail1);
        let _args_guard = protect(args);
        let result = do_c_dflt(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        assert_eq!(TYPEOF(result), INTSXP_I);
        assert_eq!(XLENGTH(result), 2);
        assert_eq!(*INTEGER(result), 1);
        assert_eq!(*INTEGER(result).add(1), 3);
    }
}

#[test]
fn test_do_c_dflt_logical_vector() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let v1 = Rf_ScalarLogical(TRUE);
        let _v1_guard = protect(v1);
        let v2 = Rf_ScalarLogical(FALSE);
        let _v2_guard = protect(v2);
        let tail = Rf_cons(v2, R_NilValue());
        let _tail_guard = protect(tail);
        let args = Rf_cons(v1, tail);
        let _args_guard = protect(args);
        let result = do_c_dflt(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        assert_eq!(TYPEOF(result), LGLSXP_I);
        assert_eq!(XLENGTH(result), 2);
        assert_eq!(*LOGICAL(result), TRUE);
        assert_eq!(*LOGICAL(result).add(1), FALSE);
    }
}

#[test]
fn test_do_c_dflt_real_vector() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let v1 = Rf_ScalarReal(1.5);
        let _v1_guard = protect(v1);
        let v2 = Rf_ScalarReal(2.5);
        let _v2_guard = protect(v2);
        let v3 = Rf_ScalarReal(3.5);
        let _v3_guard = protect(v3);
        let tail2 = Rf_cons(v3, R_NilValue());
        let _tail2_guard = protect(tail2);
        let tail1 = Rf_cons(v2, tail2);
        let _tail1_guard = protect(tail1);
        let args = Rf_cons(v1, tail1);
        let _args_guard = protect(args);
        let result = do_c_dflt(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        assert_eq!(TYPEOF(result), REALSXP_I);
        assert_eq!(XLENGTH(result), 3);
    }
}

#[test]
fn test_do_c_dflt_integer_vector() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        // Create a length-2 integer vector
        let v = Rf_allocVector3(INTSXP_I, 2);
        let _v_guard = protect(v);
        *INTEGER(v) = 10;
        *INTEGER(v).add(1) = 20;
        let args = Rf_cons(v, R_NilValue());
        let _args_guard = protect(args);
        let result = do_c_dflt(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        assert_eq!(TYPEOF(result), INTSXP_I);
        assert_eq!(XLENGTH(result), 2);
        assert_eq!(*INTEGER(result), 10);
        assert_eq!(*INTEGER(result).add(1), 20);
    }
}

#[test]
fn test_do_c_dflt_all_nulls() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        // c(NULL, NULL) should return NULL
        let tail = Rf_cons(R_NilValue(), R_NilValue());
        let _tail_guard = protect(tail);
        let args = Rf_cons(R_NilValue(), tail);
        let _args_guard = protect(args);
        let result = do_c_dflt(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        // All NULLs -> ans_flags=0, ans_length=0 -> NILSXP mode
        assert_eq!(result, R_NilValue());
    }
}

#[test]
fn test_do_c_dflt_logical_and_integer() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let v_lgl = Rf_ScalarLogical(TRUE);
        let _v_lgl_guard = protect(v_lgl);
        let v_int = Rf_ScalarInteger(42);
        let _v_int_guard = protect(v_int);
        let tail = Rf_cons(v_int, R_NilValue());
        let _tail_guard = protect(tail);
        let args = Rf_cons(v_lgl, tail);
        let _args_guard = protect(args);
        let result = do_c_dflt(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
        // logical + integer -> integer (coercion)
        assert_eq!(TYPEOF(result), INTSXP_I);
        assert_eq!(XLENGTH(result), 2);
        assert_eq!(*INTEGER(result), TRUE);
        assert_eq!(*INTEGER(result).add(1), 42);
    }
}

#[test]
fn test_integer_answer_from_logical() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let src = Rf_allocVector3(LGLSXP_I, 3);
        let _src_guard = protect(src);
        *LOGICAL(src) = TRUE;
        *LOGICAL(src).add(1) = FALSE;
        *LOGICAL(src).add(2) = NA_LOGICAL;

        let dest = Rf_allocVector3(INTSXP_I, 3);
        let _dest_guard = protect(dest);
        let mut data = BindData {
            ans_flags: 0,
            ans_ptr: dest,
            ans_length: 0,
            ans_names: ptr::null_mut(),
            ans_nnames: 0,
        };
        IntegerAnswer(src, &mut data, ptr::null_mut());
        assert_eq!(data.ans_length, 3);
        assert_eq!(*INTEGER(dest), TRUE);
        assert_eq!(*INTEGER(dest).add(1), FALSE);
        assert_eq!(*INTEGER(dest).add(2), NA_LOGICAL);
    }
}

#[test]
fn test_real_answer_from_integer() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let src = Rf_allocVector3(INTSXP_I, 3);
        let _src_guard = protect(src);
        *INTEGER(src) = 1;
        *INTEGER(src).add(1) = NA_INTEGER;
        *INTEGER(src).add(2) = -5;

        let dest = Rf_allocVector3(REALSXP_I, 3);
        let _dest_guard = protect(dest);
        let mut data = BindData {
            ans_flags: 0,
            ans_ptr: dest,
            ans_length: 0,
            ans_names: ptr::null_mut(),
            ans_nnames: 0,
        };
        RealAnswer(src, &mut data, ptr::null_mut());
        assert_eq!(data.ans_length, 3);
        assert_eq!(*REAL(dest), 1.0);
        // NA_INTEGER -> NA_REAL
        assert!((*REAL(dest).add(1)).is_nan());
        assert_eq!(*REAL(dest).add(2), -5.0);
    }
}

#[test]
fn test_logical_answer_from_integer() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let src = Rf_allocVector3(INTSXP_I, 3);
        let _src_guard = protect(src);
        *INTEGER(src) = 1;
        *INTEGER(src).add(1) = 0;
        *INTEGER(src).add(2) = NA_INTEGER;

        let dest = Rf_allocVector3(LGLSXP_I, 3);
        let _dest_guard = protect(dest);
        let mut data = BindData {
            ans_flags: 0,
            ans_ptr: dest,
            ans_length: 0,
            ans_names: ptr::null_mut(),
            ans_nnames: 0,
        };
        LogicalAnswer(src, &mut data, ptr::null_mut());
        assert_eq!(data.ans_length, 3);
        assert_eq!(*LOGICAL(dest), TRUE);
        assert_eq!(*LOGICAL(dest).add(1), FALSE);
        assert_eq!(*LOGICAL(dest).add(2), NA_LOGICAL);
    }
}

#[test]
fn test_complex_answer_from_real() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let src = Rf_allocVector3(REALSXP_I, 2);
        let _src_guard = protect(src);
        *REAL(src) = 1.0;
        *REAL(src).add(1) = 2.0;

        let dest = Rf_allocVector3(CPLXSXP_I, 2);
        let _dest_guard = protect(dest);
        let mut data = BindData {
            ans_flags: 0,
            ans_ptr: dest,
            ans_length: 0,
            ans_names: ptr::null_mut(),
            ans_nnames: 0,
        };
        ComplexAnswer(src, &mut data, ptr::null_mut());
        assert_eq!(data.ans_length, 2);
        assert_eq!((*COMPLEX(dest)).r, 1.0);
        assert_eq!((*COMPLEX(dest)).i, 0.0);
        assert_eq!((*COMPLEX(dest).add(1)).r, 2.0);
        assert_eq!((*COMPLEX(dest).add(1)).i, 0.0);
    }
}

#[test]
fn test_coerce_vector_lgl_to_int() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let src = Rf_allocVector3(LGLSXP_I, 2);
        let _src_guard = protect(src);
        *LOGICAL(src) = TRUE;
        *LOGICAL(src).add(1) = FALSE;

        let dest = coerceVector(src, SEXPTYPE::INTSXP);
        assert_eq!(TYPEOF(dest), INTSXP_I);
        assert_eq!(*INTEGER(dest), TRUE);
        assert_eq!(*INTEGER(dest).add(1), FALSE);
    }
}

#[test]
fn test_coerce_vector_int_to_real() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let src = Rf_allocVector3(INTSXP_I, 2);
        let _src_guard = protect(src);
        *INTEGER(src) = 42;
        *INTEGER(src).add(1) = NA_INTEGER;

        let dest = coerceVector(src, SEXPTYPE::REALSXP);
        assert_eq!(TYPEOF(dest), REALSXP_I);
        assert_eq!(*REAL(dest), 42.0);
        assert!((*REAL(dest).add(1)).is_nan()); // NA -> NaN
    }
}

#[test]
fn test_coerce_vector_same_type() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let src = Rf_allocVector3(INTSXP_I, 2);
        let _src_guard = protect(src);
        *INTEGER(src) = 1;
        *INTEGER(src).add(1) = 2;

        let dest = coerceVector(src, SEXPTYPE::INTSXP);
        // Should return the same pointer (no copy needed)
        assert_eq!(dest, src);
    }
}

#[test]
fn test_coerce_vector_raw_to_int() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let src = Rf_allocVector3(RAWSXP_I, 3);
        let _src_guard = protect(src);
        *RAW(src) = 10;
        *RAW(src).add(1) = 20;
        *RAW(src).add(2) = 255;

        let dest = coerceVector(src, SEXPTYPE::INTSXP);
        assert_eq!(TYPEOF(dest), INTSXP_I);
        assert_eq!(*INTEGER(dest), 10);
        assert_eq!(*INTEGER(dest).add(1), 20);
        assert_eq!(*INTEGER(dest).add(2), 255);
    }
}

#[test]
fn test_coerce_vector_raw_to_real() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let src = Rf_allocVector3(RAWSXP_I, 2);
        let _src_guard = protect(src);
        *RAW(src) = 0;
        *RAW(src).add(1) = 200;

        let dest = coerceVector(src, SEXPTYPE::REALSXP);
        assert_eq!(TYPEOF(dest), REALSXP_I);
        assert_eq!(*REAL(dest), 0.0);
        assert_eq!(*REAL(dest).add(1), 200.0);
    }
}

#[test]
fn test_coerce_vector_raw_to_complex() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let src = Rf_allocVector3(RAWSXP_I, 2);
        let _src_guard = protect(src);
        *RAW(src) = 42;
        *RAW(src).add(1) = 100;

        let dest = coerceVector(src, SEXPTYPE::CPLXSXP);
        assert_eq!(TYPEOF(dest), CPLXSXP_I);
        assert_eq!((*COMPLEX(dest)).r, 42.0);
        assert_eq!((*COMPLEX(dest)).i, 0.0);
        assert_eq!((*COMPLEX(dest).add(1)).r, 100.0);
        assert_eq!((*COMPLEX(dest).add(1)).i, 0.0);
    }
}

#[test]
fn test_coerce_vector_int_to_complex() {
    unsafe {
        let _guard = ProtectStackGuard::new();
        let src = Rf_allocVector3(INTSXP_I, 2);
        let _src_guard = protect(src);
        *INTEGER(src) = 3;
        *INTEGER(src).add(1) = NA_INTEGER;

        let dest = coerceVector(src, SEXPTYPE::CPLXSXP);
        assert_eq!(TYPEOF(dest), CPLXSXP_I);
        assert_eq!((*COMPLEX(dest)).r, 3.0);
        assert_eq!((*COMPLEX(dest)).i, 0.0);
        assert!((*COMPLEX(dest).add(1)).r.is_nan()); // NA -> NaN
        assert_eq!((*COMPLEX(dest).add(1)).i, 0.0);
    }
}
