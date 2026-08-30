#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]
#![deny(unsafe_op_in_unsafe_fn)]

//! Default method function declarations and comparison operators.
//!
//! This module ports the default method declarations and comparison/arithmetic
//! operators from eval.c that are used by the bytecode interpreter and S3 dispatch.
//!
//! These functions are declared (not defined) in eval.c and defined elsewhere,
//! except for the comparison functions (cmp_relop, cmp_arith1, cmp_arith2)
//! which are used by the bytecoded interpreter for inline operations.

use std::os::raw::{c_char, c_int};

use crate::sexp::accessors::{CHAR, LENGTH, TYPEOF};
use crate::sexp::builder::int_sequence_current;
use crate::sexp::constructors::*;
use crate::sexp::ffi::{FALSE, NA_INTEGER, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::object::Sexp;
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use super::dispatch::DispatchGroup;

// ---------------------------------------------------------------------------
// External default method declarations
// ---------------------------------------------------------------------------

/// `R_unary(call, op, args)` - unary operator default.
///
/// Defined in arithmetic.c. Dispatches unary arithmetic/math operators.
/// The function signature matches R's definition.
pub unsafe fn R_unary(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let arg_list = Rf_cons(args, R_NilValue());
        let _arg_list_guard = protect(arg_list);
        crate::eval::arithmetic::do_arith(call, op, arg_list, rho)
    }
}

/// `R_binary(call, op, lhs, rhs)` - binary operator default.
///
/// Defined in arithmetic.c. Dispatches binary arithmetic operators.
pub unsafe fn R_binary(call: SEXP, op: SEXP, lhs: SEXP, rhs: SEXP) -> SEXP {
    unsafe {
        let rhs_list = Rf_cons(rhs, R_NilValue());
        let _rhs_list_guard = protect(rhs_list);
        let arg_list = Rf_cons(lhs, rhs_list);
        let _arg_list_guard = protect(arg_list);
        crate::eval::arithmetic::do_arith(call, op, arg_list, R_NilValue())
    }
}

/// `do_math1(call, op, args, rho)` - math function of one argument.
///
/// Defined in math.c. Implements math functions like abs, sqrt, log, etc.
// no_mangle removed (duplicate)
pub unsafe fn do_math1(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        // Forward to the arithmetic module's do_math1 dispatcher.
        crate::eval::arithmetic::do_math1(call, op, args, rho)
    }
}

/// `do_relop_dflt(call, op, lhs, rhs)` - relational operator default.
///
/// Defined in relop.c. Implements comparison operators.
// no_mangle removed (duplicate)
pub unsafe fn do_relop_dflt(call: SEXP, op: SEXP, lhs: SEXP, rhs: SEXP) -> SEXP {
    unsafe { crate::mainutils::relop::do_relop_dflt(call, op, lhs, rhs) }
}

/// `do_logic(call, op, args, rho)` - logical operator default.
///
/// Defined in complex.c. Implements &, |, ! operators.
// no_mangle removed (duplicate)
pub unsafe fn do_logic(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::logic::do_logic(call, op, args, rho) }
}

/// `do_subset_dflt(call, op, args, rho)` - [ default method.
///
/// Defined in subset.c.
// no_mangle removed (duplicate)
pub unsafe fn do_subset_dflt(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::subset::do_subset_dflt(call, op, args, rho) }
}

/// `do_subassign_dflt(call, op, args, rho)` - `[<-` default method.
///
/// Defined in subassign.c.
pub unsafe fn do_subassign_dflt(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::subassign::do_subassign_dflt(call, op, args, rho) }
}

/// `do_c_dflt(call, op, args, rho)` - c() default method.
///
/// Defined in coerce.c.
// no_mangle removed (duplicate)
pub unsafe fn do_c_dflt(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::bind::do_c_dflt(call, op, args, rho) }
}

/// `do_subset2_dflt(call, op, args, rho)` - `[[` default method.
///
/// Defined in subset.c.
// no_mangle removed (duplicate)
pub unsafe fn do_subset2_dflt(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::subset::do_subset2_dflt(call, op, args, rho) }
}

/// `do_subassign2_dflt(call, op, args, rho)` - `[[<-` default method.
///
/// Defined in subassign.c.
// no_mangle removed (duplicate)
pub unsafe fn do_subassign2_dflt(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe { crate::mainutils::subassign::do_subassign2_dflt(call, op, args, rho) }
}

// ---------------------------------------------------------------------------
// seq_int -- create an integer sequence n1:n2
// ---------------------------------------------------------------------------

/// Create an integer sequence from n1 to n2 (inclusive).
///
/// Ported from R's `seq_int()` in eval.c. Used by the bytecode interpreter
/// for the STEPFOR instruction.
pub unsafe fn seq_int(n1: c_int, n2: c_int) -> SEXP {
    int_sequence_current(n1, n2).unwrap_or_else(|| unsafe { R_NilValue() })
}

