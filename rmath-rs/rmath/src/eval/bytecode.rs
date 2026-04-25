//! R bytecode evaluation.
//!
//! R compiles expressions to bytecode for faster execution.
//! The bytecode format is a vector of integers where each
//! instruction is an opcode followed by operand indices.

use std::os::raw::{c_double, c_int};

use crate::sexp::ffi::SEXPTYPE;
use crate::sexp::memory::with_arena;
use crate::sexp::safe::{Sexp, SexpError};

fn sexp_err(context: &str, err: SexpError) -> String {
    format!("{context}: {err}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlFlow {
    Normal,
    Break,
    Next,
}

pub const BCreturn: c_int = 0;
pub const BCgvar: c_int = 1;
pub const BCsvar: c_int = 2;
pub const BCint: c_int = 3;
pub const BCreal: c_int = 4;
pub const BCstring: c_int = 5;
pub const BCtrue: c_int = 6;
pub const BCfalse: c_int = 7;
pub const BCnil: c_int = 8;
pub const BCdot: c_int = 9;
pub const BCnot: c_int = 10;
pub const BCadd: c_int = 11;
pub const BCsub: c_int = 12;
pub const BCmul: c_int = 13;
pub const BCdiv: c_int = 14;
pub const BCeq: c_int = 15;
pub const BCne: c_int = 16;
pub const BClt: c_int = 17;
pub const BCle: c_int = 18;
pub const BCgt: c_int = 19;
pub const BCge: c_int = 20;
pub const BCand: c_int = 21;
pub const BCor: c_int = 22;
pub const BCcall: c_int = 23;
pub const BCpush: c_int = 24;
pub const BCpop: c_int = 25;
pub const BCdup: c_int = 26;
pub const BCprint: c_int = 27;
pub const BCbegin: c_int = 28;
pub const BCif: c_int = 29;
pub const BCjump: c_int = 30;
pub const BCfjmp: c_int = 31;
pub const BCtjmp: c_int = 32;
pub const BCfor: c_int = 33;
pub const BCwhile: c_int = 34;
pub const BCrepeat: c_int = 35;
pub const BCbreak: c_int = 36;
pub const BCnext: c_int = 37;
pub const BCclosure: c_int = 38;
pub const BCspecial: c_int = 39;
pub const BCbuiltin: c_int = 40;
pub const BCneg: c_int = 41;
pub const BCmod: c_int = 42;
pub const BCpow: c_int = 43;

fn make_lgl<'a>(val: c_int) -> Result<Sexp<'a>, String> {
    let lgl = with_arena(|arena| arena.alloc_vector(SEXPTYPE::LGLSXP, 1));
    if lgl.is_null() {
        return Err("failed to allocate logical scalar".to_string());
    }
    let sexp = Sexp::from_raw(lgl).ok_or_else(|| "invalid logical scalar pointer".to_string())?;
    sexp.try_set_logical_elt(0, val)
        .map_err(|err| sexp_err("failed to initialize logical scalar", err))?;
    unsafe {
        (*lgl).sxpinfo.set_scalar(true);
    }
    Ok(sexp)
}

fn make_real<'a>(val: c_double) -> Result<Sexp<'a>, String> {
    let real = with_arena(|arena| arena.alloc_vector(SEXPTYPE::REALSXP, 1));
    if real.is_null() {
        return Err("failed to allocate real scalar".to_string());
    }
    let sexp = Sexp::from_raw(real).ok_or_else(|| "invalid real scalar pointer".to_string())?;
    sexp.try_set_real_elt(0, val)
        .map_err(|err| sexp_err("failed to initialize real scalar", err))?;
    unsafe {
        (*real).sxpinfo.set_scalar(true);
    }
    Ok(sexp)
}

fn make_int<'a>(val: c_int) -> Result<Sexp<'a>, String> {
    let int = with_arena(|arena| arena.alloc_vector(SEXPTYPE::INTSXP, 1));
    if int.is_null() {
        return Err("failed to allocate integer scalar".to_string());
    }
    let sexp = Sexp::from_raw(int).ok_or_else(|| "invalid integer scalar pointer".to_string())?;
    sexp.try_set_integer_elt(0, val)
        .map_err(|err| sexp_err("failed to initialize integer scalar", err))?;
    unsafe {
        (*int).sxpinfo.set_scalar(true);
    }
    Ok(sexp)
}

fn scalar_int(value: Sexp<'_>, context: &str) -> Result<c_int, String> {
    value
        .try_integer_elt(0)
        .map_err(|err| sexp_err(context, err))
}

fn scalar_real(value: Sexp<'_>, context: &str) -> Result<c_double, String> {
    value.try_real_elt(0).map_err(|err| sexp_err(context, err))
}

fn scalar_f64_or_zero(value: Sexp<'_>, context: &str) -> Result<c_double, String> {
    match value.typeof_() {
        SEXPTYPE::REALSXP => scalar_real(value, context),
        SEXPTYPE::INTSXP => scalar_int(value, context).map(c_double::from),
        SEXPTYPE::LGLSXP => value
            .try_logical_elt(0)
            .map(c_double::from)
            .map_err(|err| sexp_err(context, err)),
        _ => Ok(0.0),
    }
}

fn scalar_bool_or_false(value: Sexp<'_>, context: &str) -> Result<bool, String> {
    match value.typeof_() {
        SEXPTYPE::LGLSXP => value
            .try_logical_elt(0)
            .map(|value| value != 0)
            .map_err(|err| sexp_err(context, err)),
        SEXPTYPE::INTSXP => scalar_int(value, context).map(|value| value != 0),
        SEXPTYPE::REALSXP => scalar_real(value, context).map(|value| value != 0.0),
        _ => Ok(false),
    }
}

fn apply_binary_op<'a, FR, FI>(
    a: Sexp<'a>,
    b: Sexp<'a>,
    real_op: FR,
    int_op: FI,
) -> Result<Sexp<'a>, String>
where
    FR: Fn(c_double, c_double) -> c_double,
    FI: Fn(c_int, c_int) -> c_int,
{
    if a.typeof_() == SEXPTYPE::REALSXP && b.typeof_() == SEXPTYPE::REALSXP {
        let av = scalar_real(a, "left real operand")?;
        let bv = scalar_real(b, "right real operand")?;
        make_real(real_op(av, bv))
    } else if a.typeof_() == SEXPTYPE::INTSXP && b.typeof_() == SEXPTYPE::INTSXP {
        let av = scalar_int(a, "left integer operand")?;
        let bv = scalar_int(b, "right integer operand")?;
        make_int(int_op(av, bv))
    } else {
        let av = scalar_f64_or_zero(a, "left numeric operand")?;
        let bv = scalar_f64_or_zero(b, "right numeric operand")?;
        make_real(real_op(av, bv))
    }
}

