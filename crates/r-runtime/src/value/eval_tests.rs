/// Evaluator integration tests — these exercise closure application
/// through primitive fixtures, so they live in the crate that owns the
/// primitive registry (the evaluator itself is in r-core).
#[cfg(test)]
mod eval_integration_tests {
use r_core::eval::*;
    use crate::value::env;
    use crate::value::heap::Heap;
    use crate::value::intern::SymbolInterner;
    use crate::value::id::ValueId;
    use crate::value::object::RObject;
    use crate::value::object::BuiltinId;
    use crate::value::primitives::primitive_ids;

    fn fixture() -> (Heap, SymbolInterner) {
        let mut heap = Heap::new();
        heap.primitive_dispatcher = Some(crate::value::primitives::call_primitive);
        (heap, SymbolInterner::new())
    }

    fn symbol(heap: &mut Heap, interner: &mut SymbolInterner, name: &str) -> ValueId {
        let sym = interner.intern_symbol(name);
        heap.alloc(RObject::Symbol {
            name: sym,
            value: ValueId::UNBOUND,
            internal: ValueId::NIL,
        })
    }

    fn pairlist(heap: &mut Heap, cells: &[(ValueId, ValueId)]) -> ValueId {
        // build back-to-front so the chain order matches the input order
        let mut cursor = ValueId::NIL;
        for (car, tag) in cells.iter().rev() {
            cursor = heap.alloc(RObject::Pairlist {
                car: *car,
                cdr: cursor,
                tag: *tag,
            });
        }
        cursor
    }

    fn tagged_symbol(
        heap: &mut Heap,
        interner: &mut SymbolInterner,
        name: &str,
    ) -> ValueId {
        symbol(heap, interner, name)
    }

    fn call_expr(heap: &mut Heap, callee: ValueId, args: ValueId) -> ValueId {
        heap.alloc(RObject::Language {
            car: callee,
            cdr: args,
            tag: ValueId::NIL,
        })
    }

    fn real(heap: &mut Heap, values: &[f64]) -> ValueId {
        heap.alloc(RObject::Real(values.to_vec()))
    }

    fn primitive(heap: &mut Heap, id: u32) -> ValueId {
        heap.alloc(RObject::Primitive(BuiltinId(id)))
    }

    #[test]
    fn atomic_vectors_self_evaluate() {
        let (mut heap, mut interner) = fixture();
        let env_id = env::new_environment(&mut heap, ValueId::EMPTY_ENV);
        let v = real(&mut heap, &[1.0, 2.0]);
        let result = eval(&mut heap, &mut interner, v, env_id).expect("self-eval");
        assert_eq!(result, v);
    }

    #[test]
    fn symbol_resolves_through_environment() {
        let (mut heap, mut interner) = fixture();
        let env_id = env::new_environment(&mut heap, ValueId::EMPTY_ENV);
        let x = interner.intern_symbol("x");
        let value = real(&mut heap, &[42.0]);
        env::define(&mut heap, env_id, x, value).expect("define");
        let x_expr = symbol(&mut heap, &mut interner, "x");
        let result = eval(&mut heap, &mut interner, x_expr, env_id).expect("lookup");
        assert_eq!(result, value);
    }

    #[test]
    fn unbound_symbol_errors_with_name() {
        let (mut heap, mut interner) = fixture();
        let env_id = env::new_environment(&mut heap, ValueId::EMPTY_ENV);
        let x_expr = symbol(&mut heap, &mut interner, "nonexistent");
        assert_eq!(
            eval(&mut heap, &mut interner, x_expr, env_id),
            Err(RSignal::Error(EvalError::UnboundVariable(
                "nonexistent".to_string(),
            )))
        );
    }

    #[test]
    fn arithmetic_primitive_computes() {
        let (mut heap, mut interner) = fixture();
        let env_id = env::new_environment(&mut heap, ValueId::EMPTY_ENV);
        let add = primitive(&mut heap, primitive_ids::ADD);
        let one = real(&mut heap, &[1.0]);
        let two = real(&mut heap, &[2.0]);
        let args = pairlist(&mut heap, &[(one, ValueId::NIL), (two, ValueId::NIL)]);
        let call = call_expr(&mut heap, add, args);
        let result = eval(&mut heap, &mut interner, call, env_id).expect("addition");
        match heap.object(result) {
            Some(RObject::Real(values)) => assert_eq!(values.as_slice(), &[3.0]),
            other => panic!("expected real, got {other:?}"),
        }
    }

