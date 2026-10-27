use crate::sexp::{RSession, ffi::SEXPTYPE, object::Sexp};
use std::{cell::Cell, rc::Rc};
fn source(session: &mut RSession, text: &str) -> Sexp<'static> {
    session
        .eval_code_with_output_capture(text)
        .0
        .unwrap()
        .into_owned()
        .unwrap()
}
// Retain only the actual external argument list before entering the builder.
#[test]
fn deriv_outer_graph_survives_real_collection_before_auxiliary_builders() {
    for collect in [false, true] {
        let mut session = RSession::new_for_gc_tests();
        let arguments = {
            let expression = source(&mut session, "quote(x*y+x^2)");
            let names = source(&mut session, "c('x','y')");
            let funarg = source(&mut session, "FALSE");
            let tag = source(&mut session, "'.expr'");
            let hessian = source(&mut session, "TRUE");
            let owner = session.owner_token().unwrap();
            let factory = owner.node_factory();
            let nil = factory.domain().nil();
            let mut tail = nil.clone();
            for value in [nil.clone(), expression, names, funarg, tag, hessian]
                .iter()
                .rev()
            {
                tail = factory
                    .pairlist_cell(value, &tail, &nil)
                    .unwrap()
                    .into_owned()
                    .unwrap();
            }
            tail
        };
        let count = Rc::new(Cell::new(0));
        let observed = count.clone();
        let result = session.with_active(|| {
            if collect {
                crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                    observed.set(observed.get() + 1);
                    crate::sexp::gengc::full_gc();
                }));
                unsafe {
                    crate::mainutils::memory_main::R_gc_torture(1, 1, 0);
                }
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                session
                    .sexp(super::do_deriv(arguments.as_raw()))
                    .unwrap()
                    .into_owned()
                    .unwrap()
            }));
            unsafe {
                crate::mainutils::memory_main::R_gc_torture(0, 0, 0);
            }
            result
        });
        if collect {
            assert!(count.get() > 0);
        }
        let result = result
            .expect("outer derivative graph must retain all intermediates across real collection");
        assert_eq!(result.typeof_(), SEXPTYPE::EXPRSXP);
        session.with_active(|| crate::sexp::gengc::full_gc());
        assert!(session.define_var("collected_deriv_program", result));
        assert!(source(&mut session,"local({x<-2;y<-3;v<-eval(collected_deriv_program);identical(as.numeric(v),10)&&identical(as.numeric(attr(v,'gradient')),c(7,2))&&identical(as.numeric(attr(v,'hessian')),c(2,1,1,0))})").try_to_bool().unwrap());
    }
}

