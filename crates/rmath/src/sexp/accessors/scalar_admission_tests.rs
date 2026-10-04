//! A scalar flag grants typed access only for its actual requested kind.

use super::*;
use crate::sexp::{memory::RArena, session::RSession};

#[test]
fn checked_scalar_admission_requires_requested_type_and_scalar_flag() {
    let mut arena = RArena::new();
    for kind in [
        SEXPTYPE::LGLSXP,
        SEXPTYPE::INTSXP,
        SEXPTYPE::REALSXP,
        SEXPTYPE::CPLXSXP,
        SEXPTYPE::RAWSXP,
        SEXPTYPE::STRSXP,
        SEXPTYPE::VECSXP,
    ] {
        let pointer = arena.alloc_vector(kind, 1);
        let (_, allocation) = crate::sexp::memory::checked_projection(pointer).unwrap();
        let heap = allocation.heap_identity();
        for scalar in [true, false] {
            let mut header = heap.node_snapshot(&allocation).unwrap();
            header.sxpinfo.set_scalar(scalar);
            heap.replace_node(&allocation, header).unwrap();
            for requested in -1..=31 {
                assert_eq!(
                    IS_SCALAR(pointer, requested),
                    c_int::from(scalar && kind.0 == requested),
                    "kind={kind:?}, requested={requested}, scalar={scalar}"
                );
            }
        }
    }
}

#[test]
fn checked_scalar_admission_rejects_unregistered_and_retired_storage() {
    let mut header = Box::new(SexprecCore::new(SEXPTYPE::INTSXP));
    header.sxpinfo.set_scalar(true);
    let unregistered = ptr::from_ref(header.as_ref()).cast_mut();
    let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // Invalid admission input is only an address lookup key; the accessor
        // must reject before reading unregistered storage.
        IS_SCALAR(unregistered, SEXPTYPE::INTSXP.0)
    }));
    assert!(rejected.is_err(), "alignment cannot grant typed admission");
    let retired = {
        let mut arena = RArena::new();
        let pointer = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
        // SAFETY: the arena owns the initialized vector while setting its flag.
        unsafe { SET_SCALAR(pointer, 1) };
        pointer
    };
    let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // Allocation admission must reject a retired address before reading
        // a canonical owning Cell.
        IS_SCALAR(retired, SEXPTYPE::INTSXP.0)
    }));
    assert!(rejected.is_err());
    assert_eq!(IS_SCALAR(ptr::null_mut(), SEXPTYPE::INTSXP.0), 0);
}

#[test]
fn checked_scalar_admission_checks_the_original_immutable_singleton_kind() {
    let mut session = RSession::new_for_gc_tests();
    let parent = session
        .with_arena(|arena| arena.alloc_node(SEXPTYPE::LISTSXP))
        .unwrap();
    let factory = crate::sexp::object::SessionNodeFactory::new(session.owner_token().unwrap());
    let original = factory.domain().logical(true);
    // SAFETY: the original active heap owns the initialized parent. Capturing
    // its canonical child link retains this logical lease in that heap domain.
    unsafe { SETCAR(parent, original.as_raw()) };
    crate::sexp::globals::close_immutable_singletons_for_test();
    session.with_active(|| {
        // The owning original value retains its actual immutable bank after
        // the ambient bank retires. These checked reads invoke no code.
        assert_eq!(IS_SCALAR(original.as_raw(), SEXPTYPE::LGLSXP.0), 1);
        assert_eq!(IS_SCALAR(original.as_raw(), SEXPTYPE::INTSXP.0), 0);
        assert_eq!(IS_SCALAR(original.as_raw(), SEXPTYPE::REALSXP.0), 0);
    });
}
