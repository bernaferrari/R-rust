use crate::sexp::{RSession, ffi::SEXP, object::Sexp};

fn source(session: &mut RSession, text: &str) -> Sexp<'static> {
    session
        .eval_code_with_output_capture(text)
        .0
        .unwrap()
        .into_owned()
        .unwrap()
}

fn collecting_builder(expected: &str, build: impl FnOnce(&Sexp<'_>, &Sexp<'_>) -> SEXP) {
    let mut session = RSession::new_for_gc_tests();
    let names = source(&mut session, "c('x','y')");
    let expression = source(&mut session, "quote(y+2*x)");
    let expected = source(&mut session, expected);
    assert!(session.define_var("expected_auxiliary_builder", expected));
    let collections = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = collections.clone();
    let outcome = session.with_active(|| {
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            observed.set(observed.get() + 1);
            crate::sexp::gengc::full_gc();
        }));
        unsafe {
            crate::mainutils::memory_main::R_gc_torture(1, 1, 0);
        }
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session
                .sexp(build(&names, &expression))
                .unwrap()
                .into_owned()
                .unwrap()
        }));
        unsafe {
            crate::mainutils::memory_main::R_gc_torture(0, 0, 0);
        }
        outcome
    });
    assert!(
        collections.get() > 0,
        "the builder must cross a real full collection"
    );
    let actual = outcome.expect("auxiliary construction must survive intermediate collections");
    assert!(session.define_var("actual_auxiliary_builder", actual));
    assert!(
        source(
            &mut session,
            "identical(actual_auxiliary_builder, expected_auxiliary_builder)"
        )
        .try_to_bool()
        .unwrap()
    );
}

#[test]
fn deriv_gradient_builder_survives_collection_between_all_allocations() {
    collecting_builder(
        "quote(.grad<-array(0,c(length(.value),2L),list(NULL,c('x','y'))))",
        |names, _| unsafe { super::create_grad(names.as_raw()) },
    );
}
#[test]
fn deriv_hessian_builder_survives_collection_between_all_allocations() {
    collecting_builder(
        "quote(.hessian<-array(0,c(length(.value),2L,2L),list(NULL,c('x','y'),c('x','y'))))",
        |names, _| unsafe { super::create_hess(names.as_raw()) },
    );
}
#[test]
fn deriv_gradient_assignment_survives_collection_between_all_allocations() {
    collecting_builder("quote(.grad[,'x']<-y+2*x)", |names, expression| unsafe {
        super::deriv_assign(
            names.try_string_elt(0).unwrap().as_raw(),
            expression.as_raw(),
        )
    });
}
#[test]
fn deriv_hessian_diagonal_survives_collection_between_all_allocations() {
    collecting_builder(
        "quote(.hessian[,'x','x']<-y+2*x)",
        |names, expression| unsafe {
            super::hess_assign1(
                names.try_string_elt(0).unwrap().as_raw(),
                expression.as_raw(),
            )
        },
    );
}
#[test]
fn deriv_hessian_reflection_survives_collection_between_all_allocations() {
    collecting_builder(
        "quote(.hessian[,'x','y']<-.hessian[,'y','x']<-y+2*x)",
        |names, expression| unsafe {
            super::hess_assign2(
                names.try_string_elt(0).unwrap().as_raw(),
                names.try_string_elt(1).unwrap().as_raw(),
                expression.as_raw(),
            )
        },
    );
}
#[test]
fn deriv_gradient_attachment_survives_collection_between_all_allocations() {
    collecting_builder("quote(attr(.value,'gradient')<-.grad)", |_, _| unsafe {
        super::add_grad()
    });
}
#[test]
fn deriv_hessian_attachment_survives_collection_between_all_allocations() {
    collecting_builder("quote(attr(.value,'hessian')<-.hessian)", |_, _| unsafe {
        super::add_hess()
    });
}
#[test]
fn deriv_closure_builder_survives_collection_between_all_allocations() {
    collecting_builder("function(x,y)y+2*x", |names, expression| unsafe {
        super::closure_with_formals(names.as_raw(), expression.as_raw())
    });
}

use crate::sexp::{
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    ffi::SEXPTYPE,
    heap::CheckedNode,
    object::{SexpMut, SexpResult},
};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

