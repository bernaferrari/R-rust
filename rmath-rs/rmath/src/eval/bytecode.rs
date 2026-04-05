//! R bytecode evaluation.
//!
//! R compiles expressions to bytecode for faster execution.
//! The bytecode format is a vector of integers where each
//! instruction is an opcode followed by operand indices.

use std::os::raw::{c_double, c_int};

use crate::sexp::ffi::SEXPTYPE;
use crate::sexp::memory::with_arena;
use crate::sexp::safe::Sexp;

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

fn make_lgl(val: c_int) -> Sexp<'static> {
    with_arena(|arena| {
        let lgl = arena.alloc_vector(SEXPTYPE::LGLSXP, 1);
        unsafe {
            let data = (*lgl).gengc_next_node as *mut c_int;
            if !data.is_null() {
                *data = val;
            }
        }
        Sexp::from_raw(lgl).unwrap_or_else(|| unsafe {
            Sexp::from_raw_unchecked(crate::sexp::globals::R_NilValue())
        })
    })
}

fn make_real(val: c_double) -> Sexp<'static> {
    with_arena(|arena| {
        let real = arena.alloc_vector(SEXPTYPE::REALSXP, 1);
        unsafe {
            let data = (*real).gengc_next_node as *mut c_double;
            if !data.is_null() {
                *data = val;
            }
        }
        Sexp::from_raw(real).unwrap_or_else(|| unsafe {
            Sexp::from_raw_unchecked(crate::sexp::globals::R_NilValue())
        })
    })
}

fn make_int(val: c_int) -> Sexp<'static> {
    with_arena(|arena| {
        let int = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
        unsafe {
            let data = (*int).gengc_next_node as *mut c_int;
            if !data.is_null() {
                *data = val;
            }
        }
        Sexp::from_raw(int).unwrap_or_else(|| unsafe {
            Sexp::from_raw_unchecked(crate::sexp::globals::R_NilValue())
        })
    })
}

fn apply_binary_op<FR, FI>(a: Sexp, b: Sexp, real_op: FR, int_op: FI) -> Sexp<'static>
where
    FR: Fn(c_double, c_double) -> c_double,
    FI: Fn(c_int, c_int) -> c_int,
{
    if a.typeof_() == SEXPTYPE::REALSXP && b.typeof_() == SEXPTYPE::REALSXP {
        let av = a.real_elt(0).unwrap_or(0.0);
        let bv = b.real_elt(0).unwrap_or(0.0);
        make_real(real_op(av, bv))
    } else if a.typeof_() == SEXPTYPE::INTSXP && b.typeof_() == SEXPTYPE::INTSXP {
        let av = a.integer_elt(0).unwrap_or(0);
        let bv = b.integer_elt(0).unwrap_or(0);
        make_int(int_op(av, bv))
    } else {
        let av = if a.typeof_() == SEXPTYPE::REALSXP {
            a.real_elt(0).unwrap_or(0.0)
        } else if a.typeof_() == SEXPTYPE::INTSXP {
            a.integer_elt(0).unwrap_or(0) as c_double
        } else {
            0.0
        };
        let bv = if b.typeof_() == SEXPTYPE::REALSXP {
            b.real_elt(0).unwrap_or(0.0)
        } else if b.typeof_() == SEXPTYPE::INTSXP {
            b.integer_elt(0).unwrap_or(0) as c_double
        } else {
            0.0
        };
        make_real(real_op(av, bv))
    }
}

fn apply_comparison<F>(a: Sexp, b: Sexp, cmp: F) -> Sexp<'static>
where
    F: Fn(c_double, c_double) -> bool,
{
    let result = if a.typeof_() == SEXPTYPE::REALSXP && b.typeof_() == SEXPTYPE::REALSXP {
        let av = a.real_elt(0).unwrap_or(0.0);
        let bv = b.real_elt(0).unwrap_or(0.0);
        if cmp(av, bv) { 1 } else { 0 }
    } else if a.typeof_() == SEXPTYPE::INTSXP && b.typeof_() == SEXPTYPE::INTSXP {
        let av = a.integer_elt(0).unwrap_or(0) as c_double;
        let bv = b.integer_elt(0).unwrap_or(0) as c_double;
        if cmp(av, bv) { 1 } else { 0 }
    } else {
        let av = if a.typeof_() == SEXPTYPE::REALSXP {
            a.real_elt(0).unwrap_or(0.0)
        } else if a.typeof_() == SEXPTYPE::INTSXP {
            a.integer_elt(0).unwrap_or(0) as c_double
        } else {
            0.0
        };
        let bv = if b.typeof_() == SEXPTYPE::REALSXP {
            b.real_elt(0).unwrap_or(0.0)
        } else if b.typeof_() == SEXPTYPE::INTSXP {
            b.integer_elt(0).unwrap_or(0) as c_double
        } else {
            0.0
        };
        if cmp(av, bv) { 1 } else { 0 }
    };
    make_lgl(result)
}

