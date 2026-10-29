use super::*;
use crate::sexp::{
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    memory::ArenaBudget,
    object::{SessionNodeFactory, SexpError, SexpMut, SexpResult},
    session::RSession,
};
use std::{cell::Cell, rc::Rc};

struct Provider {
    mode: Rc<Cell<u8>>,
    hook: Rc<dyn Fn()>,
}
impl AltrepClass for Provider {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::REALSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(2)
    }
    fn element<'s>(&self, c: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        (self.hook)();
        c.gc()?;
        match self.mode.get() {
            1 => {
                return Err(SexpError::Altrep {
                    reason: "anyNA provider error",
                });
            }
            2 => panic!("anyNA provider unwind"),
            3 => {
                c.object().try_real_elt(i)?;
            }
            _ => {}
        }
        Ok(AltrepElement::Real(if i == 0 { 1. } else { f64::NAN }))
    }
}
fn list<'s>(f: &SessionNodeFactory<'s>, child: &Sexp<'s>) -> Sexp<'s> {
    let x = f
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::VECSXP, 1)))
        .unwrap();
    let mut x = SexpMut::try_from_checked(x).unwrap();
    x.try_set_vector_elt(0, child.clone()).unwrap();
    x.freeze()
}
fn args<'s>(f: &SessionNodeFactory<'s>, child: &Sexp<'s>) -> Sexp<'s> {
    let nil = f.nil();
    let x = list(f, &list(f, child));
    let flag = f
        .wrap(unsafe { crate::sexp::constructors::Rf_ScalarLogical(1) })
        .unwrap();
    let tail = f.pairlist_cell(&flag, &nil, &nil).unwrap();
    f.pairlist_cell(&x, &tail, &nil).unwrap()
}
fn invoke(a: &Sexp<'_>) -> bool {
    let nil = unsafe { crate::sexp::globals::R_NilValue() };
    any_na_impl(nil, nil, a.as_raw(), nil)
}

#[test]
fn public_builtin_uses_rooted_recursive_default() {
    let s = RSession::new_for_gc_tests();
    let f = s.owner_token().unwrap().node_factory();
    let class = s
        .register_altrep_class(
            "anyNA-public",
            Provider {
                mode: Rc::new(Cell::new(0)),
                hook: Rc::new(|| {}),
            },
        )
        .unwrap();
    let source = AltrepBuilder::new(class).build().unwrap();
    let a = args(&f, &source);
    let nil = f.nil();
    let result = f
        .wrap(unsafe {
            crate::mainutils::essentials::do_any_na(
                nil.as_raw(),
                nil.as_raw(),
                a.as_raw(),
                nil.as_raw(),
            )
        })
        .unwrap();
    assert_eq!(result.typeof_(), SEXPTYPE::LGLSXP);
    assert_eq!(result.try_logical_elt(0).unwrap(), 1);
}

#[test]
fn detached_child_survives_provider_gc_and_releases_afterward() {
    let s = RSession::new_for_gc_tests();
    let owner = s.owner_token().unwrap();
    let f = owner.node_factory();
    let cell = Rc::new(Cell::new(std::ptr::null_mut()));
    let detached = cell.clone();
    let nil = f.nil().as_raw();
    let class = s
        .register_altrep_class(
            "anyNA-detached",
            Provider {
                mode: Rc::new(Cell::new(0)),
                hook: Rc::new(move || unsafe {
                    crate::sexp::accessors::SETCAR(detached.get(), nil);
                }),
            },
        )
        .unwrap();
    let source = AltrepBuilder::new(class).build().unwrap();
    let allocation = source.allocation().unwrap().clone();
    let a = args(&f, &source);
    cell.set(a.as_raw());
    drop(source);
    assert!(invoke(&a));
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    owner.full_gc().unwrap();
    assert!(!allocation.is_live());
}

#[test]
fn provider_failures_release_roots_and_same_resident_cap_recovers() {
    let s = RSession::new_for_gc_tests();
    let owner = s.owner_token().unwrap();
    let f = owner.node_factory();
    let mode = Rc::new(Cell::new(0));
    let class = s
        .register_altrep_class(
            "anyNA-failure",
            Provider {
                mode: mode.clone(),
                hook: Rc::new(|| {}),
            },
        )
        .unwrap();
    let source = AltrepBuilder::new(class).build().unwrap();
    let a = args(&f, &source);
    owner.full_gc().unwrap();
    let resident = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
    owner
        .with_arena(|a| {
            a.set_budget(ArenaBudget::new(
                resident + work::capacity_bytes(4),
                usize::MAX,
            ))
        })
        .unwrap();
    for failure in [1, 2, 3] {
        mode.set(failure);
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| invoke(&a))).is_err());
        owner.full_gc().unwrap();
        assert_eq!(
            owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
            resident
        );
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }
    mode.set(0);
    assert!(invoke(&a));
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
        .unwrap();
}

