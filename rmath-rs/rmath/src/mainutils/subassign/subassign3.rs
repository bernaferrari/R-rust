#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(unused_imports)]

//! `$<-` and `@<-` paths — do_subassign3 and R_subassign3_dflt.

use std::os::raw::{c_char, c_double, c_int};
use std::ptr;

use crate::mainutils::subscript::{
    OneIndex, get1index, int_arraySubscript, makeSubscript, mat2indsub, strmat2intmat, vectorIndex,
};
use crate::sexp::accessors::*;
use crate::sexp::constructors::*;
use crate::sexp::envir::defineVar;
use crate::sexp::ffi::{FALSE, NA_INTEGER, R_xlen_t, SEXP, SEXPTYPE, TRUE};
use crate::sexp::globals::R_NilValue;
use crate::sexp::memory_ext::{allocList, allocSExp};
use crate::sexp::protect::protect;
use crate::sexp::symbol::Rf_install;

use super::support::{SET_S4_OBJECT, SET_TRUELENGTH, UNSET_S4_OBJECT};
use super::*;

/// Port of `do_subassign3()` -- the `$<-` operator.
pub(crate) unsafe fn do_subassign3(call: SEXP, op: SEXP, args: SEXP, env: SEXP) -> SEXP {
    unsafe {
        let mut nlist: SEXP = R_NilValue();
        checkArity(op, args);
        let args = fixSubset3Args(call, args, env, &mut nlist);
        let _args_guard = protect(args);

        let mut ans: SEXP = ptr::null_mut();
        if R_DispatchOrEvalSP(
            call,
            op,
            b"$\x00<-".as_ptr() as *const c_char,
            args,
            env,
            &mut ans,
        ) != 0
        {
            return ans;
        }
        let _ans_guard = protect(ans);
        let result = R_subassign3_dflt(call, CAR(ans), nlist, CADDR(ans));
        result
    }
}

