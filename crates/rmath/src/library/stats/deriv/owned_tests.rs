use crate::sexp::{RSession, SEXPTYPE, object::Sexp, owner::WeakOwner};

fn code(session: &mut RSession, source: &str) -> Sexp<'static> {
    session
        .eval_code_with_output_capture(source)
        .0
        .unwrap()
        .into_owned()
        .unwrap()
}

fn arguments(
    owner: &WeakOwner,
    expression: &Sexp<'static>,
    variable: &Sexp<'static>,
) -> Sexp<'static> {
    let factory = owner.node_factory().unwrap();
    let nil = factory.domain().nil();
    let tail = factory.pairlist_cell(variable, &nil, &nil).unwrap();
    let tail = factory.pairlist_cell(expression, &tail, &nil).unwrap();
    factory
        .pairlist_cell(&nil, &tail, &nil)
        .unwrap()
        .into_owned()
        .unwrap()
}

#[test]
#[cfg_attr(
    miri,
    ignore = "real-base execution covered natively; owning kernel callbacks run separately"
)]
fn owned_derivative_tangent_original_shape_and_execution() {
    let mut session = RSession::new_without_default_packages();
    let expression = code(&mut session, "quote(tan(x))");
    let variable = code(&mut session, "'x'");
    let owner = crate::sexp::owner::StoredOwner::from_token(session.owner_token().unwrap())
        .managed()
        .unwrap();
    let args = arguments(&owner, &expression, &variable);
    let derivative = session.with_active(|| unsafe {
        let result = super::do_d(args.as_raw());
        session.sexp(result).unwrap().into_owned().unwrap()
    });
    assert_eq!(derivative.typeof_(), SEXPTYPE::LANGSXP);
    assert!(session.define_var("saved_derivative", derivative));
    drop(args);
    drop(expression);
    drop(variable);
    session.gc();
    let result = code(
        &mut session,
        "x <- .3; identical(saved_derivative, quote(1/cos(x)^2)) && is.null(attributes(saved_derivative)) && abs(eval(saved_derivative) - 1/cos(x)^2) < 1e-14",
    );
    assert!(result.try_to_bool().unwrap());
}

#[test]
#[cfg_attr(
    miri,
    ignore = "real-base oracle execution is covered natively; focused owning kernel callbacks run separately"
)]
fn owned_derivative_pinned_trig_chain_rule_shapes_and_values() {
    let mut session = RSession::new_without_default_packages();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let variable = code(&mut session, "'x'");
    for row in include_str!("trig-oracle.tsv").lines() {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(fields.len(), 5);
        let expression = code(&mut session, &format!("quote({})", fields[0]));
        let args = arguments(&owner, &expression, &variable);
        let derivative = session.with_active(|| unsafe {
            let result = super::do_d(args.as_raw());
            session.sexp(result).unwrap().into_owned().unwrap()
        });
        assert!(session.define_var("saved_derivative", derivative));
        drop(args);
        drop(expression);
        session.gc();
        let test = format!(
            "x <- .3; y <- .2; paste(deparse(saved_derivative), collapse=\"\") == '{}' && typeof(saved_derivative) == '{}' && is.null(attributes(saved_derivative)) && abs(eval(saved_derivative) - ({})) < 2e-14",
            fields[1], fields[2], fields[4]
        );
        assert!(
            code(&mut session, &test).try_to_bool().unwrap(),
            "{}",
            fields[0]
        );
    }
    // Pinned GNU preserves attributes and OBJECT on reused exp subexpressions.
    let expression = code(
        &mut session,
        "e <- quote(exp(x)); attr(e, 'note') <- 7L; class(e) <- 'test'; e",
    );
    let args = arguments(&owner, &expression, &variable);
    let derivative = session.with_active(|| unsafe {
        let value = super::do_d(args.as_raw());
        session.sexp(value).unwrap().into_owned().unwrap()
    });
    assert!(session.define_var("saved_derivative", derivative));
    drop(args);
    drop(expression);
    code(&mut session, "rm(e)");
    session.gc();
    assert!(code(&mut session, "identical(attributes(saved_derivative), list(note=7L, class='test')) && is.object(saved_derivative)").try_to_bool().unwrap());
}

