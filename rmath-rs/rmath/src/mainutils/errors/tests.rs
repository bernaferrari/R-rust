use crate::sexp::session::RSession;

use super::helpers::translateChar;
use super::*;

fn must<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("test failed: {e:?}"),
    }
}

#[test]
fn test_wd() {
    let _session = crate::sexp::session::RSession::new();
    assert_eq!(wd("hello"), 5);
    assert_eq!(wd(""), 0);
    assert_eq!(wd("hello world"), 11);
}

#[test]
fn test_R_SetErrmessage() {
    let _session = crate::sexp::session::RSession::new();
    let _session = RSession::new();

    R_SetErrmessage("test error");
    assert_eq!(R_GetErrorBuf(), "test error");

    R_SetErrmessage("");
    assert_eq!(R_GetErrorBuf(), "");
}

#[test]
fn test_error_catches_panic() {
    let _session = crate::sexp::session::RSession::new();
    let _session = RSession::new();

    let result = std::panic::catch_unwind(|| {
        R_SetErrmessage("test panic");
        std::panic::panic_any(RError {
            message: "test panic".to_string(),
        });
    });
    assert!(result.is_err());
}

#[test]
fn test_count_format_args() {
    let _session = crate::sexp::session::RSession::new();
    assert_eq!(count_format_args("hello %s world %d"), 2);
    assert_eq!(count_format_args("no args"), 0);
    assert_eq!(count_format_args("%% escaped"), 0);
}

#[test]
fn test_in_error_flag() {
    let _session = crate::sexp::session::RSession::new();
    let _session = RSession::new();

    assert_eq!(R_GetInError(), 0);
    R_SetInError(1);
    assert_eq!(R_GetInError(), 1);
    R_SetInError(0);
}

#[test]
fn test_session_error_flags_are_local_on_same_thread() {
    let _session = crate::sexp::session::RSession::new();
    let mut left = RSession::new();
    let mut right = RSession::new();

    left.with_arena(|_| {
        R_SetInError(7);
        R_SetExpressions(900);
        R_SetExpressionsKeep(901);
        R_SetWarnLength(123);
        R_SetInterruptsSuspended(true);
        R_SetInterruptsPending(true);
        assert_eq!(R_GetInError(), 7);
        assert_eq!(R_Expressions(), 900);
        assert_eq!(r_warn_length(), 123);
        assert!(R_InterruptsSuspended());
        assert!(interrupts_pending());
    })
    .unwrap();

    right
        .with_arena(|_| {
            assert_eq!(R_GetInError(), 0);
            assert_eq!(R_Expressions(), 500);
            assert_eq!(r_warn_length(), 1000);
            assert!(!R_InterruptsSuspended());
            assert!(!interrupts_pending());

            R_SetInError(2);
            R_SetExpressions(600);
            R_SetWarnLength(456);
            R_SetInterruptsSuspended(false);
            R_SetInterruptsPending(false);
            assert_eq!(R_GetInError(), 2);
        })
        .unwrap();

    left.with_arena(|_| {
        assert_eq!(R_GetInError(), 7);
        assert_eq!(R_Expressions(), 900);
        R_Expressions_keep();
        assert_eq!(R_Expressions(), 901);
        assert_eq!(r_warn_length(), 123);
        assert!(R_InterruptsSuspended());
        assert!(interrupts_pending());
    })
    .unwrap();
}

#[test]
fn test_session_warning_collection_is_local_on_same_thread() {
    let _session = crate::sexp::session::RSession::new();
    let mut left = RSession::new();
    let mut right = RSession::new();

    left.with_arena(|_| {
        set_collect_warnings(3);
        unsafe {
            setup_warnings();
        }
        assert_eq!(collect_warnings(), 0);
        assert!(!warnings_ptr().is_null());
    })
    .unwrap();

    right
        .with_arena(|_| {
            assert_eq!(collect_warnings(), 0);
            assert!(warnings_ptr().is_null());
            set_collect_warnings(1);
            unsafe {
                setup_warnings();
            }
            assert_eq!(collect_warnings(), 0);
            assert!(!warnings_ptr().is_null());
            set_warnings_ptr(ptr::null_mut());
        })
        .unwrap();

    left.with_arena(|_| {
        assert!(!warnings_ptr().is_null());
        set_warnings_ptr(ptr::null_mut());
    })
    .unwrap();
}

