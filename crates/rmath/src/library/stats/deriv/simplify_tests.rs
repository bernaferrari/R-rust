use super::*;
use crate::sexp::RSession;

#[test]
#[cfg(not(miri))]
fn deep_algebraic_simplifier_subprocess() {
    const CHILD: &str = "RPORT_DEEP_ALGEBRAIC_SIMPLIFIER_CHILD";
    if std::env::var_os(CHILD).is_some() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let session = RSession::new_for_gc_tests();
                let owner = session.owner_token().unwrap().weak_owner().unwrap();
                crate::sexp::owner::with_runtime(&owner, |access| {
                    let kernel = Kernel::new(access, super::super::derivative_symbol);
                    let x = kernel.symbol("x")?;
                    let y = kernel.symbol("y")?;
                    let mut nested = x.clone();
                    for _ in 0..4096 {
                        nested = kernel.call("-", &[nested])?;
                    }
                    for op in ["+", "-", "*", "/"] {
                        for (first, second) in [(&nested, &y), (&y, &nested)] {
                            let result =
                                kernel.simplify(op, first.clone(), Some(second.clone()))?;
                            assert!(result.typeof_() == SEXPTYPE::LANGSXP);
                        }
                    }
                    Ok::<_, SexpError>(())
                })
                .unwrap()
                .unwrap();
            })
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("library::stats::deriv::owned::simplify_tests::deep_algebraic_simplifier_subprocess")
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

fn shape(access: &RuntimeAccess, value: &Value) -> SexpResult<String> {
    if value.typeof_() == SEXPTYPE::SYMSXP {
        return name(value);
    }
    let call = snapshot(access, value)?;
    let mut result = format!("({}", name(&call.head)?);
    for argument in &call.arguments {
        result.push(' ');
        result.push_str(&shape(access, argument)?);
    }
    result.push(')');
    Ok(result)
}

#[test]
fn algebraic_simplifier_preserves_shape_and_constructor_order() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    crate::sexp::owner::with_runtime(&owner, |access| {
        let kernel = Kernel::new(access, super::super::derivative_symbol);
        let x = kernel.symbol("x")?;
        let y = kernel.symbol("y")?;
        for (operator, left_depth, right_depth, expected, events) in [
            ("+", 1, 0, "(- y x)", vec!["-"]),
            ("+", 0, 1, "(- x y)", vec!["-"]),
            ("-", 1, 0, "(- (+ x y))", vec!["+", "-"]),
            ("-", 0, 1, "(+ x y)", vec!["+"]),
            ("*", 1, 0, "(- (* x y))", vec!["*", "-"]),
            ("/", 0, 1, "(- (/ x y))", vec!["/", "-"]),
            ("*", 2, 0, "(* x y)", vec!["*", "-"]),
            ("/", 0, 2, "(/ x y)", vec!["/", "-"]),
            ("+", 2, 0, "(+ y x)", vec!["+"]),
            ("-", 2, 0, "(- (- y x))", vec!["-", "-"]),
            ("+", 1, 1, "(- (+ y x))", vec!["+", "-"]),
            ("-", 1, 1, "(- (- x y))", vec!["-", "-"]),
        ] {
            let mut left = x.clone();
            let mut right = y.clone();
            for _ in 0..left_depth {
                left = kernel.call("-", &[left])?;
            }
            for _ in 0..right_depth {
                right = kernel.call("-", &[right])?;
            }
            let actual_events = RefCell::new(Vec::new());
            let checked = Kernel::new(access, |access, name| {
                actual_events.borrow_mut().push(name.to_owned());
                access.with_native(|runtime| runtime.full_gc())?;
                super::super::derivative_symbol(access, name)
            });
            let result = checked.simplify(operator, left, Some(right))?;
            assert_eq!(shape(access, &result)?, expected);
            assert_eq!(*actual_events.borrow(), events);
        }
        Ok::<_, SexpError>(())
    })
    .unwrap()
    .unwrap();
}

#[test]
fn algebraic_simplifier_continuation_error_and_panic_recover() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    crate::sexp::owner::with_runtime(&owner, |access| {
        let kernel = Kernel::new(access, super::super::derivative_symbol);
        let x = kernel.symbol("x")?;
        let y = kernel.symbol("y")?;
        let negative = kernel.call("-", &[x])?;
        for panic in [false, true] {
            let events = RefCell::new(Vec::new());
            let checked = Kernel::new(access, |access, name| {
                events.borrow_mut().push(name.to_owned());
                access.with_native(|runtime| runtime.full_gc())?;
                if name == "-" {
                    if panic {
                        std::panic::panic_any(941_u32);
                    }
                    return Err(invalid("simplifier continuation marker"));
                }
                super::super::derivative_symbol(access, name)
            });
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                checked.simplify("*", negative.clone(), Some(y.clone()))
            }));
            if panic {
                assert_eq!(*result.unwrap_err().downcast::<u32>().unwrap(), 941);
            } else {
                assert!(
                    result
                        .unwrap()
                        .unwrap_err()
                        .to_string()
                        .contains("simplifier continuation marker")
                );
            }
            assert_eq!(*events.borrow(), ["*", "-"]);
            assert_eq!(
                shape(
                    access,
                    &kernel.simplify("*", negative.clone(), Some(y.clone()))?
                )?,
                "(- (* x y))"
            );
        }
        Ok::<_, SexpError>(())
    })
    .unwrap()
    .unwrap();
}

