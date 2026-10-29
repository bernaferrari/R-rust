//! Admitted, rooted plain numeric pmin/pmax; classed/character paths stay legacy.
use crate::sexp::{
    ffi::{NA_INTEGER, NA_REAL, SEXP, SEXPTYPE},
    memory::WorkspaceReservation,
    object::{SessionNodeFactory, Sexp, SexpMut},
    owner::OwnerToken,
};
struct Input<'s> {
    value: Sexp<'s>,
    length: i64,
    kind: SEXPTYPE,
}
struct Inputs<'s> {
    values: Vec<Input<'s>>,
    _ticket: WorkspaceReservation,
}
unsafe fn flag(cell: &Sexp<'_>) -> bool {
    unsafe {
        let tag = cell.try_tag().expect("parallel extrema argument tag");
        if tag.is_nil() {
            return false;
        }
        let name = tag.try_printname().expect("parallel extrema tag name");
        std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(name.as_raw())).to_bytes() == b"na.rm"
    }
}
fn supported(value: &Sexp<'_>) -> bool {
    !value.is_object()
        && [
            SEXPTYPE::NILSXP,
            SEXPTYPE::LGLSXP,
            SEXPTYPE::INTSXP,
            SEXPTYPE::REALSXP,
            SEXPTYPE::CPLXSXP,
            SEXPTYPE::RAWSXP,
        ]
        .contains(&value.typeof_())
}
fn valid(kind: SEXPTYPE) -> bool {
    [
        SEXPTYPE::NILSXP,
        SEXPTYPE::LGLSXP,
        SEXPTYPE::INTSXP,
        SEXPTYPE::REALSXP,
    ]
    .contains(&kind)
}