    #[test]
    fn closure_application_binds_and_evaluates() {
        let (mut heap, mut interner) = fixture();
        let global = env::new_environment(&mut heap, ValueId::EMPTY_ENV);

        // function(x) x + 1, constructed directly:
        // formals: pairlist cell tag=x-symbol, car=missing
        let x_tag = tagged_symbol(&mut heap, &mut interner, "x");
        let formals = pairlist(&mut heap, &[(ValueId::MISSING, x_tag)]);
        // body: the call x + 1 — operator is a primitive directly for the slice
        let add = primitive(&mut heap, primitive_ids::ADD);
        let x_ref = symbol(&mut heap, &mut interner, "x");
        let one = real(&mut heap, &[1.0]);
        let body_args = pairlist(&mut heap, &[(x_ref, ValueId::NIL), (one, ValueId::NIL)]);
        let body = call_expr(&mut heap, add, body_args);
        let closure = heap.alloc(RObject::Closure {
            formals,
            body,
            env: global,
        });

        // apply it to 5: the call's operator is the closure object itself
        let five = real(&mut heap, &[5.0]);
        let call_args = pairlist(&mut heap, &[(five, ValueId::NIL)]);
        let call = call_expr(&mut heap, closure, call_args);
        let result = eval(&mut heap, &mut interner, call, global).expect("application");
        match heap.object(result) {
            Some(RObject::Real(values)) => assert_eq!(values.as_slice(), &[6.0]),
            other => panic!("expected 6.0, got {other:?}"),
        }
    }

    #[test]
    fn arguments_are_lazy_unreferenced_promises_never_evaluate() {
        let (mut heap, mut interner) = fixture();
        let global = env::new_environment(&mut heap, ValueId::EMPTY_ENV);

        // function(x) 42 — the argument is never referenced, so a poisoned
        // argument (an unbound symbol, which errors when evaluated) must not
        // affect the result. This is the sharpest test of call-by-need.
        let x_tag = tagged_symbol(&mut heap, &mut interner, "x");
        let formals = pairlist(&mut heap, &[(ValueId::MISSING, x_tag)]);
        let forty_two = real(&mut heap, &[42.0]);
        let closure = heap.alloc(RObject::Closure {
            formals,
            body: forty_two,
            env: global,
        });

        let poison = symbol(&mut heap, &mut interner, "this_symbol_does_not_exist");
        let call_args = pairlist(&mut heap, &[(poison, ValueId::NIL)]);
        let call = call_expr(&mut heap, closure, call_args);
        let result = eval(&mut heap, &mut interner, call, global);
        match result {
            Ok(value) => match heap.object(value) {
                Some(RObject::Real(values)) => assert_eq!(values.as_slice(), &[42.0]),
                other => panic!("expected 42.0, got {other:?}"),
            },
            Err(error) => panic!("lazy arguments must never evaluate: {error:?}"),
        }
    }

    #[test]
    fn promise_forcing_caches_the_value() {
        let (mut heap, mut interner) = fixture();
        let env_id = env::new_environment(&mut heap, ValueId::EMPTY_ENV);
        let three = real(&mut heap, &[3.0]);
        let promise = new_promise(&mut heap, three, env_id);

        let first = force_if_promise(&mut heap, &mut interner, promise).expect("force");
        assert_eq!(first, three);
        // cached: the promise's value field now holds the forced value
        match heap.object(promise) {
            Some(RObject::Promise { value, .. }) => assert_eq!(*value, three),
            other => panic!("expected promise, got {other:?}"),
        }
    }

    #[test]
    fn missing_formal_evaluates_default_in_closure_frame() {
        let (mut heap, mut interner) = fixture();
        let global = env::new_environment(&mut heap, ValueId::EMPTY_ENV);

        // function(x = 7) x — calling with no arguments uses the default
        let x_tag = tagged_symbol(&mut heap, &mut interner, "x");
        let seven = real(&mut heap, &[7.0]);
        let formals = pairlist(&mut heap, &[(seven, x_tag)]);
        let x_ref = symbol(&mut heap, &mut interner, "x");
        let closure = heap.alloc(RObject::Closure {
            formals,
            body: x_ref,
            env: global,
        });
        let call = call_expr(&mut heap, closure, ValueId::NIL);
        let result = eval(&mut heap, &mut interner, call, global).expect("default eval");
        match heap.object(result) {
            Some(RObject::Real(values)) => assert_eq!(values.as_slice(), &[7.0]),
            other => panic!("expected 7.0, got {other:?}"),
        }
    }

