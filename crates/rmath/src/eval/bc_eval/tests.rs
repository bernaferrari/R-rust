//! bc_eval tests — extracted verbatim from eval/bc_eval.rs

use super::*;
use crate::sexp::accessors::SET_VECTOR_ELT;
use crate::sexp::globals::R_BaseEnv;

fn fixture_arena() -> crate::sexp::memory::RArena {
    let heap = crate::sexp::instance::with_required_current_instance(|owner| unsafe {
        (*owner).heap_identity.clone()
    });
    crate::sexp::memory::RArena::fresh_with_identity(heap)
}

#[test]
fn gnu_methods_tail_fixtures_execute_vector_calls_and_replacements() {
    let mut session = crate::sexp::session::RSession::new_for_gc_tests();
    let cases: [(&[u8], &str); 4] = [
        (
            include_bytes!("../../../../r-embed/tests/fixtures/gnu-bytecode-methods-tail/tail.rds"),
            "identical(f(c('x','y','z','w'),1L,'foo','fixturePackage'),structure(c(x='foo'),package='fixturePackage'))",
        ),
        (
            include_bytes!(
                "../../../../r-embed/tests/fixtures/gnu-bytecode-methods-tail/padding.rds"
            ),
            "identical(f('foo',3L),c('foo','ANY','ANY','ANY'))",
        ),
        (
            include_bytes!(
                "../../../../r-embed/tests/fixtures/gnu-bytecode-methods-tail/oldclass.rds"
            ),
            "identical(f('child',c('parent','oldClass')),c('child','parent','oldClass'))",
        ),
        (
            include_bytes!(
                "../../../../r-embed/tests/fixtures/gnu-bytecode-methods-tail/attribute.rds"
            ),
            "identical(f(c(x='foo'),'fixturePackage'),structure(c(x='foo'),package='fixturePackage'))",
        ),
    ];
    for (bytes, assertion) in cases {
        let raw = bytes
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let (result, _, _) = session.eval_code_with_output_capture(&format!(
            "f<-unserialize(as.raw(c({raw})));{assertion}"
        ));
        assert_eq!(result.unwrap().logical_elt(0), Some(TRUE), "{assertion}");
        unsafe {
            let function = own_operand(crate::sexp::envir::R_findVar(
                crate::sexp::symbol::Rf_install(c"f".as_ptr()),
                session.global_env().unwrap().as_raw(),
            ));
            assert!(
                BCODE_IS_GNU(crate::sexp::accessors::BODY(function.as_raw())),
                "fixture must execute GNU bytecode: {assertion}"
            );
        }
    }
    let words: Vec<i32> = cases[0]
        .0
        .windows(8)
        .position(|bytes| bytes == [0, 0, 0, 12, 0, 0, 0, 123])
        .map(|start| {
            let length =
                u32::from_be_bytes(cases[0].0[start - 4..start].try_into().unwrap()) as usize;
            cases[0].0[start..start + length * 4]
                .chunks_exact(4)
                .map(|bytes| i32::from_be_bytes(bytes.try_into().unwrap()))
                .collect()
        })
        .unwrap();
    assert_eq!(
        super::super::bytecode::validate_gnu_adapter_stream(&words, 48),
        Ok(true),
        "tail must be actually executable"
    );
    let mut bytes = cases[0].0.to_vec();
    let stream = [20_i32, 7, 16, 25, 51, 26, 22, 27, 4];
    let encoded: Vec<_> = stream.iter().flat_map(|word| word.to_be_bytes()).collect();
    let offsets: Vec<_> = bytes
        .windows(encoded.len())
        .enumerate()
        .filter_map(|(offset, value)| (value == encoded).then_some(offset))
        .collect();
    assert_eq!(offsets.len(), 1, "frozen GNU EQ stream must be unique");
    // EQ -> NE changes the compiled tail while retained source is intact.
    let offset = offsets[0] + 4 * 4;
    bytes[offset..offset + 4].copy_from_slice(&52_i32.to_be_bytes());
    let raw = bytes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let (result, _, _) = session.eval_code_with_output_capture(&format!("f<-unserialize(as.raw(c({raw})));g<-unserialize(serialize(f,NULL));length(f(c('x','y','z','w'),1L,'foo','fixturePackage'))+10L*length(g(c('x','y','z','w'),1L,'foo','fixturePackage'))"));
    let result = result.unwrap().into_owned().unwrap();
    unsafe {
        let function = own_operand(crate::sexp::envir::R_findVar(
            crate::sexp::symbol::Rf_install(c"f".as_ptr()),
            session.global_env().unwrap().as_raw(),
        ));
        let body = own_operand(crate::sexp::accessors::BODY(function.as_raw()));
        assert_eq!(
            body.typeof_(),
            SEXPTYPE::BCODESXP,
            "GNU tail cannot run retained source"
        );
        assert!(BCODE_IS_GNU(body.as_raw()), "GNU tail dialect tag");
        let words = owned_instruction_words(&own_operand(crate::sexp::accessors::VECTOR_ELT(
            body.as_raw(),
            0,
        )));
        assert_eq!(words[75], 52, "mutated EQ operand must be loaded");
    }
    assert_eq!(
        result.integer_elt(0),
        Some(44),
        "mutated bytecode must survive serialization and control the result (real {:?})",
        result.real_elt(0)
    );
}

