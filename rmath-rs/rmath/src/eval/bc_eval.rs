#![allow(
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    clippy::collapsible_if
)]

//! Bytecode interpreter — ports R's bcEval from eval.c.
//!
//! This module provides the bytecode evaluation loop for compiled R code.
//! R compiles function bodies to bytecode for faster execution.
//!
//! The bytecode format uses an instruction stream (array of integers)
//! with a separate constant pool.

use std::os::raw::c_int;
use std::ptr;

use crate::sexp::accessors::{INTEGER, LENGTH, LOGICAL, REAL, Rf_isNull, TYPEOF, VECTOR_ELT};
use crate::sexp::constructors::*;
use crate::sexp::envir::{R_findVar, defineVar, forcePromise};
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_NilValue, R_UnboundValue, set_R_Visible};

use super::bc_stack::R_bcstack_t;

// ---------------------------------------------------------------------------
// Bytecode opcodes
// ---------------------------------------------------------------------------

/// Bytecode instruction opcodes.
///
/// These match R's OPC_* defines from Defn.h.
pub mod opcodes {
    pub const OP_PUSHCONSTARG: i32 = 1;
    pub const OP_PUSHCONST: i32 = 2;
    pub const OP_GETVAR: i32 = 3;
    pub const OP_SETVAR: i32 = 4;
    pub const OP_STARTASSIGN: i32 = 5;
    pub const OP_ENDASSIGN: i32 = 6;
    pub const OP_STARTSUBSET: i32 = 7;
    pub const OP_ENDSUBSET: i32 = 8;
    pub const OP_STARTSUBSET2: i32 = 9;
    pub const OP_ENDSUBSET2: i32 = 10;
    pub const OP_DUP: i32 = 11;
    pub const OP_DUP2: i32 = 12;
    pub const OP_POP: i32 = 13;
    pub const OP_CALL: i32 = 14;
    pub const OP_CALLBUILTIN: i32 = 15;
    pub const OP_CALLSPECIAL: i32 = 16;
    pub const OP_RETURN: i32 = 17;
    pub const OP_GOTO: i32 = 18;
    pub const OP_BRANCH: i32 = 19;
    pub const OP_BRIFNOT: i32 = 20;
    pub const OP_BRIFTRUE: i32 = 21;
    pub const OP_POPAND: i32 = 22;
    pub const OP_POPOR: i32 = 23;
    pub const OP_PUSHTRUE: i32 = 24;
    pub const OP_PUSHFALSE: i32 = 25;
    pub const OP_PUSHNULL: i32 = 26;
    pub const OP_PUSHNIL: i32 = 27;
    pub const OP_MAKEPROMISE: i32 = 28;
    pub const OP_DOMISSING: i32 = 29;
    pub const OP_SETLOOPCTR: i32 = 30;
    pub const OP_BEGINLOOP: i32 = 31;
    pub const OP_ENDLOOP: i32 = 32;
    pub const OP_STEPFOR: i32 = 33;
    pub const OP_BREAK: i32 = 34;
    pub const OP_NEXTITER: i32 = 35;
    pub const OPinvisible: i32 = 36;
    pub const OP_visible: i32 = 37;
    pub const OP_HIDDENCALL: i32 = 38;
    pub const OP_LDCLOSURE: i32 = 39;
    pub const OP_CLOSEDEXPR: i32 = 40;
    pub const OP_MAKEACTIVE: i32 = 41;
    pub const OP_NOOP: i32 = 42;
    pub const OP_SWASTORE: i32 = 43;
    pub const OP_SWLOAD: i32 = 44;
    pub const OP_PUTBASE: i32 = 45;
    pub const OP_PUTBASE_SEP: i32 = 46;
    pub const OP_SEQBEGIN: i32 = 47;
    pub const OP_SEQEND: i32 = 48;
    pub const OP_PUSHARG: i32 = 49;
    pub const OP_PUSHFUN: i32 = 50;
    pub const OP_PUSHNILVALUE: i32 = 51;
    pub const OP_DFLTFUN: i32 = 52;
    pub const OP_DFLTFORM: i32 = 53;
    pub const OP_LAST: i32 = 54;
}

// ---------------------------------------------------------------------------
// Helper: evaluate a condition value to bool
// ---------------------------------------------------------------------------

/// Evaluate a value as a boolean condition for branching.
unsafe fn eval_bc_condition(val: SEXP) -> bool {
    unsafe {
        if val.is_null() || val == R_NilValue() {
            return false;
        }
        if TYPEOF(val) == SEXPTYPE::LGLSXP.0 {
            let data = crate::sexp::accessors::LOGICAL(val);
            !data.is_null() && *data != 0
        } else {
            Rf_isNull(val) == 0
        }
    }
}

