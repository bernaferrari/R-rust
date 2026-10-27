//! bcEval entry points and interpreter bootstrap — extracted verbatim from eval/bc_eval.rs

use super::*;

// ---------------------------------------------------------------------------
// bcEval — the main bytecode evaluation loop
// ---------------------------------------------------------------------------

/// Evaluate a BCODESXP.
///
/// This is the equivalent of R's `bcEval()` from eval.c.
/// Translate the final owning result to a native projection only after the
/// original runtime remains physically pinned and revocation is checked.
pub unsafe fn bcEval(body: SEXP, rho: SEXP) -> SEXP {
    let body = unsafe { own_operand(body) };
    let rho = unsafe { own_operand(rho) };
    let pin = body
        .pin_runtime()
        .unwrap_or_else(|error| bc_error(error.to_string()))
        .unwrap_or_else(|| bc_error("bytecode requires a runtime owner"));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        bc_eval_owned(body, rho, &pin)
    }));
    // A callback can revoke this exact runtime while native continuation code
    // is unwinding. Keep its physical pin through cleanup and refuse either
    // result or panic publication under a replacement runtime.
    pin.require_live()
        .unwrap_or_else(|error| bc_error(error.to_string()));
    match outcome {
        Ok(result) => result.as_raw(),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

#[inline(never)]
pub(crate) unsafe fn bc_eval_owned(
    body_owned: Sexp<'static>,
    rho_owned: Sexp<'static>,
    pin: &crate::sexp::owner::OwnerPin,
) -> Sexp<'static> {
    unsafe {
        let body = body_owned.as_raw();
        let rho = rho_owned.as_raw();
        if body.is_null() || TYPEOF(body) != SEXPTYPE::BCODESXP {
            bc_error("bcEval requires a bytecode object");
        }

        if BCODE_IS_GNU(body) {
            return eval_gnu_adapter(body, rho, pin);
        }

        bc_eval_private(body_owned, rho_owned, pin)
    }
}

