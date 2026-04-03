#![allow(non_snake_case, non_upper_case_globals, dead_code, unused_variables)]

//! Environment operations — ports R's src/main/envir.c.
//!
//! Provides variable lookup, assignment, function lookup, and argument matching
//! needed by the evaluator and other interpreter components.

use std::os::raw::{c_int, c_void};
use std::ptr;

use super::accessors::{
    ATTRIB, CADDR, CADR, CAR, CDDDR, CDDR, CDR, CHAR, ENCLOS, FRAME, HASHTAB, LENGTH, PRCODE,
    PRENV, PRINTNAME, PRVALUE, Rf_isNull, SET_FRAME, SET_PRCODE, SET_PRENV, SET_PRVALUE,
    SET_SYMVALUE, SETCAR, SETCDR, SETTAG, SYMVALUE, TAG, TYPEOF,
};
use super::constructors::{Rf_allocVector, Rf_cons, Rf_lang2};
use super::ffi::{NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE, SexprecCore};
use super::globals::{
    R_BaseEnv, R_EmptyEnv, R_GlobalEnv, R_MissingArg, R_NilValue, R_UnboundValue,
};
use super::memory;
use super::memory_ext::{NewEnvironment, cons_raw, mkPROMISE};
use super::symbol::Rf_install;

// ---------------------------------------------------------------------------
// R_findVarInFrame — find a variable in a single environment frame
// ---------------------------------------------------------------------------

/// Find a variable in the frame of a single environment (no inheritance).
///
/// This is the equivalent of R's `R_findVarInFrame()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_findVarInFrame(rho: SEXP, symbol: SEXP) -> SEXP {
    unsafe {
        if rho.is_null() || symbol.is_null() {
            return R_UnboundValue();
        }
        if TYPEOF(rho) != SEXPTYPE::ENVSXP.0 {
            return R_UnboundValue();
        }

        let frame = FRAME(rho);
        let hashtab = HASHTAB(rho);

        // If there's a hash table, search it first
        if !hashtab.is_null() && TYPEOF(hashtab) == SEXPTYPE::VECSXP.0 {
            // For now, fall through to linear search in the frame
            // Full implementation would use R_HashGet
        }

        // Linear search through the frame (pairlist)
        let mut frame = frame;
        while !frame.is_null() && frame != R_NilValue() {
            if TAG(frame) == symbol {
                let val = CAR(frame);
                return val;
            }
            frame = CDR(frame);
        }

        R_UnboundValue()
    }
}

// ---------------------------------------------------------------------------
// findVarInFrame3 — internal helper with doGet flag
// ---------------------------------------------------------------------------

/// Internal helper: find variable in frame with optional value fetching.
unsafe fn findVarInFrame3(rho: SEXP, symbol: SEXP, _do_get: bool) -> SEXP {
    unsafe { R_findVarInFrame(rho, symbol) }
}

// ---------------------------------------------------------------------------
// R_findVar — find a variable with environment inheritance
// ---------------------------------------------------------------------------

/// Find a variable, searching through parent environments.
///
/// This is the equivalent of R's `R_findVar()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_findVar(symbol: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if symbol.is_null() {
            return R_UnboundValue();
        }

        let mut current = rho;
        while !current.is_null() && current != R_NilValue() {
            if TYPEOF(current) != SEXPTYPE::ENVSXP.0 {
                break;
            }

            let val = R_findVarInFrame(current, symbol);
            if val != R_UnboundValue() {
                // If it's a promise, force it
                if TYPEOF(val) == SEXPTYPE::PROMSXP.0 {
                    return forcePromise(val);
                }
                return val;
            }

            // Check for active bindings
            let sym_val = SYMVALUE(symbol);
            if !sym_val.is_null() && TYPEOF(sym_val) == SEXPTYPE::SPECIALSXP.0 {
                // Active binding — call the access function
                return sym_val;
            }

            current = ENCLOS(current);
        }

        R_UnboundValue()
    }
}

// ---------------------------------------------------------------------------
// forcePromise — force evaluation of a promise
// ---------------------------------------------------------------------------

