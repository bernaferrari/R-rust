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
    hook: Rc<dyn Fn(SEXP)>,
}
impl AltrepClass for Provider {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::VECSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(self.length)
    }
    fn element<'s>(&self, c: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        self.calls.set(self.calls.get() + 1);
        (self.hook)(c.object().as_raw());
        c.gc()?;
        if i == 1 {
            match self.mode.get() {
                1 => {
                    return Err(SexpError::Altrep {
                        reason: "lengths provider error",
                    });
                }
                2 => panic!("lengths provider unwind"),
                3 => {
                    c.object().try_vector_elt(i)?;
                }
                _ => {}
            }
        }
        Ok(AltrepElement::List(c.data1()?.try_vector_elt(i)?))
    }
}
struct Long;
impl AltrepClass for Long {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::REALSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(i32::MAX as i64 + 1)
    }
    fn element<'s>(&self, _: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
        panic!("lengths must not materialize long child")
    }
}
fn vector<'s>(f: &SessionNodeFactory<'s>, n: i64) -> Sexp<'s> {
    f.allocate(|a| Some(a.alloc_vector(SEXPTYPE::INTSXP, n)))
        .unwrap()
}
fn list<'s>(f: &SessionNodeFactory<'s>, children: &[Sexp<'s>]) -> Sexp<'s> {
    let value = f
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::VECSXP, children.len() as i64)))
        .unwrap();
    let mut value = SexpMut::try_from_checked(value).unwrap();
    for (i, child) in children.iter().enumerate() {
        value.try_set_vector_elt(i as i64, child.clone()).unwrap();
    }
    value.freeze()
}
fn args<'s>(f: &SessionNodeFactory<'s>, value: &Sexp<'s>) -> Sexp<'s> {
    f.pairlist_cell(value, &f.nil(), &f.nil()).unwrap()
}
fn invoke<'s>(f: &SessionNodeFactory<'s>, a: &Sexp<'s>) -> Sexp<'s> {
    let nil = f.nil();
    f.wrap(unsafe { run(nil.as_raw(), nil.as_raw(), a.as_raw(), nil.as_raw()) })
        .unwrap()
}
fn provider<'s>(
    s: &'s RSession,
    f: &SessionNodeFactory<'s>,
    data: Sexp<'s>,
    length: i64,
    mode: Rc<Cell<u8>>,
    calls: Rc<Cell<usize>>,
    hook: Rc<dyn Fn(SEXP)>,
) -> Sexp<'s> {
    let class = s
        .register_altrep_class(
            if length == 4096 {
                "lengths-large-provider"
            } else {
                "lengths-provider"
            },
            Provider {
                length,
                mode,
                calls,
                hook,
            },
        )
        .unwrap();
    AltrepBuilder::new(class).data1(data).build().unwrap()
}
fn assert_error(f: &SessionNodeFactory<'_>, a: &Sexp<'_>) {
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| invoke(f, a))).is_err());
}

#[test]
fn provider_gc_detachment_and_post_callback_shape_keep_independent_roots() {
    let s = RSession::new_for_gc_tests();
    let owner = s.owner_token().unwrap();
    let f = owner.node_factory();
    let data = list(&f, &[f.nil(), vector(&f, 2), vector(&f, 3)]);
    let a_pointer = Rc::new(Cell::new(std::ptr::null_mut()));
    let captured = a_pointer.clone();
    let dim = vector(&f, 2);
    let mut dim = SexpMut::try_from_checked(dim).unwrap();
    dim.try_set_integer_elt(0, 1).unwrap();
    dim.try_set_integer_elt(1, 3).unwrap();
    let dim = dim.freeze();
    let dim_ptr = dim.as_raw();
    let nil = f.nil().as_raw();
    let source = provider(
        &s,
        &f,
        data,
        3,
        Rc::new(Cell::new(0)),
        Rc::new(Cell::new(0)),
        Rc::new(move |x| unsafe {
            crate::sexp::accessors::SETCAR(captured.get(), nil);
            crate::eval::attrib_core::setAttrib(
                x,
                crate::eval::attrib_core::R_DimSymbol(),
                dim_ptr,
            );
        }),
    );
    let original = source.allocation().unwrap().clone();
    let a = args(&f, &source);
    a_pointer.set(a.as_raw());
    drop(source);
    let result = invoke(&f, &a);
    assert_eq!(result.try_integer_elt(0).unwrap(), 0);
    assert_eq!(result.try_integer_elt(1).unwrap(), 2);
    assert_eq!(result.try_integer_elt(2).unwrap(), 3);
    let shape = attribute(
        &result,
        &f.wrap(unsafe { crate::eval::attrib_core::R_DimSymbol() })
            .unwrap(),
    );
    assert_eq!(shape.try_integer_elt(0).unwrap(), 1);
    assert_eq!(shape.try_integer_elt(1).unwrap(), 3);
    owner.full_gc().unwrap();
    assert!(!original.is_live());
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
}

