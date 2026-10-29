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

    #[test]
    fn owned_s4_validity_replacement_releases_old_root() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let factory = session.owner_token().unwrap().node_factory();
            register_s4_class("OwnedValidityRoot".into(), Vec::new(), false);
            let old = factory.strings(&["old validity method"]).unwrap();
            let node = old.allocation().unwrap().clone();
            let preserved = (*instance).preserve_stack.checked_entries_snapshot().len();
            assert!(set_s4_validity_fn("OwnedValidityRoot", old.as_raw()));
            drop(old);
            crate::sexp::gengc::full_gc_in(instance);
            assert!(node.is_live(), "the stored method must own its root");
            assert!(set_s4_validity_fn("OwnedValidityRoot", R_NilValue()));
            crate::sexp::gengc::full_gc_in(instance);
            assert!(
                !node.is_live(),
                "replacement must release the old validity method"
            );
            assert_eq!(
                (*instance).preserve_stack.checked_entries_snapshot().len(),
                preserved
            );
        });
    }

    #[test]
    fn owned_s4_native_validity_installs_and_releases_transfer_scope() {
        let mut session = crate::sexp::session::RSession::new_for_gc_tests();
        let (method, _, _) =
            session.eval_script_with_output_capture("function(object) return(TRUE)");
        let method = method.unwrap().into_owned().unwrap();
        let factory = session.owner_token().unwrap().node_factory();
        register_s4_class("OwnedNativeValidity".into(), Vec::new(), false);
        assert!(set_s4_validity_fn("OwnedNativeValidity", method.as_raw()));
        let object = factory.strings(&["object"]).unwrap();
        let class = factory.strings(&["OwnedNativeValidity"]).unwrap();
        let environment = session.global_env().unwrap();
        unsafe {
            setAttrib(object.as_raw(), R_ClassSymbol(), class.as_raw());
        }
        let arguments = factory
            .pairlist_cell(&object, &factory.nil(), &factory.nil())
            .unwrap();
        assert!(crate::sexp::transfer::active_owner_pin().is_err());
        let result = factory
            .wrap(unsafe {
                crate::mainutils::essentials::do_validObject(
                    factory.nil().as_raw(),
                    factory.nil().as_raw(),
                    arguments.as_raw(),
                    environment.as_raw(),
                )
            })
            .unwrap();
        assert_eq!(result.try_logical_elt(0).unwrap(), TRUE);
        assert!(
            crate::sexp::transfer::active_owner_pin().is_err(),
            "native execution must release only its own transfer activation"
        );
    }

    #[test]
    fn owned_s4_primitive_dispatch_restores_original_status_after_revocation() {
        use std::{cell::Cell, rc::Rc};
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let factory = session.owner_token().unwrap().node_factory();
            let operation = factory
                .wrap(crate::mainutils::names::R_Primitive(c"+".as_ptr()))
                .unwrap();
            let offset = primitive_offset(operation.as_raw()).unwrap();
            let body = factory.strings(&["unpublished result"]).unwrap();
            let generic = factory
                .wrap(crate::mainutils::dstruct::mkCLOSXP(
                    R_NilValue(),
                    body.as_raw(),
                    R_GlobalEnv(),
                ))
                .unwrap();
            do_set_prim_method(
                operation.as_raw(),
                c"set".as_ptr(),
                generic.as_raw(),
                R_NilValue(),
            );
            let fired = Rc::new(Cell::new(false));
            let observed = fired.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if !observed.replace(true) {
                    (&mut (*instance).objects_state.prim_methods)[offset] =
                        prim_methods_t::SUPPRESSED;
                    crate::sexp::instance::revoke_instance_availability(instance);
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                R_possible_dispatch(
                    R_NilValue(),
                    operation.as_raw(),
                    R_NilValue(),
                    R_GlobalEnv(),
                    TRUE,
                )
            }));
            assert!(
                fired.get(),
                "the method must actually revoke the original runtime during allocation"
            );
            assert!(
                result.is_err(),
                "revoked execution must not publish success"
            );
            assert_eq!(
                (&(*instance).objects_state.prim_methods)[offset],
                prim_methods_t::HAS_METHODS,
                "cleanup must restore the original physical runtime after revocation"
            );
        });
    }

    #[test]
    fn s4_dollar_method_callback_revocation_never_publishes_assignment() {
        use std::{cell::Cell, rc::Rc};

        let mut session = crate::sexp::session::RSession::new();
        let (setup, _, _) = session.eval_code_with_output_capture(
            "methods::setClass('NlenzRevocationBox', contains='environment'); methods::setMethod('$<-', 'NlenzRevocationBox', function(x,name,value) { .GlobalEnv$.__nlenz_method_entered__ <- TRUE; gc(); methods::slot(x, '.xData')[[name]] <- value; x }); z <- methods::new('NlenzRevocationBox'); .__nlenz_method_entered__ <- FALSE",
        );
        assert!(setup.is_ok(), "S4 method setup failed: {setup:?}");

        let owner = session.owner_token().unwrap();
        let weak = owner.weak_owner().unwrap();
        let instance = session.with_active_in(|instance| instance);
        let entered = session.with_active_in(|_| unsafe {
            crate::sexp::symbol::Rf_install(c".__nlenz_method_entered__".as_ptr())
        });
        let revoked_from_method = Rc::new(Cell::new(false));
        let callback_observed = Rc::clone(&revoked_from_method);
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| unsafe {
            let value = crate::sexp::envir::R_findVarInFrame((*instance).global_env, entered);
            if !value.is_null()
                && value != crate::sexp::globals::R_UnboundValue()
                && crate::sexp::accessors::TYPEOF(value) == SEXPTYPE::LGLSXP
                && crate::sexp::accessors::LOGICAL_ELT(value, 0) == TRUE
            {
                callback_observed.set(true);
                crate::sexp::instance::revoke_instance_availability(instance);
            }
        }));

        let evaluation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session
                .eval_code_with_output_capture("z$a <- 7L")
                .0
                .is_err()
        }));
        assert!(revoked_from_method.get(), "the explicit method GC callback must revoke its original owner");
        assert!(!weak.is_live(), "the callback must revoke the original session capability");
        assert!(
            evaluation.is_err() || evaluation.is_ok_and(|was_error| was_error),
            "a revoked assignment must not publish success"
        );

        // A fresh session created after the callback revokes its owner must
        // retain an independent method table and remain usable through GC.
        let mut replacement = crate::sexp::session::RSession::new();
        let (replacement_setup, _, _) = replacement.eval_code_with_output_capture(
            "methods::setClass('NlenzReplacementBox', contains='environment'); methods::setMethod('$<-', 'NlenzReplacementBox', function(x,name,value) { gc(); methods::slot(x, '.xData')[[name]] <- 19L; x }); z <- methods::new('NlenzReplacementBox')",
        );
        assert!(replacement_setup.is_ok(), "replacement session setup failed: {replacement_setup:?}");
        let (replacement_result, replacement_output, _) = replacement.eval_code_with_output_capture(
            "z$a <- 1L; get('a', methods::slot(z, '.xData'))",
        );
        assert!(replacement_result.is_ok(), "replacement session dispatch failed: {replacement_result:?}");
        assert_eq!(replacement_result.unwrap().integer_elt(0), Some(19), "replacement method returned unexpected value: {replacement_output:?}");
    }

    #[test]
    fn owned_s4_extends_cache_releases_bindings_and_table() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let factory = session.owner_token().unwrap().node_factory();
            let value = factory.strings(&["OwnedExtends", "parent"]).unwrap();
            let value_node = value.allocation().unwrap().clone();
            let preserved = (*instance).preserve_stack.checked_entries_snapshot().len();
            cache_class("OwnedExtends", value.as_raw());
            let table_node = (*instance)
                .objects_state
                .s4_extends_table
                .owned()
                .unwrap()
                .allocation()
                .unwrap()
                .clone();
            drop(value);
            crate::sexp::gengc::full_gc_in(instance);
            assert!(value_node.is_live());
            assert!(table_node.is_live());
            cache_class("OwnedExtends", R_NilValue());
            crate::sexp::gengc::full_gc_in(instance);
            assert!(
                !value_node.is_live(),
                "removing the binding must release its vector"
            );
            (*instance).objects_state.s4_extends_table =
                crate::sexp::instance::RuntimeValue::empty();
            crate::sexp::gengc::full_gc_in(instance);
            assert!(
                !table_node.is_live(),
                "clearing the cache must release its environment"
            );
            assert_eq!(
                (*instance).preserve_stack.checked_entries_snapshot().len(),
                preserved
            );
        });
    }

    #[test]
    fn owned_s4_primitive_tables_release_after_clear() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let factory = session.owner_token().unwrap().node_factory();
            let operation = factory
                .wrap(crate::mainutils::names::R_Primitive(c"+".as_ptr()))
                .unwrap();
            let op = operation.as_raw();
            let generic = factory
                .wrap(crate::mainutils::dstruct::mkCLOSXP(
                    R_NilValue(),
                    R_NilValue(),
                    R_GlobalEnv(),
                ))
                .unwrap();
            let methods = factory.strings(&["owned methods"]).unwrap();
            let generic_node = generic.allocation().unwrap().clone();
            let methods_node = methods.allocation().unwrap().clone();
            let preserved = (*instance).preserve_stack.checked_entries_snapshot().len();
            assert!(
                do_set_prim_method(op, c"set".as_ptr(), generic.as_raw(), methods.as_raw())
                    .is_null()
            );
            drop(generic);
            drop(methods);
            crate::sexp::gengc::full_gc_in(instance);
            assert!(generic_node.is_live());
            assert!(methods_node.is_live());
            let previous = factory
                .wrap(do_set_prim_method(
                    op,
                    c"clear".as_ptr(),
                    R_NilValue(),
                    R_NilValue(),
                ))
                .unwrap();
            assert_eq!(previous.allocation().unwrap().link(), generic_node.link());
            drop(previous);
            crate::sexp::gengc::full_gc_in(instance);
            assert!(!generic_node.is_live());
            assert!(!methods_node.is_live());
            assert_eq!(
                (*instance).preserve_stack.checked_entries_snapshot().len(),
                preserved
            );
        });
    }

    #[test]
    fn owned_s4_primitive_dispatch_survives_collecting_cache_eviction() {
        use std::{cell::Cell, rc::Rc};
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let factory = session.owner_token().unwrap().node_factory();
            let body = factory.strings(&["selected generic"]).unwrap();
            let generic = factory
                .wrap(crate::mainutils::dstruct::mkCLOSXP(
                    R_NilValue(),
                    body.as_raw(),
                    R_GlobalEnv(),
                ))
                .unwrap();
            let generic_node = generic.allocation().unwrap().clone();
            let op = crate::mainutils::names::R_Primitive(c"+".as_ptr());
            do_set_prim_method(op, c"set".as_ptr(), generic.as_raw(), R_NilValue());
            drop(generic);
            drop(body);
            let evicted = Rc::new(Cell::new(false));
            let observed = evicted.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if !observed.replace(true) {
                    (*instance).objects_state.prim_generics.clear();
                    (*instance).objects_state.prim_mlist.clear();
                    (*instance).memory_state.gc_force_gap = 0;
                    (*instance).memory_state.gc_force_wait = 0;
                    crate::sexp::gengc::full_gc_in(instance);
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let result = factory
                .wrap(R_possible_dispatch(
                    R_NilValue(),
                    op,
                    R_NilValue(),
                    R_GlobalEnv(),
                    TRUE,
                ))
                .unwrap();
            assert!(
                evicted.get(),
                "the selected generic must actually lose its cache root"
            );
            assert_eq!(
                result.try_string_value_elt(0).unwrap().as_deref(),
                Some("selected generic")
            );
            crate::sexp::gengc::full_gc_in(instance);
            assert!(
                !generic_node.is_live(),
                "the execution snapshot must release its closure after returning"
            );
        });
    }

    #[test]
    fn owned_s4_validity_survives_collecting_cache_eviction() {
        use std::{cell::Cell, rc::Rc};
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let factory = session.owner_token().unwrap().node_factory();
            register_s4_class("OwnedValidityCall".into(), Vec::new(), false);
            let object = factory.strings(&["validity argument"]).unwrap();
            let class = factory.strings(&["OwnedValidityCall"]).unwrap();
            setAttrib(object.as_raw(), R_ClassSymbol(), class.as_raw());
            let symbol = factory.wrap(Rf_install(c"object".as_ptr())).unwrap();
            let formals = factory
                .pairlist_cell(&factory.missing(), &factory.nil(), &symbol)
                .unwrap();
            let method = factory
                .wrap(crate::mainutils::dstruct::mkCLOSXP(
                    formals.as_raw(),
                    factory.domain().logical(true).as_raw(),
                    R_GlobalEnv(),
                ))
                .unwrap();
            let method_node = method.allocation().unwrap().clone();
            set_s4_validity_fn("OwnedValidityCall", method.as_raw());
            drop(method);
            drop(formals);
            let arguments = factory
                .pairlist_cell(&object, &factory.nil(), &factory.nil())
                .unwrap();
            let evicted = Rc::new(Cell::new(false));
            let observed = evicted.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if !observed.replace(true) {
                    (*instance).objects_state.s4_validity.clear();
                    (*instance).memory_state.gc_force_gap = 0;
                    (*instance).memory_state.gc_force_wait = 0;
                    crate::sexp::gengc::full_gc_in(instance);
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let result = factory
                .wrap(crate::mainutils::essentials::do_validObject(
                    R_NilValue(),
                    R_NilValue(),
                    arguments.as_raw(),
                    R_GlobalEnv(),
                ))
                .unwrap();
            assert!(
                evicted.get(),
                "the validity method must actually lose its cache root"
            );
            assert_eq!(result.try_logical_elt(0).unwrap(), TRUE);
            crate::sexp::gengc::full_gc_in(instance);
            assert!(
                !method_node.is_live(),
                "the validity execution snapshot must release its closure"
            );
        });
    }

    fn assert_r_error(action: impl FnOnce()) -> crate::sexp::context::RError {
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action))
            .expect_err("expected RError panic");
        payload
            .downcast_ref::<crate::sexp::context::RError>()
            .expect("expected RError payload")
            .clone()
    }

    #[test]
    fn owned_s4_native_admission_matches_gnu() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        let factory = session.owner_token().unwrap().node_factory();
        let name = factory.strings(&["AbsentCacheProbe"]).unwrap();
        let empty = factory.strings(&[]).unwrap();
        let invalid = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
            .unwrap();
        let nil = factory.nil();
        use crate::library::methods::methods_list_dispatch::R_getClassFromCache;
        // SAFETY: every native argument retains its original checked root or
        // singleton lease. Error unwinding crosses only Rust frames here.
        unsafe {
            assert_eq!(
                assert_r_error(|| {
                    R_getClassFromCache(name.as_raw(), nil.as_raw());
                })
                .message,
                "use of NULL environment is defunct"
            );
            assert_eq!(
                assert_r_error(|| {
                    R_getClassFromCache(invalid.as_raw(), nil.as_raw());
                })
                .message,
                "class should be either a character-string name or a class definition"
            );
            assert_eq!(
                R_getClassFromCache(empty.as_raw(), nil.as_raw()),
                nil.as_raw()
            );
            assert_eq!(
                assert_r_error(|| {
                    R_do_new_object(nil.as_raw());
                })
                .message,
                "cannot get a slot (\"virtual\") from an object of type \"NULL\""
            );
            assert_eq!(
                assert_r_error(|| {
                    R_do_new_object(ptr::null_mut());
                })
                .message,
                "C level NEW macro called with null class definition pointer"
            );
        }
        session.gc();
        assert_eq!(
            name.string_value_elt(0),
            Some(Some("AbsentCacheProbe".to_owned()))
        );
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
        // Sessions load the methods package at startup (GNU R with methods
        // attached), so table dispatch is registered from the start.
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            assert_eq!(isMethodsDispatchOn(), TRUE);
        }
    }

    #[test]
    fn test_R_set_standardGeneric_ptr_roundtrip() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            // The startup methods load already installed a dispatcher.
            let old = R_set_standardGeneric_ptr(None, ptr::null_mut());
            assert!(old.is_some());
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
        let left = crate::sexp::session::RSession::new();
        let right = crate::sexp::session::RSession::new();

        left.with_active(|| unsafe {
            let _ = R_set_standardGeneric_ptr(Some(standard_generic_a), ptr::null_mut());
            assert_eq!(isMethodsDispatchOn(), TRUE);
        });

        right.with_active(|| unsafe {
            assert_eq!(isMethodsDispatchOn(), TRUE);
            let _ = R_set_standardGeneric_ptr(Some(standard_generic_b), ptr::null_mut());
            assert_eq!(isMethodsDispatchOn(), TRUE);
        });

        left.with_active(|| unsafe {
            assert_eq!(isMethodsDispatchOn(), TRUE);
            assert!(R_set_standardGeneric_ptr(None, ptr::null_mut()).is_some());
            assert_eq!(isMethodsDispatchOn(), FALSE);
        });

        right.with_active(|| unsafe {
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
            // GNU objects.c: with methods dispatch on, a NULL op (or closure)
            // reports TRUE without consulting the primitive tables.
            assert_eq!(R_has_methods(ptr::null_mut()), TRUE);
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
    fn test_make_class_by_name() {
        let mut session = crate::sexp::session::RSession::new();
        let (definition, _, _) = session.eval_script_with_output_capture(
            "setClass('MakeClassProbe', slots=c(x='numeric')); getClassDef('MakeClassProbe')",
        );
        let definition = definition.unwrap().into_owned().unwrap();
        let result = make_class_by_name("MakeClassProbe");
        assert_eq!(result, definition);
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
        // setClass now routes through the methods package closure (GNU:
        // setClass is pure R), so the R-level body needs an active session
        // pin, just like a session-level eval.
        let session = crate::sexp::session::RSession::new();
        session.with_active(|| unsafe {
            let class_name = Rf_mkString(b"LegacyClass\0".as_ptr() as *const c_char);
            let args = Rf_cons(class_name, R_NilValue());
            let result = do_setClass(ptr::null_mut(), ptr::null_mut(), args, R_GlobalEnv());
            assert!(!result.is_null());
            // GNU setClass returns the class generator function; the class
            // definition now lives in the methods package's R-level class
            // table rather than the native registry.
            assert_eq!(Rf_isFunction(result), 1);
        });
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
            // A NULL object has no class, so the generic's `default` method
            // (print.default, found via the base search added for S3
            // dispatch) is the lookup result: 0 = default in this port's
            // 1-based findmethod contract.
            assert_eq!(result, 0);
            assert!(!method.is_null() && method != R_NilValue());
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
        use crate::mainutils::{coerce::asReal, essentials::R_do_slot};

        let mut session = crate::sexp::session::RSession::new();
        unsafe {
            // GNU objects.c R_do_new_object errors on a null class
            // definition pointer ("C level NEW macro called with null class
            // definition pointer").
            let err = assert_r_error(|| unsafe {
                R_do_new_object(ptr::null_mut());
            });
            assert!(err.message.contains("null class definition pointer"));
        }
        let (definition, _, _) = session.eval_script_with_output_capture(
            "setClass('PointPrototypeUnit', slots=c(x='numeric', y='numeric'), prototype=prototype(x=1, y=2)); getClassDef('PointPrototypeUnit')",
        );
        let definition = definition.unwrap().into_owned().unwrap();
        // SAFETY: the actual class definition retains its checked root; no
        // payload loan overlaps construction or slot reads.
        unsafe {
            let result = R_do_new_object(definition.as_raw());
            let rooted = session.sexp(result).unwrap();
            let result = rooted.as_raw();
            assert_eq!(IS_S4_OBJECT(result), TRUE);
            assert_eq!(asReal(R_do_slot(result, Rf_install(c"x".as_ptr()))), 1.0);
            assert_eq!(asReal(R_do_slot(result, Rf_install(c"y".as_ptr()))), 2.0);
        }
    }

    #[test]
    fn test_R_set_prim_method() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let code = Rf_mkString(c"clear".as_ptr());
            let _code = crate::sexp::protect::protect(code);
            let result =
                R_set_prim_method(R_NilValue(), R_NilValue(), code, R_NilValue(), R_NilValue());
            assert_eq!(TYPEOF(result), SEXPTYPE::LGLSXP);
            let _ = R_set_prim_method(
                R_NilValue(),
                R_NilValue(),
                Rf_mkString(c"set".as_ptr()),
                R_NilValue(),
                R_NilValue(),
            );
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
            let generic =
                crate::mainutils::dstruct::mkCLOSXP(R_NilValue(), R_NilValue(), R_GlobalEnv());
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
    fn as_s4_data_conversion_preserves_class_and_source_through_gc() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let factory = session.owner_token().unwrap().node_factory();
            let input = factory.wrap(Rf_ScalarInteger(42)).unwrap();
            let s3_class = factory.strings(&["Restored", "integer"]).unwrap();
            let marker = factory.strings(&["retained attribute"]).unwrap();
            let s3_symbol = Rf_install(c".S3Class".as_ptr());
            let marker_symbol = Rf_install(c"marker".as_ptr());
            setAttrib(input.as_raw(), s3_symbol, s3_class.as_raw());
            setAttrib(input.as_raw(), marker_symbol, marker.as_raw());
            SET_S4_OBJECT(input.as_raw());
            SET_NAMED(input.as_raw(), 2);
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let output = factory.wrap(asS4(input.as_raw(), FALSE, 1)).unwrap();
            crate::sexp::gengc::full_gc_in(instance);
            assert_ne!(output.as_raw(), input.as_raw());
            assert_eq!(IS_S4_OBJECT(input.as_raw()), TRUE);
            assert_eq!(IS_S4_OBJECT(output.as_raw()), FALSE);
            assert_eq!(
                getAttrib(output.as_raw(), R_ClassSymbol()),
                s3_class.as_raw()
            );
            assert_eq!(getAttrib(output.as_raw(), marker_symbol), marker.as_raw());
            assert_eq!(getAttrib(output.as_raw(), s3_symbol), R_NilValue());
            assert_eq!(getAttrib(input.as_raw(), s3_symbol), s3_class.as_raw());
            SET_INTEGER_ELT(output.as_raw(), 0, 99);
            assert_eq!(INTEGER_ELT(input.as_raw(), 0), 42);
        });
    }

    #[test]
    fn as_s4_rejects_foreign_input_before_header_access() {
        let original = crate::sexp::session::RSession::new_for_gc_tests();
        let foreign = original.with_active_in(|_| unsafe {
            original
                .owner_token()
                .unwrap()
                .node_factory()
                .wrap(Rf_ScalarInteger(42))
                .unwrap()
        });
        let other = crate::sexp::session::RSession::new_for_gc_tests();
        other.with_active_in(|_| {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                asS4(foreign.as_raw(), TRUE, 0)
            }));
            assert!(result.is_err());
        });
        original.with_active_in(|_| unsafe {
            assert_eq!(IS_S4_OBJECT(foreign.as_raw()), FALSE);
        });
    }

    #[test]
    fn as_s4_conversion_rejects_owner_revocation_during_copy() {
        use std::{cell::Cell, rc::Rc};
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let factory = session.owner_token().unwrap().node_factory();
            let input = factory.wrap(Rf_ScalarInteger(42)).unwrap();
            SET_S4_OBJECT(input.as_raw());
            SET_NAMED(input.as_raw(), 2);
            let fired = Rc::new(Cell::new(false));
            let observed = fired.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if !observed.replace(true) {
                    crate::sexp::instance::revoke_instance_availability(instance);
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                asS4(input.as_raw(), FALSE, 1)
            }));
            assert!(
                fired.get(),
                "copy must actually reach the revocation callback"
            );
            assert!(
                result.is_err(),
                "revoked conversion must not publish a result"
            );
        });
    }

    #[test]
    fn as_s4_data_slot_respects_requested_type_and_object_mode() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active_in(|_| unsafe {
            let factory = session.owner_token().unwrap().node_factory();
            let basic = factory.wrap(Rf_ScalarInteger(42)).unwrap();
            SET_S4_OBJECT(basic.as_raw());
            assert_eq!(
                crate::mainutils::subassign::R_getS4DataSlot(
                    basic.as_raw(),
                    SEXPTYPE::OBJSXP.into()
                ),
                R_NilValue()
            );
            assert_eq!(IS_S4_OBJECT(basic.as_raw()), TRUE);
            let class = factory.strings(&["integer"]).unwrap();
            setAttrib(
                basic.as_raw(),
                Rf_install(c".S3Class".as_ptr()),
                class.as_raw(),
            );
            let restored = factory
                .wrap(crate::mainutils::subassign::R_getS4DataSlot(
                    basic.as_raw(),
                    SEXPTYPE::OBJSXP.into(),
                ))
                .unwrap();
            // OBJSXP requests restore an S3 class; they do not require an
            // OBJSXP representation for a valid basic data part.
            assert_eq!(TYPEOF(restored.as_raw()), SEXPTYPE::INTSXP);
            assert_eq!(IS_S4_OBJECT(restored.as_raw()), FALSE);
            let wrapper = factory.wrap(R_allocObject()).unwrap();
            SET_S4_OBJECT(wrapper.as_raw());
            let data = factory.wrap(Rf_ScalarInteger(7)).unwrap();
            setAttrib(
                wrapper.as_raw(),
                Rf_install(c".Data".as_ptr()),
                data.as_raw(),
            );
            assert_eq!(
                crate::mainutils::subassign::R_getS4DataSlot(
                    wrapper.as_raw(),
                    SEXPTYPE::INTSXP.into()
                ),
                data.as_raw()
            );
            assert_eq!(
                crate::mainutils::subassign::R_getS4DataSlot(
                    wrapper.as_raw(),
                    SEXPTYPE::ENVSXP.into()
                ),
                R_NilValue()
            );
        });
    }

    #[test]
    fn as_s4_slot_only_conditional_remains_marked() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active_in(|_| unsafe {
            let factory = session.owner_token().unwrap().node_factory();
            let input = factory.wrap(R_allocObject()).unwrap();
            SET_S4_OBJECT(input.as_raw());
            let result = asS4(input.as_raw(), FALSE, 2);
            assert_eq!(result, input.as_raw());
            assert_eq!(IS_S4_OBJECT(result), TRUE);
        });
    }
    #[test]
    fn test_asS4_unset_conditional() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let v = Rf_ScalarInteger(42);
            SET_S4_OBJECT(v);
            // Conditional conversion accepts a basic data part.
            let result = asS4(v, FALSE, 2);
            assert_eq!(result, v);
            // Basic data parts lose their S4 marker.
            assert_eq!(IS_S4_OBJECT(result), FALSE);
        }
    }

    #[test]
    fn test_install_pname() {
        let _session = crate::sexp::session::RSession::new();
        unsafe {
            let s = Rf_install(c"test_sym".as_ptr());
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
