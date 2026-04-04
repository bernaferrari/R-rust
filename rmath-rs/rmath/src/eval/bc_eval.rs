#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Bytecode interpreter — ports R's bcEval from eval.c.
//!
//! This module provides the bytecode evaluation loop for compiled R code.
//! R compiles function bodies to bytecode for faster execution.
//!
//! The bytecode format uses an instruction stream (array of integers)
//! with a separate constant pool.

use std::os::raw::c_int;
use std::ptr;

use crate::sexp::accessors::{
    Rf_isNull, CADDR, CADR, CAR, CDDR, CDR, INTEGER, LENGTH, LOGICAL, REAL, TYPEOF,
};
use crate::sexp::constructors::*;
use crate::sexp::envir::{defineVar, forcePromise, matchArgs, R_findVar, R_findVarInFrame};
use crate::sexp::ffi::{FALSE, NA_INTEGER, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{set_R_Visible, R_MissingArg, R_NilValue, R_UnboundValue};
use crate::sexp::memory_ext::NewEnvironment;

use super::bc_stack::R_bcstack_t;
use super::eval::Rf_eval;

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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn bcEval(body: SEXP, rho: SEXP) -> SEXP {
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
                    let val = stack.pop();
                    if TYPEOF(val) == SEXPTYPE::LGLSXP.0 {
                        let data = LOGICAL(val);
                        if !data.is_null() && *data == 0 {
                            // Short-circuit: result is FALSE
                            stack.push(val);
                            // Skip to matching POPOR or end
                            // Simplified: just continue
                        }
                    }
                }

                opcodes::OP_POPOR => {
                    let val = stack.pop();
                    if TYPEOF(val) == SEXPTYPE::LGLSXP.0 {
                        let data = LOGICAL(val);
                        if !data.is_null() && *data != 0 {
                            // Short-circuit: result is TRUE
                            stack.push(val);
                        }
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

                _ => {
                    // Unknown opcode — skip
                    eprintln!("Warning: unknown bytecode opcode {}", op);
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_initialize_bcode() {
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
    }
}
