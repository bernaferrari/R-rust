use super::*;
use crate::sexp::{
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    object::{SexpError, SexpResult},
    session::RSession,
};
use std::{cell::Cell, rc::Rc};
#[test]
fn scalar_logical_spellings_follow_pinned_gnu_across_actual_callers() {
    let s = RSession::new_for_gc_tests();
    let f = s.owner_token().unwrap().node_factory();
    let mut differences = Vec::new();
    for (text, want) in [
        ("T", 1),
        ("t", NA_LOGICAL),
        ("TRUE", 1),
        ("True", 1),
        ("true", 1),
        ("tRuE", NA_LOGICAL),
        ("F", 0),
        ("f", NA_LOGICAL),
        ("FALSE", 0),
        ("False", 0),
        ("false", 0),
        ("fAlSe", NA_LOGICAL),
        (" TRUE", NA_LOGICAL),
        ("TRUE ", NA_LOGICAL),
        ("\tFALSE\n", NA_LOGICAL),
        ("falſe", NA_LOGICAL),
        ("\u{a0}TRUE", NA_LOGICAL),
        ("", NA_LOGICAL),
        ("0", NA_LOGICAL),
        ("1", NA_LOGICAL),
        ("NA", NA_LOGICAL),
    ] {
        let ch = f
            .wrap(unsafe { Rf_mkCharLen(text.as_ptr().cast(), text.len() as _) })
            .unwrap();
        let source = f
            .wrap(unsafe { Rf_allocVector3(SEXPTYPE::STRSXP, 1) })
            .unwrap();
        unsafe { SET_STRING_ELT(source.as_raw(), 0, ch.as_raw()) };
        let mut warn = 73;
        let kernel = unsafe { LogicalFromString(ch.as_raw(), &mut warn) };
        let scalar = unsafe { asLogical(source.as_raw()) };
        let vector = f
            .wrap(unsafe { coerceToLogical(source.as_raw()) })
            .unwrap()
            .try_logical_elt(0)
            .unwrap();
        assert_eq!(warn, 73);
        if [kernel, scalar, vector] != [want; 3] {
            differences.push((text, kernel, scalar, vector, want));
        }
        assert!(ch.try_char_eq(text.as_bytes()).unwrap());
    }
    assert!(
        differences.is_empty(),
        "GNU spelling mismatches (text,kernel,scalar,vector,want): {differences:?}"
    );
    assert_eq!(
        unsafe { LogicalFromString(std::ptr::null_mut(), std::ptr::null_mut()) },
        NA_LOGICAL
    );
    assert_eq!(
        unsafe { LogicalFromString(R_GlobalNaString(), std::ptr::null_mut()) },
        NA_LOGICAL
    );
}
#[test]
fn scalar_logical_kernel_rejects_actual_foreign_character_before_read() {
    let original = RSession::new_for_gc_tests();
    let f = original.owner_token().unwrap().node_factory();
    let ch = f
        .wrap(unsafe { Rf_mkCharLen(c"TRUE".as_ptr(), 4) })
        .unwrap();
    let other = RSession::new_for_gc_tests();
    let owner = other.owner_token().unwrap();
    let before = owner
        .with_arena(|a| {
            (
                a.total_bytes_allocated(),
                a.node_count(),
                a.workspace_bytes(),
            )
        })
        .unwrap();
    let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        LogicalFromString(ch.as_raw(), std::ptr::null_mut())
    }))
    .unwrap_err();
    assert!(error.is::<crate::sexp::context::RError>());
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
    original.with_active(|| assert!(ch.try_char_eq(b"TRUE").unwrap()));
}
#[test]
fn scalar_logical_invalid_native_byte_returns_na() {
    let s = RSession::new_for_gc_tests();
    let f = s.owner_token().unwrap().node_factory();
    let bytes = [0xffu8];
    let ch = f
        .wrap(unsafe { Rf_mkCharLen(bytes.as_ptr().cast(), 1) })
        .unwrap();
    assert_eq!(
        unsafe { LogicalFromString(ch.as_raw(), std::ptr::null_mut()) },
        NA_LOGICAL
    );
    let source = f
        .wrap(unsafe { Rf_allocVector3(SEXPTYPE::STRSXP, 1) })
        .unwrap();
    unsafe { SET_STRING_ELT(source.as_raw(), 0, ch.as_raw()) };
    assert_eq!(unsafe { asLogical(source.as_raw()) }, NA_LOGICAL);
    assert_eq!(
        f.wrap(unsafe { coerceToLogical(source.as_raw()) })
            .unwrap()
            .try_logical_elt(0)
            .unwrap(),
        NA_LOGICAL
    );
}
#[test]
fn scalar_logical_large_input_keeps_same_budget_and_no_input_sized_scratch() {
    use crate::sexp::memory::ArenaBudget;
    let s = RSession::new_for_gc_tests();
    let owner = s.owner_token().unwrap();
    let f = owner.node_factory();
    let bytes = vec![b'x'; if cfg!(miri) { 4096 } else { 256 * 1024 }];
    let ch = f
        .wrap(unsafe { Rf_mkCharLen(bytes.as_ptr().cast(), bytes.len() as _) })
        .unwrap();
    let yes = f
        .wrap(unsafe { Rf_mkCharLen(c"TRUE".as_ptr(), 4) })
        .unwrap();
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
    assert_eq!(
        unsafe { LogicalFromString(ch.as_raw(), std::ptr::null_mut()) },
        NA_LOGICAL
    );
    assert_eq!(
        unsafe { LogicalFromString(yes.as_raw(), std::ptr::null_mut()) },
        1
    );
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
struct Provider {
    failure: Rc<Cell<u8>>,
    reads: Rc<Cell<usize>>,
    callback: Rc<dyn Fn()>,
}
impl AltrepClass for Provider {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::STRSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(2)
    }
    fn element<'s>(&self, c: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        self.reads.set(self.reads.get() + 1);
        c.gc()?;
        (self.callback)();
        match self.failure.get() {
            1 => {
                return Err(SexpError::Altrep {
                    reason: "scalar logical provider failure",
                });
            }
            2 => panic!("scalar logical provider unwind"),
            3 => {
                c.object().try_string_elt(i)?;
            }
            _ => {}
        }
        let ch = c.string(if self.failure.get() == 4 {
            "scalar-logical-denial-unique"
        } else if i == 0 {
            "TRUE"
        } else {
            "FALSE"
        })?;
        Ok(AltrepElement::String(ch))
    }
}
#[test]
fn scalar_logical_actual_callers_root_gc_provider_and_replacement_owner() {
    let s = RSession::new_for_gc_tests();
    let f = s.owner_token().unwrap().node_factory();
    let reads = Rc::new(Cell::new(0));
    let class = s
        .register_altrep_class(
            "scalar-logical-gc",
            Provider {
                failure: Rc::new(Cell::new(0)),
                reads: reads.clone(),
                callback: Rc::new(|| {
                    let other = RSession::new_for_gc_tests();
                    other.owner_token().unwrap().full_gc().unwrap();
                }),
            },
        )
        .unwrap();
    let source = AltrepBuilder::new(class).build().unwrap();
    assert_eq!(unsafe { asLogical(source.as_raw()) }, 1);
    let result = f.wrap(unsafe { coerceToLogical(source.as_raw()) }).unwrap();
    assert_eq!(result.try_logical_elt(0).unwrap(), 1);
    assert_eq!(result.try_logical_elt(1).unwrap(), 0);
    assert_eq!(reads.get(), 3);
    f.require_active().unwrap();
}
#[test]
fn scalar_logical_scalar_provider_failures_reentry_denial_recover_same_input() {
    use crate::sexp::memory::ArenaBudget;
    let s = RSession::new_for_gc_tests();
    let owner = s.owner_token().unwrap();
    let f = owner.node_factory();
    let failure = Rc::new(Cell::new(0));
    let reads = Rc::new(Cell::new(0));
    let class = s
        .register_altrep_class(
            "scalar-logical-failures",
            Provider {
                failure: failure.clone(),
                reads: reads.clone(),
                callback: Rc::new(|| {}),
            },
        )
        .unwrap();
    let source = AltrepBuilder::new(class).build().unwrap();
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
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            asLogical(source.as_raw())
        }))
        .unwrap_err();
        if kind != 2 {
            assert!(error.is::<crate::sexp::context::RError>());
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
        assert_eq!(unsafe { asLogical(source.as_raw()) }, 1);
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
    let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        asLogical(source.as_raw())
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
    assert_eq!(unsafe { asLogical(source.as_raw()) }, 1);
}
#[test]
fn scalar_logical_kernel_rejects_revoked_original_owner() {
    let s = RSession::new_for_gc_tests();
    let owner = s.owner_token().unwrap();
    let ch = owner
        .node_factory()
        .wrap(unsafe { Rf_mkCharLen(c"TRUE".as_ptr(), 4) })
        .unwrap();
    unsafe { crate::sexp::instance::revoke_instance_availability(owner.as_ptr()) };
    let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        LogicalFromString(ch.as_raw(), std::ptr::null_mut())
    }))
    .unwrap_err();
    assert!(error.is::<crate::sexp::context::RError>());
}
