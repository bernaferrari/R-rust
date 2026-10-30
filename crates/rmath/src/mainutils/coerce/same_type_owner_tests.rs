use super::*;
use crate::sexp::{
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    object::{SexpError, SexpResult},
    session::RSession,
};
use std::{cell::Cell, rc::Rc};
fn convert<'s>(source: Sexp<'s>, mode: Sexp<'s>, api: u8) -> Result<SEXP, String> {
    match api {
        0 => as_atomic_safe(source, 1),
        1 => as_vector_safe(source, mode),
        _ => coerce_vector_safe(source, mode),
    }
}
fn mode<'s>(factory: &crate::sexp::object::SessionNodeFactory<'s>) -> Sexp<'s> {
    factory
        .wrap(unsafe { Rf_mkString(c"integer".as_ptr()) })
        .unwrap()
}
fn decorate(source: &Sexp<'_>) {
    unsafe {
        setAttrib(
            source.as_raw(),
            Rf_install(c"note".as_ptr()),
            Rf_ScalarInteger(9),
        );
    }
}
#[test]
fn same_type_helpers_reject_actual_foreign_source_fast_and_copy_paths() {
    let original = RSession::new_for_gc_tests();
    let f = original.owner_token().unwrap().node_factory();
    let plain = f.wrap(unsafe { Rf_ScalarInteger(42) }).unwrap();
    let attributed = f.wrap(unsafe { Rf_ScalarInteger(43) }).unwrap();
    decorate(&attributed);
    let other = RSession::new_for_gc_tests();
    let mode = mode(&other.owner_token().unwrap().node_factory());
    let other_owner = other.owner_token().unwrap();
    let before = other_owner
        .with_arena(|a| {
            (
                a.total_bytes_allocated(),
                a.node_count(),
                a.workspace_bytes(),
            )
        })
        .unwrap();
    let mut accepted = Vec::new();
    for source in [&plain, &attributed] {
        for api in 0..3 {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                convert(source.clone(), mode.clone(), api)
            })) {
                Ok(Ok(_)) => accepted.push((api, source.as_raw() == attributed.as_raw())),
                Ok(Err(_)) => {}
                Err(error) => assert!(
                    error.is::<crate::sexp::context::RError>(),
                    "foreign denial must remain typed"
                ),
            }
        }
    }
    assert!(
        accepted.is_empty(),
        "foreign sources accepted (api,attributed): {accepted:?}"
    );
    assert_eq!(
        other_owner
            .with_arena(|a| (
                a.total_bytes_allocated(),
                a.node_count(),
                a.workspace_bytes()
            ))
            .unwrap(),
        before
    );
    original.with_active(|| {
        assert_eq!(plain.try_integer_elt(0).unwrap(), 42);
        assert_eq!(attributed.try_integer_elt(0).unwrap(), 43);
    });
}
struct Provider {
    reads: Rc<Cell<usize>>,
    mode: Rc<Cell<u8>>,
    length: i64,
    callback: Rc<dyn Fn()>,
}
impl AltrepClass for Provider {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::INTSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(self.length)
    }
    fn element<'s>(&self, c: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        self.reads.set(self.reads.get() + 1);
        c.gc()?;
        (self.callback)();
        if i == 1 {
            match self.mode.get() {
                1 => {
                    return Err(SexpError::Altrep {
                        reason: "same-type provider failure",
                    });
                }
                2 => panic!("same-type provider unwind"),
                3 => {
                    c.object().try_integer_elt(i)?;
                }
                _ => {}
            }
        }
        Ok(AltrepElement::Integer([1, NA_INTEGER, 3][i as usize]))
    }
}
#[test]
fn same_type_copy_keeps_gc_provider_and_replacement_owner_rooted() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let f = owner.node_factory();
    let mode = mode(&f);
    let reads = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "same-type-gc",
            Provider {
                reads: reads.clone(),
                mode: Rc::new(Cell::new(0)),
                length: 3,
                callback: Rc::new(|| {
                    let replacement = RSession::new_for_gc_tests();
                    replacement.owner_token().unwrap().full_gc().unwrap();
                }),
            },
        )
        .unwrap();
    for api in 0..3 {
        let source = AltrepBuilder::new(class.clone()).build().unwrap();
        decorate(&source);
        let result = f
            .wrap(convert(source.clone(), mode.clone(), api).unwrap())
            .unwrap();
        assert_ne!(source.as_raw(), result.as_raw());
        assert!(result.try_attrib().unwrap().is_nil());
        for (i, want) in [1, NA_INTEGER, 3].into_iter().enumerate() {
            assert_eq!(result.try_integer_elt(i as _).unwrap(), want);
            assert_eq!(source.try_integer_elt(i as _).unwrap(), want);
        }
        unsafe { SET_INTEGER_ELT(result.as_raw(), 0, 77) };
        assert_eq!(source.try_integer_elt(0).unwrap(), 1);
        unsafe { SET_INTEGER_ELT(source.as_raw(), 2, 88) };
        assert_eq!(result.try_integer_elt(2).unwrap(), 3);
        f.require_active().unwrap();
    }
    assert!(reads.get() >= 9);
}
#[test]
fn same_type_copy_denial_is_typed_before_provider_read_and_recovers() {
    use crate::sexp::memory::ArenaBudget;
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let f = owner.node_factory();
    let mode = mode(&f);
    let reads = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "same-type-denied",
            Provider {
                reads: reads.clone(),
                mode: Rc::new(Cell::new(0)),
                length: 65536,
                callback: Rc::new(|| {}),
            },
        )
        .unwrap();
    let source = AltrepBuilder::new(class).build().unwrap();
    decorate(&source);
    owner.full_gc().unwrap();
    let before = owner
        .with_arena(|a| {
            (
                a.total_bytes_allocated(),
                a.node_count(),
                a.workspace_bytes(),
            )
        })
        .unwrap();
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(before.0 + 4096, usize::MAX)))
        .unwrap();
    for api in 0..3 {
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            convert(source.clone(), mode.clone(), api)
        }))
        .unwrap_err();
        assert!(error.is::<crate::sexp::context::RError>());
        assert_eq!(reads.get(), 0);
        owner.full_gc().unwrap();
        assert_eq!(
            owner
                .with_arena(|a| (
                    a.total_bytes_allocated(),
                    a.node_count(),
                    a.workspace_bytes()
                ))
                .unwrap(),
            before
        );
        let small = f.wrap(unsafe { Rf_ScalarInteger(7) }).unwrap();
        decorate(&small);
        let result = f.wrap(convert(small, mode.clone(), api).unwrap()).unwrap();
        assert_eq!(result.try_integer_elt(0).unwrap(), 7);
        drop(result);
        owner.full_gc().unwrap();
    }
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(usize::MAX, usize::MAX)))
        .unwrap();
}