use crate::sexp::{
    altrep::{AltrepBuilder, AltrepClass, AltrepContext, AltrepElement},
    ffi::NodeBody,
    heap::CheckedNode,
    object::SexpResult,
};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

struct DetachingExponent {
    facade: Weak<RefCell<Option<RSession>>>,
    source: CheckedNode,
    calls: Rc<Cell<usize>>,
    action: u8,
}
impl AltrepClass for DetachingExponent {
    fn vector_type(&self) -> SEXPTYPE {
        SEXPTYPE::REALSXP
    }
    fn length(&self, _: &AltrepContext<'_>) -> SexpResult<i64> {
        Ok(1)
    }
    fn element<'s>(&self, context: &AltrepContext<'s>, _: i64) -> SexpResult<AltrepElement<'s>> {
        if self.calls.replace(self.calls.get() + 1) == 0 {
            let head = context.data1()?;
            let nil = head.node_factory()?.domain().nil();
            let node = head.allocation()?;
            let heap = node.heap_identity();
            let mut header = heap.node_snapshot(node).unwrap();
            let NodeBody::List(body) = &mut header.data else {
                panic!("actual external argument cell");
            };
            body.carval = nil.link_in(&heap)?;
            body.cdrval = nil.link_in(&heap)?;
            heap.replace_node(node, header).unwrap();
            context.gc()?;
            assert!(
                self.source.is_live(),
                "selected expression must have an actual owning root after detachment"
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
            if self.action != 0 {
                std::panic::panic_any(917_u32);
            }
        }
        Ok(AltrepElement::Real(2.))
    }
}

fn symbol(owner: &WeakOwner, text: &str) -> Sexp<'static> {
    let text = std::ffi::CString::new(text).unwrap();
    crate::sexp::owner::with_runtime(owner, |access| {
        access.with_native(|owner| unsafe {
            owner
                .sexp(crate::sexp::symbol::Rf_install(text.as_ptr()))?
                .into_owned()
        })
    })
    .unwrap()
    .unwrap()
}

