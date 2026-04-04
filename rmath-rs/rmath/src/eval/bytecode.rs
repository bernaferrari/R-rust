//! R bytecode evaluation.
//!
//! R compiles expressions to bytecode for faster execution.
//! The bytecode format is a vector of integers where each
//! instruction is an opcode followed by operand indices.

use std::os::raw::c_int;

use crate::sexp::ffi::SEXPTYPE;
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
                let result = if val.typeof_() == SEXPTYPE::LGLSXP {
                    let v = val.logical_elt(0).unwrap_or(0);
                    if v == 0 {
                        1
                    } else {
                        0
                    }
                } else {
                    0
                };
                stack.push(val);
            }
            BCadd | BCsub | BCmul | BCdiv => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on binary op".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on binary op".to_string())?;
                stack.push(a);
            }
            BCeq | BCne | BClt | BCle | BCgt | BCge => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on comparison".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on comparison".to_string())?;
                stack.push(a);
            }
            BCand | BCor => {
                let b = stack
                    .pop()
                    .ok_or_else(|| "empty stack on logical op".to_string())?;
                let a = stack
                    .pop()
                    .ok_or_else(|| "empty stack on logical op".to_string())?;
                stack.push(a);
            }
            BCcall => {
                let idx = bytecode[pc] as usize;
                pc += 1;
                let nargs = bytecode[pc] as usize;
                pc += 1;
                let fun = get_constant(constants, idx)?;
                stack.push(fun);
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
            BCprint => {}
            BCbegin => {}
            BCif => {
                let cond = stack.pop().ok_or_else(|| "empty stack on if".to_string())?;
                let offset = bytecode[pc];
                pc += 1;
                let _ = offset;
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

fn get_constant<'a>(constants: Option<Sexp<'a>>, idx: usize) -> Result<Sexp<'a>, String> {
    match constants {
        Some(c) => c
            .vector_elt(idx as i64)
            .ok_or_else(|| format!("constant index {idx} out of bounds")),
        None => Err("no constants available".to_string()),
    }
}