#[test]
fn same_type_copy_provider_failures_reentry_and_unwind_recover() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let f = owner.node_factory();
    let mode_value = mode(&f);
    let failure = Rc::new(Cell::new(0));
    let reads = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "same-type-failure",
            Provider {
                reads,
                mode: failure.clone(),
                length: 3,
                callback: Rc::new(|| {}),
            },
        )
        .unwrap();
    for api in 0..3 {
        for kind in [1, 2, 3] {
            failure.set(kind);
            let source = AltrepBuilder::new(class.clone()).build().unwrap();
            decorate(&source);
            owner.full_gc().unwrap();
            let before = owner
                .with_arena(|a| {
                    (
                        a.total_bytes_allocated(),
                        a.node_count(),
                        a.workspace_bytes(),
                    )
                })
                .unwrap();
            let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                convert(source.clone(), mode_value.clone(), api)
            }))
            .unwrap_err();
            if kind != 2 {
                assert!(
                    error.is::<crate::sexp::context::RError>(),
                    "provider error lost typed RError: api={api},kind={kind}"
                );
            }
            owner.full_gc().unwrap();
            assert_eq!(
                owner
                    .with_arena(|a| (
                        a.total_bytes_allocated(),
                        a.node_count(),
                        a.workspace_bytes()
                    ))
                    .unwrap(),
                before
            );
            failure.set(0);
            let output = f
                .wrap(convert(source, mode_value.clone(), api).unwrap())
                .unwrap();
            assert_eq!(output.try_integer_elt(2).unwrap(), 3);
            owner.full_gc().unwrap();
        }
    }
}
#[test]
fn same_type_copy_rejects_provider_revocation_without_publication() {
    for api in 0..3 {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let f = owner.node_factory();
        let mode_value = mode(&f);
        let instance = owner.as_ptr();
        let class = session
            .register_altrep_class(
                "same-type-revoke",
                Provider {
                    reads: Rc::new(Cell::new(0)),
                    mode: Rc::new(Cell::new(0)),
                    length: 3,
                    callback: Rc::new(move || unsafe {
                        crate::sexp::instance::revoke_instance_availability(instance)
                    }),
                },
            )
            .unwrap();
        let source = AltrepBuilder::new(class).build().unwrap();
        decorate(&source);
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            convert(source, mode_value, api)
        }))
        .unwrap_err();
        assert!(error.is::<crate::sexp::context::RError>());
    }
}