pub(super) unsafe fn numeric(
    call: SEXP,
    op: SEXP,
    args: SEXP,
    rho: SEXP,
    minimum: bool,
    internal: bool,
) -> Option<SEXP> {
    unsafe {
        let owner = OwnerToken::current().expect("active parallel extrema owner");
        let pin = owner.pin().expect("live parallel extrema owner");
        let f = owner.node_factory();
        let arguments = f
            .wrap(args)
            .expect("parallel extrema arguments belong to original owner");
        let _call = (!call.is_null()).then(|| f.wrap(call).expect("parallel extrema call owner"));
        let operation =
            (!op.is_null()).then(|| f.wrap(op).expect("parallel extrema operation owner"));
        let _environment =
            (!rho.is_null()).then(|| f.wrap(rho).expect("parallel extrema environment owner"));
        let check = || {
            if let Some(pin) = &pin {
                pin.require_live()
                    .expect("live parallel extrema owner after callback");
            }
            f.require_active()
                .expect("original parallel extrema owner after callback");
        };
        let mut cursor = arguments.clone();
        let mut position = 0;
        let mut count = 0usize;
        let mut removal = None;
        let mut first_value = None;
        while !cursor.is_nil() {
            let value = cursor.try_car().expect("parallel extrema argument");
            let is_flag = if internal {
                position == 0
            } else {
                flag(&cursor)
            };
            if is_flag {
                if removal.is_none() {
                    removal = Some(value);
                }
            } else {
                if !supported(&value) {
                    return None;
                }
                if first_value.is_none() {
                    first_value = Some(value.clone());
                }
                count = count.checked_add(1).unwrap_or_else(|| {
                    crate::sexp::context::r_error("too many parallel extrema arguments")
                });
            }
            position += 1;
            cursor = cursor.try_cdr().expect("parallel extrema argument tail");
        }
        if count == 0 && !internal {
            crate::sexp::context::r_error("no arguments");
        }
        if count <= 1 {
            let na_rm = if let Some(value) = &removal {
                crate::mainutils::coerce::asLogical(value.as_raw())
            } else if internal {
                NA_INTEGER
            } else {
                0
            };
            check();
            if na_rm == NA_INTEGER {
                crate::sexp::context::r_error("invalid 'na.rm' value");
            }
            let Some(value) = first_value else {
                crate::sexp::context::r_error("no arguments");
            };
            if !valid(value.typeof_()) {
                crate::sexp::context::r_error("invalid input type");
            }
            return Some(value.as_raw());
        }
        let bytes = count
            .checked_mul(std::mem::size_of::<Input<'_>>())
            .unwrap_or_else(|| {
                crate::sexp::context::r_error("parallel extrema workspace size overflow")
            });
        let mut ticket = owner
            .with_arena(|a| a.reserve_workspace(bytes))
            .expect("parallel extrema workspace owner")
            .unwrap_or_else(|| {
                crate::sexp::context::r_error(
                    "parallel extrema snapshot exceeds session memory limit",
                )
            });
        let mut values = Vec::new();
        values.try_reserve_exact(count).unwrap_or_else(|_| {
            crate::sexp::context::r_error("cannot allocate parallel extrema snapshot")
        });
        ticket
            .try_resize(
                values
                    .capacity()
                    .checked_mul(std::mem::size_of::<Input<'_>>())
                    .expect("parallel extrema capacity bytes"),
            )
            .unwrap_or_else(|| {
                crate::sexp::context::r_error(
                    "parallel extrema snapshot exceeds session memory limit",
                )
            });
        cursor = arguments.clone();
        position = 0;
        while !cursor.is_nil() {
            if !(if internal {
                position == 0
            } else {
                flag(&cursor)
            }) {
                let value = cursor
                    .try_car()
                    .expect("snapshot parallel extrema argument");
                values.push(Input {
                    length: value.len(),
                    kind: value.typeof_(),
                    value,
                });
            }
            position += 1;
            cursor = cursor.try_cdr().expect("snapshot parallel extrema tail");
        }
        let mut inputs = Inputs {
            values,
            _ticket: ticket,
        };
        let na_rm = if let Some(removal) = &removal {
            crate::mainutils::coerce::asLogical(removal.as_raw())
        } else if internal {
            NA_INTEGER
        } else {
            0
        };
        check();
        if na_rm == NA_INTEGER {
            crate::sexp::context::r_error("invalid 'na.rm' value");
        }
        let na_rm = na_rm != 0;
        if inputs.values.is_empty() {
            crate::sexp::context::r_error("no arguments");
        }
        for input in &mut inputs.values {
            input.length = input.value.len();
            input.kind = input.value.typeof_();
        }
        let first = &inputs.values[0];
        if !valid(first.kind) {
            crate::sexp::context::r_error("invalid input type");
        }
        if inputs.values.len() == 1 {
            return Some(first.value.as_raw());
        }
        let mut kind = first.kind;
        let mut length = first.length;
        for input in &inputs.values[1..] {
            if !valid(input.kind) {
                crate::sexp::context::r_error("invalid input type");
            }
            if input.kind.as_c_int() > kind.as_c_int() {
                kind = input.kind;
            }
            if (length > 0) != (input.length > 0) {
                length = 0;
                break;
            }
            length = length.max(input.length);
        }
        if kind.as_c_int() < SEXPTYPE::INTSXP.as_c_int() {
            kind = SEXPTYPE::INTSXP;
        }
        if length > 0 && inputs.values.iter().any(|x| length % x.length != 0) {
            crate::mainutils::errors::warningcall(
                call,
                c"an argument will be fractionally recycled".as_ptr(),
            );
            check();
        }
        let result = crate::sexp::constructors::Rf_allocVector3(kind, length);
        check();
        let result = f.wrap(result).expect("root parallel extrema result");
        let mut result =
            SexpMut::try_from_checked(result).expect("mutable parallel extrema result");
        for i in 0..length {
            let mut best = read(&inputs.values[0], i % inputs.values[0].length);
            check();
            for input in &inputs.values[1..] {
                let value = read(input, i % input.length);
                check();
                if (na_rm && best.is_nan())
                    || (!best.is_nan()
                        && !value.is_nan()
                        && if minimum { value < best } else { value > best })
                    || (!na_rm && value.is_nan())
                {
                    best = value;
                }
            }
            if kind == SEXPTYPE::REALSXP {
                result
                    .try_set_real_elt(i, best)
                    .expect("write parallel real extremum");
            } else {
                result
                    .try_set_integer_elt(
                        i,
                        if best.is_nan() {
                            NA_INTEGER
                        } else {
                            best as i32
                        },
                    )
                    .expect("write parallel integer extremum");
            }
        }
        let result = result.freeze();
        let copy_shape = !internal
            && !operation
                .and_then(crate::eval::primitive::PrimitiveDescriptor::from_sexp)
                .is_some_and(|x| x.name.ends_with(".int"));
        if copy_shape && inputs.values[0].value.len() == length {
            shape(&f, &inputs.values[0].value, &result, &check);
        }
        check();
        Some(result.as_raw())
    }
}
fn read(input: &Input<'_>, index: i64) -> f64 {
    let value = if input.kind == SEXPTYPE::REALSXP {
        return input
            .value
            .try_real_elt(index)
            .expect("read parallel real input");
    } else if input.kind == SEXPTYPE::LGLSXP {
        input
            .value
            .try_logical_elt(index)
            .expect("read parallel logical input")
    } else {
        input
            .value
            .try_integer_elt(index)
            .expect("read parallel integer input")
    };
    if value == NA_INTEGER {
        NA_REAL
    } else {
        f64::from(value)
    }
}
unsafe fn shape(f: &SessionNodeFactory<'_>, input: &Sexp<'_>, result: &Sexp<'_>, check: impl Fn()) {
    unsafe {
        use crate::sexp::attrib_core::{
            R_DimNamesSymbol, R_DimSymbol, R_NamesSymbol, getAttrib, setAttrib,
        };
        let names = f
            .wrap(getAttrib(input.as_raw(), R_NamesSymbol()))
            .expect("root parallel names");
        check();
        let dims = f
            .wrap(getAttrib(input.as_raw(), R_DimSymbol()))
            .expect("root parallel dimensions");
        check();
        let dimnames = f
            .wrap(getAttrib(input.as_raw(), R_DimNamesSymbol()))
            .expect("root parallel dimnames");
        check();
        if !names.is_nil() && names.len() == result.len() {
            setAttrib(result.as_raw(), R_NamesSymbol(), names.as_raw());
            check();
        }
        if !dims.is_nil() {
            setAttrib(result.as_raw(), R_DimSymbol(), dims.as_raw());
            check();
            if !dimnames.is_nil() {
                setAttrib(result.as_raw(), R_DimNamesSymbol(), dimnames.as_raw());
                check();
            }
        }
    }
}
#[cfg(test)]
mod tests;