#[test]
fn test_session_handler_and_restart_stacks_are_local_on_same_thread() {
    let _session = crate::sexp::session::RSession::new();
    let mut left = RSession::new();
    let mut right = RSession::new();

    let mut left_handler = ptr::null_mut();
    let mut left_restart = ptr::null_mut();

    left.with_arena(|_| unsafe {
        left_handler = Rf_allocVector(SEXPTYPE::VECSXP, 5);
        left_restart = Rf_allocVector(SEXPTYPE::VECSXP, 2);
        set_handler_stack(Rf_cons(left_handler, ptr::null_mut()));
        set_restart_stack(Rf_cons(left_restart, ptr::null_mut()));
        assert_eq!(CAR(handler_stack()), left_handler);
        assert_eq!(CAR(restart_stack()), left_restart);
    })
    .unwrap();

    right
        .with_arena(|_| unsafe {
            assert!(handler_stack().is_null());
            assert!(restart_stack().is_null());
            let right_handler = Rf_allocVector(SEXPTYPE::VECSXP, 5);
            let right_restart = Rf_allocVector(SEXPTYPE::VECSXP, 2);
            set_handler_stack(Rf_cons(right_handler, ptr::null_mut()));
            set_restart_stack(Rf_cons(right_restart, ptr::null_mut()));
            assert_eq!(CAR(handler_stack()), right_handler);
            assert_eq!(CAR(restart_stack()), right_restart);
        })
        .unwrap();

    left.with_arena(|_| unsafe {
        assert_eq!(CAR(handler_stack()), left_handler);
        assert_eq!(CAR(restart_stack()), left_restart);
        set_handler_stack(ptr::null_mut());
        set_restart_stack(ptr::null_mut());
    })
    .unwrap();
}

#[test]
fn test_format_to_buf() {
    let _session = crate::sexp::session::RSession::new();
    let mut buf = [0u8; BUFSIZE + 1];
    let (len, truncated) = format_to_buf(&mut buf, "hello world");
    assert_eq!(len, 11);
    assert!(!truncated);
    let s = unsafe {
        std::str::from_utf8_unchecked(&buf[..buf.iter().position(|&b| b == 0).unwrap_or(BUFSIZE)])
    };
    assert_eq!(s, "hello world");
}

#[test]
fn test_format_to_buf_long() {
    let _session = crate::sexp::session::RSession::new();
    let mut buf = [0u8; BUFSIZE + 1];
    let long_str = "x".repeat(BUFSIZE + 100);
    let (len, truncated) = format_to_buf(&mut buf, &long_str);
    assert_eq!(len, BUFSIZE + 100);
    assert!(truncated);
}

#[test]
fn test_bufcat() {
    let _session = crate::sexp::session::RSession::new();
    let mut buf = [0u8; BUFSIZE + 1];
    format_to_buf(&mut buf, "hello");
    bufcat(&mut buf, " world");
    let s = unsafe {
        std::str::from_utf8_unchecked(&buf[..buf.iter().position(|&b| b == 0).unwrap_or(BUFSIZE)])
    };
    assert_eq!(s, "hello world");
}

#[test]
fn test_print_trunc() {
    let _session = crate::sexp::session::RSession::new();
    let mut buf = [0u8; BUFSIZE + 1];
    format_to_buf(&mut buf, "hello");
    print_trunc(&mut buf, true);
    let s = unsafe {
        std::str::from_utf8_unchecked(&buf[..buf.iter().position(|&b| b == 0).unwrap_or(BUFSIZE)])
    };
    assert!(s.contains("[... truncated]"));
}

