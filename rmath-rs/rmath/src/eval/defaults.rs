#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Default method function declarations and comparison operators.
//!
//! This module ports the default method declarations and comparison/arithmetic
//! operators from eval.c that are used by the bytecode interpreter and S3 dispatch.
//!
//! These functions are declared (not defined) in eval.c and defined elsewhere,
//! except for the comparison functions (cmp_relop, cmp_arith1, cmp_arith2)
//! which are used by the bytecoded interpreter for inline operations.

use std::os::raw::{c_char, c_int};

use crate::sexp::accessors::{CHAR, INTEGER, LENGTH, LOGICAL, TYPEOF};
use crate::sexp::constructors::*;
use crate::sexp::ffi::{FALSE, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::{R_BaseEnv, R_NilValue};
use crate::sexp::protect::Rf_protect;
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
        crate::eval::arithmetic::do_arith(call, op, arg_list, rho)
    }
}

/// `R_binary(call, op, lhs, rhs)` - binary operator default.
///
/// Defined in arithmetic.c. Dispatches binary arithmetic operators.
pub unsafe fn R_binary(call: SEXP, op: SEXP, lhs: SEXP, rhs: SEXP) -> SEXP {
    unsafe {
        let arg_list = Rf_cons(lhs, Rf_cons(rhs, R_NilValue()));
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
    unsafe {
        let n = if n1 <= n2 { n2 - n1 + 1 } else { n1 - n2 + 1 };
        let ans = Rf_allocVector(SEXPTYPE::INTSXP, n);
        Rf_protect(ans);
        let data = INTEGER(ans);
        if !data.is_null() {
            if n1 <= n2 {
                for i in 0..n {
                    *data.add(i as usize) = n1 + i;
                }
            } else {
                for i in 0..n {
                    *data.add(i as usize) = n1 - i;
                }
            }
        }
        R_NilValue() // unprotect handled by caller
    }
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
        crate::sexp::envir::R_findVar(sym, R_BaseEnv())
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
        Rf_protect(op);

        let is_obj_x = crate::eval::attrib_core::isObject(x) != 0;
        let is_obj_y = crate::eval::attrib_core::isObject(y) != 0;

        if is_obj_x || is_obj_y {
            let args = Rf_cons(x, Rf_cons(y, R_NilValue()));
            Rf_protect(args);
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
        Rf_protect(op);

        if crate::eval::attrib_core::isObject(x) != 0 {
            let args = Rf_cons(x, R_NilValue());
            Rf_protect(args);
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
        Rf_protect(op);

        let is_obj_x = crate::eval::attrib_core::isObject(x) != 0;
        let is_obj_y = crate::eval::attrib_core::isObject(y) != 0;

        if is_obj_x || is_obj_y {
            let args = Rf_cons(x, Rf_cons(y, R_NilValue()));
            Rf_protect(args);
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
pub unsafe fn asLogicalNoNA(s: SEXP, call: SEXP) -> c_int {
    unsafe {
        // Handle common scalar case directly
        if TYPEOF(s) == SEXPTYPE::LGLSXP && LENGTH(s) == 1 {
            let data = LOGICAL(s);
            if !data.is_null() {
                let v = *data;
                if v != crate::sexp::ffi::NA_INTEGER {
                    return if v != 0 { TRUE } else { FALSE };
                }
            }
        } else if TYPEOF(s) == SEXPTYPE::INTSXP && LENGTH(s) == 1 {
            let data = INTEGER(s);
            if !data.is_null() {
                let v = *data;
                if v != crate::sexp::ffi::NA_INTEGER {
                    return if v != 0 { TRUE } else { FALSE };
                }
            }
        }

        let len = LENGTH(s);
        if len > 1 {
            eprintln!("Error: the condition has length > 1");
            std::panic::panic_any(crate::sexp::context::RError {
                message: "the condition has length > 1".to_string(),
            });
        }
        if len > 0 {
            match TYPEOF(s) {
                t if t == SEXPTYPE::LGLSXP => {
                    let data = LOGICAL(s);
                    if !data.is_null() {
                        *data
                    } else {
                        crate::sexp::ffi::NA_INTEGER
                    }
                }
                t if t == SEXPTYPE::INTSXP => {
                    let data = INTEGER(s);
                    if !data.is_null() {
                        *data
                    } else {
                        crate::sexp::ffi::NA_INTEGER
                    }
                }
                _ => crate::mainutils::coerce::asLogical(s),
            }
        } else {
            crate::sexp::ffi::NA_INTEGER
        }
    }
}