#[test]
fn foreign_owner_is_rejected_before_provider_read() {
    let s = RSession::new_for_gc_tests();
    let f = s.owner_token().unwrap().node_factory();
    let a = args(&f, &f.nil());
    let other = RSession::new_for_gc_tests();
    other.with_active_in(|_| {
        let owner = other.owner_token().unwrap();
        let before = owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap();
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| invoke(&a))).is_err());
        assert_eq!(
            owner
                .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
                .unwrap(),
            before
        );
    });
}

#[test]
fn revocation_during_provider_prevents_result_publication() {
    let s = RSession::new_for_gc_tests();
    s.with_active_in(|instance| {
        let f = s.owner_token().unwrap().node_factory();
        let fired = Rc::new(Cell::new(false));
        let observed = fired.clone();
        let class = s
            .register_altrep_class(
                "anyNA-revoked",
                Provider {
                    mode: Rc::new(Cell::new(0)),
                    hook: Rc::new(move || unsafe {
                        observed.set(true);
                        crate::sexp::instance::revoke_instance_availability(instance);
                    }),
                },
            )
            .unwrap();
        let source = AltrepBuilder::new(class).build().unwrap();
        let ledger = source.allocation().unwrap().heap_identity().byte_ledger();
        let a = args(&f, &source);
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| invoke(&a))).is_err());
        assert!(fired.get());
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
    });
}

fn assert_error(a: &Sexp<'_>, expected: &str) {
    let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| invoke(a))).unwrap_err();
    let error = error
        .downcast_ref::<crate::sexp::context::RError>()
        .expect("R runtime error");
    assert!(error.message.contains(expected), "{}", error.message);
}

fn nested<'s>(f: &SessionNodeFactory<'s>, leaf: &Sexp<'s>, depth: usize) -> Sexp<'s> {
    let mut value = leaf.clone();
    for _ in 0..depth {
        value = list(f, &value);
    }
    value
}

#[test]
fn deep_checked_graph_uses_admitted_frames_and_preserves_shared_dag() {
    let s = RSession::new_for_gc_tests();
    let owner = s.owner_token().unwrap();
    let f = owner.node_factory();
    let depth = if cfg!(miri) { 48 } else { 10_000 };
    let value = nested(&f, &f.nil(), depth);
    let a = args(&f, &value);
    assert!(!invoke(&a));
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    let shared = list(&f, &f.nil());
    let dag = f
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::VECSXP, 2)))
        .unwrap();
    let mut dag = SexpMut::try_from_checked(dag).unwrap();
    for i in 0..2 {
        dag.try_set_vector_elt(i, shared.clone()).unwrap();
    }
    assert!(!invoke(&args(&f, &dag.freeze())));
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
}

#[test]
fn checked_vector_and_pairlist_cycles_fail_and_recover() {
    let s = RSession::new_for_gc_tests();
    let owner = s.owner_token().unwrap();
    let f = owner.node_factory();
    let cycle = list(&f, &f.nil());
    let mut mutable = SexpMut::try_from_checked(cycle.clone()).unwrap();
    mutable.try_set_vector_elt(0, cycle.clone()).unwrap();
    let cycle = mutable.freeze();
    let a = args(&f, &cycle);
    owner.full_gc().unwrap();
    assert_error(&a, "cyclic list in recursive anyNA");
    owner.full_gc().unwrap();
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    let mut mutable = SexpMut::try_from_checked(cycle).unwrap();
    mutable.try_set_vector_elt(0, f.nil()).unwrap();
    assert!(!invoke(&a));
    let pair = f.pairlist_cell(&f.nil(), &f.nil(), &f.nil()).unwrap();
    let mut mutable = SexpMut::try_from_checked(pair.clone()).unwrap();
    mutable.try_set_pairlist_cdr(&pair).unwrap();
    let pair = mutable.freeze();
    let a = args(&f, &pair);
    owner.full_gc().unwrap();
    assert_error(&a, "cyclic list in recursive anyNA");
    owner.full_gc().unwrap();
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    let mut mutable = SexpMut::try_from_checked(pair).unwrap();
    mutable.try_set_pairlist_cdr(&f.nil()).unwrap();
    assert!(!invoke(&a));
    let early = f
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::VECSXP, 2)))
        .unwrap();
    let missing = f
        .wrap(unsafe { crate::sexp::constructors::Rf_ScalarLogical(NA_INTEGER) })
        .unwrap();
    let mut mutable = SexpMut::try_from_checked(early.clone()).unwrap();
    mutable.try_set_vector_elt(0, missing).unwrap();
    mutable.try_set_vector_elt(1, early.clone()).unwrap();
    assert!(invoke(&args(&f, &mutable.freeze())));
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    let mut mutable = SexpMut::try_from_checked(early).unwrap();
    mutable.try_set_vector_elt(1, f.nil()).unwrap();
}