fn apply_comparison<'a, F>(a: Sexp<'a>, b: Sexp<'a>, cmp: F) -> Result<Sexp<'a>, String>
where
    F: Fn(c_double, c_double) -> bool,
{
    let result = if a.typeof_() == SEXPTYPE::REALSXP && b.typeof_() == SEXPTYPE::REALSXP {
        let av = scalar_real(a, "left real comparison operand")?;
        let bv = scalar_real(b, "right real comparison operand")?;
        if cmp(av, bv) { 1 } else { 0 }
    } else if a.typeof_() == SEXPTYPE::INTSXP && b.typeof_() == SEXPTYPE::INTSXP {
        let av = scalar_int(a, "left integer comparison operand")? as c_double;
        let bv = scalar_int(b, "right integer comparison operand")? as c_double;
        if cmp(av, bv) { 1 } else { 0 }
    } else {
        let av = scalar_f64_or_zero(a, "left comparison operand")?;
        let bv = scalar_f64_or_zero(b, "right comparison operand")?;
        if cmp(av, bv) { 1 } else { 0 }
    };
    make_lgl(result)
}

pub fn eval_bytecode<'a>(code: Sexp<'a>, env: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let bytecode = code
        .try_as_integer_slice()
        .map_err(|err| sexp_err("invalid bytecode vector", err))?;
    let mut pc: usize = 0;
    let mut stack: Vec<Sexp<'a>> = Vec::new();
    let constants = code.attrib();
    eval_bytecode_loop(bytecode, &mut pc, &mut stack, constants, env).map(|(sexp, _)| sexp)
}