use crate::sexp::{
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    heap::CheckedNode,
    object::{SexpMut, SexpResult},
};
use std::{cell::RefCell, rc::Weak};
struct OuterNames {
    sessions: Weak<RefCell<Option<RSession>>>,
    retained: Rc<RefCell<Vec<CheckedNode>>>,
    parent: Rc<RefCell<Option<CheckedNode>>>,
    calls: Rc<Cell<usize>>,
    trace: Rc<Cell<u64>>,
    action: u8,
    phase: u8,
    prebuilt: bool,
    replacement: Rc<RefCell<Option<(Sexp<'static>, crate::sexp::memory::WorkspaceReservation)>>>,
}
impl AltrepClass for OuterNames {
    fn vector_type(&self) -> SEXPTYPE {
        if self.phase == 3 {
            SEXPTYPE::REALSXP
        } else if self.phase == 2 {
            SEXPTYPE::LGLSXP
        } else {
            SEXPTYPE::STRSXP
        }
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(if self.phase == 0 && !self.prebuilt {
            2
        } else {
            1
        })
    }
    fn element<'s>(
        &self,
        context: &AltrepContext<'s>,
        index: i64,
    ) -> SexpResult<AltrepElement<'s>> {
        let value = if self.phase == 3 {
            AltrepElement::Real(2.0)
        } else if self.phase == 2 {
            AltrepElement::Logical(1)
        } else {
            AltrepElement::String(context.string(if self.phase == 1 {
                ".expr"
            } else if index == 0 {
                "x"
            } else {
                "y"
            })?)
        };
        self.trace.set(
            self.trace
                .get()
                .wrapping_mul(5)
                .wrapping_add(index as u64 + 1),
        );
        if self.calls.replace(self.calls.get() + 1) == 0 && self.action != 3 {
            let root = self.parent.borrow().as_ref().unwrap().root_lease().unwrap();
            let parent = context
                .object()
                .node_factory()?
                .domain()
                .rooted_value(&root)?;
            let nil = parent.node_factory()?.domain().nil();
            let mut parent = SexpMut::try_from_checked(parent)?;
            parent.try_set_pairlist_cdr(&nil)?;
            context.gc()?;
            assert!(
                self.retained.borrow().iter().all(CheckedNode::is_live),
                "selected derivative inputs survive argument-list detachment"
            );
            if self.action == 4 {
                let inner = if self.prebuilt {
                    // Enter the same native primitive recursively while the
                    // outer provider is active. Data2 traces only independent
                    // inner arguments; it cannot root detached outer operands.
                    let args = context.data2()?;
                    let raw = unsafe { super::do_deriv(args.as_raw()) };
                    context.wrap(raw)?.into_owned()?
                } else {
                    let sessions = self.sessions.upgrade().unwrap();
                    let mut sessions = sessions.borrow_mut();
                    source(sessions.as_mut().unwrap(), "deriv(quote(x^2),'x')")
                };
                assert_eq!(inner.typeof_(), SEXPTYPE::EXPRSXP);
            }
            if self.action == 1 {
                std::panic::panic_any(917_u32);
            }
            if self.action == 2 {
                let sessions = self.sessions.upgrade().unwrap();
                let mut slot = sessions.borrow_mut();
                slot.as_mut().unwrap().close();
                let mut replacement = RSession::new_for_gc_tests();
                let root = if self.prebuilt {
                    prebuilt_number(&replacement, 42.)
                } else {
                    source(&mut replacement, "42")
                };
                let ticket = replacement
                    .owner_token()
                    .unwrap()
                    .with_arena(|a| a.reserve_workspace(128).unwrap())
                    .unwrap();
                *self.replacement.borrow_mut() = Some((root, ticket));
                *slot = Some(replacement);
            }
        }
        Ok(value)
    }
}
fn outer_names_case_impl(action: u8, phase: u8, prebuilt: bool) {
    #[cfg(miri)]
    eprintln!("outer provider action={action}; phase={phase}; prebuilt={prebuilt}");
    let sessions = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let calls = Rc::new(Cell::new(0));
    let trace = Rc::new(Cell::new(0));
    let retained = Rc::new(RefCell::new(Vec::new()));
    let parent = Rc::new(RefCell::new(None));
    let replacement = Rc::new(RefCell::new(None));
    let (authority, arguments, held, ledger) = {
        let mut borrowed = sessions.borrow_mut();
        let session = borrowed.as_mut().unwrap();
        let (expression, funarg, ordinary_names, tag, hessian, inner) = if prebuilt {
            prebuilt_inputs(session)
        } else {
            let expression = source(session, "quote(x^2+y)");
            let funarg = source(session, "function(x=2,y=3)NULL");
            let ordinary_names = source(session, "c('x','y')");
            let tag = source(session, "'.expr'");
            let hessian = source(session, "TRUE");
            let nil = session.owner_token().unwrap().node_factory().domain().nil();
            (expression, funarg, ordinary_names, tag, hessian, nil)
        };
        session.with_active(|| {
            let class = session
                .register_altrep_class(
                    "outer_deriv_names",
                    OuterNames {
                        sessions: Rc::downgrade(&sessions),
                        retained: retained.clone(),
                        parent: parent.clone(),
                        calls: calls.clone(),
                        trace: trace.clone(),
                        action,
                        phase,
                        prebuilt,
                        replacement: replacement.clone(),
                    },
                )
                .unwrap();
            let provider = AltrepBuilder::new(class)
                .build()
                .unwrap()
                .into_owned()
                .unwrap();
            let names = if phase == 0 {
                provider.clone()
            } else {
                ordinary_names
            };
            let tag = if phase == 1 { provider.clone() } else { tag };
            let hessian = if phase == 2 {
                provider.clone()
            } else {
                hessian
            };
            if phase == 3 {
                let power = if prebuilt {
                    expression.clone()
                } else {
                    expression
                        .try_cdr()
                        .unwrap()
                        .try_car()
                        .unwrap()
                        .into_owned()
                        .unwrap()
                };
                let exponent_cell = power
                    .try_cdr()
                    .unwrap()
                    .try_cdr()
                    .unwrap()
                    .into_owned()
                    .unwrap();
                SexpMut::try_from_checked(exponent_cell)
                    .unwrap()
                    .try_set_pairlist_car(&provider)
                    .unwrap();
            }
            *retained.borrow_mut() = [&expression, &funarg, &names]
                .iter()
                .map(|v| v.allocation().unwrap().clone())
                .collect();
            let factory = names.node_factory().unwrap();
            let nil = factory.domain().nil();
            let mut arguments = nil.clone();
            for value in [nil.clone(), expression, names.clone(), funarg, tag, hessian]
                .iter()
                .rev()
            {
                arguments = factory
                    .pairlist_cell(value, &arguments, &nil)
                    .unwrap()
                    .into_owned()
                    .unwrap();
            }
            if prebuilt {
                crate::sexp::altrep::set_data2(&provider, inner.clone()).unwrap();
            }
            *parent.borrow_mut() = Some(arguments.allocation().unwrap().clone());
            let token = session.owner_token().unwrap();
            let held = token
                .with_arena(|a| a.reserve_workspace(128).unwrap())
                .unwrap();
            let ledger = token
                .with_arena(|a| a.heap_identity().byte_ledger())
                .unwrap();
            (token.weak_owner().unwrap(), arguments, held, ledger)
        })
    };
    let pin = authority.pin().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(pin.as_ptr(), || {
            let raw = super::do_deriv(arguments.as_raw());
            crate::sexp::owner::OwnerToken::current()
                .unwrap()
                .sexp(raw)
                .unwrap()
                .into_owned()
                .unwrap()
        })
    }));
    assert!(calls.get() > 0);
    assert_eq!(
        ledger.workspace_bytes_for_test(),
        128,
        "all derivative scratch releases into its original ledger"
    );
    drop(held);
    assert_eq!(ledger.workspace_bytes_for_test(), 0);
    assert_eq!(arguments.try_cdr().unwrap().is_nil(), action != 3);
    assert_eq!(unsafe { (*pin.as_ptr()).memory_state.in_gc }, 0);
    if action == 3 && !prebuilt {
        assert_eq!(
            (calls.get(), trace.get()),
            [(20, 24788207600787), (1, 1), (1, 1), (8, 97656)][phase as usize],
            "exact provider trace captured on d2f98a48"
        );
    }
    match action {
        0 | 3 | 4 => {
            let actual = result.expect("detached inputs stay owned");
            assert_eq!(actual.typeof_(), SEXPTYPE::CLOSXP);
            if prebuilt {
                assert_eq!(actual.try_body().unwrap().typeof_(), SEXPTYPE::LANGSXP);
                let first = actual.try_formals().unwrap();
                assert!(!first.is_nil());
                assert!(first.try_cdr().unwrap().is_nil());
                assert_eq!(first.try_car().unwrap().try_real_elt(0).unwrap(), 2.);
            } else {
                let mut borrowed = sessions.borrow_mut();
                let session = borrowed.as_mut().unwrap();
                assert!(session.define_var("outer_deriv_actual", actual));
                assert!(
                    source(session, "identical(as.numeric(outer_deriv_actual()),7)")
                        .try_to_bool()
                        .unwrap()
                );
            }
        }
        1 => assert_eq!(*result.unwrap_err().downcast::<u32>().unwrap(), 917),
        _ => {
            assert!(result.is_err());
            assert!(authority.node_factory().is_err());
            let kept = replacement.borrow();
            let (root, ticket) = kept.as_ref().unwrap();
            assert_eq!(root.try_real_elt(0).unwrap(), 42.0);
            assert_eq!(ticket.bytes(), 128);
            let sessions = sessions.borrow();
            let session = sessions.as_ref().unwrap();
            assert_eq!(
                session.with_active(|| session
                    .owner_token()
                    .unwrap()
                    .with_arena(|a| a.workspace_bytes())
                    .unwrap()),
                128
            );
        }
    }
}
#[test]
fn derivative_outer_names_survive_detachment_and_collection() {
    for phase in 0..4 {
        outer_names_case(0, phase);
    }
}
#[test]
fn derivative_outer_names_preserve_provider_unwind() {
    for phase in 0..4 {
        outer_names_case(1, phase);
    }
}
#[test]
fn derivative_outer_names_reject_owner_revocation() {
    for phase in 0..4 {
        outer_names_case(2, phase);
    }
}

