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

fn collecting_derivative(action: u8, common_unary: bool, binary_frame: u8) {
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let calls = Rc::new(Cell::new(0));
    let (owner, args, expression_identity, source_identity) = {
        let borrowed = facade.borrow();
        let session = borrowed.as_ref().unwrap();
        session.with_active(|| {
            let owner = session.owner_token().unwrap().weak_owner().unwrap();
            let x = symbol(&owner, "x");
            let placeholder = numeric(&owner, 2.);
            let power = call(&owner, "^", &[x.clone(), placeholder]);
            let unary = match binary_frame {
                4 => "sinh",
                5 => "cosh",
                6 => "tanh",
                7 => "asin",
                8 => "acos",
                9 => "atan",
                10 => "expm1",
                11 => "log1p",
                12 => "log2",
                13 => "log10",
                14 => "pnorm",
                15 => "dnorm",
                16 => "sinpi",
                17 => "cospi",
                18 => "tanpi",
                _ => "tan",
            };
            let tangent = if common_unary {
                let mut child = power.clone();
                for head in ["log", "exp", "sin", "cos"] {
                    child = call(&owner, head, &[child]);
                }
                call(&owner, "tan", &[child])
            } else {
                call(&owner, unary, std::slice::from_ref(&power))
            };
            let expression = if binary_frame == 1 {
                let left = call(&owner, "*", &[symbol(&owner, "y"), x]);
                call(&owner, "*", &[left, tangent])
            } else if binary_frame == 2 {
                let left = call(&owner, "*", &[symbol(&owner, "y"), x]);
                call(&owner, "/", &[left, tangent])
            } else if binary_frame == 3 {
                call(&owner, "^", &[x.clone(), power.clone()])
            } else {
                call(&owner, "+", &[symbol(&owner, "y"), tangent])
            };
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
            let mut result = result.unwrap();
            if common_unary {
                assert!(
                    result
                        .try_car()
                        .unwrap()
                        .try_printname()
                        .unwrap()
                        .try_char_eq(b"-")
                        .unwrap()
                );
                result = result
                    .try_cdr()
                    .unwrap()
                    .try_car()
                    .unwrap()
                    .into_owned()
                    .unwrap();
                // The parenthesis pass encloses the negated quotient.
                assert!(
                    result
                        .try_car()
                        .unwrap()
                        .try_printname()
                        .unwrap()
                        .try_char_eq(b"(")
                        .unwrap()
                );
                result = result
                    .try_cdr()
                    .unwrap()
                    .try_car()
                    .unwrap()
                    .into_owned()
                    .unwrap();
            }
            assert!(args.try_cdr().unwrap().is_nil());
            drop(args);
            crate::sexp::owner::with_runtime(&owner, |access| {
                access.with_native(|owner| owner.full_gc())
            })
            .unwrap()
            .unwrap();
            assert_eq!(result.typeof_(), SEXPTYPE::LANGSXP);
            let expected_head = match binary_frame {
                1 => b"+",
                2 => b"-",
                3 => b"+",
                4 | 5 | 10 | 14 | 16 => b"*",
                6 => b"/",
                7 | 9 | 11 | 12 | 13 | 18 => b"/",
                8 | 15 | 17 => b"-",
                _ => b"/",
            };
            assert!(
                result
                    .try_car()
                    .unwrap()
                    .try_printname()
                    .unwrap()
                    .try_char_eq(expected_head)
                    .unwrap()
            );
            assert!(
                !expression_identity.is_live(),
                "D output must not keep incidental original tan head alive"
            );
            if binary_frame == 0 {
                assert!(
                    !source_identity.is_live(),
                    "parenthesis pass rebuilds calls without incidental source roots"
                );
            }
            let derivative = if matches!(binary_frame, 8 | 15 | 17) {
                let mut derivative = result
                    .try_cdr()
                    .unwrap()
                    .try_car()
                    .unwrap()
                    .into_owned()
                    .unwrap();
                if derivative
                    .try_car()
                    .unwrap()
                    .try_printname()
                    .unwrap()
                    .try_char_eq(b"(")
                    .unwrap()
                {
                    derivative = derivative
                        .try_cdr()
                        .unwrap()
                        .try_car()
                        .unwrap()
                        .into_owned()
                        .unwrap();
                }
                derivative
            } else {
                result.clone()
            };
            let generated = derivative
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
            drop(derivative);
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
    collecting_derivative(0, false, 0);
}
#[test]
fn owned_derivative_gc_live_provider_panic_identity_and_recovery() {
    collecting_derivative(1, false, 0);
}
#[test]
fn owned_derivative_gc_original_revocation_rejects_success() {
    collecting_derivative(2, false, 0);
}

#[test]
fn owned_derivative_product_frame_gc_retains_first_term() {
    collecting_derivative(0, false, 1);
}

#[test]
fn owned_derivative_product_frame_panic_recovers() {
    collecting_derivative(1, false, 1);
}

#[test]
fn owned_derivative_product_frame_revocation_rejects_success() {
    collecting_derivative(2, false, 1);
}

#[test]
fn owned_derivative_quotient_frame_gc_retains_first_term() {
    collecting_derivative(0, false, 2);
}

#[test]
fn owned_derivative_quotient_frame_panic_recovers() {
    collecting_derivative(1, false, 2);
}

#[test]
fn owned_derivative_quotient_frame_revocation_rejects_success() {
    collecting_derivative(2, false, 2);
}

#[test]
fn owned_derivative_power_frame_gc_retains_first_term() {
    collecting_derivative(0, false, 3);
}

#[test]
fn owned_derivative_power_frame_panic_recovers() {
    collecting_derivative(1, false, 3);
}

#[test]
fn owned_derivative_power_frame_revocation_rejects_success() {
    collecting_derivative(2, false, 3);
}

#[test]
fn owned_derivative_sinh_frame_gc_retains_factor() {
    collecting_derivative(0, false, 4);
}

#[test]
fn owned_derivative_sinh_frame_panic_recovers() {
    collecting_derivative(1, false, 4);
}

#[test]
fn owned_derivative_sinh_frame_revocation_rejects_success() {
    collecting_derivative(2, false, 4);
}

#[test]
fn owned_derivative_cosh_frame_gc_retains_factor() {
    collecting_derivative(0, false, 5);
}

#[test]
fn owned_derivative_cosh_frame_panic_recovers() {
    collecting_derivative(1, false, 5);
}

#[test]
fn owned_derivative_cosh_frame_revocation_rejects_success() {
    collecting_derivative(2, false, 5);
}

#[test]
fn owned_derivative_tanh_frame_gc_retains_denominator() {
    collecting_derivative(0, false, 6);
}

#[test]
fn owned_derivative_tanh_frame_panic_recovers() {
    collecting_derivative(1, false, 6);
}

#[test]
fn owned_derivative_tanh_frame_revocation_rejects_success() {
    collecting_derivative(2, false, 6);
}

#[test]
fn owned_derivative_asin_frame_gc_retains_root() {
    collecting_derivative(0, false, 7);
}

#[test]
fn owned_derivative_asin_frame_panic_recovers() {
    collecting_derivative(1, false, 7);
}

#[test]
fn owned_derivative_asin_frame_revocation_rejects_success() {
    collecting_derivative(2, false, 7);
}

#[test]
fn owned_derivative_acos_frame_gc_retains_root() {
    collecting_derivative(0, false, 8);
}

#[test]
fn owned_derivative_acos_frame_panic_recovers() {
    collecting_derivative(1, false, 8);
}

#[test]
fn owned_derivative_acos_frame_revocation_rejects_success() {
    collecting_derivative(2, false, 8);
}

#[test]
fn owned_derivative_atan_frame_gc_retains_denominator() {
    collecting_derivative(0, false, 9);
}

#[test]
fn owned_derivative_atan_frame_panic_recovers() {
    collecting_derivative(1, false, 9);
}

#[test]
fn owned_derivative_atan_frame_revocation_rejects_success() {
    collecting_derivative(2, false, 9);
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
        for (operator, binary) in [
            ("+", false),
            ("-", false),
            ("(", false),
            ("+", true),
            ("-", true),
        ] {
            let values = if binary {
                vec![input.clone(), input.clone()]
            } else {
                vec![input.clone()]
            };
            let cyclic_unary = call(&owner, operator, &values);
            let cell = cyclic_unary.try_cdr().unwrap().into_owned().unwrap();
            let cell = if binary {
                cell.try_cdr().unwrap().into_owned().unwrap()
            } else {
                cell
            };
            let mut writable = crate::sexp::object::SexpMut::try_from_checked(cell).unwrap();
            writable.try_set_pairlist_car(&cyclic_unary).unwrap();
            drop(writable);
            let variable = factory.strings(&["x"]).unwrap().into_owned().unwrap();
            let cyclic_args = arguments(&owner, &cyclic_unary, &variable);
            let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                super::do_d(cyclic_args.as_raw())
            }))
            .unwrap_err();
            assert!(
                error
                    .downcast::<crate::sexp::context::RError>()
                    .unwrap()
                    .message
                    .contains("cyclic derivative expression")
            );
        }
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

#[test]
#[cfg_attr(miri, ignore = "real-base graph execution is covered natively")]
fn owned_derivative_symbolic_power_survives_source_collection() {
    let mut session = RSession::new_without_default_packages();
    let expression = code(&mut session, "quote((x+1)^(x*y))");
    let variable = code(&mut session, "'x'");
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let args = arguments(&owner, &expression, &variable);
    let derivative = session.with_active(|| unsafe {
        session
            .sexp(super::do_d(args.as_raw()))
            .unwrap()
            .into_owned()
            .unwrap()
    });
    assert!(session.define_var("saved_derivative", derivative));
    drop(args);
    drop(expression);
    drop(variable);
    session.gc();
    assert!(
        code(
            &mut session,
            "x <- 2; y <- 3; abs(eval(saved_derivative) - (3^5*6 + 3^6*log(3)*3)) < 1e-10"
        )
        .try_to_bool()
        .unwrap()
    );
}

#[test]
fn owned_psigamma_default_graph_survives_source_collection() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let expression = call(&owner, "psigamma", &[symbol(&owner, "x")]);
    let variable = owner
        .node_factory()
        .unwrap()
        .strings(&["x"])
        .unwrap()
        .into_owned()
        .unwrap();
    let args = arguments(&owner, &expression, &variable);
    let result = session.with_active(|| unsafe {
        session
            .sexp(super::do_d(args.as_raw()))
            .unwrap()
            .into_owned()
            .unwrap()
    });
    drop(args);
    drop(expression);
    drop(variable);
    crate::sexp::owner::with_runtime(&owner, |access| access.with_native(|owner| owner.full_gc()))
        .unwrap()
        .unwrap();
    assert!(
        result
            .try_car()
            .unwrap()
            .try_printname()
            .unwrap()
            .try_char_eq(b"psigamma")
            .unwrap()
    );
    let operands = result.try_cdr().unwrap();
    assert!(
        operands
            .try_car()
            .unwrap()
            .try_printname()
            .unwrap()
            .try_char_eq(b"x")
            .unwrap()
    );
    assert_eq!(
        operands
            .try_cdr()
            .unwrap()
            .try_car()
            .unwrap()
            .try_integer_elt(0)
            .unwrap(),
        1
    );
}

