//! Selected upstream base-R wrappers run by the ordinary evaluator.
//! Cached closures are preserved for the owning session's lifetime.
use crate::sexp::{
    accessors::*,
    constructors::Rf_cons,
    ffi::{SEXP, SEXPTYPE},
    globals::{R_BaseEnv, R_NilValue},
    instance::with_required_current_instance,
    protect::protect,
};

pub(crate) unsafe fn apply(
    name: &'static str,
    source: &str,
    args: SEXP,
    rho: SEXP,
    evaluated: bool,
) -> SEXP {
    unsafe {
        let cached = with_required_current_instance(|inst| {
            (*inst).base_wrappers.borrow().get(name).copied()
        });
        let fun = cached.unwrap_or_else(|| {
            let parsed =
                crate::sexp::memory::with_arena(|arena| crate::eval::parser::parse(source, arena))
                    .expect("checked-in base wrapper must parse");
            let _parsed = protect(parsed);
            let fun = crate::eval::eval::Rf_eval(parsed, R_BaseEnv());
            let _fun = protect(fun);
            crate::sexp::protect::R_PreserveObject(fun);
            with_required_current_instance(|inst| {
                (*inst).base_wrappers.borrow_mut().insert(name, fun);
            });
            fun
        });
        let mut call_args = args;
        let mut guards = Vec::new();
        if evaluated {
            let mut cells = Vec::new();
            let mut p = args;
            while p != R_NilValue() && !p.is_null() {
                cells.push((CAR(p), TAG(p)));
                p = CDR(p);
            }
            call_args = R_NilValue();
            for (value, tag) in cells.into_iter().rev() {
                let promise = crate::sexp::memory_ext::mkPROMSXP(value, rho);
                let _promise = protect(promise);
                SET_PRVALUE(promise, value);
                let cell = Rf_cons(promise, call_args);
                guards.push(protect(cell));
                SETTAG(cell, tag);
                call_args = cell;
            }
        }
        let call = Rf_cons(fun, call_args);
        let _call = protect(call);
        (*call).sxpinfo.set_type(SEXPTYPE::LANGSXP);
        crate::eval::eval::Rf_eval(call, rho)
    }
}