#[test]
fn algebraic_simplifier_cycles_reject_and_recover() {
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    crate::sexp::owner::with_runtime(&owner, |access| {
        let kernel = Kernel::new(access, super::super::derivative_symbol);
        let x = kernel.symbol("x")?;
        let y = kernel.symbol("y")?;
        let self_cycle = kernel.call("-", std::slice::from_ref(&x))?;
        let two_cycle = kernel.call("-", std::slice::from_ref(&x))?;
        let second = kernel.call("-", std::slice::from_ref(&two_cycle))?;
        for (parent, child) in [(&self_cycle, &self_cycle), (&two_cycle, &second)] {
            let cell = parent.try_cdr()?.into_owned()?;
            let mut writable = crate::sexp::object::SexpMut::try_from_checked(cell)?;
            writable.try_set_pairlist_car(child)?;
        }
        for cycle in [self_cycle, two_cycle] {
            for operator in ["+", "-", "*", "/"] {
                for (left, right) in [(&cycle, &y), (&y, &cycle)] {
                    let error = kernel
                        .simplify(operator, left.clone(), Some(right.clone()))
                        .unwrap_err();
                    assert!(error.to_string().contains("cyclic derivative expression"));
                }
            }
            // The product's first term is simplified before D(right), so the
            // derivative traversal's active path cannot detect this cycle.
            let square = kernel.call("^", &[x.clone(), kernel.scalar(2.)?])?;
            let product = kernel.call("*", &[square, cycle])?;
            let error = kernel.derive(&product, &x).unwrap_err();
            assert!(error.to_string().contains("cyclic derivative expression"));
            assert!(kernel.active.borrow().is_empty());
        }
        assert_eq!(
            shape(access, &kernel.simplify("*", x.clone(), Some(y))?)?,
            "(* x y)"
        );
        assert_eq!(kernel.derive(&x, &x)?.try_real_elt(0)?, 1.);
        Ok::<_, SexpError>(())
    })
    .unwrap()
    .unwrap();
}

#[test]
fn algebraic_simplifier_snapshot_denial_releases_workspace_and_retries() {
    use crate::sexp::memory::ArenaBudget;
    let session = RSession::new_for_gc_tests();
    let owner = session.owner_token().unwrap().weak_owner().unwrap();
    crate::sexp::owner::with_runtime(&owner, |access| {
        let kernel = Kernel::new(access, super::super::derivative_symbol);
        let x = kernel.symbol("x")?;
        let y = kernel.symbol("y")?;
        let negative = kernel.call("-", &[x])?;
        let held = access.with_arena(|arena| {
            let held = arena.workspace_bytes();
            arena.set_budget(ArenaBudget::new(arena.total_bytes_allocated(), 0));
            held
        })?;
        let error = kernel
            .simplify("*", negative.clone(), Some(y.clone()))
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("cannot retain derivative arguments"),
            "{error}"
        );
        assert_eq!(access.with_arena(|arena| arena.workspace_bytes())?, held);
        access.with_arena(|arena| arena.set_budget(ArenaBudget::unlimited()))?;
        assert_eq!(
            shape(access, &kernel.simplify("*", negative, Some(y))?)?,
            "(- (* x y))"
        );
        assert_eq!(access.with_arena(|arena| arena.workspace_bytes())?, held);
        Ok::<_, SexpError>(())
    })
    .unwrap()
    .unwrap();
}

#[test]
fn algebraic_simplifier_continuation_revocation_rejects_publication() {
    let facade = RefCell::new(Some(RSession::new_for_gc_tests()));
    let owner = facade
        .borrow()
        .as_ref()
        .unwrap()
        .owner_token()
        .unwrap()
        .weak_owner()
        .unwrap();
    let result = crate::sexp::owner::with_runtime(&owner, |access| {
        let kernel = Kernel::new(access, super::super::derivative_symbol);
        let x = kernel.symbol("x")?;
        let y = kernel.symbol("y")?;
        let negative = kernel.call("-", &[x])?;
        let events = RefCell::new(Vec::new());
        let checked = Kernel::new(access, |access, name| {
            events.borrow_mut().push(name.to_owned());
            access.with_native(|runtime| runtime.full_gc())?;
            let symbol = super::super::derivative_symbol(access, name)?;
            if name == "-" {
                facade.borrow_mut().as_mut().unwrap().close();
            }
            Ok(symbol)
        });
        let result = checked.simplify("*", negative, Some(y));
        assert_eq!(*events.borrow(), ["*", "-"]);
        result
    });
    assert!(matches!(
        result,
        Err(SexpError::RootUnavailable) | Ok(Err(SexpError::RootUnavailable))
    ));
}
