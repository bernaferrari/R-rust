use super::*;
use crate::sexp::{
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    memory::ArenaBudget,
    object::{SexpError, SexpResult},
    session::RSession,
};
use std::{cell::Cell, rc::Rc};
struct Provider {
    length: i64,
    mode: Rc<Cell<u8>>,
    calls: Rc<Cell<usize>>,
    hook: Rc<dyn Fn()>,
}
impl AltrepClass for Provider {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::REALSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(self.length)
    }
    fn element<'s>(&self, c: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        self.calls.set(self.calls.get() + 1);
        (self.hook)();
        c.gc()?;
        if self.length != 3 {
            return Err(SexpError::Altrep {
                reason: "denied output must not read input",
            });
        }
        if i == 1 {
            match self.mode.get() {
                1 => {
                    return Err(SexpError::Altrep {
                        reason: "parallel extrema provider failure",
                    });
                }
                2 => panic!("parallel extrema provider unwind"),
                3 => {
                    c.object().try_real_elt(i)?;
                }
                _ => {}
            }
        }
        Ok(AltrepElement::Real([3., 1., f64::NAN][i as usize]))
    }
}
fn dense<'s>(f: &SessionNodeFactory<'s>) -> Sexp<'s> {
    let x = f
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::REALSXP, 3)))
        .unwrap();
    let mut x = SexpMut::try_from_checked(x).unwrap();
    for (i, v) in [2., 4., 5.].into_iter().enumerate() {
        x.try_set_real_elt(i as _, v).unwrap();
    }
    x.freeze()
}
fn args<'s>(f: &SessionNodeFactory<'s>, left: &Sexp<'s>, right: Option<&Sexp<'s>>) -> Sexp<'s> {
    let nil = f.nil();
    let tail = if let Some(right) = right {
        f.pairlist_cell(right, &nil, &nil).unwrap()
    } else {
        nil.clone()
    };
    let tail = f.pairlist_cell(left, &tail, &nil).unwrap();
    let flag = f
        .wrap(unsafe { crate::sexp::constructors::Rf_ScalarLogical(0) })
        .unwrap();
    f.pairlist_cell(&flag, &tail, &nil).unwrap()
}
fn invoke<'s>(f: &SessionNodeFactory<'s>, a: &Sexp<'s>) -> Sexp<'s> {
    let nil = f.nil();
    f.wrap(unsafe {
        numeric(
            nil.as_raw(),
            nil.as_raw(),
            a.as_raw(),
            nil.as_raw(),
            true,
            true,
        )
        .unwrap()
    })
    .unwrap()
}