#[test]
fn same_type_copy_covers_each_atomic_provider_including_rooted_strings() {
    struct Typed(SEXPTYPE);
    impl AltrepClass for Typed {
        fn vector_type(&self) -> SEXPTYPE {
            self.0
        }
        fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
            Ok(2)
        }
        fn element<'s>(&self, c: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
            c.gc()?;
            Ok(match self.0 {
                SEXPTYPE::LGLSXP => AltrepElement::Logical([1, NA_LOGICAL][i as usize]),
                SEXPTYPE::INTSXP => AltrepElement::Integer([3, NA_INTEGER][i as usize]),
                SEXPTYPE::REALSXP => {
                    AltrepElement::Real([1.25, crate::sexp::ffi::NA_REAL][i as usize])
                }
                SEXPTYPE::CPLXSXP => AltrepElement::Complex(Rcomplex {
                    r: [1.25, crate::sexp::ffi::NA_REAL][i as usize],
                    i: -2.5,
                }),
                SEXPTYPE::RAWSXP => AltrepElement::Raw([0, 255][i as usize]),
                _ => AltrepElement::String(if i == 0 {
                    c.string("α")?
                } else {
                    c.wrap(unsafe { R_GlobalNaString() })?
                }),
            })
        }
    }
    let session = RSession::new_for_gc_tests();
    let f = session.owner_token().unwrap().node_factory();
    for (kind, op) in [
        (SEXPTYPE::LGLSXP, 4),
        (SEXPTYPE::INTSXP, 1),
        (SEXPTYPE::REALSXP, 2),
        (SEXPTYPE::CPLXSXP, 3),
        (SEXPTYPE::RAWSXP, 5),
        (SEXPTYPE::STRSXP, 0),
    ] {
        let class = session
            .register_altrep_class(&format!("same-type-kind-{op}"), Typed(kind))
            .unwrap();
        let source = AltrepBuilder::new(class).build().unwrap();
        decorate(&source);
        let output = f.wrap(as_atomic_safe(source.clone(), op).unwrap()).unwrap();
        assert_eq!(output.typeof_(), kind);
        assert!(output.try_attrib().unwrap().is_nil());
        match kind {
            SEXPTYPE::LGLSXP => {
                assert_eq!(output.try_logical_elt(0).unwrap(), 1);
                assert_eq!(output.try_logical_elt(1).unwrap(), NA_LOGICAL);
            }
            SEXPTYPE::INTSXP => {
                assert_eq!(output.try_integer_elt(0).unwrap(), 3);
                assert_eq!(output.try_integer_elt(1).unwrap(), NA_INTEGER);
            }
            SEXPTYPE::REALSXP => {
                assert_eq!(output.try_real_elt(0).unwrap(), 1.25);
                assert_eq!(
                    output.try_real_elt(1).unwrap().to_bits(),
                    crate::sexp::ffi::NA_REAL.to_bits()
                );
            }
            SEXPTYPE::CPLXSXP => {
                assert_eq!(output.try_complex_elt(0).unwrap().i, -2.5);
                assert_eq!(
                    output.try_complex_elt(1).unwrap().r.to_bits(),
                    crate::sexp::ffi::NA_REAL.to_bits()
                );
            }
            SEXPTYPE::RAWSXP => {
                assert_eq!(output.try_raw_elt(0).unwrap(), 0);
                assert_eq!(output.try_raw_elt(1).unwrap(), 255);
            }
            _ => {
                assert!(
                    output
                        .try_string_elt(0)
                        .unwrap()
                        .try_char_eq("α".as_bytes())
                        .unwrap()
                );
                assert!(output.try_string_elt(1).unwrap().is_na_string());
            }
        }
    }
}