// ---------------------------------------------------------------------------
// getPrimitive -- get a primitive function by name and type
// ---------------------------------------------------------------------------

/// Get a primitive function by name and type.
///
/// This is used by cmp_relop, cmp_arith1, cmp_arith2 to find the
/// appropriate builtin function for dispatch.
unsafe fn getPrimitive(name: *const c_char, kind: c_int) -> SEXP {
    unsafe {
        let sym = Rf_install(name);
        if sym.is_null() {
            return R_NilValue();
        }
        // Look up the symbol's value in the base environment
        crate::sexp::envir::R_findVar(sym, super::runtime::base_env())
    }
}

// ---------------------------------------------------------------------------
// cmp_relop -- comparison operator with S3 dispatch
// ---------------------------------------------------------------------------

/// Comparison operator with S3 dispatch.
///
/// Ported from R's `cmp_relop()` in eval.c. Used by the bytecode
/// interpreter for inline comparison operations. If either operand
/// is an object, it tries group dispatch to "Ops"; otherwise it
/// calls do_relop_dflt.
pub unsafe fn cmp_relop(
    call: SEXP,
    _opval: c_int,
    opsym: SEXP,
    x: SEXP,
    y: SEXP,
    rho: SEXP,
) -> SEXP {
    unsafe {
        let opsym_name = CHAR(opsym);
        let op = getPrimitive(opsym_name, SEXPTYPE::BUILTINSXP.as_c_int());
        let _op_guard = protect(op);

        let is_obj_x = crate::eval::attrib_core::isObject(x) != 0;
        let is_obj_y = crate::eval::attrib_core::isObject(y) != 0;

        if is_obj_x || is_obj_y {
            let tail = Rf_cons(y, R_NilValue());
            let _tail_guard = protect(tail);
            let args = Rf_cons(x, tail);
            let _args_guard = protect(args);
            let mut ans: SEXP = R_NilValue();
            let dispatched = DispatchGroup(
                b"Ops\x00".as_ptr() as *const c_char,
                call,
                op,
                args,
                rho,
                &mut ans,
            );
            if dispatched != 0 {
                // Dispatched
            } else {
                ans = do_relop_dflt(call, op, x, y);
            }
            return ans;
        }

        do_relop_dflt(call, op, x, y)
    }
}

// ---------------------------------------------------------------------------
// cmp_arith1 -- unary arithmetic with S3 dispatch
// ---------------------------------------------------------------------------

/// Unary arithmetic operator with S3 dispatch.
///
/// Ported from R's `cmp_arith1()` in eval.c. Used by the bytecode
/// interpreter for inline unary operations (-, +).
pub unsafe fn cmp_arith1(call: SEXP, opsym: SEXP, x: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let opsym_name = CHAR(opsym);
        let op = getPrimitive(opsym_name, SEXPTYPE::BUILTINSXP.as_c_int());
        let _op_guard = protect(op);

        if crate::eval::attrib_core::isObject(x) != 0 {
            let args = Rf_cons(x, R_NilValue());
            let _args_guard = protect(args);
            let mut ans: SEXP = R_NilValue();
            let dispatched = DispatchGroup(
                b"Ops\x00".as_ptr() as *const c_char,
                call,
                op,
                args,
                rho,
                &mut ans,
            );
            if dispatched != 0 {
                return ans;
            }
        }

        R_unary(call, op, x, rho)
    }
}

// ---------------------------------------------------------------------------
// cmp_arith2 -- binary arithmetic with S3 dispatch
// ---------------------------------------------------------------------------

/// Binary arithmetic operator with S3 dispatch.
///
/// Ported from R's `cmp_arith2()` in eval.c. Used by the bytecode
/// interpreter for inline binary operations (+, -, *, /, ^, etc.).
pub unsafe fn cmp_arith2(
    call: SEXP,
    _opval: c_int,
    opsym: SEXP,
    x: SEXP,
    y: SEXP,
    rho: SEXP,
) -> SEXP {
    unsafe {
        let opsym_name = CHAR(opsym);
        let op = getPrimitive(opsym_name, SEXPTYPE::BUILTINSXP.as_c_int());
        let _op_guard = protect(op);

        let is_obj_x = crate::eval::attrib_core::isObject(x) != 0;
        let is_obj_y = crate::eval::attrib_core::isObject(y) != 0;

        if is_obj_x || is_obj_y {
            let tail = Rf_cons(y, R_NilValue());
            let _tail_guard = protect(tail);
            let args = Rf_cons(x, tail);
            let _args_guard = protect(args);
            let mut ans: SEXP = R_NilValue();
            let dispatched = DispatchGroup(
                b"Ops\x00".as_ptr() as *const c_char,
                call,
                op,
                args,
                rho,
                &mut ans,
            );
            if dispatched != 0 {
                return ans;
            }
        }

        R_binary(call, op, x, y)
    }
}