fn call(owner: &WeakOwner, head: &str, values: &[Sexp<'static>]) -> Sexp<'static> {
    let head = symbol(owner, head);
    crate::sexp::owner::with_runtime(owner, |access| {
        let domain = access.domain();
        let allocator = access.allocator(&domain)?;
        let nil = domain.nil();
        let mut tail = nil.clone();
        for value in values.iter().rev() {
            tail = allocator.pairlist_cell(value, &tail, &nil)?;
        }
        allocator.call(&head, &tail)
    })
    .unwrap()
    .unwrap()
}

fn numeric(owner: &WeakOwner, number: f64) -> Sexp<'static> {
    owner
        .node_factory()
        .unwrap()
        .allocate(|arena| {
            let pointer = arena.alloc_vector(SEXPTYPE::REALSXP, 1);
            let node = arena.node_token(pointer)?;
            arena
                .heap_identity()
                .payload_lease(&node)?
                .set_real_elt(0, number)?;
            Some(pointer)
        })
        .unwrap()
        .into_owned()
        .unwrap()
}

fn collecting_derivative(action: u8) {
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let calls = Rc::new(Cell::new(0));
    let (owner, args, expression_identity, source_identity) = {
        let borrowed = facade.borrow();
        let session = borrowed.as_ref().unwrap();
        session.with_active(|| {
            let owner = session.owner_token().unwrap().weak_owner().unwrap();
            let x = symbol(&owner, "x");
            let placeholder = numeric(&owner, 2.);
            let power = call(&owner, "^", &[x, placeholder]);
            let expression = call(&owner, "tan", std::slice::from_ref(&power));
            let expression_identity = expression.allocation().unwrap().clone();
            let source_identity = power.allocation().unwrap().clone();
            let class = session
                .register_altrep_class(
                    "D.detaching.exponent",
                    DetachingExponent {
                        facade: Rc::downgrade(&facade),
                        source: expression_identity.clone(),
                        calls: calls.clone(),
                        action,
                    },
                )
                .unwrap();
            let exponent = AltrepBuilder::new(class).build().unwrap();
            let mut cell = crate::sexp::object::SexpMut::try_from_checked(
                power.try_cdr().unwrap().try_cdr().unwrap(),
            )
            .unwrap();
            cell.try_set_pairlist_car(&exponent).unwrap();
            let variable = owner
                .node_factory()
                .unwrap()
                .strings(&["x"])
                .unwrap()
                .into_owned()
                .unwrap();
            let args = arguments(&owner, &expression, &variable);
            crate::sexp::altrep::set_data1(&exponent, args.clone()).unwrap();
            // No expression/power/variable owners escape this block. Only the
            // actual external args graph retains inputs when the call starts.
            (owner, args, expression_identity, source_identity)
        })
    };
    let pin = owner.pin().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(pin.as_ptr(), || {
            let result = super::do_d(args.as_raw());
            crate::sexp::owner::OwnerToken::current()
                .unwrap()
                .sexp(result)
                .unwrap()
                .into_owned()
                .unwrap()
        })
    }));
    assert_eq!(unsafe { (*pin.as_ptr()).memory_state.in_gc }, 0);
    assert!(calls.get() > 0);
    match action {
        0 => {
            let result = result.unwrap();
            assert!(args.try_cdr().unwrap().is_nil());
            drop(args);
            crate::sexp::owner::with_runtime(&owner, |access| {
                access.with_native(|owner| owner.full_gc())
            })
            .unwrap()
            .unwrap();
            assert_eq!(result.typeof_(), SEXPTYPE::LANGSXP);
            assert!(
                result
                    .try_car()
                    .unwrap()
                    .try_printname()
                    .unwrap()
                    .try_char_eq(b"/")
                    .unwrap()
            );
            assert!(
                !expression_identity.is_live(),
                "D output must not keep incidental original tan head alive"
            );
            assert!(
                !source_identity.is_live(),
                "parenthesis pass rebuilds calls without incidental source roots"
            );
            let generated = result
                .try_cdr()
                .unwrap()
                .try_cdr()
                .unwrap()
                .try_car()
                .unwrap()
                .into_owned()
                .unwrap();
            let generated_identity = generated.allocation().unwrap().clone();
            assert!(generated_identity.is_live());
            drop(generated);
            drop(result);
            crate::sexp::owner::with_runtime(&owner, |access| {
                access.with_native(|owner| owner.full_gc())
            })
            .unwrap()
            .unwrap();
            assert!(!generated_identity.is_live());
        }
        1 => {
            assert_eq!(*result.unwrap_err().downcast::<u32>().unwrap(), 917);
            let x = symbol(&owner, "x");
            let expression = call(&owner, "tan", &[x]);
            let variable = owner
                .node_factory()
                .unwrap()
                .strings(&["x"])
                .unwrap()
                .into_owned()
                .unwrap();
            let recovery = arguments(&owner, &expression, &variable);
            unsafe {
                crate::sexp::session::with_instance_active(pin.as_ptr(), || {
                    super::do_d(recovery.as_raw());
                });
            }
        }
        2 => {
            let error = result
                .unwrap_err()
                .downcast::<crate::sexp::context::RError>()
                .unwrap();
            assert!(
                error.message == crate::sexp::object::SexpError::RootUnavailable.to_string(),
                "{}",
                error.message
            );
            assert!(owner.pin().is_err());
        }
        _ => unreachable!(),
    }
}

