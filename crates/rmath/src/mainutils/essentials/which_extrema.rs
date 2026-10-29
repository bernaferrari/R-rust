//! GNU which.min/which.max with rooted coercion and comparison values.
use crate::sexp::{
    ffi::{NA_INTEGER, SEXP, SEXPTYPE},
    object::{SessionNodeFactory, Sexp, SexpMut},
    owner::OwnerToken,
};

pub(super) unsafe fn evaluate(call: SEXP, op: SEXP, args: SEXP, rho: SEXP, minimum: bool) -> SEXP {
    unsafe {
        let owner = OwnerToken::current().expect("active extrema owner");
        let pin = owner.pin().expect("live extrema owner");
        let factory = owner.node_factory();
        let arguments = factory
            .wrap(args)
            .expect("extrema arguments belong to original owner");
        let _call = (!call.is_null()).then(|| {
            factory
                .wrap(call)
                .expect("extrema call belongs to original owner")
        });
        let _operation = (!op.is_null()).then(|| {
            factory
                .wrap(op)
                .expect("extrema operation belongs to original owner")
        });
        let environment = (!rho.is_null()).then(|| {
            factory
                .wrap(rho)
                .expect("extrema environment belongs to original owner")
        });
        let check = || {
            if let Some(pin) = &pin {
                pin.require_live()
                    .expect("live extrema owner after callback");
            }
            factory
                .require_active()
                .expect("original extrema owner after callback");
        };
        let source = arguments.try_car().expect("extrema input");
        let mut input = source.clone();
        if input.is_object() && !is_data_frame(&factory, &input, &check) {
            let symbol = crate::sexp::symbol::Rf_install(c"xtfrm".as_ptr());
            check();
            let function = factory.wrap(symbol).expect("root xtfrm symbol");
            let nil = factory.nil();
            let arguments = factory
                .pairlist_cell(&input, &nil, &nil)
                .expect("root xtfrm argument");
            let expression = factory
                .call(&function, &arguments)
                .expect("root xtfrm call");
            let environment = environment
                .as_ref()
                .expect("xtfrm needs the original environment");
            let transformed = crate::eval::eval::Rf_eval(expression.as_raw(), environment.as_raw());
            check();
            input = factory
                .wrap(transformed)
                .expect("xtfrm result belongs to original owner");
        } else if ![SEXPTYPE::LGLSXP, SEXPTYPE::INTSXP, SEXPTYPE::REALSXP]
            .contains(&input.typeof_())
        {
            let converted =
                crate::mainutils::coerce::coerceVector(input.as_raw(), SEXPTYPE::REALSXP.into());
            check();
            input = factory
                .wrap(converted)
                .expect("coerced extrema input belongs to original owner");
        }
        let mut best: Option<(i64, f64)> = None;
        for i in 0..input.len() {
            let value = match input.typeof_() {
                t if t == SEXPTYPE::LGLSXP => {
                    let n = input.try_logical_elt(i).expect("read extrema logical");
                    if n == NA_INTEGER {
                        f64::NAN
                    } else {
                        f64::from(n)
                    }
                }
                t if t == SEXPTYPE::INTSXP => {
                    let n = input.try_integer_elt(i).expect("read extrema integer");
                    if n == NA_INTEGER {
                        f64::NAN
                    } else {
                        f64::from(n)
                    }
                }
                t if t == SEXPTYPE::REALSXP => input.try_real_elt(i).expect("read extrema real"),
                _ => break,
            };
            check();
            if value.is_nan() {
                continue;
            }
            if best.is_none_or(|(_, old)| if minimum { value < old } else { value > old }) {
                best = Some((i, value));
            }
        }
        let output = index_result(&factory, best.map(|(i, _)| i));
        check();
        if let Some((index, _)) = best {
            use crate::sexp::attrib_core::{R_NamesSymbol, getAttrib, setAttrib};
            let names = factory
                .wrap(getAttrib(input.as_raw(), R_NamesSymbol()))
                .expect("root extrema names");
            check();
            if !names.is_nil() {
                let selected = names
                    .try_string_elt(index)
                    .expect("read selected extrema name");
                check();
                let scalar = factory
                    .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, 1)))
                    .expect("allocate extrema name");
                check();
                let mut scalar = SexpMut::try_from_checked(scalar).expect("mutable extrema name");
                scalar
                    .try_set_string_elt(0, selected)
                    .expect("write extrema name");
                let scalar = scalar.freeze();
                setAttrib(output.as_raw(), R_NamesSymbol(), scalar.as_raw());
                check();
            }
        }
        output.as_raw()
    }
}

unsafe fn is_data_frame(
    factory: &SessionNodeFactory<'_>,
    input: &Sexp<'_>,
    check: impl Fn(),
) -> bool {
    unsafe {
        let class = factory
            .wrap(crate::sexp::attrib_core::getAttrib(
                input.as_raw(),
                crate::sexp::attrib_core::R_ClassSymbol(),
            ))
            .expect("root extrema class");
        check();
        if class.typeof_() != SEXPTYPE::STRSXP {
            return false;
        }
        for i in 0..class.len() {
            let name = class
                .try_string_elt(i)
                .and_then(|x| x.try_as_string())
                .expect("read extrema class");
            check();
            if name == "data.frame" {
                return true;
            }
        }
        false
    }
}

fn index_result<'s>(factory: &SessionNodeFactory<'s>, index: Option<i64>) -> Sexp<'s> {
    let large = index.is_some_and(|i| i >= i64::from(i32::MAX));
    let kind = if large {
        SEXPTYPE::REALSXP
    } else {
        SEXPTYPE::INTSXP
    };
    let output = factory
        .allocate(|arena| Some(arena.alloc_vector(kind, i64::from(index.is_some()))))
        .expect("allocate extrema index");
    factory
        .require_active()
        .expect("original extrema owner after index allocation");
    let mut output = SexpMut::try_from_checked(output).expect("mutable extrema index");
    if let Some(index) = index {
        if large {
            output
                .try_set_real_elt(0, (index + 1) as f64)
                .expect("write long extrema index");
        } else {
            output
                .try_set_integer_elt(0, (index + 1) as i32)
                .expect("write extrema index");
        }
    }
    output.freeze()
}

#[cfg(test)]
mod tests;