#[test]
fn compatibility_entries_admit_source_before_mode_callback_and_reject_foreign_mode() {
    struct Mode(Rc<Cell<usize>>);
    impl AltrepClass for Mode {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::STRSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
            Ok(1)
        }
        fn element<'s>(&self, c: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
            self.0.set(self.0.get() + 1);
            c.gc()?;
            Ok(AltrepElement::String(c.string("integer")?))
        }
    }
    let original = RSession::new_for_gc_tests();
    let f = original.owner_token().unwrap().node_factory();
    let source_a = f.wrap(unsafe { Rf_ScalarInteger(42) }).unwrap();
    let mode_a = mode(&f);
    let other = RSession::new_for_gc_tests();
    let f = other.owner_token().unwrap().node_factory();
    let source_b = f.wrap(unsafe { Rf_ScalarInteger(43) }).unwrap();
    let reads = Rc::new(Cell::new(0));
    let class = other
        .register_altrep_class("owner-admitted-mode", Mode(reads.clone()))
        .unwrap();
    let lazy_mode = AltrepBuilder::new(class).build().unwrap();
    let mut foreign_modes_accepted = Vec::new();
    let mut source_before_mode = Vec::new();
    let mut foreign_sources_accepted = Vec::new();
    for api in [1, 2] {
        if convert(source_b.clone(), mode_a.clone(), api).is_ok() {
            foreign_modes_accepted.push(api)
        }
        let before = reads.get();
        if convert(source_a.clone(), lazy_mode.clone(), api).is_ok() {
            foreign_sources_accepted.push(api);
        }
        source_before_mode.push(reads.get() - before);
    }
    assert!(
        foreign_modes_accepted.is_empty()
            && foreign_sources_accepted.is_empty()
            && source_before_mode == [0, 0],
        "foreign modes accepted {foreign_modes_accepted:?}; foreign sources accepted {foreign_sources_accepted:?}; mode reads before source denial {source_before_mode:?}"
    );
}
#[test]
fn compatibility_entries_reject_stale_generations_before_classification() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let f = owner.node_factory();
    let mode_value = mode(&f);
    let mut violations = Vec::new();
    for api in 0..3 {
        let stale = f.wrap(unsafe { Rf_ScalarInteger(45) }).unwrap();
        let address = stale.as_raw();
        // Match the object safety tests: invalidate only at the allocator seam and
        // reuse the physical header, retaining a checked root for the fresh node.
        let fresh = owner
            .with_arena(|a| {
                unsafe { a.free_node(address) };
                a.alloc_node(SEXPTYPE::REALSXP)
            })
            .unwrap();
        assert_eq!(address, fresh);
        let fresh = f.wrap(fresh).unwrap();
        assert!(!stale.is_live());
        if !matches!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| convert(
                stale,
                mode_value.clone(),
                api
            ))),
            Ok(Err(_))
        ) {
            violations.push((api, "source"));
        }
        assert_eq!(fresh.typeof_(), SEXPTYPE::REALSXP);
    }
    for api in [1, 2] {
        let source = f.wrap(unsafe { Rf_ScalarInteger(46) }).unwrap();
        let stale_mode = mode(&f);
        let address = stale_mode.as_raw();
        let fresh = owner
            .with_arena(|a| {
                unsafe { a.free_node(address) };
                a.alloc_node(SEXPTYPE::STRSXP)
            })
            .unwrap();
        assert_eq!(address, fresh);
        let fresh = f.wrap(fresh).unwrap();
        assert!(!stale_mode.is_live());
        if !matches!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| convert(
                source, stale_mode, api
            ))),
            Ok(Err(_))
        ) {
            violations.push((api, "mode"));
        }
        assert_eq!(fresh.typeof_(), SEXPTYPE::STRSXP);
    }
    assert!(
        violations.is_empty(),
        "stale compatibility arguments accepted or panicked: {violations:?}"
    );
}
#[test]
fn compatibility_entries_reject_revoked_source_and_mode_without_reading_payload() {
    let original = RSession::new_for_gc_tests();
    let owner = original.owner_token().unwrap();
    let f = owner.node_factory();
    let source_a = f.wrap(unsafe { Rf_ScalarInteger(1) }).unwrap();
    let mode_a = mode(&f);
    unsafe { crate::sexp::instance::revoke_instance_availability(owner.as_ptr()) };
    let other = RSession::new_for_gc_tests();
    let f = other.owner_token().unwrap().node_factory();
    let source_b = f.wrap(unsafe { Rf_ScalarInteger(2) }).unwrap();
    let mode_b = mode(&f);
    let mut violations = Vec::new();
    for api in 0..3 {
        if !matches!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| convert(
                source_a.clone(),
                mode_b.clone(),
                api
            ))),
            Ok(Err(_))
        ) {
            violations.push((api, "source"));
        }
    }
    for api in [1, 2] {
        if !matches!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| convert(
                source_b.clone(),
                mode_a.clone(),
                api
            ))),
            Ok(Err(_))
        ) {
            violations.push((api, "mode"));
        }
    }
    assert!(
        violations.is_empty(),
        "revoked compatibility arguments accepted or panicked: {violations:?}"
    );
}

