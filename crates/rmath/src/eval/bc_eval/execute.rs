//! GNU opcode dispatch trampoline and the ~2000-line eval_gnu_adapter instruction interpreter (named size exception) — extracted verbatim from eval/bc_eval.rs

use super::*;

// A single debug frame must not reserve the temporaries of every opcode
// while an instruction calls another compiled R closure. Each ordinary arm
// owns its existing locals for one instruction; release builds inline it.
#[inline]
pub(crate) fn execute_gnu_instruction(f: impl FnOnce()) {
    f();
}

pub(crate) unsafe fn eval_gnu_adapter(
    body: SEXP,
    rho: SEXP,
    pin: &crate::sexp::owner::OwnerPin,
) -> Sexp<'static> {
    unsafe {
        let code_owned = own_operand(VECTOR_ELT(body, 0));
        let code_vec = code_owned.as_raw();
        let consts_owned = own_operand(BCODE_CONSTS(body));
        let consts = consts_owned.as_raw();
        let constants = owned_constant_snapshot(&consts_owned);
        if code_vec.is_null() || TYPEOF(code_vec) != SEXPTYPE::INTSXP {
            bc_error("GNU bytecode object has no instruction stream");
        }
        if consts.is_null() || TYPEOF(consts) != SEXPTYPE::VECSXP {
            bc_error("GNU bytecode object has no constant pool");
        }
        let owned_words = owned_instruction_words(&code_owned);
        let words = owned_words.as_slice();
        let (supported, switches) =
            super::super::bytecode::validate_gnu_adapter_with_owned_constants(words, &constants)
                .unwrap_or_else(|message| bc_error(message));
        if !supported {
            bc_mismatch("unsupported tagged GNU bytecode stream (outside bounded adapter)");
        }

        let mut pc = 1usize;
        let mut prot_top: usize = 0;
        let mut stack = R_bcstack_t::new(4);
        let mut for_loops: Vec<GnuForLoopState> = Vec::new();
        let mut loop_stack: Vec<LoopContext> = Vec::new();
        // GNU GETFUN starts a call frame.  MAKEPROM appends lazy arguments
        // until CALL consumes that frame.  Keeping the marker separate from
        // the operand stack lets the validator and runtime reject malformed
        // streams without guessing an argument count from the call object.
        let mut gnu_call_frames: Vec<GnuCallFrame> = Vec::new();
        let mut instructions_since_check = 0u8;
        while pc < words.len() {
            pin.require_live()
                .unwrap_or_else(|error| bc_error(error.to_string()));
            if instructions_since_check == 0 {
                collect_at_instruction_boundary(pin);
            }
            instructions_since_check = (instructions_since_check + 1) & 63;
            let opcode = words[pc];
            pc += 1;
            match opcode {
                super::super::bytecode::GNU_OP_RETURN => {
                    return stack_pop_checked(&mut stack, "GNU RETURN");
                }
                super::super::bytecode::GNU_OP_SWITCH => execute_gnu_instruction(|| {
                    let table = switches
                        .get(&(pc - 1))
                        .unwrap_or_else(|| bc_error("missing checked SWITCH table"));
                    let value_owned = stack_pop_checked(&mut stack, "GNU SWITCH");
                    pc = table
                        .select(&value_owned, pin)
                        .unwrap_or_else(|error| bc_error(error));
                }),
                super::super::bytecode::GNU_OP_BRIFNOT => execute_gnu_instruction(|| {
                    let _call_index = words[pc];
                    let target = words[pc + 1] as usize;
                    pc += 2;
                    let condition_owned = stack_pop_checked(&mut stack, "GNU BRIFNOT");
                    let condition = condition_owned.as_raw();
                    let branch =
                        with_stack_rooted(&stack, condition, || eval_bc_condition(condition));
                    if !branch {
                        pc = target;
                    }
                }),
                super::super::bytecode::GNU_OP_AND1ST => execute_gnu_instruction(|| {
                    let call_index = words[pc] as usize;
                    let target = words[pc + 1] as usize;
                    pc += 2;
                    let call = owned_constant_at(&constants, call_index as i64, "GNU opcode");
                    let value = stack_top_checked(&stack, "GNU AND1ST");
                    let val = with_stack_rooted(&stack, value, || {
                        fixup_scalar_logical(value, call, "'x'", "&&")
                    });
                    let index = stack.depth() - 1;
                    let boxed = with_stack_rooted(&stack, R_NilValue(), || Rf_ScalarLogical(val));
                    stack.set(index, boxed);
                    super::super::runtime::set_visible(TRUE);
                    if val == FALSE {
                        pc = target;
                    }
                }),
                super::super::bytecode::GNU_OP_AND2ND => execute_gnu_instruction(|| {
                    let call_index = words[pc] as usize;
                    pc += 1;
                    let call = owned_constant_at(&constants, call_index as i64, "GNU opcode");
                    let value_owned = stack_pop_checked(&mut stack, "GNU AND2ND");
                    let value = value_owned.as_raw();
                    let val = with_stack_rooted(&stack, value, || {
                        fixup_scalar_logical(value, call, "'y'", "&&")
                    });
                    if val == FALSE || val == NA_LOGICAL {
                        let index = stack.depth() - 1;
                        let result =
                            with_stack_rooted(&stack, R_NilValue(), || Rf_ScalarLogical(val));
                        stack.set(index, result);
                    }
                    super::super::runtime::set_visible(TRUE);
                }),
                super::super::bytecode::GNU_OP_OR1ST => execute_gnu_instruction(|| {
                    let call_index = words[pc] as usize;
                    let target = words[pc + 1] as usize;
                    pc += 2;
                    let call = owned_constant_at(&constants, call_index as i64, "GNU opcode");
                    let value = stack_top_checked(&stack, "GNU OR1ST");
                    let val = with_stack_rooted(&stack, value, || {
                        fixup_scalar_logical(value, call, "'x'", "||")
                    });
                    let index = stack.depth() - 1;
                    let boxed = with_stack_rooted(&stack, R_NilValue(), || Rf_ScalarLogical(val));
                    stack.set(index, boxed);
                    super::super::runtime::set_visible(TRUE);
                    if val != NA_LOGICAL && val != FALSE {
                        pc = target;
                    }
                }),
                super::super::bytecode::GNU_OP_OR2ND => execute_gnu_instruction(|| {
                    let call_index = words[pc] as usize;
                    pc += 1;
                    let call = owned_constant_at(&constants, call_index as i64, "GNU opcode");
                    let value_owned = stack_pop_checked(&mut stack, "GNU OR2ND");
                    let value = value_owned.as_raw();
                    let val = with_stack_rooted(&stack, value, || {
                        fixup_scalar_logical(value, call, "'y'", "||")
                    });
                    if val != FALSE {
                        let index = stack.depth() - 1;
                        let result =
                            with_stack_rooted(&stack, R_NilValue(), || Rf_ScalarLogical(val));
                        stack.set(index, result);
                    }
                    super::super::runtime::set_visible(TRUE);
                }),
                super::super::bytecode::GNU_OP_INVISIBLE => execute_gnu_instruction(|| {
                    super::super::runtime::set_visible(FALSE);
                }),
                super::super::bytecode::GNU_OP_PRINTVALUE => execute_gnu_instruction(|| {
                    let value_owned = stack_pop_checked(&mut stack, "GNU PRINTVALUE");
                    let value = value_owned.as_raw();
                    with_stack_rooted(&stack, value, || {
                        crate::mainutils::print::PrintValue(value);
                    });
                }),
                super::super::bytecode::GNU_OP_SETLOOPVAL => execute_gnu_instruction(|| {
                    if stack.depth() < 2 {
                        bc_error("GNU SETLOOPVAL has stack depth < 2");
                    }
                    let __owned = stack_pop_checked(&mut stack, "GNU SETLOOPVAL");
                    let _ = __owned.as_raw();
                    let top = stack.depth() - 1;
                    stack.set(top, R_NilValue());
                }),
                super::super::bytecode::GNU_OP_DOTSERR => execute_gnu_instruction(|| {
                    bc_error("'...' used in an incorrect context");
                }),
                super::super::bytecode::GNU_OP_DUP => execute_gnu_instruction(|| {
                    let value = stack_top_checked(&stack, "GNU DUP");
                    stack.push(value);
                }),
                super::super::bytecode::GNU_OP_DUP2ND => execute_gnu_instruction(|| {
                    if stack.depth() < 2 {
                        bc_error("GNU DUP2ND has an empty stack");
                    }
                    let value = stack_at_checked(&stack, stack.depth() - 2, "GNU DUP2ND");
                    stack.push(value);
                }),
                super::super::bytecode::GNU_OP_POP => execute_gnu_instruction(|| {
                    stack_pop_checked(&mut stack, "GNU POP");
                }),
                super::super::bytecode::GNU_OP_GOTO => execute_gnu_instruction(|| {
                    let target = words[pc] as usize;
                    pc = target;
                }),
                super::super::bytecode::GNU_OP_LDCONST => execute_gnu_instruction(|| {
                    let index = words[pc] as usize;
                    pc += 1;
                    super::super::runtime::set_visible(TRUE);
                    stack.push(owned_constant_at(&constants, index as i64, "GNU opcode"));
                }),
                super::super::bytecode::GNU_OP_LDNULL => execute_gnu_instruction(|| {
                    super::super::runtime::set_visible(TRUE);
                    stack.push(R_NilValue());
                }),
                super::super::bytecode::GNU_OP_LDTRUE => execute_gnu_instruction(|| {
                    super::super::runtime::set_visible(TRUE);
                    let value = with_stack_rooted(&stack, R_NilValue(), || Rf_ScalarLogical(TRUE));
                    stack.push(value);
                }),
                super::super::bytecode::GNU_OP_LDFALSE => execute_gnu_instruction(|| {
                    super::super::runtime::set_visible(TRUE);
                    let value = with_stack_rooted(&stack, R_NilValue(), || Rf_ScalarLogical(FALSE));
                    stack.push(value);
                }),
                super::super::bytecode::GNU_OP_GETVAR
                | super::super::bytecode::GNU_OP_GETVAR_MISSOK => execute_gnu_instruction(|| {
                    let index = words[pc] as usize;
                    pc += 1;
                    let symbol = owned_constant_at(&constants, index as i64, "GNU opcode");
                    if TYPEOF(symbol) != SEXPTYPE::SYMSXP {
                        bc_error(format!(
                            "GNU GETVAR constant pool entry {index} is not a symbol"
                        ));
                    }
                    super::super::runtime::set_visible(TRUE);
                    let keep_missing = opcode == super::super::bytecode::GNU_OP_GETVAR_MISSOK;
                    let value = with_stack_rooted(&stack, symbol, || {
                        eval_gnu_getvar(symbol, rho, keep_missing, false)
                    });
                    stack.push(value);
                }),
                super::super::bytecode::GNU_OP_VISIBLE => execute_gnu_instruction(|| {
                    super::super::runtime::set_visible(TRUE);
                }),
                super::super::bytecode::GNU_OP_INCLNK => execute_gnu_instruction(|| {
                    let value = stack_top_checked(&stack, "GNU INCLNK");
                    increment_named_link(value);
                }),
                super::super::bytecode::GNU_OP_DECLNK => execute_gnu_instruction(|| {
                    if stack.depth() < 2 {
                        bc_error("GNU DECLNK has fewer than two stack values");
                    }
                    let value = stack.at(stack.depth() - 2);
                    decrement_named_link(value);
                }),
                super::super::bytecode::GNU_OP_DECLNK_N => execute_gnu_instruction(|| {
                    let count = words[pc] as usize;
                    pc += 1;
                    if stack.depth() < count + 2 {
                        bc_error("GNU DECLNK_N count exceeds the stack depth");
                    }
                    for i in 0..count {
                        let value = stack.at(stack.depth() - 2 - i);
                        decrement_named_link(value);
                    }
                }),
                super::super::bytecode::GNU_OP_INCLNKSTK => execute_gnu_instruction(|| {
                    // GNU saves the protection-top offset and pushes it.
                    // It does not read a stack value; the stack may be empty.
                    let offset = prot_top;
                    prot_top = stack.depth();
                    let marker = with_stack_rooted(&stack, R_NilValue(), || {
                        crate::sexp::constructors::Rf_ScalarInteger(offset as c_int)
                    });
                    stack.push(marker);
                }),
                super::super::bytecode::GNU_OP_DECLNKSTK => execute_gnu_instruction(|| {
                    if stack.depth() < 2 {
                        bc_error("GNU DECLNKSTK has fewer than two stack values");
                    }
                    let marker = stack.at(stack.depth() - 2);
                    if TYPEOF(marker) != SEXPTYPE::INTSXP || LENGTH(marker) != 1 {
                        bc_error("GNU DECLNKSTK marker is missing");
                    }
                    let offset = crate::sexp::accessors::INTEGER_ELT(marker, 0);
                    if offset < 0 {
                        bc_error("GNU DECLNKSTK marker is out of range");
                    }
                    prot_top = offset as usize;
                    let top = stack.at(stack.depth() - 1);
                    stack.set(stack.depth() - 2, top);
                    stack.set_depth(stack.depth() - 1);
                }),
                super::super::bytecode::GNU_OP_GETINTLBUILTIN => execute_gnu_instruction(|| {
                    let index = words[pc] as usize;
                    pc += 1;
                    let symbol = owned_constant_at(&constants, index as i64, "GNU opcode");
                    if TYPEOF(symbol) != SEXPTYPE::SYMSXP {
                        bc_error(format!(
                            "GNU GETINTLBUILTIN constant pool entry {index} is not a symbol"
                        ));
                    }
                    let value = with_stack_rooted(&stack, symbol, || {
                        let mut internal = crate::sexp::accessors::INTERNAL(symbol);
                        if internal.is_null() || internal == R_NilValue() {
                            // On-demand primitives resolve through the
                            // internal function table like do_internal.
                            let pname = crate::sexp::accessors::PRINTNAME(symbol);
                            let name =
                                std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(pname));
                            let idx = crate::mainutils::names::StrToInternal(name.as_ptr());
                            if idx != crate::sexp::ffi::NA_INTEGER {
                                let entry = &crate::mainutils::names::R_FunTab[idx as usize];
                                if (entry.eval % 100) / 10 != 0 {
                                    internal =
                                        crate::mainutils::dstruct::mkPRIMSXP(idx, entry.eval % 10);
                                }
                            }
                        }
                        internal
                    });
                    if value.is_null()
                        || value == R_NilValue()
                        || TYPEOF(value) != SEXPTYPE::BUILTINSXP
                    {
                        let name = crate::sexp::accessors::PRINTNAME(symbol);
                        let text = if name.is_null() || name == R_NilValue() {
                            std::borrow::Cow::Borrowed("?")
                        } else {
                            std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(name))
                                .to_string_lossy()
                        };
                        bc_error(format!("there is no .Internal function '{text}'"));
                    }
                    gnu_call_frames.push(GnuCallFrame {
                        marker: stack.depth(),
                        tags: Vec::new(),
                        raw_args: false,
                        call: own_operand(R_NilValue()),
                    });
                    stack.push(value);
                }),
                super::super::bytecode::GNU_OP_DDVAL
                | super::super::bytecode::GNU_OP_DDVAL_MISSOK => execute_gnu_instruction(|| {
                    let index = words[pc] as usize;
                    pc += 1;
                    let symbol = owned_constant_at(&constants, index as i64, "GNU opcode");
                    if TYPEOF(symbol) != SEXPTYPE::SYMSXP {
                        bc_error(format!(
                            "GNU DDVAL constant pool entry {index} is not a symbol"
                        ));
                    }
                    super::super::runtime::set_visible(TRUE);
                    let keep_missing = opcode == super::super::bytecode::GNU_OP_DDVAL_MISSOK;
                    let value = with_stack_rooted(&stack, symbol, || {
                        eval_gnu_getvar(symbol, rho, keep_missing, true)
                    });
                    stack.push(value);
                }),
                super::super::bytecode::GNU_OP_MAKECLOSURE => execute_gnu_instruction(|| {
                    let index = words[pc] as usize;
                    pc += 1;
                    let fb = owned_constant_at(&constants, index as i64, "GNU opcode");
                    if TYPEOF(fb) != SEXPTYPE::VECSXP || LENGTH(fb) < 2 {
                        bc_error(format!(
                            "GNU MAKECLOSURE constant {index} is not a formals/body pairlist"
                        ));
                    }
                    let forms_owned = own_operand(VECTOR_ELT(fb, 0));
                    let body_owned = own_operand(VECTOR_ELT(fb, 1));
                    let forms = forms_owned.as_raw();
                    let body = body_owned.as_raw();
                    let value = with_stack_rooted(&stack, fb, || {
                        crate::mainutils::dstruct::mkCLOSXP(forms, body, rho)
                    });
                    let value_owned = own_operand(value);
                    if LENGTH(fb) > 2 {
                        let srcref_owned = own_operand(VECTOR_ELT(fb, 2));
                        let srcref = srcref_owned.as_raw();
                        if srcref != R_NilValue() && !srcref.is_null() {
                            let srcref_symbol = crate::sexp::symbol::Rf_install(c"srcref".as_ptr());
                            crate::attrib_core::setAttrib(value, srcref_symbol, srcref);
                        }
                    }
                    super::super::runtime::set_visible(TRUE);
                    stack.push(value);
                }),
                super::super::bytecode::GNU_OP_DODOTS => execute_gnu_instruction(|| {
                    let Some(marker) = gnu_call_frames.last().map(|frame| frame.marker) else {
                        bc_error("GNU DODOTS has no active call frame");
                    };
                    if gnu_call_frames.last().is_some_and(|frame| frame.raw_args) {
                        bc_error("GNU DODOTS has no active function call");
                    }
                    if stack.depth() <= marker {
                        bc_error("GNU DODOTS has no function on the stack");
                    }
                    let fun = stack.at(marker);
                    let ftype = TYPEOF(fun);
                    if ftype != SEXPTYPE::SPECIALSXP {
                        let h = with_stack_rooted(&stack, fun, || {
                            crate::sexp::envir::R_findVar(crate::sexp::symbol::R_DotsSymbol(), rho)
                        });
                        let h_owned = own_operand(h);
                        if TYPEOF(h) == SEXPTYPE::DOTSXP || h == R_NilValue() {
                            let mut cell = h;
                            while cell != R_NilValue() && !cell.is_null() {
                                let cell_owned = own_operand(cell);
                                let argument = with_stack_rooted(&stack, cell, || {
                                    let expr = CAR(cell);
                                    if ftype == SEXPTYPE::BUILTINSXP {
                                        crate::eval::eval::Rf_eval(expr, rho)
                                    } else if expr == R_MissingArg() {
                                        expr
                                    } else {
                                        crate::sexp::memory_ext::mkPROMSXP(expr, rho)
                                    }
                                });
                                stack.push(argument);
                                let tag = crate::sexp::accessors::TAG(cell);
                                if !tag.is_null() && tag != R_NilValue() {
                                    if let Some(frame) = gnu_call_frames.last_mut() {
                                        frame.tags.push((stack.depth() - 1, own_operand(tag)));
                                    }
                                }
                                cell = CDR(cell);
                            }
                        } else if h != R_MissingArg() {
                            bc_error("'...' used in an incorrect context");
                        }
                    }
                }),
                super::super::bytecode::GNU_OP_CALLSPECIAL => execute_gnu_instruction(|| {
                    let call_index = words[pc] as usize;
                    pc += 1;
                    let call = owned_constant_at(&constants, call_index as i64, "GNU opcode");
                    if call.is_null() || TYPEOF(call) != SEXPTYPE::LANGSXP {
                        bc_error("GNU CALLSPECIAL requires a call in the constant pool");
                    }
                    let symbol = CAR(call);
                    if symbol.is_null() || TYPEOF(symbol) != SEXPTYPE::SYMSXP {
                        bc_error("GNU CALLSPECIAL call does not have a symbol operator");
                    }
                    let result = with_stack_rooted(&stack, call, || {
                        // GNU getPrimitive: SYMVALUE of the symbol, ignoring
                        // frame bindings that may shadow the FunTab special.
                        let mut fun = crate::sexp::accessors::SYMVALUE(symbol);
                        if !fun.is_null() && TYPEOF(fun) == SEXPTYPE::PROMSXP {
                            crate::sexp::envir::forcePromise(fun);
                            fun = crate::sexp::accessors::PRVALUE(fun);
                        }

                        if fun == R_UnboundValue()
                            || fun.is_null()
                            || TYPEOF(fun) != SEXPTYPE::SPECIALSXP
                        {
                            fun = crate::sexp::envir::findFun(
                                symbol,
                                super::super::runtime::base_env(),
                            );
                        }
                        if fun == R_UnboundValue() || TYPEOF(fun) != SEXPTYPE::SPECIALSXP {
                            let name =
                                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(symbol))).to_string_lossy();
                            fun = super::super::primitive::make_primitive_binding(
                                name.as_ref(),
                                SEXPTYPE::SPECIALSXP,
                            );
                        }
                        if fun.is_null()
                            || fun == R_UnboundValue()
                            || TYPEOF(fun) != SEXPTYPE::SPECIALSXP
                        {
                            let name =
                                std::ffi::CStr::from_ptr(CHAR(PRINTNAME(symbol))).to_string_lossy();
                            let kind = if fun.is_null() || fun == R_UnboundValue() {
                                -1
                            } else {
                                TYPEOF(fun)
                            };
                            bc_error(format!(
                                "GNU CALLSPECIAL symbol did not resolve to a special: {name} has type {kind}"
                            ));
                        }

                        use crate::sexp::object::Sexp;
                        let result = super::super::apply::apply_special_safe(
                            Sexp::from_raw_unchecked(fun),
                            Sexp::from_raw_unchecked(call),
                            Sexp::from_raw_unchecked(CDR(call)),
                            Sexp::from_raw_unchecked(rho),
                        );
                        result.unwrap_or_else(|error| bc_error(error)).as_raw()
                    });
                    stack.push(result);
                }),
                super::super::bytecode::GNU_OP_GETFUN
                | super::super::bytecode::GNU_OP_GETBUILTIN
                | super::super::bytecode::GNU_OP_GETGLOBFUN
                | super::super::bytecode::GNU_OP_GETSYMFUN => execute_gnu_instruction(|| {
                    let index = words[pc] as usize;
                    pc += 1;
                    let symbol = owned_constant_at(&constants, index as i64, "GNU opcode");
                    if TYPEOF(symbol) != SEXPTYPE::SYMSXP {
                        bc_error(format!(
                            "GNU GETFUN constant pool entry {index} is not a symbol"
                        ));
                    }
                    let fun = with_stack_rooted(&stack, symbol, || {
                        if opcode == super::super::bytecode::GNU_OP_GETSYMFUN {
                            let mut value = crate::sexp::accessors::SYMVALUE(symbol);
                            if TYPEOF(value) == SEXPTYPE::PROMSXP {
                                crate::sexp::envir::forcePromise(value);
                                value = crate::sexp::accessors::PRVALUE(value);
                            }
                            value
                        } else {
                            let lookup_env = if opcode == super::super::bytecode::GNU_OP_GETBUILTIN
                            {
                                super::super::runtime::base_env()
                            } else if opcode == super::super::bytecode::GNU_OP_GETGLOBFUN {
                                crate::sexp::globals::R_GlobalEnv()
                            } else {
                                rho
                            };
                            let mut fun = crate::sexp::envir::findFun(symbol, lookup_env);
                            if (fun == R_UnboundValue() || fun.is_null())
                                && opcode == super::super::bytecode::GNU_OP_GETFUN
                            {
                                fun = crate::sexp::envir::findFun(
                                    symbol,
                                    super::super::runtime::base_env(),
                                );
                            }
                            if (fun == R_UnboundValue() || fun.is_null())
                                && opcode == super::super::bytecode::GNU_OP_GETFUN
                            {
                                if let Some(symbol) = crate::sexp::object::Sexp::from_raw(symbol) {
                                    if let Some(primitive) =
                                        super::super::eval::primitive_for_symbol(symbol)
                                    {
                                        fun = primitive.as_raw();
                                    }
                                }
                            }
                            fun
                        }
                    });
                    if fun == R_UnboundValue() || fun.is_null() {
                        let call = gnu_pending_call(words, pc, &constants);
                        crate::mainutils::errors::R_FunctionNotFoundError(symbol, call);
                    }

                    let fun_type = TYPEOF(fun);
                    if fun_type != SEXPTYPE::CLOSXP
                        && fun_type != SEXPTYPE::BUILTINSXP
                        && fun_type != SEXPTYPE::SPECIALSXP
                    {
                        bc_error("GNU GETFUN did not resolve to a function");
                    }
                    if opcode == super::super::bytecode::GNU_OP_GETBUILTIN
                        && fun_type != SEXPTYPE::BUILTINSXP
                    {
                        let name =
                            std::ffi::CStr::from_ptr(CHAR(PRINTNAME(symbol))).to_string_lossy();
                        bc_error(format!(
                            "GNU GETBUILTIN did not resolve to a builtin: {name} has type {fun_type:?}"
                        ));
                    }

                    gnu_call_frames.push(GnuCallFrame {
                        marker: stack.depth(),
                        tags: Vec::new(),
                        raw_args: false,
                        call: own_operand(R_NilValue()),
                    });
                    stack.push(fun);
                }),
                super::super::bytecode::GNU_OP_MAKEPROM => execute_gnu_instruction(|| {
                    let index = words[pc] as usize;
                    pc += 1;
                    let expression = owned_constant_at(&constants, index as i64, "GNU opcode");
                    if gnu_call_frames.is_empty() {
                        bc_error("GNU MAKEPROM has no active GETFUN call");
                    }
                    let promise = with_stack_rooted(&stack, expression, || {
                        crate::sexp::memory_ext::mkPROMSXP(expression, rho)
                    });
                    stack.push(promise);
                }),
                super::super::bytecode::GNU_OP_PUSHCONSTARG
                | super::super::bytecode::GNU_OP_PUSHNULLARG
                | super::super::bytecode::GNU_OP_PUSHTRUEARG
                | super::super::bytecode::GNU_OP_PUSHFALSEARG => execute_gnu_instruction(|| {
                    if gnu_call_frames.is_empty() {
                        bc_error("GNU constant argument has no active GETFUN call");
                    }
                    let value = match opcode {
                        super::super::bytecode::GNU_OP_PUSHCONSTARG => {
                            let index = words[pc] as usize;
                            pc += 1;
                            owned_constant_at(&constants, index as i64, "GNU opcode")
                        }
                        super::super::bytecode::GNU_OP_PUSHNULLARG => R_NilValue(),
                        super::super::bytecode::GNU_OP_PUSHTRUEARG => {
                            with_stack_rooted(&stack, R_NilValue(), || {
                                crate::sexp::constructors::Rf_ScalarLogical(1)
                            })
                        }
                        _ => with_stack_rooted(&stack, R_NilValue(), || {
                            crate::sexp::constructors::Rf_ScalarLogical(0)
                        }),
                    };
                    crate::sexp::accessors::SET_NAMED(value, 2);
                    // STARTSUBSET/DFLTSUBSET accumulate raw SEXPs. Wrapping
                    // drop=FALSE in a promise makes asLogical() NA and GNU
                    // ExtractDropArg defaults drop to TRUE → X[,j] is a
                    // vector → 1×n %*% 1×1 in predict.lm type="terms".
                    let stored = if gnu_call_frames.last().is_some_and(|frame| frame.raw_args) {
                        value
                    } else {
                        with_stack_rooted(&stack, value, || {
                            let promise = crate::sexp::memory_ext::mkPROMSXP(value, R_NilValue());
                            crate::sexp::accessors::SET_PRVALUE(promise, value);
                            promise
                        })
                    };
                    stack.push(stored);
                }),
                super::super::bytecode::GNU_OP_PUSHARG => execute_gnu_instruction(|| {
                    if gnu_call_frames.is_empty() {
                        bc_error("GNU PUSHARG has no active GETFUN call");
                    }
                    let frame = gnu_call_frames.last().unwrap();
                    if stack.depth() <= frame.marker + 1 {
                        bc_error("GNU PUSHARG has no preceding argument");
                    }
                    let value_owned = stack_pop_checked(&mut stack, "GNU PUSHARG");
                    let value = value_owned.as_raw();
                    let stored = if gnu_call_frames.last().is_some_and(|frame| frame.raw_args) {
                        value
                    } else {
                        with_stack_rooted(&stack, value, || {
                            let promise = crate::sexp::memory_ext::mkPROMSXP(value, R_NilValue());
                            crate::sexp::accessors::SET_PRVALUE(promise, value);
                            promise
                        })
                    };
                    stack.push(stored);
                }),
                super::super::bytecode::GNU_OP_CHECKFUN => execute_gnu_instruction(|| {
                    let fun = stack_top_checked(&stack, "GNU CHECKFUN");
                    let kind = TYPEOF(fun);
                    if kind != SEXPTYPE::CLOSXP
                        && kind != SEXPTYPE::BUILTINSXP
                        && kind != SEXPTYPE::SPECIALSXP
                    {
                        bc_error("GNU CHECKFUN found a non-function");
                    }
                    gnu_call_frames.push(GnuCallFrame {
                        marker: stack.depth() - 1,
                        tags: Vec::new(),
                        raw_args: false,
                        call: own_operand(R_NilValue()),
                    });
                }),
                super::super::bytecode::GNU_OP_SETTAG => execute_gnu_instruction(|| {
                    let index = words[pc] as usize;
                    pc += 1;
                    let tag = owned_constant_at(&constants, index as i64, "GNU opcode");
                    if !tag.is_null() && tag != R_NilValue() && TYPEOF(tag) != SEXPTYPE::SYMSXP {
                        bc_error("GNU SETTAG requires a symbol or NULL tag");
                    }
                    let frame = gnu_call_frames
                        .last_mut()
                        .unwrap_or_else(|| bc_error("GNU SETTAG has no active GETFUN call"));
                    if stack.depth() <= frame.marker + 1 {
                        bc_error("GNU SETTAG has no preceding call argument");
                    }
                    let slot = stack.depth() - 1;
                    if let Some((_, previous)) =
                        frame.tags.iter_mut().find(|(index, _)| *index == slot)
                    {
                        *previous = own_operand(tag);
                    } else {
                        frame.tags.push((slot, own_operand(tag)));
                    }
                }),
                super::super::bytecode::GNU_OP_CALL
                | super::super::bytecode::GNU_OP_CALLBUILTIN => {
                    let builtin_only = opcode == super::super::bytecode::GNU_OP_CALLBUILTIN;
                    let call_index = words[pc] as usize;
                    pc += 1;
                    let frame = gnu_call_frames
                        .pop()
                        .unwrap_or_else(|| bc_error("GNU CALL has no active GETFUN call"));
                    let marker = frame.marker;
                    let depth = stack.depth();
                    if depth <= marker {
                        bc_error("GNU CALL has no function");
                    }
                    let call_expr = owned_constant_at(&constants, call_index as i64, "GNU opcode");
                    if TYPEOF(call_expr) != SEXPTYPE::LANGSXP {
                        bc_error(format!(
                            "GNU CALL expression constant {call_index} is not a language object"
                        ));
                    }
                    let result =
                        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            with_stack_rooted(&stack, call_expr, || {
                                let fun = stack.at(marker);
                                let mut args = R_NilValue();
                                let mut argument_roots = Vec::new();
                                for index in (marker + 1..depth).rev() {
                                    args = Rf_cons(stack.at(index), args);
                                    argument_roots.push(own_operand(args));
                                    if let Some((_, tag)) =
                                        frame.tags.iter().find(|(slot, _)| *slot == index)
                                    {
                                        crate::sexp::accessors::SETTAG(args, tag.as_raw());
                                    }
                                }
                                use crate::sexp::object::Sexp;
                                let call = Sexp::from_raw_unchecked(call_expr);
                                let env = Sexp::from_raw_unchecked(rho);
                                let function = Sexp::from_raw_unchecked(fun);
                                if builtin_only && TYPEOF(fun) != SEXPTYPE::BUILTINSXP {
                                    bc_error("GNU CALLBUILTIN frame does not contain a builtin");
                                }
                                let result = match TYPEOF(fun) {
                                    kind if kind == SEXPTYPE::CLOSXP => {
                                        super::super::apply::apply_closure_safe(
                                            function,
                                            call,
                                            Sexp::from_raw_unchecked(args),
                                            env,
                                        )
                                    }
                                    kind if kind == SEXPTYPE::SPECIALSXP => {
                                        super::super::apply::apply_special_safe(
                                            function,
                                            call,
                                            Sexp::from_raw_unchecked(CDR(call_expr)),
                                            env,
                                        )
                                    }
                                    kind if kind == SEXPTYPE::BUILTINSXP => {
                                        if !builtin_only
                                            && super::super::apply::builtin_requires_raw_args(
                                                Sexp::from_raw_unchecked(fun),
                                                call.clone(),
                                            )
                                        {
                                            super::super::apply::apply_builtin_safe(
                                                function,
                                                call,
                                                Sexp::from_raw_unchecked(CDR(call_expr)),
                                                env,
                                            )
                                        } else {
                                            force_gnu_builtin_arglist(args);
                                            // GNU getCurrentCall uses the active bytecode
                                            // expression for inlined builtin calls. Keep
                                            // that expression on the existing context stack
                                            // without adding a function frame.
                                            let _expression =
                                                crate::sexp::context::begin_context_guard(
                                                    crate::sexp::context::ctxt_flags::CTXT_CCODE,
                                                    call_expr,
                                                    rho,
                                                    rho,
                                                    None,
                                                    R_NilValue(),
                                                    R_NilValue(),
                                                );
                                            super::super::apply::apply_builtin_values_safe(
                                                function,
                                                call,
                                                Sexp::from_raw_unchecked(args),
                                                env,
                                            )
                                        }
                                    }
                                    _ => unreachable!(),
                                };
                                result.unwrap_or_else(|error| bc_error(error)).as_raw()
                            })
                        })) {
                            Ok(value) => value,
                            Err(payload) => {
                                let jump = recover_loop_jump(payload, &loop_stack);
                                pc = apply_loop_jump(&mut stack, &mut for_loops, jump) as usize;
                                super::super::runtime::set_visible(FALSE);
                                continue;
                            }
                        };
                    stack.set_depth(marker);
                    stack.push(result);
                }
                super::super::bytecode::GNU_OP_BASEGUARD => execute_gnu_instruction(|| {
                    let expression_index = words[pc] as usize;
                    let target = words[pc + 1] as usize;
                    pc += 2;
                    let expression =
                        owned_constant_at(&constants, expression_index as i64, "GNU opcode");
                    if expression.is_null() || TYPEOF(expression) != SEXPTYPE::LANGSXP {
                        bc_error("GNU BASEGUARD requires a call in the constant pool");
                    }
                    let symbol = CAR(expression);
                    if symbol.is_null() || TYPEOF(symbol) != SEXPTYPE::SYMSXP {
                        bc_error("GNU BASEGUARD call does not have a symbol operator");
                    }
                    let current = with_stack_rooted(&stack, expression, || {
                        crate::sexp::envir::findFun(symbol, rho)
                    });
                    let current_owned = own_operand(current);
                    let base = with_stack_rooted(&stack, expression, || {
                        crate::sexp::envir::findFun(symbol, super::super::runtime::base_env())
                    });
                    if current_owned.as_raw() != base {
                        let value = with_stack_rooted(&stack, expression, || {
                            crate::eval::eval::Rf_eval(expression, rho)
                        });
                        stack.push(value);
                        pc = target;
                    }
                }),
                super::super::bytecode::GNU_OP_SETVAR | super::super::bytecode::GNU_OP_SETVAR2 => {
                    execute_gnu_instruction(|| {
                        let index = words[pc] as usize;
                        pc += 1;
                        let symbol = owned_constant_at(&constants, index as i64, "GNU opcode");
                        if TYPEOF(symbol) != SEXPTYPE::SYMSXP {
                            bc_error(format!(
                                "GNU SETVAR constant pool entry {index} is not a symbol"
                            ));
                        }
                        let value = stack_top_checked(&stack, "GNU SETVAR");
                        // GNU SETVAR does INCREMENT_NAMED before the bind.
                        // Chained `varE <- seA <- A <- vector()` is three stores
                        // of one value: 0→1, then 1→2. The second name must look
                        // shared so `[[<-` duplicates the list.
                        let named = crate::sexp::accessors::NAMED(value);
                        if named < 2 {
                            crate::sexp::accessors::SET_NAMED(value, named + 1);
                        }
                        with_stack_rooted(&stack, value, || {
                            if opcode == super::super::bytecode::GNU_OP_SETVAR2 {
                                setVar(symbol, value, ENCLOS(rho));
                            } else {
                                defineVar(symbol, value, rho);
                            }
                        });
                        super::super::runtime::set_visible(FALSE);
                    })
                }
                super::super::bytecode::GNU_OP_STARTFOR => execute_gnu_instruction(|| {
                    let _expr_index = words[pc] as usize;
                    let symbol_index = words[pc + 1] as usize;
                    let end = words[pc + 2] as usize;
                    let symbol = owned_constant_at(&constants, symbol_index as i64, "GNU opcode");
                    if TYPEOF(symbol) != SEXPTYPE::SYMSXP {
                        bc_error("GNU STARTFOR requires a symbol constant");
                    }
                    let mut sequence = stack_top_checked(&stack, "GNU STARTFOR sequence");
                    if crate::mainutils::essentials::sexp_has_class(sequence, "factor") {
                        sequence = with_stack_rooted(&stack, symbol, || {
                            crate::mainutils::coerce::asCharacterFactor(sequence)
                        });
                        stack.set(stack.depth() - 1, sequence);
                    }
                    let Some(length) = for_sequence_length(sequence) else {
                        bc_error("GNU STARTFOR requires a vector sequence");
                    };
                    with_stack_rooted(&stack, sequence, || defineVar(symbol, R_NilValue(), rho));
                    let sequence_slot = stack.depth() - 1;
                    for_loops.push(GnuForLoopState {
                        sequence_slot,
                        symbol: own_operand(symbol),
                        index: -1,
                        length,
                    });
                    pc = end;
                }),
                super::super::bytecode::GNU_OP_STEPFOR => execute_gnu_instruction(|| {
                    crate::sexp::instance::check_cancellation();
                    let _limit_check = super::super::limits::check_eval_depth()
                        .unwrap_or_else(|message| bc_error(message));
                    let body_start = words[pc] as usize;
                    pc += 1;
                    let Some(state) = for_loops.last_mut() else {
                        bc_error("GNU STEPFOR has no active loop");
                    };
                    state.index += 1;
                    if state.index < state.length {
                        let sequence =
                            stack_at_checked(&stack, state.sequence_slot, "GNU STEPFOR sequence");
                        bind_for_element(&stack, sequence, state.symbol.as_raw(), state.index, rho);
                        pc = body_start;
                    } else {
                        // Fall through to ENDFOR, which removes the loop's
                        // rooted sequence and leaves the loop expression's nil.
                    }
                }),
                super::super::bytecode::GNU_OP_ENDFOR => execute_gnu_instruction(|| {
                    let Some(completed) = for_loops.pop() else {
                        bc_error("GNU ENDFOR has no active loop");
                    };
                    if stack.depth() != completed.sequence_slot + 1 {
                        bc_error("GNU ENDFOR bytecode stack is unbalanced");
                    }
                    stack_pop_checked(&mut stack, "GNU ENDFOR sequence");
                    stack.push(R_NilValue());
                    super::super::runtime::set_visible(FALSE);
                }),
                super::super::bytecode::GNU_OP_UMINUS | super::super::bytecode::GNU_OP_UPLUS => {
                    execute_gnu_instruction(|| {
                        let call = owned_constant_at(&constants, words[pc] as i64, "GNU opcode");
                        if call.is_null() || TYPEOF(call) != SEXPTYPE::LANGSXP {
                            bc_error("GNU unary operator requires a call in the constant pool");
                        }
                        pc += 1;
                        let value_owned = stack_pop_checked(&mut stack, "GNU unary operator");
                        let value = value_owned.as_raw();
                        let symbol = if opcode == super::super::bytecode::GNU_OP_UMINUS {
                            c"-"
                        } else {
                            c"+"
                        };
                        let result = with_stack_rooted(&stack, value, || {
                            let op = R_findVar(
                                crate::sexp::symbol::Rf_install(symbol.as_ptr()),
                                super::super::runtime::base_env(),
                            );
                            let args = Rf_cons(value, R_NilValue());
                            let _args = own_operand(args);
                            super::super::arithmetic::do_arith(call, op, args, rho)
                        });
                        super::super::runtime::set_visible(TRUE);
                        stack.push(result);
                    })
                }
                super::super::bytecode::GNU_OP_SQRT | super::super::bytecode::GNU_OP_EXP => {
                    execute_gnu_instruction(|| {
                        let call = owned_constant_at(&constants, words[pc] as i64, "GNU opcode");
                        if call.is_null() || TYPEOF(call) != SEXPTYPE::LANGSXP {
                            bc_error("GNU math opcode requires a call in the constant pool");
                        }
                        pc += 1;
                        let value_owned = stack_pop_checked(&mut stack, "GNU math opcode");
                        let value = value_owned.as_raw();
                        let symbol = if opcode == super::super::bytecode::GNU_OP_SQRT {
                            c"sqrt"
                        } else {
                            c"exp"
                        };
                        let result = with_stack_rooted(&stack, value, || {
                            let fun = crate::sexp::envir::findFun(
                                crate::sexp::symbol::Rf_install(symbol.as_ptr()),
                                super::super::runtime::base_env(),
                            );
                            let expression = Rf_lang2(fun, value);
                            let _expression = own_operand(expression);
                            crate::eval::eval::Rf_eval(expression, rho)
                        });
                        super::super::runtime::set_visible(TRUE);
                        stack.push(result);
                    })
                }
                super::super::bytecode::GNU_OP_LOG | super::super::bytecode::GNU_OP_LOGBASE => {
                    execute_gnu_instruction(|| {
                        let call = owned_constant_at(&constants, words[pc] as i64, "GNU opcode");
                        if call.is_null() || TYPEOF(call) != SEXPTYPE::LANGSXP {
                            bc_error("GNU logarithm opcode requires a call in the constant pool");
                        }
                        pc += 1;
                        let result = if opcode == super::super::bytecode::GNU_OP_LOG {
                            let value_owned = stack_pop_checked(&mut stack, "GNU LOG");
                            let value = value_owned.as_raw();
                            with_stack_rooted(&stack, value, || {
                                let args = Rf_cons(value, R_NilValue());
                                let _args = own_operand(args);
                                let op = crate::sexp::envir::findFun(
                                    crate::sexp::symbol::Rf_install(c"log".as_ptr()),
                                    super::super::runtime::base_env(),
                                );
                                let result = crate::eval::arithmetic::do_math1(call, op, args, rho);
                                if crate::sexp::accessors::OBJECT(value) == 0 {
                                    gnu_math1_real_result(result)
                                } else {
                                    result
                                }
                            })
                        } else {
                            let base_owned = stack_pop_checked(&mut stack, "GNU LOGBASE");
                            let base = base_owned.as_raw();
                            let value_owned = stack_pop_checked(&mut stack, "GNU LOGBASE");
                            let value = value_owned.as_raw();
                            with_stack_rooted(&stack, value, || {
                                with_stack_rooted(&stack, base, || {
                                    if let Some(empty) = gnu_logbase_empty_result(value, base) {
                                        return empty;
                                    }
                                    let tail = Rf_cons(base, R_NilValue());
                                    let _tail = own_operand(tail);
                                    let args = Rf_cons(value, tail);
                                    let _args = own_operand(args);
                                    let op = crate::sexp::envir::findFun(
                                        crate::sexp::symbol::Rf_install(c"log".as_ptr()),
                                        super::super::runtime::base_env(),
                                    );
                                    let result =
                                        crate::eval::arithmetic::do_math1(call, op, args, rho);
                                    if crate::sexp::accessors::OBJECT(value) == 0
                                        && crate::sexp::accessors::OBJECT(base) == 0
                                    {
                                        gnu_math1_real_result(result)
                                    } else {
                                        result
                                    }
                                })
                            })
                        };
                        super::super::runtime::set_visible(TRUE);
                        stack.push(result);
                    })
                }
                super::super::bytecode::GNU_OP_MATH1 => execute_gnu_instruction(|| {
                    let call = owned_constant_at(&constants, words[pc] as i64, "GNU opcode");
                    let math_index = words[pc + 1];
                    if call.is_null() || TYPEOF(call) != SEXPTYPE::LANGSXP {
                        bc_error("GNU MATH1 requires a call in the constant pool");
                    }
                    pc += 2;
                    let names = [
                        "floor", "ceiling", "sign", "expm1", "log1p", "cos", "sin", "tan", "acos",
                        "asin", "atan", "cosh", "sinh", "tanh", "acosh", "asinh", "atanh",
                        "lgamma", "gamma", "digamma", "trigamma", "cospi", "sinpi", "tanpi",
                    ];
                    let Some(name) = names.get(math_index as usize).copied() else {
                        bc_error(format!("GNU MATH1 function index {math_index} is invalid"));
                    };
                    // Rf_install reads the bytes; keep CString alive across the call.
                    let cname = std::ffi::CString::new(name).unwrap();
                    let symbol = crate::sexp::symbol::Rf_install(cname.as_ptr());
                    if CAR(call) != symbol {
                        bc_error("math1 compiler/interpreter mismatch");
                    }
                    let value_owned = stack_pop_checked(&mut stack, "GNU MATH1");
                    let value = value_owned.as_raw();
                    let result = with_stack_rooted(&stack, value, || {
                        let args = Rf_cons(value, R_NilValue());
                        let _args = own_operand(args);
                        let op =
                            crate::sexp::envir::findFun(symbol, super::super::runtime::base_env());
                        // GNU's slow path calls do_math1 on the primitive. rport's
                        // do_math1 only covers a subset (floor/sign/log/exp/...), so
                        // dispatch through the named builtin for sin/expm1/etc.
                        let result = if let Some(handler) =
                            crate::eval::builtin::evaluated_builtin_handler(name)
                        {
                            handler(call, op, args, rho)
                        } else {
                            crate::eval::arithmetic::do_math1(call, op, args, rho)
                        };
                        if crate::sexp::accessors::OBJECT(value) != 0 {
                            return result;
                        }
                        if !result.is_null() && result != R_NilValue() && result != value {
                            let _keep = own_operand(result);
                            crate::mainutils::coerce::SHALLOW_DUPLICATE_ATTRIB(result, value);
                        }
                        if name == "floor" || name == "ceiling" {
                            gnu_math1_real_result(result)
                        } else {
                            result
                        }
                    });
                    super::super::runtime::set_visible(TRUE);
                    stack.push(result);
                }),
                super::super::bytecode::GNU_OP_ADD
                | super::super::bytecode::GNU_OP_SUB
                | super::super::bytecode::GNU_OP_MUL
                | super::super::bytecode::GNU_OP_DIV
                | super::super::bytecode::GNU_OP_EXPT
                | super::super::bytecode::GNU_OP_EQ
                | super::super::bytecode::GNU_OP_NE
                | super::super::bytecode::GNU_OP_LT
                | super::super::bytecode::GNU_OP_LE
                | super::super::bytecode::GNU_OP_GE
                | super::super::bytecode::GNU_OP_GT => execute_gnu_instruction(|| {
                    let call = owned_constant_at(&constants, words[pc] as i64, "GNU opcode");
                    if call.is_null() || TYPEOF(call) != SEXPTYPE::LANGSXP {
                        bc_error("GNU binary operator requires a call in the constant pool");
                    }
                    pc += 1;
                    let b_owned = stack_pop_checked(&mut stack, "GNU binary operator");
                    let b = b_owned.as_raw();
                    let a_owned = stack_pop_checked(&mut stack, "GNU binary operator");
                    let a = a_owned.as_raw();
                    let symbol = match opcode {
                        super::super::bytecode::GNU_OP_ADD => c"+",
                        super::super::bytecode::GNU_OP_SUB => c"-",
                        super::super::bytecode::GNU_OP_MUL => c"*",
                        super::super::bytecode::GNU_OP_DIV => c"/",
                        super::super::bytecode::GNU_OP_EXPT => c"^",
                        super::super::bytecode::GNU_OP_EQ => c"==",
                        super::super::bytecode::GNU_OP_NE => c"!=",
                        super::super::bytecode::GNU_OP_LT => c"<",
                        super::super::bytecode::GNU_OP_LE => c"<=",
                        super::super::bytecode::GNU_OP_GE => c">=",
                        super::super::bytecode::GNU_OP_GT => c">",
                        _ => unreachable!(),
                    };
                    // Share the vector, missing-value, overflow and S3
                    // semantics of the evaluator. Scalar-only VM helpers
                    // would silently truncate vectors and drop attributes.
                    let result = with_stack_rooted(&stack, a, || {
                        with_stack_rooted(&stack, b, || {
                            // The instruction fixes primitive identity; mutable
                            // base bindings and retained call text cannot change it.
                            let op = own_operand(super::super::primitive::make_primitive_binding(
                                symbol.to_str().unwrap(),
                                SEXPTYPE::BUILTINSXP,
                            ));
                            let tail = Rf_cons(b, R_NilValue());
                            let _tail = own_operand(tail);
                            let args = Rf_cons(a, tail);
                            let _args = own_operand(args);
                            if opcode <= super::super::bytecode::GNU_OP_EXPT {
                                super::super::arithmetic::do_arith(call, op.as_raw(), args, rho)
                            } else {
                                super::super::arithmetic::do_relop(call, op.as_raw(), args, rho)
                            }
                        })
                    });
                    crate::sexp::globals::set_R_Visible(TRUE);
                    stack.push(result);
                }),
                super::super::bytecode::GNU_OP_AND | super::super::bytecode::GNU_OP_OR => {
                    execute_gnu_instruction(|| {
                        let call = owned_constant_at(&constants, words[pc] as i64, "GNU opcode");
                        if call.is_null() || TYPEOF(call) != SEXPTYPE::LANGSXP {
                            bc_error("GNU logic operator requires a call in the constant pool");
                        }
                        pc += 1;
                        let b_owned = stack_pop_checked(&mut stack, "GNU logic operator");
                        let b = b_owned.as_raw();
                        let a_owned = stack_pop_checked(&mut stack, "GNU logic operator");
                        let a = a_owned.as_raw();
                        let symbol = if opcode == super::super::bytecode::GNU_OP_AND {
                            c"&"
                        } else {
                            c"|"
                        };
                        let result = with_stack_rooted(&stack, a, || {
                            with_stack_rooted(&stack, b, || {
                                eval_gnu_logic(call, symbol, a, Some(b), rho)
                            })
                        });
                        crate::sexp::globals::set_R_Visible(TRUE);
                        stack.push(result);
                    })
                }
                super::super::bytecode::GNU_OP_ISNULL
                | super::super::bytecode::GNU_OP_ISLOGICAL
                | super::super::bytecode::GNU_OP_ISINTEGER
                | super::super::bytecode::GNU_OP_ISDOUBLE
                | super::super::bytecode::GNU_OP_ISCOMPLEX
                | super::super::bytecode::GNU_OP_ISCHARACTER
                | super::super::bytecode::GNU_OP_ISSYMBOL
                | super::super::bytecode::GNU_OP_ISOBJECT
                | super::super::bytecode::GNU_OP_ISNUMERIC => execute_gnu_instruction(|| {
                    let value_owned = stack_pop_checked(&mut stack, "GNU ISTYPE");
                    let value = value_owned.as_raw();
                    let result =
                        with_stack_rooted(&stack, value, || eval_gnu_istype(opcode, value));
                    super::super::runtime::set_visible(TRUE);
                    stack.push(result);
                }),

                super::super::bytecode::GNU_OP_NOT => execute_gnu_instruction(|| {
                    let call = owned_constant_at(&constants, words[pc] as i64, "GNU opcode");
                    if call.is_null() || TYPEOF(call) != SEXPTYPE::LANGSXP {
                        bc_error("GNU NOT requires a call in the constant pool");
                    }
                    pc += 1;
                    let value_owned = stack_pop_checked(&mut stack, "GNU NOT");
                    let value = value_owned.as_raw();
                    let result = with_stack_rooted(&stack, value, || {
                        eval_gnu_logic(call, c"!", value, None, rho)
                    });
                    super::super::runtime::set_visible(TRUE);
                    stack.push(result);
                }),
                super::super::bytecode::GNU_OP_COLON => execute_gnu_instruction(|| {
                    let call = owned_constant_at(&constants, words[pc] as i64, "GNU opcode");
                    if call.is_null() || TYPEOF(call) != SEXPTYPE::LANGSXP {
                        bc_error("GNU COLON requires a call in the constant pool");
                    }
                    pc += 1;
                    let rhs_owned = stack_pop_checked(&mut stack, "GNU COLON");
                    let rhs = rhs_owned.as_raw();
                    let lhs_owned = stack_pop_checked(&mut stack, "GNU COLON");
                    let lhs = lhs_owned.as_raw();
                    let result = with_stack_rooted(&stack, lhs, || {
                        with_stack_rooted(&stack, rhs, || {
                            let op = crate::sexp::envir::findFun(
                                crate::sexp::symbol::Rf_install(c":".as_ptr()),
                                super::super::runtime::base_env(),
                            );
                            // GNU asReal coerces strings ("5", "a"). The seq
                            // helper's asReal only reads INT/REAL/LGL, so
                            // non-numeric args are coerced first. Factors stay
                            // intact so do_colon can still take cross_colon.
                            let factors =
                                crate::mainutils::essentials::sexp_has_class(lhs, "factor")
                                    && crate::mainutils::essentials::sexp_has_class(rhs, "factor");
                            let lhs_arg_owned = own_operand(if factors {
                                lhs
                            } else {
                                gnu_colon_numeric_arg(lhs)
                            });
                            let rhs_arg_owned = own_operand(if factors {
                                rhs
                            } else {
                                gnu_colon_numeric_arg(rhs)
                            });
                            let (lhs_arg, rhs_arg) =
                                (lhs_arg_owned.as_raw(), rhs_arg_owned.as_raw());
                            let tail = Rf_cons(rhs_arg, R_NilValue());
                            let _tail = own_operand(tail);
                            let args = Rf_cons(lhs_arg, tail);
                            let _args = own_operand(args);
                            crate::mainutils::seq::do_colon(call, op, args, rho)
                        })
                    });
                    super::super::runtime::set_visible(TRUE);
                    stack.push(result);
                }),
                super::super::bytecode::GNU_OP_SEQALONG | super::super::bytecode::GNU_OP_SEQLEN => {
                    execute_gnu_instruction(|| {
                        let name = if opcode == super::super::bytecode::GNU_OP_SEQALONG {
                            "GNU SEQALONG"
                        } else {
                            "GNU SEQLEN"
                        };
                        let call = owned_constant_at(&constants, words[pc] as i64, "GNU opcode");
                        if call.is_null() || TYPEOF(call) != SEXPTYPE::LANGSXP {
                            bc_error(format!("{name} requires a call in the constant pool"));
                        }
                        pc += 1;
                        let value_owned = stack_pop_checked(&mut stack, name);
                        let value = value_owned.as_raw();
                        let result = with_stack_rooted(&stack, value, || {
                            let symbol = if opcode == super::super::bytecode::GNU_OP_SEQALONG {
                                c"seq_along"
                            } else {
                                c"seq_len"
                            };
                            let op = crate::sexp::envir::findFun(
                                crate::sexp::symbol::Rf_install(symbol.as_ptr()),
                                super::super::runtime::base_env(),
                            );
                            let args = Rf_cons(value, R_NilValue());
                            let _args = own_operand(args);
                            if opcode == super::super::bytecode::GNU_OP_SEQALONG {
                                crate::mainutils::seq::do_seq_along(call, op, args, rho)
                            } else {
                                crate::mainutils::seq::do_seq_len(call, op, args, rho)
                            }
                        });
                        super::super::runtime::set_visible(TRUE);
                        stack.push(result);
                    })
                }
                super::super::bytecode::GNU_OP_DOLLAR => execute_gnu_instruction(|| {
                    let call_index = words[pc] as usize;
                    let symbol_index = words[pc + 1] as usize;
                    pc += 2;
                    let call = owned_constant_at(&constants, call_index as i64, "GNU opcode");
                    let symbol = owned_constant_at(&constants, symbol_index as i64, "GNU opcode");
                    if TYPEOF(symbol) != SEXPTYPE::SYMSXP {
                        bc_error(format!(
                            "GNU DOLLAR constant pool entry {symbol_index} is not a symbol"
                        ));
                    }
                    let x_owned = stack_pop_checked(&mut stack, "GNU DOLLAR");
                    let x = x_owned.as_raw();
                    let result =
                        with_stack_rooted(&stack, x, || eval_gnu_dollar(call, symbol, x, rho));
                    super::super::runtime::set_visible(TRUE);
                    stack.push(result);
                }),
                super::super::bytecode::GNU_OP_DOLLARGETS => execute_gnu_instruction(|| {
                    let call_index = words[pc] as usize;
                    let symbol_index = words[pc + 1] as usize;
                    pc += 2;
                    let call = owned_constant_at(&constants, call_index as i64, "GNU opcode");
                    let symbol = owned_constant_at(&constants, symbol_index as i64, "GNU opcode");
                    if TYPEOF(symbol) != SEXPTYPE::SYMSXP {
                        bc_error(format!(
                            "GNU DOLLARGETS constant pool entry {symbol_index} is not a symbol"
                        ));
                    }
                    let rhs_owned = stack_pop_checked(&mut stack, "GNU DOLLARGETS rhs");
                    let rhs = rhs_owned.as_raw();
                    let x_owned = stack_pop_checked(&mut stack, "GNU DOLLARGETS lhs");
                    let x = x_owned.as_raw();
                    let result = with_stack_rooted(&stack, rhs, || {
                        eval_gnu_dollargets(call, symbol, x, rhs, rho)
                    });
                    stack.push(result);
                }),
                super::super::bytecode::GNU_OP_STARTASSIGN => execute_gnu_instruction(|| {
                    let index = words[pc] as usize;
                    pc += 1;
                    let symbol = owned_constant_at(&constants, index as i64, "GNU opcode");
                    if TYPEOF(symbol) != SEXPTYPE::SYMSXP {
                        bc_error(format!(
                            "GNU STARTASSIGN constant pool entry {index} is not a symbol"
                        ));
                    }
                    let rhs = stack_top_checked(&stack, "GNU STARTASSIGN rhs");
                    let mut loc = super::super::missing::R_varloc_t {
                        cell: ptr::null_mut(),
                    };
                    let lhs = with_stack_rooted(&stack, symbol, || {
                        // GNU STARTASSIGN: BINDING_VALUE is unforced. Promises
                        // go through EnsureLocal so the frame cell is a local
                        // value before SETTER_CALL (eval.c:8237-8254).
                        let bound = find_var_unforced(symbol, rho);
                        let value =
                            if bound == R_UnboundValue() || TYPEOF(bound) == SEXPTYPE::PROMSXP {
                                super::super::missing::EnsureLocal(symbol, rho, &mut loc)
                            } else {
                                loc = super::super::missing::R_findVarLocInFrame(rho, symbol);
                                bound
                            };
                        if crate::sexp::accessors::NAMED(value) > 1 {
                            crate::mainutils::duplicate::shallow_duplicate(value)
                        } else {
                            value
                        }
                    });
                    let cell = if loc.cell.is_null() {
                        R_NilValue()
                    } else {
                        loc.cell
                    };
                    stack.push(cell);
                    stack.push(lhs);
                    stack.push(rhs);
                }),
                super::super::bytecode::GNU_OP_ENDASSIGN => execute_gnu_instruction(|| {
                    let index = words[pc] as usize;
                    pc += 1;
                    let symbol = owned_constant_at(&constants, index as i64, "GNU opcode");
                    if TYPEOF(symbol) != SEXPTYPE::SYMSXP {
                        bc_error(format!(
                            "GNU ENDASSIGN constant pool entry {index} is not a symbol"
                        ));
                    }
                    let value_owned = stack_pop_checked(&mut stack, "GNU ENDASSIGN value");
                    let value = value_owned.as_raw();
                    let cell_owned = stack_pop_checked(&mut stack, "GNU ENDASSIGN cell");
                    let cell = cell_owned.as_raw();
                    with_stack_rooted(&stack, value, || {
                        // GNU SET_BINDING_VALUE on the STARTASSIGN cell; fall
                        // back to defineVar when the cell was not recorded.
                        if !cell.is_null()
                            && cell != R_NilValue()
                            && TYPEOF(cell) == SEXPTYPE::LISTSXP
                        {
                            crate::sexp::accessors::SETCAR(cell, value);
                            crate::sexp::accessors::SET_MISSING(cell, 0);
                        }
                        defineVar(symbol, value, rho);
                    });
                    super::super::runtime::set_visible(FALSE);
                }),
                super::super::bytecode::GNU_OP_STARTASSIGN2 => execute_gnu_instruction(|| {
                    let index = words[pc] as usize;
                    pc += 1;
                    let symbol = owned_constant_at(&constants, index as i64, "GNU opcode");
                    if TYPEOF(symbol) != SEXPTYPE::SYMSXP {
                        bc_error(format!(
                            "GNU STARTASSIGN2 constant pool entry {index} is not a symbol"
                        ));
                    }
                    let rhs = stack_top_checked(&stack, "GNU STARTASSIGN2 rhs");
                    let parent = ENCLOS(rho);
                    let lhs = with_stack_rooted(&stack, symbol, || {
                        let value = R_findVar(symbol, parent);
                        if value == R_UnboundValue() {
                            bc_error("object not found");
                        }
                        if TYPEOF(value) == SEXPTYPE::PROMSXP {
                            forcePromise(value)
                        } else {
                            value
                        }
                    });
                    let lhs = if crate::sexp::accessors::NAMED(lhs) > 1 {
                        with_stack_rooted(&stack, lhs, || {
                            crate::mainutils::duplicate::shallow_duplicate(lhs)
                        })
                    } else {
                        lhs
                    };
                    stack.push(R_NilValue());
                    stack.push(lhs);
                    stack.push(rhs);
                }),
                super::super::bytecode::GNU_OP_ENDASSIGN2 => execute_gnu_instruction(|| {
                    let index = words[pc] as usize;
                    pc += 1;
                    let symbol = owned_constant_at(&constants, index as i64, "GNU opcode");
                    if TYPEOF(symbol) != SEXPTYPE::SYMSXP {
                        bc_error(format!(
                            "GNU ENDASSIGN2 constant pool entry {index} is not a symbol"
                        ));
                    }
                    let value_owned = stack_pop_checked(&mut stack, "GNU ENDASSIGN2 value");
                    let value = value_owned.as_raw();
                    let _cell_owned = stack_pop_checked(&mut stack, "GNU ENDASSIGN2 cell");
                    let _cell = _cell_owned.as_raw();
                    with_stack_rooted(&stack, value, || setVar(symbol, value, ENCLOS(rho)));
                    super::super::runtime::set_visible(FALSE);
                }),
                super::super::bytecode::GNU_OP_STARTSUBSET
                | super::super::bytecode::GNU_OP_STARTSUBSET2 => execute_gnu_instruction(|| {
                    let call_index = words[pc] as usize;
                    let target = words[pc + 1] as usize;
                    pc += 2;
                    let call = owned_constant_at(&constants, call_index as i64, "GNU opcode");
                    let x = stack_top_checked(&stack, "GNU STARTSUBSET");
                    let generic = if opcode == super::super::bytecode::GNU_OP_STARTSUBSET2 {
                        c"[["
                    } else {
                        c"["
                    };
                    if let Some(value) = with_stack_rooted(&stack, x, || {
                        eval_gnu_startsubset_n(generic, call, x, rho)
                    }) {
                        let index = stack.depth() - 1;
                        stack.set(index, value);
                        pc = target;
                    } else {
                        let tag = if !call.is_null() && TYPEOF(call) == SEXPTYPE::LANGSXP {
                            crate::sexp::accessors::TAG(CDR(call))
                        } else {
                            R_NilValue()
                        };
                        let mut tags = Vec::new();
                        if tag != R_NilValue() {
                            tags.push((stack.depth() - 1, own_operand(tag)));
                        }
                        gnu_call_frames.push(GnuCallFrame {
                            marker: stack.depth() - 1,
                            tags,
                            raw_args: true,
                            call: own_operand(call),
                        });
                    }
                }),
                super::super::bytecode::GNU_OP_DOMISSING => execute_gnu_instruction(|| {
                    if gnu_call_frames.is_empty() {
                        bc_error("GNU DOMISSING has no active call frame");
                    }
                    stack.push(R_MissingArg());
                }),
                super::super::bytecode::GNU_OP_DFLTSUBSET
                | super::super::bytecode::GNU_OP_DFLTSUBSET2 => execute_gnu_instruction(|| {
                    let frame = gnu_call_frames.pop().unwrap_or_else(|| {
                        bc_error("GNU DFLTSUBSET has no active STARTSUBSET frame")
                    });
                    if !frame.raw_args {
                        bc_error("GNU DFLTSUBSET requires a STARTSUBSET call frame");
                    }
                    let depth = stack.depth();
                    if depth <= frame.marker {
                        bc_error("GNU DFLTSUBSET has no object");
                    }
                    let x = stack_at_checked(&stack, frame.marker, "GNU DFLTSUBSET object");
                    let result = with_stack_rooted(&stack, x, || {
                        let mut args = R_NilValue();
                        let mut argument_roots = Vec::new();
                        for index in (frame.marker..depth).rev() {
                            args = Rf_cons(stack.at(index), args);
                            argument_roots.push(own_operand(args));
                            if let Some((_, tag)) =
                                frame.tags.iter().find(|(slot, _)| *slot == index)
                            {
                                crate::sexp::accessors::SETTAG(args, tag.as_raw());
                            }
                        }
                        let generic = if opcode == super::super::bytecode::GNU_OP_DFLTSUBSET2 {
                            c"[["
                        } else {
                            c"["
                        };
                        if let Some(shadowed) =
                            eval_gnu_lexical_generic(generic, frame.call.as_raw(), args, rho)
                        {
                            shadowed
                        } else if opcode == super::super::bytecode::GNU_OP_DFLTSUBSET2 {
                            crate::mainutils::subset::do_subset2_dflt(
                                frame.call.as_raw(),
                                crate::sexp::symbol::Rf_install(c"[[".as_ptr()),
                                args,
                                rho,
                            )
                        } else {
                            crate::mainutils::subset::subset_default_after_failed_dispatch(
                                frame.call.as_raw(),
                                crate::sexp::symbol::Rf_install(c"[".as_ptr()),
                                args,
                                rho,
                            )
                        }
                    });
                    super::super::runtime::set_visible(TRUE);
                    stack.set_depth(frame.marker);
                    stack.push(result);
                }),
                super::super::bytecode::GNU_OP_STARTSUBASSIGN
                | super::super::bytecode::GNU_OP_STARTSUBASSIGN2 => execute_gnu_instruction(|| {
                    let call_index = words[pc] as usize;
                    let target = words[pc + 1] as usize;
                    pc += 2;
                    let call = owned_constant_at(&constants, call_index as i64, "GNU opcode");
                    if stack.depth() < 2 {
                        bc_error("GNU STARTSUBASSIGN requires lhs and rhs");
                    }
                    let rhs = stack_top_checked(&stack, "GNU STARTSUBASSIGN rhs");
                    let lhs_slot = stack.depth() - 2;
                    let lhs = stack_at_checked(&stack, lhs_slot, "GNU STARTSUBASSIGN lhs");
                    let lhs = if crate::sexp::accessors::NAMED(lhs) > 1 {
                        let copy = with_stack_rooted(&stack, lhs, || {
                            crate::mainutils::duplicate::shallow_duplicate(lhs)
                        });
                        stack.set(lhs_slot, copy);
                        copy
                    } else {
                        lhs
                    };
                    if let Some(value) = with_stack_rooted(&stack, rhs, || {
                        let generic = if opcode == super::super::bytecode::GNU_OP_STARTSUBASSIGN2 {
                            c"[[<-"
                        } else {
                            c"[<-"
                        };
                        eval_gnu_startsubassign_n(generic, call, lhs, rhs, rho)
                    }) {
                        stack_pop_checked(&mut stack, "GNU STARTSUBASSIGN dispatched rhs");
                        let index = stack.depth() - 1;
                        stack.set(index, value);
                        pc = target;
                    } else {
                        let tag = if !call.is_null() && TYPEOF(call) == SEXPTYPE::LANGSXP {
                            crate::sexp::accessors::TAG(CDR(call))
                        } else {
                            R_NilValue()
                        };
                        let mut tags = Vec::new();
                        if tag != R_NilValue() {
                            tags.push((lhs_slot, own_operand(tag)));
                        }
                        gnu_call_frames.push(GnuCallFrame {
                            marker: lhs_slot,
                            tags,
                            raw_args: true,
                            call: own_operand(call),
                        });
                    }
                }),
                super::super::bytecode::GNU_OP_DFLTSUBASSIGN
                | super::super::bytecode::GNU_OP_DFLTSUBASSIGN2 => execute_gnu_instruction(|| {
                    let frame = gnu_call_frames.pop().unwrap_or_else(|| {
                        bc_error("GNU DFLTSUBASSIGN has no active STARTSUBASSIGN frame")
                    });
                    if !frame.raw_args {
                        bc_error("GNU DFLTSUBASSIGN requires a STARTSUBASSIGN call frame");
                    }
                    let depth = stack.depth();
                    if depth < frame.marker + 2 {
                        bc_error("GNU DFLTSUBASSIGN requires lhs and rhs");
                    }
                    let x = stack_at_checked(&stack, frame.marker, "GNU DFLTSUBASSIGN object");
                    let rhs = stack_at_checked(&stack, frame.marker + 1, "GNU DFLTSUBASSIGN rhs");
                    let result = with_stack_rooted(&stack, x, || {
                        let mut args = R_NilValue();
                        let mut argument_roots = Vec::new();
                        args = Rf_cons(rhs, args);
                        argument_roots.push(own_operand(args));
                        crate::sexp::accessors::SETTAG(
                            args,
                            crate::sexp::symbol::Rf_install(c"value".as_ptr()),
                        );
                        for index in (frame.marker + 2..depth).rev() {
                            args = Rf_cons(stack.at(index), args);
                            argument_roots.push(own_operand(args));
                            if let Some((_, tag)) =
                                frame.tags.iter().find(|(slot, _)| *slot == index)
                            {
                                crate::sexp::accessors::SETTAG(args, tag.as_raw());
                            }
                        }
                        args = Rf_cons(x, args);
                        argument_roots.push(own_operand(args));
                        if let Some((_, tag)) =
                            frame.tags.iter().find(|(slot, _)| *slot == frame.marker)
                        {
                            crate::sexp::accessors::SETTAG(args, tag.as_raw());
                        }
                        if opcode == super::super::bytecode::GNU_OP_DFLTSUBASSIGN2 {
                            crate::mainutils::subassign::do_subassign2_dflt(
                                frame.call.as_raw(),
                                crate::sexp::symbol::Rf_install(c"[[<-".as_ptr()),
                                args,
                                rho,
                            )
                        } else {
                            crate::mainutils::subassign::do_subassign_dflt(
                                frame.call.as_raw(),
                                crate::sexp::symbol::Rf_install(c"[<-".as_ptr()),
                                args,
                                rho,
                            )
                        }
                    });
                    stack.set_depth(frame.marker);
                    stack.push(result);
                }),
                super::super::bytecode::GNU_OP_STARTSUBSET_N
                | super::super::bytecode::GNU_OP_STARTSUBSET2_N => execute_gnu_instruction(|| {
                    let call_index = words[pc] as usize;
                    let target = words[pc + 1] as usize;
                    pc += 2;
                    let call = owned_constant_at(&constants, call_index as i64, "GNU opcode");
                    let x = stack_top_checked(&stack, "GNU STARTSUBSET_N");
                    let generic = if opcode == super::super::bytecode::GNU_OP_STARTSUBSET2_N {
                        c"[["
                    } else {
                        c"["
                    };
                    if let Some(value) = with_stack_rooted(&stack, x, || {
                        eval_gnu_startsubset_n(generic, call, x, rho)
                    }) {
                        let index = stack.depth() - 1;
                        stack.set(index, value);
                        pc = target;
                    }
                }),
                super::super::bytecode::GNU_OP_STARTSUBASSIGN_N
                | super::super::bytecode::GNU_OP_STARTSUBASSIGN2_N => {
                    execute_gnu_instruction(|| {
                        if stack.depth() < 2 {
                            bc_error("GNU STARTSUBASSIGN_N has an empty stack");
                        }
                        let call_index = words[pc] as usize;
                        let target = words[pc + 1] as usize;
                        pc += 2;
                        let call = owned_constant_at(&constants, call_index as i64, "GNU opcode");
                        let rhs = stack_top_checked(&stack, "GNU STARTSUBASSIGN_N rhs");
                        let lhs_slot = stack.depth() - 2;
                        let lhs = stack_at_checked(&stack, lhs_slot, "GNU STARTSUBASSIGN_N lhs");
                        let lhs = if crate::sexp::accessors::NAMED(lhs) > 1 {
                            let copy = with_stack_rooted(&stack, lhs, || {
                                crate::mainutils::duplicate::shallow_duplicate(lhs)
                            });
                            stack.set(lhs_slot, copy);
                            copy
                        } else {
                            lhs
                        };
                        if let Some(value) = with_stack_rooted(&stack, rhs, || {
                            let generic =
                                if opcode == super::super::bytecode::GNU_OP_STARTSUBASSIGN2_N {
                                    c"[[<-"
                                } else {
                                    c"[<-"
                                };
                            eval_gnu_startsubassign_n(generic, call, lhs, rhs, rho)
                        }) {
                            stack_pop_checked(&mut stack, "GNU STARTSUBASSIGN_N dispatched rhs");
                            let index = stack.depth() - 1;
                            stack.set(index, value);
                            pc = target;
                        }
                    })
                }
                super::super::bytecode::GNU_OP_VECSUBSET
                | super::super::bytecode::GNU_OP_VECSUBSET2 => execute_gnu_instruction(|| {
                    let call_index = words[pc] as usize;
                    pc += 1;
                    let call = owned_constant_at(&constants, call_index as i64, "GNU opcode");
                    let index_owned = stack_pop_checked(&mut stack, "GNU VECSUBSET index");
                    let index = index_owned.as_raw();
                    let x_owned = stack_pop_checked(&mut stack, "GNU VECSUBSET object");
                    let x = x_owned.as_raw();
                    let subset2 = opcode == super::super::bytecode::GNU_OP_VECSUBSET2;
                    let result = with_stack_rooted(&stack, index, || {
                        eval_gnu_vecsubset(call, x, index, rho, subset2)
                    });
                    super::super::runtime::set_visible(TRUE);
                    stack.push(result);
                }),
                super::super::bytecode::GNU_OP_MATSUBSET
                | super::super::bytecode::GNU_OP_MATSUBSET2 => execute_gnu_instruction(|| {
                    let call_index = words[pc] as usize;
                    pc += 1;
                    let call = owned_constant_at(&constants, call_index as i64, "GNU opcode");
                    let column_owned = stack_pop_checked(&mut stack, "GNU MATSUBSET column");
                    let column = column_owned.as_raw();
                    let row_owned = stack_pop_checked(&mut stack, "GNU MATSUBSET row");
                    let row = row_owned.as_raw();
                    let x_owned = stack_pop_checked(&mut stack, "GNU MATSUBSET object");
                    let x = x_owned.as_raw();
                    let result = with_stack_rooted(&stack, column, || {
                        eval_gnu_subset_indices(
                            call,
                            x,
                            &[row, column],
                            rho,
                            opcode == super::super::bytecode::GNU_OP_MATSUBSET2,
                        )
                    });
                    super::super::runtime::set_visible(TRUE);
                    stack.push(result);
                }),
                super::super::bytecode::GNU_OP_SUBSET_N
                | super::super::bytecode::GNU_OP_SUBSET2_N => execute_gnu_instruction(|| {
                    let call_index = words[pc] as usize;
                    let rank = words[pc + 1];
                    pc += 2;
                    if rank < 0 {
                        bc_error("GNU SUBSET_N rank is negative");
                    }
                    let call = owned_constant_at(&constants, call_index as i64, "GNU opcode");
                    let mut indices = Vec::with_capacity(rank as usize);
                    for _ in 0..rank {
                        indices.push(stack_pop_checked(&mut stack, "GNU SUBSET_N index"));
                    }
                    indices.reverse();
                    let x_owned = stack_pop_checked(&mut stack, "GNU SUBSET_N object");
                    let x = x_owned.as_raw();
                    let result = with_stack_rooted(&stack, x, || {
                        eval_gnu_subset_indices(
                            call,
                            x,
                            &indices.iter().map(Sexp::as_raw).collect::<Vec<_>>(),
                            rho,
                            opcode == super::super::bytecode::GNU_OP_SUBSET2_N,
                        )
                    });
                    super::super::runtime::set_visible(TRUE);
                    stack.push(result);
                }),
                super::super::bytecode::GNU_OP_VECSUBASSIGN
                | super::super::bytecode::GNU_OP_VECSUBASSIGN2 => execute_gnu_instruction(|| {
                    let call_index = words[pc] as usize;
                    pc += 1;
                    let call = owned_constant_at(&constants, call_index as i64, "GNU opcode");
                    let index_owned = stack_pop_checked(&mut stack, "GNU VECSUBASSIGN index");
                    let index = index_owned.as_raw();
                    let rhs_owned = stack_pop_checked(&mut stack, "GNU VECSUBASSIGN rhs");
                    let rhs = rhs_owned.as_raw();
                    let x_owned = stack_pop_checked(&mut stack, "GNU VECSUBASSIGN object");
                    let x = x_owned.as_raw();
                    let subset2 = opcode == super::super::bytecode::GNU_OP_VECSUBASSIGN2;
                    let result = with_stack_rooted(&stack, rhs, || {
                        eval_gnu_vecsubassign(call, x, rhs, index, rho, subset2)
                    });
                    stack.push(result);
                }),
                super::super::bytecode::GNU_OP_MATSUBASSIGN
                | super::super::bytecode::GNU_OP_MATSUBASSIGN2 => execute_gnu_instruction(|| {
                    let call_index = words[pc] as usize;
                    pc += 1;
                    let call = owned_constant_at(&constants, call_index as i64, "GNU opcode");
                    let column_owned = stack_pop_checked(&mut stack, "GNU MATSUBASSIGN column");
                    let column = column_owned.as_raw();
                    let row_owned = stack_pop_checked(&mut stack, "GNU MATSUBASSIGN row");
                    let row = row_owned.as_raw();
                    let rhs_owned = stack_pop_checked(&mut stack, "GNU MATSUBASSIGN rhs");
                    let rhs = rhs_owned.as_raw();
                    let x_owned = stack_pop_checked(&mut stack, "GNU MATSUBASSIGN object");
                    let x = x_owned.as_raw();
                    let result = with_stack_rooted(&stack, rhs, || {
                        eval_gnu_matsubassign(
                            call,
                            x,
                            rhs,
                            row,
                            column,
                            rho,
                            opcode == super::super::bytecode::GNU_OP_MATSUBASSIGN2,
                        )
                    });
                    stack.push(result);
                }),
                super::super::bytecode::GNU_OP_SUBASSIGN_N
                | super::super::bytecode::GNU_OP_SUBASSIGN2_N => execute_gnu_instruction(|| {
                    let call_index = words[pc] as usize;
                    let rank = words[pc + 1];
                    pc += 2;
                    if rank < 0 {
                        bc_error("GNU SUBASSIGN_N rank is negative");
                    }
                    let call = owned_constant_at(&constants, call_index as i64, "GNU opcode");
                    let mut indices = Vec::with_capacity(rank as usize);
                    for _ in 0..rank {
                        indices.push(stack_pop_checked(&mut stack, "GNU SUBASSIGN_N index"));
                    }
                    indices.reverse();
                    let rhs_owned = stack_pop_checked(&mut stack, "GNU SUBASSIGN_N rhs");
                    let rhs = rhs_owned.as_raw();
                    let x_owned = stack_pop_checked(&mut stack, "GNU SUBASSIGN_N object");
                    let x = x_owned.as_raw();
                    let result = with_stack_rooted(&stack, rhs, || {
                        eval_gnu_subassign_indices(
                            call,
                            x,
                            rhs,
                            &indices.iter().map(Sexp::as_raw).collect::<Vec<_>>(),
                            rho,
                            opcode == super::super::bytecode::GNU_OP_SUBASSIGN2_N,
                        )
                    });
                    stack.push(result);
                }),
                super::super::bytecode::GNU_OP_SETTER_CALL => execute_gnu_instruction(|| {
                    let call_index = words[pc] as usize;
                    let vexpr_index = words[pc + 1] as usize;
                    pc += 2;
                    let frame = gnu_call_frames
                        .pop()
                        .unwrap_or_else(|| bc_error("GNU SETTER_CALL has no active GETFUN call"));
                    if frame.raw_args {
                        bc_error("GNU SETTER_CALL requires a GETFUN call frame");
                    }
                    let marker = frame.marker;
                    let depth = stack.depth();
                    if marker < 2 {
                        bc_error("GNU SETTER_CALL has an empty stack");
                    }
                    if depth <= marker {
                        bc_error("GNU SETTER_CALL has no function");
                    }
                    let call_expr = owned_constant_at(&constants, call_index as i64, "GNU opcode");
                    let vexpr = owned_constant_at(&constants, vexpr_index as i64, "GNU opcode");
                    if TYPEOF(call_expr) != SEXPTYPE::LANGSXP {
                        bc_error(format!(
                            "GNU SETTER_CALL expression constant {call_index} is not a language object"
                        ));
                    }
                    let mut lhs = stack_at_checked(&stack, marker - 2, "GNU SETTER_CALL lhs");
                    let rhs = stack_at_checked(&stack, marker - 1, "GNU SETTER_CALL rhs");
                    if crate::sexp::accessors::NAMED(lhs) > 1 {
                        lhs = with_stack_rooted(&stack, lhs, || {
                            crate::mainutils::duplicate::shallow_duplicate(lhs)
                        });
                    }
                    let fun = stack.at(marker);
                    let result = with_stack_rooted(&stack, call_expr, || {
                        let mut args = R_NilValue();
                        let mut argument_roots = Vec::new();
                        for index in (marker + 1..depth).rev() {
                            args = Rf_cons(stack.at(index), args);
                            argument_roots.push(own_operand(args));
                            if let Some((_, tag)) =
                                frame.tags.iter().find(|(slot, _)| *slot == index)
                            {
                                crate::sexp::accessors::SETTAG(args, tag.as_raw());
                            }
                        }
                        eval_gnu_setter_call(fun, call_expr, vexpr, lhs, rhs, args, rho)
                    });
                    stack.set_depth(marker - 2);
                    stack.push(result);
                }),

                super::super::bytecode::GNU_OP_GETTER_CALL => execute_gnu_instruction(|| {
                    let call_index = words[pc] as usize;
                    pc += 1;
                    let frame = gnu_call_frames
                        .pop()
                        .unwrap_or_else(|| bc_error("GNU GETTER_CALL has no active GETFUN call"));
                    if frame.raw_args {
                        bc_error("GNU GETTER_CALL requires a GETFUN call frame");
                    }
                    let marker = frame.marker;
                    let depth = stack.depth();
                    if marker < 2 {
                        bc_error("GNU GETTER_CALL has an empty stack");
                    }
                    if depth <= marker {
                        bc_error("GNU GETTER_CALL has no function");
                    }
                    let call_expr = owned_constant_at(&constants, call_index as i64, "GNU opcode");
                    if TYPEOF(call_expr) != SEXPTYPE::LANGSXP {
                        bc_error(format!(
                            "GNU GETTER_CALL expression constant {call_index} is not a language object"
                        ));
                    }
                    let lhs = stack_at_checked(&stack, marker - 2, "GNU GETTER_CALL lhs");
                    if crate::sexp::accessors::NAMED(lhs) < 2 {
                        crate::sexp::accessors::SET_NAMED(lhs, 2);
                    }
                    let result = with_stack_rooted(&stack, call_expr, || {
                        let fun = stack.at(marker);
                        let mut args = R_NilValue();
                        let mut argument_roots = Vec::new();
                        for index in (marker + 1..depth).rev() {
                            args = Rf_cons(stack.at(index), args);
                            argument_roots.push(own_operand(args));
                            if let Some((_, tag)) =
                                frame.tags.iter().find(|(slot, _)| *slot == index)
                            {
                                crate::sexp::accessors::SETTAG(args, tag.as_raw());
                            }
                        }
                        eval_gnu_getter_call(fun, call_expr, lhs, args, rho)
                    });
                    stack.set_depth(marker);
                    stack.push(result);
                }),
                super::super::bytecode::GNU_OP_SWAP => execute_gnu_instruction(|| {
                    if stack.depth() < 2 {
                        bc_error("GNU SWAP has an empty stack");
                    }
                    let top_idx = stack.depth() - 1;
                    let second_idx = top_idx - 1;
                    let mut top = stack.at(top_idx);
                    // SWAP only appears between getter and replacement.
                    // Always copy the extracted value so later [<- cannot
                    // mutate a names/attr vector still shared with the caller.
                    top = with_stack_rooted(&stack, top, || {
                        crate::mainutils::duplicate::shallow_duplicate(top)
                    });
                    let second = stack.at(second_idx);
                    stack.set(top_idx, second);
                    stack.set(second_idx, top);
                }),
                super::super::bytecode::GNU_OP_STARTLOOPCNTXT => execute_gnu_instruction(|| {
                    let _is_for_loop = words[pc];
                    let break_target = words[pc + 1];
                    pc += 2;
                    // GNU duplicates for-loop node-stack state when is_for_loop
                    // is set. The adapter keeps that state in for_loops, so
                    // only the recoverable next/break pc pair is recorded.
                    loop_stack.push(LoopContext {
                        break_target,
                        next_target: pc as c_int,
                        stack_depth: stack.depth(),
                        for_depth: for_loops.len(),
                    });
                }),
                super::super::bytecode::GNU_OP_ENDLOOPCNTXT => execute_gnu_instruction(|| {
                    let _is_for_loop = words[pc];
                    pc += 1;
                    if loop_stack.pop().is_none() {
                        bc_error("GNU ENDLOOPCNTXT has no active loop context");
                    }
                }),
                super::super::bytecode::GNU_OP_RETURNJMP => {
                    return stack_pop_checked(&mut stack, "GNU RETURNJMP");
                }
                super::super::bytecode::GNU_OP_DOLOOPNEXT
                | super::super::bytecode::GNU_OP_DOLOOPBREAK => execute_gnu_instruction(|| {
                    let is_break = opcode == super::super::bytecode::GNU_OP_DOLOOPBREAK;
                    let Some(ctx) = loop_stack.last() else {
                        bc_error(if is_break {
                            "GNU DOLOOPBREAK has no active loop context"
                        } else {
                            "GNU DOLOOPNEXT has no active loop context"
                        });
                    };
                    let jump = loop_jump_from_context(ctx, is_break);
                    pc = apply_loop_jump(&mut stack, &mut for_loops, jump) as usize;
                }),
                super::super::bytecode::GNU_OP_DOTCALL => execute_gnu_instruction(|| {
                    let call_index = words[pc] as usize;
                    let nargs = words[pc + 1];
                    pc += 2;
                    if nargs < 0 {
                        bc_error(format!("GNU DOTCALL nargs {nargs} is negative"));
                    }
                    let needed = nargs as usize + 1;
                    if stack.depth() < needed {
                        bc_error(format!(
                            "GNU DOTCALL has stack depth {}, requires {needed}",
                            stack.depth()
                        ));
                    }
                    let call = owned_constant_at(&constants, call_index as i64, "GNU opcode");
                    if call.is_null() || TYPEOF(call) != SEXPTYPE::LANGSXP {
                        bc_error("GNU DOTCALL requires a call in the constant pool");
                    }
                    let depth = stack.depth();
                    let fun_idx = depth - needed;
                    let result = with_stack_rooted(&stack, call, || {
                        let mut args = R_NilValue();
                        let mut roots = Vec::new();
                        for index in (fun_idx..depth).rev() {
                            args = Rf_cons(stack.at(index), args);
                            roots.push(own_operand(args));
                        }
                        let sym = CAR(call);
                        let op =
                            crate::sexp::envir::findFun(sym, super::super::runtime::base_env());
                        crate::mainutils::dotcode::do_dotcall(call, op, args, rho)
                    });
                    stack.set_depth(fun_idx);
                    super::super::runtime::set_visible(TRUE);
                    stack.push(result);
                }),
                _ => bc_mismatch(format!("unsupported tagged GNU bytecode opcode {opcode}")),
            }
        }
        bc_error("GNU bytecode ended without RETURN")
    }
}
