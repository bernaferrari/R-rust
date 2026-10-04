use super::*;
use crate::sexp::accessors::{ALTREP, DATAPTR, INTEGER_ELT, REAL_ELT};
use crate::sexp::instance::RInstance;
use crate::sexp::memory::ArenaBudget;
use crate::sexp::session::RSession;
use std::panic::{AssertUnwindSafe, catch_unwind};

fn assert_r_error(f: impl FnOnce()) {
    let error = catch_unwind(AssertUnwindSafe(f))
        .expect_err("nonempty payload access must raise an R error");
    assert!(
        error
            .downcast_ref::<crate::sexp::context::RError>()
            .is_some()
    );
}

#[test]
fn compact_payload_budget_denial_precedes_allocator_access() {
    let session = RSession::new_for_gc_tests();
    let seq = session.with_active(|| unsafe { compact_int_seq(1, 1, 8) });
    let _root = session.sexp(seq).unwrap();
    session.with_active_in(|owner| unsafe {
        (*owner).arena.set_budget(ArenaBudget::new(1, 0));
    });
    let before = memory::buffer_allocation_attempts();
    session.with_active(|| unsafe { materialize(seq) });
    assert_eq!(memory::buffer_allocation_attempts(), before);
    unsafe {
        assert!((*seq).payload.is_empty());
        assert_eq!(ALTREP(seq), 1);
        assert_eq!(INTEGER_ELT(seq, 7), 8);
    }
}

#[test]
fn compact_raw_payload_failure_raises_r_error_and_can_retry() {
    let session = RSession::new_for_gc_tests();
    let seq = session.with_active(|| unsafe { compact_real_seq(1.5, 0.25, 8) });
    let _root = session.sexp(seq).unwrap();
    session.with_active_in(|owner| unsafe {
        (*owner).arena.set_budget(ArenaBudget::new(1, 0));
    });
    assert_r_error(|| {
        session.with_active(|| unsafe {
            let _ = DATAPTR(seq);
        })
    });
    unsafe {
        assert!((*seq).payload.is_empty());
        assert_eq!(ALTREP(seq), 1);
        assert_eq!(REAL_ELT(seq, 7), 3.25);
    }
    session.with_active_in(|owner| unsafe {
        (*owner).arena.set_budget(ArenaBudget::unlimited());
    });
    session.with_active(|| unsafe {
        let ptr = DATAPTR(seq).cast::<c_double>();
        assert!(!ptr.is_null());
        assert_eq!(*ptr.add(7), 3.25);
        assert_eq!(ALTREP(seq), 0);
        assert!(memory::vector_payload_is_tracked(seq));
    });
}

#[test]
fn compact_payload_nested_lends_use_original_budget_when_refusing() {
    let session = RSession::new_for_gc_tests();
    let seq = session.with_active(|| unsafe { compact_int_seq(1, 1, 8) });
    let _root = session.sexp(seq).unwrap();
    session.with_active_in(|owner| unsafe {
        (*owner).arena.set_budget(ArenaBudget::new(1, 0));
    });
    let mut other = RInstance::new_for_gc_tests();
    assert_r_error(|| {
        session.with_active_in(|owner| unsafe {
            memory::with_arena_in(owner, |_arena| {
                memory::with_arena_in(std::ptr::addr_of_mut!(other), |_other_arena| {
                    let _ = DATAPTR(seq);
                });
            });
        })
    });
    unsafe {
        assert!((*seq).payload.is_empty());
        assert_eq!(ALTREP(seq), 1);
        assert_eq!(INTEGER_ELT(seq, 7), 8);
    }
    session.with_active_in(|owner| unsafe {
        assert!(!memory::is_arena_lent(owner));
        (*owner).arena.set_budget(ArenaBudget::unlimited());
    });
    session.with_active(|| unsafe {
        assert!(!DATAPTR(seq).is_null());
    });
}

