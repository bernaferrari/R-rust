use super::*;
use crate::sexp::{constructors::Rf_allocVector, session::RSession};
use std::cell::Cell;

thread_local! {
    static INSPECT_ARGUMENTS: Cell<(i32, i32, i32)> = const { Cell::new((0, 0, 0)) };
    static SUBTREE_ARGUMENTS: Cell<(i32, i32, i32)> = const { Cell::new((0, 0, 0)) };
    static LEGACY_DUPLICATES: Cell<u32> = const { Cell::new(0) };
}
unsafe extern "C" fn length(_: SEXP) -> R_xlen_t {
    2
}
unsafe extern "C" fn real_element(_: SEXP, i: R_xlen_t) -> f64 {
    41.0 + i as f64
}
unsafe fn class(name: *const c_char) -> R_altrep_class_t {
    let class = unsafe { R_make_altreal_class(name, c"compat".as_ptr(), std::ptr::null_mut()) };
    unsafe {
        R_set_altrep_Length_method(class, Some(length));
        R_set_altreal_Elt_method(class, Some(real_element));
    }
    class
}
#[test]
fn canonical_class_handle_and_inherits_match_gnu_surface() {
    assert_eq!(
        std::mem::size_of::<R_altrep_class_t>(),
        std::mem::size_of::<SEXP>()
    );
    assert_eq!(
        std::mem::align_of::<R_altrep_class_t>(),
        std::mem::align_of::<SEXP>()
    );
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let class = class(c"handle".as_ptr());
        let other =
            R_make_altreal_class(c"other".as_ptr(), c"compat".as_ptr(), std::ptr::null_mut());
        let nil = crate::sexp::globals::R_NilValue();
        let x = session.sexp(R_new_altrep(class, nil, nil)).unwrap();
        assert_eq!(R_altrep_inherits(x.clone().as_raw(), class), 1);
        assert_eq!(R_altrep_inherits(x.clone().as_raw(), other), 0);
        assert_eq!(R_altrep_inherits(nil, class), 0);
        assert_eq!(x.try_real_elt(1).unwrap(), 42.0);
        let compact = session.sexp(R_compact_intseq(1, 3)).unwrap();
        let compact_class = R_altrep_class(compact.clone().as_raw()).into();
        assert_eq!(
            R_altrep_inherits(compact.clone().as_raw(), compact_class),
            1
        );
        assert_eq!(R_altrep_inherits(compact.clone().as_raw(), class), 0);
    });
}
#[test]
fn canonical_inspect_forwards_subtree_and_restores_owner() {
    unsafe extern "C" fn subtree(_: SEXP, pre: i32, deep: i32, pvec: i32) {
        SUBTREE_ARGUMENTS.set((pre, deep, pvec));
    }
    unsafe extern "C" fn inspect(
        x: SEXP,
        pre: i32,
        deep: i32,
        pvec: i32,
        subtree: Option<InspectSubtree>,
    ) -> i32 {
        INSPECT_ARGUMENTS.set((pre, deep, pvec));
        if let Some(subtree) = subtree {
            unsafe { subtree(x, pre + 1, deep + 1, pvec + 1) };
        }
        // Reenter class registration, proving the method table is not borrowed
        // during callbacks, then temporarily switch to a different owner.
        let descriptor = unsafe { R_altrep_class(x) };
        unsafe { R_set_altreal_Elt_method(descriptor.into(), Some(real_element)) };
        let other = RSession::new_for_gc_tests();
        other.gc();
        drop(other);
        1
    }
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let class = class(c"inspect".as_ptr());
        R_set_altrep_Inspect_method(class, Some(inspect));
        let nil = crate::sexp::globals::R_NilValue();
        let x = session.sexp(R_new_altrep(class, nil, nil)).unwrap();
        let expected_owner = session.owner_token().unwrap().as_ptr();
        assert_eq!(
            ALTREP_INSPECT(x.clone().as_raw(), 2, 3, 4, Some(subtree)),
            1
        );
        assert_eq!(INSPECT_ARGUMENTS.get(), (2, 3, 4));
        assert_eq!(SUBTREE_ARGUMENTS.get(), (3, 4, 5));
        assert_eq!(
            crate::sexp::instance::current_instance_ptr(),
            Some(expected_owner)
        );
        assert_eq!(x.try_real_elt(0).unwrap(), 41.0);
    });
}
#[test]
fn duplicate_ex_takes_precedence_and_null_declines_without_legacy_call() {
    unsafe extern "C" fn legacy(x: SEXP, _: i32) -> SEXP {
        LEGACY_DUPLICATES.set(LEGACY_DUPLICATES.get() + 1);
        unsafe { R_altrep_data1(x) }
    }
    unsafe extern "C" fn extended(x: SEXP, _: i32) -> SEXP {
        unsafe { R_altrep_data2(x) }
    }
    unsafe extern "C" fn decline(_: SEXP, _: i32) -> SEXP {
        std::ptr::null_mut()
    }
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        LEGACY_DUPLICATES.set(0);
        let class = class(c"duplicate".as_ptr());
        R_set_altrep_Duplicate_method(class, Some(legacy));
        R_set_altrep_DuplicateEX_method(class, Some(extended));
        let a = session.sexp(Rf_allocVector(SEXPTYPE::REALSXP, 2)).unwrap();
        let b = session.sexp(Rf_allocVector(SEXPTYPE::REALSXP, 2)).unwrap();
        let x = session
            .sexp(R_new_altrep(class, a.clone().as_raw(), b.clone().as_raw()))
            .unwrap();
        assert_eq!(
            R_altrep_duplicate(x.clone().as_raw(), 1),
            b.clone().as_raw()
        );
        assert_eq!(LEGACY_DUPLICATES.get(), 0);
        R_set_altrep_DuplicateEX_method(class, Some(decline));
        assert!(R_altrep_duplicate(x.clone().as_raw(), 1).is_null());
        assert_eq!(LEGACY_DUPLICATES.get(), 0);
    });
}
#[test]
fn typed_method_registration_rejects_mismatched_class() {
    unsafe extern "C" fn integer_element(_: SEXP, _: R_xlen_t) -> i32 {
        1
    }
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let class = class(c"typed".as_ptr());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            R_set_altinteger_Elt_method(class, Some(integer_element));
        }));
        assert!(
            result
                .unwrap_err()
                .downcast_ref::<crate::sexp::context::RError>()
                .is_some()
        );
        let nil = crate::sexp::globals::R_NilValue();
        let x = session.sexp(R_new_altrep(class, nil, nil)).unwrap();
        assert_eq!(x.try_real_elt(0).unwrap(), 41.0);
    });
}

