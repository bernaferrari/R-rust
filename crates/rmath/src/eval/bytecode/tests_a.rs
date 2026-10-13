//! bytecode tests (part 1/2) — extracted verbatim from eval/bytecode.rs

use super::tests_util::*;
use super::*;
use crate::sexp::session::RSession;

#[test]
fn owned_private_bytecode_boxed_for_keeps_and_releases_its_only_values() {
    let session = RSession::new_for_gc_tests();
    let (variable, sequence) = session.with_active(|| {
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        crate::sexp::owner::with_runtime(&owner, |access| {
            let domain = access.domain();
            let allocator = access.allocator(&domain).unwrap();
            let variable = allocator
                .allocate(|arena| Some(arena.alloc_node(SEXPTYPE::SYMSXP)))
                .unwrap();
            let sequence = allocator
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::INTSXP, 1)))
                .unwrap();
            (variable, sequence)
        })
        .unwrap()
    });
    let variable_pointer = variable.as_raw();
    let sequence_pointer = sequence.as_raw();
    let frame = PrivateLoop {
        origin: 0,
        end: 1,
        kind: PrivateLoopKind::For(Box::new(PrivateFor {
            variable,
            sequence,
            body: 0,
            length: 1,
            next: 0,
        })),
    };
    let collect = || session.with_active(|| session.owner_token().unwrap().full_gc().unwrap());
    collect();
    assert!(session.sexp(variable_pointer).is_some());
    assert_eq!(
        session.sexp(sequence_pointer).unwrap().integer_elt(0),
        Some(0)
    );
    drop(frame);
    collect();
    assert!(session.sexp(variable_pointer).is_none());
    assert!(session.sexp(sequence_pointer).is_none());
}

#[test]
fn eval_bytecode_rejects_truncated_operands() {
    let session = RSession::new_for_gc_tests();
    let err = run_private(&session, &[BCpush], &[]).unwrap_err();
    assert!(err.contains("push bytecode operand is truncated"));
}
#[test]
fn eval_bytecode_rejects_negative_constant_indices() {
    let session = RSession::new_for_gc_tests();
    let err = run_private(&session, &[BCpush, -1], &[]).unwrap_err();
    assert!(err.contains("push bytecode operand index -1 is negative"));
}
#[test]
fn eval_bytecode_rejects_out_of_range_jump_targets() {
    let session = RSession::new_for_gc_tests();
    let err = run_private(&session, &[BCjump, 99], &[]).unwrap_err();
    assert!(err.contains("jump bytecode jump target 99 is outside"));
}

#[test]
fn owned_private_bytecode_stack_keeps_scalars_and_detached_constants_across_full_gc() {
    let session = RSession::new_for_gc_tests();
    let code = {
        let seven = fixture_integer(&session, 7);
        let six = fixture_integer(&session, 6);
        private_code(
            &session,
            &[
                BCpush, 0, BCpush, 1, BCadd, BCpush, 0, BCpush, 1, BCmul, BCadd, BCreturn,
            ],
            &[seven, six],
        )
    };
    let callbacks = collect_and_detach_pool(&session, &code);
    let result = session
        .with_active(|| eval_bytecode(code, session.global_env().unwrap().into_owned().unwrap()))
        .unwrap();
    assert!(
        callbacks.get() >= 3,
        "scalar publication must actually collect"
    );
    session.with_active(|| session.owner_token().unwrap().full_gc().unwrap());
    assert_eq!(result.integer_elt(0), Some(55));
}

