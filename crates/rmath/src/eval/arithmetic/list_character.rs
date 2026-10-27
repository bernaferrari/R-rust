//! Owned coercion and comparison when a list meets a character vector.
use super::*;
use crate::sexp::object::SessionNodeFactory;
use crate::sexp::owner::OwnerToken;

#[derive(Clone)]
struct Shape<'a> {
    len: R_xlen_t,
    names: Sexp<'a>,
    dims: Sexp<'a>,
    dimnames: Sexp<'a>,
}

fn capture_shape<'a>(owner: OwnerToken<'a>, value: &Sexp<'a>) -> Result<Shape<'a>, String> {
    let factory = owner.node_factory();
    let get = |symbol: SEXP| -> Result<Sexp<'a>, String> {
        owner.require_active().map_err(|error| error.to_string())?;
        let attribute = unsafe { getAttrib(value.as_raw(), symbol) };
        owner.require_active().map_err(|error| error.to_string())?;
        factory.wrap(attribute).map_err(|error| error.to_string())
    };
    let (names, dims, dimnames) = unsafe { (R_NamesSymbol(), R_DimSymbol(), R_DimNamesSymbol()) };
    Ok(Shape {
        len: value.len(),
        names: get(names)?,
        dims: get(dims)?,
        dimnames: get(dimnames)?,
    })
}

fn list_to_character<'a>(
    owner: OwnerToken<'a>,
    factory: &SessionNodeFactory<'a>,
    value: &Sexp<'a>,
) -> Result<Sexp<'a>, String> {
    let n = value.len();
    let out = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::STRSXP, n)))
        .map_err(|error| error.to_string())?;
    for i in 0..n {
        owner.require_active().map_err(|error| error.to_string())?;
        let item = value.try_vector_elt(i).map_err(|error| error.to_string())?;
        owner.require_active().map_err(|error| error.to_string())?;
        let character = if item.typeof_() == SEXPTYPE::STRSXP && item.len() == 1 {
            item.try_string_elt(0).map_err(|error| error.to_string())?
        } else {
            // Deparsing may allocate or invoke a callback. Retain the actual
            // list element and validate the original owner before wrapping it.
            let raw = unsafe { crate::mainutils::deparse::deparse1line(item.as_raw(), false) };
            owner.require_active().map_err(|error| error.to_string())?;
            if raw.is_null() {
                factory
                    .wrap(unsafe { R_NaString() })
                    .map_err(|error| error.to_string())?
            } else {
                let text = factory.wrap(raw).map_err(|error| error.to_string())?;
                if text.typeof_() == SEXPTYPE::STRSXP && text.len() > 0 {
                    text.try_string_elt(0).map_err(|error| error.to_string())?
                } else {
                    factory
                        .wrap(unsafe { R_NaString() })
                        .map_err(|error| error.to_string())?
                }
            }
        };
        owner.require_active().map_err(|error| error.to_string())?;
        SexpMut::try_from_checked(out.clone())
            .and_then(|mut out| out.try_set_string_elt(i, character))
            .map_err(|error| error.to_string())?;
    }
    Ok(out)
}

fn convert<'a>(
    owner: OwnerToken<'a>,
    factory: &SessionNodeFactory<'a>,
    value: &Sexp<'a>,
) -> Result<Sexp<'a>, String> {
    match value.typeof_() {
        SEXPTYPE::STRSXP => Ok(value.clone()),
        SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP => list_to_character(owner, factory, value),
        _ => {
            let raw = unsafe { relop_operand_to_character(value.as_raw()) };
            owner.require_active().map_err(|error| error.to_string())?;
            factory.wrap(raw).map_err(|error| error.to_string())
        }
    }
}