#[test]
fn compatibility_mode_callbacks_preserve_typed_failures_and_same_input_recovery() {
    struct ModeProvider {
        failure: Rc<Cell<u8>>,
        reads: Rc<Cell<usize>>,
    }
    impl AltrepClass for ModeProvider {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::STRSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
            Ok(1)
        }
        fn element<'s>(&self, c: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
            self.reads.set(self.reads.get() + 1);
            c.gc()?;
            {
                let replacement = RSession::new_for_gc_tests();
                replacement.owner_token().unwrap().full_gc().unwrap();
            }
            match self.failure.get() {
                1 => {
                    return Err(SexpError::Altrep {
                        reason: "mode callback failure",
                    });
                }
                2 => panic!("mode callback unwind"),
                3 => {
                    c.object().try_string_elt(0)?;
                }
                _ => {}
            }
            Ok(AltrepElement::String(c.string(
                if self.failure.get() == 4 {
                    "entry-mode-node-denial-unique"
                } else {
                    "integer"
                },
            )?))
        }
    }
    use crate::sexp::memory::ArenaBudget;
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let f = owner.node_factory();
    let source = f.wrap(unsafe { Rf_ScalarInteger(17) }).unwrap();
    let failure = Rc::new(Cell::new(0));
    let reads = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "typed-owner-mode",
            ModeProvider {
                failure: failure.clone(),
                reads: reads.clone(),
            },
        )
        .unwrap();
    let mode_value = AltrepBuilder::new(class).build().unwrap();
    for api in [1, 2] {
        for kind in [1, 2, 3] {
            failure.set(kind);
            owner.full_gc().unwrap();
            let before = owner
                .with_arena(|a| {
                    (
                        a.total_bytes_allocated(),
                        a.node_count(),
                        a.workspace_bytes(),
                    )
                })
                .unwrap();
            let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                convert(source.clone(), mode_value.clone(), api)
            }))
            .unwrap_err();
            if kind != 2 {
                assert!(
                    error.is::<crate::sexp::context::RError>(),
                    "mode failure lost typed error"
                );
                if kind == 1 {
                    assert!(
                        error
                            .downcast_ref::<crate::sexp::context::RError>()
                            .unwrap()
                            .to_string()
                            .contains("mode callback failure")
                    );
                }
            }
            owner.full_gc().unwrap();
            assert_eq!(
                owner
                    .with_arena(|a| (
                        a.total_bytes_allocated(),
                        a.node_count(),
                        a.workspace_bytes()
                    ))
                    .unwrap(),
                before
            );
            failure.set(0);
            let output = f
                .wrap(convert(source.clone(), mode_value.clone(), api).unwrap())
                .unwrap();
            assert_eq!(output.try_integer_elt(0).unwrap(), 17);
        }
        failure.set(4);
        owner.full_gc().unwrap();
        let before = owner
            .with_arena(|a| {
                (
                    a.total_bytes_allocated(),
                    a.node_count(),
                    a.workspace_bytes(),
                )
            })
            .unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(usize::MAX, before.1)))
            .unwrap();
        let count = reads.get();
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            convert(source.clone(), mode_value.clone(), api)
        }))
        .unwrap_err();
        assert!(error.is::<crate::sexp::context::RError>());
        assert!(reads.get() > count);
        owner.full_gc().unwrap();
        assert_eq!(
            owner
                .with_arena(|a| (
                    a.total_bytes_allocated(),
                    a.node_count(),
                    a.workspace_bytes()
                ))
                .unwrap(),
            before
        );
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(usize::MAX, usize::MAX)))
            .unwrap();
        failure.set(0);
        assert_eq!(
            f.wrap(convert(source.clone(), mode_value.clone(), api).unwrap())
                .unwrap()
                .try_integer_elt(0)
                .unwrap(),
            17
        );
    }
}
#[test]
fn compatibility_entry_null_source_and_invalid_mode_neighbors_preserve_values() {
    let session = RSession::new_for_gc_tests();
    let f = session.owner_token().unwrap().node_factory();
    let valid = mode(&f);
    let source = f.wrap(unsafe { Rf_ScalarInteger(9) }).unwrap();
    for api in 0..3 {
        let output = f
            .wrap(convert(f.nil(), valid.clone(), api).unwrap())
            .unwrap();
        assert_eq!(output.typeof_(), SEXPTYPE::INTSXP);
        assert_eq!(output.len(), 0);
    }
    for invalid in [
        f.nil(),
        f.wrap(unsafe { Rf_ScalarInteger(1) }).unwrap(),
        f.wrap(unsafe { Rf_allocVector3(SEXPTYPE::STRSXP, 0) })
            .unwrap(),
        f.wrap(unsafe { Rf_mkString(c"unknown-mode".as_ptr()) })
            .unwrap(),
    ] {
        for api in [1, 2] {
            assert_eq!(
                convert(source.clone(), invalid.clone(), api).unwrap_err(),
                "invalid 'mode' argument"
            );
        }
    }
    let any = f.wrap(unsafe { Rf_mkString(c"any".as_ptr()) }).unwrap();
    for api in [1, 2] {
        assert_eq!(
            convert(source.clone(), any.clone(), api).unwrap(),
            source.as_raw()
        );
    }
}

