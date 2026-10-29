use super::*;
use crate::sexp::{
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    memory::ArenaBudget,
    object::{SessionNodeFactory, SexpError, SexpMut, SexpResult},
    session::RSession,
};
use std::{cell::Cell, rc::Rc};
struct Provider {
    kind: SEXPTYPE,
    length: i64,
    mode: Rc<Cell<u8>>,
    calls: Rc<Cell<usize>>,
    hook: Rc<dyn Fn()>,
}
impl AltrepClass for Provider {
    fn vector_type(&self) -> SEXPTYPE {
        self.kind
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(self.length)
    }
    fn element<'s>(&self, c: &AltrepContext<'s>, i: i64) -> SexpResult<AltrepElement<'s>> {
        self.calls.set(self.calls.get() + 1);
        (self.hook)();
        c.gc()?;
        if i == 1 {
            match self.mode.get() {
                1 => {
                    return Err(SexpError::Altrep {
                        reason: "Boolean provider error",
                    });
                }
                2 => panic!("Boolean provider unwind"),
                3 => {
                    c.object().try_integer_elt(i)?;
                }
                _ => {}
            }
        }
        if self.kind == SEXPTYPE::LGLSXP {
            Ok(AltrepElement::Logical(if self.length == 1 {
                NA_INTEGER
            } else {
                [NA_INTEGER, 0, 1][i as usize]
            }))
        } else if self.kind == SEXPTYPE::VECSXP {
            Ok(AltrepElement::List(c.data1()?.try_vector_elt(i)?))
        } else {
            Ok(AltrepElement::Integer([1, 0, NA_INTEGER][i as usize]))
        }
    }
}
fn dense<'s>(f: &SessionNodeFactory<'s>, kind: SEXPTYPE, values: &[i32]) -> Sexp<'s> {
    let v = f
        .allocate(|a| Some(a.alloc_vector(kind, values.len() as i64)))
        .unwrap();
    let mut v = SexpMut::try_from_checked(v).unwrap();
    for (i, x) in values.iter().enumerate() {
        if kind == SEXPTYPE::LGLSXP {
            v.try_set_logical_elt(i as i64, *x).unwrap();
        } else {
            v.try_set_integer_elt(i as i64, *x).unwrap();
        }
    }
    v.freeze()
}
fn arguments<'s>(f: &SessionNodeFactory<'s>, values: &[(Sexp<'s>, bool)]) -> Sexp<'s> {
    let nil = f.nil();
    let tag = f
        .wrap(unsafe { crate::sexp::symbol::Rf_install(c"na.rm".as_ptr()) })
        .unwrap();
    let mut a = nil.clone();
    for (v, flag) in values.iter().rev() {
        a = f
            .pairlist_cell(v, &a, if *flag { &tag } else { &nil })
            .unwrap();
    }
    a
}
fn invoke<'s>(f: &SessionNodeFactory<'s>, args: &Sexp<'_>, any: bool) -> Sexp<'s> {
    let nil = f.nil();
    f.wrap(unsafe { run(nil.as_raw(), nil.as_raw(), args.as_raw(), nil.as_raw(), any) })
        .unwrap()
}
fn assert_error(f: &SessionNodeFactory<'_>, a: &Sexp<'_>) {
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| invoke(f, a, true))).is_err());
}
fn register<'s>(
    s: &'s RSession,
    name: &str,
    kind: SEXPTYPE,
    length: i64,
    mode: Rc<Cell<u8>>,
    calls: Rc<Cell<usize>>,
    hook: Rc<dyn Fn()>,
) -> crate::sexp::altrep::AltrepClassHandle<'s> {
    s.register_altrep_class(
        name,
        Provider {
            kind,
            length,
            mode,
            calls,
            hook,
        },
    )
    .unwrap()
}