#[test]
fn owned_derivative_symbolic_power_graph_survives_gc() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let expression = call(&owner, "^", &[symbol(&owner, "x"), symbol(&owner, "y")]);
    let variable = owner
        .node_factory()
        .unwrap()
        .strings(&["x"])
        .unwrap()
        .into_owned()
        .unwrap();
    let args = arguments(&owner, &expression, &variable);
    let result = session.with_active(|| unsafe {
        session
            .sexp(super::do_d(args.as_raw()))
            .unwrap()
            .into_owned()
            .unwrap()
    });
    drop(args);
    drop(expression);
    drop(variable);
    crate::sexp::owner::with_runtime(&owner, |access| access.with_native(|owner| owner.full_gc()))
        .unwrap()
        .unwrap();
    assert_eq!(result.typeof_(), SEXPTYPE::LANGSXP);
    assert!(
        result
            .try_car()
            .unwrap()
            .try_printname()
            .unwrap()
            .try_char_eq(b"*")
            .unwrap()
    );
    assert_eq!(
        result.try_cdr().unwrap().try_car().unwrap().typeof_(),
        SEXPTYPE::LANGSXP
    );
}

#[test]
#[cfg(not(miri))]
fn owned_derivative_deep_unary_traversal_subprocess() {
    const CHILD: &str = "RPORT_DERIV_DEEP_TRAVERSAL_CHILD";
    if std::env::var_os(CHILD).is_some() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let session = RSession::new_for_gc_tests();
                let owner = session.owner_token().unwrap().weak_owner().unwrap();
                let mut expression = symbol(&owner, "x");
                for _ in 0..4096 {
                    expression = call(&owner, "+", &[expression]);
                }
                let variable = owner
                    .node_factory()
                    .unwrap()
                    .strings(&["x"])
                    .unwrap()
                    .into_owned()
                    .unwrap();
                let args = arguments(&owner, &expression, &variable);
                let result = session.with_active(|| unsafe {
                    session
                        .sexp(super::do_d(args.as_raw()))
                        .unwrap()
                        .into_owned()
                        .unwrap()
                });
                assert_eq!(result.try_real_elt(0).unwrap(), 1.0);
            })
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("library::stats::deriv::owned_tests::owned_derivative_deep_unary_traversal_subprocess")
        .args(["--exact", "--nocapture"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[cfg(not(miri))]
fn owned_derivative_deep_parentheses_subprocess() {
    const CHILD: &str = "RPORT_DERIV_DEEP_PARENTHESES_CHILD";
    if std::env::var_os(CHILD).is_some() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let session = RSession::new_for_gc_tests();
                let owner = session.owner_token().unwrap().weak_owner().unwrap();
                let mut expression = symbol(&owner, "x");
                for _ in 0..4096 {
                    expression = call(&owner, "f", &[expression]);
                }
                let result = crate::sexp::owner::with_runtime(&owner, |access| {
                    let kernel = super::owned::Kernel::new(access, super::derivative_symbol);
                    kernel.parentheses(&expression)
                })
                .unwrap()
                .unwrap();
                assert_eq!(result.typeof_(), SEXPTYPE::LANGSXP);
            })
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("library::stats::deriv::owned_tests::owned_derivative_deep_parentheses_subprocess")
        .args(["--exact", "--nocapture"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn owned_parentheses_rejects_cycles_and_accepts_shared_children() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let x = symbol(&owner, "x");
    let cycle = call(&owner, "+", std::slice::from_ref(&x));
    let cell = cycle.try_cdr().unwrap().into_owned().unwrap();
    let mut writable = crate::sexp::object::SexpMut::try_from_checked(cell).unwrap();
    writable.try_set_pairlist_car(&cycle).unwrap();
    drop(writable);
    let child = call(&owner, "+", &[x.clone(), x]);
    let shared = call(&owner, "*", &[child.clone(), child]);
    let result = crate::sexp::owner::with_runtime(&owner, |access| {
        let kernel = super::owned::Kernel::new(access, super::derivative_symbol);
        let error = kernel.parentheses(&cycle).unwrap_err();
        assert!(error.to_string().contains("cyclic derivative expression"));
        kernel.parentheses(&shared)
    })
    .unwrap()
    .unwrap();
    drop(shared);
    crate::sexp::owner::with_runtime(&owner, |access| access.with_native(|owner| owner.full_gc()))
        .unwrap()
        .unwrap();
    assert_eq!(result.typeof_(), SEXPTYPE::LANGSXP);
    assert!(
        result
            .try_car()
            .unwrap()
            .try_printname()
            .unwrap()
            .try_char_eq(b"*")
            .unwrap()
    );
}

fn parentheses_callback_cleanup(action: u8) {
    let facade = Rc::new(RefCell::new(Some(RSession::new_for_gc_tests())));
    let owner = facade
        .borrow()
        .as_ref()
        .unwrap()
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap();
    let x = symbol(&owner, "x");
    let child = call(&owner, "+", &[x.clone(), x.clone()]);
    let expression = call(&owner, "*", &[child, x]);
    let calls = Cell::new(0);
    let pin = owner.pin().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        crate::sexp::session::with_instance_active(pin.as_ptr(), || {
            crate::sexp::owner::with_runtime(&owner, |access| {
                let kernel = super::owned::Kernel::new(access, |access, name| {
                    calls.set(calls.get() + 1);
                    access.with_native(|runtime| runtime.full_gc())?;
                    if action == 2 {
                        facade.borrow_mut().as_mut().unwrap().close();
                    }
                    if action == 1 {
                        std::panic::panic_any(918_u32);
                    }
                    super::derivative_symbol(access, name)
                });
                kernel.parentheses(&expression)
            })
        })
    }));
    assert!(calls.get() > 0);
    match action {
        0 => assert_eq!(
            result.unwrap().unwrap().unwrap().typeof_(),
            SEXPTYPE::LANGSXP
        ),
        1 => assert_eq!(*result.unwrap_err().downcast::<u32>().unwrap(), 918),
        2 => assert!(matches!(
            result.unwrap(),
            Err(crate::sexp::object::SexpError::RootUnavailable)
        )),
        _ => unreachable!(),
    }
}