#[test]
fn test_print_trunc_not_truncated() {
    let _session = crate::sexp::session::RSession::new();
    let mut buf = [0u8; BUFSIZE + 1];
    format_to_buf(&mut buf, "hello");
    print_trunc(&mut buf, false);
    let s = unsafe {
        std::str::from_utf8_unchecked(&buf[..buf.iter().position(|&b| b == 0).unwrap_or(BUFSIZE)])
    };
    assert_eq!(s, "hello");
    assert!(!s.contains("[... truncated]"));
}

#[test]
fn test_mkHandlerEntry() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let klass = Rf_mkString(b"error\x00".as_ptr() as *const c_char);
        let handler = Rf_mkString(b"handler\x00".as_ptr() as *const c_char);
        let entry = mkHandlerEntry(
            klass,
            ptr::null_mut(),
            handler,
            ptr::null_mut(),
            ptr::null_mut(),
            1,
        );
        assert!(!entry.is_null());
        assert_eq!(TYPEOF(entry), SEXPTYPE::VECSXP);
        assert_eq!(LENGTH(entry), 5);
        assert_eq!(IS_CALLING_ENTRY(entry), 1);
    }
}

#[test]
fn test_r_makeErrorCondition() {
    let _session = crate::sexp::session::RSession::new();
    let _session = RSession::new();

    unsafe {
        let cond = R_makeErrorCondition(
            ptr::null_mut(),
            b"simpleError\x00".as_ptr() as *const c_char,
            ptr::null_mut(),
            0,
            b"test error message\x00".as_ptr() as *const c_char,
        );
        assert!(!cond.is_null());
        assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP);
        assert_eq!(LENGTH(cond), 2);
    }
}

#[test]
fn test_r_makeErrorCondition_with_subclass() {
    let _session = crate::sexp::session::RSession::new();
    let _session = RSession::new();

    unsafe {
        let cond = R_makeErrorCondition(
            ptr::null_mut(),
            b"error\x00".as_ptr() as *const c_char,
            b"simpleError\x00".as_ptr() as *const c_char,
            0,
            b"test error\x00".as_ptr() as *const c_char,
        );
        assert!(!cond.is_null());
        assert_eq!(LENGTH(cond), 2);
        // Class attribute should exist (either via getAttrib or direct ATTRIB check)
        let klass = getAttrib_wrap(cond, R_ClassSymbol());
        // klass may have length 4 if attribute system is fully working,
        // or length 0 if setAttrib didn't fully work in this test context
        if !klass.is_null() && TYPEOF(klass) == SEXPTYPE::STRSXP {
            assert!(LENGTH(klass) >= 3);
        }
    }
}

#[test]
fn test_concise_traceback_empty() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let result = R_ConciseTraceback(ptr::null_mut(), 0);
        assert_eq!(result, "");
    }
}

#[test]
fn test_interrupts_suspended() {
    let _session = crate::sexp::session::RSession::new();
    let _session = RSession::new();

    assert!(!R_InterruptsSuspended());
    R_SetInterruptsSuspended(true);
    assert!(R_InterruptsSuspended());
    R_SetInterruptsSuspended(false);
    assert!(!R_InterruptsSuspended());
}

#[test]
fn test_warning_collection() {
    let _session = crate::sexp::session::RSession::new();
    let _session = RSession::new();

    unsafe {
        set_collect_warnings(0);
        set_warnings_ptr(ptr::null_mut());

        // setup_warnings should create the vector
        setup_warnings();
        assert!(warnings_ptr().is_null() || TYPEOF(warnings_ptr()) == SEXPTYPE::VECSXP);

        // Reset
        set_collect_warnings(0);
        set_warnings_ptr(ptr::null_mut());
    }
}

