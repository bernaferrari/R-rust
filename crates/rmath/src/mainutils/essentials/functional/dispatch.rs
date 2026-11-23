//! Shared callable/dispatch helpers and element extraction.

use super::*;

/// Whether `expr` is already an evaluated R value (self-evaluating or a
/// container), as opposed to a symbol/closure-call that still needs
/// evaluation in `rho`.
pub(super) fn is_already_a_value(expr: SEXP) -> bool {
    {
        let t = crate::sexp::accessors::TYPEOF(expr);
        t != SEXPTYPE::SYMSXP.0
            && t != SEXPTYPE::LANGSXP.0
            && t != SEXPTYPE::PROMSXP.0
            && t != SEXPTYPE::BCODESXP.0
    }
}

pub(super) fn callable_expr(fun: SEXP) -> SEXP {
    if fun.is_null() || fun == R_NilValue() {
        return fun;
    }
    if TYPEOF(fun) == SEXPTYPE::STRSXP && XLENGTH(fun) > 0 {
        let charsxp = STRING_ELT(fun, 0);
        if charsxp.is_null() || charsxp == crate::sexp::globals::R_NaString() {
            return R_NilValue();
        }
        install_cstr(&charsxp_cstring(charsxp))
    } else {
        fun
    }
}

pub(super) fn apply_unary_value(fun: SEXP, value: SEXP, rho: SEXP) -> SEXP {
    apply_fun_to_element(fun, value, R_NilValue(), rho)
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
    {
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

pub(super) fn simplify_scalar_list_as(list: SEXP, elem_type: SEXPTYPE) -> SEXP {
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
            set_real_checked(result, (i as usize) as R_xlen_t, real_elt_checked(elem, 0));
        } else if elem_type == SEXPTYPE::INTSXP {
            set_integer_checked(
                result,
                (i as usize) as R_xlen_t,
                integer_elt_checked(elem, 0),
            );
        } else if elem_type == SEXPTYPE::LGLSXP {
            set_logical_checked(
                result,
                (i as usize) as R_xlen_t,
                logical_elt_checked(elem, 0),
            );
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

pub(crate) fn extract_element(x: SEXP, i: R_xlen_t) -> SEXP {
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
    let owner = active_owner();
    let x = owner_sexp(owner, x);
    let elem = Rf_allocVector3(t, 1);
    if elem.is_null() {
        return R_NilValue();
    }
    let elem = owner_sexp(owner, elem);
    let mut dst = sexp_ok(crate::sexp::object::SexpMut::try_from_checked(elem.clone()));
    match SEXPTYPE(t) {
        SEXPTYPE::REALSXP => sexp_ok(dst.try_set_real_elt(0, sexp_ok(x.try_real_elt(i)))),
        SEXPTYPE::INTSXP => sexp_ok(dst.try_set_integer_elt(0, sexp_ok(x.try_integer_elt(i)))),
        SEXPTYPE::LGLSXP => sexp_ok(dst.try_set_logical_elt(0, sexp_ok(x.try_logical_elt(i)))),
        SEXPTYPE::CPLXSXP => sexp_ok(dst.try_set_complex_elt(0, sexp_ok(x.try_complex_elt(i)))),
        SEXPTYPE::RAWSXP => sexp_ok(dst.try_set_raw_elt(0, sexp_ok(x.try_raw_elt(i)))),
        SEXPTYPE::STRSXP => sexp_ok(dst.try_set_string_elt(0, sexp_ok(x.try_string_elt(i)))),
        _ => unreachable!("checked atomic element type"),
    }
    sexp_ok(owner.require_active());
    elem.as_raw()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::altrep::{
        AltrepBuilder, AltrepClass, AltrepContext, AltrepElement, is_materialized,
    };
    use crate::sexp::{object::SexpResult, session::RSession};

    struct CollectingInteger;

    impl AltrepClass for CollectingInteger {
        fn vector_type(&self) -> SEXPTYPE {
            SEXPTYPE::INTSXP
        }
        fn length(&self, _: &AltrepContext<'_>) -> SexpResult<R_xlen_t> {
            Ok(2)
        }
        fn element<'s>(
            &self,
            context: &AltrepContext<'s>,
            i: R_xlen_t,
        ) -> SexpResult<AltrepElement<'s>> {
            context.gc()?;
            Ok(AltrepElement::Integer([7, 9][i as usize]))
        }
    }

    #[test]
    fn apply_scalar_extraction_roots_destination_during_altrep_collection() {
        let session = RSession::new_for_gc_tests();
        let class = session
            .register_altrep_class("collecting-apply-integer", CollectingInteger)
            .unwrap();
        let input = AltrepBuilder::new(class).build().unwrap();
        session.with_active(|| {
            let out = session.sexp(extract_element(input.as_raw(), 1)).unwrap();
            assert_eq!(out.try_integer_elt(0), Ok(9));
            assert!(!is_materialized(&input));
        });
    }
}