#[test]
fn owned_private_bytecode_calls_keep_argument_cells_across_collecting_closure_builtin_and_special()
{
    // Core primitive resolution and closure evaluation suffice; loading
    // the full base library is unrelated to this ownership regression.
    let mut session = RSession::new_for_gc_tests();
    let (closure, _, _) = session.eval_code_with_output_capture("function(x, y) { gc(); x + y }");
    let closure = closure.unwrap().into_owned().unwrap();
    assert_eq!(closure.typeof_(), SEXPTYPE::CLOSXP);
    let code = {
        let x = fixture_integer(&session, 17);
        let y = fixture_integer(&session, 29);
        private_code(
            &session,
            &[BCpush, 1, BCpush, 2, BCcall, 0, 2, BCreturn],
            &[closure, x, y],
        )
    };
    let callbacks = collect_and_detach_pool(&session, &code);
    let result = session
        .with_active(|| eval_bytecode(code, session.global_env().unwrap().into_owned().unwrap()))
        .unwrap();
    assert!(callbacks.get() >= 2);
    assert_eq!(result.integer_elt(0), Some(46));

    let (list, _, _) = session.eval_code_with_output_capture("list");
    let list = list.unwrap().into_owned().unwrap();
    assert_eq!(list.typeof_(), SEXPTYPE::BUILTINSXP);
    let code = {
        let x = fixture_integer(&session, 17);
        let y = fixture_integer(&session, 29);
        private_code(
            &session,
            &[BCpush, 1, BCpush, 2, BCbuiltin, 0, 2, BCreturn],
            &[list, x, y],
        )
    };
    let callbacks = collect_and_detach_pool(&session, &code);
    let result = session
        .with_active(|| eval_bytecode(code, session.global_env().unwrap().into_owned().unwrap()))
        .unwrap();
    assert!(callbacks.get() >= 3);
    assert_eq!(result.try_vector_elt(0).unwrap().integer_elt(0), Some(17));
    assert_eq!(result.try_vector_elt(1).unwrap().integer_elt(0), Some(29));

    let (quote, _, _) = session.eval_code_with_output_capture("quote");
    let quote = quote.unwrap().into_owned().unwrap();
    assert_eq!(quote.typeof_(), SEXPTYPE::SPECIALSXP);
    let (symbol, _, _) = session.eval_code_with_output_capture("quote(saved_private_argument)");
    let symbol = symbol.unwrap().into_owned().unwrap();
    let expected_symbol = symbol.as_raw();
    let code = private_code(
        &session,
        &[BCpush, 1, BCspecial, 0, 1, BCreturn],
        &[quote, symbol],
    );
    let callbacks = collect_and_detach_pool(&session, &code);
    let result = session
        .with_active(|| eval_bytecode(code, session.global_env().unwrap().into_owned().unwrap()))
        .unwrap();
    assert!(callbacks.get() >= 2);
    assert_eq!(result.typeof_(), SEXPTYPE::SYMSXP);
    assert_eq!(result.as_raw(), expected_symbol);
}

#[test]
fn owned_private_bytecode_rejects_argument_underflow_and_handles_integer_edges_without_panics() {
    let session = RSession::new_for_gc_tests();
    let nil = session.with_active(|| {
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        crate::sexp::owner::with_runtime(&owner, |access| access.domain().nil()).unwrap()
    });
    for opcode in [BCcall, BCbuiltin, BCspecial] {
        let error = run_private(
            &session,
            &[opcode, 0, c_int::MAX, BCreturn],
            std::slice::from_ref(&nil),
        )
        .unwrap_err();
        assert!(error.contains("argument stack underflow"), "{error}");
    }
    let min = fixture_integer(&session, c_int::MIN);
    let minus_one = fixture_integer(&session, -1);
    let zero = fixture_integer(&session, 0);
    let result = run_private(
        &session,
        &[BCpush, 0, BCneg, BCreturn],
        std::slice::from_ref(&min),
    )
    .unwrap();
    assert_eq!(result.integer_elt(0), Some(c_int::MIN));
    let result = run_private(
        &session,
        &[BCpush, 0, BCpush, 1, BCmod, BCreturn],
        &[min, minus_one.clone()],
    )
    .unwrap();
    assert_eq!(result.integer_elt(0), Some(c_int::MIN));
    let result = run_private(
        &session,
        &[BCpush, 0, BCpush, 1, BCdiv, BCreturn],
        &[minus_one, zero],
    )
    .unwrap();
    assert_eq!(result.real_elt(0), Some(f64::NEG_INFINITY));
}

