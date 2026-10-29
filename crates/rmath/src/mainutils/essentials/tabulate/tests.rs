use super::*;
use crate::sexp::{
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    memory::ArenaBudget,
    object::{SessionNodeFactory, Sexp, SexpError, SexpResult},
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
        SEXPTYPE::INTSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(self.length)
    }
    fn element<'s>(&self, context: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        self.calls.set(self.calls.get() + 1);
        (self.hook)();
        context.gc()?;
        if i == 1 {
            match self.mode.get() {
                1 => {
                    return Err(SexpError::Altrep {
                        reason: "tabulate provider failure",
                    });
                }
                2 => panic!("tabulate provider unwind"),
                3 => {
                    context.object().try_integer_elt(i)?;
                }
                _ => {}
            }
        }
        let value = match self.length {
            1 => 3,
            5 => [1, 2, 2, 0, NA_INTEGER][i as usize],
            _ => {
                return Err(SexpError::Altrep {
                    reason: "zero-bin table must not read huge input",
                });
            }
        };
        Ok(AltrepElement::Integer(value))
    }
}
fn scalar<'s>(f: &SessionNodeFactory<'s>, v: i32) -> Sexp<'s> {
    let x = f
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, 1)))
        .unwrap();
    let mut x = SexpMut::try_from_checked(x).unwrap();
    x.try_set_integer_elt(0, v).unwrap();
    x.freeze()
}
fn arguments<'s>(f: &SessionNodeFactory<'s>, x: &Sexp<'s>, n: &Sexp<'s>) -> Sexp<'s> {
    let nil = f.nil();
    let tail = f.pairlist_cell(n, &nil, &nil).unwrap();
    f.pairlist_cell(x, &tail, &nil).unwrap()
}
fn invoke<'s>(f: &SessionNodeFactory<'s>, a: &Sexp<'s>) -> Sexp<'s> {
    let nil = f.nil();
    f.wrap(unsafe { evaluate(nil.as_raw(), nil.as_raw(), a.as_raw(), nil.as_raw()) })
        .unwrap()
}

#[test]
fn tabulate_roots_detached_arguments_and_keeps_zero_long_input_lazy() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let f = owner.node_factory();
    let pointer: Rc<Cell<SEXP>> = Rc::new(Cell::new(std::ptr::null_mut()));
    let observed = pointer.clone();
    let nil = f.nil();
    let nil_ptr = nil.as_raw();
    let bins = session
        .register_altrep_class(
            "tabulate-bins",
            Provider {
                length: 5,
                mode: Rc::new(Cell::new(0)),
                calls: Rc::new(Cell::new(0)),
                hook: Rc::new(|| {}),
            },
        )
        .unwrap();
    let size = session
        .register_altrep_class(
            "tabulate-size",
            Provider {
                length: 1,
                mode: Rc::new(Cell::new(0)),
                calls: Rc::new(Cell::new(0)),
                hook: Rc::new(move || unsafe {
                    let a = observed.get();
                    crate::sexp::accessors::SETCAR(a, nil_ptr);
                    crate::sexp::accessors::SETCDR(a, nil_ptr);
                }),
            },
        )
        .unwrap();
    let input = AltrepBuilder::new(bins).build().unwrap();
    let input_node = input.allocation().unwrap().clone();
    let n = AltrepBuilder::new(size).build().unwrap();
    let size_node = n.allocation().unwrap().clone();
    let a = arguments(&f, &input, &n);
    pointer.set(a.as_raw());
    drop(input);
    drop(n);
    let y = invoke(&f, &a);
    assert_eq!(
        (0..3)
            .map(|i| y.try_integer_elt(i).unwrap())
            .collect::<Vec<_>>(),
        vec![1, 2, 0]
    );
    owner.full_gc().unwrap();
    assert!(!input_node.is_live());
    assert!(!size_node.is_live());
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    let calls = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "tabulate-virtual-long",
            Provider {
                length: i64::from(i32::MAX) + 1,
                mode: Rc::new(Cell::new(0)),
                calls: calls.clone(),
                hook: Rc::new(|| {}),
            },
        )
        .unwrap();
    let input = AltrepBuilder::new(class).build().unwrap();
    let n = scalar(&f, 0);
    let a = arguments(&f, &input, &n);
    let y = invoke(&f, &a);
    assert_eq!(y.typeof_(), SEXPTYPE::REALSXP);
    assert_eq!(y.len(), 0);
    assert_eq!(calls.get(), 0);
    assert!(!crate::sexp::altrep::is_materialized(&input));
}

#[test]
fn tabulate_failed_counts_release_and_same_limit_recovers() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let f = owner.node_factory();
    let mode = Rc::new(Cell::new(0));
    let class = session
        .register_altrep_class(
            "tabulate-failure",
            Provider {
                length: 5,
                mode: mode.clone(),
                calls: Rc::new(Cell::new(0)),
                hook: Rc::new(|| {}),
            },
        )
        .unwrap();
    let input = AltrepBuilder::new(class).build().unwrap();
    let n = scalar(&f, 3);
    let a = arguments(&f, &input, &n);
    let large = scalar(&f, 4096);
    let big = arguments(&f, &input, &large);
    owner.full_gc().unwrap();
    let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
    for failure in [1, 2, 3] {
        mode.set(failure);
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| invoke(&f, &a))).is_err());
        mode.set(0);
        owner.full_gc().unwrap();
        assert_eq!(
            owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
            before
        );
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        let y = invoke(&f, &a);
        assert_eq!(y.try_integer_elt(1).unwrap(), 2);
        drop(y);
        owner.full_gc().unwrap();
        assert_eq!(
            owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
            before
        );
    }
    let cap = before + 1024;
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(cap, 0)))
        .unwrap();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| invoke(&f, &big))).is_err());
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), cap);
    let y = invoke(&f, &a);
    assert_eq!(y.try_integer_elt(1).unwrap(), 2);
    assert!(owner.with_arena(|a| a.total_bytes_allocated()).unwrap() <= cap);
    drop(y);
    owner.full_gc().unwrap();
    assert_eq!(
        owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
        before
    );
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
        .unwrap();
}

#[test]
fn tabulate_revoked_provider_cannot_publish_partial_counts() {
    let session = RSession::new_for_gc_tests();
    session.with_active_in(|instance| {
        let f = session.owner_token().unwrap().node_factory();
        let fired = Rc::new(Cell::new(false));
        let observed = fired.clone();
        let class = session
            .register_altrep_class(
                "tabulate-revocation",
                Provider {
                    length: 5,
                    mode: Rc::new(Cell::new(0)),
                    calls: Rc::new(Cell::new(0)),
                    hook: Rc::new(move || unsafe {
                        observed.set(true);
                        crate::sexp::instance::revoke_instance_availability(instance);
                    }),
                },
            )
            .unwrap();
        let x = AltrepBuilder::new(class).build().unwrap();
        let n = scalar(&f, 3);
        let a = arguments(&f, &x, &n);
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| invoke(&f, &a))).is_err());
        assert!(fired.get());
    });
}

#[test]
fn tabulate_foreign_arguments_are_rejected_before_traversal() {
    let original = RSession::new_for_gc_tests();
    let f = original.owner_token().unwrap().node_factory();
    let x = scalar(&f, 1);
    let n = scalar(&f, 3);
    let a = arguments(&f, &x, &n);
    let other = RSession::new_for_gc_tests();
    other.with_active_in(|_| {
        let other_f = other.owner_token().unwrap().node_factory();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| invoke(&other_f, &a)))
                .is_err()
        );
    });
}
