//! Shared checked mapping for GNU's finite/infinite/NaN predicates.
use crate::sexp::{
    ffi::{NA_INTEGER, SEXP, SEXPTYPE},
    object::{SessionNodeFactory, Sexp, SexpMut},
    owner::OwnerToken,
};

#[derive(Clone, Copy)]
pub(super) enum Predicate {
    Finite,
    Infinite,
    Nan,
}

impl Predicate {
    fn name(self) -> &'static [u8] {
        match self {
            Self::Finite => b"is.finite\0",
            Self::Infinite => b"is.infinite\0",
            Self::Nan => b"is.nan\0",
        }
    }
    fn real(self, value: f64) -> bool {
        match self {
            Self::Finite => value.is_finite(),
            Self::Infinite => value.is_infinite(),
            Self::Nan => value.is_nan() && !crate::sexp::ffi::is_na_real(value),
        }
    }
}

/// Root and pin the original invocation before dispatch or ALTREP callbacks.
/// No payload span or runtime borrow is held while a provider executes.
pub(super) unsafe fn evaluate(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
    kind: Predicate,
) -> SEXP {
    unsafe {
        let owner = OwnerToken::current().expect("active predicate owner");
        let pin = owner.pin().expect("live predicate owner");
        let factory = owner.node_factory();
        let _arguments = factory
            .wrap(args)
            .expect("predicate arguments belong to original owner");
        let _call = (!call.is_null()).then(|| {
            factory
                .wrap(call)
                .expect("predicate call belongs to original owner")
        });
        let _operation = (!op.is_null()).then(|| {
            factory
                .wrap(op)
                .expect("predicate operation belongs to original owner")
        });
        let _environment = (!rho.is_null()).then(|| {
            factory
                .wrap(rho)
                .expect("predicate environment belongs to original owner")
        });
        let check = || {
            if let Some(pin) = &pin {
                pin.require_live()
                    .expect("live predicate owner after callback");
            }
            factory
                .require_active()
                .expect("original predicate owner after callback");
        };
        let dispatched = super::vectors::dispatch_is(call, op, args, rho, kind.name());
        check();
        if let Some(value) = dispatched {
            return factory
                .wrap(value)
                .expect("predicate dispatch result belongs to original owner")
                .as_raw();
        }
        let input = factory
            .wrap(crate::sexp::accessors::CAR(args))
            .expect("predicate input belongs to original owner");
        let t = input.typeof_();
        if ![
            SEXPTYPE::NILSXP,
            SEXPTYPE::LGLSXP,
            SEXPTYPE::INTSXP,
            SEXPTYPE::REALSXP,
            SEXPTYPE::CPLXSXP,
            SEXPTYPE::STRSXP,
            SEXPTYPE::RAWSXP,
        ]
        .contains(&t)
        {
            let name = if t == SEXPTYPE::OBJSXP
                && input.header().sxpinfo.gp() & crate::sexp::accessors::S4_OBJECT_MASK == 0
            {
                std::borrow::Cow::Borrowed("object")
            } else {
                std::ffi::CStr::from_ptr(crate::mainutils::util_main::type2char(t.into()))
                    .to_string_lossy()
            };
            crate::mainutils::errors::errorcall_str(
                call,
                &format!("default method not implemented for type '{name}'"),
            );
        }
        let length = input.len();
        let output = factory
            .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::LGLSXP, length)))
            .expect("allocate predicate result");
        check();
        let shape = if matches!(kind, Predicate::Nan) {
            None
        } else {
            Some(snapshot_shape(&factory, &input, &check))
        };
        let mut output = SexpMut::try_from_checked(output).expect("mutable predicate result");
        for i in 0..length {
            let value = if t == SEXPTYPE::REALSXP {
                kind.real(input.try_real_elt(i).expect("read predicate real element"))
            } else if t == SEXPTYPE::INTSXP {
                let value = input
                    .try_integer_elt(i)
                    .expect("read predicate integer element");
                matches!(kind, Predicate::Finite) && value != NA_INTEGER
            } else if t == SEXPTYPE::LGLSXP {
                let value = input
                    .try_logical_elt(i)
                    .expect("read predicate logical element");
                matches!(kind, Predicate::Finite) && value != NA_INTEGER
            } else if t == SEXPTYPE::CPLXSXP {
                let value = input
                    .try_complex_elt(i)
                    .expect("read predicate complex element");
                if matches!(kind, Predicate::Finite) {
                    kind.real(value.r) && kind.real(value.i)
                } else {
                    kind.real(value.r) || kind.real(value.i)
                }
            } else {
                false
            };
            check();
            output
                .try_set_logical_elt(i, i32::from(value))
                .expect("write predicate result element");
        }
        let output = output.freeze();
        let (dims, names) = shape.unwrap_or_else(|| snapshot_shape(&factory, &input, &check));
        apply_shape(&output, &dims, &names, check);
        output.as_raw()
    }
}

unsafe fn snapshot_shape<'s>(
    factory: &SessionNodeFactory<'s>,
    input: &Sexp<'_>,
    check: impl Fn(),
) -> (Sexp<'s>, Sexp<'s>) {
    unsafe {
        use crate::sexp::attrib_core::{R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, getAttrib};
        let dims = factory
            .wrap(getAttrib(input.as_raw(), R_DimSymbol()))
            .expect("root predicate dimensions");
        check();
        let symbol = if dims.is_nil() {
            R_NamesSymbol()
        } else {
            R_DimNamesSymbol()
        };
        let names = factory
            .wrap(getAttrib(input.as_raw(), symbol))
            .expect("root predicate names");
        check();
        (dims, names)
    }
}

unsafe fn apply_shape(output: &Sexp<'_>, dims: &Sexp<'_>, names: &Sexp<'_>, check: impl Fn()) {
    unsafe {
        use crate::sexp::attrib_core::{R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, setAttrib};
        if !dims.is_nil() {
            setAttrib(output.as_raw(), R_DimSymbol(), dims.as_raw());
            check();
        }
        if !names.is_nil() {
            let symbol = if dims.is_nil() {
                R_NamesSymbol()
            } else {
                R_DimNamesSymbol()
            };
            setAttrib(output.as_raw(), symbol, names.as_raw());
            check();
        }
    }
}

#[cfg(test)]
mod tests;