#[test]
fn gnu_source_marker_does_not_override_compiled_instructions() {
    let mut session = crate::sexp::session::RSession::new_without_default_packages();
    let fixture =
        include_bytes!("../../../../r-embed/tests/fixtures/gnu-bytecode-source-marker/branch.rds");
    let words: [i32; 14] = [12, 17, 4, 20, 2, 3, 3, 11, 16, 4, 1, 16, 5, 1];
    let encoded: Vec<_> = words.iter().flat_map(|word| word.to_be_bytes()).collect();
    let offset = fixture
        .windows(encoded.len())
        .position(|bytes| bytes == encoded)
        .unwrap();
    let mut bytes = fixture.to_vec();
    bytes[offset + 9 * 4..offset + 10 * 4].copy_from_slice(&5_i32.to_be_bytes());
    let raw = bytes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let (value, _, _) =
        session.eval_code_with_output_capture(&format!("f<-unserialize(as.raw(c({raw})));f(TRUE)"));
    let value = value.unwrap();
    unsafe {
        // SAFETY: the evaluation result is live and no R reentry occurs while reading.
        assert_eq!(value.integer_elt(0), Some(42));
    }
}

fn assert_r_error(action: impl FnOnce()) -> RError {
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action))
        .expect_err("expected RError panic");
    payload
        .downcast_ref::<RError>()
        .expect("expected RError payload")
        .clone()
}

fn bcode_with(
    arena: &mut crate::sexp::memory::RArena,
    instructions: &[c_int],
    consts: SEXP,
) -> SEXP {
    let code = arena.alloc_vector(SEXPTYPE::INTSXP, instructions.len() as i64);
    let code_data = unsafe { crate::sexp::accessors::DATAPTR(code) as *mut c_int };
    for (i, instruction) in instructions.iter().enumerate() {
        unsafe {
            code_data.add(i).write(*instruction);
        }
    }

    let stack_hint = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
    let stack_data = unsafe { crate::sexp::accessors::DATAPTR(stack_hint) as *mut c_int };
    unsafe {
        *stack_data = 8;
    }

    let bcode = arena.alloc_vector(SEXPTYPE::BCODESXP, 3);
    arena.set_reference_element(bcode, 0, code).unwrap();
    arena.set_reference_element(bcode, 1, consts).unwrap();
    arena.set_reference_element(bcode, 2, stack_hint).unwrap();
    bcode
}

fn empty_env(arena: &mut crate::sexp::memory::RArena) -> SEXP {
    let env = arena.alloc_node(SEXPTYPE::ENVSXP);
    unsafe {
        crate::sexp::accessors::SET_FRAME(env, R_NilValue());
        crate::sexp::accessors::SET_ENCLOS(env, R_NilValue());
        crate::sexp::accessors::SET_HASHTAB(env, ptr::null_mut());
    }
    env
}

#[test]
fn test_bc_stack_basic() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    let mut stack = R_bcstack_t::new(4);
    unsafe {
        let a = Rf_ScalarInteger(1);
        let b = Rf_ScalarInteger(2);
        stack.push(a);
        stack.push(b);
        assert_eq!(stack.depth(), 2);
        assert_eq!(stack.pop_owned().as_raw(), b);
        assert_eq!(stack.depth(), 1);
    }
}