#[test]
fn compatibility_large_and_byte_modes_fit_same_budget_without_input_string_copy() {
    use crate::sexp::memory::ArenaBudget;
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let f = owner.node_factory();
    let source = f.wrap(unsafe { Rf_ScalarInteger(9) }).unwrap();
    let valid = mode(&f);
    // Native exercises the large input; Miri checks the same borrow/UTF-8 and
    // budget paths with 4 KiB to bound interpreter byte-validity checking.
    let mode_length = if cfg!(miri) { 4096 } else { 256 * 1024 };
    for (bytes, accept) in [
        (vec![b'x'; mode_length], false),
        ("møde".as_bytes().to_vec(), false),
        (vec![0xff], false),
        (Vec::new(), false),
        (b"integer".to_vec(), true),
        (b"integer\0suffix".to_vec(), true),
    ] {
        // The final case preserves legacy CStr prefix semantics for native bytes;
        // it does not claim GNU accepts embedded-NUL R character construction.
        let chars = f
            .wrap(unsafe { Rf_mkCharLen(bytes.as_ptr().cast(), bytes.len() as _) })
            .unwrap();
        let mode_value = f
            .wrap(unsafe { Rf_allocVector3(SEXPTYPE::STRSXP, 1) })
            .unwrap();
        let mut mode_value = crate::sexp::object::SexpMut::try_from_checked(mode_value).unwrap();
        mode_value.try_set_string_elt(0, chars).unwrap();
        let mode_value = mode_value.freeze();
        owner.full_gc().unwrap();
        let before = owner
            .with_arena(|a| {
                (
                    a.total_bytes_allocated(),
                    a.node_count(),
                    a.workspace_bytes(),
                )
            })
            .unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(before.0 + 4096, before.1)))
            .unwrap();
        for api in [1, 2] {
            let result = convert(source.clone(), mode_value.clone(), api);
            if accept {
                assert_eq!(result.unwrap(), source.as_raw())
            } else {
                assert_eq!(result.unwrap_err(), "invalid 'mode' argument")
            };
            assert_eq!(
                convert(source.clone(), valid.clone(), api).unwrap(),
                source.as_raw()
            );
        }
        owner.full_gc().unwrap();
        assert_eq!(
            owner
                .with_arena(|a| (
                    a.total_bytes_allocated(),
                    a.node_count(),
                    a.workspace_bytes()
                ))
                .unwrap(),
            before
        );
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(usize::MAX, usize::MAX)))
            .unwrap();
    }
}