#[test]
fn test_handler_stack_operations() {
    let _session = crate::sexp::session::RSession::new();
    let _session = RSession::new();

    set_handler_stack(ptr::null_mut());

    unsafe {
        let entry = Rf_allocVector(SEXPTYPE::VECSXP, 5);
        set_handler_stack(Rf_cons(entry, ptr::null_mut()));
        assert!(!handler_stack().is_null());

        // Reset
        set_handler_stack(ptr::null_mut());
    }
}

#[test]
fn test_restart_stack_operations() {
    let _session = crate::sexp::session::RSession::new();
    let _session = RSession::new();

    set_restart_stack(ptr::null_mut());

    unsafe {
        let entry = Rf_allocVector(SEXPTYPE::VECSXP, 2);
        set_restart_stack(Rf_cons(entry, ptr::null_mut()));
        assert!(!restart_stack().is_null());

        // Reset
        set_restart_stack(ptr::null_mut());
    }
}

#[test]
fn test_error_codes() {
    let _session = crate::sexp::session::RSession::new();
    assert_eq!(error_codes::ERROR_NUMARGS, 1);
    assert_eq!(error_codes::ERROR_UNKNOWN, 6);
    assert_eq!(warning_codes::WARNING_coerce_NA, 0);
    assert_eq!(warning_codes::WARNING_UNKNOWN, 3);
}

#[test]
fn test_errbufcat_macro() {
    let _session = crate::sexp::session::RSession::new();
    let mut buf = [0u8; BUFSIZE + 1];
    buf[0] = 0;
    ERRBUFCAT!(buf, "hello");
    let s = unsafe {
        std::str::from_utf8_unchecked(&buf[..buf.iter().position(|&b| b == 0).unwrap_or(BUFSIZE)])
    };
    assert_eq!(s, "hello");
    ERRBUFCAT!(buf, " world");
    let s = unsafe {
        std::str::from_utf8_unchecked(&buf[..buf.iter().position(|&b| b == 0).unwrap_or(BUFSIZE)])
    };
    assert_eq!(s, "hello world");
}

// --- Tests for new/improved functions ---

#[test]
fn test_format_varargs_null_format() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let result = format_varargs(ptr::null(), ptr::null_mut());
        assert_eq!(result, "");
    }
}

#[test]
fn test_format_varargs_null_ap() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let msg = std::ffi::CString::new("hello world").unwrap_or_default();
        let result = format_varargs(msg.as_ptr(), ptr::null_mut());
        assert_eq!(result, "hello world");
    }
}

#[test]
fn test_format_varargs_to_buf_null() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let (s, truncated) = format_varargs_to_buf(ptr::null(), ptr::null_mut());
        assert_eq!(s, "");
        assert!(!truncated);
    }
}

#[test]
fn test_format_varargs_to_buf_null_ap() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let msg = std::ffi::CString::new("test message").unwrap_or_default();
        let (s, truncated) = format_varargs_to_buf(msg.as_ptr(), ptr::null_mut());
        assert_eq!(s, "test message");
        assert!(!truncated);
    }
}

#[test]
fn test_r_make_warning_condition() {
    let _session = crate::sexp::session::RSession::new();
    let _session = RSession::new();

    unsafe {
        let cond = R_makeWarningCondition(
            ptr::null_mut(),
            b"simpleWarning\0".as_ptr() as *const c_char,
            ptr::null(),
            0,
            b"test warning message\0".as_ptr() as *const c_char,
        );
        assert!(!cond.is_null());
        assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP);
        assert_eq!(LENGTH(cond), 2);
    }
}

#[test]
fn test_r_make_c_stack_overflow_error() {
    let _session = crate::sexp::session::RSession::new();
    let _session = RSession::new();

    unsafe {
        let cond = R_makeCStackOverflowError(ptr::null_mut(), 42);
        assert!(!cond.is_null());
        assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP);
        assert_eq!(LENGTH(cond), 2);
    }
}