#[test]
fn owned_private_bytecode_reuses_nested_loop_origins_after_next_and_break_gc() {
    for words in [
        // A next from the inner while's condition propagates to the for.
        vec![
            BCfor, 0, 1, 5, 11, BCwhile, 9, 10, 11, BCnext, BCbreak, BCgvar, 0, BCreturn,
        ],
        // An inner repeat's break completes that loop normally, allowing
        // the enclosing for to re-enter the same repeat opcode each time.
        vec![
            BCfor, 0, 1, 5, 9, BCrepeat, 8, 9, BCbreak, BCgvar, 0, BCreturn,
        ],
    ] {
        let session = RSession::new_for_gc_tests();
        let (variable, sequence) = session.with_active(|| {
            let owner = session.owner_token().unwrap().weak_owner().unwrap();
            crate::sexp::owner::with_runtime(&owner, |access| {
                let domain = access.domain();
                let variable = access
                    .with_native(|_| {
                        domain.wrap(unsafe {
                            crate::sexp::symbol::Rf_install(c"private_loop_reuse".as_ptr())
                        })
                    })
                    .unwrap();
                let sequence = access
                    .allocator(&domain)
                    .unwrap()
                    .allocate(|arena| {
                        let pointer = arena.alloc_vector(SEXPTYPE::INTSXP, 3);
                        let heap = arena.heap_identity();
                        let node = arena.node_token(pointer)?;
                        let payload = heap.payload_lease(&node)?;
                        for (index, value) in [17, 23, 29].into_iter().enumerate() {
                            payload.set_integer_elt(index, value)?;
                        }
                        Some(pointer)
                    })
                    .unwrap();
                (variable, sequence)
            })
            .unwrap()
        });
        let code = private_code(&session, &words, &[variable, sequence]);
        let callbacks = collect_and_detach_pool(&session, &code);
        let result = session
            .with_active(|| {
                eval_bytecode(code, session.global_env().unwrap().into_owned().unwrap())
            })
            .unwrap();
        assert_eq!(result.integer_elt(0), Some(29));
        assert!(
            callbacks.get() >= 3,
            "every resumed for iteration must actually collect"
        );
    }
}

#[test]
fn owned_private_bytecode_for_break_returns_original_false_after_collecting_callback() {
    let session = RSession::new_for_gc_tests();
    let (variable, original_false) = session.with_active(|| {
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        crate::sexp::owner::with_runtime(&owner, |access| {
            let domain = access.domain();
            let variable = access
                .with_native(|_| {
                    domain.wrap(unsafe {
                        crate::sexp::symbol::Rf_install(c"private_for_break".as_ptr())
                    })
                })
                .unwrap();
            (variable, domain.logical(false))
        })
        .unwrap()
    });
    let sequence = fixture_integer(&session, 17);
    let code = private_code(
        &session,
        &[BCfor, 0, 1, 5, 6, BCbreak, BCreturn],
        &[variable, sequence],
    );
    let callbacks = collect_and_detach_pool(&session, &code);
    let result = session
        .with_active(|| eval_bytecode(code, session.global_env().unwrap().into_owned().unwrap()))
        .unwrap();
    assert!(
        callbacks.get() > 0,
        "the for iteration must actually collect"
    );
    assert_eq!(result.logical_elt(0), Some(0));
    assert_eq!(result.as_raw(), original_false.as_raw());
}

#[test]
fn owned_private_bytecode_rejects_self_recursive_loop_offsets() {
    let session = RSession::new_for_gc_tests();
    let error = run_private(&session, &[BCwhile, 0, 0, 4], &[]).unwrap_err();
    assert!(error.contains("recursive loop target cycle"));
    assert_eq!(
        run_private(&session, &[BCtrue, BCreturn], &[])
            .unwrap()
            .logical_elt(0),
        Some(1)
    );
}

#[test]
fn owned_private_bytecode_supports_deep_valid_loops_with_constant_host_stack() {
    let session = RSession::new_for_gc_tests();
    // The innermost while has a false condition; its false result
    // completes the enclosing condition segments outward iteratively.
    let depth = 1024;
    let end = depth * 4 + 2;
    let mut words = Vec::new();
    for index in 0..depth {
        words.extend_from_slice(&[
            BCwhile,
            (index * 4 + 4) as c_int,
            end as c_int,
            end as c_int,
        ]);
    }
    words.extend_from_slice(&[BCfalse, BCreturn]);
    let result = run_private(&session, &words, &[]).unwrap();
    assert_eq!(result.logical_elt(0), Some(0));
    let original_false = session.with_active(|| {
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        crate::sexp::owner::with_runtime(&owner, |access| access.domain().logical(false)).unwrap()
    });
    assert_eq!(result.as_raw(), original_false.as_raw());
}