struct DetachingNames {
    sessions: Weak<RefCell<Option<RSession>>>,
    source: Rc<RefCell<Option<CheckedNode>>>,
    calls: Rc<Cell<usize>>,
    action: u8,
}
impl AltrepClass for DetachingNames {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::STRSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(2)
    }
    fn element<'s>(
        &self,
        context: &AltrepContext<'s>,
        index: i64,
    ) -> SexpResult<AltrepElement<'s>> {
        let previous = self.calls.replace(self.calls.get() + 1);
        assert_eq!(
            index, previous as i64,
            "read provider names once in GNU index order"
        );
        if previous == 0 {
            let parent = context.data1()?;
            let nil = parent.node_factory()?.domain().nil();
            let mut parent = SexpMut::try_from_checked(parent)?;
            parent.try_set_pairlist_car(&nil)?;
            context.gc()?;
            assert!(
                self.source.borrow().as_ref().unwrap().is_live(),
                "the selected names provider remains rooted after its source edge is removed"
            );
            if self.action == 1 {
                std::panic::panic_any(917_u32);
            }
            if self.action == 2 {
                self.sessions
                    .upgrade()
                    .unwrap()
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .close();
            }
        }
        Ok(AltrepElement::String(context.string(if index == 0 {
            "x"
        } else {
            "y"
        })?))
    }
}
fn detaching_names_case(action: u8) {
    let sessions = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let calls = Rc::new(Cell::new(0));
    let source_node = Rc::new(RefCell::new(None));
    let (owner, parent, raw, expected) = {
        let mut borrowed = sessions.borrow_mut();
        let session = borrowed.as_mut().unwrap();
        let expected = source(
            session,
            "quote(.grad<-array(0,c(length(.value),2L),list(NULL,c('x','y'))))",
        );
        session.with_active(|| {
            let class = session
                .register_altrep_class(
                    "detaching_deriv_names",
                    DetachingNames {
                        sessions: Rc::downgrade(&sessions),
                        source: source_node.clone(),
                        calls: calls.clone(),
                        action,
                    },
                )
                .unwrap();
            let names = AltrepBuilder::new(class).build().unwrap();
            *source_node.borrow_mut() = Some(names.allocation().unwrap().clone());
            let factory = names.node_factory().unwrap();
            let nil = factory.domain().nil();
            let parent = factory
                .pairlist_cell(&names, &nil, &nil)
                .unwrap()
                .into_owned()
                .unwrap();
            crate::sexp::altrep::set_data1(&names, parent.clone()).unwrap();
            (
                session.owner_token().unwrap().weak_owner().unwrap(),
                parent,
                names.as_raw(),
                expected,
            )
        })
    };
    let pin = owner.pin().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(pin.as_ptr(), || {
            let result = super::create_grad(raw);
            crate::sexp::owner::OwnerToken::current()
                .unwrap()
                .sexp(result)
                .unwrap()
                .into_owned()
                .unwrap()
        })
    }));
    assert!(calls.get() > 0);
    assert!(parent.try_car().unwrap().is_nil());
    assert_eq!(unsafe { (*pin.as_ptr()).memory_state.in_gc }, 0);
    if action == 0 {
        let actual = result.unwrap();
        let mut sessions = sessions.borrow_mut();
        let session = sessions.as_mut().unwrap();
        assert!(session.define_var("actual_detached_deriv_names", actual));
        assert!(session.define_var("expected_detached_deriv_names", expected));
        assert!(
            source(
                session,
                "identical(actual_detached_deriv_names,expected_detached_deriv_names)"
            )
            .try_to_bool()
            .unwrap()
        );
    } else if action == 1 {
        assert_eq!(*result.unwrap_err().downcast::<u32>().unwrap(), 917);
    } else {
        assert!(result.is_err());
        assert!(owner.node_factory().is_err());
    }
}
#[test]
fn deriv_auxiliary_names_survive_source_detachment_and_provider_collection() {
    detaching_names_case(0);
}
#[test]
fn deriv_auxiliary_names_preserve_provider_panic_and_cleanup() {
    detaching_names_case(1);
}
#[test]
fn deriv_auxiliary_names_reject_provider_owner_revocation() {
    detaching_names_case(2);
}

#[test]
fn deriv_closure_builder_denies_unadmitted_name_scratch_and_recovers() {
    let mut session = RSession::new_for_gc_tests();
    let names = source(&mut session, "c('x','y')");
    let body = source(&mut session, "quote(y+2*x)");
    let owner = session.owner_token().unwrap();
    let held = owner
        .with_arena(|a| a.reserve_workspace(128).unwrap())
        .unwrap();
    let (budget, before) = owner
        .with_arena(|a| {
            let budget = a.budget();
            let before = (a.total_bytes_allocated(), a.workspace_bytes());
            a.set_budget(crate::sexp::memory::ArenaBudget::new(
                before.0 + before.1,
                0,
            ));
            (budget, before)
        })
        .unwrap();
    let authority = owner.weak_owner().unwrap();
    let denied = crate::sexp::owner::with_runtime(&authority, |access| {
        super::builders::Builder::new(access).closure(&names, &body)
    })
    .unwrap();
    assert!(matches!(
        denied,
        Err(crate::sexp::object::SexpError::AllocationFailed {
            object: "derivative formal name"
        })
    ));
    assert_eq!(
        owner
            .with_arena(|a| (a.total_bytes_allocated(), a.workspace_bytes()))
            .unwrap(),
        before
    );
    owner.with_arena(|a| a.set_budget(budget)).unwrap();
    let actual = session.with_active(|| unsafe {
        session
            .sexp(super::closure_with_formals(names.as_raw(), body.as_raw()))
            .unwrap()
            .into_owned()
            .unwrap()
    });
    assert!(session.define_var("recovered_deriv_closure", actual));
    assert!(
        source(
            &mut session,
            "identical(recovered_deriv_closure,function(x,y)y+2*x)"
        )
        .try_to_bool()
        .unwrap()
    );
    drop(held);
}