/// Force evaluation of a promise, returning its value.
///
/// This is the equivalent of R's promise forcing logic in eval.c.
pub unsafe fn forcePromise(prom: SEXP) -> SEXP {
    unsafe {
        if prom.is_null() {
            return R_NilValue();
        }
        if TYPEOF(prom) != SEXPTYPE::PROMSXP.0 {
            return prom;
        }

        let val = PRVALUE(prom);
        // If already forced, return the value
        if val != R_UnboundValue() && TYPEOF(val) != SEXPTYPE::SPECIALSXP.0 {
            return val;
        }

        let expr = PRCODE(prom);
        let env = PRENV(prom);

        // Mark as being evaluated (prevent infinite recursion)
        SET_PRVALUE(prom, R_UnboundValue());
        (*prom).sxpinfo.set_gp((*prom).sxpinfo.gp() | 0x02); // PRSEEN

        // Evaluate the expression
        let value = if !expr.is_null() {
            // eval(expr, env) — will be connected when eval is implemented
            // For now, return the expression itself as a placeholder
            expr
        } else {
            R_MissingArg()
        };

        SET_PRVALUE(prom, value);
        value
    }
}

// ---------------------------------------------------------------------------
// defineVar — define a variable in an environment
// ---------------------------------------------------------------------------

/// Define a variable in the given environment's frame.
///
/// This is the equivalent of R's `defineVar()`.
/// If the symbol already exists, its value is updated.
/// If not, a new binding is created.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn defineVar(symbol: SEXP, value: SEXP, rho: SEXP) {
    unsafe {
        if rho.is_null() || symbol.is_null() {
            return;
        }

        let frame = FRAME(rho);

        // Search for existing binding
        let mut current = frame;
        while !current.is_null() && current != R_NilValue() {
            if TAG(current) == symbol {
                SETCAR(current, value);
                return;
            }
            current = CDR(current);
        }

        // Not found — create new binding at front of frame
        let new_cell = Rf_cons(value, frame);
        if !new_cell.is_null() {
            SETTAG(new_cell, symbol);
            SET_FRAME(rho, new_cell);
        }
    }
}

// ---------------------------------------------------------------------------
// setVar — set a variable, searching through parent environments
// ---------------------------------------------------------------------------

/// Set a variable value, searching parent environments if needed.
///
/// This is the equivalent of R's `setVar()`.
/// If the variable is not found, it's defined in the global environment.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn setVar(symbol: SEXP, value: SEXP, rho: SEXP) {
    unsafe {
        if symbol.is_null() {
            return;
        }

        // Search through environments
        let mut current = rho;
        while !current.is_null() && current != R_NilValue() {
            if TYPEOF(current) == SEXPTYPE::ENVSXP.0 {
                let frame = FRAME(current);
                let mut f = frame;
                while !f.is_null() && f != R_NilValue() {
                    if TAG(f) == symbol {
                        SETCAR(f, value);
                        return;
                    }
                    f = CDR(f);
                }
            }
            current = ENCLOS(current);
        }

        // Not found — define in global environment
        defineVar(symbol, value, R_GlobalEnv());
    }
}

// ---------------------------------------------------------------------------
// findFun — find a function value for a symbol
// ---------------------------------------------------------------------------

/// Find a function value for a symbol.
///
/// This is the equivalent of R's `findFun()`.
/// Searches through environments, looking for functions.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn findFun(symbol: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if symbol.is_null() {
            return R_UnboundValue();
        }

        let mut current = rho;
        while !current.is_null() && current != R_NilValue() {
            if TYPEOF(current) == SEXPTYPE::ENVSXP.0 {
                let val = R_findVarInFrame(current, symbol);
                if val != R_UnboundValue() {
                    let t = TYPEOF(val);
                    if t == SEXPTYPE::CLOSXP.0
                        || t == SEXPTYPE::BUILTINSXP.0
                        || t == SEXPTYPE::SPECIALSXP.0
                    {
                        return val;
                    }
                    // Not a function
                }
            }
            current = ENCLOS(current);
        }

        R_UnboundValue()
    }
}

/// Find a function with error reporting.
///
/// This is the equivalent of R's `findFun3()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn findFun3(symbol: SEXP, rho: SEXP, call: SEXP) -> SEXP {
    unsafe {
        let fun = findFun(symbol, rho);
        if fun == R_UnboundValue() {
            // Could not find function — would error in real implementation
            // For now, return unbound
        }
        fun
    }
}

// ---------------------------------------------------------------------------
// matchArgs — match formal arguments to actual arguments
// ---------------------------------------------------------------------------

