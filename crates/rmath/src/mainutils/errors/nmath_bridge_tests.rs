use super::*;
use crate::sexp::{RSession, memory::ArenaBudget};
use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};
#[test]
fn nmath_bridge_actual_storage_is_admitted_through_unwind() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    session.with_active_in(|instance| unsafe {
        (*instance).error_state.nwarnings = 3;
        nmath_warning_hook("warm bridge");
        let ledger = (*instance).heap_identity.byte_ledger();
        let before = ledger.workspace_bytes_for_test();
        let peak = Rc::new(Cell::new(0));
        let observed = peak.clone();
        let once = Rc::new(Cell::new(false));
        let first = once.clone();
        let accounting = ledger.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            if !first.replace(true) {
                observed.set(accounting.workspace_bytes_for_test());
                std::panic::panic_any(713_u32);
            }
        }));
        (*instance).memory_state.gc_force_gap = 1;
        (*instance).memory_state.gc_force_wait = 1;
        let message = "nmath bridge accounting";
        let error = catch_unwind(AssertUnwindSafe(|| nmath_warning_hook(message))).unwrap_err();
        assert_eq!(error.downcast_ref::<u32>(), Some(&713));
        assert!(once.get());
        assert!(
            peak.get() >= before + message.len() + 1,
            "C-string storage must remain admitted through callback; observed {}, before {}",
            peak.get(),
            before
        );
        assert_eq!(ledger.workspace_bytes_for_test(), before);
    });
}

#[test]
fn nmath_bridge_buffer_bytes_overlap_limits_and_loan_recovery() {
    use super::nmath_bridge_buffer::WarningBuffer;
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let ledger = unsafe { (*owner.as_ptr()).heap_identity.byte_ledger() };
    for (source, expected) in [
        ("", ""),
        ("\n", ""),
        ("a\n\n", "a\n"),
        (" a %s %% \n", " a %s %% "),
        ("λé\n", "λé"),
        ("a\0b\n", ""),
    ] {
        let before = ledger.workspace_bytes_for_test();
        let buffer = WarningBuffer::copy(&ledger, source).unwrap();
        assert_eq!(buffer.as_c_str().to_bytes(), expected.as_bytes());
        assert_eq!(
            ledger.workspace_bytes_for_test(),
            before + buffer.capacity()
        );
        drop(buffer);
        assert_eq!(ledger.workspace_bytes_for_test(), before);
    }
    owner
        .with_arena(|arena| {
            let before = arena.workspace_bytes();
            let buffer = WarningBuffer::copy(&ledger, "inside registered arena lend").unwrap();
            assert_eq!(arena.workspace_bytes(), before + buffer.capacity());
            drop(buffer);
            assert_eq!(arena.workspace_bytes(), before);
        })
        .unwrap();
    let first = WarningBuffer::copy(&ledger, "old storage").unwrap();
    let second = "replacement storage longer";
    let limit = ledger.allocated_bytes() + first.capacity() + second.len();
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(limit, 0)))
        .unwrap();
    assert!(WarningBuffer::copy(&ledger, second).is_err());
    assert_eq!(first.as_c_str().to_bytes(), b"old storage");
    assert_eq!(ledger.workspace_bytes_for_test(), first.capacity());
    drop(first);
    let second = WarningBuffer::copy(&ledger, second).unwrap();
    assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), limit);
    drop(second);
    assert_eq!(ledger.workspace_bytes_for_test(), 0);
}

