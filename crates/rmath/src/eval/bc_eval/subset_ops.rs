//! GNU subset/subassign operators and lexical generics — extracted verbatim from eval/bc_eval.rs

use super::*;

/// GNU STARTSUBSET_N / STARTSUBSET2_N match eval.c DO_STARTDISPATCH_N.
/// Object methods jump to the label; the default path leaves x on the stack.
pub(crate) unsafe fn eval_gnu_startsubset_n(
    generic: &std::ffi::CStr,
    call: SEXP,
    x: SEXP,
    rho: SEXP,
) -> Option<SEXP> {
    unsafe {
        if crate::sexp::accessors::OBJECT(x) == 0
            || call.is_null()
            || TYPEOF(call) != SEXPTYPE::LANGSXP
        {
            return None;
        }
        let mut value = R_NilValue();
        if super::super::missing::tryDispatch(generic.as_ptr() as *mut _, call, x, rho, &mut value)
            != 0
        {
            Some(value)
        } else {
            None
        }
    }
}

/// GNU compiled subset defaults honor lexical scoping: when the running
/// closure's environment shadows `[` / `[[` with a function other than the
/// base binding, that function receives the call with the arguments the
/// bytecode already evaluated (upstream keeps compiled subsetting lexically
/// scoped like every other closure call). Returns the shadow's result, or
/// `None` when the generic still resolves to the base primitive and the
/// default machinery should run.
pub(crate) unsafe fn eval_gnu_lexical_generic(
    generic: &std::ffi::CStr,
    call: SEXP,
    args: SEXP,
    rho: SEXP,
) -> Option<SEXP> {
    unsafe {
        let symbol = crate::sexp::symbol::Rf_install(generic.as_ptr());
        let lexical = crate::sexp::envir::findFun(symbol, rho);
        if lexical.is_null()
            || lexical == R_UnboundValue()
            || call.is_null()
            || TYPEOF(call) != SEXPTYPE::LANGSXP
        {
            return None;
        }
        let base_binding =
            crate::sexp::envir::R_findVarInFrame(super::super::runtime::base_env(), symbol);
        if base_binding.is_null() || base_binding == R_UnboundValue() || lexical == base_binding {
            return None;
        }
        let function = Sexp::from_raw_unchecked(lexical);
        let call_s = Sexp::from_raw_unchecked(call);
        let args_s = Sexp::from_raw_unchecked(args);
        let env_s = Sexp::from_raw_unchecked(rho);
        match TYPEOF(lexical) {
            kind if kind == SEXPTYPE::CLOSXP => Some(
                super::super::apply::apply_closure_safe(function, call_s, args_s, env_s)
                    .unwrap_or_else(|error| bc_error(error))
                    .as_raw(),
            ),
            kind if kind == SEXPTYPE::BUILTINSXP => {
                force_gnu_builtin_arglist(args);
                Some(
                    super::super::apply::apply_builtin_values_safe(function, call_s, args_s, env_s)
                        .unwrap_or_else(|error| bc_error(error))
                        .as_raw(),
                )
            }
            _ => None,
        }
    }
}

/// GNU STARTSUBASSIGN_N matches eval.c DO_START_ASSIGN_DISPATCH_N.
pub(crate) unsafe fn eval_gnu_startsubassign_n(
    generic: &std::ffi::CStr,
    call: SEXP,
    lhs: SEXP,
    rhs: SEXP,
    rho: SEXP,
) -> Option<SEXP> {
    unsafe {
        if crate::sexp::accessors::OBJECT(lhs) == 0
            || call.is_null()
            || TYPEOF(call) != SEXPTYPE::LANGSXP
        {
            return None;
        }
        let mut value = R_NilValue();
        if super::super::missing::tryAssignDispatch(
            generic.as_ptr() as *mut _,
            call,
            lhs,
            rhs,
            rho,
            &mut value,
        ) != 0
        {
            Some(value)
        } else {
            None
        }
    }
}

/// GNU VECSUBSET / VECSUBSET2 fall through to do_subset(_2)_dflt, honoring a
/// lexical shadow of the generic first.
pub(crate) unsafe fn eval_gnu_vecsubset(
    call: SEXP,
    x: SEXP,
    index: SEXP,
    rho: SEXP,
    subset2: bool,
) -> SEXP {
    unsafe {
        let args = Rf_cons(x, Rf_cons(index, R_NilValue()));
        let _args = own_operand(args);
        let generic = if subset2 { c"[[" } else { c"[" };
        if let Some(shadowed) = eval_gnu_lexical_generic(generic, call, args, rho) {
            return shadowed;
        }
        if subset2 {
            crate::mainutils::subset::do_subset2_dflt(
                call,
                crate::sexp::symbol::Rf_install(c"[[".as_ptr()),
                args,
                rho,
            )
        } else if TYPEOF(x) == SEXPTYPE::VECSXP
            && crate::mainutils::essentials::sexp_has_class(x, "POSIXlt")
        {
            // POSIXlt subsetting needs the full `[` path (S3 semantics,
            // component recycling, balanced bookkeeping).
            crate::mainutils::subset::do_subset(
                call,
                crate::sexp::symbol::Rf_install(c"[".as_ptr()),
                args,
                rho,
            )
        } else {
            crate::mainutils::subset::subset_default_after_failed_dispatch(
                call,
                crate::sexp::symbol::Rf_install(c"[".as_ptr()),
                args,
                rho,
            )
        }
    }
}