// ---------------------------------------------------------------------------
// BCODESXP accessors
// ---------------------------------------------------------------------------

/// Get the bytecode instruction array from a BCODESXP.
pub unsafe fn BCODE_CODE(x: SEXP) -> *const c_int {
    unsafe {
        if x.is_null() {
            return ptr::null();
        }
        // The code is stored as the first element of the vector
        let v = crate::sexp::accessors::VECTOR_ELT(x, 0);
        if v.is_null() {
            return ptr::null();
        }
        INTEGER(v) as *const c_int
    }
}

/// Get the constant pool from a BCODESXP.
pub unsafe fn BCODE_CONSTS(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() {
            return R_NilValue();
        }
        // Constants are the second element
        crate::sexp::accessors::VECTOR_ELT(x, 1)
    }
}

/// Get the stack depth from a BCODESXP.
pub unsafe fn BCODE_STACK(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return 0;
        }
        // Stack depth hint is the third element
        let v = crate::sexp::accessors::VECTOR_ELT(x, 2);
        if v.is_null() {
            return 0;
        }
        *INTEGER(v)
    }
}

// ---------------------------------------------------------------------------
// bcEval — the main bytecode evaluation loop
// ---------------------------------------------------------------------------

/// Evaluate a BCODESXP.
///
/// This is the equivalent of R's `bcEval()` from eval.c.
pub unsafe fn bcEval(body: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if body.is_null() || TYPEOF(body) != SEXPTYPE::BCODESXP.0 {
            return R_NilValue();
        }

        let code_ptr = BCODE_CODE(body);
        let consts = BCODE_CONSTS(body);
        let stack_depth = BCODE_STACK(body);

        if code_ptr.is_null() {
            return R_NilValue();
        }

        // Get code length
        let code_vec = crate::sexp::accessors::VECTOR_ELT(body, 0);
        let code_len = if !code_vec.is_null() {
            LENGTH(code_vec)
        } else {
            0
        };

        let mut pc: c_int = 0; // program counter
        let mut stack = R_bcstack_t::new(stack_depth as usize);

        while pc < code_len {
            let op = *code_ptr.add(pc as usize);
            pc += 1;

            match op {
                opcodes::OP_PUSHCONST => {
                    // Push constant from pool
                    let idx = *code_ptr.add(pc as usize);
                    pc += 1;
                    let val = if !consts.is_null() && idx >= 0 {
                        crate::sexp::accessors::VECTOR_ELT(consts, idx as i64)
                    } else {
                        R_NilValue()
                    };
                    stack.push(val);
                }

                opcodes::OP_PUSHCONSTARG => {
                    let idx = *code_ptr.add(pc as usize);
                    pc += 1;
                    let val = if !consts.is_null() && idx >= 0 {
                        crate::sexp::accessors::VECTOR_ELT(consts, idx as i64)
                    } else {
                        R_NilValue()
                    };
                    stack.push(val);
                }

                opcodes::OP_GETVAR => {
                    let idx = *code_ptr.add(pc as usize);
                    pc += 1;
                    let sym = if !consts.is_null() && idx >= 0 {
                        crate::sexp::accessors::VECTOR_ELT(consts, idx as i64)
                    } else {
                        R_NilValue()
                    };
                    let val = R_findVar(sym, rho);
                    if val == R_UnboundValue() {
                        eprintln!("Error: object not found");
                        std::panic::panic_any(crate::sexp::context::RError {
                            message: "object not found".to_string(),
                        });
                    } else if TYPEOF(val) == SEXPTYPE::PROMSXP.0 {
                        stack.push(forcePromise(val));
                    } else {
                        stack.push(val);
                    }
                }

                opcodes::OP_SETVAR => {
                    let idx = *code_ptr.add(pc as usize);
                    pc += 1;
                    let sym = if !consts.is_null() && idx >= 0 {
                        crate::sexp::accessors::VECTOR_ELT(consts, idx as i64)
                    } else {
                        R_NilValue()
                    };
                    let val = stack.pop();
                    defineVar(sym, val, rho);
                    set_R_Visible(FALSE);
                }

                opcodes::OP_DUP => {
                    let val = stack.top();
                    stack.push(val);
                }

                opcodes::OP_POP => {
                    stack.pop();
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
                    let val = stack.pop();
                    return if val.is_null() { R_NilValue() } else { val };
                }

                opcodes::OP_visible => {
                    set_R_Visible(TRUE);
                }

                opcodes::OPinvisible => {
                    set_R_Visible(FALSE);
                }

                opcodes::OP_NOOP => {
                    // No operation
                }

                opcodes::OP_POPAND => {
                    let target = *code_ptr.add(pc as usize);
                    pc += 1;
                    let val = stack.top();
                    let is_false = if !val.is_null() && TYPEOF(val) == SEXPTYPE::LGLSXP.0 {
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
                    let target = *code_ptr.add(pc as usize);
                    pc += 1;
                    let val = stack.top();
                    let is_true = if !val.is_null() && TYPEOF(val) == SEXPTYPE::LGLSXP.0 {
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
                    let target = *code_ptr.add(pc as usize);
                    pc = target;
                }

                opcodes::OP_BRIFNOT => {
                    let target = *code_ptr.add(pc as usize);
                    pc += 1;
                    let val = stack.top();
                    let cond = eval_bc_condition(val);
                    if !cond {
                        pc = target;
                    }
                }

                opcodes::OP_BRIFTRUE => {
                    let target = *code_ptr.add(pc as usize);
                    pc += 1;
                    let val = stack.top();
                    let cond = eval_bc_condition(val);
                    if cond {
                        pc = target;
                    }
                }

                opcodes::OP_DUP2 => {
                    let top = stack.top();
                    let s1 = stack.depth();
                    let next = if s1 >= 2 {
                        stack.at(s1 - 2)
                    } else {
                        R_NilValue()
                    };
                    stack.push(next);
                    stack.push(top);
                }

                opcodes::OP_GOTO => {
                    let target = *code_ptr.add(pc as usize);
                    pc = target;
                }

                opcodes::OP_MAKEPROMISE => {
                    let idx = *code_ptr.add(pc as usize);
                    pc += 1;
                    let expr = if !consts.is_null() && idx >= 0 {
                        VECTOR_ELT(consts, idx as i64)
                    } else {
                        R_NilValue()
                    };
                    let prom = crate::sexp::memory_ext::mkPROMSXP(expr, rho);
                    stack.push(prom);
                }

                opcodes::OP_DOMISSING => {
                    stack.push(R_NilValue());
                }

                opcodes::OP_CALL => {
                    let nargs = *code_ptr.add(pc as usize);
                    pc += 1;
                    let fun = stack.pop();
                    let mut args = R_NilValue();
                    for _ in 0..nargs {
                        let arg = stack.pop();
                        args = Rf_cons(arg, args);
                    }
                    if fun.is_null() {
                        stack.push(R_NilValue());
                    } else {
                        let call = Rf_lang2(fun, args);
                        let result = crate::eval::eval::Rf_eval(call, rho);
                        stack.push(result);
                    }
                }

                opcodes::OP_CALLBUILTIN => {
                    let nargs = *code_ptr.add(pc as usize);
                    pc += 1;
                    let fun = stack.pop();
                    let mut args = R_NilValue();
                    for _ in 0..nargs {
                        let arg = stack.pop();
                        args = Rf_cons(arg, args);
                    }
                    if fun.is_null() {
                        stack.push(R_NilValue());
                    } else {
                        let call = Rf_lang2(fun, args);
                        let result = crate::eval::eval::Rf_eval(call, rho);
                        stack.push(result);
                    }
                    set_R_Visible(TRUE);
                }

                opcodes::OP_CALLSPECIAL => {
                    let nargs = *code_ptr.add(pc as usize);
                    pc += 1;
                    let fun = stack.pop();
                    let mut args = R_NilValue();
                    for _ in 0..nargs {
                        let arg = stack.pop();
                        args = Rf_cons(arg, args);
                    }
                    if fun.is_null() {
                        stack.push(R_NilValue());
                    } else {
                        let call = Rf_lang2(fun, args);
                        let result = crate::eval::eval::Rf_eval(call, rho);
                        stack.push(result);
                    }
                    set_R_Visible(FALSE);
                }

                opcodes::OP_STARTASSIGN => {
                    let idx = *code_ptr.add(pc as usize);
                    pc += 1;
                    let sym = if !consts.is_null() && idx >= 0 {
                        VECTOR_ELT(consts, idx as i64)
                    } else {
                        R_NilValue()
                    };
                    let val = R_findVar(sym, rho);
                    stack.push(sym);
                    stack.push(val);
                }

                opcodes::OP_ENDASSIGN => {
                    let _nargs = *code_ptr.add(pc as usize);
                    pc += 1;
                    let val = stack.pop();
                    let sym = stack.pop();
                    defineVar(sym, val, rho);
                    stack.push(val);
                    set_R_Visible(FALSE);
                }

                opcodes::OP_STEPFOR => {
                    let target = *code_ptr.add(pc as usize);
                    pc += 1;
                    let depth = stack.depth();
                    if depth < 3 {
                        pc = target;
                    } else {
                        let seq_val = stack.at(depth - 1);
                        let loop_var = stack.at(depth - 2);
                        let ctr_ptr = stack.at(depth - 3);

                        if seq_val.is_null() || ctr_ptr.is_null() {
                            pc = target;
                        } else {
                            let mut seq_len: c_int = 0;
                            if TYPEOF(seq_val) == SEXPTYPE::INTSXP.0
                                || TYPEOF(seq_val) == SEXPTYPE::REALSXP.0
                            {
                                seq_len = LENGTH(seq_val);
                            }
                            let ctr = *INTEGER(ctr_ptr);
                            if ctr + 1 >= seq_len {
                                pc = target;
                            } else {
                                *INTEGER(ctr_ptr) = ctr + 1;
                                let idx = (ctr + 1) as usize;
                                if TYPEOF(seq_val) == SEXPTYPE::INTSXP.0 {
                                    let data = INTEGER(seq_val);
                                    if !data.is_null() {
                                        defineVar(loop_var, Rf_ScalarInteger(*data.add(idx)), rho);
                                    }
                                } else if TYPEOF(seq_val) == SEXPTYPE::REALSXP.0 {
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
                    let target = *code_ptr.add(pc as usize);
                    pc = target;
                }

                opcodes::OP_NEXTITER => {
                    let target = *code_ptr.add(pc as usize);
                    pc = target;
                }

                opcodes::OP_SETLOOPCTR => {
                    let ctr = stack.pop();
                    if !ctr.is_null() {
                        if TYPEOF(ctr) == SEXPTYPE::INTSXP.0 {
                            let d = INTEGER(ctr);
                            if !d.is_null() {
                                *d = -1;
                            }
                        }
                        stack.push(ctr);
                    } else {
                        stack.push(R_NilValue());
                    }
                }

                opcodes::OP_BEGINLOOP => {
                    stack.push(R_NilValue());
                }

                opcodes::OP_ENDLOOP => {
                    stack.pop();
                }

                opcodes::OP_HIDDENCALL => {
                    let _nargs = *code_ptr.add(pc as usize);
                    pc += 1;
                    let fun = stack.pop();
                    let args = stack.pop();
                    if fun.is_null() {
                        stack.push(R_NilValue());
                    } else {
                        let call = Rf_lang2(fun, args);
                        let result = crate::eval::eval::Rf_eval(call, rho);
                        stack.push(result);
                    }
                    set_R_Visible(FALSE);
                }

                opcodes::OP_PUSHARG => {
                    let idx = *code_ptr.add(pc as usize);
                    pc += 1;
                    let val = if !consts.is_null() && idx >= 0 {
                        VECTOR_ELT(consts, idx as i64)
                    } else {
                        R_NilValue()
                    };
                    stack.push(val);
                }

                opcodes::OP_PUSHFUN => {
                    let idx = *code_ptr.add(pc as usize);
                    pc += 1;
                    let val = if !consts.is_null() && idx >= 0 {
                        VECTOR_ELT(consts, idx as i64)
                    } else {
                        R_NilValue()
                    };
                    stack.push(val);
                }

                opcodes::OP_PUSHNILVALUE => {
                    stack.push(R_NilValue());
                }

                opcodes::OP_DFLTFUN => {
                    let idx = *code_ptr.add(pc as usize);
                    pc += 1;
                    let val = if !consts.is_null() && idx >= 0 {
                        VECTOR_ELT(consts, idx as i64)
                    } else {
                        R_NilValue()
                    };
                    stack.push(val);
                }

                opcodes::OP_DFLTFORM => {
                    let idx = *code_ptr.add(pc as usize);
                    pc += 1;
                    let val = if !consts.is_null() && idx >= 0 {
                        VECTOR_ELT(consts, idx as i64)
                    } else {
                        R_NilValue()
                    };
                    stack.push(val);
                }

                opcodes::OP_STARTSUBSET | opcodes::OP_ENDSUBSET => {
                    let _nargs = *code_ptr.add(pc as usize);
                    pc += 1;
                    let idx = *code_ptr.add(pc as usize);
                    pc += 1;
                    stack.push(R_NilValue());
                }

                opcodes::OP_STARTSUBSET2 | opcodes::OP_ENDSUBSET2 => {
                    let _nargs = *code_ptr.add(pc as usize);
                    pc += 1;
                    let idx = *code_ptr.add(pc as usize);
                    pc += 1;
                    stack.push(R_NilValue());
                }

                opcodes::OP_LDCLOSURE => {
                    let idx = *code_ptr.add(pc as usize);
                    pc += 1;
                    let val = if !consts.is_null() && idx >= 0 {
                        VECTOR_ELT(consts, idx as i64)
                    } else {
                        R_NilValue()
                    };
                    stack.push(val);
                }

                opcodes::OP_CLOSEDEXPR => {
                    let idx = *code_ptr.add(pc as usize);
                    pc += 1;
                    let val = if !consts.is_null() && idx >= 0 {
                        VECTOR_ELT(consts, idx as i64)
                    } else {
                        R_NilValue()
                    };
                    stack.push(val);
                }

                opcodes::OP_MAKEACTIVE => {
                    let idx = *code_ptr.add(pc as usize);
                    pc += 1;
                    let val = if !consts.is_null() && idx >= 0 {
                        VECTOR_ELT(consts, idx as i64)
                    } else {
                        R_NilValue()
                    };
                    stack.push(val);
                }

                opcodes::OP_SWASTORE
                | opcodes::OP_SWLOAD
                | opcodes::OP_PUTBASE
                | opcodes::OP_PUTBASE_SEP
                | opcodes::OP_SEQBEGIN
                | opcodes::OP_SEQEND => {
                    pc += 1;
                }

                _ => {
                    eprintln!("Error: unknown bytecode opcode {} at pc {}", op, pc - 1);
                    return R_NilValue();
                }
            }
        }

        stack.pop()
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bc_stack_basic() {
        let mut stack = R_bcstack_t::new(4);
        unsafe {
            stack.push(0x1 as SEXP);
            stack.push(0x2 as SEXP);
            assert_eq!(stack.depth(), 2);
            assert_eq!(stack.pop(), 0x2 as SEXP);
            assert_eq!(stack.depth(), 1);
        }
    }

    #[test]
    fn test_opcodes_defined() {
        assert!(opcodes::OP_RETURN > 0);
        assert!(opcodes::OP_PUSHTRUE > 0);
        assert!(opcodes::OP_PUSHFALSE > 0);
        assert!(opcodes::OP_CALL > 0);
        assert!(opcodes::OP_STEPFOR > 0);
        assert!(opcodes::OP_BREAK > 0);
    }

    #[test]
    fn test_bc_push_pop_sequence() {
        let mut stack = R_bcstack_t::new(8);
        unsafe {
            stack.push(R_NilValue());
            stack.push(Rf_ScalarInteger(1));
            stack.push(Rf_ScalarInteger(2));
            assert_eq!(stack.depth(), 3);
            let top = stack.pop();
            assert!(!top.is_null());
            assert_eq!(stack.depth(), 2);
        }
    }

    #[test]
    fn test_bc_dup2() {
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
        use crate::sexp::memory::RArena;
        let mut arena = RArena::new();

        let code = arena.alloc_vector(SEXPTYPE::INTSXP, 5);
        let code_data = unsafe { (*code).gengc_next_node as *mut c_int };
        unsafe {
            code_data.add(0).write(opcodes::OP_PUSHNULL);
            code_data.add(1).write(opcodes::OP_DUP);
            code_data.add(2).write(opcodes::OP_RETURN);
        }

        let consts = arena.alloc_vector(SEXPTYPE::VECSXP, 0);
        let stack_hint = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
        let stack_data = unsafe { (*stack_hint).gengc_next_node as *mut c_int };
        unsafe {
            *stack_data = 8;
        }

        let bcode = arena.alloc_vector(SEXPTYPE::VECSXP, 3);
        let bcode_data = unsafe { (*bcode).gengc_next_node as *mut SEXP };
        unsafe {
            *bcode_data.add(0) = code;
            *bcode_data.add(1) = consts;
            *bcode_data.add(2) = stack_hint;
        }

        let env = arena.alloc_node(SEXPTYPE::ENVSXP);
        unsafe {
            (*env).data.envsxp.frame = R_NilValue();
            (*env).data.envsxp.enclos = R_NilValue();
            (*env).data.envsxp.hashtab = ptr::null_mut();
        }

        let result = unsafe { bcEval(bcode, env) };
        assert!(!result.is_null());
    }
}
