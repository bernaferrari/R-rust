#![forbid(unsafe_code)]
//! The observation map used by GNU naresid.exclude, with owning row labels.
use super::{Result, Value, active, attribute, checked, numeric};
use crate::sexp::{ffi::SEXPTYPE, owner::RuntimeAccess};

pub(super) struct Rows {
    // These canonical roots survive provider/allocation callbacks independently
    // of the model's later mutation or removal.
    _action: Value,
    labels: Value,
    positions: Vec<i64>,
}
impl Rows {
    pub(super) fn capture(access: &RuntimeAccess, action: Value) -> Result<Option<Self>> {
        if action.is_nil() {
            return Ok(None);
        }
        let class = attribute(&action, b"class")?;
        let labels = attribute(&action, b"names")?;
        let mut exclude = false;
        for i in 0..class.len() {
            let name = checked(class.try_string_value_elt(i))?;
            active(access)?;
            if name.as_deref() == Some("exclude") {
                exclude = true;
                break;
            }
        }
        if !exclude {
            return Ok(None);
        }
        if action.len() == 0 || !matches!(action.typeof_(), SEXPTYPE::INTSXP | SEXPTYPE::REALSXP) {
            return Err("invalid argument 'omit'".into());
        }
        let mut positions = Vec::new();
        for position in numeric(access, &action)? {
            if position.is_nan() {
                return Err("NAs are not allowed in subscripted assignments".into());
            }
            if !position.is_finite() || position.abs() >= i64::MAX as f64 {
                return Err("invalid argument 'omit'".into());
            }
            positions.push(position.trunc() as i64);
        }
        if positions.iter().any(|i| *i < 0) && positions.iter().any(|i| *i > 0) {
            return Err("only 0's may be mixed with negative subscripts".into());
        }
        Ok(Some(Self {
            _action: action,
            labels,
            positions,
        }))
    }
    fn map(
        &self,
        length: usize,
        mut warn: impl FnMut(usize, usize) -> Result<()>,
    ) -> Result<Vec<Option<usize>>> {
        let total = length
            .checked_add(self.positions.len())
            .filter(|total| i64::try_from(*total).is_ok())
            .ok_or("invalid row count")?;
        let positive = self.positions.iter().any(|i| *i > 0);
        let negative = self.positions.iter().any(|i| *i < 0);
        let positions: std::collections::HashSet<_> = self.positions.iter().copied().collect();
        let selected: Vec<_> = (0..total)
            .filter(|i| {
                let position = (*i + 1) as i64;
                if positive {
                    !positions.contains(&position)
                } else if negative {
                    positions.contains(&-position)
                } else {
                    false
                }
            })
            .collect();
        warn(selected.len(), length)?;
        let mut result = Vec::new();
        result
            .try_reserve_exact(total)
            .map_err(|_| "cannot allocate row map")?;
        result.resize(total, None);
        if length > 0 {
            for (source, target) in selected.into_iter().enumerate() {
                result[target] = Some(source % length);
            }
        }
        Ok(result)
    }
    pub(super) fn restore(
        &self,
        access: &RuntimeAccess,
        values: &[f64],
        intern: &mut impl FnMut(&str) -> Result<Value>,
    ) -> Result<Vec<f64>> {
        Ok(self
            .map(values.len(), |target, source| {
                super::warn_recycling_assignment(access, target, source, intern)
            })?
            .into_iter()
            .map(|i| i.map_or(crate::sexp::ffi::NA_REAL, |i| values[i]))
            .collect())
    }
    pub(super) fn names(
        &self,
        access: &RuntimeAccess,
        names: &Value,
        length: usize,
    ) -> Result<Value> {
        if names.is_nil() {
            return Ok(access.domain().nil());
        }
        if names.len() != length as i64 {
            return Err("invalid model row names".into());
        }
        let map = self.map(length, |_, _| Ok(()))?;
        let missing = checked(
            access.domain().wrap(
                crate::sexp::globals::immutable_na_string_projection()
                    .ok_or("missing original string sentinel")?,
            ),
        )?;
        let mut result = Vec::new();
        for source in map {
            result.push(if let Some(source) = source {
                let name = checked(names.try_string_elt(source as i64))?;
                active(access)?;
                checked(name.into_owned())?
            } else {
                missing.clone()
            });
        }
        let positive = self.positions.iter().any(|i| *i > 0);
        let negative = self.positions.iter().any(|i| *i < 0);
        let targets: Vec<_> = if positive {
            self.positions
                .iter()
                .filter(|i| **i > 0)
                .map(|i| (*i - 1) as usize)
                .collect()
        } else if negative {
            (0..result.len())
                .filter(|i| !self.positions.contains(&-((*i + 1) as i64)))
                .collect()
        } else {
            Vec::new()
        };
        if let Some(maximum) = targets.iter().max().copied().filter(|i| *i >= result.len()) {
            return Err(format!(
                "'names' attribute [{}] must be the same length as the vector [{}]",
                maximum + 1,
                result.len()
            ));
        }
        if !targets.is_empty() && self.labels.len() == 0 {
            return Err("replacement has length zero".into());
        }
        for (index, target) in targets.into_iter().enumerate() {
            let label = checked(self.labels.try_string_elt(index as i64 % self.labels.len()))?;
            active(access)?;
            result[target] = checked(label.into_owned())?;
        }
        strings(access, &result)
    }
}
pub(super) fn select_names(
    access: &RuntimeAccess,
    names: &Value,
    length: usize,
    indices: &[usize],
) -> Result<Value> {
    if names.is_nil() || names.len() != length as i64 {
        return Ok(access.domain().nil());
    }
    let mut children = Vec::new();
    for index in indices {
        let child = checked(names.try_string_elt(*index as i64))?;
        active(access)?;
        children.push(checked(child.into_owned())?);
    }
    strings(access, &children)
}
fn strings(access: &RuntimeAccess, children: &[Value]) -> Result<Value> {
    let domain = access.domain();
    let links = children
        .iter()
        .map(|child| checked(domain.link(child)))
        .collect::<Result<Vec<_>>>()?;
    let allocator = checked(access.allocator(&domain))?;
    let value = checked(allocator.allocate(|arena| {
        let pointer = arena.alloc_vector(SEXPTYPE::STRSXP, links.len() as i64);
        let node = arena.node_token(pointer)?;
        let heap = arena.heap_identity();
        let payload = heap.payload_lease(&node)?;
        for (index, link) in links.iter().copied().enumerate() {
            payload.set_reference_elt(index, link)?;
        }
        Some(pointer)
    }))?;
    active(access)?;
    Ok(value)
}