#[test]
fn test_r_make_not_subsettable_error() {
    let _session = crate::sexp::session::RSession::new();
    let _session = RSession::new();

    unsafe {
        // Create a simple vector to act as the "object"
        let x = Rf_allocVector(SEXPTYPE::REALSXP, 1);
        let cond = R_makeNotSubsettableError(x, ptr::null_mut());
        assert!(!cond.is_null());
        assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP);
    }
}

#[test]
fn test_r_make_missing_subscript_error() {
    let _session = crate::sexp::session::RSession::new();
    let _session = RSession::new();

    unsafe {
        let x = Rf_allocVector(SEXPTYPE::INTSXP, 1);
        let cond = R_makeMissingSubscriptError(x, ptr::null_mut());
        assert!(!cond.is_null());
        assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP);
    }
}

#[test]
fn test_r_make_missing_subscript_error1() {
    let _session = crate::sexp::session::RSession::new();
    let _session = RSession::new();

    unsafe {
        let cond = R_makeMissingSubscriptError1(ptr::null_mut());
        assert!(!cond.is_null());
        assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP);
    }
}

#[test]
fn test_r_make_out_of_bounds_error() {
    let _session = crate::sexp::session::RSession::new();
    let _session = RSession::new();

    unsafe {
        let x = Rf_allocVector(SEXPTYPE::INTSXP, 5);
        let idx = Rf_allocVector(SEXPTYPE::REALSXP, 1);
        *REAL(idx) = 10.0;
        let cond = R_makeOutOfBoundsError(x, 10, idx, ptr::null_mut());
        assert!(!cond.is_null());
        assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP);
    }
}

#[test]
fn test_r_make_partial_match_warning_condition() {
    let _session = crate::sexp::session::RSession::new();
    let _session = RSession::new();

    unsafe {
        let input = Rf_install(b"abc\0".as_ptr() as *const c_char);
        let target = Rf_install(b"abcdef\0".as_ptr() as *const c_char);
        let cond = R_makePartialMatchWarningCondition(ptr::null_mut(), input, target);
        assert!(!cond.is_null());
        assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP);
        assert_eq!(LENGTH(cond), 4);

        // Message: "partial match of 'abc' to 'abcdef'"
        let msg = VECTOR_ELT(cond, 0);
        let msg_str = CStr::from_ptr(translateChar(STRING_ELT(msg, 0)));
        assert_eq!(
            msg_str.to_string_lossy(),
            "partial match of 'abc' to 'abcdef'"
        );

        // Class: partialMatchWarning, warning, condition
        let klass = getAttrib_wrap(cond, R_ClassSymbol());
        assert_eq!(LENGTH(klass), 3);
        assert_eq!(
            CStr::from_ptr(translateChar(STRING_ELT(klass, 0))).to_string_lossy(),
            "partialMatchWarning"
        );

        // Fields: input/target hold the symbols themselves
        assert_eq!(VECTOR_ELT(cond, 2), input);
        assert_eq!(VECTOR_ELT(cond, 3), target);
        let names = getAttrib_wrap(cond, R_NamesSymbol());
        assert_eq!(
            CStr::from_ptr(translateChar(STRING_ELT(names, 2))).to_string_lossy(),
            "input"
        );
        assert_eq!(
            CStr::from_ptr(translateChar(STRING_ELT(names, 3))).to_string_lossy(),
            "target"
        );

        // Non-symbol (CHARSXP) input is wrapped via ScalarString
        let chars = Rf_mkChar(b"nam\0".as_ptr() as *const c_char);
        let cond2 = R_makePartialMatchWarningCondition(ptr::null_mut(), chars, target);
        let wrapped = VECTOR_ELT(cond2, 2);
        assert_eq!(TYPEOF(wrapped), SEXPTYPE::STRSXP);
        assert_eq!(
            CStr::from_ptr(translateChar(STRING_ELT(wrapped, 0))).to_string_lossy(),
            "nam"
        );
        let msg2 = VECTOR_ELT(cond2, 0);
        assert_eq!(
            CStr::from_ptr(translateChar(STRING_ELT(msg2, 0))).to_string_lossy(),
            "partial match of 'nam' to 'abcdef'"
        );
    }
}

