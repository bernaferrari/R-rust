#![forbid(unsafe_code)]
//! Owning GNU covariance-ratio inputs and callback-free arithmetic.
use crate::sexp::{ffi::SEXPTYPE, object::Sexp, owner::RuntimeAccess};

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
pub(super) fn evaluate(access: &RuntimeAccess, args: Value, names_symbol: Value) -> Result<Value> {
    let [model, influence, residuals] = arguments(access, args)?;
    let model = model.ok_or("argument \"model\" is missing, with no default")?;
    let domain = access.domain();
    // Capture owning children before any element/provider/allocation callback.
    let model_residuals = field(access, &model, b"residuals")?;
    let rank = field(access, &model, b"rank")?;
    let cached_hat = field(access, &model, b"hat")?;
    let qr = field(access, &model, b"qr")?;
    let res = residuals.unwrap_or_else(|| model_residuals.clone());
    let res_names = attribute(&res, b"names")?;
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
    let e = if influence.is_none() {
        numeric(access, &model_residuals)?
    } else {
        Vec::new()
    };
    let (hat_value, sigma_value) = if let Some(influence) = &influence {
        (
            field(access, influence, b"hat")?,
            field(access, influence, b"sigma")?,
        )
    } else if qr_matrix.is_nil() {
        (cached_hat.clone(), domain.nil())
    } else {
        (domain.nil(), domain.nil())
    };
    let hat_names = attribute(&hat_value, b"names")?;
    let mut hat = numeric(access, &hat_value)?;
    let sigma = if influence.is_some() {
        numeric(access, &sigma_value)?
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
    let residuals = numeric(access, &res)?;
    let length = if hat.is_empty() || sigma.is_empty() || residuals.is_empty() {
        0
    } else {
        hat.len().max(sigma.len()).max(residuals.len())
    };
    let names = if res_names.len() == length as i64 && length > 0 {
        res_names
    } else if hat_names.len() == length as i64 && length > 0 {
        hat_names
    } else {
        domain.nil()
    };
    let mut values = Vec::with_capacity(length);
    for i in 0..length {
        let omh = 1. - hat[i % hat.len()];
        let mut star = residuals[i % residuals.len()] / (sigma[i % sigma.len()] * omh.sqrt());
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