#[test]
fn flag_callback_detaches_args_but_snapshot_roots_and_original_charge_survive() {
    let s = RSession::new_for_gc_tests();
    let owner = s.owner_token().unwrap();
    let f = owner.node_factory();
    let pointer = Rc::new(Cell::new(std::ptr::null_mut()));
    let captured = pointer.clone();
    let nil = f.nil().as_raw();
    let ledger = owner
        .with_arena(|a| a.heap_identity().byte_ledger())
        .unwrap();
    let observed = Rc::new(Cell::new(0));
    let seen = observed.clone();
    let class = register(
        &s,
        "Boolean-flag",
        SEXPTYPE::LGLSXP,
        1,
        Rc::new(Cell::new(0)),
        Rc::new(Cell::new(0)),
        Rc::new(move || unsafe {
            seen.set(ledger.workspace_bytes_for_test());
            crate::sexp::accessors::SETCAR(captured.get(), nil);
            crate::sexp::accessors::SETCDR(captured.get(), nil);
        }),
    );
    let flag = AltrepBuilder::new(class).build().unwrap();
    for any in [true, false] {
        let source = dense(&f, SEXPTYPE::LGLSXP, &[NA_INTEGER, NA_INTEGER, NA_INTEGER]);
        let original = source.allocation().unwrap().clone();
        let a = arguments(&f, &[(source.clone(), false), (flag.clone(), true)]);
        pointer.set(a.as_raw());
        drop(source);
        let result = invoke(&f, &a, any);
        assert_eq!(result.try_logical_elt(0).unwrap(), if any { 0 } else { 1 });
        assert_eq!(observed.get(), input::bytes(2));
        owner.full_gc().unwrap();
        assert!(!original.is_live());
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }
}

#[test]
fn full_integer_coercion_errors_and_large_output_denial_release_and_recover() {
    let s = RSession::new_for_gc_tests();
    let owner = s.owner_token().unwrap();
    let f = owner.node_factory();
    let mode = Rc::new(Cell::new(0));
    let calls = Rc::new(Cell::new(0));
    let class = register(
        &s,
        "Boolean-integer",
        SEXPTYPE::INTSXP,
        3,
        mode.clone(),
        calls.clone(),
        Rc::new(|| {}),
    );
    let source = AltrepBuilder::new(class).build().unwrap();
    let a = arguments(&f, &[(source.clone(), false)]);
    drop(invoke(&f, &a, true));
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
    let big = AltrepBuilder::new(register(
        &s,
        "Boolean-large",
        SEXPTYPE::INTSXP,
        4096,
        mode,
        big_calls.clone(),
        Rc::new(|| {}),
    ))
    .build()
    .unwrap();
    let big_args = arguments(&f, &[(big, false)]);
    owner.full_gc().unwrap();
    let resident = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
    let cap = resident + input::bytes(1) + 4096;
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(cap, usize::MAX)))
        .unwrap();
    assert_error(&f, &big_args);
    assert_eq!(big_calls.get(), 0);
    assert_eq!(invoke(&f, &a, true).try_logical_elt(0).unwrap(), 1);
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
fn snapshot_denial_precedes_reads_and_cyclic_args_repair_under_same_cap() {
    let s = RSession::new_for_gc_tests();
    let owner = s.owner_token().unwrap();
    let f = owner.node_factory();
    let calls = Rc::new(Cell::new(0));
    let class = register(
        &s,
        "Boolean-snapshot",
        SEXPTYPE::INTSXP,
        3,
        Rc::new(Cell::new(0)),
        calls.clone(),
        Rc::new(|| {}),
    );
    let source = AltrepBuilder::new(class).build().unwrap();
    let many = arguments(&f, &vec![(source.clone(), false); 16]);
    let small = arguments(&f, &[(source.clone(), false)]);
    let cycle = f.pairlist_cell(&source, &f.nil(), &f.nil()).unwrap();
    let mut mutable = SexpMut::try_from_checked(cycle.clone()).unwrap();
    mutable.try_set_pairlist_cdr(&cycle).unwrap();
    let cycle = mutable.freeze();
    owner.full_gc().unwrap();
    let resident = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
    let cap = resident + input::bytes(16) - 1;
    owner
        .with_arena(|a| a.set_budget(ArenaBudget::new(cap, usize::MAX)))
        .unwrap();
    assert_error(&f, &many);
    assert_eq!(calls.get(), 0);
    assert_error(&f, &cycle);
    assert_eq!(calls.get(), 0);
    let mut mutable = SexpMut::try_from_checked(cycle).unwrap();
    mutable.try_set_pairlist_cdr(&f.nil()).unwrap();
    assert_eq!(invoke(&f, &small, true).try_logical_elt(0).unwrap(), 1);
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
fn foreign_arguments_and_provider_revocation_preserve_original_accounting() {
    let s = RSession::new_for_gc_tests();
    let f = s.owner_token().unwrap().node_factory();
    let a = arguments(&f, &[(dense(&f, SEXPTYPE::LGLSXP, &[1]), false)]);
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
        let class = register(
            &s,
            "Boolean-revoke",
            SEXPTYPE::INTSXP,
            3,
            Rc::new(Cell::new(0)),
            Rc::new(Cell::new(0)),
            Rc::new(move || unsafe {
                seen.set(true);
                crate::sexp::instance::revoke_instance_availability(instance);
            }),
        );
        let source = AltrepBuilder::new(class).build().unwrap();
        let ledger = source.allocation().unwrap().heap_identity().byte_ledger();
        let a = arguments(&f, &[(source, false)]);
        assert_error(&f, &a);
        assert!(fired.get());
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
    });
}

