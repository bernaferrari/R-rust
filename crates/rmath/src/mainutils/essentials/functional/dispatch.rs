//! Shared callable/dispatch helpers and element extraction.

use super::*;

/// Whether `expr` is already an evaluated R value (self-evaluating or a
/// container), as opposed to a symbol/closure-call that still needs
/// evaluation in `rho`.
pub(super) fn is_already_a_value(expr: SEXP) -> bool {
    unsafe {
        let t = crate::sexp::accessors::TYPEOF(expr);
        t != SEXPTYPE::SYMSXP.0
            && t != SEXPTYPE::LANGSXP.0
            && t != SEXPTYPE::PROMSXP.0
            && t != SEXPTYPE::BCODESXP.0
    }
}

pub(super) fn callable_expr(fun: SEXP) -> SEXP {
    unsafe {
        if fun.is_null() || fun == R_NilValue() {
            return fun;
        }
        if TYPEOF(fun) == SEXPTYPE::STRSXP && XLENGTH(fun) > 0 {
            let charsxp = STRING_ELT(fun, 0);
            if charsxp.is_null() || charsxp == crate::sexp::globals::R_NaString() {
                return R_NilValue();
            }
            let name = CHAR(charsxp);
            if name.is_null() {
                R_NilValue()
            } else {
                Rf_install(name)
            }
        } else {
            fun
        }
    }
}

pub(super) fn apply_unary_value(fun: SEXP, value: SEXP, rho: SEXP) -> SEXP {
    unsafe { apply_fun_to_element(fun, value, R_NilValue(), rho) }
}

/// Call FUN on one extracted element, forwarding the caller's collected
/// `...` (evaluated once, tags preserved). The element goes in as a
/// pre-forced promise: splicing a language object (`1 + 2` from a saved
/// expression vector) straight into the call makes evalList EVALUATE it;
/// upstream lapply.c passes each element as a forced promise too.
/// Binding the missing-arg sentinel itself would make lookup treat the
/// formal as missing; a forced promise whose value is that sentinel is
/// the empty symbol, matching `vapply(formals(f), FUN, ...)`.
pub(super) fn apply_fun_to_element(fun: SEXP, elem: SEXP, extra_args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let elem_promise = crate::sexp::memory_ext::mkPROMSXP(elem, rho);
        if !elem_promise.is_null() {
            crate::sexp::accessors::SET_PRVALUE(elem_promise, elem);
        }
        let _promise_guard = protect(elem_promise);
        let call_args = Rf_cons(elem_promise, extra_args);
        let _call_args_guard = protect(call_args);
        let call = Rf_cons(fun, call_args);
        if !call.is_null() {
            crate::sexp::accessors::SET_TYPEOF(call, SEXPTYPE::LANGSXP.as_c_int());
        }
        let _call_guard = protect(call);
        crate::eval::closure::applyClosure(
            call,
            fun,
            call_args,
            rho,
            R_NilValue(),
            crate::sexp::ffi::FALSE,
        )
    }
}

pub(super) fn simplify_scalar_list(list: SEXP) -> SEXP {
    unsafe {
        if list.is_null() || TYPEOF(list) != SEXPTYPE::VECSXP {
            return list;
        }
        let n = XLENGTH(list);
        if n == 0 {
            return list;
        }
        let first = VECTOR_ELT(list, 0);
        if first.is_null()
            || !matches!(
                SEXPTYPE(TYPEOF(first)),
                SEXPTYPE::REALSXP | SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP | SEXPTYPE::STRSXP
            )
            || XLENGTH(first) != 1
        {
            return list;
        }
        simplify_scalar_list_as(list, SEXPTYPE(TYPEOF(first)))
    }
}

