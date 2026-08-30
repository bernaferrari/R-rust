#![allow(
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_imports
)]

use super::*;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_r_error(action: impl FnOnce()) -> crate::sexp::context::RError {
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action))
            .expect_err("expected RError panic");
        payload
            .downcast_ref::<crate::sexp::context::RError>()
            .expect("expected RError payload")
            .clone()
    }

    #[test]
    fn test_equalS3Signature_exact_match() {
        let _session = crate::sexp::session::RSession::new();
        let signature = b"print.default\0";
        let left = b"print\0";
        let right = b"default\0";
        assert_eq!(
            unsafe {
                equalS3Signature(
                    signature.as_ptr() as *const c_char,
                    left.as_ptr() as *const c_char,
                    right.as_ptr() as *const c_char,
                )
            },
            TRUE
        );
    }

    #[test]
    fn test_equalS3Signature_no_match() {
        let _session = crate::sexp::session::RSession::new();
        let signature = b"print.data.frame\0";
        let left = b"print\0";
        let right = b"default\0";
        assert_eq!(
            unsafe {
                equalS3Signature(
                    signature.as_ptr() as *const c_char,
                    left.as_ptr() as *const c_char,
                    right.as_ptr() as *const c_char,
                )
            },
            FALSE
        );
    }

    #[test]
    fn test_equalS3Signature_empty_right() {
        let _session = crate::sexp::session::RSession::new();
        let signature = b"foo.bar\0";
        let left = b"foo\0";
        let right = b"baz\0";
        assert_eq!(
            unsafe {
                equalS3Signature(
                    signature.as_ptr() as *const c_char,
                    left.as_ptr() as *const c_char,
                    right.as_ptr() as *const c_char,
                )
            },
            FALSE
        );
    }

    #[test]
    fn test_length_counts_pairlists_by_cdr_chain() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let tail = Rf_cons(R_NilValue(), R_NilValue());
            let _tail_guard = protect(tail);
            let pairlist = Rf_cons(R_NilValue(), tail);
            let _pairlist_guard = protect(pairlist);

            assert_eq!(TYPEOF(pairlist), SEXPTYPE::LISTSXP);
            assert_eq!(length(pairlist), 2);
        }
    }

    #[test]
    fn test_equalS3Signature_missing_dot() {
        let _session = crate::sexp::session::RSession::new();
        let signature = b"foobar\0";
        let left = b"foo\0";
        let right = b"bar\0";
        assert_eq!(
            unsafe {
                equalS3Signature(
                    signature.as_ptr() as *const c_char,
                    left.as_ptr() as *const c_char,
                    right.as_ptr() as *const c_char,
                )
            },
            FALSE
        );
    }

    #[test]
    fn test_equalS3Signature_signature_longer() {
        let _session = crate::sexp::session::RSession::new();
        let signature = b"print.default.extra\0";
        let left = b"print\0";
        let right = b"default\0";
        assert_eq!(
            unsafe {
                equalS3Signature(
                    signature.as_ptr() as *const c_char,
                    left.as_ptr() as *const c_char,
                    right.as_ptr() as *const c_char,
                )
            },
            FALSE
        );
    }

    #[test]
    fn test_equalS3Signature_null_pointers() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(
            unsafe {
                equalS3Signature(
                    ptr::null(),
                    b"foo\0".as_ptr() as *const c_char,
                    b"bar\0".as_ptr() as *const c_char,
                )
            },
            FALSE
        );
    }

    #[test]
    fn test_equalS3Signature_single_char() {
        let _session = crate::sexp::session::RSession::new();
        let signature = b"a.b\0";
        let left = b"a\0";
        let right = b"b\0";
        assert_eq!(
            unsafe {
                equalS3Signature(
                    signature.as_ptr() as *const c_char,
                    left.as_ptr() as *const c_char,
                    right.as_ptr() as *const c_char,
                )
            },
            TRUE
        );
    }

    #[test]
    fn test_IS_S4_OBJECT_null() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(unsafe { IS_S4_OBJECT(ptr::null_mut()) }, FALSE);
    }

    #[test]
    fn test_isS4_null() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(unsafe { isS4(ptr::null_mut()) }, FALSE);
    }

    #[test]
    fn test_isS4_with_vector() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            assert_eq!(isS4(v), FALSE);
        }
    }

    #[test]
    fn test_SET_S4_OBJECT_and_check() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            assert_eq!(IS_S4_OBJECT(v), FALSE);
            SET_S4_OBJECT(v);
            assert_eq!(IS_S4_OBJECT(v), TRUE);
            UNSET_S4_OBJECT(v);
            assert_eq!(IS_S4_OBJECT(v), FALSE);
        }
    }

    #[test]
    fn test_isString() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_mkString(b"hello\0".as_ptr() as *const c_char);
            assert_eq!(isString(s), TRUE);
            let v = Rf_ScalarInteger(42);
            assert_eq!(isString(v), FALSE);
        }
    }

    #[test]
    fn test_isFunction_checks() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(isFunction(ptr::null_mut()), FALSE);
            assert_eq!(isPrimitive(ptr::null_mut()), FALSE);
            assert_eq!(isClosure(ptr::null_mut()), FALSE);
        }
    }

    #[test]
    fn test_isObject_checks() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            assert_eq!(isObject(v), FALSE);
        }
    }

    #[test]
    fn test_isValidString() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_mkString(b"hello\0".as_ptr() as *const c_char);
            assert_eq!(isValidString(s), TRUE);
            let empty = Rf_mkString(b"\x00".as_ptr() as *const c_char);
            assert_eq!(isValidString(empty), FALSE);
            assert_eq!(isValidString(ptr::null_mut()), FALSE);
        }
    }

    #[test]
    fn test_length() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(length(ptr::null_mut()), 0);
            assert_eq!(length(R_NilValue()), 0);
            let v = Rf_allocVector(SEXPTYPE::INTSXP, 5);
            assert_eq!(length(v), 5);
        }
    }

    #[test]
    fn test_isNull() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(isNull(ptr::null_mut()), TRUE);
            assert_eq!(isNull(R_NilValue()), TRUE);
            let v = Rf_ScalarInteger(42);
            assert_eq!(isNull(v), FALSE);
        }
    }

    #[test]
    fn test_asRbool() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let t = Rf_ScalarLogical(TRUE);
            assert_eq!(asRbool(t, ptr::null_mut()), TRUE);
            let f = Rf_ScalarLogical(FALSE);
            assert_eq!(asRbool(f, ptr::null_mut()), FALSE);
        }
    }

    #[test]
    fn test_inherits2_null() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(
            unsafe { inherits2(ptr::null_mut(), b"foo\0".as_ptr() as *const c_char) },
            FALSE
        );
    }

    #[test]
    fn test_inherits2_no_class() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            assert_eq!(inherits2(v, b"numeric\0".as_ptr() as *const c_char), FALSE);
        }
    }

    #[test]
    fn test_inherits2_with_class() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            let class_vec = Rf_allocVector(SEXPTYPE::STRSXP, 1);
            let _class_vec_guard = protect(class_vec);
            SET_STRING_ELT(
                class_vec,
                0,
                Rf_mkChar(b"myclass\0".as_ptr() as *const c_char),
            );
            setAttrib(v, R_ClassSymbol(), class_vec);
            SET_OBJECT(v, 1); // Ensure OBJECT bit is set for this test
            assert_eq!(inherits2(v, b"myclass\0".as_ptr() as *const c_char), TRUE);
            assert_eq!(
                inherits2(v, b"otherclass\0".as_ptr() as *const c_char),
                FALSE
            );
        }
    }

    #[test]
    fn test_do_inherits_basic() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            let what = Rf_mkString(b"integer\0".as_ptr() as *const c_char);
            let which = Rf_ScalarLogical(FALSE);
            let args = Rf_cons(v, Rf_cons(what, Rf_cons(which, R_NilValue())));
            let result = do_inherits(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
        }
    }

    #[test]
    fn test_do_class() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            let args = Rf_cons(v, R_NilValue());
            let result = do_class_objects(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            // Should return "integer" class
        }
    }

    #[test]
    fn test_do_isobject() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            let args = Rf_cons(v, R_NilValue());
            let result = do_isobject(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            // Scalar integer has no explicit class
            assert_eq!(LOGICAL(result).is_null() || *LOGICAL(result) == FALSE, true);
        }
    }

    #[test]
    fn test_do_isS4() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            let args = Rf_cons(v, R_NilValue());
            let result = do_isS4(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
        }
    }

    #[test]
    fn test_do_oldClass() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            let args = Rf_cons(v, R_NilValue());
            let result = do_oldClass(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
        }
    }

    #[test]
    fn test_do_procdest() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = do_procdest(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert!(result.is_null() || result == R_NilValue());
        }
    }

    #[test]
    fn test_do_S4on() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = do_S4on(
                ptr::null_mut(),
                ptr::null_mut(),
                R_NilValue(),
                ptr::null_mut(),
            );
            assert!(!result.is_null());
        }
    }

    #[test]
    fn test_isMethodsDispatchOn_initial() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(isMethodsDispatchOn(), FALSE);
        }
    }

    #[test]
    fn test_R_set_standardGeneric_ptr_roundtrip() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let old = R_set_standardGeneric_ptr(None, ptr::null_mut());
            assert!(old.is_none());
            assert_eq!(isMethodsDispatchOn(), FALSE);
        }
    }

    unsafe fn standard_generic_a(_arg: SEXP, _env: SEXP, _fdef: SEXP) -> SEXP {
        unsafe { R_NilValue() }
    }

    unsafe fn standard_generic_b(_arg: SEXP, _env: SEXP, _fdef: SEXP) -> SEXP {
        unsafe { R_NilValue() }
    }

    #[test]
    fn test_standard_generic_ptr_is_session_local() {
        let mut left = crate::sexp::session::RSession::new();
        let mut right = crate::sexp::session::RSession::new();

        left.with_arena(|_| unsafe {
            assert!(R_set_standardGeneric_ptr(Some(standard_generic_a), ptr::null_mut()).is_none());
            assert_eq!(isMethodsDispatchOn(), TRUE);
        });

        right.with_arena(|_| unsafe {
            assert_eq!(isMethodsDispatchOn(), FALSE);
            assert!(R_set_standardGeneric_ptr(Some(standard_generic_b), ptr::null_mut()).is_none());
            assert_eq!(isMethodsDispatchOn(), TRUE);
        });

        left.with_arena(|_| unsafe {
            assert_eq!(isMethodsDispatchOn(), TRUE);
            assert!(R_set_standardGeneric_ptr(None, ptr::null_mut()).is_some());
            assert_eq!(isMethodsDispatchOn(), FALSE);
        });

        right.with_arena(|_| unsafe {
            assert_eq!(isMethodsDispatchOn(), TRUE);
        });
    }

    #[test]
    fn test_isBasicClass() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(
            unsafe { isBasicClass(b"numeric\0".as_ptr() as *const c_char) },
            FALSE
        );
    }

    #[test]
    fn test_R_has_methods_attached() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(unsafe { R_has_methods_attached() }, FALSE);
    }

    #[test]
    fn test_R_check_class_etc() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let valid: Vec<*const c_char> = vec![b"foo\0".as_ptr() as *const c_char, ptr::null()];
            assert_eq!(R_check_class_etc(ptr::null_mut(), valid.as_ptr()), -1);
        }
    }

    #[test]
    fn test_R_check_class_and_super() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let valid: Vec<*const c_char> = vec![b"foo\0".as_ptr() as *const c_char, ptr::null()];
            assert_eq!(
                R_check_class_and_super(ptr::null_mut(), valid.as_ptr(), ptr::null_mut()),
                -1
            );
        }
    }

    #[test]
    fn test_R_check_class_and_super_with_class() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            let class_vec = Rf_allocVector(SEXPTYPE::STRSXP, 1);
            let _class_vec_guard = protect(class_vec);
            SET_STRING_ELT(
                class_vec,
                0,
                Rf_mkChar(b"myclass\0".as_ptr() as *const c_char),
            );
            setAttrib(v, R_ClassSymbol(), class_vec);
            SET_OBJECT(v, 1); // Ensure OBJECT bit is set for this test
            let valid: Vec<*const c_char> = vec![
                b"other\0".as_ptr() as *const c_char,
                b"myclass\0".as_ptr() as *const c_char,
                ptr::null(),
            ];
            let result = R_check_class_and_super(v, valid.as_ptr(), ptr::null_mut());
            // Result should be >= 0 (found) or -1 (not found)
            // If class attribute infrastructure works, result should be 1
            if isObject(v) != FALSE {
                assert_eq!(result, 1);
            }
        }
    }

    #[test]
    fn test_R_has_methods() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(R_has_methods(ptr::null_mut()), FALSE);
        }
    }

    #[test]
    fn test_R_deferred_default_method() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = R_deferred_default_method();
            assert!(!result.is_null());
        }
    }

    #[test]
    fn test_R_do_MAKE_CLASS() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = R_do_MAKE_CLASS(b"foo\0".as_ptr() as *const c_char);
            assert_eq!(TYPEOF(result), SEXPTYPE::VECSXP);
            assert_eq!(
                sexp_to_string(named_vec_elt(result, "className")).as_deref(),
                Some("foo")
            );
        }
    }

    #[test]
    fn test_R_getClassDef() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = R_getClassDef(b"foo\0".as_ptr() as *const c_char);
            assert!(result.is_null() || result == R_NilValue());

            register_s4_class_with_extends(
                "Child".to_string(),
                vec!["x".to_string()],
                vec!["Parent".to_string()],
                false,
            );
            let result = R_getClassDef(b"Child\0".as_ptr() as *const c_char);
            assert_eq!(TYPEOF(result), SEXPTYPE::VECSXP);
            assert_eq!(
                sexp_to_string(named_vec_elt(result, "className")).as_deref(),
                Some("Child")
            );
            assert_eq!(LENGTH(named_vec_elt(result, "slots")), 1);
            assert_eq!(LENGTH(named_vec_elt(result, "contains")), 1);
        }
    }

    #[test]
    fn test_R_seemsOldStyleS4Object() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(R_seemsOldStyleS4Object(ptr::null_mut()), FALSE);
        }
    }

    #[test]
    fn test_R_possible_dispatch() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = R_possible_dispatch(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                0,
            );
            assert!(result.is_null());
        }
    }

    #[test]
    fn test_usemethod_returns_zero() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut ans: SEXP = ptr::null_mut();
            assert_eq!(
                usemethod(
                    b"print\0".as_ptr() as *const c_char,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut ans,
                ),
                0
            );
        }
    }

    #[test]
    fn test_prim_methods_t_values() {
        let _session = crate::sexp::session::RSession::new();
        assert_eq!(prim_methods_t::NO_METHODS as c_int, 0);
        assert_eq!(prim_methods_t::NEEDS_RESET as c_int, 1);
        assert_eq!(prim_methods_t::HAS_METHODS as c_int, 2);
        assert_eq!(prim_methods_t::SUPPRESSED as c_int, 3);
    }

    #[test]
    fn test_createS3Vars() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let generic = Rf_mkString(b"print\0".as_ptr() as *const c_char);
            let group = Rf_mkString(b"\x00".as_ptr() as *const c_char);
            let klass = Rf_mkString(b"foo\0".as_ptr() as *const c_char);
            let method = Rf_mkString(b"print.foo\0".as_ptr() as *const c_char);
            let callenv = R_GlobalEnv();
            let defenv = R_GlobalEnv();

            let vars = createS3Vars(generic, group, klass, method, callenv, defenv);
            assert!(!vars.is_null());

            // Should have 6 elements: .Generic, .Class, .Method, .Group, .GenericCallEnv, .GenericDefEnv
            let mut count = 0;
            let mut current = vars;
            while !current.is_null() && current != R_NilValue() {
                count += 1;
                current = CDR(current);
            }
            assert_eq!(count, 6);
        }
    }

    #[test]
    fn test_addS3Var() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let name = sym(".Generic");
            let value = Rf_mkString(b"print\0".as_ptr() as *const c_char);
            let vars = addS3Var(R_NilValue(), name, value);
            assert!(!vars.is_null());
            assert_eq!(TAG(vars), name);
            assert_eq!(CAR(vars), value);
            assert_eq!(CDR(vars), R_NilValue());
        }
    }

    #[test]
    fn test_newintoold_empty_new() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let old = Rf_cons(Rf_ScalarInteger(1), R_NilValue());
            let result = newintoold(R_NilValue(), old);
            assert!(result.is_null() || result == R_NilValue());
        }
    }

    #[test]
    fn test_listAppend() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v1 = Rf_ScalarInteger(1);
            let v2 = Rf_ScalarInteger(2);
            let v3 = Rf_ScalarInteger(3);
            let t = Rf_cons(v1, Rf_cons(v2, R_NilValue()));
            let s = Rf_cons(v3, R_NilValue());
            let result = listAppend(t, s);
            assert!(!result.is_null());
            assert_eq!(CAR(result), v1);
            assert_eq!(CAR(CDR(result)), v2);
            assert_eq!(CAR(CDR(CDR(result))), v3);
        }
    }

    #[test]
    fn test_listAppend_null_t() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_cons(Rf_ScalarInteger(1), R_NilValue());
            let result = listAppend(R_NilValue(), s);
            assert_eq!(result, s);
        }
    }

    #[test]
    fn test_stringPositionTr() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let klass = Rf_allocVector(SEXPTYPE::STRSXP, 3);
            let _klass_guard = protect(klass);
            SET_STRING_ELT(klass, 0, Rf_mkChar(b"foo\0".as_ptr() as *const c_char));
            SET_STRING_ELT(klass, 1, Rf_mkChar(b"bar\0".as_ptr() as *const c_char));
            SET_STRING_ELT(klass, 2, Rf_mkChar(b"baz\0".as_ptr() as *const c_char));

            assert_eq!(
                stringPositionTr(klass, b"foo\0".as_ptr() as *const c_char),
                0
            );
            assert_eq!(
                stringPositionTr(klass, b"bar\0".as_ptr() as *const c_char),
                1
            );
            assert_eq!(
                stringPositionTr(klass, b"baz\0".as_ptr() as *const c_char),
                2
            );
            assert_eq!(
                stringPositionTr(klass, b"qux\0".as_ptr() as *const c_char),
                -1
            );
            assert_eq!(
                stringPositionTr(klass, b"\x00".as_ptr() as *const c_char),
                -1
            );
        }
    }

    #[test]
    fn test_stringSuffix() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let klass = Rf_allocVector(SEXPTYPE::STRSXP, 3);
            let _klass_guard = protect(klass);
            SET_STRING_ELT(klass, 0, Rf_mkChar(b"foo\0".as_ptr() as *const c_char));
            SET_STRING_ELT(klass, 1, Rf_mkChar(b"bar\0".as_ptr() as *const c_char));
            SET_STRING_ELT(klass, 2, Rf_mkChar(b"baz\0".as_ptr() as *const c_char));

            let suffix = stringSuffix(klass, 1);
            assert!(!suffix.is_null());
            assert_eq!(LENGTH(suffix), 2);
            assert_eq!(Seql(STRING_ELT(suffix, 0), STRING_ELT(klass, 1)), TRUE);
            assert_eq!(Seql(STRING_ELT(suffix, 1), STRING_ELT(klass, 2)), TRUE);

            let suffix0 = stringSuffix(klass, 0);
            assert_eq!(LENGTH(suffix0), 3);

            let suffix_empty = stringSuffix(klass, 3);
            assert!(suffix_empty.is_null() || suffix_empty == R_NilValue());
        }
    }

    #[test]
    fn test_do_setS4Object_null_args() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = do_setS4Object(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert!(result.is_null() || result == R_NilValue());
        }
    }

    #[test]
    fn test_do_setS4Object_set() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            let flag = Rf_ScalarLogical(TRUE);
            let complete = Rf_ScalarInteger(2);
            let args = Rf_cons(v, Rf_cons(flag, Rf_cons(complete, R_NilValue())));
            let result = do_setS4Object(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(IS_S4_OBJECT(result), TRUE);
        }
    }

    #[test]
    fn test_do_setS4Object_unset() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            SET_S4_OBJECT(v);
            let flag = Rf_ScalarLogical(FALSE);
            let complete = Rf_ScalarInteger(2); // conditional
            let args = Rf_cons(v, Rf_cons(flag, Rf_cons(complete, R_NilValue())));
            let result = do_setS4Object(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            // With complete=2 (conditional), should return unchanged
        }
    }

    #[test]
    fn test_do_objsxp_allocates_bare_object() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = do_objsxp(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::OBJSXP.as_c_int());
            // A bare OBJSXP has no S4 bit (upstream R_allocObject, not
            // allocS4Object): typeof() is "object", isS4() is FALSE.
            assert_eq!(IS_S4_OBJECT(result), FALSE);
        }
    }

    #[test]
    fn test_do_asS4_set() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            let args = Rf_cons(
                v,
                Rf_cons(
                    Rf_ScalarLogical(TRUE),
                    Rf_cons(Rf_ScalarInteger(1), R_NilValue()),
                ),
            );
            let result = do_asS4(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(IS_S4_OBJECT(result), TRUE);
        }
    }

    #[test]
    fn test_do_setClass() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let class_name = Rf_mkString(b"LegacyClass\0".as_ptr() as *const c_char);
            let args = Rf_cons(class_name, R_NilValue());
            let result = do_setClass(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            assert!(s4_class("LegacyClass").is_some());
        }
    }

    #[test]
    fn test_do_setRefClass() {
        let _session = crate::sexp::session::RSession::new();
        let err = assert_r_error(|| unsafe {
            do_setRefClass(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        });
        assert!(err.message.contains("setRefClass is not implemented"));
    }

    #[test]
    fn test_R_S4_method_dispatch() {
        let _session = crate::sexp::session::RSession::new();
        let err = assert_r_error(|| unsafe {
            R_S4_method_dispatch(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        });
        assert!(err.message.contains("S4 method dispatch is not available"));
    }

    #[test]
    fn test_findmethod_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut method: SEXP = ptr::null_mut();
            let result = findmethod(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                b"print\0".as_ptr() as *const c_char,
                &mut method,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(result, -1);
        }
    }

    #[test]
    fn test_DispatchGroup_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = DispatchGroup(
                ptr::null_mut(),
                b"Ops\0".as_ptr() as *const c_char,
                ptr::null_mut(),
                b"+\0".as_ptr() as *const c_char,
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(result, 0);
        }
    }

    #[test]
    fn test_DispatchOrEval_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut ans: SEXP = ptr::null_mut();
            let result = DispatchOrEval_objects(
                ptr::null_mut(),
                ptr::null_mut(),
                b"print\0".as_ptr() as *const c_char,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut ans,
            );
            assert_eq!(result, 0);
        }
    }

    #[test]
    fn test_do_standardGeneric_null() {
        let _session = crate::sexp::session::RSession::new();
        let err = assert_r_error(|| unsafe {
            do_standardGeneric(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
        });
        assert!(err.message.contains("requires a generic function name"));
    }

    #[test]
    fn test_readS3VarsFromFrame_null() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let mut generic: SEXP = ptr::null_mut();
            let mut group: SEXP = ptr::null_mut();
            let mut klass: SEXP = ptr::null_mut();
            let mut method: SEXP = ptr::null_mut();
            let mut callenv: SEXP = ptr::null_mut();
            let mut defenv: SEXP = ptr::null_mut();
            readS3VarsFromFrame(
                ptr::null_mut(),
                &mut generic,
                &mut group,
                &mut klass,
                &mut method,
                &mut callenv,
                &mut defenv,
            );
            // Should not crash
        }
    }

    #[test]
    fn test_R_do_new_object() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = R_do_new_object(ptr::null_mut());
            assert!(result.is_null() || result == R_NilValue());

            register_s4_class(
                "Point".to_string(),
                vec!["x".to_string(), "y".to_string()],
                false,
            );
            let class_def = R_getClassDef(b"Point\0".as_ptr() as *const c_char);
            let result = R_do_new_object(class_def);
            assert_eq!(TYPEOF(result), SEXPTYPE::VECSXP);
            assert_eq!(IS_S4_OBJECT(result), TRUE);
            assert_eq!(LENGTH(result), 2);
        }
    }

    #[test]
    fn test_R_isVirtualClass() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(R_isVirtualClass(ptr::null_mut(), ptr::null_mut()), FALSE);
            register_s4_class("VirtualClass".to_string(), Vec::new(), true);
            let class_def = R_getClassDef(b"VirtualClass\0".as_ptr() as *const c_char);
            assert_eq!(R_isVirtualClass(class_def, ptr::null_mut()), TRUE);
        }
    }

    #[test]
    fn test_R_extends() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(
                R_extends(ptr::null_mut(), ptr::null_mut(), ptr::null_mut()),
                FALSE
            );
            register_s4_class("Ancestor".to_string(), Vec::new(), false);
            register_s4_class_with_extends(
                "Descendant".to_string(),
                Vec::new(),
                vec!["Ancestor".to_string()],
                false,
            );
            let descendant = R_getClassDef(b"Descendant\0".as_ptr() as *const c_char);
            let ancestor = R_getClassDef(b"Ancestor\0".as_ptr() as *const c_char);
            assert_eq!(R_extends(descendant, ancestor, ptr::null_mut()), TRUE);
        }
    }

    #[test]
    fn test_R_set_prim_method() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = R_set_prim_method(
                R_NilValue(),
                ptr::null_mut(),
                R_NilValue(),
                R_NilValue(),
                R_NilValue(),
            );
            assert!(result.is_null() || result == R_NilValue());
        }
    }

    #[test]
    fn test_R_primitive_methods() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = R_primitive_methods(ptr::null_mut());
            assert!(result.is_null() || result == R_NilValue());
        }
    }

    #[test]
    fn test_R_primitive_generic() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = R_primitive_generic(ptr::null_mut());
            assert!(result.is_null() || result == R_NilValue());
        }
    }

    #[test]
    fn test_primitive_methods_roundtrip_by_primitive_offset() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let op = crate::mainutils::names::R_Primitive(c"+".as_ptr());
            assert!(!op.is_null());
            let generic = Rf_ScalarInteger(101);
            let methods = Rf_mkString(c"plus-methods".as_ptr());

            let name = Rf_mkString(c"+".as_ptr());
            R_set_prim_method(name, op, Rf_mkString(c"set".as_ptr()), generic, methods);

            assert_eq!(R_primitive_generic(op), generic);
            assert_eq!(R_primitive_methods(op), methods);

            R_set_standardGeneric_ptr(Some(standard_generic_a), ptr::null_mut());
            assert_eq!(R_has_methods(op), TRUE);

            R_set_prim_method(
                name,
                op,
                Rf_mkString(c"clear".as_ptr()),
                R_NilValue(),
                R_NilValue(),
            );
            assert_eq!(R_primitive_generic(op), R_NilValue());
            assert_eq!(R_primitive_methods(op), R_NilValue());
            assert_eq!(R_has_methods(op), FALSE);
        }
    }

    #[test]
    fn test_R_set_quick_method_check() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            R_set_quick_method_check(None);
            // Should not crash
        }
    }

    #[test]
    fn test_R_getClassDef_R() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = R_getClassDef_R(R_NilValue());
            assert!(result.is_null() || result == R_NilValue());
        }
    }

    #[test]
    fn test_do_nextmethod_null_args() {
        let _session = crate::sexp::session::RSession::new();
        // do_nextmethod panics via R_GlobalContext() with null context,
        // which is expected. Just verify the function signature is valid.
        // We can't easily test it because the panic goes through extern "C"
        // and can't be caught with catch_unwind.
    }

    #[test]
    fn test_do_usemethod_null_args() {
        let _session = crate::sexp::session::RSession::new();
        // do_usemethod panics with null args, so we can't easily test it
        // Just verify it compiles
    }

    #[test]
    fn test_objects_do_unclass_null_args() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let result = objects_do_unclass(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert!(result.is_null() || result == R_NilValue());
        }
    }

    #[test]
    fn test_objects_do_unclass_no_class() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            let args = Rf_cons(v, R_NilValue());
            let result =
                objects_do_unclass(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            // Should return the object unchanged since it has no class
        }
    }

    #[test]
    fn test_objects_do_unclass_with_class() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            let class_vec = Rf_allocVector(SEXPTYPE::STRSXP, 1);
            let _class_vec_guard = protect(class_vec);
            SET_STRING_ELT(
                class_vec,
                0,
                Rf_mkChar(b"myclass\0".as_ptr() as *const c_char),
            );
            setAttrib(v, R_ClassSymbol(), class_vec);
            SET_OBJECT(v, 1); // Ensure OBJECT bit is set for this test
            assert_eq!(isObject(v), TRUE);

            let args = Rf_cons(v, R_NilValue());
            let result =
                objects_do_unclass(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            // Verify class was cleared by reading attribute directly
            assert_eq!(getAttrib(result, R_ClassSymbol()), R_NilValue());
        }
    }

    #[test]
    fn test_inherits3_basic() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            let what = Rf_mkString(b"integer\0".as_ptr() as *const c_char);
            let which = Rf_ScalarLogical(FALSE);
            let result = inherits3(v, what, which);
            assert!(!result.is_null());
            // The class of an integer scalar without explicit class is "integer"
            // So inherits3 should check the implicit class
        }
    }

    #[test]
    fn test_inherits3_which_true() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            let what = Rf_allocVector(SEXPTYPE::STRSXP, 2);
            let _what_guard = protect(what);
            SET_STRING_ELT(what, 0, Rf_mkChar(b"numeric\0".as_ptr() as *const c_char));
            SET_STRING_ELT(what, 1, Rf_mkChar(b"integer\0".as_ptr() as *const c_char));
            let which = Rf_ScalarLogical(TRUE);
            let result = inherits3(v, what, which);
            assert!(!result.is_null());
            assert_eq!(TYPEOF(result), SEXPTYPE::INTSXP);
            assert_eq!(LENGTH(result), 2);
        }
    }

    #[test]
    fn test_inherits3_with_explicit_class() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            let class_vec = Rf_allocVector(SEXPTYPE::STRSXP, 2);
            let _class_vec_guard = protect(class_vec);
            SET_STRING_ELT(class_vec, 0, Rf_mkChar(b"foo\0".as_ptr() as *const c_char));
            SET_STRING_ELT(class_vec, 1, Rf_mkChar(b"bar\0".as_ptr() as *const c_char));
            setAttrib(v, R_ClassSymbol(), class_vec);

            let what = Rf_allocVector(SEXPTYPE::STRSXP, 3);
            let _what_guard = protect(what);
            SET_STRING_ELT(what, 0, Rf_mkChar(b"baz\0".as_ptr() as *const c_char));
            SET_STRING_ELT(what, 1, Rf_mkChar(b"bar\0".as_ptr() as *const c_char));
            SET_STRING_ELT(what, 2, Rf_mkChar(b"foo\0".as_ptr() as *const c_char));
            let which = Rf_ScalarLogical(TRUE);
            let result = inherits3(v, what, which);
            assert!(!result.is_null());
            assert_eq!(LENGTH(result), 3);
            // baz -> not found (0), bar -> found at position 2 (2), foo -> found at position 1 (1)
            assert_eq!(*INTEGER_ELT_mut(result, 0), 0);
            assert_eq!(*INTEGER_ELT_mut(result, 1), 2);
            assert_eq!(*INTEGER_ELT_mut(result, 2), 1);
        }
    }

    #[test]
    fn test_asS4_flag_matches() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            // S4 bit not set, flag=TRUE -> should set it
            let result = asS4(v, TRUE, 2);
            assert_eq!(IS_S4_OBJECT(result), TRUE);

            // S4 bit set, flag=TRUE -> should return unchanged
            let result2 = asS4(result, TRUE, 2);
            assert_eq!(result2, result);
        }
    }

    #[test]
    fn test_asS4_unset_conditional() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            SET_S4_OBJECT(v);
            // complete=2 (conditional) should return unchanged without error
            let result = asS4(v, FALSE, 2);
            assert_eq!(result, v);
            // S4 bit should still be set (conditional mode returns unchanged)
            assert_eq!(IS_S4_OBJECT(result), TRUE);
        }
    }

    #[test]
    fn test_install_pname() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_install(
                std::ffi::CString::new("test_sym")
                    .unwrap_or_default()
                    .as_ptr(),
            );
            assert!(!s.is_null());
            let pname = PRINTNAME(s);
            assert!(!pname.is_null(), "PRINTNAME should not be null");
            let cs = CHAR(pname);
            assert!(!cs.is_null(), "CHAR(PRINTNAME) should not be null");
            let name = std::ffi::CStr::from_ptr(cs).to_str().unwrap_or("");
            assert_eq!(name, "test_sym");
        }
    }

    #[test]
    fn test_do_oldClass_set() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            let class_sym = R_ClassSymbol();
            let class_vec = Rf_allocVector(SEXPTYPE::STRSXP, 1);
            let _class_vec_guard = protect(class_vec);
            SET_STRING_ELT(
                class_vec,
                0,
                Rf_mkChar(b"myclass\0".as_ptr() as *const c_char),
            );
            setAttrib(v, class_sym, class_vec);
            SET_OBJECT(v, 1); // Ensure OBJECT bit is set for this test
            assert_eq!(isObject(v), TRUE);
            let args = Rf_cons(v, Rf_cons(class_vec, R_NilValue()));
            let result = do_oldClass(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            assert_eq!(isObject(CAR(args)), TRUE);
        }
    }

    #[test]
    fn test_do_oldClass_clear() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            let class_sym = R_ClassSymbol();
            let class_vec = Rf_allocVector(SEXPTYPE::STRSXP, 1);
            let _class_vec_guard = protect(class_vec);
            SET_STRING_ELT(
                class_vec,
                0,
                Rf_mkChar(b"myclass\0".as_ptr() as *const c_char),
            );
            setAttrib(v, class_sym, class_vec);
            SET_OBJECT(v, 1); // Ensure OBJECT bit is set for this test

            // Clear the class
            let args = Rf_cons(v, Rf_cons(R_NilValue(), R_NilValue()));
            let result = do_oldClass(ptr::null_mut(), ptr::null_mut(), args, ptr::null_mut());
            assert!(!result.is_null());
            // Class should be cleared
            assert_eq!(getAttrib(v, R_ClassSymbol()), R_NilValue());
        }
    }
}