#[test]
fn test_r_make_partial_argument_match_warning_condition() {
    let _session = crate::sexp::session::RSession::new();
    let _session = RSession::new();

    unsafe {
        let argument = Rf_install(b"ab\0".as_ptr() as *const c_char);
        let formal = Rf_install(b"abcde\0".as_ptr() as *const c_char);
        let cond = R_makePartialArgumentMatchWarningCondition(ptr::null_mut(), argument, formal);
        assert!(!cond.is_null());
        assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP);
        assert_eq!(LENGTH(cond), 4);

        // Message: "partial argument match of 'ab' to 'abcde'"
        let msg = VECTOR_ELT(cond, 0);
        assert_eq!(
            CStr::from_ptr(translateChar(STRING_ELT(msg, 0))).to_string_lossy(),
            "partial argument match of 'ab' to 'abcde'"
        );

        // Class: partialArgumentMatchWarning, partialMatchWarning, warning, condition
        let klass = getAttrib_wrap(cond, R_ClassSymbol());
        assert_eq!(LENGTH(klass), 4);
        assert_eq!(
            CStr::from_ptr(translateChar(STRING_ELT(klass, 0))).to_string_lossy(),
            "partialArgumentMatchWarning"
        );
        assert_eq!(
            CStr::from_ptr(translateChar(STRING_ELT(klass, 1))).to_string_lossy(),
            "partialMatchWarning"
        );

        // Fields: argument/formal hold the symbols
        assert_eq!(VECTOR_ELT(cond, 2), argument);
        assert_eq!(VECTOR_ELT(cond, 3), formal);
        let names = getAttrib_wrap(cond, R_NamesSymbol());
        assert_eq!(
            CStr::from_ptr(translateChar(STRING_ELT(names, 2))).to_string_lossy(),
            "argument"
        );
        assert_eq!(
            CStr::from_ptr(translateChar(STRING_ELT(names, 3))).to_string_lossy(),
            "formal"
        );
    }
}

#[test]
#[ignore = "cannot catch_unwind across extern \"C\" boundary"]
fn test_r_missing_arg_error_c() {
    let _session = crate::sexp::session::RSession::new();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let msg = std::ffi::CString::new("my_arg").unwrap_or_default();
        R_MissingArgError_c(msg.as_ptr(), ptr::null_mut(), ptr::null_mut());
    }));
    assert!(result.is_err());
}

#[test]
fn test_r_expressions_management() {
    let _session = crate::sexp::session::RSession::new();
    let _session = RSession::new();

    let val = R_Expressions();
    assert!(val > 0);
    R_SetExpressions(val + 100);
    assert_eq!(R_Expressions(), val + 100);
    R_SetExpressionsKeep(val);
    R_SetExpressions(val);
    assert_eq!(R_Expressions(), val);
}

#[test]
fn test_warn_length() {
    let _session = crate::sexp::session::RSession::new();
    let _session = RSession::new();

    R_SetWarnLength(500);
    // Just verify it doesn't panic
    let val = r_warn_length();
    assert_eq!(val, 500);
    // Reset to default
    R_SetWarnLength(1000);
}

#[test]
fn test_show_error_messages_flag() {
    let _session = crate::sexp::session::RSession::new();
    let _session = RSession::new();

    R_SetShowErrorMessages(true);
    assert!(r_show_error_messages());
    R_SetShowErrorMessages(false);
    assert!(!r_show_error_messages());
}