#[test]
fn owned_parentheses_callback_gc_and_panic_cleanup() {
    parentheses_callback_cleanup(0);
    parentheses_callback_cleanup(1);
}

#[test]
fn owned_parentheses_callback_original_owner_revocation() {
    parentheses_callback_cleanup(2);
}

#[test]
#[cfg(not(miri))]
fn owned_derivative_deep_mixed_unary_subprocess() {
    const CHILD: &str = "RPORT_DERIV_DEEP_MIXED_UNARY_CHILD";
    if std::env::var_os(CHILD).is_some() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let session = RSession::new_for_gc_tests();
                let owner = session.owner_token().unwrap().weak_owner().unwrap();
                let mut expression = symbol(&owner, "x");
                for index in 0..4096 {
                    let head = match index % 4 {
                        0 => "+",
                        1 | 2 => "-",
                        _ => "(",
                    };
                    expression = call(&owner, head, &[expression]);
                }
                let variable = owner
                    .node_factory()
                    .unwrap()
                    .strings(&["x"])
                    .unwrap()
                    .into_owned()
                    .unwrap();
                let args = arguments(&owner, &expression, &variable);
                let result = session.with_active(|| unsafe {
                    session
                        .sexp(super::do_d(args.as_raw()))
                        .unwrap()
                        .into_owned()
                        .unwrap()
                });
                assert_eq!(result.try_real_elt(0).unwrap(), 1.0);
            })
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("library::stats::deriv::owned_tests::owned_derivative_deep_mixed_unary_subprocess")
        .args(["--exact", "--nocapture"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[cfg(not(miri))]
fn owned_derivative_deep_additive_subprocess() {
    const CHILD: &str = "RPORT_DERIV_DEEP_ADDITIVE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let session = RSession::new_for_gc_tests();
                let owner = session.owner_token().unwrap().weak_owner().unwrap();
                let mut expression = symbol(&owner, "x");
                let constant = symbol(&owner, "y");
                for index in 0..2048 {
                    expression = match index % 4 {
                        0 => call(&owner, "+", &[expression, constant.clone()]),
                        1 => call(&owner, "-", &[constant.clone(), expression]),
                        2 => call(&owner, "-", &[expression]),
                        _ => call(&owner, "(", &[expression]),
                    };
                }
                let variable = owner
                    .node_factory()
                    .unwrap()
                    .strings(&["x"])
                    .unwrap()
                    .into_owned()
                    .unwrap();
                let args = arguments(&owner, &expression, &variable);
                let result = session.with_active(|| unsafe {
                    session
                        .sexp(super::do_d(args.as_raw()))
                        .unwrap()
                        .into_owned()
                        .unwrap()
                });
                assert_eq!(result.try_real_elt(0).unwrap(), 1.0);
            })
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("library::stats::deriv::owned_tests::owned_derivative_deep_additive_subprocess")
        .args(["--exact", "--nocapture"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[cfg_attr(miri, ignore = "real-base GNU shape checks run natively")]
fn owned_derivative_additive_pinned_shapes_and_left_error() {
    let mut session = RSession::new_without_default_packages();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let variable = code(&mut session, "'x'");
    for (source, expected) in [
        ("(x+y)-(y-x)", "1 + 1"),
        ("x - (y - x)", "1 + 1"),
        ("-(x^2) + (x*y-y*x)", "y - y - 2 * x"),
        ("(x*y)+(x*y)", "y + y"),
    ] {
        let expression = code(&mut session, &format!("quote({source})"));
        let args = arguments(&owner, &expression, &variable);
        let derivative = session.with_active(|| unsafe {
            session
                .sexp(super::do_d(args.as_raw()))
                .unwrap()
                .into_owned()
                .unwrap()
        });
        assert!(session.define_var("saved_derivative", derivative));
        assert!(
            code(
                &mut session,
                &format!("identical(saved_derivative, quote({expected}))")
            )
            .try_to_bool()
            .unwrap(),
            "{source}"
        );
    }
    let expression = code(&mut session, "quote(foo(x)+bar(x))");
    let args = arguments(&owner, &expression, &variable);
    let error = session.with_active(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::do_d(args.as_raw())
        }))
        .unwrap_err()
    });
    assert!(
        error
            .downcast::<crate::sexp::context::RError>()
            .unwrap()
            .message
            .contains("Function 'foo' is not in the derivatives table")
    );
}

#[test]
fn owned_derivative_additive_shared_occurrences_are_independent() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let shared = call(&owner, "^", &[symbol(&owner, "x"), numeric(&owner, 2.)]);
    let expression = call(&owner, "+", &[shared.clone(), shared]);
    let variable = owner
        .node_factory()
        .unwrap()
        .strings(&["x"])
        .unwrap()
        .into_owned()
        .unwrap();
    let args = arguments(&owner, &expression, &variable);
    let result = session.with_active(|| unsafe {
        session
            .sexp(super::do_d(args.as_raw()))
            .unwrap()
            .into_owned()
            .unwrap()
    });
    let first = result
        .try_cdr()
        .unwrap()
        .try_car()
        .unwrap()
        .into_owned()
        .unwrap();
    let second = result
        .try_cdr()
        .unwrap()
        .try_cdr()
        .unwrap()
        .try_car()
        .unwrap()
        .into_owned()
        .unwrap();
    assert_eq!(first.typeof_(), SEXPTYPE::LANGSXP);
    assert_eq!(second.typeof_(), SEXPTYPE::LANGSXP);
    assert_ne!(first.as_raw(), second.as_raw());
    drop(args);
    drop(expression);
    crate::sexp::owner::with_runtime(&owner, |access| access.with_native(|owner| owner.full_gc()))
        .unwrap()
        .unwrap();
    assert_eq!(
        first
            .try_car()
            .unwrap()
            .try_printname()
            .unwrap()
            .try_as_string()
            .unwrap(),
        "*"
    );
    assert_eq!(
        second
            .try_car()
            .unwrap()
            .try_printname()
            .unwrap()
            .try_as_string()
            .unwrap(),
        "*"
    );
}

#[test]
#[cfg(not(miri))]
fn owned_derivative_deep_common_unary_subprocess() {
    const CHILD: &str = "RPORT_DERIV_DEEP_COMMON_UNARY_CHILD";
    if std::env::var_os(CHILD).is_some() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let session = RSession::new_for_gc_tests();
                let owner = session.owner_token().unwrap().weak_owner().unwrap();
                let mut expression = symbol(&owner, "y");
                for index in 0..2048 {
                    expression = call(
                        &owner,
                        ["sin", "cos", "tan", "exp", "log", "-", "("][index % 7],
                        &[expression],
                    );
                }
                let variable = owner
                    .node_factory()
                    .unwrap()
                    .strings(&["x"])
                    .unwrap()
                    .into_owned()
                    .unwrap();
                let args = arguments(&owner, &expression, &variable);
                let result = session.with_active(|| unsafe {
                    session
                        .sexp(super::do_d(args.as_raw()))
                        .unwrap()
                        .into_owned()
                        .unwrap()
                });
                assert_eq!(result.try_real_elt(0).unwrap(), 0.0);
            })
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("library::stats::deriv::owned_tests::owned_derivative_deep_common_unary_subprocess")
        .args(["--exact", "--nocapture"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn owned_derivative_common_unary_gc_detached_inputs() {
    collecting_derivative(0, true, 0);
}
#[test]
fn owned_derivative_common_unary_gc_panic_identity_and_recovery() {
    collecting_derivative(1, true, 0);
}
#[test]
fn owned_derivative_common_unary_gc_original_revocation() {
    collecting_derivative(2, true, 0);
}

#[test]
#[cfg_attr(miri, ignore = "real-base GNU shape controls run natively")]
fn owned_derivative_common_unary_pinned_shapes_and_log_error_order() {
    let mut session = RSession::new_without_default_packages();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let variable = code(&mut session, "'x'");
    for (source, expected) in [
        ("sin(exp(x))", "cos(exp(x)) * exp(x)"),
        ("cos(log(x))", "-(sin(log(x)) * (1/x))"),
        ("tan(sin(x))", "cos(x)/cos(sin(x))^2"),
        ("log(exp(x))", "exp(x)/exp(x)"),
        ("sin(x)+cos(x)-tan(x)", "cos(x) - sin(x) - 1/cos(x)^2"),
        ("exp(log(x))", "exp(log(x)) * (1/x)"),
    ] {
        let expression = code(&mut session, &format!("quote({source})"));
        let args = arguments(&owner, &expression, &variable);
        let derivative = session.with_active(|| unsafe {
            session
                .sexp(super::do_d(args.as_raw()))
                .unwrap()
                .into_owned()
                .unwrap()
        });
        assert!(session.define_var("saved_derivative", derivative));
        assert!(
            code(
                &mut session,
                &format!("identical(saved_derivative, quote({expected}))")
            )
            .try_to_bool()
            .unwrap(),
            "{source}"
        );
    }
    let expression = code(&mut session, "quote(log(foo(x), 2))");
    let args = arguments(&owner, &expression, &variable);
    let error = session.with_active(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::do_d(args.as_raw())
        }))
        .unwrap_err()
    });
    assert!(
        error
            .downcast::<crate::sexp::context::RError>()
            .unwrap()
            .message
            .starts_with("only single-argument calls to log() are supported")
    );
}

