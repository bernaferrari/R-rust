//! GNU's integer tabulation kernel; the original R wrapper owns public coercion.
use crate::sexp::{
    ffi::{NA_INTEGER, SEXP, SEXPTYPE},
    object::SexpMut,
    owner::OwnerToken,
};

pub(super) unsafe fn evaluate(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let owner = OwnerToken::current().expect("active tabulate owner");
        let pin = owner.pin().expect("live tabulate owner");
        let factory = owner.node_factory();
        let arguments = factory
            .wrap(args)
            .expect("tabulate arguments belong to original owner");
        let _call = (!call.is_null()).then(|| {
            factory
                .wrap(call)
                .expect("tabulate call belongs to original owner")
        });
        let _op = (!op.is_null()).then(|| {
            factory
                .wrap(op)
                .expect("tabulate operation belongs to original owner")
        });
        let _env = (!rho.is_null()).then(|| {
            factory
                .wrap(rho)
                .expect("tabulate environment belongs to original owner")
        });
        let check = || {
            if let Some(pin) = &pin {
                pin.require_live()
                    .expect("live tabulate owner after callback");
            }
            factory
                .require_active()
                .expect("original tabulate owner after callback");
        };
        // The fallback descriptor does not specify this internal arity.
        // Check the real GNU kernel contract before reading either argument.
        let mut cursor = arguments.clone();
        let mut actual = 0;
        while !cursor.is_nil() {
            actual += 1;
            cursor = cursor.try_cdr().unwrap_or_else(|_| {
                crate::sexp::context::r_error("invalid tabulate argument list")
            });
        }
        if actual != 2 {
            let noun = if actual == 1 { "argument" } else { "arguments" };
            crate::sexp::context::r_error(format!(
                "{actual} {noun} passed to .Internal(tabulate) which requires 2"
            ));
        }
        let input = arguments.try_car().expect("tabulate input");
        if input.typeof_() != SEXPTYPE::INTSXP {
            crate::sexp::context::r_error("invalid input");
        }
        let size = arguments
            .try_cdr()
            .and_then(|tail| tail.try_car())
            .expect("tabulate nbin");
        let nbins = crate::mainutils::coerce::asInteger(size.as_raw());
        check();
        if nbins == NA_INTEGER || nbins < 0 {
            crate::sexp::context::r_error("invalid 'nbin' argument");
        }
        let long = input.len() > i64::from(i32::MAX);
        let kind = if long {
            SEXPTYPE::REALSXP
        } else {
            SEXPTYPE::INTSXP
        };
        // Retain constructor admission, collection/retry, and resource errors.
        // There is no temporary vector proportional to input or output length.
        let output = crate::sexp::constructors::Rf_allocVector3(kind, i64::from(nbins));
        check();
        let output = factory.wrap(output).expect("root admitted tabulate result");
        let mut output = SexpMut::try_from_checked(output).expect("mutable tabulate result");
        for i in 0..i64::from(nbins) {
            if long {
                output
                    .try_set_real_elt(i, 0.)
                    .expect("initialize long tabulate count");
            } else {
                output
                    .try_set_integer_elt(i, 0)
                    .expect("initialize tabulate count");
            }
        }
        // A zero-bin table needs no provider reads or materialized input cache.
        if nbins > 0 {
            for i in 0..input.len() {
                let bin = input.try_integer_elt(i).expect("read tabulate bin");
                check();
                if bin == NA_INTEGER || bin <= 0 || bin > nbins {
                    continue;
                }
                let index = i64::from(bin - 1);
                if long {
                    let count = output
                        .try_real_elt(index)
                        .expect("read long tabulate count");
                    output
                        .try_set_real_elt(index, count + 1.)
                        .expect("increment long tabulate count");
                } else {
                    let count = output.integer_elt(index).expect("read tabulate count");
                    output
                        .try_set_integer_elt(
                            index,
                            count
                                .checked_add(1)
                                .expect("short tabulate count fits input length"),
                        )
                        .expect("increment tabulate count");
                }
            }
        }
        check();
        output.freeze().as_raw()
    }
}

#[cfg(test)]
mod tests;