// ---------------------------------------------------------------------------
// STACKVAL_TO_SEXP -- convert a stack value to an SEXP
// ---------------------------------------------------------------------------

/// Convert a boxed bytecode stack value to an SEXP.
///
/// In the C implementation, the bytecode stack can hold "boxed" scalars
/// directly (as doubles, ints, or logicals) for performance. This function
/// converts such a boxed value back to a proper SEXP.
///
/// In our Rust port, the stack always holds SEXP values, so this is
/// essentially a no-op, but we provide it for API compatibility.
pub unsafe fn STACKVAL_TO_SEXP(val: SEXP) -> SEXP {
    val
}

// ---------------------------------------------------------------------------
// isNumericOnly -- check if x is numeric but not logical
// ---------------------------------------------------------------------------

/// Check if x is numeric but not logical.
///
/// Used in the bytecode interpreter for type checks.
pub unsafe fn isNumericOnly(x: SEXP) -> c_int {
    unsafe {
        let t = TYPEOF(x);
        if t == SEXPTYPE::REALSXP || t == SEXPTYPE::CPLXSXP || t == SEXPTYPE::INTSXP {
            TRUE
        } else {
            FALSE
        }
    }
}

// ---------------------------------------------------------------------------
// asLogicalNoNA -- coerce to logical without allowing NA
// ---------------------------------------------------------------------------

/// Coerce a value to logical, erroring on NA values.
///
/// Ported from R's `asLogicalNoNA()` in eval.c. Used in condition
/// evaluation for if/while statements.
pub unsafe fn asLogicalNoNA(s: SEXP, _call: SEXP) -> c_int {
    unsafe {
        let len = LENGTH(s);
        if len > 1 {
            eprintln!("Error: the condition has length > 1");
            std::panic::panic_any(crate::sexp::context::RError {
                message: "the condition has length > 1".to_string(),
            });
        }
        if len > 0 {
            let Some(value) = Sexp::from_raw(s) else {
                return NA_INTEGER;
            };
            match value.clone().typeof_(){
                SEXPTYPE::LGLSXP => {
                    logical_scalar_no_na(value.logical_elt(0).unwrap_or(NA_INTEGER))
                }
                SEXPTYPE::INTSXP => {
                    logical_scalar_no_na(value.integer_elt(0).unwrap_or(NA_INTEGER))
                }
                _ => crate::mainutils::coerce::asLogical(s),
            }
        } else {
            NA_INTEGER
        }
    }
}

fn logical_scalar_no_na(value: c_int) -> c_int {
    if value == NA_INTEGER {
        NA_INTEGER
    } else if value != 0 {
        TRUE
    } else {
        FALSE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::constructors::{Rf_ScalarInteger, Rf_ScalarLogical};
    use crate::sexp::session::RSession;

    #[test]
    fn seq_int_returns_filled_ascending_vector() {
        let _session = RSession::new();
        unsafe {
            let seq = seq_int(2, 5);
            let seq = Sexp::from_raw(seq).expect("sequence should allocate");
            assert_eq!(seq.clone().typeof_(), SEXPTYPE::INTSXP);
            assert_eq!(seq.clone().len(), 4);
            assert_eq!(seq.clone().integer_elt(0), Some(2));
            assert_eq!(seq.clone().integer_elt(1), Some(3));
            assert_eq!(seq.clone().integer_elt(2), Some(4));
            assert_eq!(seq.integer_elt(3), Some(5));
        }
    }

    #[test]
    fn seq_int_returns_filled_descending_vector() {
        let _session = RSession::new();
        unsafe {
            let seq = seq_int(3, 0);
            let seq = Sexp::from_raw(seq).expect("sequence should allocate");
            assert_eq!(seq.clone().typeof_(), SEXPTYPE::INTSXP);
            assert_eq!(seq.clone().len(), 4);
            assert_eq!(seq.clone().integer_elt(0), Some(3));
            assert_eq!(seq.clone().integer_elt(1), Some(2));
            assert_eq!(seq.clone().integer_elt(2), Some(1));
            assert_eq!(seq.integer_elt(3), Some(0));
        }
    }

    #[test]
    fn as_logical_no_na_uses_typed_scalar_access() {
        let _session = RSession::new();
        unsafe {
            assert_eq!(asLogicalNoNA(Rf_ScalarLogical(TRUE), R_NilValue()), TRUE);
            assert_eq!(asLogicalNoNA(Rf_ScalarLogical(FALSE), R_NilValue()), FALSE);
            assert_eq!(asLogicalNoNA(Rf_ScalarInteger(42), R_NilValue()), TRUE);
            assert_eq!(asLogicalNoNA(Rf_ScalarInteger(0), R_NilValue()), FALSE);
        }
    }
}