#[test]
fn owned_derivative_common_unary_cycles_reject_and_recover() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let x = symbol(&owner, "x");
    let variable = owner
        .node_factory()
        .unwrap()
        .strings(&["x"])
        .unwrap()
        .into_owned()
        .unwrap();
    for head in ["sin", "cos", "tan", "exp", "log"] {
        let cyclic = call(&owner, head, std::slice::from_ref(&x));
        let cell = cyclic.try_cdr().unwrap().into_owned().unwrap();
        let mut writable = crate::sexp::object::SexpMut::try_from_checked(cell).unwrap();
        writable.try_set_pairlist_car(&cyclic).unwrap();
        drop(writable);
        let args = arguments(&owner, &cyclic, &variable);
        let error = session.with_active(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                super::do_d(args.as_raw())
            }))
            .unwrap_err()
        });
        assert!(
            error
                .downcast::<crate::sexp::context::RError>()
                .unwrap()
                .message
                .contains("cyclic derivative expression")
        );
    }
    let args = arguments(&owner, &x, &variable);
    let result = session.with_active(|| unsafe {
        session
            .sexp(super::do_d(args.as_raw()))
            .unwrap()
            .into_owned()
            .unwrap()
    });
    assert_eq!(result.try_real_elt(0).unwrap(), 1.);
}

#[test]
#[cfg_attr(miri, ignore = "real-base shape and evaluation checks run natively")]
fn owned_derivative_product_quotient_shapes_and_error_order() {
    let mut session = RSession::new_without_default_packages();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let variable = code(&mut session, "'x'");
    for (source, expected, value) in [
        ("x*y", "y", "3"),
        ("x/y", "1/y", "1/3"),
        ("x*(y+1)", "(y+1)", "4"),
    ] {
        let expression = code(&mut session, &format!("quote({source})"));
        let args = arguments(&owner, &expression, &variable);
        let derivative = session.with_active(|| unsafe {
            session
                .sexp(super::do_d(args.as_raw()))
                .unwrap()
                .into_owned()
                .unwrap()
        });
        assert!(session.define_var("saved_derivative", derivative));
        assert!(
            code(
                &mut session,
                &format!("identical(saved_derivative, quote({expected}))")
            )
            .try_to_bool()
            .unwrap(),
            "{source}"
        );
        assert!(
            code(
                &mut session,
                &format!("x <- 2; y <- 3; abs(eval(saved_derivative) - ({value})) < 1e-14")
            )
            .try_to_bool()
            .unwrap(),
            "{source}"
        );
    }

    // GNU walks the left operand first, so both operations report foo before bar.
    for source in ["foo(x) * bar(x)", "foo(x) / bar(x)"] {
        let expression = code(&mut session, &format!("quote({source})"));
        let args = arguments(&owner, &expression, &variable);
        let error = session.with_active(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                super::do_d(args.as_raw())
            }))
            .unwrap_err()
        });
        assert!(
            error
                .downcast::<crate::sexp::context::RError>()
                .unwrap()
                .message
                .contains("Function 'foo' is not in the derivatives table"),
            "{source}"
        );
    }
    // Missing right operands are rejected before traversing the left child.
    for operator in ["*", "/"] {
        let left = call(&owner, "foo", &[symbol(&owner, "x")]);
        let expression = call(&owner, operator, &[left]);
        let args = arguments(&owner, &expression, &variable);
        let error = session.with_active(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                super::do_d(args.as_raw())
            }))
            .unwrap_err()
        });
        assert!(
            error
                .downcast::<crate::sexp::context::RError>()
                .unwrap()
                .message
                .contains("invalid binary derivative expression"),
            "{operator}"
        );
    }
}

#[test]
fn owned_derivative_product_quotient_cycles_reject_and_recover() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let x = symbol(&owner, "x");
    let y = symbol(&owner, "y");
    let variable = owner
        .node_factory()
        .unwrap()
        .strings(&["x"])
        .unwrap()
        .into_owned()
        .unwrap();
    for operator in ["*", "/"] {
        let expression = call(&owner, operator, &[x.clone(), y.clone()]);
        let first_cell = expression.try_cdr().unwrap().into_owned().unwrap();
        let mut writable = crate::sexp::object::SexpMut::try_from_checked(first_cell).unwrap();
        writable.try_set_pairlist_car(&expression).unwrap();
        drop(writable);
        let args = arguments(&owner, &expression, &variable);
        let error = session.with_active(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                super::do_d(args.as_raw())
            }))
            .unwrap_err()
        });
        assert!(
            error
                .downcast::<crate::sexp::context::RError>()
                .unwrap()
                .message
                .contains("cyclic derivative expression")
        );
    }
    let args = arguments(&owner, &x, &variable);
    let result =
        session.with_active(|| unsafe { session.sexp(super::do_d(args.as_raw())).unwrap() });
    assert_eq!(result.try_real_elt(0).unwrap(), 1.);
}

#[test]
fn owned_derivative_product_frame_admission_denial_recovers() {
    let mut session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let x = symbol(&owner, "x");
    let mut expression = x.clone();
    for _ in 0..64 {
        expression = call(&owner, "*", &[expression, x.clone()]);
    }
    let variable = owner
        .node_factory()
        .unwrap()
        .strings(&["x"])
        .unwrap()
        .into_owned()
        .unwrap();
    let args = arguments(&owner, &expression, &variable);
    let preinstalled: Vec<_> = ["+", "-", "*", "/", "^"]
        .into_iter()
        .map(|operator| symbol(&owner, operator))
        .collect();
    let prior = session.arena_budget();
    let used = crate::sexp::owner::with_runtime(&owner, |access| {
        access.with_arena(|arena| arena.total_bytes_allocated())
    })
    .unwrap()
    .unwrap();
    session.set_arena_budget(crate::sexp::memory::ArenaBudget::new(
        used + 8192,
        prior.max_nodes,
    ));
    let error = session.with_active(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::do_d(args.as_raw())
        }))
        .unwrap_err()
    });
    let message = error
        .downcast::<crate::sexp::context::RError>()
        .unwrap()
        .message;
    assert!(
        message.contains("cannot inspect derivative expression path")
            || message.contains("cannot retain derivative frames")
            || message.contains("session memory limit"),
        "{message}"
    );
    drop(preinstalled);
    session.set_arena_budget(prior);
    let result =
        session.with_active(|| unsafe { session.sexp(super::do_d(args.as_raw())).unwrap() });
    assert_eq!(result.typeof_(), SEXPTYPE::LANGSXP);
}

#[test]
#[cfg(not(miri))]
fn owned_derivative_deep_product_quotient_subprocess() {
    const CHILD: &str = "RPORT_DERIV_DEEP_PRODUCT_QUOTIENT_CHILD";
    if std::env::var_os(CHILD).is_some() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let session = RSession::new_for_gc_tests();
                let owner = session.owner_token().unwrap().weak_owner().unwrap();
                let y = symbol(&owner, "y");
                let mut expression = y.clone();
                for index in 0..2048 {
                    expression = match index % 4 {
                        0 => call(&owner, "*", &[expression, y.clone()]),
                        1 => call(&owner, "/", &[y.clone(), expression]),
                        2 => call(&owner, "*", &[y.clone(), expression]),
                        _ => call(&owner, "/", &[expression, y.clone()]),
                    };
                }
                let variable = owner
                    .node_factory()
                    .unwrap()
                    .strings(&["x"])
                    .unwrap()
                    .into_owned()
                    .unwrap();
                let args = arguments(&owner, &expression, &variable);
                let result = session.with_active(|| unsafe {
                    session
                        .sexp(super::do_d(args.as_raw()))
                        .unwrap()
                        .into_owned()
                        .unwrap()
                });
                assert_eq!(result.try_real_elt(0).unwrap(), 0.);
            })
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .arg(
            "library::stats::deriv::owned_tests::owned_derivative_deep_product_quotient_subprocess",
        )
        .args(["--exact", "--nocapture"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[cfg_attr(
    miri,
    ignore = "GNU hyperbolic shape and numerical checks run natively"
)]
fn owned_derivative_hyperbolic_shapes_and_order() {
    let mut session = RSession::new_without_default_packages();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let variable = code(&mut session, "'x'");
    for (source, expected, value) in [
        ("sinh(x)", "cosh(x)", "cosh(1)"),
        ("cosh(x)", "sinh(x)", "sinh(1)"),
        ("tanh(x)", "1/cosh(x)^2", "1/cosh(1)^2"),
    ] {
        let expression = code(&mut session, &format!("quote({source})"));
        let args = arguments(&owner, &expression, &variable);
        let derivative = session.with_active(|| unsafe {
            session
                .sexp(super::do_d(args.as_raw()))
                .unwrap()
                .into_owned()
                .unwrap()
        });
        assert!(session.define_var("saved_derivative", derivative));
        assert!(
            code(
                &mut session,
                &format!("identical(saved_derivative, quote({expected}))")
            )
            .try_to_bool()
            .unwrap(),
            "shape for {source}"
        );
        assert!(
            code(
                &mut session,
                &format!("x <- 1; abs(eval(saved_derivative) - ({value})) < 1e-12")
            )
            .try_to_bool()
            .unwrap(),
            "value for {source}"
        );
    }
    for operator in ["sinh", "cosh", "tanh"] {
        let expression = code(&mut session, &format!("quote({operator}(foo(x)))"));
        let args = arguments(&owner, &expression, &variable);
        let error = session.with_active(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                super::do_d(args.as_raw())
            }))
            .unwrap_err()
        });
        assert!(
            error
                .downcast::<crate::sexp::context::RError>()
                .unwrap()
                .message
                .contains("Function 'foo' is not in the derivatives table"),
            "{operator}"
        );
    }
}