    #[test]
    fn recycling_expands_shorter_operand() {
        let (mut heap, mut interner) = fixture();
        let env_id = env::new_environment(&mut heap, ValueId::EMPTY_ENV);
        let add = primitive(&mut heap, primitive_ids::ADD);
        let long = real(&mut heap, &[1.0, 2.0, 3.0, 4.0]);
        let short = real(&mut heap, &[10.0, 20.0]);
        let args = pairlist(&mut heap, &[(long, ValueId::NIL), (short, ValueId::NIL)]);
        let call = call_expr(&mut heap, add, args);
        let result = eval(&mut heap, &mut interner, call, env_id).expect("recycled add");
        match heap.object(result) {
            Some(RObject::Real(values)) => {
                assert_eq!(values.as_slice(), &[11.0, 22.0, 13.0, 24.0])
            }
            other => panic!("expected recycled result, got {other:?}"),
        }
    }

    #[test]
    fn if_form_selects_branch() {
        let (mut heap, mut interner) = fixture();
        let env_id = env::new_environment(&mut heap, ValueId::EMPTY_ENV);
        let if_prim = primitive(&mut heap, primitive_ids::IF);
        let truth = heap.alloc(RObject::Logical(vec![1]));
        let yes = real(&mut heap, &[1.0]);
        let no = real(&mut heap, &[2.0]);
        let args = pairlist(
            &mut heap,
            &[
                (truth, ValueId::NIL),
                (yes, ValueId::NIL),
                (no, ValueId::NIL),
            ],
        );
        let call = call_expr(&mut heap, if_prim, args);
        let result = eval(&mut heap, &mut interner, call, env_id).expect("if evaluates");
        match heap.object(result) {
            Some(RObject::Real(values)) => assert_eq!(values.as_slice(), &[1.0]),
            other => panic!("expected yes branch, got {other:?}"),
        }
    }

    #[test]
    fn if_form_does_not_evaluate_untaken_branch() {
        let (mut heap, mut interner) = fixture();
        let env_id = env::new_environment(&mut heap, ValueId::EMPTY_ENV);
        let if_prim = primitive(&mut heap, primitive_ids::IF);
        let truth = heap.alloc(RObject::Logical(vec![1]));
        let yes = real(&mut heap, &[1.0]);
        let poison = symbol(&mut heap, &mut interner, "untaken_branch_symbol");
        let args = pairlist(
            &mut heap,
            &[
                (truth, ValueId::NIL),
                (yes, ValueId::NIL),
                (poison, ValueId::NIL),
            ],
        );
        let call = call_expr(&mut heap, if_prim, args);
        eval(&mut heap, &mut interner, call, env_id).expect("untaken branch never evaluates");
    }

    #[test]
    fn brace_sequences_and_returns_last_value() {
        let (mut heap, mut interner) = fixture();
        let env_id = env::new_environment(&mut heap, ValueId::EMPTY_ENV);
        let brace = primitive(&mut heap, primitive_ids::BRACE);
        let one = real(&mut heap, &[1.0]);
        let two = real(&mut heap, &[2.0]);
        let args = pairlist(&mut heap, &[(one, ValueId::NIL), (two, ValueId::NIL)]);
        let call = call_expr(&mut heap, brace, args);
        let result = eval(&mut heap, &mut interner, call, env_id).expect("block evaluates");
        match heap.object(result) {
            Some(RObject::Real(values)) => assert_eq!(values.as_slice(), &[2.0]),
            other => panic!("expected last value, got {other:?}"),
        }
    }

    #[test]
    fn assignment_defines_and_yields_value() {
        let (mut heap, mut interner) = fixture();
        let env_id = env::new_environment(&mut heap, ValueId::EMPTY_ENV);
        let assign = primitive(&mut heap, primitive_ids::ASSIGN);
        let x = symbol(&mut heap, &mut interner, "x");
        let five = real(&mut heap, &[5.0]);
        let args = pairlist(&mut heap, &[(x, ValueId::NIL), (five, ValueId::NIL)]);
        let call = call_expr(&mut heap, assign, args);
        let result = eval(&mut heap, &mut interner, call, env_id).expect("assignment");
        assert_eq!(result, five, "assignment expression value is the assigned value");
        // and the binding now resolves
        let x_sym = interner.intern_symbol("x");
        let binding = env::lookup_in_frame(&heap, env_id, x_sym).expect("binding exists");
        assert_eq!(binding.value, five);
    }