#[test]
fn nonrecursive_checked_pairlist_cycle_is_rejected_before_shallow_fallback() {
    let s = RSession::new_for_gc_tests();
    let owner = s.owner_token().unwrap();
    let f = owner.node_factory();
    let pair = f.pairlist_cell(&f.nil(), &f.nil(), &f.nil()).unwrap();
    let mut mutable = SexpMut::try_from_checked(pair.clone()).unwrap();
    mutable.try_set_pairlist_cdr(&pair).unwrap();
    let pair = mutable.freeze();
    let a = f.pairlist_cell(&pair, &f.nil(), &f.nil()).unwrap();
    owner.full_gc().unwrap();
    assert_error(&a, "cyclic pairlist in anyNA");
    // The compatibility class setter operates on already checked/rooted nodes;
    // all cyclic edges themselves were made with SexpMut's checked API.
    let class = f
        .wrap(unsafe { crate::sexp::constructors::Rf_mkString(c"anyNA_cycle_probe".as_ptr()) })
        .unwrap();
    unsafe {
        crate::eval::attrib_core::R_classgets(pair.as_raw(), class.as_raw());
    }
    assert!(pair.is_object());
    assert_error(&a, "cyclic pairlist in anyNA");
    assert_error(&args(&f, &pair), "cyclic pairlist in anyNA");
    owner.full_gc().unwrap();
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    let mut mutable = SexpMut::try_from_checked(pair).unwrap();
    mutable.try_set_pairlist_cdr(&f.nil()).unwrap();
    let _repaired = mutable.freeze();
    assert!(!invoke(&args(&f, &f.nil())));
}

#[test]
fn workspace_growth_denial_precedes_provider_and_same_cap_recovers_with_gc() {
    let s = RSession::new_for_gc_tests();
    let owner = s.owner_token().unwrap();
    let f = owner.node_factory();
    let observed = Rc::new(Cell::new(0));
    let seen = observed.clone();
    let ledger = owner
        .with_arena(|a| a.heap_identity().byte_ledger())
        .unwrap();
    let class = s
        .register_altrep_class(
            "anyNA-budget",
            Provider {
                mode: Rc::new(Cell::new(0)),
                hook: Rc::new(move || seen.set(ledger.workspace_bytes_for_test())),
            },
        )
        .unwrap();
    let source = AltrepBuilder::new(class).build().unwrap();
    let deep = args(&f, &nested(&f, &source, 8));
    let replacement = args(&f, &nested(&f, &source, 3));
    let small = args(&f, &source);
    owner.full_gc().unwrap();
    let resident = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
    for allowance in [work::capacity_bytes(4) - 1, work::capacity_bytes(4)] {
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(resident + allowance, usize::MAX)))
            .unwrap();
        assert_error(&deep, "anyNA traversal exceeds session memory limit");
        assert_eq!(observed.get(), 0);
        owner.full_gc().unwrap();
        assert_eq!(
            owner
                .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
                .unwrap(),
            (resident, 0)
        );
    }
    // Five active frames need a final capacity of eight. That final buffer
    // fits this cap; replacing the old four-frame allocation needs twelve.
    let final_bytes = work::capacity_bytes(8);
    assert!(final_bytes < work::capacity_bytes(4) + final_bytes);
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(resident + final_bytes, usize::MAX)))
        .unwrap();
    assert_error(&replacement, "anyNA traversal exceeds session memory limit");
    assert_eq!(observed.get(), 0);
    owner.full_gc().unwrap();
    assert_eq!(
        owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap(),
        (resident, 0)
    );
    assert!(invoke(&small));
    assert_eq!(observed.get(), work::capacity_bytes(4));
    assert_eq!(
        owner.with_arena(|a| a.budget().max_bytes).unwrap(),
        resident + final_bytes
    );
    owner.full_gc().unwrap();
    assert_eq!(
        owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap(),
        (resident, 0)
    );
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
        .unwrap();
    assert!(invoke(&deep));
    assert_eq!(observed.get(), work::capacity_bytes(16));
    owner.full_gc().unwrap();
    assert_eq!(
        owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap(),
        (resident, 0)
    );
}