#[test]
#[cfg_attr(
    miri,
    ignore = "GNU inverse-trig shape and numerical checks run natively"
)]
fn owned_derivative_inverse_trig_shapes_and_order() {
    let mut session = RSession::new_without_default_packages();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let variable = code(&mut session, "'x'");
    for (source, expected, value) in [
        ("asin(x)", "1/sqrt(1 - x^2)", "1/sqrt(1 - 0.3^2)"),
        ("acos(x)", "-(1/sqrt(1 - x^2))", "-1/sqrt(1 - 0.3^2)"),
        ("atan(x)", "1/(1 + x^2)", "1/(1 + 0.3^2)"),
    ] {
        let expression = code(&mut session, &format!("quote({source})"));
        let args = arguments(&owner, &expression, &variable);
        let derivative = session.with_active(|| unsafe {
            session
                .sexp(super::do_d(args.as_raw()))
                .unwrap()
                .into_owned()
                .unwrap()
        });
        assert!(session.define_var("saved_derivative", derivative));
        assert!(
            code(
                &mut session,
                &format!("identical(saved_derivative, quote({expected}))")
            )
            .try_to_bool()
            .unwrap(),
            "shape for {source}"
        );
        assert!(
            code(
                &mut session,
                &format!("x <- 0.3; abs(eval(saved_derivative) - ({value})) < 1e-12")
            )
            .try_to_bool()
            .unwrap(),
            "value for {source}"
        );
    }
    for operator in ["asin", "acos", "atan"] {
        let expression = code(&mut session, &format!("quote({operator}(foo(x)))"));
        let args = arguments(&owner, &expression, &variable);
        let error = session.with_active(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                super::do_d(args.as_raw())
            }))
            .unwrap_err()
        });
        assert!(
            error
                .downcast::<crate::sexp::context::RError>()
                .unwrap()
                .message
                .contains("Function 'foo' is not in the derivatives table"),
            "{operator}"
        );
    }
}

#[test]
fn owned_derivative_inverse_trig_cycles_reject_and_recover() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let x = symbol(&owner, "x");
    let variable = owner
        .node_factory()
        .unwrap()
        .strings(&["x"])
        .unwrap()
        .into_owned()
        .unwrap();
    for operator in ["asin", "acos", "atan"] {
        let expression = call(&owner, operator, std::slice::from_ref(&x));
        let first_cell = expression.try_cdr().unwrap().into_owned().unwrap();
        let mut writable = crate::sexp::object::SexpMut::try_from_checked(first_cell).unwrap();
        writable.try_set_pairlist_car(&expression).unwrap();
        drop(writable);
        let args = arguments(&owner, &expression, &variable);
        let error = session.with_active(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                super::do_d(args.as_raw())
            }))
            .unwrap_err()
        });
        assert!(
            error
                .downcast::<crate::sexp::context::RError>()
                .unwrap()
                .message
                .contains("cyclic derivative expression"),
            "{operator}"
        );
    }
    let args = arguments(&owner, &x, &variable);
    let result =
        session.with_active(|| unsafe { session.sexp(super::do_d(args.as_raw())).unwrap() });
    assert_eq!(result.try_real_elt(0).unwrap(), 1.);
}

#[test]
fn owned_derivative_inverse_trig_frame_admission_denial_recovers() {
    let mut session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let y = symbol(&owner, "y");
    let mut expression = y.clone();
    for index in 0..64 {
        expression = call(&owner, ["asin", "acos", "atan"][index % 3], &[expression]);
    }
    let variable = owner
        .node_factory()
        .unwrap()
        .strings(&["x"])
        .unwrap()
        .into_owned()
        .unwrap();
    let args = arguments(&owner, &expression, &variable);
    let preinstalled: Vec<_> = ["+", "-", "*", "/", "^", "sqrt", "asin", "acos", "atan"]
        .into_iter()
        .map(|operator| symbol(&owner, operator))
        .collect();
    let prior = session.arena_budget();
    let used = crate::sexp::owner::with_runtime(&owner, |access| {
        access.with_arena(|arena| arena.total_bytes_allocated())
    })
    .unwrap()
    .unwrap();
    session.set_arena_budget(crate::sexp::memory::ArenaBudget::new(
        used + 8192,
        prior.max_nodes,
    ));
    let error = session.with_active(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::do_d(args.as_raw())
        }))
        .unwrap_err()
    });
    let message = error
        .downcast::<crate::sexp::context::RError>()
        .unwrap()
        .message;
    assert!(
        message.contains("cannot inspect derivative expression path")
            || message.contains("cannot retain derivative frames")
            || message.contains("session memory limit"),
        "{message}"
    );
    drop(preinstalled);
    session.set_arena_budget(prior);
    let args = arguments(&owner, &y, &variable);
    let result =
        session.with_active(|| unsafe { session.sexp(super::do_d(args.as_raw())).unwrap() });
    assert_eq!(result.try_real_elt(0).unwrap(), 0.);
}

#[test]
#[cfg(not(miri))]
fn owned_derivative_deep_inverse_trig_traversal_subprocess() {
    const CHILD: &str = "RPORT_DERIV_DEEP_INVERSE_TRIG_CHILD";
    if std::env::var_os(CHILD).is_some() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let session = RSession::new_for_gc_tests();
                let owner = session.owner_token().unwrap().weak_owner().unwrap();
                let y = symbol(&owner, "y");
                let mut expression = y.clone();
                let operators = ["asin", "acos", "atan"];
                for index in 0..2048 {
                    expression = call(&owner, operators[index % operators.len()], &[expression]);
                }
                let variable = owner
                    .node_factory()
                    .unwrap()
                    .strings(&["x"])
                    .unwrap()
                    .into_owned()
                    .unwrap();
                let args = arguments(&owner, &expression, &variable);
                let result = session.with_active(|| unsafe {
                    session
                        .sexp(super::do_d(args.as_raw()))
                        .unwrap()
                        .into_owned()
                        .unwrap()
                });
                assert_eq!(result.try_real_elt(0).unwrap(), 0.);
            })
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("library::stats::deriv::owned_tests::owned_derivative_deep_inverse_trig_traversal_subprocess")
        .args(["--exact", "--nocapture"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn owned_derivative_hyperbolic_cycles_reject_and_recover() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let x = symbol(&owner, "x");
    let variable = owner
        .node_factory()
        .unwrap()
        .strings(&["x"])
        .unwrap()
        .into_owned()
        .unwrap();
    for operator in ["sinh", "cosh", "tanh"] {
        let expression = call(&owner, operator, std::slice::from_ref(&x));
        let first_cell = expression.try_cdr().unwrap().into_owned().unwrap();
        let mut writable = crate::sexp::object::SexpMut::try_from_checked(first_cell).unwrap();
        writable.try_set_pairlist_car(&expression).unwrap();
        drop(writable);
        let args = arguments(&owner, &expression, &variable);
        let error = session.with_active(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                super::do_d(args.as_raw())
            }))
            .unwrap_err()
        });
        assert!(
            error
                .downcast::<crate::sexp::context::RError>()
                .unwrap()
                .message
                .contains("cyclic derivative expression"),
            "{operator}"
        );
    }
    let args = arguments(&owner, &x, &variable);
    let result =
        session.with_active(|| unsafe { session.sexp(super::do_d(args.as_raw())).unwrap() });
    assert_eq!(result.try_real_elt(0).unwrap(), 1.);
}