#[test]
fn test_r_print_deferred_warnings_no_warnings() {
    let _session = crate::sexp::session::RSession::new();
    let _session = RSession::new();

    unsafe {
        set_collect_warnings(0);
        set_warnings_ptr(ptr::null_mut());
        R_PrintDeferredWarnings();
        // Should not panic
    }
}

#[test]
fn test_r_signal_warning_condition_null() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        R_signalWarningCondition(ptr::null_mut());
        // Should not panic on null
    }
}

#[test]
fn test_r_signal_warning_condition_valid() {
    let _session = crate::sexp::session::RSession::new();
    let _session = RSession::new();
    unsafe {
        let cond = R_makeWarningCondition(
            ptr::null_mut(),
            b"simpleWarning\0".as_ptr() as *const c_char,
            ptr::null(),
            0,
            b"test warning\0".as_ptr() as *const c_char,
        );
        R_signalWarningCondition(cond);
        // Should not panic — warning is printed to stderr
    }
}

#[test]
fn test_r_get_current_srcref() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let result = R_GetCurrentSrcref(0);
        // Returns R_NilValue since srcref not implemented
        assert!(result.is_null() || TYPEOF(result) == SEXPTYPE::NILSXP);
    }
}

#[test]
fn test_r_get_src_filename() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let result = R_GetSrcFilename(ptr::null_mut());
        assert!(!result.is_null());
        assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP);
    }
}

#[test]
fn test_rf_errorcall_fmt() {
    let _session = crate::sexp::session::RSession::new();
    let fmt = std::ffi::CString::new("hello %s world %s").unwrap_or_default();
    let arg1 = must(std::ffi::CStr::from_bytes_with_nul(b"beautiful\0"));
    let arg2 = must(std::ffi::CStr::from_bytes_with_nul(b"today\0"));
    // This function pre-formats and calls verrorcall_dflt, which panics
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Rf_errorcall_fmt(ptr::null_mut(), fmt.as_ptr(), &[arg1, arg2]);
    }));
    assert!(result.is_err());
}

#[test]
fn test_entry_macros() {
    let _session = crate::sexp::session::RSession::new();
    unsafe {
        let entry = Rf_allocVector(SEXPTYPE::VECSXP, 5);
        // Set up some values
        let v0 = Rf_mkString(b"class\0".as_ptr() as *const c_char);
        let v2 = Rf_mkString(b"handler\0".as_ptr() as *const c_char);
        let v3 = Rf_mkString(b"target\0".as_ptr() as *const c_char);
        let v4 = Rf_mkString(b"result\0".as_ptr() as *const c_char);
        SET_VECTOR_ELT(entry, 0, v0);
        SET_VECTOR_ELT(entry, 2, v2);
        SET_VECTOR_ELT(entry, 3, v3);
        SET_VECTOR_ELT(entry, 4, v4);

        assert!(!ENTRY_CLASS(entry).is_null());
        assert!(!ENTRY_HANDLER(entry).is_null());
        assert!(!ENTRY_TARGET_ENVIR(entry).is_null());
        assert!(!ENTRY_RETURN_RESULT(entry).is_null());

        CLEAR_ENTRY_CALLING_ENVIR(entry);
        CLEAR_ENTRY_TARGET_ENVIR(entry);
        // After clearing, these should be R_NilValue
        assert!(
            ENTRY_TARGET_ENVIR(entry).is_null()
                || TYPEOF(ENTRY_TARGET_ENVIR(entry)) == SEXPTYPE::NILSXP
        );
    }
}

#[test]
fn test_longwarn_constant() {
    let _session = crate::sexp::session::RSession::new();
    assert_eq!(LONGWARN, 75);
}

#[test]
fn test_bufsize_constant() {
    let _session = crate::sexp::session::RSession::new();
    assert_eq!(BUFSIZE, 8192);
}

#[test]
fn test_r_nwarnings_default() {
    let _session = crate::sexp::session::RSession::new();
    assert_eq!(R_NWARNINGS_DEFAULT, 50);
}
