//! The default renderer owns its wire bytes and restores its original runtime.
use super::*;
use crate::sexp::{context, session::RSession};
use std::{
    cell::{Cell, RefCell},
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

fn deparse_callback(action: u8, stop_condition: bool) {
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let foreign = Rc::new(RefCell::new(None::<RSession>));
    let foreign_bytes = Rc::new(Cell::new(0usize));
    let weak = facade
        .borrow()
        .as_ref()
        .unwrap()
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap();
    let pin = weak.pin().unwrap();
    let instance = pin.as_ptr();
    unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            let call = context::own_control_value(Rf_lang2(
                Rf_install(c"render_target".as_ptr()),
                Rf_install(c"x".as_ptr()),
            ));
            let stop_message = if stop_condition {
                let factory = crate::sexp::owner::OwnerToken::from_raw(instance).node_factory();
                let character = factory
                    .allocate(|a| {
                        crate::sexp::encoding::allocate_character(
                            a,
                            b"a\xffb",
                            crate::sexp::encoding::TextEncoding::Native,
                        )
                    })
                    .unwrap();
                let mut message = crate::sexp::object::SexpMut::try_from_checked(
                    factory
                        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::STRSXP, 1)))
                        .unwrap(),
                )
                .unwrap();
                message.try_set_string_elt(0, character).unwrap();
                // The selected message need not be the first condition field.
                let condition = factory
                    .allocate(|a| Some(a.alloc_vector(SEXPTYPE::VECSXP, 1)))
                    .unwrap();
                Some((message.freeze(), condition))
            } else {
                None
            };
            R_SetShowErrorMessages(false);
            R_SetExpressions(503);
            R_SetErrmessage_c(c"a\xffb".as_ptr());
            let heap = call.allocation().unwrap().heap_identity();
            let observed_heap = heap.clone();
            let seen = Rc::new(Cell::new(0));
            let called = seen.clone();
            let revoker = Rc::downgrade(&facade);
            let other = foreign.clone();
            let other_bytes = foreign_bytes.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if called.get() != 0 {
                    return;
                }
                for (_, node) in
                    crate::sexp::memory::fresh_allocation_roots_for_marking(&observed_heap)
                {
                    let header = observed_heap.node_snapshot(&node).unwrap();
                    if header.sxpinfo.type_of() != SEXPTYPE::CHARSXP {
                        continue;
                    }
                    let payload = observed_heap.payload_lease(&node).unwrap();
                    if payload.byte_elt(0) != Some(b'r') || payload.byte_elt(13) != Some(b'(') {
                        continue;
                    }
                    called.set(1);
                    (*instance).memory_state.gc_force_gap = 0;
                    R_SetErrmessage("mutated during real deparse publication");
                    R_SetExpressions(991);
                    crate::sexp::gengc::full_gc();
                    match action {
                        1 => std::panic::panic_any(991u32),
                        2 => drop(revoker.upgrade().unwrap().borrow_mut().take()),
                        3 => {
                            let session = RSession::new_for_gc_tests();
                            session.with_active(|| {
                                R_SetInError(73);
                                R_SetExpressions(812);
                                R_SetErrmessage("foreign error state");
                            });
                            other_bytes.set(session.with_active(|| {
                                session
                                    .owner_token()
                                    .unwrap()
                                    .with_arena(|a| a.total_bytes_allocated() + a.workspace_bytes())
                                    .unwrap()
                            }));
                            *other.borrow_mut() = Some(session);
                            drop(revoker.upgrade().unwrap().borrow_mut().take());
                        }
                        _ => {}
                    }
                    break;
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            // The native format pointer aliases the exact state being rendered.
            // It must be snapshotted before any mutable error-state lend.
            let format = R_curErrorBuf();
            let result = catch_unwind(AssertUnwindSafe(|| {
                if let Some((message, condition)) = &stop_message {
                    conditions::signal_stop_condition(
                        condition.as_raw(),
                        call.as_raw(),
                        message.as_raw(),
                    );
                } else {
                    render::verrorcall_dflt(call.as_raw(), format, ptr::null_mut());
                }
            }));
            assert_eq!(
                seen.get(),
                1,
                "actual deparse character constructor reached"
            );
            assert_eq!((*instance).memory_state.in_gc, 0);
            assert_eq!((*instance).error_state.in_error, 0);
            assert_eq!((*instance).error_state.expressions, 503);
            assert_eq!(heap.byte_ledger().workspace_bytes_for_test(), 0);
            if action == 1 {
                assert_eq!(*result.unwrap_err().downcast::<u32>().unwrap(), 991);
            } else {
                let error = result.unwrap_err();
                let error = error.downcast_ref::<context::RError>().unwrap();
                if action == 0 {
                    assert_eq!(error.message, "a\\xffb");
                    let snapshot = state::error_buffer_snapshot();
                    assert_eq!(snapshot.as_bytes(), b"Error in render_target(x) : a\xffb\n");
                    assert_eq!(R_GetErrorBuf(), "Error in render_target(x) : a\\xffb\n");
                } else {
                    assert!(!weak.is_live());
                }
            }
            if let Some(other) = foreign.borrow().as_ref() {
                other.with_active(|| {
                    assert_eq!(R_GetInError(), 73);
                    assert_eq!(R_Expressions(), 812);
                    assert_eq!(R_GetErrorBuf(), "foreign error state");
                    assert_eq!(
                        other
                            .owner_token()
                            .unwrap()
                            .with_arena(|a| a.total_bytes_allocated() + a.workspace_bytes())
                            .unwrap(),
                        foreign_bytes.get(),
                        "default rendering cannot allocate in a replacement runtime"
                    );
                });
            }
        });
    }
}
#[test]
fn owned_default_byte_self_buffer_and_real_deparse_gc_keep_original_message() {
    deparse_callback(0, false);
}
#[test]
fn owned_default_byte_real_deparse_panic_restores_original_flags() {
    deparse_callback(1, false);
}
#[test]
fn owned_default_byte_real_deparse_revocation_restores_original_flags() {
    deparse_callback(2, false);
}
#[test]
fn owned_default_byte_real_deparse_revocation_cannot_restore_into_foreign_runtime() {
    deparse_callback(3, false);
}

#[test]
fn owned_default_byte_clipping_repairs_only_cut_suffix_and_preserves_complete_invalid_tail() {
    let complete = byte_buffer::Snapshot::raw(|i| b"a\xff".get(i).copied()).clipped(3, true);
    assert_eq!(complete.as_bytes(), b"a\xff");
    let clipped = byte_buffer::Snapshot::raw(|i| b"a\x80\x80z".get(i).copied()).clipped(3, true);
    assert_eq!(clipped.as_bytes(), b"a");
    let native = byte_buffer::Snapshot::raw(|i| b"a\x80\x80z".get(i).copied()).clipped(3, false);
    assert_eq!(native.as_bytes(), b"a\x80\x80");
}

#[test]
fn owned_default_byte_selected_stop_message_survives_real_deparse_gc() {
    deparse_callback(0, true);
}
#[test]
fn owned_default_byte_selected_stop_message_panic_restores_original_flags() {
    deparse_callback(1, true);
}
#[test]
fn owned_default_byte_selected_stop_message_revocation_restores_original_flags() {
    deparse_callback(2, true);
}
#[test]
fn owned_default_byte_selected_stop_message_cannot_publish_in_foreign_runtime() {
    deparse_callback(3, true);
}