#[test]
fn compact_payload_nested_lends_ignore_foreign_budget_and_commit_to_owner() {
    let session = RSession::new_for_gc_tests();
    let seq = session.with_active(|| unsafe { compact_int_seq(10, 2, 8) });
    let _root = session.sexp(seq).unwrap();
    let mut other = RInstance::new_for_gc_tests();
    other.arena.set_budget(ArenaBudget::new(1, 0));
    let other_bytes = other.arena.total_bytes_allocated();
    let owner_bytes =
        session.with_active_in(|owner| unsafe { (*owner).arena.total_bytes_allocated() });
    session.with_active_in(|owner| unsafe {
        memory::with_arena_in(owner, |_arena| {
            memory::with_arena_in(std::ptr::addr_of_mut!(other), |_other_arena| {
                let ptr = DATAPTR(seq).cast::<c_int>();
                assert!(!ptr.is_null());
                assert_eq!(*ptr.add(7), 24);
                assert!(memory::vector_payload_is_tracked(seq));
            });
        });
        assert_eq!(
            (*owner).arena.total_bytes_allocated(),
            owner_bytes + 8 * std::mem::size_of::<c_int>()
        );
        assert!(memory::vector_payload_is_tracked(seq));
    });
    assert_eq!(other.arena.total_bytes_allocated(), other_bytes);
    session.with_active_in(|owner| unsafe {
        crate::sexp::gengc::full_gc_in(owner);
    });
    assert_eq!(unsafe { INTEGER_ELT(seq, 7) }, 24);
}

#[test]
fn compact_empty_vector_pointer_access_stays_valid() {
    let session = RSession::new_for_gc_tests();
    let seq = session.with_active(|| unsafe { compact_int_seq(1, 1, 0) });
    let _root = session.sexp(seq).unwrap();
    session.with_active(|| unsafe {
        assert!(DATAPTR(seq).is_null());
        assert_eq!(ALTREP(seq), 0);
    });
    assert_r_error(|| session.with_active(|| unsafe {
        let _ = INTEGER_ELT(seq, 0);
    }));
}

#[test]
fn compact_checked_write_budget_failure_returns_a_typed_error() {
    use crate::sexp::object::{SexpError, SexpMut};
    let session = RSession::new_for_gc_tests();
    let seq = session.with_active(|| unsafe { compact_int_seq(10, 2, 8) });
    let value = session.sexp(seq).unwrap();
    session.with_active_in(|owner| unsafe {
        (*owner).arena.set_budget(ArenaBudget::new(1, 0));
    });
    let mut value = SexpMut::try_from_checked(value).unwrap();
    assert!(matches!(
        value.try_set_integer_elt(1, 99),
        Err(SexpError::MissingData {
            sexptype: SEXPTYPE::INTSXP
        })
    ));
    assert_eq!(value.freeze().integer_elt(1), Some(12));
    assert_eq!(unsafe { ALTREP(seq) }, 1);
}

#[test]
fn compact_raw_payload_rejects_negative_header_length() {
    let session = RSession::new_for_gc_tests();
    let seq = session.with_active(|| unsafe { compact_int_seq(1, 1, 1) });
    let _root = session.sexp(seq).unwrap();
    unsafe {
        (*seq).set_vecsxp_length(-1);
    }
    assert_r_error(|| {
        session.with_active(|| unsafe {
            let _ = DATAPTR(seq);
        })
    });
    assert!(unsafe { (*seq).payload.is_empty() });
}

#[test]
fn compact_comparisons_read_values_without_expansion_under_budget() {
    let session = RSession::new_for_gc_tests();
    for real in [false, true] {
        let (left, right, args) = session.with_active(|| unsafe {
            let make = || {
                if real {
                    compact_real_seq(1.5, 0.25, 8)
                } else {
                    compact_int_seq(10, 2, 8)
                }
            };
            let left = make();
            let right = make();
            let args = crate::sexp::constructors::Rf_cons(
                left,
                crate::sexp::constructors::Rf_cons(right, crate::sexp::globals::R_NilValue()),
            );
            (left, right, args)
        });
        let _left = session.sexp(left).unwrap();
        let _right = session.sexp(right).unwrap();
        let _args = session.sexp(args).unwrap();
        session.with_active_in(|owner| unsafe {
            (*owner).arena.set_budget(ArenaBudget::new(1, 0));
        });
        let before = memory::buffer_allocation_attempts();
        session.with_active(|| unsafe {
            assert_eq!(
                crate::mainutils::identical::R_compute_identical(left, right, 0),
                1
            );
            assert_eq!(
                crate::mainutils::all_equal::do_all_equal(
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    args,
                    std::ptr::null_mut()
                ),
                crate::sexp::globals::R_True()
            );
            assert!((*left).payload.is_empty());
            assert!((*right).payload.is_empty());
        });
        assert_eq!(memory::buffer_allocation_attempts(), before);
        session.with_active_in(|owner| unsafe {
            (*owner).arena.set_budget(ArenaBudget::unlimited());
        });
    }
}