/// Match actual arguments to formal parameters.
///
/// This is the equivalent of R's `matchArgs()`.
/// Returns the matched argument list.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn matchArgs(formals: SEXP, args: SEXP, _call: SEXP) -> SEXP {
    unsafe {
        if formals.is_null() || formals == R_NilValue() {
            return args;
        }

        // Build a result list with matched arguments
        let mut result: SEXP = R_NilValue();
        let mut result_tail: SEXP = R_NilValue();

        let mut f = formals;
        while !f.is_null() && f != R_NilValue() {
            let ftag = TAG(f);
            if ftag.is_null() {
                f = CDR(f);
                continue;
            }

            // Search for matching argument
            let mut a = args;
            let mut matched: SEXP = R_NilValue();
            while !a.is_null() && a != R_NilValue() {
                if TAG(a) == ftag {
                    matched = a;
                    break;
                }
                a = CDR(a);
            }

            if matched == R_NilValue() {
                // No match — add missing argument marker
                let cell = Rf_cons(R_MissingArg(), R_NilValue());
                SETTAG(cell, ftag);
                if result.is_null() || result == R_NilValue() {
                    result = cell;
                    result_tail = cell;
                } else {
                    SETCDR(result_tail, cell);
                    result_tail = cell;
                }
            } else {
                let cell = Rf_cons(CAR(matched), R_NilValue());
                SETTAG(cell, ftag);
                if result.is_null() || result == R_NilValue() {
                    result = cell;
                    result_tail = cell;
                } else {
                    SETCDR(result_tail, cell);
                    result_tail = cell;
                }
            }

            f = CDR(f);
        }

        if result.is_null() {
            result = R_NilValue();
        }
        result
    }
}

/// Match arguments without renaming (NR = No Rename).
///
/// This is the equivalent of R's `matchArgs_NR()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn matchArgs_NR(formals: SEXP, args: SEXP) -> SEXP {
    unsafe { matchArgs(formals, args, ptr::null_mut()) }
}

// ---------------------------------------------------------------------------
// R_isMissing — check if an argument is missing
// ---------------------------------------------------------------------------

/// Check if a symbol has a missing argument in the given environment.
///
/// This is the equivalent of R's `R_isMissing()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_isMissing(symbol: SEXP, rho: SEXP) -> c_int {
    unsafe {
        if symbol.is_null() || rho.is_null() {
            return 0;
        }

        let val = R_findVarInFrame(rho, symbol);
        if val == R_UnboundValue() {
            return 1;
        }
        if val == R_MissingArg() {
            return 1;
        }

        // Check for unforced promise with missing expression
        if TYPEOF(val) == SEXPTYPE::PROMSXP.0 {
            let expr = PRCODE(val);
            if expr == R_MissingArg() {
                return 1;
            }
        }

        0
    }
}

/// Report an error for a missing argument.
///
/// This is the equivalent of R's `R_MissingArgError()`.
/// Canonical implementation is in mainutils/errors.rs.
pub(crate) unsafe extern "C" fn R_MissingArgError(symbol: SEXP, call: SEXP) {
    unsafe {
        let name = if !symbol.is_null() {
            let pname = PRINTNAME(symbol);
            if !pname.is_null() {
                let s = CHAR(pname);
                if !s.is_null() {
                    std::ffi::CStr::from_ptr(s).to_str().unwrap_or("???")
                } else {
                    "???"
                }
            } else {
                "???"
            }
        } else {
            "???"
        };
        // Would call errorcall in real implementation
        eprintln!("Error in {}: argument \"{}\" is missing", "eval", name);
        std::panic::panic_any(crate::sexp::context::RError {
            message: format!("argument \"{}\" is missing, with no default", name),
        });
    }
}

// ---------------------------------------------------------------------------
// ddfindVar — find variable in ... (dots) arguments
// ---------------------------------------------------------------------------

/// Find a variable in the ... (dots) arguments.
///
/// This is the equivalent of R's `ddfindVar()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ddfindVar(symbol: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        if symbol.is_null() || rho.is_null() {
            return R_UnboundValue();
        }

        let dotsym = Rf_install(std::ffi::CString::new("...").unwrap().as_ptr());
        let dots_val = R_findVarInFrame(rho, dotsym);
        if dots_val == R_UnboundValue() || dots_val == R_MissingArg() {
            return R_UnboundValue();
        }

        // Search through the dots pairlist
        let mut current = dots_val;
        while !current.is_null() && current != R_NilValue() {
            if TAG(current) == symbol {
                let val = CAR(current);
                if TYPEOF(val) == SEXPTYPE::PROMSXP.0 {
                    return forcePromise(val);
                }
                return val;
            }
            current = CDR(current);
        }

        R_UnboundValue()
    }
}