#[test]
fn owned_derivative_hyperbolic_frame_admission_denial_recovers() {
    let mut session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let y = symbol(&owner, "y");
    let mut expression = y.clone();
    for index in 0..64 {
        expression = call(&owner, ["sinh", "cosh", "tanh"][index % 3], &[expression]);
    }
    let variable = owner
        .node_factory()
        .unwrap()
        .strings(&["x"])
        .unwrap()
        .into_owned()
        .unwrap();
    let args = arguments(&owner, &expression, &variable);
    let preinstalled: Vec<_> = ["+", "-", "*", "/", "^", "sinh", "cosh", "tanh"]
        .into_iter()
        .map(|operator| symbol(&owner, operator))
        .collect();
    let prior = session.arena_budget();
    let used = crate::sexp::owner::with_runtime(&owner, |access| {
        access.with_arena(|arena| arena.total_bytes_allocated())
    })
    .unwrap()
    .unwrap();
    session.set_arena_budget(crate::sexp::memory::ArenaBudget::new(
        used + 8192,
        prior.max_nodes,
    ));
    let error = session.with_active(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::do_d(args.as_raw())
        }))
        .unwrap_err()
    });
    let message = error
        .downcast::<crate::sexp::context::RError>()
        .unwrap()
        .message;
    assert!(
        message.contains("cannot inspect derivative expression path")
            || message.contains("cannot retain derivative frames")
            || message.contains("session memory limit"),
        "{message}"
    );
    drop(preinstalled);
    session.set_arena_budget(prior);
    let args = arguments(&owner, &y, &variable);
    let result =
        session.with_active(|| unsafe { session.sexp(super::do_d(args.as_raw())).unwrap() });
    assert_eq!(result.try_real_elt(0).unwrap(), 0.);
}

#[test]
#[cfg(not(miri))]
fn owned_derivative_deep_hyperbolic_traversal_subprocess() {
    const CHILD: &str = "RPORT_DERIV_DEEP_HYPERBOLIC_CHILD";
    if std::env::var_os(CHILD).is_some() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let session = RSession::new_for_gc_tests();
                let owner = session.owner_token().unwrap().weak_owner().unwrap();
                let y = symbol(&owner, "y");
                let mut expression = y.clone();
                let operators = ["sinh", "cosh", "tanh"];
                for index in 0..2048 {
                    expression = call(&owner, operators[index % operators.len()], &[expression]);
                }
                let variable = owner
                    .node_factory()
                    .unwrap()
                    .strings(&["x"])
                    .unwrap()
                    .into_owned()
                    .unwrap();
                let args = arguments(&owner, &expression, &variable);
                let result = session.with_active(|| unsafe {
                    session
                        .sexp(super::do_d(args.as_raw()))
                        .unwrap()
                        .into_owned()
                        .unwrap()
                });
                assert_eq!(result.try_real_elt(0).unwrap(), 0.);
            })
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("library::stats::deriv::owned_tests::owned_derivative_deep_hyperbolic_traversal_subprocess")
        .args(["--exact", "--nocapture"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[cfg_attr(miri, ignore = "GNU shape and numerical oracle checks run natively")]
fn owned_derivative_power_shapes_and_construction_order() {
    let mut session = RSession::new_without_default_packages();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let variable = code(&mut session, "'x'");
    for (source, expected, value) in [
        ("x^2", "2 * x", "4"),
        ("x^y", "x^(y - 1) * y", "2^(3-1) * 3"),
        ("y^x", "y^x * log(y)", "3^2 * log(3)"),
    ] {
        let expression = code(&mut session, &format!("quote({source})"));
        let args = arguments(&owner, &expression, &variable);
        let derivative = session.with_active(|| unsafe {
            session
                .sexp(super::do_d(args.as_raw()))
                .unwrap()
                .into_owned()
                .unwrap()
        });
        assert!(session.define_var("saved_derivative", derivative));
        assert!(
            code(
                &mut session,
                &format!("identical(saved_derivative, quote({expected}))")
            )
            .try_to_bool()
            .unwrap(),
            "shape for {source}"
        );
        assert!(
            code(
                &mut session,
                &format!("x <- 2; y <- 3; abs(eval(saved_derivative) - ({value})) < 1e-12")
            )
            .try_to_bool()
            .unwrap(),
            "value for {source}"
        );
    }

    // For symbolic exponents GNU finishes D(base) before D(exponent).
    let expression = code(&mut session, "quote(foo(x)^bar(x))");
    let args = arguments(&owner, &expression, &variable);
    let error = session.with_active(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::do_d(args.as_raw())
        }))
        .unwrap_err()
    });
    assert!(
        error
            .downcast::<crate::sexp::context::RError>()
            .unwrap()
            .message
            .contains("Function 'foo' is not in the derivatives table")
    );

    // As in GNU, a missing exponent is reported before traversing the base.
    let left = call(&owner, "foo", &[symbol(&owner, "x")]);
    let expression = call(&owner, "^", &[left]);
    let args = arguments(&owner, &expression, &variable);
    let error = session.with_active(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::do_d(args.as_raw())
        }))
        .unwrap_err()
    });
    assert!(
        error
            .downcast::<crate::sexp::context::RError>()
            .unwrap()
            .message
            .contains("invalid binary derivative expression")
    );
}

#[test]
fn owned_derivative_power_cycles_reject_and_recover() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let x = symbol(&owner, "x");
    let y = symbol(&owner, "y");
    let variable = owner
        .node_factory()
        .unwrap()
        .strings(&["x"])
        .unwrap()
        .into_owned()
        .unwrap();
    let expression = call(&owner, "^", &[x.clone(), y]);
    let first_cell = expression.try_cdr().unwrap().into_owned().unwrap();
    let mut writable = crate::sexp::object::SexpMut::try_from_checked(first_cell).unwrap();
    writable.try_set_pairlist_car(&expression).unwrap();
    drop(writable);
    let args = arguments(&owner, &expression, &variable);
    let error = session.with_active(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::do_d(args.as_raw())
        }))
        .unwrap_err()
    });
    assert!(
        error
            .downcast::<crate::sexp::context::RError>()
            .unwrap()
            .message
            .contains("cyclic derivative expression")
    );
    let args = arguments(&owner, &x, &variable);
    let result =
        session.with_active(|| unsafe { session.sexp(super::do_d(args.as_raw())).unwrap() });
    assert_eq!(result.try_real_elt(0).unwrap(), 1.);
}

#[test]
fn owned_derivative_power_frame_admission_denial_recovers() {
    let mut session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let y = symbol(&owner, "y");
    let exponent = numeric(&owner, 2.);
    let mut expression = y.clone();
    for _ in 0..64 {
        expression = call(&owner, "^", &[expression, exponent.clone()]);
    }
    let variable = owner
        .node_factory()
        .unwrap()
        .strings(&["x"])
        .unwrap()
        .into_owned()
        .unwrap();
    let args = arguments(&owner, &expression, &variable);
    let preinstalled: Vec<_> = ["+", "-", "*", "/", "^"]
        .into_iter()
        .map(|operator| symbol(&owner, operator))
        .collect();
    let prior = session.arena_budget();
    let used = crate::sexp::owner::with_runtime(&owner, |access| {
        access.with_arena(|arena| arena.total_bytes_allocated())
    })
    .unwrap()
    .unwrap();
    session.set_arena_budget(crate::sexp::memory::ArenaBudget::new(
        used + 8192,
        prior.max_nodes,
    ));
    let error = session.with_active(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::do_d(args.as_raw())
        }))
        .unwrap_err()
    });
    let message = error
        .downcast::<crate::sexp::context::RError>()
        .unwrap()
        .message;
    assert!(
        message.contains("cannot inspect derivative expression path")
            || message.contains("cannot retain derivative frames")
            || message.contains("session memory limit"),
        "{message}"
    );
    drop(preinstalled);
    session.set_arena_budget(prior);
    let args = arguments(&owner, &y, &variable);
    let result =
        session.with_active(|| unsafe { session.sexp(super::do_d(args.as_raw())).unwrap() });
    assert_eq!(result.try_real_elt(0).unwrap(), 0.);
}

