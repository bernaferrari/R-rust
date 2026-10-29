//! Rooted default lengths mapper. Method dispatch remains a separate concern.
use crate::sexp::{
    ffi::{NA_INTEGER, SEXP, SEXPTYPE},
    object::{SessionNodeFactory, Sexp, SexpMut},
    owner::OwnerToken,
};

fn pair_kind(x: &Sexp<'_>) -> bool {
    matches!(
        x.typeof_(),
        SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP | SEXPTYPE::DOTSXP
    )
}
fn next<'s>(x: &Sexp<'s>) -> Sexp<'s> {
    if x.is_nil() {
        return x.clone();
    }
    if !pair_kind(x) {
        crate::sexp::context::r_error("improper pairlist in lengths");
    }
    x.try_cdr().expect("lengths checked pairlist tail")
}
fn validate_pairlist(x: &Sexp<'_>) {
    let mut slow = x.clone();
    let mut fast = x.clone();
    loop {
        slow = next(&slow);
        fast = next(&next(&fast));
        if slow.is_nil() || fast.is_nil() {
            return;
        }
        if slow.as_raw() == fast.as_raw() {
            crate::sexp::context::r_error("cyclic pairlist in lengths");
        }
    }
}
fn ordinary_length(x: &Sexp<'_>) -> i64 {
    if x.is_nil() {
        return 0;
    }
    if pair_kind(x) {
        validate_pairlist(x);
        let mut cell = x.clone();
        let mut count = 0i64;
        while !cell.is_nil() {
            count = count
                .checked_add(1)
                .unwrap_or_else(|| crate::sexp::context::r_error("lengths size overflow"));
            cell = next(&cell);
        }
        return count;
    }
    if x.is_vector() || x.typeof_() == SEXPTYPE::CHARSXP {
        x.len()
    } else {
        1
    }
}

fn attribute<'s>(x: &Sexp<'s>, symbol: &Sexp<'_>) -> Sexp<'s> {
    let attributes = x.try_attrib().expect("lengths attribute root");
    validate_pairlist(&attributes);
    let mut cell = attributes;
    while !cell.is_nil() {
        if cell.try_tag().expect("lengths attribute tag").as_raw() == symbol.as_raw() {
            return cell.try_car().expect("lengths attribute value");
        }
        cell = next(&cell);
    }
    // The nil tail belongs to the original allocation domain too.
    cell
}

unsafe fn child_length(x: &Sexp<'_>, i: i64, class_symbol: &Sexp<'_>, check: &impl Fn()) -> i64 {
    let child = x.try_vector_elt(i).expect("lengths checked child");
    check();
    // Preserve the existing POSIXlt observation-count policy in this bounded
    // default migration; other class methods are not newly dispatched here.
    let mut posixlt = false;
    if child.typeof_() == SEXPTYPE::VECSXP {
        let class = attribute(&child, class_symbol);
        if class.typeof_() == SEXPTYPE::STRSXP {
            for j in 0..class.len() {
                let name = class.try_string_elt(j).expect("lengths class name");
                check();
                let matches = unsafe {
                    std::ffi::CStr::from_ptr(crate::sexp::accessors::CHAR(name.as_raw()))
                }
                .to_bytes()
                    == b"POSIXlt";
                if matches {
                    posixlt = true;
                    break;
                }
            }
        }
    }
    if posixlt {
        let mut count = 0;
        for j in 0..child.len() {
            let field = child.try_vector_elt(j).expect("lengths POSIXlt field");
            check();
            count = count.max(ordinary_length(&field));
        }
        count
    } else {
        ordinary_length(&child)
    }
}

unsafe fn allocate<'s>(
    f: &SessionNodeFactory<'s>,
    kind: SEXPTYPE,
    n: i64,
    check: &impl Fn(),
) -> SexpMut<'s> {
    let output = unsafe { crate::sexp::constructors::Rf_allocVector3(kind, n) };
    check();
    SexpMut::try_from_checked(f.wrap(output).expect("lengths admitted output owner"))
        .expect("lengths fresh output mutation")
}