#[test]
fn duplicate_dispatch_applies_attributes_only_to_legacy_distinct_result() {
    unsafe extern "C" fn legacy(x: SEXP, _: i32) -> SEXP {
        unsafe { R_altrep_data1(x) }
    }
    unsafe extern "C" fn extended(x: SEXP, _: i32) -> SEXP {
        unsafe { R_altrep_data2(x) }
    }
    unsafe extern "C" fn identity(x: SEXP, _: i32) -> SEXP {
        x
    }
    let session = RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let class = class(c"attributes".as_ptr());
        R_set_altrep_Duplicate_method(class, Some(legacy));
        let nil = crate::sexp::globals::R_NilValue();
        let a = session.sexp(Rf_allocVector(SEXPTYPE::REALSXP, 2)).unwrap();
        let b = session.sexp(Rf_allocVector(SEXPTYPE::REALSXP, 2)).unwrap();
        let x = session
            .sexp(R_new_altrep(class, a.clone().as_raw(), b.clone().as_raw()))
            .unwrap();
        let value = session
            .sexp(crate::sexp::constructors::Rf_ScalarInteger(17))
            .unwrap();
        let attributes = session
            .sexp(crate::sexp::constructors::Rf_cons(value.as_raw(), nil))
            .unwrap();
        SETTAG(
            attributes.clone().as_raw(),
            crate::sexp::symbol::Rf_install(c"custom".as_ptr()),
        );
        SETCDR(ATTRIB(x.clone().as_raw()), attributes.clone().as_raw());
        SET_OBJECT(x.clone().as_raw(), 1);
        let duplicate = session
            .sexp(R_altrep_duplicate(x.clone().as_raw(), 1))
            .unwrap();
        assert_eq!(INTEGER_ELT(CAR(ATTRIB(duplicate.clone().as_raw())), 0), 17);
        assert_eq!(OBJECT(duplicate.clone().as_raw()), 1);
        assert_ne!(ATTRIB(duplicate.as_raw()), attributes.clone().as_raw());
        let own_value = session
            .sexp(crate::sexp::constructors::Rf_ScalarInteger(29))
            .unwrap();
        let own_attributes = session
            .sexp(crate::sexp::constructors::Rf_cons(own_value.as_raw(), nil))
            .unwrap();
        SETTAG(
            own_attributes.clone().as_raw(),
            crate::sexp::symbol::Rf_install(c"own".as_ptr()),
        );
        SET_ATTRIB(b.clone().as_raw(), own_attributes.clone().as_raw());
        R_set_altrep_DuplicateEX_method(class, Some(extended));
        let duplicate = R_altrep_duplicate(x.clone().as_raw(), 1);
        assert_eq!(duplicate, b.clone().as_raw());
        assert_eq!(ATTRIB(duplicate), own_attributes.as_raw());
        R_set_altrep_DuplicateEX_method(class, None);
        R_set_altrep_Duplicate_method(class, Some(identity));
        assert_eq!(
            R_altrep_duplicate(x.clone().as_raw(), 1),
            x.clone().as_raw()
        );
        assert_eq!(CDR(ATTRIB(x.as_raw())), attributes.as_raw());
    });
}