#[test]
fn nmath_bridge_budget_denial_matches_allocation_and_same_cap_retry() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    session.with_active_in(|instance| unsafe {
        (*instance).error_state.nwarnings = 3;
        nmath_warning_hook("warm bridge");
        let ledger = (*instance).heap_identity.byte_ledger();
        let limit = ledger.allocated_bytes() + 256 * 1024;
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(limit, 0)))
            .unwrap();
        let held = ledger.reserve(256 * 1024 - 1).unwrap();
        let callbacks = Rc::new(Cell::new(0));
        let seen = callbacks.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| seen.set(seen.get() + 1)));
        (*instance).memory_state.gc_force_gap = 1;
        (*instance).memory_state.gc_force_wait = 1;
        let denied = catch_unwind(AssertUnwindSafe(|| {
            owner
                .with_arena(|_| nmath_warning_hook("denied bridge storage"))
                .unwrap()
        }))
        .unwrap_err();
        let denied = denied.downcast::<RError>().unwrap();
        assert_eq!(
            callbacks.get(),
            0,
            "admission precedes constructors/callbacks"
        );
        assert_eq!((*instance).error_state.collect_warnings, 1);
        assert_eq!(ledger.workspace_bytes_for_test(), held.bytes());
        let ordinary = catch_unwind(AssertUnwindSafe(|| {
            Rf_allocVector3(SEXPTYPE::RAWSXP, (limit + 1) as _)
        }))
        .unwrap_err();
        let ordinary = ordinary.downcast::<RError>().unwrap();
        assert_eq!(denied.message, ordinary.message);
        assert_eq!(
            denied.message,
            "R allocation failed: memory or node budget exceeded"
        );
        drop(held);
        (*instance).memory_state.gc_force_gap = 0;
        (*instance).memory_state.gc_force_wait = 0;
        nmath_warning_hook("denied bridge storage");
        assert_eq!((*instance).error_state.collect_warnings, 2);
        assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), limit);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
    });
}

#[test]
fn nmath_bridge_revocation_refunds_original_not_replacement() {
    use std::cell::RefCell;
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let weak = facade
        .borrow()
        .as_ref()
        .unwrap()
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap();
    let pin = weak.pin().unwrap();
    let pointer = pin.as_ptr();
    let replacement = Rc::new(RefCell::new(None));
    let after = replacement.clone();
    let revoked = facade.clone();
    let once = Rc::new(Cell::new(false));
    let first = once.clone();
    unsafe {
        crate::sexp::session::with_instance_active(pointer, || {
            (*pointer).error_state.nwarnings = 3;
            nmath_warning_hook("warm bridge");
            let ledger = (*pointer).heap_identity.byte_ledger();
            let before = ledger.workspace_bytes_for_test();
            let accounting = ledger.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if !first.replace(true) {
                    assert!(
                        accounting.workspace_bytes_for_test()
                            >= before + "revoking bridge".len() + 1
                    );
                    drop(revoked.borrow_mut().take());
                    let next = RSession::new_for_gc_tests();
                    let bytes = next
                        .owner_token()
                        .unwrap()
                        .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
                        .unwrap();
                    after.replace(Some((next, bytes)));
                }
            }));
            (*pointer).memory_state.gc_force_gap = 1;
            (*pointer).memory_state.gc_force_wait = 1;
            let error = catch_unwind(AssertUnwindSafe(|| nmath_warning_hook("revoking bridge")))
                .unwrap_err();
            assert_eq!(
                error.downcast_ref::<RError>().unwrap().message,
                crate::sexp::object::SexpError::RootUnavailable.to_string()
            );
            assert!(once.get());
            assert!(!weak.is_live());
            assert_eq!(ledger.workspace_bytes_for_test(), before);
        });
    }
    let result = replacement.borrow();
    let (next, before) = result.as_ref().unwrap();
    next.with_active(|| {
        assert_eq!(
            next.owner_token()
                .unwrap()
                .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
                .unwrap(),
            *before
        )
    });
}

fn legacy_nmath_hook(message: &str) {
    let msg = message.strip_suffix('\n').unwrap_or(message);
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    unsafe {
        let call = mathlib_warning_call();
        if !call.is_null() && call != globals::R_NilValue() {
            Rf_warningcall1(call, c_msg.as_ptr());
        } else {
            Rf_warning1(c_msg.as_ptr());
        }
    }
}