#[cfg(target_pointer_width = "32")]
#[test]
fn compact_payload_rejects_header_length_that_would_truncate() {
    let session = RSession::new_for_gc_tests();
    let seq = session.with_active(|| unsafe { compact_int_seq(1, 1, 1) });
    let _root = session.sexp(seq).unwrap();
    // Exercise the defensive admission boundary without requesting huge storage.
    unsafe {
        (*seq).set_vecsxp_length(usize::MAX as i64 + 2);
    }
    let before = memory::buffer_allocation_attempts();
    session.with_active(|| unsafe {
        materialize(seq);
    });
    assert_eq!(memory::buffer_allocation_attempts(), before);
    assert_r_error(|| {
        session.with_active(|| unsafe {
            let _ = DATAPTR(seq);
        })
    });
    unsafe {
        assert!((*seq).payload.is_empty());
        assert_eq!(ALTREP(seq), 1);
    }
}

#[test]
fn compact_sequence_automatic_roots_precede_post_lend_compatibility_guards() {
    use std::{cell::Cell, rc::Rc};

    for real in [false, true] {
        for length in [0, 8] {
            let session = RSession::new_for_gc_tests();
            let heap = session.with_active_in(|owner| unsafe { (*owner).heap_identity.clone() });
            // Symbol interning is complete before enabling allocation callbacks.
            session.with_active(|| unsafe {
                super::super::symbol::Rf_install(ALTSEQ_TAG_NAME.as_ptr());
            });
            let before = super::super::protect::R_ProtectCount();
            let roots_before = super::super::protect::with_protected_objects(|_, roots| roots.len());
            let observed = Rc::new(Cell::new(0));
            let notifications = observed.clone();
            let kind = if real {
                SEXPTYPE::REALSXP
            } else {
                SEXPTYPE::INTSXP
            };
            let active = Rc::new(Cell::new(true));
            let callback_active = active.clone();
            super::super::gengc::register_gc_callback(Box::new(move |_| {
                if !callback_active.get() {
                    return;
                }
                // The compatibility guard must not be captured while the
                // exclusive arena lend is live. Its owning replacement is
                // already visible to collection before this notification.
                assert_eq!(super::super::protect::R_ProtectCount(), before);
                super::super::protect::with_protected_objects(|_, roots| {
                    assert_eq!(roots.len(), roots_before);
                });
                let roots = memory::automatic_roots(&heap);
                let (projection, allocation) = roots
                    .into_iter()
                    .find(|(_, node)| {
                        heap.node_snapshot(node).is_some_and(|header| {
                            header.sxpinfo.type_of() == kind
                                && header.vecsxp_length() == length as i64
                        })
                    })
                    .expect("published compact vector has an automatic root");
                super::super::gengc::full_gc();
                assert!(allocation.is_live());
                let header = heap.node_snapshot(&allocation).unwrap();
                if length != 0 {
                    assert!(header.sxpinfo.alt());
                    assert!(heap.resolve_link(header.attrib).is_some());
                    if real {
                        assert_eq!(unsafe { REAL_ELT(projection, 7) }, 3.25);
                    } else {
                        assert_eq!(unsafe { INTEGER_ELT(projection, 7) }, 24);
                    }
                }
                notifications.set(notifications.get() + 1);
            }));
            session.with_active_in(|owner| unsafe {
                (*owner).memory_state.gc_force_gap = 1;
                (*owner).memory_state.gc_force_wait = 1;
            });
            let (projection, guard) = session.with_active(|| unsafe {
                if real {
                    compact_real_seq_protected(1.5, 0.25, length)
                } else {
                    compact_int_seq_protected(10, 2, length)
                }
            });
            active.set(false);
            session.with_active_in(|owner| unsafe {
                (*owner).memory_state.gc_force_gap = 0;
            });
            assert!(observed.get() > 0);
            assert_eq!(super::super::protect::R_ProtectCount(), before);
            super::super::protect::with_protected_objects(|_, roots| {
                assert_eq!(roots.len(), roots_before + 1);
            });
            let value = session.sexp(projection).unwrap();
            let token = memory::checked_projection(projection).unwrap().1;
            drop(guard);
            assert_eq!(super::super::protect::R_ProtectCount(), before);
            super::super::protect::with_protected_objects(|_, roots| {
                assert_eq!(roots.len(), roots_before);
            });
            super::super::gengc::full_gc();
            assert!(token.is_live());
            assert_eq!(value.len(), length as i64);
            drop(value);
            super::super::gengc::full_gc();
            assert!(!token.is_live());
        }
    }
}