#[test]
fn test_opcodes_defined() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    assert!(opcodes::OP_RETURN > 0);
    assert!(opcodes::OP_PUSHTRUE > 0);
    assert!(opcodes::OP_PUSHFALSE > 0);
    assert!(opcodes::OP_CALL > 0);
    assert!(opcodes::OP_STEPFOR > 0);
    assert!(opcodes::OP_BREAK > 0);
}

#[test]
fn gnu_constant_return_uses_code_pool_after_source_edit() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    unsafe {
        let code = Rf_allocVector(SEXPTYPE::INTSXP, 4);
        let code_data = INTEGER(code);
        *code_data = super::super::bytecode::GNU_BC_MAX_VERSION;
        *code_data.add(1) = 16; // LDCONST
        *code_data.add(2) = 0;
        *code_data.add(3) = 1; // RETURN

        let consts = Rf_allocVector(SEXPTYPE::VECSXP, 1);
        SET_VECTOR_ELT(consts, 0, Rf_ScalarInteger(42));
        let stack_hint = Rf_ScalarInteger(4);
        let marker = Rf_ScalarInteger(super::super::bytecode::GNU_BC_DIALECT_MARKER);
        let source = Rf_ScalarInteger(7);
        let bcode = Rf_allocVector(SEXPTYPE::BCODESXP, 5);
        SET_VECTOR_ELT(bcode, 0, code);
        SET_VECTOR_ELT(bcode, 1, consts);
        SET_VECTOR_ELT(bcode, 2, stack_hint);
        SET_VECTOR_ELT(bcode, 3, marker);
        SET_VECTOR_ELT(bcode, 4, source);

        // The retained source is editable metadata; execution must use
        // the tagged GNU instruction stream and its constant pool.
        SET_VECTOR_ELT(bcode, 4, Rf_ScalarInteger(99));
        let result = bcEval(bcode, R_BaseEnv());
        assert_eq!(*INTEGER(result), 42);
    }
}

fn gnu_tagged_bcode(instructions: &[c_int], consts: SEXP) -> SEXP {
    unsafe {
        let code = Rf_allocVector(SEXPTYPE::INTSXP, instructions.len() as c_int);
        let code_data = INTEGER(code);
        for (i, instruction) in instructions.iter().enumerate() {
            *code_data.add(i) = *instruction;
        }
        let stack_hint = Rf_ScalarInteger(8);
        let marker = Rf_ScalarInteger(super::super::bytecode::GNU_BC_DIALECT_MARKER);
        let source = Rf_ScalarInteger(0);
        let bcode = Rf_allocVector(SEXPTYPE::BCODESXP, 5);
        SET_VECTOR_ELT(bcode, 0, code);
        SET_VECTOR_ELT(bcode, 1, consts);
        SET_VECTOR_ELT(bcode, 2, stack_hint);
        SET_VECTOR_ELT(bcode, 3, marker);
        SET_VECTOR_ELT(bcode, 4, source);
        bcode
    }
}

#[test]
fn gnu_doloopbreak_skips_the_loop_body_constant() {
    // GNU DOLOOPBREAK findcontext(CTXT_BREAK) jumps to STARTLOOPCNTXT's
    // break operand and drops values the body pushed.
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    unsafe {
        let code = [
            super::super::bytecode::GNU_BC_MAX_VERSION,
            super::super::bytecode::GNU_OP_STARTLOOPCNTXT,
            0, // while, not for
            7, // break target: the result LDCONST
            super::super::bytecode::GNU_OP_LDCONST,
            0, // poison 7, must not be returned
            super::super::bytecode::GNU_OP_DOLOOPBREAK,
            super::super::bytecode::GNU_OP_LDCONST,
            1, // 42
            super::super::bytecode::GNU_OP_RETURN,
        ];
        let consts = Rf_allocVector(SEXPTYPE::VECSXP, 2);
        SET_VECTOR_ELT(consts, 0, Rf_ScalarInteger(7));
        SET_VECTOR_ELT(consts, 1, Rf_ScalarInteger(42));
        let result = bcEval(gnu_tagged_bcode(&code, consts), R_BaseEnv());
        assert_eq!(*INTEGER(result), 42);
    }
}

