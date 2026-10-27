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
    CAR, CDR, CHAR, COMPLEX, ENCLOS, INTEGER, LENGTH, LOGICAL, PRINTNAME, RAW, REAL, STRING_ELT,
    TYPEOF, VECTOR_ELT,
};
use crate::sexp::constructors::*;
use crate::sexp::context::RError;
use crate::sexp::envir::{R_findVar, defineVar, forcePromise, setVar};
use crate::sexp::ffi::{FALSE, NA_LOGICAL, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_MissingArg, R_NilValue, R_UnboundValue};

use super::bc_stack::{R_bcstack_t, own_operand};
use crate::sexp::object::Sexp;

fn bc_error(message: impl Into<String>) -> ! {
    std::panic::panic_any(RError {
        message: message.into(),
    });
}

/// Loud refusal for bytecode ABI / opcode gaps — prefer hard failure over a
/// silent wrong answer. Message shape mirrors GNU R's `BCMISMATCH` signal.
fn bc_mismatch(detail: impl Into<String>) -> ! {
    bc_error(format!("BCMISMATCH: {}", detail.into()));
}

/// Instruction boundaries retain the body, environment, constants, call
/// frames and operand stack as owning values, with no native payload loan.
fn collect_at_instruction_boundary(pin: &crate::sexp::owner::OwnerPin) {
    pin.require_live()
        .unwrap_or_else(|error| bc_error(error.to_string()));
    let owner = unsafe { crate::sexp::owner::OwnerToken::current() }
        .unwrap_or_else(|error| bc_error(error.to_string()));
    if owner.as_ptr() != pin.as_ptr() {
        bc_error("bytecode collection requires the original active runtime");
    }
    crate::sexp::gengc::maybe_collect_at_eval_safe_point();
    unsafe { crate::eval::profiling::sample_at_safe_point(owner) }
        .unwrap_or_else(|error| bc_error(error.to_string()));
    owner
        .require_active()
        .unwrap_or_else(|error| bc_error(error.to_string()));
    pin.require_live()
        .unwrap_or_else(|error| bc_error(error.to_string()));
}

/// Language object of the CALL that follows this GETFUN in the GNU stream,
/// falling back to the current function context. GNU `findFun` attributes
/// `R_FunctionNotFoundError` with `R_CurrentExpression`; in bytecode that
/// is the CALL constant.
unsafe fn gnu_pending_call(words: &[c_int], mut pc: usize, constants: &[Sexp<'static>]) -> SEXP {
    unsafe {
        while pc < words.len() {
            let opcode = words[pc];
            if opcode == super::bytecode::GNU_OP_CALL
                || opcode == super::bytecode::GNU_OP_CALLBUILTIN
            {
                if pc + 1 >= words.len() {
                    break;
                }
                let call = owned_constant_at(constants, words[pc + 1] as i64, "GNU pending call");
                if TYPEOF(call) == SEXPTYPE::LANGSXP {
                    return call;
                }
                break;
            }
            let Some(&width) = super::bytecode::GNU_BC_OPERAND_WIDTHS.get(opcode as usize) else {
                break;
            };
            pc += 1 + width as usize;
        }
        crate::mainutils::errors::R_getCurrentCall()
    }
}

fn bc_missing_arg_error(arg_sym: SEXP) -> ! {
    unsafe {
        crate::mainutils::errors::R_MissingArgError(
            arg_sym,
            crate::mainutils::errors::R_getCurrentCall(),
            c"getvarError".as_ptr(),
        );
    }
    unreachable!("R_MissingArgError signals");
}

fn bc_unbound_object_error(symbol: SEXP, rho: SEXP) -> ! {
    unsafe {
        crate::mainutils::errors::R_ObjectNotFoundError(
            symbol,
            super::context::get_lexical_call(rho),
            None,
        )
    }
}

// ---------------------------------------------------------------------------
// Bytecode opcodes
// ---------------------------------------------------------------------------

/// Bytecode instruction opcodes for the private runtime dialect.
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
    pub const OP_STARTFOR: i32 = 54;
    pub const OP_MAKECLOSURE: i32 = 59;
    pub const OP_NEXTFOR: i32 = 55;
    /// Superassignment `<<-`: store into the enclosing frame (eval.c SETVAR2).
    pub const OP_SETVAR2: i32 = 56;
    /// Attach the formal-name tag to the argument on top of the stack
    /// (eval.c SETTAG), so lazy call arguments bind by name.
    pub const OP_SETTAG: i32 = 57;
    /// Mark the stack top shared (NAMED = 2) because it stays live across a later call.
    pub const OP_MARK_SHARED: i32 = 60;
    /// Temporary link while a later index still runs. 3 is sticky.
    pub const OP_BUMP_LINK: i32 = 61;
    pub const OP_DROP_LINK: i32 = 62;
    /// Invoke a flat replacement function, store its object, retain the RHS.
    pub const OP_REPLACEMENT: i32 = 63;
    /// Bounded root/optional-dollar replacement with explicit local/super scope.
    pub const OP_REPLACEMENT_PATH: i32 = 64;
    /// Validated owning getter/setter levels for general replacement syntax.
    pub const OP_REPLACEMENT_CHAIN: i32 = 65;
    /// Call with cached arguments and a separate original syntax constant.
    pub const OP_CALL_WITH_SOURCE: i32 = 66;
    /// Apply original expressions, including dots, with the original call.
    pub const OP_CALL_SYNTAX_WITH_SOURCE: i32 = 67;
    pub const OP_LAST: i32 = 67;
}

mod bcode_accessors;
mod calls;
mod entry;
mod execute;
mod ops;
mod private_calls;
mod stack;
mod subset_ops;

pub(crate) use bcode_accessors::*;
pub(crate) use calls::*;
pub(crate) use entry::*;
pub(crate) use execute::*;
pub(crate) use ops::*;
pub(crate) use private_calls::*;
pub(crate) use stack::*;
pub(crate) use subset_ops::*;

#[cfg(test)]
mod tests;