#[test]
fn compatibility_mode_callback_revocation_stops_before_raw_character_reads() {
    struct RevokeMode;
    impl AltrepClass for RevokeMode {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::STRSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
            Ok(1)
        }
        fn element<'s>(&self, c: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
            c.gc()?;
            let chars = c.string("integer")?;
            unsafe {
                let owner = crate::sexp::owner::OwnerToken::current()?;
                crate::sexp::instance::revoke_instance_availability(owner.as_ptr());
            }
            Ok(AltrepElement::String(chars))
        }
    }
    for api in [1, 2] {
        let session = RSession::new_for_gc_tests();
        let f = session.owner_token().unwrap().node_factory();
        let source = f.wrap(unsafe { Rf_ScalarInteger(11) }).unwrap();
        let class = session
            .register_altrep_class("revoke-mode-callback", RevokeMode)
            .unwrap();
        let mode_value = AltrepBuilder::new(class).build().unwrap();
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            convert(source, mode_value, api)
        }))
        .unwrap_err();
        assert!(error.is::<crate::sexp::context::RError>());
    }
}

#[test]
fn public_as_vector_altrep_mode_reads_match_pinned_gnu() {
    struct Mode(Rc<Cell<usize>>);
    impl AltrepClass for Mode {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::STRSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
            Ok(1)
        }
        fn element<'s>(&self, c: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
            let read = self.0.get() + 1;
            self.0.set(read);
            c.gc()?;
            Ok(AltrepElement::String(c.string(if read == 1 {
                "integer"
            } else {
                "double"
            })?))
        }
    }
    let mut session = RSession::new_for_gc_tests();
    let reads = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class("public-mode-count", Mode(reads.clone()))
        .unwrap();
    let mode_value = AltrepBuilder::new(class)
        .build()
        .unwrap()
        .into_owned()
        .unwrap();
    assert!(session.define_var("rport_test_mode", mode_value));
    let (result, _, _) =
        session.eval_code_with_output_capture("as.vector(c(1L, NA_integer_, 3L), rport_test_mode)");
    let result = result.unwrap();
    assert_eq!(result.typeof_(), SEXPTYPE::REALSXP);
    assert_eq!(result.len(), 3);
    assert_eq!(result.try_real_elt(0).unwrap(), 1.0);
    assert!(crate::sexp::ffi::is_na_real(
        result.try_real_elt(1).unwrap()
    ));
    assert_eq!(result.try_real_elt(2).unwrap(), 3.0);
    assert_eq!(
        reads.get(),
        2,
        "pinned GNU R evaluates the mode element twice"
    );
}