fn eval_bytecode_loop<'a>(
    bytecode: &[c_int],
    pc: &mut usize,
    stack: &mut Vec<Sexp<'a>>,
    constants: Option<Sexp<'a>>,
    env: Sexp<'a>,
) -> Result<(Sexp<'a>, ControlFlow), String> {
    while *pc < bytecode.len() {
        let opcode = bytecode[*pc] as c_int;
        *pc += 1;

        match opcode {
            BCreturn => {
                let val = stack
                    .pop()
                    .ok_or_else(|| "empty stack on return".to_string())?;
                return Ok((val, ControlFlow::Normal));
            }
            BCgvar | BCsvar => {
                let idx = bytecode[*pc] as usize;
                *pc += 1;
                let sym = get_constant(constants, idx)?;
                let val = crate::eval::eval::find_var_result(sym, env)?
                    .ok_or_else(|| "variable not found".to_string())?;
                stack.push(val);
            }
            BCint | BCreal | BCstring => {
                let idx = bytecode[*pc] as usize;
                *pc += 1;
                let val = get_constant(constants, idx)?;
                stack.push(val);
            }
            BCtrue => {
                stack.push(unsafe { Sexp::from_raw_unchecked(crate::sexp::globals::R_True()) });
            }
            BCfalse => {
                stack.push(unsafe { Sexp::from_raw_unchecked(crate::sexp::globals::R_False()) });
            }
            BCnil => {
                stack.push(unsafe { Sexp::from_raw_unchecked(crate::sexp::globals::R_NilValue()) });
            }
            BCdot => {
                return Err("'...' used in incorrect context".to_string());
            }
            BCnot => {
                let val = stack
                    .pop()
                    .ok_or_else(|| "empty stack on not".to_string())?;
                let v = if scalar_bool_or_false(val, "bytecode not operand")? {
                    1
                } else {
                    0
                };
                stack.push(make_lgl(if v != 0 { 0 } else { 1 })?);
            }
            BCadd => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on add".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on add".to_string())?;
                stack.push(apply_binary_op(
                    a,
                    b,
                    |x, y| x + y,
                    |x, y| x.wrapping_add(y),
                )?);
            }
            BCsub => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on sub".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on sub".to_string())?;
                stack.push(apply_binary_op(
                    a,
                    b,
                    |x, y| x - y,
                    |x, y| x.wrapping_sub(y),
                )?);
            }
            BCmul => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on mul".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on mul".to_string())?;
                stack.push(apply_binary_op(
                    a,
                    b,
                    |x, y| x * y,
                    |x, y| x.wrapping_mul(y),
                )?);
            }
            BCdiv => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on div".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on div".to_string())?;
                stack.push(apply_binary_op(a, b, |x, y| x / y, |x, y| x / y)?);
            }
            BCmod => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on mod".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on mod".to_string())?;
                if a.typeof_() == SEXPTYPE::REALSXP && b.typeof_() == SEXPTYPE::REALSXP {
                    stack.push(make_real(
                        scalar_real(a, "left real modulo operand")?
                            % scalar_real(b, "right real modulo operand")?,
                    )?);
                } else if a.typeof_() == SEXPTYPE::INTSXP && b.typeof_() == SEXPTYPE::INTSXP {
                    let bv = scalar_int(b, "right integer modulo operand")?;
                    if bv != 0 {
                        stack.push(make_int(
                            scalar_int(a, "left integer modulo operand")? % bv,
                        )?);
                    } else {
                        stack.push(make_real(f64::NAN)?);
                    }
                } else {
                    stack.push(make_real(
                        scalar_f64_or_zero(a, "left modulo operand")?
                            % scalar_f64_or_zero(b, "right modulo operand")?,
                    )?);
                }
            }
            BCpow => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on pow".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on pow".to_string())?;
                stack.push(make_real(
                    scalar_f64_or_zero(a, "left power operand")?
                        .powf(scalar_f64_or_zero(b, "right power operand")?),
                )?);
            }
            BCeq => {
                let b = stack.pop().ok_or_else(|| "empty stack on eq".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on eq".to_string())?;
                stack.push(apply_comparison(a, b, |x, y| x == y)?);
            }
            BCne => {
                let b = stack.pop().ok_or_else(|| "empty stack on ne".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on ne".to_string())?;
                stack.push(apply_comparison(a, b, |x, y| x != y)?);
            }
            BClt => {
                let b = stack.pop().ok_or_else(|| "empty stack on lt".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on lt".to_string())?;
                stack.push(apply_comparison(a, b, |x, y| x < y)?);
            }
            BCle => {
                let b = stack.pop().ok_or_else(|| "empty stack on le".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on le".to_string())?;
                stack.push(apply_comparison(a, b, |x, y| x <= y)?);
            }
            BCgt => {
                let b = stack.pop().ok_or_else(|| "empty stack on gt".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on gt".to_string())?;
                stack.push(apply_comparison(a, b, |x, y| x > y)?);
            }
            BCge => {
                let b = stack.pop().ok_or_else(|| "empty stack on ge".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on ge".to_string())?;
                stack.push(apply_comparison(a, b, |x, y| x >= y)?);
            }
            BCand => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on and".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on and".to_string())?;
                stack.push(make_lgl(
                    if scalar_bool_or_false(a, "left and operand")?
                        && scalar_bool_or_false(b, "right and operand")?
                    {
                        1
                    } else {
                        0
                    },
                )?);
            }
            BCor => {
                let b = stack.pop().ok_or_else(|| "empty stack on or".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on or".to_string())?;
                stack.push(make_lgl(
                    if scalar_bool_or_false(a, "left or operand")?
                        || scalar_bool_or_false(b, "right or operand")?
                    {
                        1
                    } else {
                        0
                    },
                )?);
            }
            BCcall => {
                let idx = bytecode[*pc] as usize;
                *pc += 1;
                let nargs = bytecode[*pc] as usize;
                *pc += 1;

                let mut args_vec = Vec::with_capacity(nargs);
                for _ in 0..nargs {
                    if let Some(arg) = stack.pop() {
                        args_vec.push(arg);
                    }
                }
                args_vec.reverse();
                let fun = get_constant(constants, idx)?;

                if fun.typeof_() == SEXPTYPE::CLOSXP {
                    let mut arg_list =
                        unsafe { Sexp::from_raw_unchecked(crate::sexp::globals::R_NilValue()) };
                    for arg in args_vec.into_iter().rev() {
                        let cell = with_arena(|arena| {
                            arena.cons(arg.as_raw(), arg_list.as_raw(), std::ptr::null_mut())
                        });
                        arg_list = Sexp::from_raw(cell).unwrap_or(arg_list);
                    }
                    let result = crate::eval::closure::apply_closure_safe(fun, arg_list, env)
                        .map_err(|e| format!("closure call failed: {e}"))?;
                    stack.push(result);
                } else {
                    stack.push(fun);
                }
            }
            BCpush => {
                let idx = bytecode[*pc] as usize;
                *pc += 1;
                stack.push(get_constant(constants, idx)?);
            }
            BCpop => {
                stack.pop();
            }
            BCdup => {
                if let Some(top) = stack.last() {
                    stack.push(*top);
                }
            }
            BCprint => {
                if let Some(top) = stack.last() {
                    let type_name = match top.typeof_() {
                        SEXPTYPE::NILSXP => "NULL",
                        SEXPTYPE::INTSXP => "integer",
                        SEXPTYPE::REALSXP => "double",
                        SEXPTYPE::LGLSXP => "logical",
                        SEXPTYPE::STRSXP => "character",
                        SEXPTYPE::VECSXP => "list",
                        SEXPTYPE::EXPRSXP => "expression",
                        SEXPTYPE::RAWSXP => "raw",
                        SEXPTYPE::CPLXSXP => "complex",
                        SEXPTYPE::SYMSXP => "symbol",
                        SEXPTYPE::CLOSXP => "closure",
                        SEXPTYPE::ENVSXP => "environment",
                        SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP => "pairlist",
                        SEXPTYPE::CHARSXP => "charsxp",
                        SEXPTYPE::PROMSXP => "promise",
                        SEXPTYPE::DOTSXP => "...",
                        SEXPTYPE::SPECIALSXP => "special",
                        SEXPTYPE::BUILTINSXP => "builtin",
                        SEXPTYPE::EXTPTRSXP => "externalptr",
                        SEXPTYPE::WEAKREFSXP => "weakref",
                        SEXPTYPE::BCODESXP => "bytecode",
                        SEXPTYPE::OBJSXP => "object",
                        _ => "unknown",
                    };
                    let output = format!("[{}; length={}]", type_name, top.len());
                    if crate::sexp::output::is_capturing() {
                        crate::sexp::output::capture_stdout(&output);
                        crate::sexp::output::capture_stdout("\n");
                    } else {
                        println!("{}", output);
                    }
                }
            }
            BCbegin => {}
            BCif => {
                let cond = stack.pop().ok_or_else(|| "empty stack on if".to_string())?;
                let true_offset = bytecode[*pc] as usize;
                *pc += 1;
                let false_offset = bytecode[*pc] as usize;
                *pc += 1;
                if scalar_bool_or_false(cond, "if condition")? {
                    *pc = true_offset;
                } else {
                    *pc = false_offset;
                }
            }
            BCjump => {
                let offset = bytecode[*pc] as usize;
                *pc = offset;
            }
            BCfjmp => {
                let cond = stack
                    .pop()
                    .ok_or_else(|| "empty stack on fjmp".to_string())?;
                let offset = bytecode[*pc] as usize;
                *pc += 1;
                if !scalar_bool_or_false(cond, "false jump condition")? {
                    *pc = offset;
                }
            }
            BCtjmp => {
                let cond = stack
                    .pop()
                    .ok_or_else(|| "empty stack on tjmp".to_string())?;
                let offset = bytecode[*pc] as usize;
                *pc += 1;
                if scalar_bool_or_false(cond, "true jump condition")? {
                    *pc = offset;
                }
            }
            BCfor => {
                let var_idx = bytecode[*pc] as usize;
                *pc += 1;
                let seq_idx = bytecode[*pc] as usize;
                *pc += 1;
                let body_offset = bytecode[*pc] as usize;
                *pc += 1;
                let end_offset = bytecode[*pc] as usize;
                *pc += 1;

                let var_sym = get_constant(constants, var_idx)?;
                let seq_val = get_constant(constants, seq_idx)?;
                let len = seq_val.len();

                for i in 0..len as usize {
                    let idx_val = if seq_val.typeof_() == SEXPTYPE::INTSXP {
                        make_int(
                            seq_val
                                .try_integer_elt(i as i64)
                                .map_err(|err| sexp_err("for-loop integer sequence", err))?,
                        )?
                    } else if seq_val.typeof_() == SEXPTYPE::REALSXP {
                        make_real(
                            seq_val
                                .try_real_elt(i as i64)
                                .map_err(|err| sexp_err("for-loop real sequence", err))?,
                        )?
                    } else {
                        make_int(i as c_int)?
                    };

                    unsafe {
                        crate::sexp::envir::defineVar(
                            var_sym.as_raw(),
                            idx_val.as_raw(),
                            env.as_raw(),
                        );
                    }
                    stack.push(idx_val);

                    let mut loop_pc = body_offset;
                    let (_, control) =
                        eval_bytecode_loop(bytecode, &mut loop_pc, stack, constants, env)?;

                    if control == ControlFlow::Break {
                        *pc = end_offset;
                        return Ok((make_lgl(0)?, ControlFlow::Normal));
                    }
                }
                *pc = end_offset;
            }
            BCwhile => {
                let cond_offset = bytecode[*pc] as usize;
                *pc += 1;
                let body_offset = bytecode[*pc] as usize;
                *pc += 1;
                let end_offset = bytecode[*pc] as usize;
                *pc += 1;

                loop {
                    let mut cond_pc = cond_offset;
                    let (cond_result, cond_control) =
                        eval_bytecode_loop(bytecode, &mut cond_pc, stack, constants, env)?;
                    if cond_control != ControlFlow::Normal {
                        return Ok((cond_result, cond_control));
                    }

                    if !scalar_bool_or_false(cond_result, "while condition")? {
                        *pc = end_offset;
                        return Ok((make_lgl(0)?, ControlFlow::Normal));
                    }

                    let mut body_pc = body_offset;
                    let (body_result, body_control) =
                        eval_bytecode_loop(bytecode, &mut body_pc, stack, constants, env)?;

                    if body_control == ControlFlow::Break {
                        *pc = end_offset;
                        return Ok((body_result, ControlFlow::Normal));
                    }
                }
            }
            BCrepeat => {
                let body_offset = bytecode[*pc] as usize;
                *pc += 1;
                let end_offset = bytecode[*pc] as usize;
                *pc += 1;

                loop {
                    let mut body_pc = body_offset;
                    let (body_result, body_control) =
                        eval_bytecode_loop(bytecode, &mut body_pc, stack, constants, env)?;

                    if body_control == ControlFlow::Break {
                        *pc = end_offset;
                        return Ok((body_result, ControlFlow::Normal));
                    }
                }
            }
            BCbreak => {
                return Ok((make_lgl(0)?, ControlFlow::Break));
            }
            BCnext => {
                return Ok((make_lgl(0)?, ControlFlow::Next));
            }
            BCspecial => {
                let idx = bytecode[*pc] as usize;
                *pc += 1;
                let nargs = bytecode[*pc] as usize;
                *pc += 1;

                let mut args_vec = Vec::with_capacity(nargs);
                for _ in 0..nargs {
                    if let Some(arg) = stack.pop() {
                        args_vec.push(arg);
                    }
                }
                args_vec.reverse();
                let fun = get_constant(constants, idx)?;

                if fun.typeof_() == SEXPTYPE::SPECIALSXP || fun.typeof_() == SEXPTYPE::BUILTINSXP {
                    let mut arg_list =
                        unsafe { Sexp::from_raw_unchecked(crate::sexp::globals::R_NilValue()) };
                    for arg in args_vec.into_iter().rev() {
                        let cell = with_arena(|arena| {
                            arena.cons(arg.as_raw(), arg_list.as_raw(), std::ptr::null_mut())
                        });
                        arg_list = Sexp::from_raw(cell).unwrap_or(arg_list);
                    }

                    let call = with_arena(|arena| {
                        arena.cons(fun.as_raw(), arg_list.as_raw(), std::ptr::null_mut())
                    });
                    let call_sexp = unsafe { Sexp::from_raw_unchecked(call) };

                    let result = crate::eval::eval::eval_lang_safe(call_sexp, env)
                        .map_err(|e| format!("special call failed: {e}"))?;
                    stack.push(result);
                } else {
                    stack.push(fun);
                }
            }
            BCbuiltin => {
                let idx = bytecode[*pc] as usize;
                *pc += 1;
                let nargs = bytecode[*pc] as usize;
                *pc += 1;

                let mut args_vec = Vec::with_capacity(nargs);
                for _ in 0..nargs {
                    if let Some(arg) = stack.pop() {
                        args_vec.push(arg);
                    }
                }
                args_vec.reverse();
                let fun = get_constant(constants, idx)?;

                if fun.typeof_() == SEXPTYPE::BUILTINSXP || fun.typeof_() == SEXPTYPE::SPECIALSXP {
                    let mut arg_list =
                        unsafe { Sexp::from_raw_unchecked(crate::sexp::globals::R_NilValue()) };
                    for arg in args_vec.into_iter().rev() {
                        let cell = with_arena(|arena| {
                            arena.cons(arg.as_raw(), arg_list.as_raw(), std::ptr::null_mut())
                        });
                        arg_list = Sexp::from_raw(cell).unwrap_or(arg_list);
                    }

                    let call = with_arena(|arena| {
                        arena.cons(fun.as_raw(), arg_list.as_raw(), std::ptr::null_mut())
                    });
                    let call_sexp = unsafe { Sexp::from_raw_unchecked(call) };

                    let result = crate::eval::eval::eval_lang_safe(call_sexp, env)
                        .map_err(|e| format!("builtin call failed: {e}"))?;
                    stack.push(result);
                } else {
                    stack.push(fun);
                }
            }
            BCneg => {
                let val = stack
                    .pop()
                    .ok_or_else(|| "empty stack on neg".to_string())?;
                if val.typeof_() == SEXPTYPE::REALSXP {
                    stack.push(make_real(-scalar_real(val, "real negation operand")?)?);
                } else if val.typeof_() == SEXPTYPE::INTSXP {
                    stack.push(make_int(-scalar_int(val, "integer negation operand")?)?);
                } else {
                    stack.push(val);
                }
            }
            BCclosure => {
                let idx = bytecode[*pc] as usize;
                *pc += 1;
                stack.push(get_constant(constants, idx)?);
            }
            _ => {
                return Err(format!("unknown bytecode opcode: {opcode}"));
            }
        }
    }

    let val = stack
        .pop()
        .ok_or_else(|| "empty stack at end of bytecode".to_string())?;
    Ok((val, ControlFlow::Normal))
}

fn get_constant(constants: Option<Sexp<'_>>, idx: usize) -> Result<Sexp<'_>, String> {
    match constants {
        Some(c) => c
            .try_vector_elt(idx as i64)
            .map_err(|err| sexp_err(&format!("constant index {idx}"), err)),
        None => Err("no constants available".to_string()),
    }
}