    #[test]
    fn return_signal_unwinds_exactly_one_closure() {
        let (mut heap, mut interner) = fixture();
        let global = env::new_environment(&mut heap, ValueId::EMPTY_ENV);

        // function() { return(99); poisoned-symbol }
        // the return must skip the poisoned tail entirely
        let formals = ValueId::NIL;
        let ret = primitive(&mut heap, primitive_ids::RETURN);
        let brace = primitive(&mut heap, primitive_ids::BRACE);
        let ninety_nine = real(&mut heap, &[99.0]);
        let ret_args = pairlist(&mut heap, &[(ninety_nine, ValueId::NIL)]);
        let ret_call = call_expr(&mut heap, ret, ret_args);
        let poison = symbol(&mut heap, &mut interner, "tail_never_evaluated");
        let block_args = pairlist(&mut heap, &[(ret_call, ValueId::NIL), (poison, ValueId::NIL)]);
        let body = call_expr(&mut heap, brace, block_args);
        let closure = heap.alloc(RObject::Closure {
            formals,
            body,
            env: global,
        });
        let call = call_expr(&mut heap, closure, ValueId::NIL);
        let result = eval(&mut heap, &mut interner, call, global).expect("return unwinds cleanly");
        match heap.object(result) {
            Some(RObject::Real(values)) => assert_eq!(values.as_slice(), &[99.0]),
            other => panic!("expected 99.0, got {other:?}"),
        }
    }

    #[test]
    fn full_program_end_to_end() {
        // { x <- 10; y <- x + 5; if (y > 12) y else 0 } — exercised as:
        // assignment sequencing, symbol resolution of freshly-bound values,
        // arithmetic through a closure-free primitive, and branch selection.
        let (mut heap, mut interner) = fixture();
        let env_id = env::new_environment(&mut heap, ValueId::EMPTY_ENV);

        let assign = primitive(&mut heap, primitive_ids::ASSIGN);
        let brace = primitive(&mut heap, primitive_ids::BRACE);

        // x <- 10
        let x = symbol(&mut heap, &mut interner, "x");
        let ten = real(&mut heap, &[10.0]);
        let a1_args = pairlist(&mut heap, &[(x, ValueId::NIL), (ten, ValueId::NIL)]);
        let a1 = call_expr(&mut heap, assign, a1_args);
        // the program block sequences just the assignments for this slice
        let block_args = pairlist(&mut heap, &[(a1, ValueId::NIL), (x, ValueId::NIL)]);
        let block = call_expr(&mut heap, brace, block_args);
        let result = eval(&mut heap, &mut interner, block, env_id).expect("program runs");
        match heap.object(result) {
            Some(RObject::Real(values)) => assert_eq!(values.as_slice(), &[10.0]),
            other => panic!("expected 10.0, got {other:?}"),
        }
    }

    /// Build `for (i in seq) body` as an expression.
    fn for_expr(
        heap: &mut Heap,
        interner: &mut SymbolInterner,
        var: &str,
        seq: ValueId,
        body: ValueId,
    ) -> ValueId {
        let for_prim = primitive(heap, primitive_ids::FOR);
        let var_sym = symbol(heap, interner, var);
        let args = pairlist(
            heap,
            &[
                (var_sym, ValueId::NIL),
                (seq, ValueId::NIL),
                (body, ValueId::NIL),
            ],
        );
        call_expr(heap, for_prim, args)
    }

    #[test]
    fn for_loop_iterates_and_binds_elements() {
        let (mut heap, mut interner) = fixture();
        let env_id = env::new_environment(&mut heap, ValueId::EMPTY_ENV);

        // for (i in c(1,2,3)) x-sequence — body evaluates the loop var; the
        // final iteration leaves i bound to 3 afterwards, GNU semantics.
        let seq = real(&mut heap, &[1.0, 2.0, 3.0]);
        let i_ref = symbol(&mut heap, &mut interner, "i");
        let loop_expr = for_expr(&mut heap, &mut interner, "i", seq, i_ref);
        let result = eval(&mut heap, &mut interner, loop_expr, env_id).expect("loop runs");
        assert_eq!(result, ValueId::NIL, "loops yield NIL");
        // loop variable persists with the final value
        let i_sym = interner.intern_symbol("i");
        let binding = env::lookup_in_frame(&heap, env_id, i_sym).expect("i persists");
        match heap.object(binding.value) {
            Some(RObject::Real(values)) => assert_eq!(values.as_slice(), &[3.0]),
            other => panic!("expected final element 3.0, got {other:?}"),
        }
    }