#[test]
#[cfg(not(miri))]
fn owned_derivative_deep_power_traversal_subprocess() {
    const CHILD: &str = "RPORT_DERIV_DEEP_POWER_CHILD";
    if std::env::var_os(CHILD).is_some() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let session = RSession::new_for_gc_tests();
                let owner = session.owner_token().unwrap().weak_owner().unwrap();
                let y = symbol(&owner, "y");
                let mut expression = y.clone();
                let exponent = numeric(&owner, 2.);
                for index in 0..1536 {
                    expression = if index % 2 == 0 {
                        call(&owner, "^", &[expression, exponent.clone()])
                    } else {
                        call(&owner, "^", &[y.clone(), expression])
                    };
                }
                let variable = owner
                    .node_factory()
                    .unwrap()
                    .strings(&["x"])
                    .unwrap()
                    .into_owned()
                    .unwrap();
                let args = arguments(&owner, &expression, &variable);
                let result = session.with_active(|| unsafe {
                    session
                        .sexp(super::do_d(args.as_raw()))
                        .unwrap()
                        .into_owned()
                        .unwrap()
                });
                assert_eq!(result.try_real_elt(0).unwrap(), 0.);
            })
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("library::stats::deriv::owned_tests::owned_derivative_deep_power_traversal_subprocess")
        .args(["--exact", "--nocapture"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[cfg(not(miri))]
fn owned_derivative_deep_distribution_logs_traversal_subprocess() {
    const CHILD: &str = "RPORT_DERIV_DEEP_DISTRIBUTION_LOGS_CHILD";
    if std::env::var_os(CHILD).is_some() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let session = RSession::new_for_gc_tests();
                let owner = session.owner_token().unwrap().weak_owner().unwrap();
                let y = symbol(&owner, "y");
                let mut expression = y.clone();
                let operators = ["expm1", "log1p", "log2", "log10", "pnorm", "dnorm"];
                for index in 0..2048 {
                    expression = call(&owner, operators[index % operators.len()], &[expression]);
                }
                let variable = owner
                    .node_factory()
                    .unwrap()
                    .strings(&["x"])
                    .unwrap()
                    .into_owned()
                    .unwrap();
                let args = arguments(&owner, &expression, &variable);
                let result = session.with_active(|| unsafe {
                    session
                        .sexp(super::do_d(args.as_raw()))
                        .unwrap()
                        .into_owned()
                        .unwrap()
                });
                assert_eq!(result.try_real_elt(0).unwrap(), 0.);
            })
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("library::stats::deriv::owned_tests::owned_derivative_deep_distribution_logs_traversal_subprocess")
        .args(["--exact", "--nocapture"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn owned_derivative_distribution_logs_cycles_reject_and_recover() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let x = symbol(&owner, "x");
    let variable = owner
        .node_factory()
        .unwrap()
        .strings(&["x"])
        .unwrap()
        .into_owned()
        .unwrap();
    for operator in ["expm1", "log1p", "log2", "log10", "pnorm", "dnorm"] {
        let expression = call(&owner, operator, std::slice::from_ref(&x));
        let first_cell = expression.try_cdr().unwrap().into_owned().unwrap();
        let mut writable = crate::sexp::object::SexpMut::try_from_checked(first_cell).unwrap();
        writable.try_set_pairlist_car(&expression).unwrap();
        drop(writable);
        let args = arguments(&owner, &expression, &variable);
        let error = session.with_active(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                super::do_d(args.as_raw())
            }))
            .unwrap_err()
        });
        assert!(
            error
                .downcast::<crate::sexp::context::RError>()
                .unwrap()
                .message
                .contains("cyclic derivative expression"),
            "{operator}"
        );
    }
    let args = arguments(&owner, &x, &variable);
    let result =
        session.with_active(|| unsafe { session.sexp(super::do_d(args.as_raw())).unwrap() });
    assert_eq!(result.try_real_elt(0).unwrap(), 1.);
}

#[test]
fn owned_derivative_distribution_logs_frame_admission_denial_recovers() {
    let mut session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let y = symbol(&owner, "y");
    let mut expression = y.clone();
    for index in 0..64 {
        expression = call(
            &owner,
            ["expm1", "log1p", "log2", "log10", "pnorm", "dnorm"][index % 6],
            &[expression],
        );
    }
    let variable = owner
        .node_factory()
        .unwrap()
        .strings(&["x"])
        .unwrap()
        .into_owned()
        .unwrap();
    let args = arguments(&owner, &expression, &variable);
    let preinstalled: Vec<_> = [
        "+", "-", "*", "/", "^", "exp", "log", "dnorm", "expm1", "log1p", "log2", "log10", "pnorm",
    ]
    .into_iter()
    .map(|operator| symbol(&owner, operator))
    .collect();
    let prior = session.arena_budget();
    let used = crate::sexp::owner::with_runtime(&owner, |access| {
        access.with_arena(|arena| arena.total_bytes_allocated())
    })
    .unwrap()
    .unwrap();
    session.set_arena_budget(crate::sexp::memory::ArenaBudget::new(
        used + 8192,
        prior.max_nodes,
    ));
    let error = session.with_active(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::do_d(args.as_raw())
        }))
        .unwrap_err()
    });
    let message = error
        .downcast::<crate::sexp::context::RError>()
        .unwrap()
        .message;
    assert!(
        message.contains("cannot inspect derivative expression path")
            || message.contains("cannot retain derivative frames")
            || message.contains("session memory limit"),
        "{message}"
    );
    drop(preinstalled);
    session.set_arena_budget(prior);
    let args = arguments(&owner, &y, &variable);
    let result =
        session.with_active(|| unsafe { session.sexp(super::do_d(args.as_raw())).unwrap() });
    assert_eq!(result.try_real_elt(0).unwrap(), 0.);
}

#[test]
fn owned_derivative_expm1_frame_gc_retains_generated_graph() {
    collecting_derivative(0, false, 10);
}

#[test]
fn owned_derivative_expm1_frame_panic_recovers() {
    collecting_derivative(1, false, 10);
}

#[test]
fn owned_derivative_expm1_frame_revocation_rejects_success() {
    collecting_derivative(2, false, 10);
}

#[test]
fn owned_derivative_log1p_frame_gc_retains_generated_graph() {
    collecting_derivative(0, false, 11);
}

#[test]
fn owned_derivative_log1p_frame_panic_recovers() {
    collecting_derivative(1, false, 11);
}

#[test]
fn owned_derivative_log1p_frame_revocation_rejects_success() {
    collecting_derivative(2, false, 11);
}

#[test]
fn owned_derivative_log2_frame_gc_retains_generated_graph() {
    collecting_derivative(0, false, 12);
}

#[test]
fn owned_derivative_log2_frame_panic_recovers() {
    collecting_derivative(1, false, 12);
}

#[test]
fn owned_derivative_log2_frame_revocation_rejects_success() {
    collecting_derivative(2, false, 12);
}

#[test]
fn owned_derivative_log10_frame_gc_retains_generated_graph() {
    collecting_derivative(0, false, 13);
}

#[test]
fn owned_derivative_log10_frame_panic_recovers() {
    collecting_derivative(1, false, 13);
}

#[test]
fn owned_derivative_log10_frame_revocation_rejects_success() {
    collecting_derivative(2, false, 13);
}

#[test]
fn owned_derivative_pnorm_frame_gc_retains_generated_graph() {
    collecting_derivative(0, false, 14);
}

#[test]
fn owned_derivative_pnorm_frame_panic_recovers() {
    collecting_derivative(1, false, 14);
}

#[test]
fn owned_derivative_pnorm_frame_revocation_rejects_success() {
    collecting_derivative(2, false, 14);
}

#[test]
fn owned_derivative_dnorm_frame_gc_retains_generated_graph() {
    collecting_derivative(0, false, 15);
}

#[test]
fn owned_derivative_dnorm_frame_panic_recovers() {
    collecting_derivative(1, false, 15);
}

#[test]
fn owned_derivative_dnorm_frame_revocation_rejects_success() {
    collecting_derivative(2, false, 15);
}

#[test]
#[cfg_attr(
    miri,
    ignore = "GNU distribution/logarithm shape and numerical checks run natively"
)]
fn owned_derivative_distribution_logs_shapes_and_order() {
    let mut session = RSession::new_without_default_packages();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let variable = code(&mut session, "'x'");
    for (source, expected, value) in [
        ("expm1(x)", "exp(x)", "exp(0.3)"),
        ("log1p(x)", "1/(1 + x)", "1/1.3"),
        ("log2(x)", "1/(x * log(2))", "1/(0.3 * log(2))"),
        ("log10(x)", "1/(x * log(10))", "1/(0.3 * log(10))"),
        ("pnorm(x)", "dnorm(x)", "dnorm(0.3)"),
        ("dnorm(x)", "-(x * dnorm(x))", "-(0.3 * dnorm(0.3))"),
    ] {
        let expression = code(&mut session, &format!("quote({source})"));
        let args = arguments(&owner, &expression, &variable);
        let derivative = session.with_active(|| unsafe {
            session
                .sexp(super::do_d(args.as_raw()))
                .unwrap()
                .into_owned()
                .unwrap()
        });
        assert!(session.define_var("saved_derivative", derivative));
        assert!(
            code(
                &mut session,
                &format!("identical(saved_derivative, quote({expected}))")
            )
            .try_to_bool()
            .unwrap(),
            "shape for {source}"
        );
        assert!(
            code(
                &mut session,
                &format!("x <- 0.3; abs(eval(saved_derivative) - ({value})) < 1e-12")
            )
            .try_to_bool()
            .unwrap(),
            "value for {source}"
        );
    }
    for operator in ["expm1", "log1p", "log2", "log10", "pnorm", "dnorm"] {
        let expression = code(&mut session, &format!("quote({operator}(foo(x)))"));
        let args = arguments(&owner, &expression, &variable);
        let error = session.with_active(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                super::do_d(args.as_raw())
            }))
            .unwrap_err()
        });
        assert!(
            error
                .downcast::<crate::sexp::context::RError>()
                .unwrap()
                .message
                .contains("Function 'foo' is not in the derivatives table"),
            "{operator}"
        );
    }
}