fn attributes<'a>(
    owner: OwnerToken<'a>,
    factory: &SessionNodeFactory<'a>,
    out: &Sexp<'a>,
    a: &Shape<'a>,
    b: &Shape<'a>,
    n: R_xlen_t,
) -> Result<(), String> {
    let aa = !a.dims.is_nil();
    let ba = !b.dims.is_nil();
    let applies_a = aa && !(a.len == 1 && b.len != 1) && (b.len != 0 || a.len == 0);
    let applies_b = ba && !(b.len == 1 && a.len != 1) && (a.len != 0 || b.len == 0);
    let (dims, dimnames, names) = if applies_a && applies_b {
        if a.dims.len() != b.dims.len()
            || a.dims.typeof_() != SEXPTYPE::INTSXP
            || b.dims.typeof_() != SEXPTYPE::INTSXP
        {
            return Err("non-conformable arrays".into());
        }
        for i in 0..a.dims.len() {
            if a.dims
                .try_integer_elt(i)
                .map_err(|error| error.to_string())?
                != b.dims
                    .try_integer_elt(i)
                    .map_err(|error| error.to_string())?
            {
                return Err("non-conformable arrays".into());
            }
        }
        (
            Some(&a.dims),
            if a.dimnames.is_nil() {
                Some(&b.dimnames)
            } else {
                Some(&a.dimnames)
            },
            None,
        )
    } else if applies_a && a.len == n {
        (Some(&a.dims), Some(&a.dimnames), None)
    } else if applies_b && b.len == n {
        (Some(&b.dims), Some(&b.dimnames), None)
    } else if applies_a || applies_b {
        (None, None, None)
    } else {
        let names = if a.len == n && !a.names.is_nil() {
            Some(&a.names)
        } else if b.len == n && !b.names.is_nil() {
            Some(&b.names)
        } else {
            None
        };
        (None, None, names)
    };
    let mut chain = factory.nil();
    for (symbol, value) in [
        (unsafe { R_NamesSymbol() }, names),
        (unsafe { R_DimNamesSymbol() }, dimnames),
        (unsafe { R_DimSymbol() }, dims),
    ] {
        if let Some(value) = value.filter(|value| !value.is_nil()) {
            owner.require_active().map_err(|error| error.to_string())?;
            let tag = factory.wrap(symbol).map_err(|error| error.to_string())?;
            chain = factory
                .pairlist_cell(value, &chain, &tag)
                .map_err(|error| error.to_string())?;
        }
    }
    SexpMut::try_from_checked(out.clone())
        .and_then(|mut out| out.try_set_attribute(&chain))
        .map_err(|error| error.to_string())
}