#[test]
fn provider_failure_and_denied_output_restore_original_budget_and_recover() {
    let s = RSession::new_for_gc_tests();
    let owner = s.owner_token().unwrap();
    let f = owner.node_factory();
    let data = list(&f, &[f.nil(), vector(&f, 2), vector(&f, 3)]);
    let mode = Rc::new(Cell::new(0));
    let calls = Rc::new(Cell::new(0));
    let source = provider(
        &s,
        &f,
        data.clone(),
        3,
        mode.clone(),
        calls.clone(),
        Rc::new(|_| {}),
    );
    let a = args(&f, &source);
    drop(invoke(&f, &a));
    owner.full_gc().unwrap();
    let resident = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
    for failure in [1, 2, 3] {
        mode.set(failure);
        assert_error(&f, &a);
        owner.full_gc().unwrap();
        assert_eq!(
            owner
                .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
                .unwrap(),
            (resident, 0)
        );
    }
    mode.set(0);
    let big_calls = Rc::new(Cell::new(0));
    let big = provider(&s, &f, data, 4096, mode, big_calls.clone(), Rc::new(|_| {}));
    let big_args = args(&f, &big);
    owner.full_gc().unwrap();
    let resident = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
    let cap = resident + 4096;
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(cap, usize::MAX)))
        .unwrap();
    assert_error(&f, &big_args);
    assert_eq!(big_calls.get(), 0);
    assert_eq!(invoke(&f, &a).try_integer_elt(2).unwrap(), 3);
    owner.full_gc().unwrap();
    assert_eq!(
        owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap(),
        (resident, 0)
    );
    assert_eq!(owner.with_arena(|a| a.budget().max_bytes).unwrap(), cap);
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
        .unwrap();
}

#[test]
fn long_child_restarts_in_real_width_without_materializing_and_peak_denial_recovers() {
    let s = RSession::new_for_gc_tests();
    let owner = s.owner_token().unwrap();
    let f = owner.node_factory();
    let class = s.register_altrep_class("lengths-long", Long).unwrap();
    let long = AltrepBuilder::new(class).build().unwrap();
    let data = list(&f, &[f.nil(), vector(&f, 2), long]);
    let calls = Rc::new(Cell::new(0));
    let source = provider(
        &s,
        &f,
        data,
        3,
        Rc::new(Cell::new(0)),
        calls.clone(),
        Rc::new(|_| {}),
    );
    let a = args(&f, &source);
    let result = invoke(&f, &a);
    assert_eq!(result.typeof_(), SEXPTYPE::REALSXP);
    assert_eq!(result.try_real_elt(0).unwrap(), 0.);
    assert_eq!(result.try_real_elt(1).unwrap(), 2.);
    assert_eq!(result.try_real_elt(2).unwrap(), 2147483648.);
    assert_eq!(calls.get(), 6);
    drop(result);
    let small = args(&f, &vector(&f, 3));
    owner.full_gc().unwrap();
    let (resident, nodes) = owner
        .with_arena(|a| (a.total_bytes_allocated(), a.node_count()))
        .unwrap();
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(usize::MAX, nodes + 1)))
        .unwrap();
    assert_error(&f, &a);
    owner.full_gc().unwrap();
    assert_eq!(
        owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap(),
        (resident, 0)
    );
    assert_eq!(invoke(&f, &small).try_integer_elt(0).unwrap(), 1);
    owner.full_gc().unwrap();
    assert_eq!(
        owner.with_arena(|a| a.budget().max_nodes).unwrap(),
        nodes + 1
    );
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
        .unwrap();
}

#[test]
fn foreign_owner_and_revocation_cannot_publish_or_charge_another_owner() {
    let s = RSession::new_for_gc_tests();
    let f = s.owner_token().unwrap().node_factory();
    let a = args(&f, &vector(&f, 3));
    let other = RSession::new_for_gc_tests();
    other.with_active_in(|_| {
        let owner = other.owner_token().unwrap();
        let before = owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap();
        assert_error(&owner.node_factory(), &a);
        assert_eq!(
            owner
                .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
                .unwrap(),
            before
        );
    });
    s.with_active_in(|instance| {
        let f = s.owner_token().unwrap().node_factory();
        let fired = Rc::new(Cell::new(false));
        let seen = fired.clone();
        let source = provider(
            &s,
            &f,
            list(&f, &[f.nil(), f.nil(), f.nil()]),
            3,
            Rc::new(Cell::new(0)),
            Rc::new(Cell::new(0)),
            Rc::new(move |_| unsafe {
                seen.set(true);
                crate::sexp::instance::revoke_instance_availability(instance);
            }),
        );
        let a = args(&f, &source);
        let ledger = source.allocation().unwrap().heap_identity().byte_ledger();
        assert_error(&f, &a);
        assert!(fired.get());
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
    });
}

#[test]
fn checked_pairlist_cycles_fail_without_recursive_walk_and_recover() {
    let s = RSession::new_for_gc_tests();
    let owner = s.owner_token().unwrap();
    let f = owner.node_factory();
    let pair = f.pairlist_cell(&f.nil(), &f.nil(), &f.nil()).unwrap();
    let mut mutable = SexpMut::try_from_checked(pair.clone()).unwrap();
    mutable.try_set_pairlist_cdr(&pair).unwrap();
    let pair = mutable.freeze();
    let a = args(&f, &list(&f, &[pair.clone()]));
    owner.full_gc().unwrap();
    assert_error(&f, &a);
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    let mut mutable = SexpMut::try_from_checked(pair).unwrap();
    mutable.try_set_pairlist_cdr(&f.nil()).unwrap();
    assert_eq!(invoke(&f, &a).try_integer_elt(0).unwrap(), 1);
}

#[test]
fn explicitly_missing_use_names_retains_default() {
    let s = RSession::new_for_gc_tests();
    let f = s.owner_token().unwrap().node_factory();
    let source = vector(&f, 3);
    let missing = f
        .wrap(unsafe { crate::sexp::globals::R_MissingArg() })
        .unwrap();
    let tail = f.pairlist_cell(&missing, &f.nil(), &f.nil()).unwrap();
    let a = f.pairlist_cell(&source, &tail, &f.nil()).unwrap();
    let result = invoke(&f, &a);
    assert_eq!(result.typeof_(), SEXPTYPE::INTSXP);
    for i in 0..3 {
        assert_eq!(result.try_integer_elt(i).unwrap(), 1);
    }
}