/// GNU MATSUBSET / SUBSET_N fall through to the default subset helpers,
/// honoring a lexical shadow of the generic first.
pub(crate) unsafe fn eval_gnu_subset_indices(
    call: SEXP,
    x: SEXP,
    indices: &[SEXP],
    rho: SEXP,
    subset2: bool,
) -> SEXP {
    unsafe {
        let _x = own_operand(x);
        let _index_roots = indices
            .iter()
            .map(|index| own_operand(*index))
            .collect::<Vec<_>>();
        let mut args = R_NilValue();
        let mut roots = Vec::with_capacity(indices.len() + 1);
        for &index in indices.iter().rev() {
            args = Rf_cons(index, args);
            roots.push(own_operand(args));
        }
        args = Rf_cons(x, args);
        let _args = own_operand(args);
        let _ = roots;
        let generic = if subset2 { c"[[" } else { c"[" };
        if let Some(shadowed) = eval_gnu_lexical_generic(generic, call, args, rho) {
            return shadowed;
        }
        if subset2 {
            crate::mainutils::subset::do_subset2_dflt(
                call,
                crate::sexp::symbol::Rf_install(c"[[".as_ptr()),
                args,
                rho,
            )
        } else {
            crate::mainutils::subset::subset_default_after_failed_dispatch(
                call,
                crate::sexp::symbol::Rf_install(c"[".as_ptr()),
                args,
                rho,
            )
        }
    }
}

/// GNU VECSUBASSIGN / VECSUBASSIGN2 fall through to do_subassign(_2)_dflt.
pub(crate) unsafe fn eval_gnu_vecsubassign(
    call: SEXP,
    x: SEXP,
    rhs: SEXP,
    index: SEXP,
    rho: SEXP,
    subset2: bool,
) -> SEXP {
    unsafe {
        let mut x = x;
        if crate::sexp::accessors::NAMED(x) > 1 {
            x = crate::mainutils::duplicate::shallow_duplicate(x);
        }
        let _x = own_operand(x);
        let rhs = crate::mainutils::subset::factor_replacement_codes(x, rhs, call);
        let value_owned = own_operand(Rf_cons(rhs, R_NilValue()));
        let value = value_owned.as_raw();
        crate::sexp::accessors::SETTAG(value, crate::sexp::symbol::Rf_install(c"value".as_ptr()));
        let args = Rf_cons(x, Rf_cons(index, value));
        let _args = own_operand(args);
        if subset2 {
            crate::mainutils::subassign::do_subassign2_dflt(
                call,
                crate::sexp::symbol::Rf_install(c"[[<-".as_ptr()),
                args,
                rho,
            )
        } else {
            crate::mainutils::subassign::do_subassign_dflt(
                call,
                crate::sexp::symbol::Rf_install(c"[<-".as_ptr()),
                args,
                rho,
            )
        }
    }
}

/// GNU MATSUBASSIGN / MATSUBASSIGN2 fall through to do_subassign(_2)_dflt.
pub(crate) unsafe fn eval_gnu_matsubassign(
    call: SEXP,
    x: SEXP,
    rhs: SEXP,
    row: SEXP,
    column: SEXP,
    rho: SEXP,
    subset2: bool,
) -> SEXP {
    unsafe {
        let mut x = x;
        if crate::sexp::accessors::NAMED(x) > 1 {
            x = crate::mainutils::duplicate::shallow_duplicate(x);
        }
        let _x = own_operand(x);
        let _rhs = own_operand(rhs);
        let _row = own_operand(row);
        let _column = own_operand(column);
        let value_owned = own_operand(Rf_cons(rhs, R_NilValue()));
        let value = value_owned.as_raw();
        crate::sexp::accessors::SETTAG(value, crate::sexp::symbol::Rf_install(c"value".as_ptr()));
        let args = Rf_cons(x, Rf_cons(row, Rf_cons(column, value)));
        let _args = own_operand(args);
        if subset2 {
            crate::mainutils::subassign::do_subassign2_dflt(
                call,
                crate::sexp::symbol::Rf_install(c"[[<-".as_ptr()),
                args,
                rho,
            )
        } else {
            crate::mainutils::subassign::do_subassign_dflt(
                call,
                crate::sexp::symbol::Rf_install(c"[<-".as_ptr()),
                args,
                rho,
            )
        }
    }
}

/// GNU SUBASSIGN_N / SUBASSIGN2_N fall through to do_subassign(_2)_dflt.
pub(crate) unsafe fn eval_gnu_subassign_indices(
    call: SEXP,
    x: SEXP,
    rhs: SEXP,
    indices: &[SEXP],
    rho: SEXP,
    subset2: bool,
) -> SEXP {
    unsafe {
        let mut x = x;
        if crate::sexp::accessors::NAMED(x) > 1 {
            x = crate::mainutils::duplicate::shallow_duplicate(x);
        }
        let _x = own_operand(x);
        let _rhs = own_operand(rhs);
        let _index_roots = indices
            .iter()
            .map(|index| own_operand(*index))
            .collect::<Vec<_>>();
        let value_owned = own_operand(Rf_cons(rhs, R_NilValue()));
        let value = value_owned.as_raw();
        crate::sexp::accessors::SETTAG(value, crate::sexp::symbol::Rf_install(c"value".as_ptr()));
        let mut args = value;
        let mut roots = Vec::with_capacity(indices.len() + 1);
        roots.push(own_operand(args));
        for &index in indices.iter().rev() {
            args = Rf_cons(index, args);
            roots.push(own_operand(args));
        }
        args = Rf_cons(x, args);
        let _args = own_operand(args);
        let _ = roots;
        if subset2 {
            crate::mainutils::subassign::do_subassign2_dflt(
                call,
                crate::sexp::symbol::Rf_install(c"[[<-".as_ptr()),
                args,
                rho,
            )
        } else {
            crate::mainutils::subassign::do_subassign_dflt(
                call,
                crate::sexp::symbol::Rf_install(c"[<-".as_ptr()),
                args,
                rho,
            )
        }
    }
}