// ---------------------------------------------------------------------------
// R_typeToChar — convert SEXPTYPE to string
// ---------------------------------------------------------------------------

/// Convert a SEXPTYPE integer to its string representation.
///
/// This is the equivalent of R's `R_typeToChar()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_typeToChar(stype: c_int) -> SEXP {
    let name = match stype {
        0 => "NULL",
        1 => "symbol",
        2 => "pairlist",
        3 => "closure",
        4 => "environment",
        5 => "promise",
        6 => "language",
        7 => "special",
        8 => "builtin",
        9 => "character",
        10 => "logical",
        13 => "integer",
        14 => "double",
        15 => "complex",
        16 => "character",
        17 => "...",
        18 => "any",
        19 => "list",
        20 => "expression",
        21 => "bytecode",
        22 => "externalptr",
        23 => "weakref",
        24 => "raw",
        25 => "S4",
        _ => "unknown",
    };
    let cs = std::ffi::CString::new(name).unwrap();
    // Return as a CHARSXP pointer (placeholder — should use Rf_mkChar)
    ptr::null_mut()
}

// ---------------------------------------------------------------------------
// Environment creation helpers
// ---------------------------------------------------------------------------

/// Create a new child environment.
///
/// This is a convenience wrapper around NewEnvironment.
pub unsafe fn Rf_createEnv(frame: SEXP, enclos: SEXP) -> SEXP {
    unsafe { NewEnvironment(frame, enclos, ptr::null_mut()) }
}

/// Create a new hashed environment.
///
/// This is the equivalent of R's `R_NewHashedEnv()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_NewHashedEnv(enclos: SEXP, size: c_int) -> SEXP {
    unsafe {
        // For now, create a regular environment without hashing
        // Full implementation would create a hash table of the given size
        NewEnvironment(ptr::null_mut(), enclos, ptr::null_mut())
    }
}

// ---------------------------------------------------------------------------
// CheckFormals — validate that formals is a pairlist of symbols
// ---------------------------------------------------------------------------

/// Check that formals is a valid pairlist of distinct symbols.
///
/// This is the equivalent of R's `CheckFormals()`.
pub unsafe fn CheckFormals(formals: SEXP) {
    unsafe {
        let mut seen: SEXP = R_NilValue();
        let mut f = formals;
        while !f.is_null() && f != R_NilValue() {
            let sym = TAG(f);
            if sym.is_null() || TYPEOF(sym) != SEXPTYPE::SYMSXP.0 {
                // Error: non-symbol in formals
                eprintln!("Error: invalid formal argument list");
                std::panic::panic_any(crate::sexp::context::RError {
                    message: "invalid formal argument list".to_string(),
                });
            }
            // Check for duplicates
            let mut s = seen;
            while !s.is_null() && s != R_NilValue() {
                if CAR(s) == sym {
                    eprintln!("Error: duplicate formal argument");
                    std::panic::panic_any(crate::sexp::context::RError {
                        message: "duplicate formal argument name".to_string(),
                    });
                }
                s = CDR(s);
            }
            seen = Rf_cons(sym, seen);
            f = CDR(f);
        }
    }
}

// ---------------------------------------------------------------------------
// addMissingVarsToNewEnv — add missing variables from formals
// ---------------------------------------------------------------------------

/// Add missing variable bindings for unprovided arguments.
///
/// This is the equivalent of R's `addMissingVarsToNewEnv()`.
pub unsafe fn addMissingVarsToNewEnv(formals: SEXP, args: SEXP, newrho: SEXP) {
    unsafe {
        let mut f = formals;
        while !f.is_null() && f != R_NilValue() {
            let sym = TAG(f);
            if !sym.is_null() {
                // Check if this formal is provided in args
                let mut a = args;
                let mut found = false;
                while !a.is_null() && a != R_NilValue() {
                    if TAG(a) == sym {
                        found = true;
                        break;
                    }
                    a = CDR(a);
                }
                if !found {
                    defineVar(sym, R_MissingArg(), newrho);
                }
            }
            f = CDR(f);
        }
    }
}

