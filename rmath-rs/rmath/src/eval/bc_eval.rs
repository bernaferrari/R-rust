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

use crate::sexp::accessors::{
    CHAR, INTEGER, LENGTH, LOGICAL, PRINTNAME, REAL, Rf_isNull, TYPEOF, VECTOR_ELT,
};
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::envir::{R_findVar, defineVar, forcePromise};
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_MissingArg, R_NilValue, R_UnboundValue};

use super::bc_stack::R_bcstack_t;

fn bc_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

fn bc_missing_arg_error(arg_sym: SEXP) -> ! {
    let message = unsafe {
        if arg_sym.is_null() {
            "argument is missing, with no default".to_string()
        } else {
            let pname = PRINTNAME(arg_sym);
            let name = if pname.is_null() {
                "???".to_string()
            } else {
                let chars = CHAR(pname);
                if chars.is_null() {
                    "???".to_string()
                } else {
                    std::ffi::CStr::from_ptr(chars)
                        .to_str()
                        .map(str::to_string)
                        .unwrap_or_else(|_| "???".to_string())
                }
            };
            format!("argument \"{name}\" is missing, with no default")
        }
    };
    // Attribute to the enclosing call like upstream signalMissingArgError
    // (which passes the bc interpreter's current expression); the innermost
    // context call is the closure call being evaluated.
    crate::mainutils::errors::errorcall_str(
        unsafe { crate::mainutils::errors::R_getCurrentCall() },
        &message,
    )
}

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
        if TYPEOF(val) == SEXPTYPE::LGLSXP {
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

/// Get the source expression stored in bytecode constant slot 0.
pub unsafe fn BCODE_EXPR(x: SEXP) -> SEXP {
    unsafe {
        if x.is_null() || TYPEOF(x) != SEXPTYPE::BCODESXP {
            return R_NilValue();
        }
        let consts = BCODE_CONSTS(x);
        if consts.is_null() || TYPEOF(consts) != SEXPTYPE::VECSXP || LENGTH(consts) == 0 {
            return R_NilValue();
        }
        VECTOR_ELT(consts, 0)
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

unsafe fn read_operand(
    code_ptr: *const c_int,
    pc: &mut c_int,
    code_len: c_int,
    opname: &str,
) -> c_int {
    unsafe {
        if *pc >= code_len {
            bc_error(format!("{opname} bytecode operand is truncated"));
        }
        let value = *code_ptr.add(*pc as usize);
        *pc += 1;
        value
    }
}

unsafe fn read_jump_target(
    code_ptr: *const c_int,
    pc: &mut c_int,
    code_len: c_int,
    opname: &str,
) -> c_int {
    unsafe {
        let target = read_operand(code_ptr, pc, code_len, opname);
        if target < 0 || target > code_len {
            bc_error(format!(
                "{opname} bytecode jump target {target} is outside instruction stream length {code_len}"
            ));
        }
        target
    }
}

unsafe fn constant_at(consts: SEXP, idx: c_int, context: &str) -> SEXP {
    unsafe {
        if idx < 0 {
            bc_error(format!("{context} uses negative constant index {idx}"));
        }
        if consts.is_null() || consts == R_NilValue() {
            bc_error(format!("{context} requires a bytecode constant pool"));
        }
        if TYPEOF(consts) != SEXPTYPE::VECSXP && TYPEOF(consts) != SEXPTYPE::EXPRSXP {
            bc_error(format!(
                "{context} requires a vector constant pool, got {:?}",
                TYPEOF(consts)
            ));
        }
        let len = LENGTH(consts);
        if idx >= len {
            bc_error(format!(
                "{context} constant index {idx} out of range for pool length {len}"
            ));
        }
        VECTOR_ELT(consts, idx as i64)
    }
}

unsafe fn stack_pop_checked(stack: &mut R_bcstack_t, context: &str) -> SEXP {
    unsafe {
        let value = stack.pop();
        if value.is_null() {
            bc_error(format!("{context} bytecode stack underflow"));
        }
        value
    }
}

unsafe fn stack_top_checked(stack: &R_bcstack_t, context: &str) -> SEXP {
    unsafe {
        let value = stack.top();
        if value.is_null() {
            bc_error(format!("{context} bytecode stack underflow"));
        }
        value
    }
}

unsafe fn stack_at_checked(stack: &R_bcstack_t, index: usize, context: &str) -> SEXP {
    unsafe {
        if index >= stack.depth() {
            bc_error(format!("{context} bytecode stack slot {index} is missing"));
        }
        stack.at(index)
    }
}

/// Run `f` with the residual operand stack (and `extra`) rooted on the
/// protection stack.
///
/// The bytecode operand stack lives in a Rust local, invisible to the
/// collector's root scan. Nested evaluation (`Rf_eval`, `forcePromise`) can
/// reach an eval safe point and run a deferred collection; without this
/// window, every value still live in operand slots below the ones an op has
/// already popped — plus the freshly built call/args cons cells — would be
/// swept mid-op. `protect_n` returns an RAII guard, so the bounded window
/// also unwinds cleanly when the nested evaluation raises an R error.
unsafe fn with_stack_rooted<F, R>(stack: &R_bcstack_t, extra: SEXP, f: F) -> R
where
    F: FnOnce() -> R,
{
    unsafe {
        let depth = stack.depth();
        let mut rooted = 0usize;
        crate::sexp::instance::with_required_current_instance(|inst| {
            for i in 0..depth {
                let val = stack.at(i);
                if !val.is_null() {
                    crate::sexp::protect::push_protect_in(inst, val);
                    rooted += 1;
                }
            }
            if !extra.is_null() {
                crate::sexp::protect::push_protect_in(inst, extra);
                rooted += 1;
            }
        });
        let _unwind = crate::sexp::protect::protect_n(rooted);
        f()
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
        if body.is_null() || TYPEOF(body) != SEXPTYPE::BCODESXP {
            bc_error("bcEval requires a bytecode object");
        }

        let code_ptr = BCODE_CODE(body);
        let consts = BCODE_CONSTS(body);
        let stack_depth = BCODE_STACK(body);

        if code_ptr.is_null() {
            bc_error("bytecode object has no instruction stream");
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
                    let idx = read_operand(code_ptr, &mut pc, code_len, "PUSHCONST");
                    let val = constant_at(consts, idx, "PUSHCONST");
                    stack.push(val);
                }

                opcodes::OP_PUSHCONSTARG => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "PUSHCONSTARG");
                    let val = constant_at(consts, idx, "PUSHCONSTARG");
                    stack.push(val);
                }

                opcodes::OP_GETVAR => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "GETVAR");
                    let sym = constant_at(consts, idx, "GETVAR");
                    let val = R_findVar(sym, rho);
                    if val == R_UnboundValue() {
                        bc_error("object not found");
                    } else if val == R_MissingArg() {
                        bc_missing_arg_error(sym);
                    } else if TYPEOF(val) == SEXPTYPE::DOTSXP {
                        // A `...` binding is a DOTSXP, never an ordinary
                        // value. The compiler must not emit GETVAR for it
                        // (GNU R handles dots ops as runtime builtins); if a
                        // hand-built bytecode does anyway, fall back to the
                        // AST evaluator instead of pushing the raw DOTSXP.
                        let call = Rf_cons(sym, R_NilValue());
                        if !call.is_null() {
                            (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                        }
                        let evaluated = with_stack_rooted(&stack, call, || unsafe {
                            crate::eval::eval::Rf_eval(call, rho)
                        });
                        stack.push(evaluated);
                    } else if TYPEOF(val) == SEXPTYPE::PROMSXP {
                        let forced =
                            with_stack_rooted(&stack, val, || unsafe { forcePromise(val) });
                        if forced == R_MissingArg() {
                            bc_missing_arg_error(sym);
                        }
                        stack.push(forced);
                    } else {
                        stack.push(val);
                    }
                }

                opcodes::OP_SETVAR => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "SETVAR");
                    let sym = constant_at(consts, idx, "SETVAR");
                    let val = stack_pop_checked(&mut stack, "SETVAR");
                    defineVar(sym, val, rho);
                    super::runtime::set_visible(FALSE);
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
                    return stack_pop_checked(&mut stack, "RETURN");
                }

                opcodes::OP_visible => {
                    super::runtime::set_visible(TRUE);
                }

                opcodes::OPinvisible => {
                    super::runtime::set_visible(FALSE);
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
                    let val = stack_top_checked(&stack, "BRIFNOT");
                    let cond = eval_bc_condition(val);
                    if !cond {
                        pc = target;
                    }
                }

                opcodes::OP_BRIFTRUE => {
                    let target = read_jump_target(code_ptr, &mut pc, code_len, "BRIFTRUE");
                    let val = stack_top_checked(&stack, "BRIFTRUE");
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
                    let expr = constant_at(consts, idx, "MAKEPROMISE");
                    let prom = crate::sexp::memory_ext::mkPROMSXP(expr, rho);
                    stack.push(prom);
                }

                opcodes::OP_DOMISSING => {
                    stack.push(R_NilValue());
                }

                opcodes::OP_CALL => {
                    let nargs = read_operand(code_ptr, &mut pc, code_len, "CALL");
                    let fun = stack_pop_checked(&mut stack, "CALL function");
                    let mut args = R_NilValue();
                    for _ in 0..nargs {
                        let arg = stack_pop_checked(&mut stack, "CALL argument");
                        args = Rf_cons(arg, args);
                    }
                    if fun.is_null() {
                        stack.push(R_NilValue());
                    } else {
                        let call = Rf_cons(fun, args);
                        if !call.is_null() {
                            (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                        }
                        let result = with_stack_rooted(&stack, call, || unsafe {
                            crate::eval::eval::Rf_eval(call, rho)
                        });
                        stack.push(result);
                    }
                }

                opcodes::OP_CALLBUILTIN => {
                    let nargs = read_operand(code_ptr, &mut pc, code_len, "CALLBUILTIN");
                    let fun = stack_pop_checked(&mut stack, "CALLBUILTIN function");
                    let mut args = R_NilValue();
                    for _ in 0..nargs {
                        let arg = stack_pop_checked(&mut stack, "CALLBUILTIN argument");
                        args = Rf_cons(arg, args);
                    }
                    if fun.is_null() {
                        stack.push(R_NilValue());
                    } else {
                        let call = Rf_cons(fun, args);
                        if !call.is_null() {
                            (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                        }
                        let result = with_stack_rooted(&stack, call, || unsafe {
                            crate::eval::eval::Rf_eval(call, rho)
                        });
                        stack.push(result);
                    }
                    super::runtime::set_visible(TRUE);
                }

                opcodes::OP_CALLSPECIAL => {
                    let nargs = read_operand(code_ptr, &mut pc, code_len, "CALLSPECIAL");
                    let fun = stack_pop_checked(&mut stack, "CALLSPECIAL function");
                    let mut args = R_NilValue();
                    for _ in 0..nargs {
                        let arg = stack_pop_checked(&mut stack, "CALLSPECIAL argument");
                        args = Rf_cons(arg, args);
                    }
                    if fun.is_null() {
                        stack.push(R_NilValue());
                    } else {
                        let call = Rf_cons(fun, args);
                        if !call.is_null() {
                            (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                        }
                        let result = with_stack_rooted(&stack, call, || unsafe {
                            crate::eval::eval::Rf_eval(call, rho)
                        });
                        stack.push(result);
                    }
                    super::runtime::set_visible(FALSE);
                }

                opcodes::OP_STARTASSIGN => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "STARTASSIGN");
                    let sym = constant_at(consts, idx, "STARTASSIGN");
                    let val = R_findVar(sym, rho);
                    stack.push(sym);
                    stack.push(val);
                }

                opcodes::OP_ENDASSIGN => {
                    let _nargs = read_operand(code_ptr, &mut pc, code_len, "ENDASSIGN");
                    let val = stack_pop_checked(&mut stack, "ENDASSIGN value");
                    let sym = stack_pop_checked(&mut stack, "ENDASSIGN symbol");
                    defineVar(sym, val, rho);
                    stack.push(val);
                    super::runtime::set_visible(FALSE);
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
                    let ctr = stack_pop_checked(&mut stack, "SETLOOPCTR");
                    if TYPEOF(ctr) == SEXPTYPE::INTSXP {
                        let d = INTEGER(ctr);
                        if !d.is_null() {
                            *d = -1;
                        }
                    }
                    stack.push(ctr);
                }

                opcodes::OP_BEGINLOOP => {
                    stack.push(R_NilValue());
                }

                opcodes::OP_ENDLOOP => {
                    stack_pop_checked(&mut stack, "ENDLOOP");
                }

                opcodes::OP_HIDDENCALL => {
                    let _nargs = read_operand(code_ptr, &mut pc, code_len, "HIDDENCALL");
                    let fun = stack_pop_checked(&mut stack, "HIDDENCALL function");
                    let args = stack_pop_checked(&mut stack, "HIDDENCALL arguments");
                    if fun.is_null() {
                        stack.push(R_NilValue());
                    } else {
                        let call = Rf_lang2(fun, args);
                        let result = with_stack_rooted(&stack, call, || unsafe {
                            crate::eval::eval::Rf_eval(call, rho)
                        });
                        stack.push(result);
                    }
                    super::runtime::set_visible(FALSE);
                }

                opcodes::OP_PUSHARG => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "PUSHARG");
                    let val = constant_at(consts, idx, "PUSHARG");
                    stack.push(val);
                }

                opcodes::OP_PUSHFUN => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "PUSHFUN");
                    let val = constant_at(consts, idx, "PUSHFUN");
                    stack.push(val);
                }

                opcodes::OP_PUSHNILVALUE => {
                    stack.push(R_NilValue());
                }

                opcodes::OP_DFLTFUN => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "DFLTFUN");
                    let val = constant_at(consts, idx, "DFLTFUN");
                    stack.push(val);
                }

                opcodes::OP_DFLTFORM => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "DFLTFORM");
                    let val = constant_at(consts, idx, "DFLTFORM");
                    stack.push(val);
                }

                opcodes::OP_STARTSUBSET | opcodes::OP_ENDSUBSET => {
                    let nargs = read_operand(code_ptr, &mut pc, code_len, "SUBSET");
                    let _idx = read_operand(code_ptr, &mut pc, code_len, "SUBSET");
                    // x[i, j, ...] — pop nargs index values, then the object
                    let mut indices: Vec<SEXP> = Vec::new();
                    for _ in 0..nargs {
                        indices.push(stack_pop_checked(&mut stack, "SUBSET index"));
                    }
                    let obj = stack_pop_checked(&mut stack, "SUBSET object");
                    if obj.is_null() || indices.is_empty() {
                        stack.push(R_NilValue());
                    } else {
                        // Call `[`(obj, i, j, ...)
                        {
                            let bracket_sym = crate::sexp::symbol::Rf_install(c"[".as_ptr());
                            let nil = R_NilValue();
                            let mut arg_list = nil;
                            // Build args in reverse order
                            for idx_expr in indices.into_iter().rev() {
                                arg_list = Rf_cons(idx_expr, arg_list);
                            }
                            // Prepend the object
                            arg_list = Rf_cons(obj, arg_list);
                            let call = Rf_cons(bracket_sym, arg_list);
                            if !call.is_null() {
                                (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
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
                    let _idx = read_operand(code_ptr, &mut pc, code_len, "SUBSET2");
                    // x[[i]] — pop index value, then the object
                    let idx = if nargs > 0 {
                        stack_pop_checked(&mut stack, "SUBSET2 index")
                    } else {
                        R_NilValue()
                    };
                    let obj = stack_pop_checked(&mut stack, "SUBSET2 object");
                    if obj.is_null() {
                        stack.push(R_NilValue());
                    } else {
                        {
                            let dbracket_sym = crate::sexp::symbol::Rf_install(c"[[".as_ptr());
                            let nil = R_NilValue();
                            let args = Rf_cons(obj, Rf_cons(idx, nil));
                            let call = Rf_cons(dbracket_sym, args);
                            if !call.is_null() {
                                (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
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
                    let val = constant_at(consts, idx, "LDCLOSURE");
                    stack.push(val);
                }

                opcodes::OP_CLOSEDEXPR => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "CLOSEDEXPR");
                    let val = constant_at(consts, idx, "CLOSEDEXPR");
                    stack.push(val);
                }

                opcodes::OP_MAKEACTIVE => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "MAKEACTIVE");
                    let val = constant_at(consts, idx, "MAKEACTIVE");
                    stack.push(val);
                }

                opcodes::OP_SWASTORE => {
                    let _idx = read_operand(code_ptr, &mut pc, code_len, "SWASTORE");
                    let val = stack_pop_checked(&mut stack, "SWASTORE");
                    stack.push(val); // simplified: just pass through
                }

                opcodes::OP_SWLOAD => {
                    let idx = read_operand(code_ptr, &mut pc, code_len, "SWLOAD");
                    let _switch_val = stack_pop_checked(&mut stack, "SWLOAD");
                    let val = constant_at(consts, idx, "SWLOAD");
                    stack.push(val);
                }

                opcodes::OP_PUTBASE => {
                    let _nargs = read_operand(code_ptr, &mut pc, code_len, "PUTBASE");
                    let val = stack_pop_checked(&mut stack, "PUTBASE value");
                    let sym = stack_pop_checked(&mut stack, "PUTBASE symbol");
                    if !sym.is_null() && TYPEOF(sym) == SEXPTYPE::SYMSXP {
                        {
                            defineVar(sym, val, super::runtime::base_env());
                        }
                    }
                    stack.push(val);
                    super::runtime::set_visible(FALSE);
                }

                opcodes::OP_PUTBASE_SEP => {
                    let _nargs = read_operand(code_ptr, &mut pc, code_len, "PUTBASE_SEP");
                    let val = stack_pop_checked(&mut stack, "PUTBASE_SEP value");
                    let sym = stack_pop_checked(&mut stack, "PUTBASE_SEP symbol");
                    if !sym.is_null() && TYPEOF(sym) == SEXPTYPE::SYMSXP {
                        {
                            defineVar(sym, val, super::runtime::base_env());
                        }
                    }
                    stack.push(val);
                }

                opcodes::OP_SEQBEGIN | opcodes::OP_SEQEND => {
                    let _idx = read_operand(code_ptr, &mut pc, code_len, "SEQ");
                }

                _ => {
                    bc_error(format!("unknown bytecode opcode {} at pc {}", op, pc - 1));
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
        let code_data = unsafe { (*code).gengc_next_node as *mut c_int };
        for (i, instruction) in instructions.iter().enumerate() {
            unsafe {
                code_data.add(i).write(*instruction);
            }
        }

        let stack_hint = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
        let stack_data = unsafe { (*stack_hint).gengc_next_node as *mut c_int };
        unsafe {
            *stack_data = 8;
        }

        let bcode = arena.alloc_vector(SEXPTYPE::BCODESXP, 3);
        let bcode_data = unsafe { (*bcode).gengc_next_node as *mut SEXP };
        unsafe {
            *bcode_data.add(0) = code;
            *bcode_data.add(1) = consts;
            *bcode_data.add(2) = stack_hint;
        }
        bcode
    }

    fn empty_env(arena: &mut crate::sexp::memory::RArena) -> SEXP {
        let env = arena.alloc_node(SEXPTYPE::ENVSXP);
        unsafe {
            (*env).data.envsxp.frame = R_NilValue();
            (*env).data.envsxp.enclos = R_NilValue();
            (*env).data.envsxp.hashtab = ptr::null_mut();
        }
        env
    }

    #[test]
    fn test_bc_stack_basic() {
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
        assert!(opcodes::OP_RETURN > 0);
        assert!(opcodes::OP_PUSHTRUE > 0);
        assert!(opcodes::OP_PUSHFALSE > 0);
        assert!(opcodes::OP_CALL > 0);
        assert!(opcodes::OP_STEPFOR > 0);
        assert!(opcodes::OP_BREAK > 0);
    }

    #[test]
    fn test_bc_push_pop_sequence() {
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
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
        let _session = crate::sexp::session::RSession::new();
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

        let bcode = arena.alloc_vector(SEXPTYPE::BCODESXP, 3);
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

    #[test]
    fn test_bc_eval_rejects_unknown_opcode() {
        let _session = crate::sexp::session::RSession::new();
        use crate::sexp::memory::RArena;
        let mut arena = RArena::new();

        let code = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
        let code_data = unsafe { (*code).gengc_next_node as *mut c_int };
        unsafe {
            code_data.write(9999);
        }

        let consts = arena.alloc_vector(SEXPTYPE::VECSXP, 0);
        let stack_hint = arena.alloc_vector(SEXPTYPE::INTSXP, 1);
        let stack_data = unsafe { (*stack_hint).gengc_next_node as *mut c_int };
        unsafe {
            *stack_data = 8;
        }

        let bcode = arena.alloc_vector(SEXPTYPE::BCODESXP, 3);
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

        let err = assert_r_error(|| unsafe {
            bcEval(bcode, env);
        });
        assert!(err.message.contains("unknown bytecode opcode"));
    }

    #[test]
    fn test_bc_eval_rejects_truncated_operand() {
        let _session = crate::sexp::session::RSession::new();
        let mut arena = crate::sexp::memory::RArena::new();
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
        let _session = crate::sexp::session::RSession::new();
        let mut arena = crate::sexp::memory::RArena::new();
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
        let _session = crate::sexp::session::RSession::new();
        let mut arena = crate::sexp::memory::RArena::new();
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
        let _session = crate::sexp::session::RSession::new();
        let mut arena = crate::sexp::memory::RArena::new();
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
        let _session = crate::sexp::session::RSession::new();
        let mut arena = crate::sexp::memory::RArena::new();
        let consts = arena.alloc_vector(SEXPTYPE::VECSXP, 1);
        let value = unsafe { Rf_ScalarInteger(42) };
        unsafe {
            let data = (*consts).gengc_next_node as *mut SEXP;
            *data = value;
        }
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
        let mut session = crate::sexp::session::RSession::new();

        // Callee whose body forces a full collection while the caller's
        // bytecode frame is suspended mid-OP_CALL.
        let gc_closure = session
            .with_arena(|arena| unsafe {
                let clos = arena.alloc_node(SEXPTYPE::CLOSXP);
                (*clos).data.closxp.formals = R_NilValue();
                let body = Rf_cons(
                    crate::sexp::symbol::Rf_install(c"gc".as_ptr()),
                    R_NilValue(),
                );
                (*body).sxpinfo.set_type(SEXPTYPE::LANGSXP);
                (*clos).data.closxp.body = body;
                (*clos).data.closxp.env = crate::sexp::globals::R_BaseEnv();
                clos
            })
            .expect("session active");
        let _callee_guard = crate::sexp::protect::protect(gc_closure);

        // Detached arena keeps the bytecode alive for the whole test but is
        // invisible to the instance collector.
        let mut arena = crate::sexp::memory::RArena::new();
        let consts = arena.alloc_vector(SEXPTYPE::VECSXP, 1);
        unsafe {
            let data = (*consts).gengc_next_node as *mut SEXP;
            *data.add(0) = gc_closure;
        }
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
}