#[test]
fn public_as_vector_mode_short_circuit_errors_and_revocation() {
    struct Mode {
        reads: Rc<Cell<usize>>,
        scenario: u8,
        owner: *mut crate::sexp::instance::RInstance,
    }
    impl AltrepClass for Mode {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::STRSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
            Ok(1)
        }
        fn element<'s>(&self, c: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
            let n = self.reads.get() + 1;
            self.reads.set(n);
            c.gc()?;
            if self.scenario == 2 && n == 2 {
                return Err(SexpError::Altrep {
                    reason: "second mode failure",
                });
            }
            if self.scenario == 3 && n == 2 {
                unsafe {
                    crate::sexp::instance::revoke_instance_availability(self.owner);
                }
            }
            Ok(AltrepElement::String(c.string(if self.scenario == 1 {
                "function"
            } else if self.scenario == 4 && n == 2 {
                "function"
            } else {
                "integer"
            })?))
        }
    }
    for scenario in 1..=4 {
        let mut session = RSession::new_for_gc_tests();
        let reads = Rc::new(Cell::new(0));
        let pointer = session.owner_token().unwrap().as_ptr();
        let class = session
            .register_altrep_class(
                "public-mode-edge",
                Mode {
                    reads: reads.clone(),
                    scenario,
                    owner: pointer,
                },
            )
            .unwrap();
        let mode = AltrepBuilder::new(class)
            .build()
            .unwrap()
            .into_owned()
            .unwrap();
        assert!(session.define_var("rport_mode_edge", mode));
        let (result, _, _) =
            session.eval_code_with_output_capture("as.vector(1L, rport_mode_edge)");
        if scenario == 1 {
            assert_eq!(result.unwrap().typeof_(), SEXPTYPE::CLOSXP);
            assert_eq!(reads.get(), 1);
        } else {
            let error = result.unwrap_err().to_string();
            assert_eq!(reads.get(), 2);
            if scenario == 2 {
                assert!(error.contains("second mode failure"), "{error}");
            }
            if scenario == 4 {
                assert!(error.contains("invalid 'mode' argument"), "{error}");
            }
        }
    }
}

#[test]
fn compatibility_as_vector_uses_second_stateful_mode() {
    struct Mode(Rc<Cell<usize>>);
    impl AltrepClass for Mode {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::STRSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
            Ok(1)
        }
        fn element<'s>(&self, c: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
            let n = self.0.get() + 1;
            self.0.set(n);
            c.gc()?;
            Ok(AltrepElement::String(c.string(if n == 1 {
                "integer"
            } else {
                "double"
            })?))
        }
    }
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let source = factory.wrap(unsafe { Rf_ScalarInteger(7) }).unwrap();
    let reads = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class("compat-as-vector-stateful", Mode(reads.clone()))
        .unwrap();
    let mode = AltrepBuilder::new(class).build().unwrap();
    let result = factory.wrap(as_vector_safe(source, mode).unwrap()).unwrap();
    assert_eq!(reads.get(), 2);
    assert_eq!(result.typeof_(), SEXPTYPE::REALSXP);
    assert_eq!(result.try_real_elt(0).unwrap(), 7.0);
}

#[test]
fn compatibility_as_vector_any_preserves_list_and_strips_atomic_attributes() {
    let session = RSession::new_for_gc_tests();
    let factory = session.owner_token().unwrap().node_factory();
    let any = factory
        .wrap(unsafe { Rf_mkString(c"any".as_ptr()) })
        .unwrap();
    let atomic = factory.wrap(unsafe { Rf_ScalarInteger(7) }).unwrap();
    let list = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, 1)))
        .unwrap();
    decorate(&atomic);
    decorate(&list);
    let atomic_result = factory
        .wrap(as_vector_safe(atomic.clone(), any.clone()).unwrap())
        .unwrap();
    assert!(atomic_result.try_attrib().unwrap().is_nil());
    assert!(!atomic.try_attrib().unwrap().is_nil());
    let list_result = factory
        .wrap(as_vector_safe(list.clone(), any).unwrap())
        .unwrap();
    assert_eq!(list_result, list);
    assert!(!list_result.try_attrib().unwrap().is_nil());
}