#[test]
fn gnu_doloopnext_outside_a_loop_errors() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    let err = assert_r_error(|| unsafe {
        // False condition jumps to DOLOOPNEXT. The other edge reaches
        // RETURN, so the stream is otherwise a complete GNU program.
        let code = [
            super::super::bytecode::GNU_BC_MAX_VERSION,
            super::super::bytecode::GNU_OP_LDFALSE,
            super::super::bytecode::GNU_OP_BRIFNOT,
            0,
            8,
            super::super::bytecode::GNU_OP_LDCONST,
            0,
            super::super::bytecode::GNU_OP_RETURN,
            super::super::bytecode::GNU_OP_DOLOOPNEXT,
        ];
        let consts = Rf_allocVector(SEXPTYPE::VECSXP, 1);
        SET_VECTOR_ELT(consts, 0, Rf_ScalarInteger(42));
        let _ = bcEval(gnu_tagged_bcode(&code, consts), R_BaseEnv());
    });
    assert!(err.message.contains("DOLOOPNEXT"), "{}", err.message);
}

#[test]
fn test_bc_push_pop_sequence() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    let mut stack = R_bcstack_t::new(8);
    unsafe {
        stack.push(R_NilValue());
        stack.push(Rf_ScalarInteger(1));
        stack.push(Rf_ScalarInteger(2));
        assert_eq!(stack.depth(), 3);
        let top_owned = stack.pop_owned();
        let top = top_owned.as_raw();
        assert!(!top.is_null());
        assert_eq!(stack.depth(), 2);
    }
}

#[test]
fn test_bc_dup2() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    let mut stack = R_bcstack_t::new(8);
    unsafe {
        stack.push(Rf_ScalarInteger(10));
        stack.push(Rf_ScalarInteger(20));
        assert_eq!(stack.depth(), 2);
        let top = stack.top();
        let s1 = stack.depth();
        let next = stack.at(s1 - 2);
        stack.push(next);
        stack.push(top);
        assert_eq!(stack.depth(), 4);
    }
}

#[test]
fn test_bc_goto_and_branch() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    let mut stack = R_bcstack_t::new(8);
    unsafe {
        stack.push(Rf_ScalarLogical(TRUE));
        let val = stack.top();
        let cond = eval_bc_condition(val);
        assert!(cond);

        stack.push(Rf_ScalarLogical(FALSE));
        let val2 = stack.top();
        let cond2 = eval_bc_condition(val2);
        assert!(!cond2);
    }
}

#[test]
fn test_bc_eval_simple_code() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();

    let mut arena = fixture_arena();

    let code = arena.alloc_vector(SEXPTYPE::INTSXP, 5);
    let code_data = unsafe { crate::sexp::accessors::DATAPTR(code) as *mut c_int };
    unsafe {
        code_data.add(0).write(opcodes::OP_PUSHNULL);
        code_data.add(1).write(opcodes::OP_DUP);
        code_data.add(2).write(opcodes::OP_RETURN);
    }

    let consts = arena.alloc_vector(SEXPTYPE::VECSXP, 0);
    let stack_hint = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
    let stack_data = unsafe { crate::sexp::accessors::DATAPTR(stack_hint) as *mut c_int };
    unsafe {
        *stack_data = 8;
    }

    let bcode = arena.alloc_vector(SEXPTYPE::BCODESXP, 3);
    arena.set_reference_element(bcode, 0, code).unwrap();
    arena.set_reference_element(bcode, 1, consts).unwrap();
    arena.set_reference_element(bcode, 2, stack_hint).unwrap();

    let env = arena.alloc_node(SEXPTYPE::ENVSXP);
    unsafe {
        crate::sexp::accessors::SET_FRAME(env, R_NilValue());
        crate::sexp::accessors::SET_ENCLOS(env, R_NilValue());
        crate::sexp::accessors::SET_HASHTAB(env, ptr::null_mut());
    }

    let result = unsafe { bcEval(bcode, env) };
    assert!(!result.is_null());
}

