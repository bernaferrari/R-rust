//! Logic operators, builtin arglist forcing, setter/getter calls — extracted verbatim from eval/bc_eval.rs

use super::*;

/// GNU AND/OR/NOT reuse the same primitive as interpreted `&` / `|` / `!`.
pub(crate) unsafe fn eval_gnu_logic(
    call: SEXP,
    symbol: &std::ffi::CStr,
    left: SEXP,
    right: Option<SEXP>,
    rho: SEXP,
) -> SEXP {
    unsafe {
        let op = super::super::primitive::make_primitive_binding(
            symbol.to_str().unwrap_or(""),
            SEXPTYPE::BUILTINSXP,
        );
        if let Some(right) = right {
            let tail = Rf_cons(right, R_NilValue());
            let _tail = own_operand(tail);
            let args = Rf_cons(left, tail);
            let _args = own_operand(args);
            if let Some(result) =
                super::super::arithmetic::try_ops_group_dispatch(call, op, args, rho)
            {
                return result;
            }
            crate::mainutils::logic::do_logic(call, op, args, rho)
        } else {
            let args = Rf_cons(left, R_NilValue());
            let _args = own_operand(args);
            if let Some(result) =
                super::super::arithmetic::try_ops_group_dispatch(call, op, args, rho)
            {
                return result;
            }
            crate::mainutils::logic::do_logic(call, op, args, rho)
        }
    }
}

/// GNU bytecode wraps PUSHCONSTARG in already-forced promises for CLOSXP
/// calls. Builtin handlers take values. MAKEPROM args (e.g. `attr(x, n) <-`)
/// are unforced; GNU SPECIAL `attr<-` evaluates them, and a BUILTIN port
/// must force them before `apply_builtin_values_safe`.
pub(crate) unsafe fn force_gnu_builtin_arglist(mut args: SEXP) {
    unsafe {
        while !args.is_null() && args != R_NilValue() {
            let car = CAR(args);
            if TYPEOF(car) == SEXPTYPE::PROMSXP {
                let value = forcePromise(car);
                crate::sexp::accessors::SETCAR(args, value);
            }
            args = CDR(args);
        }
    }
}

/// GNU SETTER_CALL matches eval.c: replace the first call argument with lhs,
/// append rhs tagged `value`, and apply the replacement function.
pub(crate) unsafe fn eval_gnu_setter_call(
    fun: SEXP,
    call: SEXP,
    vexpr: SEXP,
    lhs: SEXP,
    rhs: SEXP,
    frame_args: SEXP,
    rho: SEXP,
) -> SEXP {
    unsafe {
        use crate::sexp::object::Sexp;
        let value_sym = crate::sexp::symbol::Rf_install(c"value".as_ptr());
        let tmp_sym = crate::sexp::symbol::Rf_install(c"*tmp*".as_ptr());
        match TYPEOF(fun) {
            kind if kind == SEXPTYPE::BUILTINSXP => {
                if frame_args == R_NilValue() {
                    bc_error("GNU SETTER_CALL has no first argument");
                }
                crate::sexp::accessors::SETCAR(frame_args, lhs);
                let mut last = frame_args;
                while CDR(last) != R_NilValue() {
                    last = CDR(last);
                }
                let value_owned = own_operand(Rf_cons(rhs, R_NilValue()));
                let value = value_owned.as_raw();
                crate::sexp::accessors::SETTAG(value, value_sym);
                crate::sexp::accessors::SETCDR(last, value);
                // PUSHCONSTARG stores forced promises so CLOSXP calls keep
                // literals. Builtin handlers take values (GNU PUSHCONSTARG).
                force_gnu_builtin_arglist(frame_args);
                super::super::apply::apply_builtin_values_safe(
                    Sexp::from_raw_unchecked(fun),
                    Sexp::from_raw_unchecked(call),
                    Sexp::from_raw_unchecked(frame_args),
                    Sexp::from_raw_unchecked(rho),
                )
                .unwrap_or_else(|error| bc_error(error))
                .as_raw()
            }
            kind if kind == SEXPTYPE::SPECIALSXP => {
                let args = crate::mainutils::duplicate::duplicate(CDR(call));
                let _args = own_operand(args);
                if args == R_NilValue() {
                    bc_error("GNU SETTER_CALL special has no arguments");
                }
                let lhs_prom = crate::sexp::memory_ext::R_mkEVPROMISE(tmp_sym, lhs);
                crate::sexp::accessors::SETCAR(args, lhs_prom);
                let mut last = args;
                while CDR(last) != R_NilValue() {
                    last = CDR(last);
                }
                let rhs_prom = crate::sexp::memory_ext::R_mkEVPROMISE(vexpr, rhs);
                crate::sexp::accessors::SETCAR(last, rhs_prom);
                super::super::apply::apply_special_safe(
                    Sexp::from_raw_unchecked(fun),
                    Sexp::from_raw_unchecked(call),
                    Sexp::from_raw_unchecked(args),
                    Sexp::from_raw_unchecked(rho),
                )
                .unwrap_or_else(|error| bc_error(error))
                .as_raw()
            }
            kind if kind == SEXPTYPE::CLOSXP => {
                if frame_args == R_NilValue() {
                    bc_error("GNU SETTER_CALL has no first argument");
                }
                let lhs_prom = crate::sexp::memory_ext::R_mkEVPROMISE(tmp_sym, lhs);
                crate::sexp::accessors::SETCAR(frame_args, lhs_prom);
                let mut last = frame_args;
                while CDR(last) != R_NilValue() {
                    last = CDR(last);
                }
                let rhs_prom = crate::sexp::memory_ext::R_mkEVPROMISE(vexpr, rhs);
                let value = Rf_cons(rhs_prom, R_NilValue());
                crate::sexp::accessors::SETTAG(value, value_sym);
                crate::sexp::accessors::SETCDR(last, value);
                super::super::apply::apply_closure_safe(
                    Sexp::from_raw_unchecked(fun),
                    Sexp::from_raw_unchecked(call),
                    Sexp::from_raw_unchecked(frame_args),
                    Sexp::from_raw_unchecked(rho),
                )
                .unwrap_or_else(|error| bc_error(error))
                .as_raw()
            }
            _ => bc_error("GNU SETTER_CALL found a non-function"),
        }
    }
}

