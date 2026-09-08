#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use super::*;

// ---------------------------------------------------------------------------
// curlyahead — check if expression is a curly-brace block
// ---------------------------------------------------------------------------

/// Check if s is a list whose first element is a curly brace ({).
/// Used for correct if-then-else formatting.
pub unsafe fn curlyahead(s: SEXP) -> bool {
    unsafe {
        if (isList(s) || isLanguage(s)) && isSymbol(CAR(s)) {
            CAR(s) == R_BraceSymbol()
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// needsparens — determine if an argument needs parenthesization
// ---------------------------------------------------------------------------

/// Check if an argument to a unary or binary operator needs parentheses.
///
/// mainop_kind, mainop_prec, mainop_rightassoc describe the outer operator.
/// arg is an argument to it, on the left if left == 1.
/// deepLeft is the precedence from further up the left side.
pub unsafe fn needsparens(
    mainop_kind: c_int,
    mainop_prec: c_int,
    mainop_rightassoc: c_int,
    arg: SEXP,
    left: c_int,
    deepLeft: c_int,
) -> bool {
    unsafe {
        if let Some(mut arginfo) = get_arg_ppinfo(arg) {
            // Not all binary ops are binary!
            match arginfo.kind {
                PP_BINARY | PP_BINARY2 => {
                    let nargs = Rf_length(CDR(arg));
                    match nargs {
                        1 => {
                            // binary +/- precedence upgraded as unary
                            if arginfo.prec == PREC_SUM {
                                arginfo.prec = PREC_SIGN;
                            }
                            arginfo.kind = PP_UNARY;
                        }
                        2 => {}
                        _ => return false,
                    }
                }
                _ => {} // intentionally unhandled: SEXPTYPE not relevant for deparse precedence
            }

            match arginfo.kind {
                PP_SUBSET => {
                    match mainop_kind {
                        PP_DOLLAR | PP_SUBSET => {
                            if mainop_prec > arginfo.prec {
                                return false;
                            }
                            // else fall through
                        }
                        _ => {} // intentionally unhandled: unknown precedence level for deparse
                    }
                    // fall through
                }
                PP_BINARY | PP_BINARY2 => {
                    if mainop_prec == PREC_COMPARE && arginfo.prec == PREC_COMPARE {
                        return true; // a < b < c is not legal syntax
                    }
                    // fall through
                }
                PP_ASSIGN | PP_ASSIGN2 | PP_DOLLAR => {}
                _ => {} // intentionally unhandled: unknown PP pattern for deparse
            }

            match arginfo.kind {
                PP_BINARY | PP_BINARY2 | PP_ASSIGN | PP_ASSIGN2 | PP_DOLLAR => {
                    if mainop_prec > arginfo.prec
                        || (mainop_prec == arginfo.prec && left == mainop_rightassoc)
                    {
                        return true;
                    }
                }
                PP_UNARY => {
                    return (left != 0 && mainop_prec > arginfo.prec)
                        || (deepLeft != 0 && deepLeft > arginfo.prec);
                }
                PP_FOR | PP_IF | PP_WHILE | PP_REPEAT => {
                    return left != 0 || deepLeft != 0;
                }
                PP_SUBSET => {
                    if mainop_kind != PP_DOLLAR
                        && mainop_kind != PP_SUBSET
                        && (mainop_prec > arginfo.prec
                            || (mainop_prec == arginfo.prec && left == mainop_rightassoc))
                    {
                        return true;
                    }
                }
                _ => return false,
            }
        } else if isLanguage(arg)
            && isUserBinop(CAR(arg))
            && (mainop_prec > PREC_PERCENT
                || (mainop_prec == PREC_PERCENT && left == mainop_rightassoc))
        {
            return true;
        }
        false
    }
}

// ---------------------------------------------------------------------------
// usable_nice_names — check if names can be used in nice form
// ---------------------------------------------------------------------------

/// Check if the character vector x contains no NA_character_ or all "",
/// or if isAtomic, does not contain "recursive" or "use.names".
pub unsafe fn usable_nice_names(x: SEXP, isAtomic: bool) -> bool {
    unsafe {
        if !isString(x) {
            return true;
        }
        let n = XLENGTH(x) as usize;
        let mut all_0 = true;
        for i in 0..n {
            let elt = STRING_ELT(x, i as R_xlen_t);
            if elt.is_null() || elt == R_NilValue() {
                return false;
            }
            if isAtomic {
                let name = CHAR(elt);
                if !name.is_null() {
                    let bytes = std::ffi::CStr::from_ptr(name).to_bytes();
                    if bytes == b"recursive" || bytes == b"use.names" {
                        return false;
                    }
                }
            }
            if all_0 {
                let name = CHAR(elt);
                if !name.is_null() && *name != 0 {
                    all_0 = false;
                }
            }
        }
        !all_0
    }
}

// ---------------------------------------------------------------------------

// parenthesizeCaller — check if a function caller needs parentheses
// ---------------------------------------------------------------------------

/// Check if a function caller needs to be parenthesized.
/// For example: `(f+g)(z)` needs parens, but `x$f(z)` does not.
pub unsafe fn parenthesizeCaller(s: SEXP) -> bool {
    unsafe {
        if TYPEOF(s) != SEXPTYPE::LANGSXP {
            return false;
        }
        let op = CAR(s);
        if isSymbol(op) {
            if isUserBinop(op) {
                return true;
            } // %foo%
            let sym = SYMVALUE(op);
            let t = TYPEOF(sym);
            let pp = if t == SEXPTYPE::BUILTINSXP || t == SEXPTYPE::SPECIALSXP {
                Some(getPPinfo(sym))
            } else {
                // This port dispatches language specials by name rather
                // than binding them into the symbol table, so `function`
                // (and friends) are unbound here even though stock R
                // resolves them to SPECIALSXPs. The funtab carries the
                // same PPINFO trunk uses — look it up by name.
                getPPinfo_for_symbol(op)
            };
            match pp {
                Some(pp) => {
                    !(pp.prec >= PREC_SUBSET
                        || pp.kind == PP_FUNCALL
                        || pp.kind == PP_PAREN
                        || pp.kind == PP_CURLY)
                }
                None => false, // regular function call
            }
        } else if TYPEOF(op) == SEXPTYPE::CLOSXP {
            return true;
        } else {
            return true; // something strange, like (1)(x)
        }
    }
}

// ---------------------------------------------------------------------------