#[test]
fn owned_private_bytecode_rejects_inactive_or_revoked_original_runtime() {
    let mut original = RSession::new_for_gc_tests();
    let code = private_code(&original, &[BCtrue, BCreturn], &[]);
    let environment = original.global_env().unwrap().into_owned().unwrap();
    let replacement = RSession::new_for_gc_tests();
    replacement.with_active(|| {
        let error = eval_bytecode(code.clone(), environment.clone()).unwrap_err();
        assert!(error.contains("owner"), "{error}");
    });
    original.close();
    replacement.with_active(|| {
        let error = eval_bytecode(code.clone(), environment.clone()).unwrap_err();
        assert!(error.contains("owner"), "{error}");
    });
    // Owning code remains physically readable, but grants no execution.
    assert_eq!(code.integer_elt(0), Some(BCtrue));
}

#[test]
fn owned_private_bytecode_uses_original_bank_after_ambient_singleton_replacement() {
    let session = RSession::new_for_gc_tests();
    let code = private_code(&session, &[BCtrue, BCreturn], &[]);
    session.with_active(|| {
        let owner = session.owner_token().unwrap().weak_owner().unwrap();
        let original_true =
            crate::sexp::owner::with_runtime(&owner, |access| access.domain().logical(true))
                .unwrap();
        crate::sexp::globals::close_immutable_singletons_for_test();
        let replacement = crate::sexp::globals::immutable_singleton_pool();
        assert_ne!(
            original_true.as_raw(),
            replacement.logical(true).projection()
        );
        let result = eval_bytecode(code, session.global_env().unwrap()).unwrap();
        assert_eq!(result.as_raw(), original_true.as_raw());
        assert_eq!(result.logical_elt(0), Some(1));
    });
}