// GNU execution must not reserve the private instruction dispatch's locals.
// Both branches retain the same original roots and scoped owning runtime pin.
#[inline(never)]
pub(crate) unsafe fn bc_eval_private(
    body_owned: Sexp<'static>,
    rho_owned: Sexp<'static>,
    pin: &crate::sexp::owner::OwnerPin,
) -> Sexp<'static> {
    unsafe {
        let body = body_owned.as_raw();
        let rho = rho_owned.as_raw();
        let code_owned = own_operand(VECTOR_ELT(body, 0));
        let code = owned_instruction_words(&code_owned);
        let code_ptr = code.as_slice();
        let consts_owned = own_operand(BCODE_CONSTS(body));
        let consts = consts_owned.as_raw();
        let constants = owned_constant_snapshot(&consts_owned);
        let stack_depth = BCODE_STACK(body);

        if code_ptr.is_empty() {
            bc_error("bytecode object has no instruction stream");
        }

        let code_len = c_int::try_from(code.len())
            .unwrap_or_else(|_| bc_error("bytecode instruction stream is too long"));

        let mut pc: c_int = 0;
        let capacity = usize::try_from(stack_depth)
            .unwrap_or_else(|_| bc_error("negative bytecode stack hint"));
        let mut stack = R_bcstack_t::new(capacity.min(code.len()));
        let mut for_loops: Vec<ForLoopState> = Vec::new();
        let mut loop_stack: Vec<LoopContext> = Vec::new();
        // OP_SETTAG records (stack depth, tag symbol) for the argument
        // values awaiting a call opcode; the call handlers apply them to
        // the rebuilt argument cells (eval.c SETTAG semantics).
        let mut pending_arg_tags: Vec<(usize, Sexp<'static>)> = Vec::new();
        let mut instructions_since_check = 0u8;
        while pc < code_len {
            pin.require_live()
                .unwrap_or_else(|error| bc_error(error.to_string()));
            if instructions_since_check == 0 {
                collect_at_instruction_boundary(pin);
            }
            instructions_since_check = (instructions_since_check + 1) & 63;
            let op = code_ptr[pc as usize];
            pc += 1;

            match op {
                opcodes::OP_PUSHCONST => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "PUSHCONST");
                    let val = owned_constant_at(&constants, idx as i64, "PUSHCONST");
                    // eval.c LDCONST: constants load visible.
                    super::super::runtime::set_visible(TRUE);
                    stack.push(val);
                }

                opcodes::OP_PUSHCONSTARG => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "PUSHCONSTARG");
                    let val = owned_constant_at(&constants, idx as i64, "PUSHCONSTARG");
                    super::super::runtime::set_visible(TRUE);
                    stack.push(val);
                }

                opcodes::OP_GETVAR => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "GETVAR");
                    let sym = owned_constant_at(&constants, idx as i64, "GETVAR");
                    // eval.c DO_GETVAR: variable loads are visible.
                    super::super::runtime::set_visible(TRUE);
                    // Lookup can force a promise or invoke an active binding before
                    // returning; root the operand stack for the lookup itself.
                    let mut val = with_stack_rooted(&stack, sym, || find_var_unforced(sym, rho));
                    if val == R_UnboundValue() {
                        if let Some(primitive) = super::super::eval::primitive_for_symbol(
                            crate::sexp::object::Sexp::from_raw_unchecked(sym),
                        ) {
                            val = primitive.as_raw();
                        } else {
                            bc_unbound_object_error(sym, rho);
                        }
                    }
                    if val == R_MissingArg() {
                        bc_missing_arg_error(sym);
                    } else if TYPEOF(val) == SEXPTYPE::DOTSXP {
                        // A `...` binding is a DOTSXP, never an ordinary
                        // value. The compiler must not emit GETVAR for it
                        // (GNU R handles dots ops as runtime builtins); if a
                        // hand-built bytecode does anyway, fall back to the
                        // AST evaluator instead of pushing the raw DOTSXP.
                        bc_error("'...' used in an invalid context");
                    } else if TYPEOF(val) == SEXPTYPE::PROMSXP {
                        let code = crate::sexp::accessors::PRCODE(val);
                        if code == R_MissingArg() || code.is_null() {
                            bc_missing_arg_error(sym);
                        }
                        let forced =
                            with_stack_rooted(&stack, val, || unsafe { forcePromise(val) });
                        stack.push(forced);
                    } else {
                        stack.push(val);
                    }
                }

                opcodes::OP_MARK_SHARED => {
                    let val = stack_top_checked(&stack, "MARK_SHARED");
                    crate::sexp::accessors::SET_NAMED(val, 2);
                }

                opcodes::OP_BUMP_LINK => {
                    let val = stack_top_checked(&stack, "BUMP_LINK");
                    let n = crate::sexp::accessors::NAMED(val);
                    if n < 3 {
                        crate::sexp::accessors::SET_NAMED(val, n + 1);
                    }
                }

                opcodes::OP_DROP_LINK => {
                    let depth = read_operand(code_ptr, &mut pc, code_len, "DROP_LINK");
                    let slot = stack.depth() - 1 - depth as usize;
                    let val = stack_at_checked(&stack, slot, "DROP_LINK");
                    let n = crate::sexp::accessors::NAMED(val);
                    if n > 0 && n < 3 {
                        crate::sexp::accessors::SET_NAMED(val, n - 1);
                    }
                }

                opcodes::OP_SETVAR => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "SETVAR");
                    let sym = owned_constant_at(&constants, idx as i64, "SETVAR");
                    // GNU R's SETVAR leaves the assigned value on the operand
                    // stack so assignment remains an expression. Blocks and
                    // loops decide separately whether to discard that value.
                    let val = stack_top_checked(&stack, "SETVAR");
                    with_stack_rooted(&stack, val, || defineVar(sym, val, rho));
                    super::super::runtime::set_visible(FALSE);
                }

                opcodes::OP_SETVAR2 => {
                    // GNU R SETVAR2_OP: `<<-` — setVar starting at ENCLOS(rho).
                    // Declared previously without a match arm; falling through
                    // to unknown would refuse loudly, but implementing the
                    // enclosing-frame store closes the declared-but-missing gap.
                    let idx = read_operand(code_ptr, &mut pc, code_len, "SETVAR2");
                    let sym = owned_constant_at(&constants, idx as i64, "SETVAR2");
                    if TYPEOF(sym) != SEXPTYPE::SYMSXP {
                        bc_error("SETVAR2 constant must be a symbol");
                    }
                    let val = stack_top_checked(&stack, "SETVAR2");
                    let parent = ENCLOS(rho);
                    with_stack_rooted(&stack, val, || setVar(sym, val, parent));
                    super::super::runtime::set_visible(FALSE);
                }

                opcodes::OP_DUP => {
                    let val = stack_top_checked(&stack, "DUP");
                    stack.push(val);
                }

                opcodes::OP_POP => {
                    stack_pop_checked(&mut stack, "POP");
                }

                opcodes::OP_PUSHTRUE => {
                    stack.push(Rf_ScalarLogical(TRUE));
                }

                opcodes::OP_PUSHFALSE => {
                    stack.push(Rf_ScalarLogical(FALSE));
                }

                opcodes::OP_PUSHNULL => {
                    stack.push(R_NilValue());
                }

                opcodes::OP_PUSHNIL => {
                    stack.push(R_NilValue());
                }

                opcodes::OP_RETURN => {
                    let result_owned = stack_pop_checked(&mut stack, "RETURN");
                    return result_owned;
                }

                opcodes::OP_visible => {
                    super::super::runtime::set_visible(TRUE);
                }

                opcodes::OPinvisible => {
                    super::super::runtime::set_visible(FALSE);
                }

                opcodes::OP_NOOP => {
                    // No operation
                }

                opcodes::OP_POPAND => {
                    let target = read_jump_target(code_ptr, &mut pc, code_len, "POPAND");
                    let val = stack_top_checked(&stack, "POPAND");
                    let is_false = if !val.is_null() && TYPEOF(val) == SEXPTYPE::LGLSXP {
                        let data = LOGICAL(val);
                        !data.is_null() && *data == 0
                    } else {
                        false
                    };
                    if is_false {
                        pc = target;
                    }
                }

                opcodes::OP_POPOR => {
                    let target = read_jump_target(code_ptr, &mut pc, code_len, "POPOR");
                    let val = stack_top_checked(&stack, "POPOR");
                    let is_true = if !val.is_null() && TYPEOF(val) == SEXPTYPE::LGLSXP {
                        let data = LOGICAL(val);
                        !data.is_null() && *data != 0
                    } else {
                        !val.is_null()
                    };
                    if is_true {
                        pc = target;
                    }
                }

                opcodes::OP_BRANCH => {
                    let target = read_jump_target(code_ptr, &mut pc, code_len, "BRANCH");
                    pc = target;
                }

                opcodes::OP_BRIFNOT => {
                    let target = read_jump_target(code_ptr, &mut pc, code_len, "BRIFNOT");
                    let val_owned = stack_pop_checked(&mut stack, "BRIFNOT");
                    let val = val_owned.as_raw();
                    let cond = eval_bc_condition(val);
                    if !cond {
                        pc = target;
                    }
                }

                opcodes::OP_BRIFTRUE => {
                    let target = read_jump_target(code_ptr, &mut pc, code_len, "BRIFTRUE");
                    let val_owned = stack_pop_checked(&mut stack, "BRIFTRUE");
                    let val = val_owned.as_raw();
                    let cond = eval_bc_condition(val);
                    if cond {
                        pc = target;
                    }
                }

                opcodes::OP_DUP2 => {
                    let s1 = stack.depth();
                    if s1 < 2 {
                        bc_error("DUP2 bytecode stack underflow");
                    }
                    let next = stack_at_checked(&stack, s1 - 2, "DUP2");
                    let top = stack_top_checked(&stack, "DUP2");
                    stack.push(next);
                    stack.push(top);
                }

                opcodes::OP_GOTO => {
                    let target = read_jump_target(code_ptr, &mut pc, code_len, "GOTO");
                    pc = target;
                }

                opcodes::OP_MAKEPROMISE => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "MAKEPROMISE");
                    let expr = owned_constant_at(&constants, idx as i64, "MAKEPROMISE");
                    let prom = crate::sexp::memory_ext::mkPROMSXP(expr, rho);
                    stack.push(prom);
                }

                opcodes::OP_DOMISSING => {}

                opcodes::OP_SETTAG => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "SETTAG");
                    let tag = owned_constant_at(&constants, idx as i64, "SETTAG");
                    if !tag.is_null() {
                        // Drop stale entries whose value was already popped
                        // by an opcode other than a call (defensive).
                        let depth_now = stack.depth();
                        pending_arg_tags.retain(|(d, _)| *d < depth_now);
                        pending_arg_tags.push((depth_now.saturating_sub(1), own_operand(tag)));
                    }
                }
                opcodes::OP_CALL
                | opcodes::OP_CALLBUILTIN
                | opcodes::OP_CALLSPECIAL
                | opcodes::OP_CALL_WITH_SOURCE
                | opcodes::OP_CALL_SYNTAX_WITH_SOURCE => {
                    let nargs = read_operand(code_ptr, &mut pc, code_len, "CALL");
                    let source = if matches!(
                        op,
                        opcodes::OP_CALL_WITH_SOURCE | opcodes::OP_CALL_SYNTAX_WITH_SOURCE
                    ) {
                        let index = read_operand(code_ptr, &mut pc, code_len, "CALL source");
                        Some(own_operand(owned_constant_at(
                            &constants,
                            index as i64,
                            "CALL source",
                        )))
                    } else {
                        None
                    };
                    let nargs = usize::try_from(nargs)
                        .unwrap_or_else(|_| bc_error("CALL has a negative argument count"));
                    let function = stack_pop_checked(&mut stack, "CALL function");
                    let top = stack.depth();
                    if nargs > top {
                        bc_error("CALL argument stack underflow");
                    }
                    let mut arguments = Vec::new();
                    arguments
                        .try_reserve(nargs)
                        .unwrap_or_else(|_| bc_error("cannot allocate bytecode arguments"));
                    for _ in 0..nargs {
                        arguments.push(stack_pop_checked(&mut stack, "CALL argument"));
                    }
                    let tags = private_argument_tags(&mut pending_arg_tags, top, nargs);
                    let call = private_call_owned(
                        &function,
                        &arguments,
                        &tags,
                        &rho_owned,
                        matches!(
                            op,
                            opcodes::OP_CALLSPECIAL | opcodes::OP_CALL_SYNTAX_WITH_SOURCE
                        ),
                        source.as_ref(),
                    );
                    let src = owned_constant_at(&constants, 0, "CALL source");
                    if TYPEOF(src) == SEXPTYPE::LANGSXP {
                        let srcref_symbol = crate::sexp::symbol::Rf_install(c"srcref".as_ptr());
                        let srcref = own_operand(crate::attrib_core::getAttrib(src, srcref_symbol));
                        if srcref.as_raw() != R_NilValue() {
                            crate::attrib_core::setAttrib(
                                call.as_raw(),
                                srcref_symbol,
                                srcref.as_raw(),
                            );
                        }
                    }
                    let attempt = if let Some(source) = source {
                        eval_source_call(&call, &function, &source, &rho_owned, &stack, &loop_stack)
                    } else {
                        eval_nested_call(call.as_raw(), rho, &stack, &loop_stack)
                    };
                    let result = match attempt {
                        Ok(value) => value,
                        Err(jump) => {
                            pc = apply_loop_jump(&mut stack, &mut for_loops, jump);
                            super::super::runtime::set_visible(FALSE);
                            continue;
                        }
                    };
                    pin.require_live()
                        .unwrap_or_else(|error| bc_error(error.to_string()));
                    stack.push_owned(own_operand(result));
                }

                opcodes::OP_REPLACEMENT => {
                    let setter_idx =
                        read_operand(code_ptr, &mut pc, code_len, "REPLACEMENT function");
                    let symbol_idx =
                        read_operand(code_ptr, &mut pc, code_len, "REPLACEMENT object");
                    let count = read_operand(code_ptr, &mut pc, code_len, "REPLACEMENT arguments");
                    let rhs_expr_idx =
                        read_operand(code_ptr, &mut pc, code_len, "REPLACEMENT RHS expression");
                    let count = usize::try_from(count)
                        .unwrap_or_else(|_| bc_error("negative replacement argument count"));
                    if count == 0 || count >= stack.depth() {
                        bc_error("REPLACEMENT argument stack underflow");
                    }
                    let function = own_operand(owned_constant_at(
                        &constants,
                        setter_idx as i64,
                        "REPLACEMENT function",
                    ));
                    let symbol = own_operand(owned_constant_at(
                        &constants,
                        symbol_idx as i64,
                        "REPLACEMENT object",
                    ));
                    if function.typeof_() != SEXPTYPE::SYMSXP
                        || symbol.typeof_() != SEXPTYPE::SYMSXP
                    {
                        bc_error("REPLACEMENT constants must be symbols");
                    }
                    let top = stack.depth();
                    let mut arguments = Vec::new();
                    arguments
                        .try_reserve(count + 1)
                        .unwrap_or_else(|_| bc_error("cannot allocate replacement arguments"));
                    for _ in 0..count {
                        arguments.push(stack_pop_checked(&mut stack, "REPLACEMENT argument"));
                    }
                    let rhs = stack_pop_checked(&mut stack, "REPLACEMENT value");
                    let mut tags = private_argument_tags(&mut pending_arg_tags, top, count);
                    // The RHS was evaluated before the object and other arguments.
                    // Reversely stored call arguments become (object, ..., value).
                    arguments.insert(0, rhs.clone());
                    tags.try_reserve(1)
                        .unwrap_or_else(|_| bc_error("cannot allocate replacement tags"));
                    tags.insert(0, None);
                    let rhs_expression = own_operand(owned_constant_at(
                        &constants,
                        rhs_expr_idx as i64,
                        "REPLACEMENT RHS expression",
                    ));
                    let result = match private_replacement_owned(
                        &function,
                        &arguments,
                        &tags,
                        &rhs_expression,
                        &rho_owned,
                        &loop_stack,
                    ) {
                        Ok(value) => value,
                        Err(jump) => {
                            pc = apply_loop_jump(&mut stack, &mut for_loops, jump);
                            super::super::runtime::set_visible(FALSE);
                            continue;
                        }
                    };
                    pin.require_live()
                        .unwrap_or_else(|error| bc_error(error.to_string()));
                    with_stack_rooted(&stack, result.as_raw(), || {
                        defineVar(symbol.as_raw(), result.as_raw(), rho)
                    });
                    stack.push_owned(rhs);
                    super::super::runtime::set_visible(FALSE);
                }

                opcodes::OP_REPLACEMENT_PATH => {
                    let setter =
                        read_operand(code_ptr, &mut pc, code_len, "REPLACEMENT_PATH function");
                    let root = read_operand(code_ptr, &mut pc, code_len, "REPLACEMENT_PATH root");
                    let field = read_operand(code_ptr, &mut pc, code_len, "REPLACEMENT_PATH field");
                    let scope = read_operand(code_ptr, &mut pc, code_len, "REPLACEMENT_PATH scope");
                    let count =
                        read_operand(code_ptr, &mut pc, code_len, "REPLACEMENT_PATH extras");
                    let expression =
                        read_operand(code_ptr, &mut pc, code_len, "REPLACEMENT_PATH RHS");
                    let count = usize::try_from(count)
                        .unwrap_or_else(|_| bc_error("negative replacement path argument count"));
                    if count >= stack.depth() {
                        bc_error("REPLACEMENT_PATH argument stack underflow");
                    }
                    let function = own_operand(owned_constant_at(
                        &constants,
                        setter as i64,
                        "REPLACEMENT_PATH function",
                    ));
                    let symbol = own_operand(owned_constant_at(
                        &constants,
                        root as i64,
                        "REPLACEMENT_PATH root",
                    ));
                    let field = own_operand(owned_constant_at(
                        &constants,
                        field as i64,
                        "REPLACEMENT_PATH field",
                    ));
                    let expression = own_operand(owned_constant_at(
                        &constants,
                        expression as i64,
                        "REPLACEMENT_PATH RHS",
                    ));
                    if function.typeof_() != SEXPTYPE::SYMSXP
                        || symbol.typeof_() != SEXPTYPE::SYMSXP
                        || (!field.is_nil() && field.typeof_() != SEXPTYPE::SYMSXP)
                        || !matches!(scope, 0 | 1)
                    {
                        bc_error("invalid REPLACEMENT_PATH constants or scope");
                    }
                    let top = stack.depth();
                    let mut arguments = Vec::new();
                    arguments
                        .try_reserve(count + 1)
                        .unwrap_or_else(|_| bc_error("cannot allocate replacement path arguments"));
                    for _ in 0..count {
                        arguments.push(stack_pop_checked(&mut stack, "REPLACEMENT_PATH extra"));
                    }
                    let rhs = stack_pop_checked(&mut stack, "REPLACEMENT_PATH RHS");
                    let mut tags = private_argument_tags(&mut pending_arg_tags, top, count);
                    arguments.insert(0, rhs.clone());
                    tags.try_reserve(1)
                        .unwrap_or_else(|_| bc_error("cannot allocate replacement path tags"));
                    tags.insert(0, None);
                    match private_replacement_path_owned(
                        &function,
                        &symbol,
                        &field,
                        scope == 1,
                        &mut arguments,
                        &mut tags,
                        &expression,
                        &rho_owned,
                        &loop_stack,
                    ) {
                        Ok(_changed) => {}
                        Err(jump) => {
                            pc = apply_loop_jump(&mut stack, &mut for_loops, jump);
                            super::super::runtime::set_visible(FALSE);
                            continue;
                        }
                    }
                    pin.require_live()
                        .unwrap_or_else(|error| bc_error(error.to_string()));
                    stack.push_owned(rhs);
                    super::super::runtime::set_visible(FALSE);
                }

                opcodes::OP_REPLACEMENT_CHAIN => {
                    let root = read_operand(code_ptr, &mut pc, code_len, "REPLACEMENT_CHAIN root");
                    let scope =
                        read_operand(code_ptr, &mut pc, code_len, "REPLACEMENT_CHAIN scope");
                    let expression =
                        read_operand(code_ptr, &mut pc, code_len, "REPLACEMENT_CHAIN RHS");
                    let count =
                        read_operand(code_ptr, &mut pc, code_len, "REPLACEMENT_CHAIN levels");
                    let count = usize::try_from(count)
                        .unwrap_or_else(|_| bc_error("negative replacement level count"));
                    let remaining = usize::try_from(code_len - pc).unwrap_or(0);
                    if count == 0 || count > remaining / 3 || !matches!(scope, 0 | 1) {
                        bc_error("invalid REPLACEMENT_CHAIN framing or scope");
                    }
                    let root = own_operand(owned_constant_at(
                        &constants,
                        root as i64,
                        "REPLACEMENT_CHAIN root",
                    ));
                    if root.typeof_() != SEXPTYPE::SYMSXP {
                        bc_error("REPLACEMENT_CHAIN root must be a symbol");
                    }
                    let expression = own_operand(owned_constant_at(
                        &constants,
                        expression as i64,
                        "REPLACEMENT_CHAIN RHS",
                    ));
                    let mut levels = Vec::new();
                    levels
                        .try_reserve(count)
                        .unwrap_or_else(|_| bc_error("cannot allocate replacement levels"));
                    for _ in 0..count {
                        let getter =
                            read_operand(code_ptr, &mut pc, code_len, "REPLACEMENT_CHAIN getter");
                        let setter =
                            read_operand(code_ptr, &mut pc, code_len, "REPLACEMENT_CHAIN setter");
                        let extras = read_operand(
                            code_ptr,
                            &mut pc,
                            code_len,
                            "REPLACEMENT_CHAIN extra count",
                        );
                        let extras = usize::try_from(extras)
                            .unwrap_or_else(|_| bc_error("negative replacement extra count"));
                        if extras > usize::try_from(code_len - pc).unwrap_or(0) / 2 {
                            bc_error("truncated replacement extra operands");
                        }
                        let getter = own_operand(owned_constant_at(
                            &constants,
                            getter as i64,
                            "REPLACEMENT_CHAIN getter",
                        ));
                        let setter = own_operand(owned_constant_at(
                            &constants,
                            setter as i64,
                            "REPLACEMENT_CHAIN setter",
                        ));
                        if !super::private_calls::is_replacement_function_head(&getter)
                            || !super::private_calls::is_replacement_function_head(&setter)
                        {
                            bc_error("invalid REPLACEMENT_CHAIN function head");
                        }
                        let mut arguments = Vec::new();
                        arguments.try_reserve(extras).unwrap_or_else(|_| {
                            bc_error("cannot allocate replacement extra syntax")
                        });
                        for _ in 0..extras {
                            let expression = read_operand(
                                code_ptr,
                                &mut pc,
                                code_len,
                                "REPLACEMENT_CHAIN extra expression",
                            );
                            let tag = read_operand(
                                code_ptr,
                                &mut pc,
                                code_len,
                                "REPLACEMENT_CHAIN extra tag",
                            );
                            let expression = own_operand(owned_constant_at(
                                &constants,
                                expression as i64,
                                "REPLACEMENT_CHAIN extra expression",
                            ));
                            let tag = own_operand(owned_constant_at(
                                &constants,
                                tag as i64,
                                "REPLACEMENT_CHAIN extra tag",
                            ));
                            if !tag.is_nil() && tag.typeof_() != SEXPTYPE::SYMSXP {
                                bc_error("REPLACEMENT_CHAIN extra tag must be a symbol or nil");
                            }
                            arguments.push((expression, tag));
                        }
                        levels.push(ReplacementLevel {
                            getter,
                            setter,
                            extras: arguments,
                        });
                    }
                    let rhs = stack_pop_checked(&mut stack, "REPLACEMENT_CHAIN RHS");
                    match private_replacement_chain_owned(
                        &root,
                        scope == 1,
                        &levels,
                        &rhs,
                        &expression,
                        &rho_owned,
                        &loop_stack,
                    ) {
                        Ok(_changed) => {}
                        Err(jump) => {
                            pc = apply_loop_jump(&mut stack, &mut for_loops, jump);
                            super::super::runtime::set_visible(FALSE);
                            continue;
                        }
                    }
                    pin.require_live()
                        .unwrap_or_else(|error| bc_error(error.to_string()));
                    stack.push_owned(rhs);
                    super::super::runtime::set_visible(FALSE);
                }

                opcodes::OP_STARTASSIGN => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "STARTASSIGN");
                    let sym = owned_constant_at(&constants, idx as i64, "STARTASSIGN");
                    // Lookup can force a promise or invoke an active binding before
                    // returning; root the operand stack for the lookup itself.
                    let val = with_stack_rooted(&stack, sym, || R_findVar(sym, rho));
                    stack.push(sym);
                    stack.push(val);
                }

                opcodes::OP_ENDASSIGN => {
                    let _nargs = read_operand(code_ptr, &mut pc, code_len, "ENDASSIGN");
                    let val_owned = stack_pop_checked(&mut stack, "ENDASSIGN value");
                    let val = val_owned.as_raw();
                    let sym_owned = stack_pop_checked(&mut stack, "ENDASSIGN symbol");
                    let sym = sym_owned.as_raw();
                    with_stack_rooted(&stack, val, || defineVar(sym, val, rho));
                    stack.push(val);
                    super::super::runtime::set_visible(FALSE);
                }

                opcodes::OP_STEPFOR => {
                    let target = read_jump_target(code_ptr, &mut pc, code_len, "STEPFOR");
                    let depth = stack.depth();
                    if depth < 3 {
                        pc = target;
                    } else {
                        let seq_val = stack_at_checked(&stack, depth - 1, "STEPFOR sequence");
                        let loop_var = stack_at_checked(&stack, depth - 2, "STEPFOR loop symbol");
                        let ctr_ptr = stack_at_checked(&stack, depth - 3, "STEPFOR counter");

                        if seq_val.is_null() || ctr_ptr.is_null() {
                            pc = target;
                        } else {
                            let mut seq_len: c_int = 0;
                            if TYPEOF(seq_val) == SEXPTYPE::INTSXP
                                || TYPEOF(seq_val) == SEXPTYPE::REALSXP
                            {
                                seq_len = LENGTH(seq_val);
                            }
                            let ctr = *INTEGER(ctr_ptr);
                            if ctr + 1 >= seq_len {
                                pc = target;
                            } else {
                                *INTEGER(ctr_ptr) = ctr + 1;
                                let idx = (ctr + 1) as usize;
                                if TYPEOF(seq_val) == SEXPTYPE::INTSXP {
                                    let data = INTEGER(seq_val);
                                    if !data.is_null() {
                                        defineVar(loop_var, Rf_ScalarInteger(*data.add(idx)), rho);
                                    }
                                } else if TYPEOF(seq_val) == SEXPTYPE::REALSXP {
                                    let data = REAL(seq_val);
                                    if !data.is_null() {
                                        defineVar(loop_var, Rf_ScalarReal(*data.add(idx)), rho);
                                    }
                                }
                            }
                        }
                    }
                }

                opcodes::OP_BREAK => {
                    let target = read_jump_target(code_ptr, &mut pc, code_len, "BREAK");
                    pc = target;
                }

                opcodes::OP_NEXTITER => {
                    let target = read_jump_target(code_ptr, &mut pc, code_len, "NEXTITER");
                    pc = target;
                }

                opcodes::OP_SETLOOPCTR => {
                    let ctr_owned = stack_pop_checked(&mut stack, "SETLOOPCTR");
                    let ctr = ctr_owned.as_raw();
                    if TYPEOF(ctr) == SEXPTYPE::INTSXP {
                        let d = INTEGER(ctr);
                        if !d.is_null() {
                            *d = -1;
                        }
                    }
                    stack.push(ctr);
                }

                opcodes::OP_BEGINLOOP => {
                    // eval.c STARTLOOPCNTXT: record the exit targets and the
                    // operand-stack/for-state depths at loop entry, so a
                    // break/next from mid-body can unwind that state.
                    let break_target = read_jump_target(code_ptr, &mut pc, code_len, "BEGINLOOP");
                    let next_target = read_jump_target(code_ptr, &mut pc, code_len, "BEGINLOOP");
                    loop_stack.push(LoopContext {
                        break_target,
                        next_target,
                        stack_depth: stack.depth(),
                        for_depth: for_loops.len(),
                    });
                }

                opcodes::OP_ENDLOOP => {
                    // eval.c ENDLOOPCNTXT: normal loop exit pops the context.
                    if loop_stack.pop().is_none() {
                        bc_error("ENDLOOP bytecode with no active loop context");
                    }
                }

                opcodes::OP_HIDDENCALL => {
                    let _nargs = read_operand(code_ptr, &mut pc, code_len, "HIDDENCALL");
                    let fun_owned = stack_pop_checked(&mut stack, "HIDDENCALL function");
                    let fun = fun_owned.as_raw();
                    let args_owned = stack_pop_checked(&mut stack, "HIDDENCALL arguments");
                    let args = args_owned.as_raw();
                    if fun.is_null() {
                        stack.push(R_NilValue());
                    } else {
                        let call = Rf_lang2(fun, args);
                        let result = match eval_nested_call(call, rho, &stack, &loop_stack) {
                            Ok(value) => value,
                            Err(jump) => {
                                pc = apply_loop_jump(&mut stack, &mut for_loops, jump);
                                super::super::runtime::set_visible(FALSE);
                                continue;
                            }
                        };
                        stack.push(result);
                    }
                    super::super::runtime::set_visible(FALSE);
                }

                opcodes::OP_PUSHARG => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "PUSHARG");
                    let val = owned_constant_at(&constants, idx as i64, "PUSHARG");
                    stack.push(val);
                }

                opcodes::OP_PUSHFUN => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "PUSHFUN");
                    let val = owned_constant_at(&constants, idx as i64, "PUSHFUN");
                    stack.push(val);
                }

                opcodes::OP_PUSHNILVALUE => {
                    stack.push(R_NilValue());
                }

                opcodes::OP_DFLTFUN => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "DFLTFUN");
                    let val = owned_constant_at(&constants, idx as i64, "DFLTFUN");
                    stack.push(val);
                }

                opcodes::OP_DFLTFORM => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "DFLTFORM");
                    let val = owned_constant_at(&constants, idx as i64, "DFLTFORM");
                    stack.push(val);
                }

                opcodes::OP_STARTFOR => {
                    let symbol_idx = read_operand(code_ptr, &mut pc, code_len, "STARTFOR");
                    let end = read_jump_target(code_ptr, &mut pc, code_len, "STARTFOR");
                    let symbol = owned_constant_at(&constants, symbol_idx as i64, "STARTFOR");
                    if TYPEOF(symbol) != SEXPTYPE::SYMSXP {
                        bc_error("STARTFOR requires a symbol constant");
                    }
                    let mut sequence = stack_top_checked(&stack, "STARTFOR sequence");
                    if crate::mainutils::essentials::sexp_has_class(sequence, "factor") {
                        sequence = with_stack_rooted(&stack, symbol, || {
                            crate::mainutils::coerce::asCharacterFactor(sequence)
                        });
                        let sequence_slot = stack.depth() - 1;
                        stack.set(sequence_slot, sequence);
                    }
                    let Some(length) = for_sequence_length(sequence) else {
                        bc_error("invalid for() loop sequence");
                    };
                    defineVar(symbol, R_NilValue(), rho);
                    if length == 0 {
                        stack_pop_checked(&mut stack, "STARTFOR empty sequence");
                        stack.push(R_NilValue());
                        pc = end;
                    } else {
                        bind_for_element(&stack, sequence, symbol, 0, rho);
                        for_loops.push(ForLoopState {
                            sequence_slot: stack.depth() - 1,
                            symbol: own_operand(symbol),
                            index: 0,
                            length,
                        });
                    }
                }

                opcodes::OP_NEXTFOR => {
                    crate::sexp::instance::check_cancellation();
                    let body_start = read_jump_target(code_ptr, &mut pc, code_len, "NEXTFOR");
                    let Some(state) = for_loops.last_mut() else {
                        bc_error("NEXTFOR has no active for loop");
                    };
                    state.index += 1;
                    if state.index < state.length {
                        let sequence =
                            stack_at_checked(&stack, state.sequence_slot, "NEXTFOR sequence");
                        bind_for_element(&stack, sequence, state.symbol.as_raw(), state.index, rho);
                        pc = body_start;
                    } else {
                        let completed = for_loops.pop().expect("active loop checked above");
                        if stack.depth() != completed.sequence_slot + 1 {
                            bc_error("NEXTFOR bytecode stack is unbalanced");
                        }
                        stack_pop_checked(&mut stack, "NEXTFOR sequence");
                        stack.push(R_NilValue());
                        super::super::runtime::set_visible(FALSE);
                    }
                }

                opcodes::OP_STARTSUBSET | opcodes::OP_ENDSUBSET => {
                    let nargs = read_operand(code_ptr, &mut pc, code_len, "SUBSET");
                    if nargs < 0 {
                        bc_error("SUBSET has a negative argument count");
                    }
                    let _idx = read_operand(code_ptr, &mut pc, code_len, "SUBSET");
                    // x[i, j, ...] — pop nargs index values, then the object
                    let mut indices: Vec<Sexp<'static>> = Vec::new();
                    for _ in 0..nargs {
                        indices.push(stack_pop_checked(&mut stack, "SUBSET index"));
                    }
                    let obj_owned = stack_pop_checked(&mut stack, "SUBSET object");
                    let obj = obj_owned.as_raw();
                    if obj.is_null() || indices.is_empty() {
                        stack.push(R_NilValue());
                    } else {
                        // Call `[`(obj, i, j, ...)
                        {
                            let bracket_sym = crate::sexp::symbol::Rf_install(c"[".as_ptr());
                            let nil = R_NilValue();
                            let mut arg_list_owned = own_operand(nil);
                            let mut arg_list = arg_list_owned.as_raw();
                            // Build args in reverse order
                            for idx_expr in indices.into_iter().rev() {
                                arg_list_owned = own_operand(Rf_cons(idx_expr.as_raw(), arg_list));
                                arg_list = arg_list_owned.as_raw();
                            }
                            // Prepend the object
                            arg_list_owned = own_operand(Rf_cons(obj, arg_list));
                            arg_list = arg_list_owned.as_raw();
                            let call = Rf_cons(bracket_sym, arg_list);
                            if !call.is_null() {
                                crate::sexp::accessors::SET_TYPEOF(
                                    call,
                                    SEXPTYPE::LANGSXP.as_c_int(),
                                );
                            }
                            let result = with_stack_rooted(&stack, call, || unsafe {
                                crate::eval::eval::Rf_eval(call, rho)
                            });
                            stack.push(result);
                        }
                    }
                }

                opcodes::OP_STARTSUBSET2 | opcodes::OP_ENDSUBSET2 => {
                    let nargs = read_operand(code_ptr, &mut pc, code_len, "SUBSET2");
                    if nargs < 0 {
                        bc_error("SUBSET2 has a negative argument count");
                    }
                    let _idx = read_operand(code_ptr, &mut pc, code_len, "SUBSET2");
                    // x[[i]] — pop index value, then the object
                    let idx = if nargs > 0 {
                        stack_pop_checked(&mut stack, "SUBSET2 index")
                    } else {
                        own_operand(R_NilValue())
                    };
                    let idx_owned = idx;
                    let idx = idx_owned.as_raw();
                    let obj_owned = stack_pop_checked(&mut stack, "SUBSET2 object");
                    let obj = obj_owned.as_raw();
                    if obj.is_null() {
                        stack.push(R_NilValue());
                    } else {
                        {
                            let dbracket_sym = crate::sexp::symbol::Rf_install(c"[[".as_ptr());
                            let nil = R_NilValue();
                            let args = Rf_cons(obj, Rf_cons(idx, nil));
                            let call = Rf_cons(dbracket_sym, args);
                            if !call.is_null() {
                                crate::sexp::accessors::SET_TYPEOF(
                                    call,
                                    SEXPTYPE::LANGSXP.as_c_int(),
                                );
                            }
                            let result = with_stack_rooted(&stack, call, || unsafe {
                                crate::eval::eval::Rf_eval(call, rho)
                            });
                            stack.push(result);
                        }
                    }
                }

                opcodes::OP_LDCLOSURE => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "LDCLOSURE");
                    let val = owned_constant_at(&constants, idx as i64, "LDCLOSURE");
                    stack.push(val);
                }
                opcodes::OP_MAKECLOSURE => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "MAKECLOSURE");
                    let fb = owned_constant_at(&constants, idx as i64, "MAKECLOSURE");
                    if TYPEOF(fb) != SEXPTYPE::VECSXP || LENGTH(fb) < 2 {
                        bc_error("MAKECLOSURE constant is not a formals/body pairlist");
                    }
                    let value = with_stack_rooted(&stack, fb, || {
                        crate::mainutils::dstruct::mkCLOSXP(
                            VECTOR_ELT(fb, 0),
                            VECTOR_ELT(fb, 1),
                            rho,
                        )
                    });
                    super::super::runtime::set_visible(TRUE);
                    stack.push(value);
                }

                opcodes::OP_CLOSEDEXPR => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "CLOSEDEXPR");
                    let val = owned_constant_at(&constants, idx as i64, "CLOSEDEXPR");
                    stack.push(val);
                }

                opcodes::OP_MAKEACTIVE => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "MAKEACTIVE");
                    let val = owned_constant_at(&constants, idx as i64, "MAKEACTIVE");
                    stack.push(val);
                }

                opcodes::OP_SWASTORE => {
                    let _idx = read_operand(code_ptr, &mut pc, code_len, "SWASTORE");
                    let val_owned = stack_pop_checked(&mut stack, "SWASTORE");
                    let val = val_owned.as_raw();
                    stack.push(val); // simplified: just pass through
                }

                opcodes::OP_SWLOAD => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "SWLOAD");
                    let _switch_val_owned = stack_pop_checked(&mut stack, "SWLOAD");
                    let _switch_val = _switch_val_owned.as_raw();
                    let val = owned_constant_at(&constants, idx as i64, "SWLOAD");
                    stack.push(val);
                }

                opcodes::OP_PUTBASE => {
                    let _nargs = read_operand(code_ptr, &mut pc, code_len, "PUTBASE");
                    let val_owned = stack_pop_checked(&mut stack, "PUTBASE value");
                    let val = val_owned.as_raw();
                    let sym_owned = stack_pop_checked(&mut stack, "PUTBASE symbol");
                    let sym = sym_owned.as_raw();
                    if !sym.is_null() && TYPEOF(sym) == SEXPTYPE::SYMSXP {
                        {
                            with_stack_rooted(&stack, val, || {
                                defineVar(sym, val, super::super::runtime::base_env())
                            });
                        }
                    }
                    stack.push(val);
                    super::super::runtime::set_visible(FALSE);
                }

                opcodes::OP_PUTBASE_SEP => {
                    let _nargs = read_operand(code_ptr, &mut pc, code_len, "PUTBASE_SEP");
                    let val_owned = stack_pop_checked(&mut stack, "PUTBASE_SEP value");
                    let val = val_owned.as_raw();
                    let sym_owned = stack_pop_checked(&mut stack, "PUTBASE_SEP symbol");
                    let sym = sym_owned.as_raw();
                    if !sym.is_null() && TYPEOF(sym) == SEXPTYPE::SYMSXP {
                        {
                            with_stack_rooted(&stack, val, || {
                                defineVar(sym, val, super::super::runtime::base_env())
                            });
                        }
                    }
                    stack.push(val);
                }

                opcodes::OP_SEQBEGIN | opcodes::OP_SEQEND => {
                    let _idx = read_operand(code_ptr, &mut pc, code_len, "SEQ");
                }

                _ => {
                    bc_mismatch(format!(
                        "unknown private-dialect bytecode opcode {} at pc {} (OP_LAST={})",
                        op,
                        pc - 1,
                        opcodes::OP_LAST
                    ));
                }
            }
        }

        stack_pop_checked(&mut stack, "end of bytecode")
    }
}

// ---------------------------------------------------------------------------
// R_initialize_bcode — initialize bytecode system
// ---------------------------------------------------------------------------

/// Initialize the bytecode evaluation system.
pub unsafe fn R_initialize_bcode() {
    // In the full implementation, this sets up the bytecode interpreter
    // and registers the compiler.
}
