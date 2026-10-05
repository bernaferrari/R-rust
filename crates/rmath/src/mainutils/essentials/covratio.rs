#![forbid(unsafe_code)]
//! Owning GNU covariance-ratio inputs and callback-free arithmetic.
use crate::sexp::{ffi::SEXPTYPE, object::Sexp, owner::RuntimeAccess};

#[path = "covratio/rows.rs"]
mod rows;

#[path = "covratio/condition_calls.rs"]
mod condition_calls;

type Value = Sexp<'static>;
type Result<T> = std::result::Result<T, String>;
fn checked<T>(value: crate::sexp::object::SexpResult<T>) -> Result<T> {
    value.map_err(|e| e.to_string())
}
fn active(access: &RuntimeAccess) -> Result<()> {
    checked(access.require_active())
}
fn field(access: &RuntimeAccess, value: &Value, name: &[u8]) -> Result<Value> {
    if value.is_nil() {
        return Ok(access.domain().nil());
    }
    if value.typeof_() != SEXPTYPE::VECSXP {
        return Err("$ operator is invalid for atomic vectors".into());
    }
    let names = attribute(value, b"names")?;
    if names.is_nil() {
        return Ok(access.domain().nil());
    }
    for i in 0..value.len().min(names.len()) {
        let text = checked(names.try_string_value_elt(i))?;
        active(access)?;
        if text.as_ref().is_some_and(|s| s.as_bytes() == name) {
            let result = checked(value.try_vector_elt(i))?;
            active(access)?;
            return checked(result.into_owned());
        }
    }
    Ok(access.domain().nil())
}
fn attribute(value: &Value, name: &[u8]) -> Result<Value> {
    let mut cell = checked(value.try_attrib())?;
    let mut seen = std::collections::HashSet::new();
    while !cell.is_nil() {
        let allocation = checked(cell.allocation())?;
        if !seen.insert(allocation.link()) {
            return Err("cyclic attribute list".into());
        }
        if checked(cell.try_tag_name_eq(name))? {
            return checked(checked(cell.try_car())?.into_owned());
        }
        cell = checked(cell.try_cdr())?;
    }
    checked(cell.into_owned())
}
fn numeric(access: &RuntimeAccess, value: &Value) -> Result<Vec<f64>> {
    let length = usize::try_from(value.len()).map_err(|_| "invalid vector length")?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(length)
        .map_err(|_| "cannot allocate numeric snapshot")?;
    for i in 0..value.len() {
        let number = match value.typeof_() {
            SEXPTYPE::REALSXP => checked(value.try_real_elt(i))?,
            SEXPTYPE::INTSXP => {
                let n = checked(value.try_integer_elt(i))?;
                if n == crate::sexp::ffi::NA_INTEGER {
                    crate::sexp::ffi::NA_REAL
                } else {
                    f64::from(n)
                }
            }
            SEXPTYPE::LGLSXP => {
                let n = checked(value.try_logical_elt(i))?;
                if n == crate::sexp::ffi::NA_LOGICAL {
                    crate::sexp::ffi::NA_REAL
                } else {
                    f64::from(n)
                }
            }
            SEXPTYPE::NILSXP => return Ok(Vec::new()),
            _ => return Err("non-numeric argument to binary operator".into()),
        };
        active(access)?;
        output.push(number);
    }
    Ok(output)
}
fn arguments(access: &RuntimeAccess, args: Value) -> Result<[Option<Value>; 3]> {
    let mut captured = Vec::new();
    let mut cell = args;
    let mut seen = std::collections::HashSet::new();
    while !cell.is_nil() {
        let allocation = checked(cell.allocation())?;
        if !seen.insert(allocation.link()) {
            return Err("cyclic argument list".into());
        }
        let tag = checked(cell.try_tag())?;
        let name = if tag.is_nil() {
            String::new()
        } else {
            checked(checked(tag.try_printname())?.try_as_string())?
        };
        captured.push((name, checked(checked(cell.try_car())?.into_owned())?));
        cell = checked(cell.try_cdr())?;
    }
    let formals = ["model", "infl", "res"];
    let mut result: [Option<Value>; 3] = [None, None, None];
    for (name, value) in &captured {
        if let Some(index) = formals.iter().position(|formal| *formal == name) {
            if result[index].replace(value.clone()).is_some() {
                return Err(format!(
                    "formal argument \"{name}\" matched by multiple actual arguments"
                ));
            }
        }
    }
    for (name, value) in &captured {
        if name.is_empty() || formals.contains(&name.as_str()) {
            continue;
        }
        let matches: Vec<_> = formals
            .iter()
            .enumerate()
            .filter(|(_, formal)| formal.starts_with(name))
            .map(|(index, _)| index)
            .collect();
        if matches.len() > 1 {
            return Err(format!(
                "argument matches multiple formal arguments: {name}"
            ));
        }
        let index = matches
            .first()
            .copied()
            .ok_or_else(|| format!("unused argument ({name})"))?;
        if result[index].replace(value.clone()).is_some() {
            return Err(format!(
                "formal argument '{}' matched by multiple actual arguments",
                formals[index]
            ));
        }
    }
    for (name, value) in captured {
        if !name.is_empty() {
            continue;
        }
        let index = result
            .iter()
            .position(Option::is_none)
            .ok_or("unused argument")?;
        result[index] = Some(value);
    }
    active(access)?;
    Ok(result)
}
fn cleaned_residuals(residuals: &[f64]) -> Vec<f64> {
    let mut e = residuals.to_vec();
    let mut absolute: Vec<_> = e.iter().map(|v| v.abs()).collect();
    absolute.sort_by(f64::total_cmp);
    let median = if absolute.is_empty() {
        f64::NAN
    } else if absolute.len() % 2 == 0 {
        (absolute[absolute.len() / 2 - 1] + absolute[absolute.len() / 2]) * 0.5
    } else {
        absolute[absolute.len() / 2]
    };
    for value in &mut e {
        if value.abs() < 100. * f64::EPSILON * median {
            *value = 0.;
        }
    }
    e
}
fn deletion_sigma(residuals: &[f64], hat: &[f64], n: f64, p: f64) -> Vec<f64> {
    let e = cleaned_residuals(residuals);
    let sum = e.iter().map(|v| v * v).sum::<f64>();
    e.iter()
        .zip(hat)
        .map(|(e, h)| {
            if *h < 1. {
                ((sum - e * e / (1. - h)) / (n - p - 1.)).sqrt()
            } else {
                (sum / (n - p - 1.)).sqrt()
            }
        })
        .collect()
}
/// The default residual input is stats::weighted.residuals(model): working
/// residuals scaled by working weights, with zero prior-weight cases omitted.
struct ResidualSnapshot {
    values: Vec<f64>,
    names: Value,
}
fn weighted_residuals(
    access: &RuntimeAccess,
    residuals: &Value,
    names: &Value,
    weights: &Value,
    drop_weights: &Value,
    omitted: Option<&rows::Rows>,
    intern: &mut impl FnMut(&str) -> Result<Value>,
) -> Result<ResidualSnapshot> {
    let raw = numeric(access, residuals)?;
    let mut values = if let Some(rows) = omitted {
        rows.restore(access, &raw, intern)?
    } else {
        raw
    };
    let names = if let Some(rows) = omitted {
        rows.names(access, names, residuals.len() as usize)?
    } else {
        names.clone()
    };
    if !weights.is_nil() {
        let weights = numeric(access, weights)?;
        let weights = if let Some(rows) = omitted {
            rows.restore(access, &weights, intern)?
        } else {
            weights
        };
        if weights.len() != values.len() {
            return Err("model weights do not match working residuals".into());
        }
        for (value, weight) in values.iter_mut().zip(weights) {
            if crate::sexp::ffi::is_na_real(*value) || crate::sexp::ffi::is_na_real(weight) {
                *value = crate::sexp::ffi::NA_REAL;
            } else {
                *value *= weight.sqrt();
            }
        }
    }
    if drop_weights.is_nil() {
        return Ok(ResidualSnapshot { values, names });
    }
    let weights = numeric(access, drop_weights)?;
    let weights = if let Some(rows) = omitted {
        rows.restore(access, &weights, intern)?
    } else {
        weights
    };
    if weights.len() != values.len() {
        return Err("model prior weights do not match working residuals".into());
    }
    let retained: Vec<_> = weights
        .iter()
        .enumerate()
        .filter_map(|(i, weight)| (*weight != 0.).then_some(i))
        .collect();
    let names = rows::select_names(access, &names, values.len(), &retained)?;
    values = retained
        .iter()
        .map(|i| {
            if weights[*i].is_nan() {
                crate::sexp::ffi::NA_REAL
            } else {
                values[*i]
            }
        })
        .collect();
    Ok(ResidualSnapshot { values, names })
}
fn warn_recycling(
    access: &RuntimeAccess,
    lhs: usize,
    rhs: usize,
    phase: condition_calls::Phase,
    intern: &mut impl FnMut(&str) -> Result<Value>,
) -> Result<()> {
    if lhs > 0 && rhs > 0 && !lhs.max(rhs).is_multiple_of(lhs.min(rhs)) {
        condition_calls::emit(access, phase, intern)?;
    }
    Ok(())
}
fn warn_recycling_assignment(
    access: &RuntimeAccess,
    target: usize,
    source: usize,
    intern: &mut impl FnMut(&str) -> Result<Value>,
) -> Result<()> {
    if target > 0 && source > 0 && !target.is_multiple_of(source) {
        condition_calls::emit(access, condition_calls::Phase::RowMap, intern)?;
    }
    Ok(())
}
fn binary_length(lhs: usize, rhs: usize) -> usize {
    if lhs == 0 || rhs == 0 {
        0
    } else {
        lhs.max(rhs)
    }
}
fn binary_names(
    access: &RuntimeAccess,
    lhs: &Value,
    lhs_len: usize,
    rhs: &Value,
    rhs_len: usize,
) -> Value {
    let length = binary_length(lhs_len, rhs_len);
    if length > 0 && lhs_len == length && !lhs.is_nil() && lhs.len() == length as i64 {
        lhs.clone()
    } else if length > 0 && rhs_len == length && !rhs.is_nil() && rhs.len() == length as i64 {
        rhs.clone()
    } else {
        access.domain().nil()
    }
}
fn is_glm(access: &RuntimeAccess, model: &Value) -> Result<bool> {
    let class = attribute(model, b"class")?;
    if class.is_nil() {
        return Ok(false);
    }
    if class.typeof_() != SEXPTYPE::STRSXP {
        return Err("invalid model class".into());
    }
    for i in 0..class.len() {
        let text = checked(class.try_string_value_elt(i))?;
        active(access)?;
        if text.as_deref() == Some("glm") {
            return Ok(true);
        }
    }
    Ok(false)
}
/// stats::estDisp is false for explicit dispersion, poisson and binomial.
/// In that branch sigma.glm is sqrt(summary(model)$dispersion), independent
/// of the supplied per-case influence sigma.
fn fixed_glm_sigma(access: &RuntimeAccess, family: &Value) -> Result<Option<f64>> {
    let dispersion = field(access, family, b"dispersion")?;
    let name = field(access, family, b"family")?;
    let dispersion = numeric(access, &dispersion)?;
    if let Some(value) = dispersion.first().copied().filter(|value| !value.is_nan()) {
        return Ok(Some(value.sqrt()));
    }
    if !name.is_nil() {
        let name = checked(name.try_string_value_elt(0))?;
        active(access)?;
        if matches!(name.as_deref(), Some("poisson" | "binomial")) {
            return Ok(Some(1.));
        }
    }
    Ok(None)
}
pub(super) fn evaluate(
    access: &RuntimeAccess,
    args: Value,
    names_symbol: Value,
    mut intern: impl FnMut(&str) -> Result<Value>,
) -> Result<Value> {
    let [model, influence, residuals] = arguments(access, args)?;
    let model = model.ok_or("argument \"model\" is missing, with no default")?;
    let domain = access.domain();
    // Capture owning children before any element/provider/allocation callback.
    let model_residuals = field(access, &model, b"residuals")?;
    let rank = field(access, &model, b"rank")?;
    let cached_hat = field(access, &model, b"hat")?;
    let qr = field(access, &model, b"qr")?;
    let model_names = attribute(&model_residuals, b"names")?;
    let glm = is_glm(access, &model)?;
    let family = if glm {
        field(access, &model, b"family")?
    } else {
        domain.nil()
    };
    let arguments_residual_default = residuals.is_none();
    let default_needed = arguments_residual_default || influence.is_none();
    let omitted = if default_needed {
        rows::Rows::capture(access, field(access, &model, b"na.action")?)?
    } else {
        None
    };
    let df_residual = if influence.is_none() && omitted.is_some() {
        field(access, &model, b"df.residual")?
    } else {
        domain.nil()
    };
    let weights = if default_needed {
        field(access, &model, b"weights")?
    } else {
        domain.nil()
    };
    let drop_weights = if glm && default_needed {
        field(access, &model, b"prior.weights")?
    } else {
        weights.clone()
    };
    let fixed_sigma = if glm {
        fixed_glm_sigma(access, &family)?
    } else {
        None
    };
    let res_names = if let Some(res) = &residuals {
        attribute(res, b"names")?
    } else {
        model_names.clone()
    };
    // A row-map warning can invoke user handlers. Keep the exact selected
    // influence children before default residual preprocessing reaches it.
    let supplied_influence = if let Some(influence) = &influence {
        Some((
            field(access, influence, b"hat")?,
            if fixed_sigma.is_none() {
                field(access, influence, b"sigma")?
            } else {
                domain.nil()
            },
        ))
    } else {
        None
    };
    let default_residuals = if default_needed {
        Some(weighted_residuals(
            access,
            &model_residuals,
            &model_names,
            &weights,
            &drop_weights,
            omitted.as_ref(),
            &mut intern,
        )?)
    } else {
        None
    };
    let p = numeric(access, &rank)?
        .first()
        .copied()
        .ok_or("invalid model rank")?;
    // Supplied influence/residuals must not invoke an unused residual provider.
    let mut n = model_residuals.len() as f64;
    let qr_matrix = if qr.is_nil() {
        domain.nil()
    } else {
        field(access, &qr, b"qr")?
    };
    if !qr_matrix.is_nil() {
        let dimensions = attribute(&qr_matrix, b"dim")?;
        let dims = numeric(access, &dimensions)?;
        if dims.len() != 2 || dims[0] < 0. || dims[0].fract() != 0. {
            return Err("invalid model QR matrix".into());
        }
        n = dims[0];
    }
    let retained: Vec<_> = if influence.is_none() {
        default_residuals
            .as_ref()
            .ok_or("missing default influence residuals")?
            .values
            .iter()
            .enumerate()
            .filter_map(|(i, v)| (!v.is_nan()).then_some(i))
            .collect()
    } else {
        Vec::new()
    };
    let e = if influence.is_none() {
        let values = &default_residuals
            .as_ref()
            .ok_or("missing default influence residuals")?
            .values;
        retained.iter().map(|i| values[*i]).collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let (hat_value, sigma_value) = if let Some(values) = supplied_influence {
        values
    } else if qr_matrix.is_nil() {
        (cached_hat.clone(), domain.nil())
    } else {
        (domain.nil(), domain.nil())
    };
    let hat_names = attribute(&hat_value, b"names")?;
    let mut hat = numeric(access, &hat_value)?;
    let mut sigma = if influence.is_some() {
        if let Some(sigma) = fixed_sigma {
            vec![sigma]
        } else {
            numeric(access, &sigma_value)?
        }
    } else if !qr_matrix.is_nil() {
        let aux = field(access, &qr, b"qraux")?;
        let qr_rank = field(access, &qr, b"rank")?;
        let dimensions = attribute(&qr_matrix, b"dim")?;
        let dims = numeric(access, &dimensions)?;
        let matrix = numeric(access, &qr_matrix)?;
        let aux = numeric(access, &aux)?;
        let k = numeric(access, &qr_rank)?
            .first()
            .copied()
            .ok_or("invalid model QR rank")?;
        let count = n as usize;
        if k < 0. || k.fract() != 0. || k > n || dims[1] < k || dims[1].fract() != 0. {
            return Err("invalid model QR matrix".into());
        }
        let k = k as usize;
        if e.len() != count || aux.len() < k || matrix.len() < count.saturating_mul(k) {
            return Err("invalid model QR matrix".into());
        }
        hat = vec![0.; count];
        let mut sigma = vec![0.; count];
        let residuals = cleaned_residuals(&e);
        crate::library::stats::influence::lminfl_compute(
            &matrix,
            count,
            count,
            k,
            1,
            &aux,
            &residuals,
            &mut hat,
            &mut sigma,
            10. * f64::EPSILON,
        );
        sigma
    } else {
        if hat.len() != e.len() {
            return Err("non-NA residual length does not match cases used in fitting".into());
        }
        for value in &mut hat {
            if *value > 1. - 10. * f64::EPSILON {
                *value = 1.;
            }
        }
        deletion_sigma(&e, &hat, n, p)
    };
    let hat_names = if influence.is_none() {
        let snapshot = default_residuals
            .as_ref()
            .ok_or("missing default influence residuals")?;
        let names = rows::select_names(access, &snapshot.names, snapshot.values.len(), &retained)?;
        if let Some(rows) = omitted.as_ref() {
            let cleaned = cleaned_residuals(&e);
            let df = numeric(access, &df_residual)?
                .first()
                .copied()
                .ok_or("invalid residual degrees of freedom")?;
            let fallback = (cleaned.iter().map(|v| v * v).sum::<f64>() / df).sqrt();
            let names = rows.names(access, &names, e.len())?;
            hat = rows
                .restore(access, &hat, &mut intern)?
                .into_iter()
                .map(|v| if v.is_nan() { 0. } else { v })
                .collect();
            sigma = rows
                .restore(access, &sigma, &mut intern)?
                .into_iter()
                .map(|v| if v.is_nan() { fallback } else { v })
                .collect();
            names
        } else {
            names
        }
    } else {
        hat_names
    };
    let sigma_names = if influence.is_some() && fixed_sigma.is_none() {
        attribute(&sigma_value, b"names")?
    } else if influence.is_none() && fixed_sigma.is_none() {
        hat_names.clone()
    } else {
        domain.nil()
    };
    let sigma = fixed_sigma.map_or(sigma, |sigma| vec![sigma]);
    let residuals = if let Some(res) = &residuals {
        numeric(access, res)?
    } else {
        default_residuals
            .as_ref()
            .ok_or("missing default residuals")?
            .values
            .clone()
    };
    let res_names =
        if residuals.len() > 0 && default_residuals.is_some() && arguments_residual_default {
            default_residuals
                .as_ref()
                .ok_or("missing default residuals")?
                .names
                .clone()
        } else {
            res_names
        };
    // Follow the three GNU vector operations independently: denominator,
    // studentized residual, and final product. Their recycling and left-hand
    // name precedence differ when only one default argument restores rows.
    let denominator_length = binary_length(sigma.len(), hat.len());
    warn_recycling(
        access,
        sigma.len(),
        hat.len(),
        condition_calls::Phase::Denominator,
        &mut intern,
    )?;
    let denominator_names = binary_names(access, &sigma_names, sigma.len(), &hat_names, hat.len());
    let star_length = binary_length(residuals.len(), denominator_length);
    warn_recycling(
        access,
        residuals.len(),
        denominator_length,
        condition_calls::Phase::Studentized {
            fixed_dispersion: fixed_sigma.is_some(),
        },
        &mut intern,
    )?;
    let star_names = binary_names(
        access,
        &res_names,
        residuals.len(),
        &denominator_names,
        denominator_length,
    );
    let length = binary_length(hat.len(), star_length);
    warn_recycling(
        access,
        hat.len(),
        star_length,
        condition_calls::Phase::Product,
        &mut intern,
    )?;
    let names = binary_names(access, &hat_names, hat.len(), &star_names, star_length);
    let mut values = Vec::with_capacity(length);
    for i in 0..length {
        let star_index = i % star_length;
        let denominator_index = star_index % denominator_length;
        let denominator_omh = 1. - hat[denominator_index % hat.len()];
        let residual = residuals[star_index % residuals.len()];
        let deviation = sigma[denominator_index % sigma.len()];
        let omh = 1. - hat[i % hat.len()];
        if [residual, deviation, denominator_omh, omh]
            .into_iter()
            .any(crate::sexp::ffi::is_na_real)
        {
            values.push(crate::sexp::ffi::NA_REAL);
            continue;
        }
        let mut star = residual / (deviation * denominator_omh.sqrt());
        if star.is_infinite() {
            star = f64::NAN;
        }
        values.push(1. / (omh * ((n - p - 1. + star * star) / (n - p)).powf(p)));
    }
    let allocator = checked(access.allocator(&domain))?;
    let attributes = if names.is_nil() {
        domain.nil()
    } else {
        checked(allocator.pairlist_cell(&names, &domain.nil(), &names_symbol))?
    };
    let attributes_link = checked(domain.link(&attributes))?;
    active(access)?;
    let result = checked(allocator.allocate(|arena| {
        let pointer = arena.alloc_vector(SEXPTYPE::REALSXP, length as i64);
        let node = arena.node_token(pointer)?;
        let heap = arena.heap_identity();
        if !values.is_empty() {
            let payload = heap.payload_lease(&node)?;
            for (index, value) in values.iter().copied().enumerate() {
                payload.set_real_elt(index, value)?;
            }
        }
        let mut header = heap.node_snapshot(&node)?;
        header.attrib = attributes_link;
        heap.replace_node(&node, header)?;
        Some(pointer)
    }))?;
    active(access)?;
    Ok(result)
}

/// Model-frame labels come from data rows, then response names, then 1:n.
pub(super) fn row_labels(
    access: &RuntimeAccess,
    data: Option<Value>,
    response: Value,
) -> Result<Vec<String>> {
    let length = response.len();
    let supplied = if let Some(data) = data {
        attribute(&data, b"row.names")?
    } else {
        access.domain().nil()
    };
    let supplied = if supplied.is_nil() {
        attribute(&response, b"names")?
    } else {
        supplied
    };
    if supplied.typeof_() == SEXPTYPE::STRSXP && supplied.len() == length {
        let mut names = Vec::new();
        for index in 0..length {
            names.push(
                checked(supplied.try_string_value_elt(index))?.ok_or("missing model row name")?,
            );
            active(access)?;
        }
        Ok(names)
    } else {
        Ok((1..=length).map(|index| index.to_string()).collect())
    }
}