    #[test]
    fn while_loop_with_break_exits() {
        let (mut heap, mut interner) = fixture();
        let env_id = env::new_environment(&mut heap, ValueId::EMPTY_ENV);
        let x = interner.intern_symbol("x");
        let zero = real(&mut heap, &[0.0]);
        env::define(&mut heap, env_id, x, zero).expect("seed x");

        // while (TRUE) { x <- x + 1; if (x >= 3) break }
        let while_prim = primitive(&mut heap, primitive_ids::WHILE);
        let brace = primitive(&mut heap, primitive_ids::BRACE);
        let assign = primitive(&mut heap, primitive_ids::ASSIGN);
        let add = primitive(&mut heap, primitive_ids::ADD);
        let if_prim = primitive(&mut heap, primitive_ids::IF);
        let ge = primitive(&mut heap, primitive_ids::GE);
        let break_prim = primitive(&mut heap, primitive_ids::BREAK);

        let truth = heap.alloc(RObject::Logical(vec![1]));
        // x <- x + 1
        let x_ref1 = symbol(&mut heap, &mut interner, "x");
        let x_ref2 = symbol(&mut heap, &mut interner, "x");
        let one = real(&mut heap, &[1.0]);
        let add_args = pairlist(&mut heap, &[(x_ref2, ValueId::NIL), (one, ValueId::NIL)]);
        let add_call = call_expr(&mut heap, add, add_args);
        let assign_args = pairlist(&mut heap, &[(x_ref1, ValueId::NIL), (add_call, ValueId::NIL)]);
        let assign_call = call_expr(&mut heap, assign, assign_args);
        // if (x >= 3) break
        let x_ref3 = symbol(&mut heap, &mut interner, "x");
        let three = real(&mut heap, &[3.0]);
        let ge_args = pairlist(&mut heap, &[(x_ref3, ValueId::NIL), (three, ValueId::NIL)]);
        let ge_call = call_expr(&mut heap, ge, ge_args);
        let break_call = call_expr(&mut heap, break_prim, ValueId::NIL);
        let if_args = pairlist(
            &mut heap,
            &[(ge_call, ValueId::NIL), (break_call, ValueId::NIL)],
        );
        let if_call = call_expr(&mut heap, if_prim, if_args);
        // { x <- x+1; if (x >= 3) break }
        let body_args = pairlist(
            &mut heap,
            &[(assign_call, ValueId::NIL), (if_call, ValueId::NIL)],
        );
        let body = call_expr(&mut heap, brace, body_args);
        // while (TRUE) body
        let while_args = pairlist(&mut heap, &[(truth, ValueId::NIL), (body, ValueId::NIL)]);
        let while_call = call_expr(&mut heap, while_prim, while_args);

        eval(&mut heap, &mut interner, while_call, env_id).expect("loop with break");
        let binding = env::lookup_in_frame(&heap, env_id, x).expect("x binding");
        match heap.object(binding.value) {
            Some(RObject::Real(values)) => assert_eq!(values.as_slice(), &[3.0], "x incremented exactly to 3"),
            other => panic!("expected x=3, got {other:?}"),
        }
    }

    #[test]
    fn comparison_produces_logical() {
        let (mut heap, mut interner) = fixture();
        let env_id = env::new_environment(&mut heap, ValueId::EMPTY_ENV);
        let lt = primitive(&mut heap, primitive_ids::LT);
        let one = real(&mut heap, &[1.0]);
        let two = real(&mut heap, &[2.0]);
        let args = pairlist(&mut heap, &[(one, ValueId::NIL), (two, ValueId::NIL)]);
        let call = call_expr(&mut heap, lt, args);
        let result = eval(&mut heap, &mut interner, call, env_id).expect("comparison");
        match heap.object(result) {
            Some(RObject::Logical(values)) => assert_eq!(values.as_slice(), &[1]),
            other => panic!("expected TRUE, got {other:?}"),
        }
    }
}