#[test]
fn test_bc_eval_rejects_unknown_opcode() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();

    let mut arena = fixture_arena();

    let code = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
    let code_data = unsafe { crate::sexp::accessors::DATAPTR(code) as *mut c_int };
    unsafe {
        code_data.write(9999);
    }

    let consts = arena.alloc_vector(SEXPTYPE::VECSXP, 0);
    let stack_hint = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
    let stack_data = unsafe { crate::sexp::accessors::DATAPTR(stack_hint) as *mut c_int };
    unsafe {
        *stack_data = 8;
    }

    let bcode = arena.alloc_vector(SEXPTYPE::BCODESXP, 3);
    arena.set_reference_element(bcode, 0, code).unwrap();
    arena.set_reference_element(bcode, 1, consts).unwrap();
    arena.set_reference_element(bcode, 2, stack_hint).unwrap();

    let env = arena.alloc_node(SEXPTYPE::ENVSXP);
    unsafe {
        crate::sexp::accessors::SET_FRAME(env, R_NilValue());
        crate::sexp::accessors::SET_ENCLOS(env, R_NilValue());
        crate::sexp::accessors::SET_HASHTAB(env, ptr::null_mut());
    }

    let err = assert_r_error(|| unsafe {
        bcEval(bcode, env);
    });
    assert!(err.message.contains("BCMISMATCH"));
    assert!(
        err.message
            .contains("unknown private-dialect bytecode opcode")
    );
}

#[test]
fn test_bc_eval_setvar2_writes_enclosing_frame() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();

    let mut arena = fixture_arena();

    let parent = empty_env(&mut arena);
    let child = empty_env(&mut arena);
    unsafe {
        crate::sexp::accessors::SET_ENCLOS(child, parent);
    }

    let sym = unsafe { crate::sexp::symbol::Rf_install(c"x".as_ptr()) };
    // Pre-bind in the parent so SETVAR2's setVar updates that frame
    // (mirrors <<- when the symbol already exists in an enclosing env).
    unsafe {
        defineVar(sym, Rf_ScalarInteger(7), parent);
    }
    let value = unsafe { Rf_ScalarInteger(42) };
    let consts = arena.alloc_vector(SEXPTYPE::VECSXP, 2);
    unsafe {
        SET_VECTOR_ELT(consts, 0, sym);
        SET_VECTOR_ELT(consts, 1, value);
    }
    let bcode = bcode_with(
        &mut arena,
        &[
            opcodes::OP_PUSHCONST,
            1,
            opcodes::OP_SETVAR2,
            0,
            opcodes::OP_RETURN,
        ],
        consts,
    );

    let result = unsafe { bcEval(bcode, child) };
    unsafe {
        assert_eq!(*INTEGER(result), 42);
        let found_parent = crate::sexp::envir::R_findVarInFrame(parent, sym);
        assert_ne!(found_parent, R_UnboundValue());
        assert_eq!(*INTEGER(found_parent), 42);
        let found_child = crate::sexp::envir::R_findVarInFrame(child, sym);
        assert_eq!(found_child, R_UnboundValue());
    }
}

#[test]
fn test_bc_eval_rejects_truncated_operand() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    let mut arena = fixture_arena();
    let consts = arena.alloc_vector(SEXPTYPE::VECSXP, 0);
    let bcode = bcode_with(&mut arena, &[opcodes::OP_PUSHCONST], consts);
    let env = empty_env(&mut arena);

    let err = assert_r_error(|| unsafe {
        bcEval(bcode, env);
    });
    assert!(
        err.message
            .contains("PUSHCONST bytecode operand is truncated")
    );
}

#[test]
fn test_bc_eval_rejects_invalid_constant_index() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    let mut arena = fixture_arena();
    let consts = arena.alloc_vector(SEXPTYPE::VECSXP, 0);
    let bcode = bcode_with(
        &mut arena,
        &[opcodes::OP_PUSHCONST, 0, opcodes::OP_RETURN],
        consts,
    );
    let env = empty_env(&mut arena);

    let err = assert_r_error(|| unsafe {
        bcEval(bcode, env);
    });
    assert!(
        err.message
            .contains("PUSHCONST constant index 0 out of range")
    );
}