#[test]
#[cfg(not(miri))]
fn owned_derivative_deep_pi_trig_traversal_subprocess() {
    const CHILD: &str = "RPORT_DERIV_DEEP_PI_TRIG_CHILD";
    if std::env::var_os(CHILD).is_some() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let session = RSession::new_for_gc_tests();
                let owner = session.owner_token().unwrap().weak_owner().unwrap();
                let y = symbol(&owner, "y");
                let mut expression = y.clone();
                let operators = ["sinpi", "cospi", "tanpi"];
                for index in 0..2048 {
                    expression = call(&owner, operators[index % operators.len()], &[expression]);
                }
                let variable = owner
                    .node_factory()
                    .unwrap()
                    .strings(&["x"])
                    .unwrap()
                    .into_owned()
                    .unwrap();
                let args = arguments(&owner, &expression, &variable);
                let result = session.with_active(|| unsafe {
                    session
                        .sexp(super::do_d(args.as_raw()))
                        .unwrap()
                        .into_owned()
                        .unwrap()
                });
                assert_eq!(result.try_real_elt(0).unwrap(), 0.);
            })
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("library::stats::deriv::owned_tests::owned_derivative_deep_pi_trig_traversal_subprocess")
        .args(["--exact", "--nocapture"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn owned_derivative_pi_trig_cycles_reject_and_recover() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let x = symbol(&owner, "x");
    let variable = owner
        .node_factory()
        .unwrap()
        .strings(&["x"])
        .unwrap()
        .into_owned()
        .unwrap();
    for operator in ["sinpi", "cospi", "tanpi"] {
        let expression = call(&owner, operator, std::slice::from_ref(&x));
        let first_cell = expression.try_cdr().unwrap().into_owned().unwrap();
        let mut writable = crate::sexp::object::SexpMut::try_from_checked(first_cell).unwrap();
        writable.try_set_pairlist_car(&expression).unwrap();
        drop(writable);
        let args = arguments(&owner, &expression, &variable);
        let error = session.with_active(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                super::do_d(args.as_raw())
            }))
            .unwrap_err()
        });
        assert!(
            error
                .downcast::<crate::sexp::context::RError>()
                .unwrap()
                .message
                .contains("cyclic derivative expression"),
            "{operator}"
        );
    }
    let args = arguments(&owner, &x, &variable);
    let result =
        session.with_active(|| unsafe { session.sexp(super::do_d(args.as_raw())).unwrap() });
    assert_eq!(result.try_real_elt(0).unwrap(), 1.);
}

#[test]
fn owned_derivative_pi_trig_frame_admission_denial_recovers() {
    let mut session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let y = symbol(&owner, "y");
    let mut expression = y.clone();
    for index in 0..64 {
        expression = call(
            &owner,
            ["sinpi", "cospi", "tanpi"][index % 3],
            &[expression],
        );
    }
    let variable = owner
        .node_factory()
        .unwrap()
        .strings(&["x"])
        .unwrap()
        .into_owned()
        .unwrap();
    let args = arguments(&owner, &expression, &variable);
    let preinstalled: Vec<_> = ["+", "-", "*", "/", "^", "pi", "sinpi", "cospi", "tanpi"]
        .into_iter()
        .map(|operator| symbol(&owner, operator))
        .collect();
    let prior = session.arena_budget();
    let used = crate::sexp::owner::with_runtime(&owner, |access| {
        access.with_arena(|arena| arena.total_bytes_allocated())
    })
    .unwrap()
    .unwrap();
    session.set_arena_budget(crate::sexp::memory::ArenaBudget::new(
        used + 8192,
        prior.max_nodes,
    ));
    let error = session.with_active(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::do_d(args.as_raw())
        }))
        .unwrap_err()
    });
    let message = error
        .downcast::<crate::sexp::context::RError>()
        .unwrap()
        .message;
    assert!(
        message.contains("cannot inspect derivative expression path")
            || message.contains("cannot retain derivative frames")
            || message.contains("session memory limit"),
        "{message}"
    );
    drop(preinstalled);
    session.set_arena_budget(prior);
    let args = arguments(&owner, &y, &variable);
    let result =
        session.with_active(|| unsafe { session.sexp(super::do_d(args.as_raw())).unwrap() });
    assert_eq!(result.try_real_elt(0).unwrap(), 0.);
}

#[test]
fn owned_derivative_sinpi_frame_gc_retains_generated_graph() {
    collecting_derivative(0, false, 16);
}

#[test]
fn owned_derivative_sinpi_frame_panic_recovers() {
    collecting_derivative(1, false, 16);
}

#[test]
fn owned_derivative_sinpi_frame_revocation_rejects_success() {
    collecting_derivative(2, false, 16);
}

#[test]
fn owned_derivative_cospi_frame_gc_retains_generated_graph() {
    collecting_derivative(0, false, 17);
}

#[test]
fn owned_derivative_cospi_frame_panic_recovers() {
    collecting_derivative(1, false, 17);
}

#[test]
fn owned_derivative_cospi_frame_revocation_rejects_success() {
    collecting_derivative(2, false, 17);
}

#[test]
fn owned_derivative_tanpi_frame_gc_retains_generated_graph() {
    collecting_derivative(0, false, 18);
}

#[test]
fn owned_derivative_tanpi_frame_panic_recovers() {
    collecting_derivative(1, false, 18);
}

#[test]
fn owned_derivative_tanpi_frame_revocation_rejects_success() {
    collecting_derivative(2, false, 18);
}

#[test]
#[cfg_attr(miri, ignore = "GNU pi-trig shape and numerical checks run natively")]
fn owned_derivative_pi_trig_shapes_and_order() {
    let mut session = RSession::new_without_default_packages();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    let variable = code(&mut session, "'x'");
    for (source, expected, value) in [
        ("sinpi(x)", "cospi(x) * pi", "cospi(0.3) * pi"),
        ("cospi(x)", "-(sinpi(x) * pi)", "-(sinpi(0.3) * pi)"),
        ("tanpi(x)", "pi/cospi(x)^2", "pi/cospi(0.3)^2"),
        (
            "sinpi(x^2)",
            "cospi(x^2) * (pi * (2 * x))",
            "cospi(0.3^2) * (pi * (2 * 0.3))",
        ),
        (
            "cospi(x^2)",
            "-(sinpi(x^2) * (pi * (2 * x)))",
            "-(sinpi(0.3^2) * (pi * (2 * 0.3)))",
        ),
        (
            "tanpi(x^2)",
            "pi * (2 * x)/cospi(x^2)^2",
            "pi * (2 * 0.3)/cospi(0.3^2)^2",
        ),
    ] {
        let expression = code(&mut session, &format!("quote({source})"));
        let args = arguments(&owner, &expression, &variable);
        let derivative = session.with_active(|| unsafe {
            session
                .sexp(super::do_d(args.as_raw()))
                .unwrap()
                .into_owned()
                .unwrap()
        });
        assert!(session.define_var("saved_derivative", derivative));
        assert!(
            code(
                &mut session,
                &format!("identical(saved_derivative, quote({expected}))")
            )
            .try_to_bool()
            .unwrap(),
            "shape for {source}"
        );
        assert!(
            code(
                &mut session,
                &format!("x <- 0.3; abs(eval(saved_derivative) - ({value})) < 1e-12")
            )
            .try_to_bool()
            .unwrap(),
            "value for {source}"
        );
    }
    for operator in ["sinpi", "cospi", "tanpi"] {
        let expression = code(&mut session, &format!("quote({operator}(foo(x)))"));
        let args = arguments(&owner, &expression, &variable);
        let error = session.with_active(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                super::do_d(args.as_raw())
            }))
            .unwrap_err()
        });
        assert!(
            error
                .downcast::<crate::sexp::context::RError>()
                .unwrap()
                .message
                .contains("Function 'foo' is not in the derivatives table"),
            "{operator}"
        );
    }
}