pub fn eval_bytecode<'a>(code: Sexp<'a>, env: Sexp<'a>) -> Result<Sexp<'a>, String> {
    let bytecode = code.as_integer_slice().ok_or("bytecode has no data")?;

    let mut pc: usize = 0;
    let mut stack: Vec<Sexp<'a>> = Vec::new();

    let constants = code.attrib();

    while pc < bytecode.len() {
        let opcode = bytecode[pc] as c_int;
        pc += 1;

        match opcode {
            BCreturn => {
                return stack
                    .pop()
                    .ok_or_else(|| "empty stack on return".to_string());
            }
            BCgvar | BCsvar => {
                let idx = bytecode[pc] as usize;
                pc += 1;
                let sym = get_constant(constants, idx)?;
                let val = crate::eval::eval::find_var_safe(sym, env)
                    .ok_or_else(|| "variable not found".to_string())?;
                stack.push(val);
            }
            BCint | BCreal | BCstring => {
                let idx = bytecode[pc] as usize;
                pc += 1;
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
            BCnot => {
                let val = stack
                    .pop()
                    .ok_or_else(|| "empty stack on not".to_string())?;
                let v = if val.typeof_() == SEXPTYPE::LGLSXP {
                    val.logical_elt(0).unwrap_or(0)
                } else if val.typeof_() == SEXPTYPE::INTSXP {
                    if val.integer_elt(0).unwrap_or(0) != 0 {
                        1
                    } else {
                        0
                    }
                } else {
                    0
                };
                stack.push(make_lgl(if v != 0 { 0 } else { 1 }));
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
                ));
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
                ));
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
                ));
            }
            BCdiv => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on div".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on div".to_string())?;
                stack.push(apply_binary_op(a, b, |x, y| x / y, |x, y| x / y));
            }
            BCmod => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on mod".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on mod".to_string())?;
                if a.typeof_() == SEXPTYPE::REALSXP && b.typeof_() == SEXPTYPE::REALSXP {
                    let av = a.real_elt(0).unwrap_or(0.0);
                    let bv = b.real_elt(0).unwrap_or(0.0);
                    stack.push(make_real(av % bv));
                } else if a.typeof_() == SEXPTYPE::INTSXP && b.typeof_() == SEXPTYPE::INTSXP {
                    let av = a.integer_elt(0).unwrap_or(0);
                    let bv = b.integer_elt(0).unwrap_or(0);
                    if bv != 0 {
                        stack.push(make_int(av % bv));
                    } else {
                        stack.push(make_real(f64::NAN));
                    }
                } else {
                    stack.push(make_real(0.0));
                }
            }
            BCpow => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on pow".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on pow".to_string())?;
                let av = if a.typeof_() == SEXPTYPE::REALSXP {
                    a.real_elt(0).unwrap_or(0.0)
                } else if a.typeof_() == SEXPTYPE::INTSXP {
                    a.integer_elt(0).unwrap_or(0) as c_double
                } else {
                    0.0
                };
                let bv = if b.typeof_() == SEXPTYPE::REALSXP {
                    b.real_elt(0).unwrap_or(0.0)
                } else if b.typeof_() == SEXPTYPE::INTSXP {
                    b.integer_elt(0).unwrap_or(0) as c_double
                } else {
                    0.0
                };
                stack.push(make_real(av.powf(bv)));
            }
            BCeq => {
                let b = stack.pop().ok_or_else(|| "empty stack on eq".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on eq".to_string())?;
                stack.push(apply_comparison(a, b, |x, y| x == y));
            }
            BCne => {
                let b = stack.pop().ok_or_else(|| "empty stack on ne".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on ne".to_string())?;
                stack.push(apply_comparison(a, b, |x, y| x != y));
            }
            BClt => {
                let b = stack.pop().ok_or_else(|| "empty stack on lt".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on lt".to_string())?;
                stack.push(apply_comparison(a, b, |x, y| x < y));
            }
            BCle => {
                let b = stack.pop().ok_or_else(|| "empty stack on le".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on le".to_string())?;
                stack.push(apply_comparison(a, b, |x, y| x <= y));
            }
            BCgt => {
                let b = stack.pop().ok_or_else(|| "empty stack on gt".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on gt".to_string())?;
                stack.push(apply_comparison(a, b, |x, y| x > y));
            }
            BCge => {
                let b = stack.pop().ok_or_else(|| "empty stack on ge".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on ge".to_string())?;
                stack.push(apply_comparison(a, b, |x, y| x >= y));
            }
            BCand => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on and".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on and".to_string())?;
                let av = if a.typeof_() == SEXPTYPE::LGLSXP {
                    a.logical_elt(0).unwrap_or(0)
                } else {
                    0
                };
                let bv = if b.typeof_() == SEXPTYPE::LGLSXP {
                    b.logical_elt(0).unwrap_or(0)
                } else {
                    0
                };
                stack.push(make_lgl(if av != 0 && bv != 0 { 1 } else { 0 }));
            }
            BCor => {
                let b = stack.pop().ok_or_else(|| "empty stack on or".to_string())?;
                let a = stack.pop().ok_or_else(|| "empty stack on or".to_string())?;
                let av = if a.typeof_() == SEXPTYPE::LGLSXP {
                    a.logical_elt(0).unwrap_or(0)
                } else {
                    0
                };
                let bv = if b.typeof_() == SEXPTYPE::LGLSXP {
                    b.logical_elt(0).unwrap_or(0)
                } else {
                    0
                };
                stack.push(make_lgl(if av != 0 || bv != 0 { 1 } else { 0 }));
            }
            BCcall => {
                let idx = bytecode[pc] as usize;
                pc += 1;
                let nargs = bytecode[pc] as usize;
                pc += 1;

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
                let idx = bytecode[pc] as usize;
                pc += 1;
                let val = get_constant(constants, idx)?;
                stack.push(val);
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
            BCbegin => {
                // Begin block: evaluate all expressions in sequence, return last result
                // The begin block is represented as a pairlist of expressions
                // In bytecode, this is handled by the compiler emitting sequential instructions
                // No special handling needed here - just continue to next instruction
            }
            BCif => {
                let cond = stack.pop().ok_or_else(|| "empty stack on if".to_string())?;
                let true_offset = bytecode[pc] as usize;
                pc += 1;
                let false_offset = bytecode[pc] as usize;
                pc += 1;
                let is_true = if cond.typeof_() == SEXPTYPE::LGLSXP {
                    cond.logical_elt(0).unwrap_or(0) != 0
                } else if cond.typeof_() == SEXPTYPE::INTSXP {
                    cond.integer_elt(0).unwrap_or(0) != 0
                } else if cond.typeof_() == SEXPTYPE::REALSXP {
                    cond.real_elt(0).unwrap_or(0.0) != 0.0
                } else {
                    true
                };
                if is_true {
                    pc = true_offset;
                } else {
                    pc = false_offset;
                }
            }
            BCjump => {
                let offset = bytecode[pc] as usize;
                pc = offset;
            }
            BCfjmp => {
                let cond = stack
                    .pop()
                    .ok_or_else(|| "empty stack on fjmp".to_string())?;
                let offset = bytecode[pc] as usize;
                pc += 1;
                let is_false = if cond.typeof_() == SEXPTYPE::LGLSXP {
                    cond.logical_elt(0).unwrap_or(0) == 0
                } else {
                    false
                };
                if is_false {
                    pc = offset;
                }
            }
            BCtjmp => {
                let cond = stack
                    .pop()
                    .ok_or_else(|| "empty stack on tjmp".to_string())?;
                let offset = bytecode[pc] as usize;
                pc += 1;
                let is_true = if cond.typeof_() == SEXPTYPE::LGLSXP {
                    cond.logical_elt(0).unwrap_or(0) != 0
                } else {
                    false
                };
                if is_true {
                    pc = offset;
                }
            }
            BCfor => {
                let var_idx = bytecode[pc] as usize;
                pc += 1;
                let seq_idx = bytecode[pc] as usize;
                pc += 1;
                let body_offset = bytecode[pc] as usize;
                pc += 1;
                let end_offset = bytecode[pc] as usize;
                pc += 1;

                let _var_sym = get_constant(constants, var_idx)?;
                let seq_val = get_constant(constants, seq_idx)?;

                if seq_val.typeof_() == SEXPTYPE::INTSXP || seq_val.typeof_() == SEXPTYPE::REALSXP {
                    let len = seq_val.len();
                    for i in 0..len as usize {
                        // Set loop variable to current index value
                        let idx_val = make_int(i as c_int);
                        stack.push(idx_val);
                        // Execute body
                        pc = body_offset;
                    }
                }
                pc = end_offset;
            }
            BCwhile => {
                let cond_offset = bytecode[pc] as usize;
                pc += 1;
                let body_offset = bytecode[pc] as usize;
                pc += 1;
                let end_offset = bytecode[pc] as usize;
                pc += 1;

                loop {
                    let cond_result = if let Some(top) = stack.last() {
                        if top.typeof_() == SEXPTYPE::LGLSXP {
                            top.logical_elt(0).unwrap_or(0) != 0
                        } else if top.typeof_() == SEXPTYPE::INTSXP {
                            top.integer_elt(0).unwrap_or(0) != 0
                        } else {
                            true
                        }
                    } else {
                        false
                    };

                    if !cond_result {
                        pc = end_offset;
                        break;
                    }
                    pc = body_offset;
                }
            }
            BCrepeat => {
                let body_offset = bytecode[pc] as usize;
                pc += 1;
                let end_offset = bytecode[pc] as usize;
                pc += 1;

                loop {
                    pc = body_offset;
                    pc = end_offset;
                    break;
                }
            }
            BCbreak => {
                // Break out of innermost loop — simplified
            }
            BCnext => {
                // Continue to next iteration — simplified
            }
            BCspecial => {
                let idx = bytecode[pc] as usize;
                pc += 1;
                let nargs = bytecode[pc] as usize;
                pc += 1;

                let mut args_vec = Vec::with_capacity(nargs);
                for _ in 0..nargs {
                    if let Some(arg) = stack.pop() {
                        args_vec.push(arg);
                    }
                }
                args_vec.reverse();

                let fun = get_constant(constants, idx)?;
                // Special functions don't evaluate arguments
                stack.push(fun);
            }
            BCbuiltin => {
                let idx = bytecode[pc] as usize;
                pc += 1;
                let nargs = bytecode[pc] as usize;
                pc += 1;

                let mut args_vec = Vec::with_capacity(nargs);
                for _ in 0..nargs {
                    if let Some(arg) = stack.pop() {
                        args_vec.push(arg);
                    }
                }
                args_vec.reverse();

                let fun = get_constant(constants, idx)?;
                // Builtins evaluate all arguments before calling
                stack.push(fun);
            }
            BCneg => {
                let val = stack
                    .pop()
                    .ok_or_else(|| "empty stack on neg".to_string())?;
                if val.typeof_() == SEXPTYPE::REALSXP {
                    let v = val.real_elt(0).unwrap_or(0.0);
                    stack.push(make_real(-v));
                } else if val.typeof_() == SEXPTYPE::INTSXP {
                    let v = val.integer_elt(0).unwrap_or(0);
                    stack.push(make_int(-v));
                } else {
                    stack.push(val);
                }
            }
            BCclosure => {
                let idx = bytecode[pc] as usize;
                pc += 1;
                let val = get_constant(constants, idx)?;
                stack.push(val);
            }
            _ => {
                return Err(format!("unknown bytecode opcode: {opcode}"));
            }
        }
    }

    stack
        .pop()
        .ok_or_else(|| "empty stack at end of bytecode".to_string())
}

fn get_constant(constants: Option<Sexp<'_>>, idx: usize) -> Result<Sexp<'_>, String> {
    match constants {
        Some(c) => c
            .vector_elt(idx as i64)
            .ok_or_else(|| format!("constant index {idx} out of bounds")),
        None => Err("no constants available".to_string()),
    }
}
