use super::*;
use crate::sexp::{memory::ArenaBudget, session::RSession};

fn fill(session: &RSession) {
    session
        .owner_token()
        .unwrap()
        .with_arena(|arena| {
            arena.set_budget(ArenaBudget::new(0, 8));
            while arena.node_count() < 8 {
                assert!(!arena.alloc_vector(SEXPTYPE::INTSXP, 0).is_null());
            }
        })
        .unwrap();
}

#[test]
fn constructor_pressure_reclaims_garbage_but_preserves_retained_exhaustion_and_recovers() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        fill(&session);
        let value = Rf_allocVector3(SEXPTYPE::INTSXP, 1);
        let factory = session.owner_token().unwrap().node_factory();
        let mut retained = vec![factory.wrap(value).unwrap().into_owned().unwrap()];
        for _ in 1..8 {
            retained.push(
                factory
                    .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 0)))
                    .unwrap()
                    .into_owned()
                    .unwrap(),
            );
        }
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Rf_allocVector3(SEXPTYPE::INTSXP, 1)
        }));
        assert!(failure.is_err());
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::sexp::memory_ext::allocSExp(SEXPTYPE::PROMSXP)
            }))
            .is_err(),
            "bare headers must report allocation failure rather than publish NULL operands"
        );
        assert_eq!(
            session
                .owner_token()
                .unwrap()
                .with_arena(|a| a.budget().max_nodes)
                .unwrap(),
            8
        );
        drop(retained);
        assert!(!Rf_allocList(5).is_null());
    });
}

#[test]
fn constructor_pressure_roots_cons_and_promise_inputs_across_the_collecting_retry() {
    for promise in [false, true] {
        let session = RSession::new_for_gc_tests();
        session.with_active(|| unsafe {
            let owner = session.owner_token().unwrap();
            let (car, cdr) = owner
                .with_arena(|arena| {
                    arena.set_budget(ArenaBudget::new(0, 8));
                    let car = arena.alloc_vector(SEXPTYPE::INTSXP, 0);
                    let cdr = if promise {
                        arena.alloc_node(SEXPTYPE::ENVSXP)
                    } else {
                        arena.alloc_vector(SEXPTYPE::REALSXP, 0)
                    };
                    while arena.node_count() < 8 {
                        assert!(!arena.alloc_vector(SEXPTYPE::INTSXP, 0).is_null());
                    }
                    (car, cdr)
                })
                .unwrap();
            if promise {
                let value = crate::sexp::memory_ext::mkPROMSXP(car, cdr);
                assert_eq!(crate::sexp::accessors::PRCODE(value), car);
                assert_eq!(crate::sexp::accessors::PRENV(value), cdr);
            } else {
                let pair = Rf_cons(car, cdr);
                assert_eq!(crate::sexp::accessors::CAR(pair), car);
                assert_eq!(crate::sexp::accessors::CDR(pair), cdr);
            }
            assert!(owner.with_arena(|a| a.node_count() < 8).unwrap());
        });
    }
}

#[test]
fn constructor_pressure_never_retries_after_original_owner_revocation_in_a_gc_callback() {
    let left = RSession::new_for_gc_tests();
    let right = RSession::new_for_gc_tests();
    let right_pointer = right.owner_token().unwrap().as_ptr();
    let left_pointer = left.owner_token().unwrap().as_ptr();
    left.with_active(|| unsafe {
        fill(&left);
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            crate::sexp::instance::revoke_instance_availability(left_pointer);
            crate::sexp::instance::set_current_instance(right_pointer);
        }));
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Rf_allocVector3(SEXPTYPE::INTSXP, 1)
        }))
        .expect_err("callback-selected owner must not receive the retry");
        let error = failure
            .downcast_ref::<crate::sexp::context::RError>()
            .unwrap();
        assert_eq!(
            error.message,
            crate::sexp::object::SexpError::RootUnavailable.to_string()
        );
    });
    right.with_active(|| {
        assert_eq!(
            right
                .owner_token()
                .unwrap()
                .with_arena(|a| a.node_count())
                .unwrap(),
            0
        );
    });
}