#[test]
fn derivative_outer_provider_order_matches_legacy() {
    for phase in 0..4 {
        outer_names_case(3, phase);
    }
}

#[test]
fn derivative_outer_inputs_allow_nested_deriv_reentry() {
    for phase in 0..4 {
        outer_names_case(4, phase);
    }
}

// Public parser/evaluator cases always retain their original full workflow.
// The separately labeled representative controls below avoid repeated public
// parser/bootstrap work while executing actual native/provider boundaries.
fn outer_names_case(action: u8, phase: u8) {
    outer_names_case_impl(action, phase, false);
}

fn prebuilt_number(session: &RSession, number: f64) -> Sexp<'static> {
    session.with_active(|| {
        let factory = session.owner_token().unwrap().node_factory();
        let value = factory
            .allocate(|a| Some(a.alloc_vector(SEXPTYPE::REALSXP, 1)))
            .unwrap()
            .into_owned()
            .unwrap();
        let mut value = SexpMut::try_from_checked(value).unwrap();
        value.try_set_real_elt(0, number).unwrap();
        value.freeze()
    })
}
fn prebuilt_inputs(
    session: &RSession,
) -> (
    Sexp<'static>,
    Sexp<'static>,
    Sexp<'static>,
    Sexp<'static>,
    Sexp<'static>,
    Sexp<'static>,
) {
    session.with_active(|| {
        let owner = session.owner_token().unwrap();
        let factory = owner.node_factory();
        let domain = factory.domain();
        let nil = domain.nil().into_owned().unwrap();
        let symbol = |name: &std::ffi::CStr| unsafe {
            owner
                .sexp(crate::sexp::symbol::Rf_install(name.as_ptr()))
                .unwrap()
                .into_owned()
                .unwrap()
        };
        let list = |values: &[Sexp<'static>]| {
            let mut tail = nil.clone();
            for value in values.iter().rev() {
                tail = factory
                    .pairlist_cell(value, &tail, &nil)
                    .unwrap()
                    .into_owned()
                    .unwrap();
            }
            tail
        };
        let x = symbol(c"x");
        let two = prebuilt_number(session, 2.);
        let expression = factory
            .call(&symbol(c"^"), &list(&[x.clone(), two.clone()]))
            .unwrap()
            .into_owned()
            .unwrap();
        let formals = factory
            .pairlist_cell(&two, &nil, &x)
            .unwrap()
            .into_owned()
            .unwrap();
        let authority = owner.weak_owner().unwrap();
        let function = crate::sexp::owner::with_runtime(&authority, |access| {
            let environment = super::builder_environment(access)?;
            super::builders::Builder::new(access).closure_with_parts(&formals, &nil, &environment)
        })
        .unwrap()
        .unwrap();
        let inner_names = factory.strings(&["x"]).unwrap().into_owned().unwrap();
        let inner_tag = factory.strings(&[".inner"]).unwrap().into_owned().unwrap();
        let no = domain.logical(false).into_owned().unwrap();
        let inner = list(&[nil.clone(), x, inner_names, no.clone(), inner_tag, no]);
        (
            expression,
            function,
            factory.strings(&["x"]).unwrap().into_owned().unwrap(),
            factory.strings(&[".expr"]).unwrap().into_owned().unwrap(),
            domain.logical(false).into_owned().unwrap(),
            inner,
        )
    })
}
// Explicitly labeled representative controls: full public x^2+y/two-variable
// cases above remain unchanged and are exercised by the native suite.
#[test]
fn derivative_outer_representative_collection() {
    for phase in 0..4 {
        outer_names_case_impl(0, phase, true);
    }
}
#[test]
fn derivative_outer_representative_provider_unwind() {
    for phase in 0..4 {
        outer_names_case_impl(1, phase, true);
    }
}
#[test]
fn derivative_outer_representative_owner_replacement() {
    for phase in 0..4 {
        outer_names_case_impl(2, phase, true);
    }
}
#[test]
fn derivative_outer_representative_native_reentry() {
    for phase in 0..4 {
        outer_names_case_impl(4, phase, true);
    }
}
