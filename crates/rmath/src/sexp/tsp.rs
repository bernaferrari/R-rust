#![forbid(unsafe_code)]
//! Original-runtime admission for GNU time-series attributes.

use super::{ffi::SEXPTYPE, object::Sexp, owner::RuntimeAccess};

type Value = Sexp<'static>;
type Result<T> = std::result::Result<T, String>;

fn checked<T>(result: super::object::SexpResult<T>) -> Result<T> {
    result.map_err(|error| error.to_string())
}

fn attribute(access: &RuntimeAccess, value: &Value, name: &[u8]) -> Result<Value> {
    let domain = access.domain();
    let mut cell = checked(checked(value.try_attrib())?.into_owned())?;
    let mut seen = std::collections::HashSet::new();
    while !cell.is_nil() {
        let identity = checked(domain.link(&cell))?;
        seen.try_reserve(1)
            .map_err(|_| "cannot inspect attributes")?;
        if !seen.insert(identity) {
            return Err("cyclic attribute list".into());
        }
        if checked(cell.try_tag_name_eq(name))? {
            let result = checked(checked(cell.try_car())?.into_owned())?;
            checked(domain.link(&result))?;
            return Ok(result);
        }
        cell = checked(checked(cell.try_cdr())?.into_owned())?;
    }
    Ok(domain.nil())
}