/// Port of `R_subassign3_dflt()` -- default `$<-` implementation.
pub unsafe fn R_subassign3_dflt(call: SEXP, x: SEXP, nlist: SEXP, val: SEXP) -> SEXP {
    unsafe {
        let mut x = x;
        let mut val = val;
        // Upstream has no early NULL return: a NULL target is grown below
        // (coerced to an empty list / new one-element pairlist as needed).

        let s4 = IS_S4_OBJECT(x);
        let mut xS4: SEXP = R_NilValue();
        let nprotect = 0;

        if MAYBE_SHARED(x) {
            x = shallow_duplicate(x);
        }

        // Code to allow classes to extend ENVSXP
        if TYPEOF(x) == OBJSXP {
            xS4 = x;
            x = R_getS4DataSlot(x, ANYSXP);
            if isNull(x) {
                // Error: no method
                return xS4;
            }
        }

        if (isList(x) || isLanguage(x)) && !isNull(x) {
            if TAG(x) == nlist {
                if isNull(val) {
                    SET_ATTRIB(CDR(x), ATTRIB(x));
                    if IS_S4_OBJECT(x) != 0 {
                        SET_S4_OBJECT(CDR(x));
                    } else {
                        UNSET_S4_OBJECT(CDR(x));
                    }
                    SET_OBJECT(CDR(x), OBJECT(x));
                    RAISE_NAMED(CDR(x), NAMED(x));
                    SETCAR(x, R_NilValue());
                    x = CDR(x);
                } else {
                    if MAYBE_REFERENCED(val) && CAR(x) != val {
                        val = R_FixupRHS(x, val);
                    }
                    SETCAR(x, val);
                }
            } else {
                let mut t = x;
                while !isNull(t) {
                    if TAG(CDR(t)) == nlist {
                        if isNull(val) {
                            SETCAR(CDR(t), R_NilValue());
                            SETCDR(t, CDDR(t));
                        } else {
                            if MAYBE_REFERENCED(val) && CADR(t) != val {
                                val = R_FixupRHS(x, val);
                            }
                            SETCAR(CDR(t), val);
                        }
                        break;
                    } else if isNull(CDR(t)) && !isNull(val) {
                        SETCDR(t, allocSExp(SEXPTYPE::LISTSXP));
                        SETTAG(CDR(t), nlist);
                        if MAYBE_REFERENCED(val) {
                            ENSURE_NAMEDMAX(val);
                        }
                        SETCADR(t, val);
                        break;
                    }
                    t = CDR(t);
                }
            }
            if isNull(x) && !isNull(val) {
                x = allocList(1);
                if MAYBE_REFERENCED(val) {
                    ENSURE_NAMEDMAX(val);
                }
                SETCAR(x, val);
                SETTAG(x, nlist);
            }
        } else if TYPEOF(x) == ENVSXP {
            defineVar(nlist, val, x);
            INCREMENT_NAMED(val);
        } else if TYPEOF(x) == SYMSXP
            || TYPEOF(x) == CLOSXP
            || TYPEOF(x) == SPECIALSXP
            || TYPEOF(x) == BUILTINSXP
        {
            errorNotSubsettable(x);
        } else {
            let nx = XLENGTH(x);
            let mut atype = VECSXP;

            if isExpression(x) {
                atype = EXPRSXP;
            } else if !isNewList(x) {
                x = coerceVector(x, VECSXP);
            }

            let names = getAttrib(x, crate::eval::attrib_core::R_NamesSymbol());
            let nlist_name = PRINTNAME(nlist);

            if isNull(val) {
                // Element deletion
                if !isNull(names) {
                    let mut imatch: i64 = -1;
                    for i in 0..nx {
                        if NonNullStringMatch(STRING_ELT(names, i), nlist_name) != 0 {
                            imatch = i as i64;
                            break;
                        }
                    }
                    if imatch >= 0 {
                        let ans = Rf_allocVector3(atype, nx - 1);
                        let ansnames = Rf_allocVector3(STRSXP, nx - 1);
                        let mut ii: R_xlen_t = 0;
                        for i in 0..nx {
                            if i != imatch as R_xlen_t {
                                SET_VECTOR_ELT(ans, ii, VECTOR_ELT(x, i));
                                SET_STRING_ELT(ansnames, ii, STRING_ELT(names, i));
                                ii += 1;
                            }
                        }
                        setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), ansnames);
                        copyMostAttrib(x, ans);
                        x = ans;
                    }
                }
            } else {
                // Replace or add element
                let mut imatch: i64 = -1;
                if !isNull(names) {
                    for i in 0..nx {
                        if NonNullStringMatch(STRING_ELT(names, i), nlist_name) != 0 {
                            imatch = i as i64;
                            break;
                        }
                    }
                }
                if imatch >= 0 {
                    // Replace existing element
                    if MAYBE_REFERENCED(val) && VECTOR_ELT(x, imatch as R_xlen_t) != val {
                        val = R_FixupRHS(x, val);
                    }
                    SET_VECTOR_ELT(x, imatch as R_xlen_t, val);
                } else {
                    // Add new element
                    let ans = Rf_allocVector3(VECSXP, nx + 1);
                    let ansnames = Rf_allocVector3(STRSXP, nx + 1);
                    for i in 0..nx {
                        SET_VECTOR_ELT(ans, i, VECTOR_ELT(x, i));
                        if isNull(names) {
                            SET_STRING_ELT(ansnames, i, R_BlankString());
                        } else {
                            SET_STRING_ELT(ansnames, i, STRING_ELT(names, i));
                        }
                    }
                    if MAYBE_REFERENCED(val) {
                        ENSURE_NAMEDMAX(val);
                    }
                    SET_VECTOR_ELT(ans, nx, val);
                    SET_STRING_ELT(ansnames, nx, nlist_name);
                    setAttrib(ans, crate::eval::attrib_core::R_NamesSymbol(), ansnames);
                    copyMostAttrib(x, ans);
                    x = ans;
                }
            }
        }

        if !isNull(xS4) {
            x = xS4;
        }
        SETTER_CLEAR_NAMED(x);
        if s4 != 0 {
            SET_S4_OBJECT(x);
        }
        x
    }
}
