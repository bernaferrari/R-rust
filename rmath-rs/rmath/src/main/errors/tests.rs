#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::sexp::accessors::*;
    use crate::sexp::constructors::*;
    use crate::sexp::ffi::{SEXP, SEXPTYPE};
    use std::ptr;
    use std::sync::atomic::Ordering;

    #[test]
    fn test_wd() {
        assert_eq!(format::wd("hello"), 5);
        assert_eq!(format::wd(""), 0);
        assert_eq!(format::wd("hello world"), 11);
    }

    #[test]
    fn test_R_SetErrmessage() {
        R_SetErrmessage("test error");
        assert_eq!(R_GetErrorBuf(), "test error");

        R_SetErrmessage("");
        assert_eq!(R_GetErrorBuf(), "");
    }

    #[test]
    fn test_error_catches_panic() {
        use crate::sexp::context::RError;
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
        assert_eq!(format::count_format_args("hello %s world %d"), 2);
        assert_eq!(format::count_format_args("no args"), 0);
        assert_eq!(format::count_format_args("%% escaped"), 0);
    }

    #[test]
    fn test_in_error_flag() {
        assert_eq!(R_GetInError(), 0);
        R_SetInError(1);
        assert_eq!(R_GetInError(), 1);
        R_SetInError(0);
    }

    #[test]
    fn test_format_to_buf() {
        let mut buf = [0u8; BUFSIZE + 1];
        let (len, truncated) = format::format_to_buf(&mut buf, "hello world");
        assert_eq!(len, 11);
        assert!(!truncated);
        let s = unsafe {
            std::str::from_utf8_unchecked(
                &buf[..buf.iter().position(|&b| b == 0).unwrap_or(BUFSIZE)],
            )
        };
        assert_eq!(s, "hello world");
    }

    #[test]
    fn test_format_to_buf_long() {
        let mut buf = [0u8; BUFSIZE + 1];
        let long_str = "x".repeat(BUFSIZE + 100);
        let (len, truncated) = format::format_to_buf(&mut buf, &long_str);
        assert_eq!(len, BUFSIZE + 100);
        assert!(truncated);
    }

    #[test]
    fn test_bufcat() {
        let mut buf = [0u8; BUFSIZE + 1];
        format::format_to_buf(&mut buf, "hello");
        format::bufcat(&mut buf, " world");
        let s = unsafe {
            std::str::from_utf8_unchecked(
                &buf[..buf.iter().position(|&b| b == 0).unwrap_or(BUFSIZE)],
            )
        };
        assert_eq!(s, "hello world");
    }

    #[test]
    fn test_print_trunc() {
        let mut buf = [0u8; BUFSIZE + 1];
        format::format_to_buf(&mut buf, "hello");
        format::print_trunc(&mut buf, true);
        let s = unsafe {
            std::str::from_utf8_unchecked(
                &buf[..buf.iter().position(|&b| b == 0).unwrap_or(BUFSIZE)],
            )
        };
        assert!(s.contains("[... truncated]"));
    }

    #[test]
    fn test_print_trunc_not_truncated() {
        let mut buf = [0u8; BUFSIZE + 1];
        format::format_to_buf(&mut buf, "hello");
        format::print_trunc(&mut buf, false);
        let s = unsafe {
            std::str::from_utf8_unchecked(
                &buf[..buf.iter().position(|&b| b == 0).unwrap_or(BUFSIZE)],
            )
        };
        assert_eq!(s, "hello");
        assert!(!s.contains("[... truncated]"));
    }

    #[test]
    fn test_mkHandlerEntry() {
        unsafe {
            let klass = Rf_mkString(b"error\x00".as_ptr() as *const std::os::raw::c_char);
            let handler = Rf_mkString(b"handler\x00".as_ptr() as *const std::os::raw::c_char);
            let entry = conditions::mkHandlerEntry(
                klass,
                ptr::null_mut(),
                handler,
                ptr::null_mut(),
                ptr::null_mut(),
                1,
            );
            assert!(!entry.is_null());
            assert_eq!(TYPEOF(entry), SEXPTYPE::VECSXP.0);
            assert_eq!(LENGTH(entry), 5);
            assert_eq!(conditions::IS_CALLING_ENTRY(entry), 1);
        }
    }

    #[test]
    fn test_r_makeErrorCondition() {
        unsafe {
            let cond = conditions::R_makeErrorCondition(
                ptr::null_mut(),
                b"simpleError\x00".as_ptr() as *const std::os::raw::c_char,
                ptr::null_mut(),
                0,
                b"test error message\x00".as_ptr() as *const std::os::raw::c_char,
            );
            assert!(!cond.is_null());
            assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP.0);
            assert_eq!(LENGTH(cond), 2);
        }
    }

    #[test]
    fn test_r_makeErrorCondition_with_subclass() {
        unsafe {
            let cond = conditions::R_makeErrorCondition(
                ptr::null_mut(),
                b"error\x00".as_ptr() as *const std::os::raw::c_char,
                b"simpleError\x00".as_ptr() as *const std::os::raw::c_char,
                0,
                b"test error\x00".as_ptr() as *const std::os::raw::c_char,
            );
            assert!(!cond.is_null());
            assert_eq!(LENGTH(cond), 2);
            let klass = format::getAttrib_wrap(cond, crate::attrib_core::R_ClassSymbol());
            if !klass.is_null() && TYPEOF(klass) == SEXPTYPE::STRSXP.0 {
                assert!(LENGTH(klass) >= 3);
            }
        }
    }

    #[test]
    fn test_concise_traceback_empty() {
        unsafe {
            let result = warning::R_ConciseTraceback(ptr::null_mut(), 0);
            assert_eq!(result, "");
        }
    }

    #[test]
    fn test_interrupts_suspended() {
        assert!(!R_InterruptsSuspended());
        R_SetInterruptsSuspended(true);
        assert!(R_InterruptsSuspended());
        R_SetInterruptsSuspended(false);
        assert!(!R_InterruptsSuspended());
    }

    #[test]
    fn test_warning_collection() {
        unsafe {
            R_COLLECT_WARNINGS.store(0, Ordering::Relaxed);
            R_WARNINGS.store(ptr::null_mut(), Ordering::Relaxed);

            warning::setup_warnings();
            assert!(
                R_WARNINGS.load(Ordering::Relaxed).is_null()
                    || TYPEOF(R_WARNINGS.load(Ordering::Relaxed)) == SEXPTYPE::VECSXP.0
            );

            R_COLLECT_WARNINGS.store(0, Ordering::Relaxed);
            R_WARNINGS.store(ptr::null_mut(), Ordering::Relaxed);
        }
    }

    #[test]
    fn test_handler_stack_operations() {
        R_HANDLER_STACK.with(|stack| {
            *stack.borrow_mut() = ptr::null_mut();
        });

        unsafe {
            let entry = Rf_allocVector(SEXPTYPE::VECSXP.0, 5);
            R_HANDLER_STACK.with(|stack| {
                *stack.borrow_mut() = Rf_cons(entry, ptr::null_mut());
                assert!(!(*stack.borrow()).is_null());
            });

            R_HANDLER_STACK.with(|stack| {
                *stack.borrow_mut() = ptr::null_mut();
            });
        }
    }

    #[test]
    fn test_restart_stack_operations() {
        R_RESTART_STACK.with(|stack| {
            *stack.borrow_mut() = ptr::null_mut();
        });

        unsafe {
            let entry = Rf_allocVector(SEXPTYPE::VECSXP.0, 2);
            R_RESTART_STACK.with(|stack| {
                *stack.borrow_mut() = Rf_cons(entry, ptr::null_mut());
                assert!(!(*stack.borrow()).is_null());
            });

            R_RESTART_STACK.with(|stack| {
                *stack.borrow_mut() = ptr::null_mut();
            });
        }
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(error_codes::ERROR_NUMARGS, 1);
        assert_eq!(error_codes::ERROR_UNKNOWN, 6);
        assert_eq!(warning_codes::WARNING_coerce_NA, 0);
        assert_eq!(warning_codes::WARNING_UNKNOWN, 3);
    }

    #[test]
    fn test_format_varargs_null_format() {
        unsafe {
            let result = format::format_varargs(ptr::null(), ptr::null_mut());
            assert_eq!(result, "");
        }
    }

    #[test]
    fn test_format_varargs_null_ap() {
        unsafe {
            let msg = std::ffi::CString::new("hello world").unwrap();
            let result = format::format_varargs(msg.as_ptr(), ptr::null_mut());
            assert_eq!(result, "hello world");
        }
    }

    #[test]
    fn test_format_varargs_to_buf_null() {
        unsafe {
            let (s, truncated) = format::format_varargs_to_buf(ptr::null(), ptr::null_mut());
            assert_eq!(s, "");
            assert!(!truncated);
        }
    }

    #[test]
    fn test_format_varargs_to_buf_null_ap() {
        unsafe {
            let msg = std::ffi::CString::new("test message").unwrap();
            let (s, truncated) = format::format_varargs_to_buf(msg.as_ptr(), ptr::null_mut());
            assert_eq!(s, "test message");
            assert!(!truncated);
        }
    }

    #[test]
    fn test_r_make_warning_condition() {
        unsafe {
            let cond = conditions::R_makeWarningCondition(
                ptr::null_mut(),
                b"simpleWarning\0".as_ptr() as *const std::os::raw::c_char,
                0,
                b"test warning message\0".as_ptr() as *const std::os::raw::c_char,
            );
            assert!(!cond.is_null());
            assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP.0);
            assert_eq!(LENGTH(cond), 2);
        }
    }

    #[test]
    fn test_r_make_c_stack_overflow_error() {
        unsafe {
            let cond = conditions::R_makeCStackOverflowError(ptr::null_mut(), 42);
            assert!(!cond.is_null());
            assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP.0);
            assert_eq!(LENGTH(cond), 2);
        }
    }

    #[test]
    fn test_r_make_not_subsettable_error() {
        unsafe {
            let x = Rf_allocVector(SEXPTYPE::REALSXP.0, 1);
            let cond = conditions::R_makeNotSubsettableError(x, ptr::null_mut());
            assert!(!cond.is_null());
            assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP.0);
        }
    }

    #[test]
    fn test_r_make_missing_subscript_error() {
        unsafe {
            let x = Rf_allocVector(SEXPTYPE::INTSXP.0, 1);
            let cond = conditions::R_makeMissingSubscriptError(x, ptr::null_mut());
            assert!(!cond.is_null());
            assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP.0);
        }
    }

    #[test]
    fn test_r_make_missing_subscript_error1() {
        unsafe {
            let cond = conditions::R_makeMissingSubscriptError1(ptr::null_mut());
            assert!(!cond.is_null());
            assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP.0);
        }
    }

    #[test]
    fn test_r_make_out_of_bounds_error() {
        unsafe {
            let x = Rf_allocVector(SEXPTYPE::INTSXP.0, 5);
            let idx = Rf_allocVector(SEXPTYPE::REALSXP.0, 1);
            *REAL(idx) = 10.0;
            let cond = conditions::R_makeOutOfBoundsError(x, 10, idx, ptr::null_mut());
            assert!(!cond.is_null());
            assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP.0);
        }
    }

    #[test]
    fn test_r_make_partial_match_warning_condition() {
        unsafe {
            let arg = Rf_install(b"abc\0".as_ptr() as *const std::os::raw::c_char);
            let formal = Rf_install(b"abcdef\0".as_ptr() as *const std::os::raw::c_char);
            let cond = conditions::R_makePartialMatchWarningCondition(ptr::null_mut(), arg, formal);
            assert!(!cond.is_null());
            assert_eq!(TYPEOF(cond), SEXPTYPE::VECSXP.0);
        }
    }

    #[test]
    #[ignore] // cannot catch_unwind across extern "C" boundary
    fn test_r_missing_arg_error_c() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            let msg = std::ffi::CString::new("my_arg").unwrap();
            error::R_MissingArgError_c(msg.as_ptr(), ptr::null_mut(), ptr::null_mut());
        }));
        assert!(result.is_err());
    }

    #[test]
    fn test_r_expressions_management() {
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
        R_SetWarnLength(500);
        let val = R_WARN_LENGTH.load(Ordering::Relaxed);
        assert_eq!(val, 500);
        R_SetWarnLength(1000);
    }

    #[test]
    fn test_show_error_messages_flag() {
        R_SetShowErrorMessages(true);
        R_SetShowErrorMessages(false);
    }

    #[test]
    fn test_r_print_deferred_warnings_no_warnings() {
        unsafe {
            R_COLLECT_WARNINGS.store(0, Ordering::Relaxed);
            R_WARNINGS.store(ptr::null_mut(), Ordering::Relaxed);
            warning::R_PrintDeferredWarnings();
        }
    }

    #[test]
    fn test_r_signal_warning_condition_null() {
        unsafe {
            conditions::R_signalWarningCondition(ptr::null_mut());
        }
    }

    #[test]
    fn test_r_signal_warning_condition_valid() {
        unsafe {
            let cond = conditions::R_makeWarningCondition(
                ptr::null_mut(),
                b"simpleWarning\0".as_ptr() as *const std::os::raw::c_char,
                0,
                b"test warning\0".as_ptr() as *const std::os::raw::c_char,
            );
            conditions::R_signalWarningCondition(cond);
        }
    }

    #[test]
    fn test_r_get_current_srcref() {
        unsafe {
            let result = warning::R_GetCurrentSrcref(0);
            assert!(result.is_null() || TYPEOF(result) == SEXPTYPE::NILSXP.0);
        }
    }

    #[test]
    fn test_r_get_src_filename() {
        unsafe {
            let result = warning::R_GetSrcFilename(ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::STRSXP.0);
        }
    }

    #[test]
    fn test_rf_errorcall_fmt() {
        unsafe {
            let fmt = std::ffi::CString::new("hello %s world %s").unwrap();
            let arg1 = std::ffi::CStr::from_bytes_with_nul(b"beautiful\0").unwrap();
            let arg2 = std::ffi::CStr::from_bytes_with_nul(b"today\0").unwrap();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                error::Rf_errorcall_fmt(ptr::null_mut(), fmt.as_ptr(), &[arg1, arg2]);
            }));
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_entry_macros() {
        unsafe {
            let entry = Rf_allocVector(SEXPTYPE::VECSXP.0, 5);
            let v0 = Rf_mkString(b"class\0".as_ptr() as *const std::os::raw::c_char);
            let v2 = Rf_mkString(b"handler\0".as_ptr() as *const std::os::raw::c_char);
            let v3 = Rf_mkString(b"target\0".as_ptr() as *const std::os::raw::c_char);
            let v4 = Rf_mkString(b"result\0".as_ptr() as *const std::os::raw::c_char);
            SET_VECTOR_ELT(entry, 0, v0);
            SET_VECTOR_ELT(entry, 2, v2);
            SET_VECTOR_ELT(entry, 3, v3);
            SET_VECTOR_ELT(entry, 4, v4);

            assert!(!conditions::ENTRY_CLASS(entry).is_null());
            assert!(!conditions::ENTRY_HANDLER(entry).is_null());
            assert!(!conditions::ENTRY_TARGET_ENVIR(entry).is_null());
            assert!(!conditions::ENTRY_RETURN_RESULT(entry).is_null());

            conditions::CLEAR_ENTRY_CALLING_ENVIR(entry);
            conditions::CLEAR_ENTRY_TARGET_ENVIR(entry);
            assert!(
                conditions::ENTRY_TARGET_ENVIR(entry).is_null()
                    || TYPEOF(conditions::ENTRY_TARGET_ENVIR(entry)) == SEXPTYPE::NILSXP.0
            );
        }
    }

    #[test]
    fn test_longwarn_constant() {
        assert_eq!(LONGWARN, 75);
    }

    #[test]
    fn test_bufsize_constant() {
        assert_eq!(BUFSIZE, 8192);
    }

    #[test]
    fn test_r_nwarnings_default() {
        assert_eq!(R_NWARNINGS_DEFAULT, 50);
    }
}