pub(super) fn simplify_scalar_list_as(list: SEXP, elem_type: SEXPTYPE) -> SEXP {
    unsafe {
        if list.is_null() || TYPEOF(list) != SEXPTYPE::VECSXP {
            return list;
        }
        if elem_type != SEXPTYPE::REALSXP
            && elem_type != SEXPTYPE::INTSXP
            && elem_type != SEXPTYPE::LGLSXP
            && elem_type != SEXPTYPE::STRSXP
        {
            return list;
        }
        let n = XLENGTH(list);
        let result = Rf_allocVector3(elem_type, n);
        if result.is_null() {
            return list;
        }
        let _result_guard = protect(result);
        for i in 0..n {
            let elem = VECTOR_ELT(list, i as i64);
            if elem.is_null() || TYPEOF(elem) != elem_type || XLENGTH(elem) != 1 {
                return list;
            }
            if elem_type == SEXPTYPE::REALSXP {
                *REAL(result).add(i as usize) = *REAL(elem);
            } else if elem_type == SEXPTYPE::INTSXP {
                *INTEGER(result).add(i as usize) = *INTEGER(elem);
            } else if elem_type == SEXPTYPE::LGLSXP {
                *LOGICAL(result).add(i as usize) = *LOGICAL(elem);
            } else if elem_type == SEXPTYPE::STRSXP {
                SET_STRING_ELT(result, i, STRING_ELT(elem, 0));
            }
        }
        let names =
            crate::sexp::attrib_core::getAttrib(list, crate::sexp::attrib_core::R_NamesSymbol());
        if !names.is_null()
            && names != R_NilValue()
            && TYPEOF(names) == SEXPTYPE::STRSXP
            && XLENGTH(names) == n
        {
            crate::sexp::attrib_core::setAttrib(
                result,
                crate::sexp::attrib_core::R_NamesSymbol(),
                names,
            );
        }
        result
    }
}

pub(crate) fn extract_element(x: SEXP, i: R_xlen_t) -> SEXP {
    unsafe {
        let t = TYPEOF(x);
        if t == SEXPTYPE::VECSXP {
            return crate::sexp::accessors::VECTOR_ELT(x, i as i64);
        }
        if t == SEXPTYPE::LISTSXP || t == SEXPTYPE::LANGSXP {
            let mut current = x;
            let mut index = 0;
            while !current.is_null() && current != R_NilValue() {
                if index == i {
                    return CAR(current);
                }
                index += 1;
                current = CDR(current);
            }
            return R_NilValue();
        }
        // Only allocate a scalar element for atomic vector inputs.
        // `Rf_allocVector3(TYPEOF(x), 1)` with an arbitrary type would
        // fabricate a node whose union payload is a vector header
        // ({length, truelength}) but whose SEXPTYPE is non-vector — the GC
        // then traces e.g. a PROMSXP-typed node whose value/expr slots hold
        // the integers 1/1 and whose env slot is uninitialized memory.
        if t != SEXPTYPE::REALSXP
            && t != SEXPTYPE::INTSXP
            && t != SEXPTYPE::LGLSXP
            && t != SEXPTYPE::CPLXSXP
            && t != SEXPTYPE::RAWSXP
            && t != SEXPTYPE::STRSXP
        {
            return R_NilValue();
        }
        let elem = Rf_allocVector3(t, 1);
        if elem.is_null() {
            return R_NilValue();
        }
        if t == SEXPTYPE::REALSXP {
            *REAL(elem) = *REAL(x).add(i as usize);
        } else if t == SEXPTYPE::INTSXP {
            *INTEGER(elem) = *INTEGER(x).add(i as usize);
        } else if t == SEXPTYPE::LGLSXP {
            *LOGICAL(elem) = *LOGICAL(x).add(i as usize);
        } else if t == SEXPTYPE::CPLXSXP {
            *crate::sexp::accessors::COMPLEX(elem) =
                *crate::sexp::accessors::COMPLEX(x).add(i as usize);
        } else if t == SEXPTYPE::RAWSXP {
            *crate::sexp::accessors::RAW(elem) = *crate::sexp::accessors::RAW(x).add(i as usize);
        } else if t == SEXPTYPE::STRSXP {
            crate::sexp::accessors::SET_STRING_ELT(
                elem,
                0,
                crate::sexp::accessors::STRING_ELT(x, i as i64),
            );
        }
        elem
    }
}