#[test]
fn owned_derivative_gc_detached_inputs_and_generated_graph() {
    collecting_derivative(0);
}
#[test]
fn owned_derivative_gc_live_provider_panic_identity_and_recovery() {
    collecting_derivative(1);
}
#[test]
fn owned_derivative_gc_original_revocation_rejects_success() {
    collecting_derivative(2);
}

#[test]
fn owned_derivative_gc_malformed_inputs_rejected_and_recovery() {
    let session = RSession::new_for_gc_tests();
    session.with_active(|| {
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        let factory = owner.node_factory().unwrap();
        let empty = factory.strings(&[]).unwrap().into_owned().unwrap();
        let input = symbol(&owner, "x");
        let args = arguments(&owner, &input, &empty);
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::do_d(args.as_raw())
        }))
        .unwrap_err();
        assert!(
            error
                .downcast::<crate::sexp::context::RError>()
                .unwrap()
                .message
                .contains("variable must be a character string")
        );
        let cyclic = call(&owner, "tan", std::slice::from_ref(&input));
        let cell = cyclic.try_cdr().unwrap().into_owned().unwrap();
        let mut writable = crate::sexp::object::SexpMut::try_from_checked(cell.clone()).unwrap();
        writable.try_set_pairlist_cdr(&cell).unwrap();
        drop(writable);
        let variable = factory.strings(&["x"]).unwrap().into_owned().unwrap();
        let cyclic_args = arguments(&owner, &cyclic, &variable);
        let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::do_d(cyclic_args.as_raw())
        }))
        .unwrap_err();
        assert!(
            error
                .downcast::<crate::sexp::context::RError>()
                .unwrap()
                .message
                .contains("cyclic derivative argument list")
        );
        let variable = factory.strings(&["x"]).unwrap().into_owned().unwrap();
        let args = arguments(&owner, &input, &variable);
        let result = unsafe { super::do_d(args.as_raw()) };
        assert_eq!(session.sexp(result).unwrap().try_real_elt(0).unwrap(), 1.);
    });
}

#[test]
#[cfg_attr(
    miri,
    ignore = "real-base psigamma numerical oracle is covered natively"
)]
fn owned_derivative_publication_psigamma_default_and_explicit_order_oracle() {
    let mut session = RSession::new_without_default_packages();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let variable = code(&mut session, "'x'");
    for row in include_str!("psigamma-oracle.tsv").lines() {
        let fields: Vec<_> = row.split('\t').collect();
        let expression = code(&mut session, &format!("quote({})", fields[0]));
        let args = arguments(&owner, &expression, &variable);
        let derivative = session.with_active(|| unsafe {
            let result = super::do_d(args.as_raw());
            session.sexp(result).unwrap().into_owned().unwrap()
        });
        assert!(session.define_var("saved_derivative", derivative));
        drop(args);
        drop(expression);
        session.gc();
        let test = format!(
            "x <- 1.3; y <- 2; paste(deparse(saved_derivative), collapse=\"\") == '{}' && typeof(saved_derivative) == '{}' && is.null(attributes(saved_derivative)) && abs(eval(saved_derivative) - ({})) < 2e-14",
            fields[1], fields[2], fields[4]
        );
        assert!(
            code(&mut session, &test).try_to_bool().unwrap(),
            "{}",
            fields[0]
        );
    }
}

#[test]
#[cfg_attr(
    miri,
    ignore = "public stats namespace and condition-call attribution exercised natively"
)]
fn owned_derivative_publication_public_sequential_error_calls_and_recovery() {
    let mut session = RSession::new_without_default_packages();
    let result = code(
        &mut session,
        r#"
        D <- stats::D
        check <- function(e) {
            error <- tryCatch(D(e, "x"), error=function(e) e)
            is.call(conditionCall(error)) && identical(conditionCall(error), quote(D(e, "x")))
        }
        a <- check(quote(unknown(x)))
        b <- check(quote(log(x, 2)))
        c <- check(quote(log(x, base=2)))
        a && b && c && identical(D(quote(x), "x"), 1)
    "#,
    );
    assert!(result.try_to_bool().unwrap());
}