pub(super) unsafe fn run(call: SEXP, op: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        let owner = OwnerToken::current().expect("active lengths owner");
        let pin = owner.pin().expect("live lengths owner");
        let f = owner.node_factory();
        let args = f.wrap(args).expect("lengths argument owner");
        let _call = (!call.is_null()).then(|| f.wrap(call).expect("lengths call owner"));
        let _op = (!op.is_null()).then(|| f.wrap(op).expect("lengths operation owner"));
        let _rho = (!rho.is_null()).then(|| f.wrap(rho).expect("lengths environment owner"));
        let check = || {
            if let Some(pin) = &pin {
                pin.require_live()
                    .expect("live lengths owner after callback");
            }
            f.require_active()
                .expect("original lengths owner after callback");
        };
        let x = args.try_car().expect("lengths input root");
        let tail = args.try_cdr().expect("lengths flag cell");
        let use_names = if tail.is_nil() {
            true
        } else {
            let flag = tail.try_car().expect("lengths flag root");
            let value = if flag.as_raw() == crate::sexp::globals::R_MissingArg() {
                1 // The R wrapper's use.names default also covers an empty slot.
            } else {
                crate::mainutils::coerce::asLogical(flag.as_raw())
            };
            check();
            if value == NA_INTEGER {
                crate::sexp::context::r_error("invalid 'use.names' value");
            }
            value != 0
        };
        let list = matches!(x.typeof_(), SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP);
        if !list
            && !matches!(
                x.typeof_(),
                SEXPTYPE::NILSXP
                    | SEXPTYPE::CHARSXP
                    | SEXPTYPE::LGLSXP
                    | SEXPTYPE::INTSXP
                    | SEXPTYPE::REALSXP
                    | SEXPTYPE::CPLXSXP
                    | SEXPTYPE::STRSXP
                    | SEXPTYPE::RAWSXP
            )
        {
            crate::sexp::context::r_error("'x' must be a list or atomic vector");
        }
        let n = x.len();
        let class_symbol = f.wrap(crate::eval::attrib_core::R_ClassSymbol()).unwrap();
        let mut output = allocate(&f, SEXPTYPE::INTSXP, n, &check);
        for i in 0..n {
            let count = if list {
                child_length(&x, i, &class_symbol, &check)
            } else {
                1
            };
            if count > i32::MAX as i64 {
                // GNU restarts in REAL width after the first long child.
                let mut wide = allocate(&f, SEXPTYPE::REALSXP, n, &check);
                for j in 0..n {
                    let value = child_length(&x, j, &class_symbol, &check);
                    wide.try_set_real_elt(j, value as f64)
                        .expect("lengths wide element");
                }
                output = wide;
                break;
            }
            output
                .try_set_integer_elt(i, count as i32)
                .expect("lengths integer element");
        }
        let output = output.freeze();
        // GNU reads shape after element callbacks, in dim/names/dimnames
        // order. Root each selection before its setter can allocate or collect.
        let dim_symbol = f.wrap(crate::eval::attrib_core::R_DimSymbol()).unwrap();
        let names_symbol = f.wrap(crate::eval::attrib_core::R_NamesSymbol()).unwrap();
        let dimnames_symbol = f
            .wrap(crate::eval::attrib_core::R_DimNamesSymbol())
            .unwrap();
        for (symbol, copy) in [
            (&dim_symbol, true),
            (&names_symbol, use_names),
            (&dimnames_symbol, use_names),
        ] {
            if copy {
                let value = attribute(&x, symbol);
                if !value.is_nil() {
                    crate::eval::attrib_core::setAttrib(
                        output.as_raw(),
                        symbol.as_raw(),
                        value.as_raw(),
                    );
                    check();
                }
            }
        }
        check();
        output.as_raw()
    }
}

#[cfg(test)]
mod tests;