#[test]
fn test_bc_eval_rejects_stack_underflow() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    let mut arena = fixture_arena();
    let consts = arena.alloc_vector(SEXPTYPE::VECSXP, 0);
    let bcode = bcode_with(&mut arena, &[opcodes::OP_RETURN], consts);
    let env = empty_env(&mut arena);

    let err = assert_r_error(|| unsafe {
        bcEval(bcode, env);
    });
    assert!(err.message.contains("RETURN bytecode stack underflow"));
}

#[test]
fn test_bc_eval_rejects_invalid_jump_target() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    let mut arena = fixture_arena();
    let consts = arena.alloc_vector(SEXPTYPE::VECSXP, 0);
    let bcode = bcode_with(&mut arena, &[opcodes::OP_BRANCH, -1], consts);
    let env = empty_env(&mut arena);

    let err = assert_r_error(|| unsafe {
        bcEval(bcode, env);
    });
    assert!(
        err.message
            .contains("BRANCH bytecode jump target -1 is outside")
    );
}

#[test]
fn test_bc_eval_swload_loads_constant_table_entry() {
    let _session = crate::sexp::session::RSession::new_without_default_packages();
    let mut arena = fixture_arena();
    let consts = arena.alloc_vector(SEXPTYPE::VECSXP, 1);
    let value = unsafe { Rf_ScalarInteger(42) };
    arena.set_reference_element(consts, 0, value).unwrap();
    let bcode = bcode_with(
        &mut arena,
        &[
            opcodes::OP_PUSHNULL,
            opcodes::OP_SWLOAD,
            0,
            opcodes::OP_RETURN,
        ],
        consts,
    );
    let env = empty_env(&mut arena);

    let result = unsafe { bcEval(bcode, env) };
    unsafe {
        assert_eq!(*INTEGER(result), 42);
    }
}

#[test]
fn test_bc_eval_operand_stack_survives_gc_in_callee() {
    let mut session = crate::sexp::session::RSession::new_without_default_packages();

    // Callee whose body forces a full collection while the caller's
    // bytecode frame is suspended mid-OP_CALL.
    let gc_closure = session.with_active(|| unsafe {
        let clos = crate::sexp::memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::CLOSXP));
        crate::sexp::accessors::SET_FORMALS(clos, R_NilValue());
        let body = Rf_cons(
            crate::sexp::symbol::Rf_install(c"gc".as_ptr()),
            R_NilValue(),
        );
        crate::sexp::accessors::SET_TYPEOF(body, SEXPTYPE::LANGSXP.as_c_int());
        crate::sexp::accessors::SET_BODY(clos, body);
        crate::sexp::accessors::SET_CLOENV(clos, crate::sexp::globals::R_BaseEnv());
        clos
    });
    let _callee_guard = unsafe {
        /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */
        own_operand(gc_closure)
    };

    // Detached arena keeps the bytecode alive for the whole test but is
    // invisible to the instance collector.
    let mut arena = fixture_arena();
    let consts = arena.alloc_vector(SEXPTYPE::VECSXP, 1);
    arena.set_reference_element(consts, 0, gc_closure).unwrap();
    let bcode = bcode_with(
        &mut arena,
        &[
            opcodes::OP_PUSHTRUE, // fresh young scalar: operand-stack-only
            opcodes::OP_PUSHCONST,
            0, // push the gc() closure
            opcodes::OP_CALL,
            0,                  // nested evaluation runs a full GC
            opcodes::OP_POP,    // drop gc()'s NULL result
            opcodes::OP_RETURN, // return the residual operand value
        ],
        consts,
    );
    let env = empty_env(&mut arena);

    let result = unsafe { bcEval(bcode, env) };
    // The residual operand value must survive the collection that ran
    // inside the callee: still active, still the TRUE logical.
    let still_active = session
        .with_arena(|a| a.active_nodes().any(|node| node == result))
        .expect("session active");
    assert!(
        still_active,
        "operand-stack value was swept by the GC run inside the callee"
    );
    unsafe {
        assert_eq!(TYPEOF(result), SEXPTYPE::LGLSXP);
        assert_eq!(*LOGICAL(result), TRUE);
    }
}
#[test]
fn test_bc_eval_operand_stack_survives_gc_during_lookup() {
    let mut session = crate::sexp::session::RSession::new_without_default_packages();
    let (symbol, env) = session.with_active(|| unsafe {
        let env = crate::sexp::globals::R_GlobalEnv();
        let symbol = crate::sexp::symbol::Rf_install(c"lookup_gc_value".as_ptr());
        let call = Rf_cons(
            crate::sexp::symbol::Rf_install(c"gc".as_ptr()),
            R_NilValue(),
        );
        crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
        let promise = crate::sexp::memory_ext::mkPROMISE(call, env);
        defineVar(symbol, promise, env);
        (symbol, env)
    });
    let mut arena = fixture_arena();
    let consts = arena.alloc_vector(SEXPTYPE::VECSXP, 1);
    unsafe {
        crate::sexp::accessors::SET_VECTOR_ELT(consts, 0, symbol);
    }
    let bcode = bcode_with(
        &mut arena,
        &[
            opcodes::OP_PUSHTRUE,
            opcodes::OP_GETVAR,
            0,
            opcodes::OP_POP,
            opcodes::OP_RETURN,
        ],
        consts,
    );
    let result = session.with_active(|| unsafe { bcEval(bcode, env) });
    assert!(
        session
            .with_arena(|a| a.active_nodes().any(|p| p == result))
            .unwrap()
    );
    unsafe {
        assert_eq!(TYPEOF(result), SEXPTYPE::LGLSXP);
        assert_eq!(*LOGICAL(result), TRUE);
    }
}
#[test]
fn owned_bytecode_gnu_deep_stack_executes_above_legacy_cutoff() {
    let session = crate::sexp::session::RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let pool = own_operand(Rf_allocVector(SEXPTYPE::VECSXP, 0));
        let mut words = vec![
            super::super::bytecode::GNU_BC_MAX_VERSION,
            super::super::bytecode::GNU_OP_LDNULL,
        ];
        words.extend(std::iter::repeat_n(super::super::bytecode::GNU_OP_DUP, 256));
        words.extend(std::iter::repeat_n(super::super::bytecode::GNU_OP_POP, 256));
        words.push(super::super::bytecode::GNU_OP_RETURN);
        let bytecode = own_operand(gnu_tagged_bcode(&words, pool.as_raw()));
        let result = own_operand(bcEval(bytecode.as_raw(), R_BaseEnv()));
        assert!(result.is_null_value());
    });
}

