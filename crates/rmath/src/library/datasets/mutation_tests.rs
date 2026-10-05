//! Small physical namespace fixtures exercise production admission and lookup;
//! the public tests separately use the complete original portable namespace.
use super::*;
use crate::sexp::RSession;

fn minimal_namespace(session: &RSession) -> (Sexp<'static>, Sexp<'static>, Sexp<'static>) {
    let base = super::super::current_base().unwrap();
    let owner = base.runtime_owner.as_ref().unwrap().clone();
    let (info, lazy) = with_runtime(&owner, |access| {
        let namespace = environment(access, &base).unwrap();
        let info = environment(access, &base).unwrap();
        let lazy = environment(access, &base).unwrap();
        super::super::bind(access, &namespace, ".__NAMESPACE__.", &info).unwrap();
        super::super::bind(access, &info, "lazydata", &lazy).unwrap();
        super::super::publish_namespace(access, &namespace).unwrap();
        (info, lazy)
    })
    .unwrap();
    assert!(session.is_active());
    (base, info, lazy)
}

#[test]
fn owned_portable_dataset_mutation_removed_binding_is_absent_after_full_gc() {
    let session = RSession::new_for_gc_tests();
    let (base, _, lazy) = minimal_namespace(&session);
    let owner = base.runtime_owner.as_ref().unwrap().clone();
    let identity = with_runtime(&owner, |access| {
        let original = integers(access, &[7]).unwrap();
        let identity = original.allocation().unwrap().clone();
        super::super::bind(access, &lazy, "mtcars", &original).unwrap();
        let symbol = super::super::symbol(access, "mtcars").unwrap();
        access
            .with_native(|_| {
                crate::sexp::envir::remove_binding_raw(lazy.as_raw(), symbol.as_raw());
                Ok(())
            })
            .unwrap();
        identity
    })
    .unwrap();
    session.with_active(crate::sexp::gengc::full_gc);
    assert!(
        !identity.is_live(),
        "removed value must have no incidental owner"
    );
    assert!(value(base.clone(), "mtcars").unwrap().is_none());
    with_runtime(&owner, |access| {
        let replacement = integers(access, &[9]).unwrap();
        super::super::bind(access, &lazy, "mtcars", &replacement).unwrap();
    })
    .unwrap();
    assert_eq!(
        value(base, "mtcars")
            .unwrap()
            .unwrap()
            .try_integer_elt(0)
            .unwrap(),
        9
    );
}

#[test]
fn owned_portable_dataset_mutation_rejects_non_environment_metadata_and_recovers() {
    let session = RSession::new_for_gc_tests();
    let (base, info, lazy) = minimal_namespace(&session);
    let owner = base.runtime_owner.as_ref().unwrap().clone();
    with_runtime(&owner, |access| {
        let original = integers(access, &[7]).unwrap();
        super::super::bind(access, &lazy, "mtcars", &original).unwrap();
        let invalid = integers(access, &[1]).unwrap();
        super::super::bind(access, &info, "lazydata", &invalid).unwrap();
    })
    .unwrap();
    session.with_active(crate::sexp::gengc::full_gc);
    let error = value(base.clone(), "mtcars").unwrap_err();
    assert!(error.contains("environment"), "{error}");
    with_runtime(&owner, |access| {
        let invalid = integers(access, &[1]).unwrap();
        assert!(matches!(
            super::super::lookup(access, &invalid, "not-an-environment"),
            Err(SexpError::TypeMismatch {
                expected: "environment",
                ..
            })
        ));
        assert!(matches!(
            super::super::bind(access, &invalid, "not-an-environment", &base),
            Err(SexpError::TypeMismatch {
                expected: "environment",
                ..
            })
        ));
        super::super::bind(access, &info, "lazydata", &lazy).unwrap();
    })
    .unwrap();
    assert_eq!(
        value(base, "mtcars")
            .unwrap()
            .unwrap()
            .try_integer_elt(0)
            .unwrap(),
        7
    );
}