/// Preserve the original owner, operands and shape across coercion callbacks.
pub(super) unsafe fn compare(op: &str, left: SEXP, right: SEXP) -> Result<SEXP, String> {
    let owner = unsafe { OwnerToken::current() }.map_err(|error| error.to_string())?;
    let _pin = owner.pin().map_err(|error| error.to_string())?;
    let factory = owner.node_factory();
    let left = factory.wrap(left).map_err(|error| error.to_string())?;
    let right = factory.wrap(right).map_err(|error| error.to_string())?;
    let a = capture_shape(owner, &left)?;
    let b = capture_shape(owner, &right)?;
    let n = if a.len == 0 || b.len == 0 {
        0
    } else {
        a.len.max(b.len)
    };
    if (!a.dims.is_nil() && b.dims.is_nil() && a.len > 0 && b.len > 1 && a.len % b.len != 0)
        || (!b.dims.is_nil() && a.dims.is_nil() && b.len > 0 && a.len > 1 && b.len % a.len != 0)
    {
        let (product, other) = if !a.dims.is_nil() {
            (a.len, b.len)
        } else {
            (b.len, a.len)
        };
        return Err(format!(
            "dims [product {product}] do not match the length of object [{other}]"
        ));
    }
    // Validate conformability before a warning or coercion can invoke R.
    if !a.dims.is_nil() && !b.dims.is_nil() {
        if a.dims.typeof_() != SEXPTYPE::INTSXP
            || b.dims.typeof_() != SEXPTYPE::INTSXP
            || a.dims.len() != b.dims.len()
        {
            return Err("non-conformable arrays".into());
        }
        for i in 0..a.dims.len() {
            if a.dims
                .try_integer_elt(i)
                .map_err(|error| error.to_string())?
                != b.dims
                    .try_integer_elt(i)
                    .map_err(|error| error.to_string())?
            {
                return Err("non-conformable arrays".into());
            }
        }
    }
    let converted = if n == 0 {
        None
    } else {
        warn_if_non_multiple_recycling(a.len, b.len);
        owner.require_active().map_err(|error| error.to_string())?;
        Some((
            convert(owner, &factory, &left)?,
            convert(owner, &factory, &right)?,
        ))
    };
    // The operands and their original shape remain rooted through conversion.
    let out = factory
        .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::LGLSXP, n)))
        .map_err(|error| error.to_string())?;
    let Some((a_value, b_value)) = converted else {
        attributes(owner, &factory, &out, &a, &b, n)?;
        return Ok(out.as_raw());
    };
    let na = factory
        .wrap(unsafe { R_NaString() })
        .map_err(|error| error.to_string())?;
    for i in 0..n {
        owner.require_active().map_err(|error| error.to_string())?;
        let ac = a_value
            .try_string_elt(i % a.len)
            .map_err(|error| error.to_string())?;
        owner.require_active().map_err(|error| error.to_string())?;
        let bc = b_value
            .try_string_elt(i % b.len)
            .map_err(|error| error.to_string())?;
        owner.require_active().map_err(|error| error.to_string())?;
        let value = if ac.as_raw() == na.as_raw() || bc.as_raw() == na.as_raw() {
            NA_LOGICAL
        } else if op == "==" || op == "!=" {
            let equal = unsafe { crate::mainutils::relop::Seql(ac.as_raw(), bc.as_raw()) } != 0;
            if equal == (op == "==") {
                TRUE
            } else {
                FALSE
            }
        } else {
            let a_bytes = unsafe { crate::sexp::accessors::charsxp_as_utf8(ac.as_raw()) };
            let b_bytes = unsafe { crate::sexp::accessors::charsxp_as_utf8(bc.as_raw()) };
            let a_text = String::from_utf8_lossy(&a_bytes);
            let b_text = String::from_utf8_lossy(&b_bytes);
            if compare_strings(op, &a_text, &b_text) {
                TRUE
            } else {
                FALSE
            }
        };
        SexpMut::try_from_checked(out.clone())
            .and_then(|mut out| out.try_set_logical_elt(i, value))
            .map_err(|error| error.to_string())?;
    }
    owner.require_active().map_err(|error| error.to_string())?;
    attributes(owner, &factory, &out, &a, &b, n)?;
    Ok(out.as_raw())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};

    #[test]
    fn list_character_coercion_keeps_original_operands_during_collection() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active_in(|instance| unsafe {
            let factory = session.owner_token().unwrap().node_factory();
            let number = factory
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::REALSXP, 1)))
                .unwrap();
            let mut number = SexpMut::try_from_checked(number).unwrap();
            number.try_set_real_elt(0, 0.25).unwrap();
            let list = factory
                .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, 1)))
                .unwrap();
            let mut list_mut = SexpMut::try_from_checked(list.clone()).unwrap();
            list_mut.try_set_vector_elt(0, number.freeze()).unwrap();
            drop(list_mut);
            let text = factory.strings(&["0.25"]).unwrap();
            let holder = factory.pairlist_cell(&list, &text, &factory.nil()).unwrap();
            let (left, right) = (list.as_raw(), text.as_raw());
            drop(list);
            drop(text);
            let observed = Rc::new(Cell::new(0));
            let seen = observed.clone();
            let retained = holder.clone().into_owned().unwrap();
            let nil = factory.nil().into_owned().unwrap();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if seen.replace(seen.get() + 1) == 0 {
                    (*instance).memory_state.gc_force_gap = 0;
                    let mut holder = SexpMut::try_from_checked(retained.clone()).unwrap();
                    holder.try_set_pairlist_car(&nil).unwrap();
                    holder.try_set_pairlist_cdr(&nil).unwrap();
                    crate::sexp::gengc::full_gc_in(instance);
                }
            }));
            (*instance).memory_state.gc_force_gap = 1;
            (*instance).memory_state.gc_force_wait = 1;
            let result = compare("==", left, right).unwrap();
            let result = session
                .owner_token()
                .unwrap()
                .node_factory()
                .wrap(result)
                .unwrap();
            assert!(
                observed.get() > 0,
                "coercion must actually invoke collection"
            );
            assert_eq!(result.try_logical_elt(0).unwrap(), TRUE);
        });
    }

    #[test]
    fn list_character_coercion_rejects_owner_revoked_by_collection() {
        use std::cell::RefCell;
        let facade = Rc::new(RefCell::new(Some(
            crate::sexp::session::RSession::new_for_gc_tests(),
        )));
        let (owner, holder, left, right) = {
            let borrowed = facade.borrow();
            let session = borrowed.as_ref().unwrap();
            session.with_active(|| {
                let owner = session.owner_token().unwrap().weak_owner().unwrap();
                let factory = owner.node_factory().unwrap();
                let number = factory
                    .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::REALSXP, 1)))
                    .unwrap();
                let mut number = SexpMut::try_from_checked(number).unwrap();
                number.try_set_real_elt(0, 0.25).unwrap();
                let list = factory
                    .allocate(|arena| Some(arena.alloc_vector(SEXPTYPE::VECSXP, 1)))
                    .unwrap();
                let mut list_mut = SexpMut::try_from_checked(list.clone()).unwrap();
                list_mut.try_set_vector_elt(0, number.freeze()).unwrap();
                drop(list_mut);
                let text = factory.strings(&["0.25"]).unwrap();
                let holder = factory
                    .pairlist_cell(&list, &text, &factory.nil())
                    .unwrap()
                    .into_owned()
                    .unwrap();
                (owner, holder, list.as_raw(), text.as_raw())
            })
        };
        let pin = owner.pin().unwrap();
        let instance = pin.as_ptr();
        let seen = Rc::new(Cell::new(false));
        let observed = seen.clone();
        let callback_facade = Rc::downgrade(&facade);
        let result = unsafe {
            crate::sexp::session::with_instance_active(instance, || {
                crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                    if !observed.replace(true) {
                        (*instance).memory_state.gc_force_gap = 0;
                        drop(callback_facade.upgrade().unwrap().borrow_mut().take());
                    }
                }));
                (*instance).memory_state.gc_force_gap = 1;
                (*instance).memory_state.gc_force_wait = 1;
                compare("==", left, right)
            })
        };
        assert!(
            seen.get(),
            "comparison must actually enter the collecting callback"
        );
        assert!(result.is_err(), "a revoked owner must not publish a result");
        drop(holder);
    }
}