#[test]
fn validates_every_pinned_gnu_opcode_width() {
    // Independent copy of the pinned `OP(name, argc)` table in
    // r-source/src/main/eval.c.  Keeping this fixture separate catches a
    // drift in the implementation table instead of inferring expected
    // widths by calling the validator under test.
    let expected: Vec<u8> = include_str!("../../../tests/fixtures/gnu-bytecode-opcodes.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| line.split('\t').nth(2).unwrap().parse().unwrap())
        .collect();
    assert_eq!(expected.len(), GNU_BC_OPCODE_COUNT);
    assert_eq!(GNU_BC_OPERAND_WIDTHS.as_slice(), expected.as_slice());

    for (opcode, &width) in expected.iter().enumerate() {
        // Zero is a valid framing value for every operand slot; semantic
        // indices are intentionally outside this wire-level validator's
        // scope.
        let mut stream = vec![GNU_BC_MAX_VERSION, opcode as c_int];
        stream.extend(std::iter::repeat_n(0, usize::from(width)));
        assert!(
            validate_gnu_bytecode_stream(&stream).is_ok(),
            "GNU opcode {opcode} should accept its pinned operand width"
        );
    }
}

#[test]
fn rejects_gnu_bytecode_version_opcode_and_truncation() {
    assert!(validate_gnu_bytecode_stream(&[]).is_err());
    let bad_ver = validate_gnu_bytecode_stream(&[GNU_BC_MIN_VERSION - 1]).unwrap_err();
    assert!(bad_ver.contains("BCMISMATCH"));
    assert!(bad_ver.contains("unsupported GNU R bytecode version"));
    let bad_ver_hi = validate_gnu_bytecode_stream(&[GNU_BC_MAX_VERSION + 1]).unwrap_err();
    assert!(bad_ver_hi.contains("BCMISMATCH"));
    let bad_op = validate_gnu_bytecode_stream(&[GNU_BC_MAX_VERSION, GNU_BC_OPCODE_COUNT as c_int])
        .unwrap_err();
    assert!(bad_op.contains("BCMISMATCH"));
    assert!(bad_op.contains("unknown GNU R bytecode opcode"));
    // GNU GOTO (opcode 2) has one operand.
    let err = validate_gnu_bytecode_stream(&[GNU_BC_MAX_VERSION, 2]).unwrap_err();
    assert!(err.contains("truncated GNU R bytecode opcode 2"));
}

#[test]
fn constant_return_adapter_rejects_shape_and_pool_index_mutations() {
    let valid = [GNU_BC_MAX_VERSION, 16, 1, 1];
    assert_eq!(validate_gnu_constant_return_stream(&valid, 2), Ok(1));
    assert!(
        validate_gnu_constant_return_stream(&valid, 1)
            .unwrap_err()
            .contains("out of range")
    );
    assert!(
        validate_gnu_constant_return_stream(&[GNU_BC_MAX_VERSION, 16, -1, 1], 2)
            .unwrap_err()
            .contains("negative")
    );
    assert!(
        validate_gnu_constant_return_stream(&[GNU_BC_MAX_VERSION, 16, 0, 0], 2)
            .unwrap_err()
            .contains("only LDCONST+RETURN")
    );
}

#[test]
fn scalar_return_adapter_accepts_only_pinned_gnu_shapes() {
    assert_eq!(
        validate_gnu_return_stream(&[GNU_BC_MAX_VERSION, 17, 1], 2),
        Ok(GnuConstantReturn::Null)
    );
    assert_eq!(
        validate_gnu_return_stream(&[GNU_BC_MAX_VERSION, 18, 1], 2),
        Ok(GnuConstantReturn::True)
    );
    assert_eq!(
        validate_gnu_return_stream(&[GNU_BC_MAX_VERSION, 19, 1], 2),
        Ok(GnuConstantReturn::False)
    );
    assert!(validate_gnu_return_stream(&[GNU_BC_MAX_VERSION, 17, 0], 2).is_err());
    // A private-dialect opcode with the same length is not accepted as a
    // scalar form merely because it happens to fit the shape.
    assert!(validate_gnu_return_stream(&[GNU_BC_MAX_VERSION, 20, 1], 2).is_err());
}

#[test]
fn gnu_arithmetic_and_comparison_streams_have_binary_stack_effects() {
    let add = [
        GNU_BC_MAX_VERSION,
        GNU_OP_LDCONST,
        0,
        GNU_OP_LDCONST,
        1,
        GNU_OP_ADD,
        0,
        GNU_OP_RETURN,
    ];
    assert!(validate_gnu_adapter_stream(&add, 2).unwrap());
    let cmp = [
        GNU_BC_MAX_VERSION,
        GNU_OP_LDCONST,
        0,
        GNU_OP_LDCONST,
        1,
        GNU_OP_LT,
        0,
        GNU_OP_RETURN,
    ];
    assert!(validate_gnu_adapter_stream(&cmp, 2).unwrap());
    let underflow = [
        GNU_BC_MAX_VERSION,
        GNU_OP_LDCONST,
        0,
        GNU_OP_ADD,
        0,
        GNU_OP_RETURN,
    ];
    assert!(validate_gnu_adapter_stream(&underflow, 1).is_err());
    let bad_operand = [
        GNU_BC_MAX_VERSION,
        GNU_OP_LDCONST,
        0,
        GNU_OP_LDCONST,
        1,
        GNU_OP_ADD,
        2,
        GNU_OP_RETURN,
    ];
    assert!(validate_gnu_adapter_stream(&bad_operand, 2).is_err());
    let unary = [
        GNU_BC_MAX_VERSION,
        GNU_OP_LDCONST,
        0,
        GNU_OP_UMINUS,
        1,
        GNU_OP_RETURN,
    ];
    assert!(validate_gnu_adapter_stream(&unary, 2).unwrap());
    let unary_underflow = [GNU_BC_MAX_VERSION, GNU_OP_UMINUS, 0, GNU_OP_RETURN];
    assert!(validate_gnu_adapter_stream(&unary_underflow, 1).is_err());
    let power = [
        GNU_BC_MAX_VERSION,
        GNU_OP_LDCONST,
        0,
        GNU_OP_LDCONST,
        1,
        GNU_OP_EXPT,
        0,
        GNU_OP_RETURN,
    ];
    assert!(validate_gnu_adapter_stream(&power, 2).unwrap());
}

#[test]
fn bounded_gnu_adapter_proves_branch_targets_and_stack_shape() {
    assert!(validate_gnu_adapter_stream(&[12], 0).is_err());
    let branch = [12, 20, 1, 3, 0, 9, 16, 2, 1, 16, 3, 1];
    assert_eq!(validate_gnu_adapter_stream(&branch, 4), Ok(true));

    let mut non_boundary = branch;
    non_boundary[5] = 7;
    assert!(
        validate_gnu_adapter_stream(&non_boundary, 4)
            .unwrap_err()
            .contains("instruction boundary")
    );

    assert!(
        validate_gnu_adapter_stream(&[12, 1], 0)
            .unwrap_err()
            .contains("stack depth 1")
    );
    assert!(
        validate_gnu_adapter_stream(&[12, 18, 19, 1], 0)
            .unwrap_err()
            .contains("stack depth 1")
    );
    assert!(
        validate_gnu_adapter_stream(&[12, 18], 0)
            .unwrap_err()
            .contains("invalid instruction")
    );

    // ADD is well-framed, but remains source-fallback territory.
    assert_eq!(validate_gnu_adapter_stream(&[12, 66, 0, 1], 1), Ok(false));
}

#[test]
fn bounded_gnu_adapter_validates_superassignment_stack_and_pool() {
    assert_eq!(
        validate_gnu_adapter_stream(&[12, 20, 0, 95, 1, 1], 2),
        Ok(true)
    );
    assert!(validate_gnu_adapter_stream(&[12, 95, 0, 1], 1).is_err());
    assert!(validate_gnu_adapter_stream(&[12, 20, 0, 95, 2, 1], 2).is_err());
}

#[test]
fn bounded_gnu_adapter_validates_settag_call_frames() {
    let valid = [12, 23, 0, 29, 1, 31, 2, 38, 3, 1];
    assert_eq!(validate_gnu_adapter_stream(&valid, 4), Ok(true));

    let no_frame = [12, 31, 0, 1];
    assert!(
        validate_gnu_adapter_stream(&no_frame, 1)
            .unwrap_err()
            .contains("SETTAG has no active GETFUN call")
    );

    let no_argument = [12, 23, 0, 31, 1, 38, 2, 1];
    assert!(
        validate_gnu_adapter_stream(&no_argument, 3)
            .unwrap_err()
            .contains("SETTAG has no preceding call argument")
    );

    let bad_tag = [12, 23, 0, 29, 1, 31, 3, 38, 2, 1];
    assert!(
        validate_gnu_adapter_stream(&bad_tag, 3)
            .unwrap_err()
            .contains("SETTAG constant pool index 3")
    );
}

#[test]
fn bounded_gnu_adapter_validates_builtin_call_frames() {
    let valid = [12, 123, 0, 11, 26, 1, 20, 2, 33, 39, 0, 1];
    assert_eq!(validate_gnu_adapter_stream(&valid, 3), Ok(true));
    assert!(
        validate_gnu_adapter_stream(&[12, 33, 39, 0, 1], 1)
            .unwrap_err()
            .contains("PUSHARG has no active call")
    );
    assert!(
        validate_gnu_adapter_stream(&[12, 39, 0, 1], 1)
            .unwrap_err()
            .contains("CALL has no active GETFUN call")
    );
    assert!(validate_gnu_adapter_stream(&[12, 26, 2, 1], 2).is_err());
}

#[test]
fn bounded_gnu_adapter_validates_checkfun_stack() {
    assert_eq!(
        validate_gnu_adapter_stream(&[12, 16, 0, 28, 29, 0, 38, 1, 1], 2),
        Ok(true)
    );
    assert!(
        validate_gnu_adapter_stream(&[12, 28, 1], 1)
            .unwrap_err()
            .contains("CHECKFUN has an empty stack")
    );
}

#[test]
fn bounded_gnu_adapter_rejects_malformed_for_control_flow() {
    // LDCONST; STARTFOR -> STEPFOR; LDNULL/POP body; STEPFOR -> body;
    // ENDFOR; RETURN.
    let valid = [12, 16, 0, 11, 1, 2, 9, 17, 4, 12, 7, 13, 1];
    assert_eq!(validate_gnu_adapter_stream(&valid, 3), Ok(true));

    let mut start_to_body = valid;
    start_to_body[6] = 7;
    assert!(validate_gnu_adapter_stream(&start_to_body, 3).is_err());

    let mut stray_step = valid;
    stray_step[6] = 12;
    assert!(validate_gnu_adapter_stream(&stray_step, 3).is_err());

    let stray_end = [12, 17, 13, 1];
    assert!(
        validate_gnu_adapter_stream(&stray_end, 0)
            .unwrap_err()
            .contains("no active for loop")
    );

    let mut non_boundary = valid;
    non_boundary[10] = 10;
    assert!(
        validate_gnu_adapter_stream(&non_boundary, 3)
            .unwrap_err()
            .contains("instruction boundary")
    );

    let early_return = [12, 16, 0, 11, 1, 2, 11, 16, 0, 1, 4, 12, 7, 13, 1];
    assert_eq!(validate_gnu_adapter_stream(&early_return, 3), Ok(true));
}

#[test]
fn bounded_gnu_adapter_accepts_loopcntxt_returnjmp_streams() {
    // compiler:::disassemble of
    // function(x) { for (i in x) { eval(quote(NULL)); if (i > 0L) return(i) }; 0L }
    let eval_loop = [
        12, 20, 1, 11, 3, 2, 7, 7, 1, 37, 2, 35, 23, 4, 29, 6, 38, 5, 4, 20, 2, 16, 7, 56, 8, 3, 9,
        33, 20, 2, 103, 2, 34, 17, 4, 12, 12, 8, 1, 13, 4, 16, 7, 1,
    ];
    assert_eq!(validate_gnu_adapter_stream(&eval_loop, 10), Ok(true));

    // compiler:::disassemble of function(x) { repeat { eval(quote(NULL)); return(x) } }
    let repeat_loop = [
        12, 7, 0, 17, 23, 2, 29, 4, 38, 3, 4, 20, 5, 103, 4, 2, 4, 8, 0, 17, 15, 1,
    ];
    assert_eq!(validate_gnu_adapter_stream(&repeat_loop, 6), Ok(true));

    assert!(
        validate_gnu_adapter_stream(&[12, 103], 0)
            .unwrap_err()
            .contains("stack")
    );

    // STARTLOOPCNTXT without the GOTO-to-STEPFOR preamble is not a for entry.
    let missing_goto = [12, 16, 0, 11, 1, 2, 7, 7, 1, 12, 17, 4, 12, 10, 8, 1, 13, 1];
    assert!(validate_gnu_adapter_stream(&missing_goto, 3).is_err());
}

#[test]
fn bounded_gnu_calls_validate_frame_lifetime_and_preserve_outer_operands() {
    for words in [
        vec![12, 38, 0, 1],        // CALL without GETFUN
        vec![12, 29, 0, 1],        // MAKEPROM without GETFUN
        vec![12, 23, 0, 4, 17, 1], // POP destroys the active function
        vec![12, 23, 0, 1],        // unfinished call at RETURN
    ] {
        assert!(validate_gnu_adapter_stream(&words, 1).is_err(), "{words:?}");
    }
    // An argument call in the right operand of ADD must retain the left.
    assert_eq!(
        validate_gnu_adapter_stream(&[12, 16, 0, 23, 1, 29, 2, 38, 3, 44, 4, 1], 5),
        Ok(true)
    );
}

#[test]
fn gnu_constant_arguments_validate_pool_indices_and_call_frames() {
    for words in [
        vec![12, 34, 0, 1],
        vec![12, 35, 1],
        vec![12, 36, 1],
        vec![12, 37, 1],
        vec![12, 23, 0, 34, -1, 38, 0, 1],
        vec![12, 23, 0, 34, 1, 38, 0, 1],
    ] {
        assert!(validate_gnu_adapter_stream(&words, 1).is_err(), "{words:?}");
    }
    for opcode in [35, 36, 37] {
        assert_eq!(
            validate_gnu_adapter_stream(&[12, 23, 0, opcode, 38, 0, 1], 1),
            Ok(true)
        );
    }
    assert_eq!(
        validate_gnu_adapter_stream(&[12, 23, 0, 34, 0, 38, 0, 1], 1),
        Ok(true)
    );
}

#[test]
fn accepts_pinned_compiler_fixture_stream() {
    // `compiler:::disassemble(compiler::cmpfun(function(x)
    // if (x) x + 1 else 0))` under the pinned GNU R oracle reports this
    // exact integer stream (version 12, GETVAR, BRIFNOT, LDCONST, ADD,
    // RETURN).  The opcode names and operands are therefore sourced from
    // an actual compiler-produced fixture, not the private dialect.
    let fixture = [12, 20, 1, 3, 0, 13, 20, 1, 16, 2, 44, 3, 1, 16, 4, 17];
    assert!(validate_gnu_bytecode_stream(&fixture).is_ok());
}

#[test]
fn gnu_dup_validator_rejects_underflow_and_unbalanced_deep_returns() {
    assert!(
        validate_gnu_adapter_stream(&[12, 5, 1], 0)
            .unwrap_err()
            .contains("empty stack")
    );
    let mut words = vec![12, 17];
    words.extend(std::iter::repeat_n(5, 64));
    words.push(1);
    assert!(
        validate_gnu_adapter_stream(&words, 0)
            .unwrap_err()
            .contains("requires stack depth 1, found 65")
    );
    assert!(validate_gnu_adapter_stream(&[12, 17, 5, 4, 1], 0).unwrap());
}