#[test]
fn owned_bytecode_rejects_empty_wrong_and_negative_stack_hints() {
    let session = crate::sexp::session::RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        for hint in [
            Rf_allocVector(SEXPTYPE::INTSXP, 0),
            R_NilValue(),
            Rf_ScalarInteger(-1),
        ] {
            let mut arena = fixture_arena();
            let consts = arena.alloc_vector(SEXPTYPE::VECSXP, 0);
            let body = bcode_with(&mut arena, &[opcodes::OP_RETURN], consts);
            SET_VECTOR_ELT(body, 2, hint);
            let error = assert_r_error(|| {
                bcEval(body, R_BaseEnv());
            });
            assert!(error.message.contains("stack hint"), "{}", error.message);
        }
    });
}

#[test]
fn owned_gnu_instruction_safe_point_keeps_original_pool_snapshot() {
    let session = crate::sexp::session::RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let value = Rf_ScalarInteger(17);
        let pool = Rf_allocVector(SEXPTYPE::VECSXP, 1);
        SET_VECTOR_ELT(pool, 0, value);
        let body = own_operand(gnu_tagged_bcode(
            &[
                super::super::bytecode::GNU_BC_MAX_VERSION,
                super::super::bytecode::GNU_OP_LDCONST,
                0,
                super::super::bytecode::GNU_OP_RETURN,
            ],
            pool,
        ));
        let pool = own_operand(pool);
        let callbacks = std::rc::Rc::new(std::cell::Cell::new(0));
        let observed = callbacks.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            SET_VECTOR_ELT(pool.as_raw(), 0, R_NilValue());
            observed.set(observed.get() + 1);
        }));
        session.with_active_in(|instance| {
            (*instance).gc_state.gc_pending = true;
            (*instance).gc_state.safe_point_collections = 63;
        });
        let result = own_operand(bcEval(body.as_raw(), R_BaseEnv()));
        assert_eq!(callbacks.get(), 1);
        assert_eq!(result.integer_elt(0), Some(17));
    });
}