fn numeric(access: &RuntimeAccess, value: &Value) -> Result<bool> {
    if !matches!(value.typeof_(), SEXPTYPE::INTSXP | SEXPTYPE::REALSXP) {
        return Ok(false);
    }
    if value.typeof_() == SEXPTYPE::INTSXP {
        let class = attribute(access, value, b"class")?;
        if class.typeof_() == SEXPTYPE::STRSXP {
            for index in 0..class.len() {
                let name = checked(checked(class.try_string_elt(index))?.into_owned())?;
                checked(access.require_active())?;
                if checked(name.try_char_eq(b"factor"))? {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

/// Read copied numbers, finish callbacks, validate, then allocate initialized
/// output. No payload borrow or raw cursor crosses a scalar provider.
pub(super) fn normalize(
    access: &RuntimeAccess,
    object: &Value,
    value: &Value,
    epsilon: impl FnOnce(&RuntimeAccess) -> Result<f64>,
) -> Result<Value> {
    let domain = access.domain();
    checked(domain.link(object))?;
    checked(domain.link(value))?;
    if object.is_nil() {
        return Err("attempt to set an attribute on NULL".into());
    }
    let is_numeric = numeric(access, value)?;
    if object.is_s4() {
        if !is_numeric {
            return Err("'tsp' attribute must be numeric".into());
        }
        return Ok(value.clone());
    }
    if !is_numeric || value.len() != 3 {
        return Err("'tsp' attribute must be numeric of length three".into());
    }
    let mut parameters = [0.; 3];
    for (index, number) in parameters.iter_mut().enumerate() {
        *number = match value.typeof_() {
            SEXPTYPE::REALSXP => checked(value.try_real_elt(index as i64))?,
            SEXPTYPE::INTSXP => match checked(value.try_integer_elt(index as i64))? {
                super::ffi::NA_INTEGER => super::ffi::NA_REAL,
                integer => f64::from(integer),
            },
            _ => return Err("'tsp' attribute must be numeric of length three".into()),
        };
        checked(access.require_active())?;
    }
    if parameters[2] <= 0. {
        return Err("invalid time series parameters specified (0)".into());
    }
    let dimensions = attribute(access, object, b"dim")?;
    let rows = if dimensions.is_nil() {
        object.len()
    } else {
        checked(dimensions.try_integer_elt(0))? as i64
    };
    checked(access.require_active())?;
    if rows == 0 {
        return Err("cannot assign 'tsp' to zero-length vector".into());
    }
    let tolerance = epsilon(access)?;
    checked(access.require_active())?;
    validate(parameters, rows, tolerance)?;
    let allocator = checked(access.allocator(&domain))?;
    checked(allocator.allocate(|arena| {
        let pointer = arena.alloc_vector(SEXPTYPE::REALSXP, 3);
        let token = arena.node_token(pointer)?;
        let payload = arena.heap_identity().payload_lease(&token)?;
        for (index, number) in parameters.into_iter().enumerate() {
            payload.set_real_elt(index, number)?;
        }
        Some(pointer)
    }))
}

/// Allocate before selecting the current attribute chain. A collecting
/// callback may replace that chain; no saved tail is used after allocation.
pub(super) fn install(
    access: &RuntimeAccess,
    object: &Value,
    name: &Value,
    value: &Value,
) -> Result<()> {
    let domain = access.domain();
    checked(domain.link(object))?;
    checked(domain.link(name))?;
    checked(domain.link(value))?;
    let allocator = checked(access.allocator(&domain))?;
    let added = if value.is_nil() {
        None
    } else {
        Some(checked(allocator.pairlist_cell(
            value,
            &domain.nil(),
            name,
        ))?)
    };
    checked(access.require_active())?;
    let mut previous = None;
    let mut cell = checked(checked(object.try_attrib())?.into_owned())?;
    let mut seen = std::collections::HashSet::new();
    while !cell.is_nil() {
        let identity = checked(domain.link(&cell))?;
        seen.try_reserve(1)
            .map_err(|_| "cannot inspect attributes")?;
        if !seen.insert(identity) {
            return Err("cyclic attribute list".into());
        }
        let tag = checked(checked(cell.try_tag())?.into_owned())?;
        checked(domain.link(&tag))?;
        if tag == *name {
            if value.is_nil() {
                let rest = checked(checked(cell.try_cdr())?.into_owned())?;
                if let Some(previous) = previous {
                    checked(
                        checked(super::object::SexpMut::try_from_checked(previous))?
                            .try_set_pairlist_cdr(&rest),
                    )?;
                } else {
                    checked(
                        checked(super::object::SexpMut::try_from_checked(object.clone()))?
                            .try_set_attribute(&rest),
                    )?;
                }
            } else {
                checked(
                    checked(super::object::SexpMut::try_from_checked(cell))?
                        .try_set_pairlist_car(value),
                )?;
            }
            return Ok(());
        }
        let rest = checked(checked(cell.try_cdr())?.into_owned())?;
        previous = Some(cell);
        cell = rest;
    }
    if let Some(added) = added {
        if let Some(previous) = previous {
            checked(
                checked(super::object::SexpMut::try_from_checked(previous))?
                    .try_set_pairlist_cdr(&added),
            )?;
        } else {
            checked(
                checked(super::object::SexpMut::try_from_checked(object.clone()))?
                    .try_set_attribute(&added),
            )?;
        }
    }
    Ok(())
}

fn validate([start, end, frequency]: [f64; 3], rows: i64, tolerance: f64) -> Result<()> {
    // GNU's comparison intentionally admits IEEE NaN. Infinity is not
    // rejected independently: Inf-Inf and zero/frequency can yield NaN.
    if (end - start - (rows - 1) as f64 / frequency).abs() > tolerance {
        Err("invalid time series parameters specified (1)".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_ieee_time_series_validation() {
        assert!(validate([1., 4., 1.], 4, 1e-5).is_ok());
        assert!(validate([1., 4., f64::NAN], 4, 1e-5).is_ok());
        assert!(validate([f64::NAN, 4., 1.], 4, 1e-5).is_ok());
        assert!(validate([1., 1., f64::INFINITY], 4, 1e-5).is_ok());
        assert!(validate([f64::INFINITY, f64::INFINITY, 1.], 4, 1e-5).is_ok());
        assert!(validate([1., 4., 1.], 4, f64::NAN).is_ok());
        assert_eq!(
            validate([4., 1., 1.], 4, 1e-5).unwrap_err(),
            "invalid time series parameters specified (1)"
        );
    }
}
