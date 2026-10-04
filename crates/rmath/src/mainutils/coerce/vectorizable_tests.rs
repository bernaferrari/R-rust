use crate::sexp::{
    RSession, SEXPTYPE,
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    object::{Sexp, SexpError, SexpResult},
    owner::with_runtime,
};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

#[test]
fn vectorizable_predicate_matches_gnu_null_list_pairlist_and_expression_categories() {
    let mut session = RSession::new_for_gc_tests();
    for (expression, expected) in [
        ("NULL", true),
        ("list()", true),
        ("pairlist()", true),
        ("pairlist(1L,2L)", true),
        ("pairlist(integer(),TRUE)", true),
        ("list(expression(1L))", true),
        ("list(list())", true),
        ("list(NULL)", false),
        ("pairlist(1:2)", false),
        ("expression(1L)", false),
        ("1L", false),
        ("quote(a+b)", false),
    ] {
        let value = session
            .eval_code_with_output_capture(expression)
            .0
            .unwrap()
            .into_owned()
            .unwrap();
        let actual = session.with_active(|| unsafe { super::isVectorizable(value.as_raw()) });
        assert_eq!(actual, expected, "{expression}");
    }
}

#[test]
fn vectorizable_public_pairlist_and_expression_coercion_follow_gnu() {
    let mut session = RSession::new_for_gc_tests();
    for expression in [
        "identical(as.integer(pairlist(1L,2L)),c(1L,2L))",
        "identical(as.logical(pairlist(logical(),TRUE)),c(NA,TRUE))",
        "identical(as.integer(list(integer(),2L)),c(NA_integer_,2L))",
        "inherits(try(as.integer(expression(1L)),silent=TRUE),'try-error')",
    ] {
        let actual = session
            .eval_code_with_output_capture(expression)
            .0
            .unwrap_or_else(|error| panic!("{expression}: {error}"));
        assert_eq!(actual.try_logical_elt(0).unwrap(), 1, "{expression}");
    }
}

#[test]
fn vectorizable_checked_walk_rejects_cyclic_and_improper_pairlists() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        let factory = owner.node_factory().unwrap();
        let scalar = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
            .unwrap();
        let cell = factory
            .pairlist_cell(&scalar, &factory.nil(), &factory.nil())
            .unwrap()
            .into_owned()
            .unwrap();
        unsafe { crate::sexp::accessors::SETCDR(cell.as_raw(), cell.as_raw()) };
        let cycle = with_runtime(&owner, |access| {
            super::vectorizable::check(cell.clone(), access)
        })
        .unwrap()
        .unwrap_err();
        assert!(cycle.to_string().contains("cyclic pairlist"));
        unsafe { crate::sexp::accessors::SETCDR(cell.as_raw(), scalar.as_raw()) };
        let improper = with_runtime(&owner, |access| {
            super::vectorizable::check(cell.clone(), access)
        })
        .unwrap()
        .unwrap_err();
        assert!(improper.to_string().contains("improper pairlist"));
        // Restore the intentional malformed fixture before unrelated traversal.
        unsafe { crate::sexp::accessors::SETCDR(cell.as_raw(), factory.nil().as_raw()) };
    });
}

#[test]
fn vectorizable_checked_walk_rejects_foreign_runtime_before_provider_use() {
    let mut original = RSession::new_for_gc_tests();
    let value = original
        .eval_code_with_output_capture("list(1L)")
        .0
        .unwrap()
        .into_owned()
        .unwrap();
    let other = RSession::new_for_gc_tests();
    other.with_active(|| {
        let owner = other.owner_token().unwrap().weak_owner().unwrap();
        assert!(
            with_runtime(&owner, |access| super::vectorizable::check(value, access))
                .unwrap()
                .is_err()
        );
    });
}

struct CollectingList {
    facade: Weak<RefCell<Option<RSession>>>,
    calls: Rc<Cell<usize>>,
    action: u8,
}
impl AltrepClass for CollectingList {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::VECSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(1)
    }
    fn element<'s>(&self, context: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
        self.calls.set(self.calls.get() + 1);
        let child = context.data2()?;
        let identity = child.allocation()?.clone();
        // Remove the only source edge, then collect while this independent
        // selected child is retained. No parsed expression supports it.
        context.set_data2(Sexp::nil())?;
        context.gc()?;
        assert!(
            identity.is_live(),
            "detached selected child survives full GC"
        );
        if self.action == 2 {
            self.facade
                .upgrade()
                .unwrap()
                .borrow_mut()
                .as_mut()
                .unwrap()
                .close();
        }
        if self.action > 0 {
            std::panic::panic_any(731_u32);
        }
        Ok(AltrepElement::List(child))
    }
}
fn exercise_provider(action: u8) {
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let calls = Rc::new(Cell::new(0));
    let (owner, value) = {
        let borrow = facade.borrow();
        let session = borrow.as_ref().unwrap();
        session.with_active(|| {
            let owner = session.owner_token().unwrap().weak_owner().unwrap();
            let factory = owner.node_factory().unwrap();
            let child = factory
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
                .unwrap();
            let class = session
                .register_altrep_class(
                    "vectorizable.collecting.list",
                    CollectingList {
                        facade: Rc::downgrade(&facade),
                        calls: calls.clone(),
                        action,
                    },
                )
                .unwrap()
                .into_owned()
                .unwrap();
            let value = AltrepBuilder::new(class).data2(child).build().unwrap();
            (owner, value)
        })
    };
    let pin = owner.pin().unwrap();
    let instance = pin.as_ptr();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(instance, || {
            with_runtime(&owner, |access| super::vectorizable::check(value, access))
        })
    }));
    assert_eq!(calls.get(), 1);
    assert_eq!(unsafe { (*instance).memory_state.in_gc }, 0);
    match action {
        0 => assert!(outcome.unwrap().unwrap().unwrap()),
        1 => assert_eq!(*outcome.unwrap_err().downcast::<u32>().unwrap(), 731),
        _ => assert!(matches!(outcome.unwrap(), Err(SexpError::RootUnavailable))),
    }
    if action == 2 {
        assert!(!facade.borrow().as_ref().unwrap().is_active());
        let mut replacement = RSession::new_for_gc_tests();
        assert_eq!(
            replacement
                .eval_code_with_output_capture("1L+1L")
                .0
                .unwrap()
                .try_integer_elt(0)
                .unwrap(),
            2
        );
    }
}
#[test]
fn vectorizable_provider_retains_detached_child_through_actual_full_gc() {
    exercise_provider(0);
}
#[test]
fn vectorizable_provider_preserves_live_unwind_and_gc_cleanup() {
    exercise_provider(1);
}
#[test]
fn vectorizable_provider_revocation_denies_publication_and_unwind() {
    exercise_provider(2);
}