#[test]
fn owned_gnu_makeclosure_survives_collecting_srcref_and_pool_replacement() {
    let session = crate::sexp::session::RSession::new_for_gc_tests();
    session.with_active(|| unsafe {
        let owner = session.owner_token().unwrap();
        let fb = Rf_allocVector(SEXPTYPE::VECSXP, 3);
        SET_VECTOR_ELT(fb, 0, R_NilValue());
        SET_VECTOR_ELT(fb, 1, Rf_ScalarInteger(7));
        let srcref = Rf_ScalarInteger(91);
        SET_VECTOR_ELT(fb, 2, srcref);
        let pool = Rf_allocVector(SEXPTYPE::VECSXP, 1);
        SET_VECTOR_ELT(pool, 0, fb);
        let bytecode = own_operand(gnu_tagged_bcode(
            &[
                super::super::bytecode::GNU_BC_MAX_VERSION,
                super::super::bytecode::GNU_OP_MAKECLOSURE,
                0,
                super::super::bytecode::GNU_OP_RETURN,
            ],
            pool,
        ));
        let pool_owned = own_operand(pool);
        let callbacks = std::rc::Rc::new(std::cell::Cell::new(0));
        let observed = callbacks.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            SET_VECTOR_ELT(pool_owned.as_raw(), 0, R_NilValue());
            crate::sexp::gengc::full_gc();
            observed.set(observed.get() + 1);
        }));
        session.with_active_in(|instance| {
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
        });
        let result = own_operand(bcEval(bytecode.as_raw(), R_BaseEnv()));
        assert!(callbacks.get() >= 2);
        assert_eq!(result.typeof_(), SEXPTYPE::CLOSXP);
        assert_eq!(result.try_body().unwrap().integer_elt(0), Some(7));
        let symbol = crate::sexp::symbol::Rf_install(c"srcref".as_ptr());
        assert_eq!(
            *INTEGER(crate::attrib_core::getAttrib(result.as_raw(), symbol)),
            91
        );
        owner.full_gc().unwrap();
        assert_eq!(result.typeof_(), SEXPTYPE::CLOSXP);
    });
}
#[test]
fn owned_gnu_colon_keeps_coercions_and_constants_across_collecting_callbacks() {
    let session = crate::sexp::session::RSession::new_without_default_packages();
    session.with_active(|| unsafe {
        let lhs = crate::sexp::constructors::Rf_mkString(c"1".as_ptr());
        let rhs = crate::sexp::constructors::Rf_mkString(c"3".as_ptr());
        let call = Rf_cons(
            crate::sexp::symbol::Rf_install(c":".as_ptr()),
            Rf_cons(lhs, Rf_cons(rhs, R_NilValue())),
        );
        crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
        let pool = Rf_allocVector(SEXPTYPE::VECSXP, 3);
        SET_VECTOR_ELT(pool, 0, call);
        SET_VECTOR_ELT(pool, 1, lhs);
        SET_VECTOR_ELT(pool, 2, rhs);
        let bytecode = own_operand(gnu_tagged_bcode(
            &[
                super::super::bytecode::GNU_BC_MAX_VERSION,
                super::super::bytecode::GNU_OP_LDCONST,
                1,
                super::super::bytecode::GNU_OP_LDCONST,
                2,
                super::super::bytecode::GNU_OP_COLON,
                0,
                super::super::bytecode::GNU_OP_RETURN,
            ],
            pool,
        ));
        let pool_owned = own_operand(pool);
        let callbacks = std::rc::Rc::new(std::cell::Cell::new(0));
        let observed = callbacks.clone();
        crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
            for index in 0..3 {
                SET_VECTOR_ELT(pool_owned.as_raw(), index, R_NilValue());
            }
            crate::sexp::gengc::full_gc();
            observed.set(observed.get() + 1);
        }));
        session.with_active_in(|instance| {
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
        });
        let result = own_operand(bcEval(bytecode.as_raw(), R_BaseEnv()));
        assert!(callbacks.get() >= 2);
        assert_eq!(result.len(), 3);
        assert_eq!(result.integer_elt(0), Some(1));
        assert_eq!(result.integer_elt(1), Some(2));
        assert_eq!(result.integer_elt(2), Some(3));
    });
}