#[test]
fn list_and_scalar_providers_collect_during_bounded_two_pass_coercion() {
    let s = RSession::new_for_gc_tests();
    let owner = s.owner_token().unwrap();
    let f = owner.node_factory();
    let scalar_calls = Rc::new(Cell::new(0));
    let scalar = AltrepBuilder::new(register(
        &s,
        "Boolean-list-scalar",
        SEXPTYPE::INTSXP,
        1,
        Rc::new(Cell::new(0)),
        scalar_calls.clone(),
        Rc::new(|| {}),
    ))
    .build()
    .unwrap();
    let data = f
        .allocate(|a| Some(a.alloc_vector(SEXPTYPE::VECSXP, 3)))
        .unwrap();
    let mut data = SexpMut::try_from_checked(data).unwrap();
    for (i, v) in [
        scalar.clone(),
        dense(&f, SEXPTYPE::INTSXP, &[]),
        dense(&f, SEXPTYPE::INTSXP, &[0]),
    ]
    .into_iter()
    .enumerate()
    {
        data.try_set_vector_elt(i as i64, v).unwrap();
    }
    let pointer = Rc::new(Cell::new(std::ptr::null_mut()));
    let captured = pointer.clone();
    let nil = f.nil().as_raw();
    let calls = Rc::new(Cell::new(0));
    let class = register(
        &s,
        "Boolean-list",
        SEXPTYPE::VECSXP,
        3,
        Rc::new(Cell::new(0)),
        calls.clone(),
        Rc::new(move || unsafe {
            crate::sexp::accessors::SETCAR(captured.get(), nil);
            crate::sexp::accessors::SETCDR(captured.get(), nil);
        }),
    );
    let source = AltrepBuilder::new(class)
        .data1(data.freeze())
        .build()
        .unwrap();
    let original = source.allocation().unwrap().clone();
    let scalar_original = scalar.allocation().unwrap().clone();
    let a = arguments(&f, &[(source.clone(), false)]);
    pointer.set(a.as_raw());
    drop(source);
    drop(scalar);
    assert_eq!(invoke(&f, &a, true).try_logical_elt(0).unwrap(), 1);
    assert_eq!(calls.get(), 6);
    assert_eq!(scalar_calls.get(), 1);
    owner.full_gc().unwrap();
    assert!(!original.is_live());
    assert!(!scalar_original.is_live());
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
}