#[test]
fn nmath_bridge_text_attribution_and_warn2_match_legacy() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    session.with_active_in(|instance| unsafe {
        (*instance).error_state.nwarnings = 3;
        let call = owner
            .sexp(Rf_lang2(
                Rf_install(c"attributed_warning".as_ptr()),
                Rf_ScalarInteger(23),
            ))
            .unwrap()
            .into_owned()
            .unwrap();
        let _attribution = mathlib_warning_call_guard(call.as_raw());
        for (source, expected) in [
            ("", ""),
            ("\n", ""),
            ("a\n\n", "a\n"),
            (" a %s %% \n", " a %s %% "),
            ("λé\n", "λé"),
            ("a\0b\n", ""),
        ] {
            for hook in [
                legacy_nmath_hook as fn(&str),
                nmath_warning_hook as fn(&str),
            ] {
                (*instance).error_state.collect_warnings = 0;
                hook(source);
                assert_eq!((*instance).error_state.collect_warnings, 1);
                let warnings = (*instance).error_state.warnings.owned().unwrap();
                assert_eq!(warnings.try_vector_elt(0).unwrap().as_raw(), call.as_raw());
                let names = warnings.try_attrib().unwrap().try_car().unwrap();
                assert_eq!(
                    names.try_string_value_elt(0).unwrap().as_deref(),
                    Some(expected)
                );
                assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
            }
        }
        crate::mainutils::options::SetOptionByName("warn", Rf_ScalarInteger(2));
        R_SetShowErrorMessages(false);
        let mut results = Vec::new();
        for hook in [
            legacy_nmath_hook as fn(&str),
            nmath_warning_hook as fn(&str),
        ] {
            let error = catch_unwind(AssertUnwindSafe(|| hook("converted bridge\n"))).unwrap_err();
            results.push(error.downcast::<RError>().unwrap().message);
            state::clear_last_rendered_message();
            assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        }
        assert_eq!(results[0], results[1]);
        assert_eq!(results[1], "(converted from warning) converted bridge");
    });
}

#[test]
fn nmath_bridge_reclaimable_pressure_retries_same_cap_and_snapshots_attribution() {
    use crate::sexp::{instance::RuntimeValue, object::Sexp};
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    session.with_active_in(|instance| unsafe {
        (*instance).error_state.nwarnings = 3;
        nmath_warning_hook("warm pressure bridge");
        owner.full_gc().unwrap();
        (*instance).error_state.collect_warnings = 0;
        let f = owner.node_factory();
        let mathlib = owner
            .sexp(Rf_lang2(
                Rf_install(c"original_mathlib_call".as_ptr()),
                Rf_ScalarInteger(11),
            ))
            .unwrap()
            .into_owned()
            .unwrap();
        let selected = owner
            .sexp(Rf_lang2(
                Rf_install(c"original_override_call".as_ptr()),
                Rf_ScalarInteger(23),
            ))
            .unwrap()
            .into_owned()
            .unwrap();
        let selected_pointer = selected.as_raw();
        let _mathlib = mathlib_warning_call_guard(mathlib.as_raw());
        let _override = warning_call_guard(selected.as_raw());
        drop(mathlib);
        drop(selected);
        let garbage = f
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::RAWSXP, 32 * 1024)))
            .unwrap();
        drop(garbage);
        let ledger = (*instance).heap_identity.byte_ledger();
        let before = ledger.allocated_bytes();
        let message = "pressure warning";
        let limit = before + message.len();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(limit, 0)))
            .unwrap();
        assert!(matches!(
            super::nmath_bridge_buffer::WarningBuffer::copy(&ledger, message),
            Err(super::nmath_bridge_buffer::BufferError::AdmissionDenied)
        ));
        let observed = Rc::new(Cell::new(false));
        let once = observed.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            if !once.replace(true) {
                (*instance).error_state.mathlib_warning_call = RuntimeValue::empty();
                (*instance).error_state.warning_call = RuntimeValue::empty();
                let owner = crate::sexp::owner::OwnerToken::from_raw(instance);
                let changed = owner
                    .sexp(Rf_lang2(
                        Rf_install(c"changed_override_call".as_ptr()),
                        Rf_ScalarInteger(99),
                    ))
                    .and_then(Sexp::into_owned)
                    .unwrap();
                (*instance).error_state.warning_call = RuntimeValue::from_owned(changed);
                crate::sexp::gengc::full_gc_in(instance);
            }
        }));
        nmath_warning_hook(message);
        assert!(
            observed.get(),
            "bridge must collect reclaimable arena pressure once"
        );
        assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), limit);
        assert!(
            ledger.allocated_bytes() < before,
            "garbage was reclaimed at the unchanged cap"
        );
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        let warnings = (*instance).error_state.warnings.owned().unwrap();
        assert_eq!(
            warnings.try_vector_elt(0).unwrap().as_raw(),
            selected_pointer,
            "entry attribution survives callback replacement and nested collection"
        );
        let names = warnings.try_attrib().unwrap().try_car().unwrap();
        assert_eq!(
            names.try_string_value_elt(0).unwrap().as_deref(),
            Some(message)
        );
    });
}