#[test]
fn deriv_closure_builder_releases_name_scratch_before_reentrant_revocation() {
    let sessions = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let replacement = Rc::new(RefCell::new(None));
    let fired = Rc::new(Cell::new(false));
    let observed = Rc::new(Cell::new(0));
    let (owner, names, body, held, ledger) = {
        let mut borrowed = sessions.borrow_mut();
        let session = borrowed.as_mut().unwrap();
        let names = source(session, "c('x','y')");
        let body = source(session, "quote(y+2*x)");
        let token = session.owner_token().unwrap();
        let held = token
            .with_arena(|a| a.reserve_workspace(128).unwrap())
            .unwrap();
        let ledger = token
            .with_arena(|a| a.heap_identity().byte_ledger())
            .unwrap();
        let owner = token.weak_owner().unwrap();
        let original = sessions.clone();
        let replacement = replacement.clone();
        let observed = observed.clone();
        let once = fired.clone();
        let callback_owner = owner.clone();
        session.with_active(|| {
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if once.replace(true) {
                    return;
                }
                observed.set(
                    crate::sexp::owner::with_runtime(&callback_owner, |access| {
                        access.with_arena(|a| a.workspace_bytes())
                    })
                    .unwrap()
                    .unwrap(),
                );
                crate::sexp::gengc::full_gc();
                original.borrow_mut().take().unwrap().close();
                let other = RSession::new_for_gc_tests();
                let token = other.owner_token().unwrap();
                let root = token
                    .node_factory()
                    .character("replacement remains live")
                    .unwrap()
                    .into_owned()
                    .unwrap();
                let ticket = token
                    .with_arena(|a| a.reserve_workspace(256).unwrap())
                    .unwrap();
                let ledger = token
                    .with_arena(|a| a.heap_identity().byte_ledger())
                    .unwrap();
                *replacement.borrow_mut() = Some((other, root, ticket, ledger));
            }));
            unsafe {
                crate::mainutils::memory_main::R_gc_torture(1, 1, 0);
            }
        });
        (owner, names, body, held, ledger)
    };
    let pin = owner.pin().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(pin.as_ptr(), || {
            super::closure_with_formals(names.as_raw(), body.as_raw())
        })
    }));
    assert!(result.is_err());
    assert!(fired.get());
    assert_eq!(
        observed.get(),
        128,
        "native name storage is released before pairlist allocation can collect/revoke"
    );
    assert_eq!(
        ledger.workspace_bytes_for_test(),
        128,
        "the revoked original owner retains only its held ticket"
    );
    drop((names, body, held));
    assert_eq!(ledger.workspace_bytes_for_test(), 0);
    let replacement = replacement.borrow();
    let (session, root, _, ledger) = replacement.as_ref().unwrap();
    assert_eq!(ledger.workspace_bytes_for_test(), 256);
    assert_eq!(root.try_as_string().unwrap(), "replacement remains live");
    assert!(session.with_active(|| {
        session
            .owner_token()
            .unwrap()
            .node_factory()
            .character("still active")
            .is_ok()
    }));
}

#[test]
fn deriv_closure_builder_symbol_denial_releases_admitted_name_buffer() {
    let mut session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap();
    let formal = "fresh_deriv_formal_for_admitted_symbol_failure";
    let names = owner
        .node_factory()
        .strings(&[formal])
        .unwrap()
        .into_owned()
        .unwrap();
    let body = source(&mut session, "quote(1)");
    let owner = session.owner_token().unwrap();
    let held = owner
        .with_arena(|a| a.reserve_workspace(128).unwrap())
        .unwrap();
    let peak = formal.len() * 2 + 1;
    let budget = owner
        .with_arena(|a| {
            let old = a.budget();
            a.set_budget(crate::sexp::memory::ArenaBudget::new(
                a.total_bytes_allocated() + 128 + peak,
                0,
            ));
            old
        })
        .unwrap();
    let authority = owner.weak_owner().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::sexp::owner::with_runtime(&authority, |access| {
            super::builders::Builder::new(access).closure(&names, &body)
        })
    }));
    assert!(
        result.is_err(),
        "the byte buffer fits, but the previously uninterned symbol cannot fit"
    );
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 128);
    owner.with_arena(|a| a.set_budget(budget)).unwrap();
    let actual = crate::sexp::owner::with_runtime(&authority, |access| {
        super::builders::Builder::new(access).closure(&names, &body)
    })
    .unwrap()
    .unwrap();
    assert_eq!(
        actual
            .try_formals()
            .unwrap()
            .try_tag()
            .unwrap()
            .try_printname()
            .unwrap()
            .try_as_string()
            .unwrap(),
        formal
    );
    assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 128);
    drop(held);
}