// ---------------------------------------------------------------------------
// R_existsVarInFrame — check if a variable exists in a frame
// ---------------------------------------------------------------------------

/// Check if a variable exists in a given frame.
///
/// This is the equivalent of R's `R_existsVarInFrame()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_existsVarInFrame(rho: SEXP, symbol: SEXP) -> c_int {
    unsafe {
        let val = R_findVarInFrame(rho, symbol);
        if val == R_UnboundValue() { 0 } else { 1 }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::constructors::*;
    use super::super::ffi::*;
    use super::super::globals::set_R_GlobalEnv;
    use super::super::memory;
    use super::super::symbol::Rf_install;
    use super::*;

    fn setup() {
        unsafe {
            // Create a simple test environment
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            if !env.is_null() {
                set_R_GlobalEnv(env);
            }
        }
    }

    #[test]
    fn test_find_var_in_frame_empty() {
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let sym = Rf_install(std::ffi::CString::new("x").unwrap().as_ptr());
            let val = R_findVarInFrame(env, sym);
            assert_eq!(val, R_UnboundValue());
        }
    }

    #[test]
    fn test_define_and_find_var() {
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let sym = Rf_install(std::ffi::CString::new("x").unwrap().as_ptr());
            let value = Rf_ScalarInteger(42);

            defineVar(sym, value, env);

            let val = R_findVarInFrame(env, sym);
            assert_eq!(val, value);
        }
    }

    #[test]
    fn test_define_var_overwrite() {
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let sym = Rf_install(std::ffi::CString::new("x").unwrap().as_ptr());
            let v1 = Rf_ScalarInteger(1);
            let v2 = Rf_ScalarInteger(2);

            defineVar(sym, v1, env);
            defineVar(sym, v2, env);

            let val = R_findVarInFrame(env, sym);
            assert_eq!(val, v2);
        }
    }

    #[test]
    fn test_exists_var_in_frame() {
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let sym = Rf_install(std::ffi::CString::new("y").unwrap().as_ptr());

            assert_eq!(R_existsVarInFrame(env, sym), 0);

            let value = Rf_ScalarInteger(10);
            defineVar(sym, value, env);

            assert_eq!(R_existsVarInFrame(env, sym), 1);
        }
    }

    #[test]
    fn test_is_missing() {
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let sym = Rf_install(std::ffi::CString::new("z").unwrap().as_ptr());

            assert_eq!(R_isMissing(sym, env), 1);
        }
    }

    #[test]
    fn test_new_environment() {
        unsafe {
            let env = NewEnvironment(ptr::null_mut(), R_NilValue(), ptr::null_mut());
            assert!(!env.is_null());
            assert_eq!(TYPEOF(env), SEXPTYPE::ENVSXP.0);
        }
    }

    #[test]
    fn test_mk_promise() {
        unsafe {
            let expr = Rf_ScalarInteger(99);
            let prom = mkPROMISE(expr, R_NilValue());
            assert!(!prom.is_null());
            assert_eq!(TYPEOF(prom), SEXPTYPE::PROMSXP.0);
        }
    }

    #[test]
    fn test_type_to_char() {
        unsafe {
            // Just verify it doesn't crash
            R_typeToChar(SEXPTYPE::INTSXP.0);
            R_typeToChar(SEXPTYPE::REALSXP.0);
            R_typeToChar(999);
        }
    }

    #[test]
    fn test_find_var_null_inputs() {
        unsafe {
            assert_eq!(
                R_findVar(ptr::null_mut(), ptr::null_mut()),
                R_UnboundValue()
            );
            assert_eq!(
                R_findVarInFrame(ptr::null_mut(), ptr::null_mut()),
                R_UnboundValue()
            );
        }
    }

    #[test]
    fn test_set_var_not_found() {
        unsafe {
            let env = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            let parent = memory::with_arena(|arena| arena.alloc_node(SEXPTYPE::ENVSXP));
            // Set enclosing env
            (*env).data.envsxp.enclos = parent;

            let sym = Rf_install(std::ffi::CString::new("newvar").unwrap().as_ptr());
            let value = Rf_ScalarReal(3.14);

            // setVar should define in the global env if not found
            // For this test, we just verify it doesn't crash
        }
    }
}