#[test]
fn snapshot_roots_detached_inputs_and_charges_original_workspace() {
    let s = RSession::new_for_gc_tests();
    let owner = s.owner_token().unwrap();
    let f = owner.node_factory();
    let pointer: Rc<Cell<SEXP>> = Rc::new(Cell::new(std::ptr::null_mut()));
    let p = pointer.clone();
    let observe_ledger = owner
        .with_arena(|a| a.heap_identity().byte_ledger())
        .unwrap();
    let observed = Rc::new(Cell::new(0));
    let seen = observed.clone();
    let nil = f.nil().as_raw();
    let left_class = s
        .register_altrep_class(
            "parallel-left",
            Provider {
                length: 3,
                mode: Rc::new(Cell::new(0)),
                calls: Rc::new(Cell::new(0)),
                hook: Rc::new(move || unsafe {
                    let bytes = observe_ledger.workspace_bytes_for_test();
                    seen.set(bytes);
                    assert_eq!(bytes, 2 * std::mem::size_of::<Input<'_>>());
                    crate::sexp::accessors::SETCAR(p.get(), nil);
                    crate::sexp::accessors::SETCDR(p.get(), nil);
                }),
            },
        )
        .unwrap();
    let right_class = s
        .register_altrep_class(
            "parallel-right",
            Provider {
                length: 3,
                mode: Rc::new(Cell::new(0)),
                calls: Rc::new(Cell::new(0)),
                hook: Rc::new(|| {}),
            },
        )
        .unwrap();
    let left = AltrepBuilder::new(left_class).build().unwrap();
    let ln = left.allocation().unwrap().clone();
    let right = AltrepBuilder::new(right_class).build().unwrap();
    let rn = right.allocation().unwrap().clone();
    let a = args(&f, &left, Some(&right));
    pointer.set(a.as_raw());
    drop(left);
    drop(right);
    let result = invoke(&f, &a);
    assert_eq!(result.try_real_elt(0).unwrap(), 3.);
    assert!(result.try_real_elt(2).unwrap().is_nan());
    assert!(observed.get() > 0);
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    owner.full_gc().unwrap();
    assert!(!ln.is_live());
    assert!(!rn.is_live());
}

#[test]
fn workspace_and_partial_outputs_release_and_same_cap_recovers() {
    let s = RSession::new_for_gc_tests();
    let owner = s.owner_token().unwrap();
    let f = owner.node_factory();
    let mode = Rc::new(Cell::new(0));
    let class = s
        .register_altrep_class(
            "parallel-failure",
            Provider {
                length: 3,
                mode: mode.clone(),
                calls: Rc::new(Cell::new(0)),
                hook: Rc::new(|| {}),
            },
        )
        .unwrap();
    let left = AltrepBuilder::new(class).build().unwrap();
    let right = dense(&f);
    let a = args(&f, &left, Some(&right));
    let one = args(&f, &right, None);
    let big_calls = Rc::new(Cell::new(0));
    let class = s
        .register_altrep_class(
            "parallel-large",
            Provider {
                length: 4096,
                mode: Rc::new(Cell::new(0)),
                calls: big_calls.clone(),
                hook: Rc::new(|| {}),
            },
        )
        .unwrap();
    let big = AltrepBuilder::new(class).build().unwrap();
    let big_args = args(&f, &big, Some(&big));
    owner.full_gc().unwrap();
    let resident = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
    for failure in [1, 2, 3] {
        mode.set(failure);
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| invoke(&f, &a))).is_err());
        mode.set(0);
        owner.full_gc().unwrap();
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        assert_eq!(
            owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
            resident
        );
    }
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(resident, 0)))
        .unwrap();
    let single = invoke(&f, &one);
    assert_eq!(single.as_raw(), right.as_raw());
    drop(single);
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    let scratch = 2 * std::mem::size_of::<Input<'_>>();
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(resident + scratch - 1, 0)))
        .unwrap();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| invoke(&f, &a))).is_err());
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    let cap = resident + scratch + 4096;
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(cap, 0)))
        .unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| invoke(&f, &big_args))).is_err()
    );
    assert_eq!(big_calls.get(), 0);
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), cap);
    let result = invoke(&f, &a);
    assert_eq!(result.try_real_elt(0).unwrap(), 2.);
    drop(result);
    owner.full_gc().unwrap();
    assert_eq!(
        owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
        resident
    );
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
        .unwrap();
}
#[test]
fn revoked_provider_releases_original_snapshot_reservation() {
    let s = RSession::new_for_gc_tests();
    s.with_active_in(|instance| {
        let owner = s.owner_token().unwrap();
        let f = owner.node_factory();
        let fired = Rc::new(Cell::new(false));
        let seen = fired.clone();
        let class = s
            .register_altrep_class(
                "parallel-revoke",
                Provider {
                    length: 3,
                    mode: Rc::new(Cell::new(0)),
                    calls: Rc::new(Cell::new(0)),
                    hook: Rc::new(move || unsafe {
                        seen.set(true);
                        crate::sexp::instance::revoke_instance_availability(instance);
                    }),
                },
            )
            .unwrap();
        let left = AltrepBuilder::new(class).build().unwrap();
        let ledger = left.allocation().unwrap().heap_identity().byte_ledger();
        let right = dense(&f);
        let a = args(&f, &left, Some(&right));
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| invoke(&f, &a))).is_err());
        assert!(fired.get());
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
    });
}
#[test]
fn foreign_arguments_cannot_charge_another_owner() {
    let original = RSession::new_for_gc_tests();
    let f = original.owner_token().unwrap().node_factory();
    let left = dense(&f);
    let right = dense(&f);
    let a = args(&f, &left, Some(&right));
    let other = RSession::new_for_gc_tests();
    other.with_active_in(|_| {
        let owner = other.owner_token().unwrap();
        let f = owner.node_factory();
        let before = owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap();
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| invoke(&f, &a))).is_err());
        assert_eq!(
            owner
                .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
                .unwrap(),
            before
        );
    });
}