/// GNU GETTER_CALL matches eval.c: replace the first call argument with lhs
/// and apply the getter, leaving lhs/rhs below the result for SWAP.
pub(crate) unsafe fn eval_gnu_getter_call(
    fun: SEXP,
    call: SEXP,
    lhs: SEXP,
    frame_args: SEXP,
    rho: SEXP,
) -> SEXP {
    unsafe {
        use crate::sexp::object::Sexp;
        let tmp_sym = crate::sexp::symbol::Rf_install(c"*tmp*".as_ptr());
        match TYPEOF(fun) {
            kind if kind == SEXPTYPE::BUILTINSXP => {
                if frame_args == R_NilValue() {
                    bc_error("GNU GETTER_CALL has no first argument");
                }
                crate::sexp::accessors::SETCAR(frame_args, lhs);
                force_gnu_builtin_arglist(frame_args);
                super::super::apply::apply_builtin_values_safe(
                    Sexp::from_raw_unchecked(fun),
                    Sexp::from_raw_unchecked(call),
                    Sexp::from_raw_unchecked(frame_args),
                    Sexp::from_raw_unchecked(rho),
                )
                .unwrap_or_else(|error| bc_error(error))
                .as_raw()
            }

            kind if kind == SEXPTYPE::SPECIALSXP => {
                let args = crate::mainutils::duplicate::duplicate(CDR(call));
                let _args = own_operand(args);
                if args == R_NilValue() {
                    bc_error("GNU GETTER_CALL special has no arguments");
                }
                let lhs_prom = crate::sexp::memory_ext::R_mkEVPROMISE(tmp_sym, lhs);
                crate::sexp::accessors::SETCAR(args, lhs_prom);
                super::super::apply::apply_special_safe(
                    Sexp::from_raw_unchecked(fun),
                    Sexp::from_raw_unchecked(call),
                    Sexp::from_raw_unchecked(args),
                    Sexp::from_raw_unchecked(rho),
                )
                .unwrap_or_else(|error| bc_error(error))
                .as_raw()
            }
            kind if kind == SEXPTYPE::CLOSXP => {
                if frame_args == R_NilValue() {
                    bc_error("GNU GETTER_CALL has no first argument");
                }
                let lhs_prom = crate::sexp::memory_ext::R_mkEVPROMISE(tmp_sym, lhs);
                crate::sexp::accessors::SETCAR(frame_args, lhs_prom);
                super::super::apply::apply_closure_safe(
                    Sexp::from_raw_unchecked(fun),
                    Sexp::from_raw_unchecked(call),
                    Sexp::from_raw_unchecked(frame_args),
                    Sexp::from_raw_unchecked(rho),
                )
                .unwrap_or_else(|error| bc_error(error))
                .as_raw()
            }
            _ => bc_error("GNU GETTER_CALL found a non-function"),
        }
    }
}
