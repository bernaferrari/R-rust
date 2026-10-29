//! GNU default Boolean summaries, with bounded owning argument selection.
use crate::sexp::{
    ffi::{NA_INTEGER, SEXP, SEXPTYPE},
    object::Sexp,
    owner::OwnerToken,
};
mod coercion;
mod input;

fn fail(message: &str) -> ! {
    crate::sexp::context::r_error(message)
}

pub(super) unsafe fn run(call: SEXP, op: SEXP, args: SEXP, rho: SEXP, any: bool) -> SEXP {
    unsafe {
        let owner = OwnerToken::current().expect("active Boolean summary owner");
        let pin = owner.pin().expect("live Boolean summary owner");
        let f = owner.node_factory();
        let arguments = f.wrap(args).expect("Boolean summary argument owner");
        let _call = (!call.is_null()).then(|| f.wrap(call).expect("Boolean summary call owner"));
        let operation =
            (!op.is_null()).then(|| f.wrap(op).expect("Boolean summary operation owner"));
        let _rho =
            (!rho.is_null()).then(|| f.wrap(rho).expect("Boolean summary environment owner"));
        let check = || {
            if let Some(pin) = &pin {
                pin.require_live()
                    .expect("live Boolean summary owner after callback");
            }
            f.require_active()
                .expect("original Boolean summary owner after callback");
        };
        let selected = input::Arguments::new(&arguments, &owner);
        // Keep the existing Summary generic. The default's owning selection
        // cannot be detached by dispatch, flag, warning, or provider callbacks.
        if operation.as_ref().is_some_and(|op| !op.is_nil()) {
            let mut result = crate::sexp::globals::R_NilValue();
            let dispatched = crate::eval::dispatch::DispatchGroup(
                c"Summary".as_ptr(),
                call,
                op,
                args,
                rho,
                &mut result,
            ) != 0;
            check();
            if dispatched {
                return f
                    .wrap(result)
                    .expect("Boolean Summary method result owner")
                    .as_raw();
            }
        }
        let na_rm = if let Some(flag) = selected.values.iter().find(|v| v.flag) {
            let value = coercion::scalar(&flag.value, true, &check);
            check();
            value != 0 // GNU's int narm also treats NA_LOGICAL as nonzero.
        } else {
            false
        };
        let answer = reduce(&selected, &f, call, any, na_rm, &check);
        check();
        let result = crate::sexp::constructors::Rf_ScalarLogical(answer);
        check();
        f.wrap(result)
            .expect("Boolean summary scalar result owner")
            .as_raw()
    }
}

fn reduce(
    selected: &input::Arguments<'_>,
    f: &crate::sexp::object::SessionNodeFactory<'_>,
    call: SEXP,
    any: bool,
    na_rm: bool,
    check: &impl Fn(),
) -> i32 {
    let mut missing = false;
    for entry in &selected.values {
        if entry.flag {
            continue;
        }
        let value = &entry.value;
        let n = coercion::length(value);
        check();
        if n == 0 {
            continue;
        } // GNU avoids warnings/coercion for empty inputs.
        let logical: Sexp<'_> = if value.typeof_() == SEXPTYPE::LGLSXP {
            value.clone()
        } else {
            if value.typeof_() != SEXPTYPE::INTSXP {
                unsafe {
                    crate::mainutils::errors::Rf_warningcall1(
                        call,
                        coercion::warning(value.typeof_()).as_ptr(),
                    );
                }
                check();
            }
            coercion::logical(value, f, check)
        };
        for i in 0..logical.len() {
            let value = logical
                .try_logical_elt(i)
                .expect("Boolean summary copied logical element");
            check();
            if value == NA_INTEGER {
                if !na_rm {
                    missing = true;
                }
            } else if value == if any { 1 } else { 0 } {
                return if any { 1 } else { 0 };
            }
        }
    }
    if missing {
        NA_INTEGER
    } else if any {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests;
