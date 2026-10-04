//! Checked packed-model prediction from pinned GNU ppr.f, without COMMON state.
#![forbid(unsafe_code)]
mod sort;
use std::fmt;

#[derive(Debug)]
pub(crate) struct Error(&'static str);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Shape {
    pub(crate) predictors: usize,
    pub(crate) responses: usize,
    pub(crate) observations: usize,
    pub(crate) used: usize,
    means_end: usize,
    coefficients: usize,
    curves: usize,
    projections: usize,
}
fn count(x: f64) -> Result<usize, Error> {
    let value = (x + 0.1).trunc();
    if !value.is_finite() || value < 0. || value > f64::from(i32::MAX) {
        return Err(Error("invalid packed PPR model dimension"));
    }
    Ok(value as usize)
}
fn add(a: usize, b: usize) -> Result<usize, Error> {
    a.checked_add(b)
        .ok_or(Error("packed PPR model extent overflow"))
}
fn mul(a: usize, b: usize) -> Result<usize, Error> {
    a.checked_mul(b)
        .ok_or(Error("packed PPR model extent overflow"))
}
impl Shape {
    pub(crate) fn read(model: &[f64]) -> Result<Self, Error> {
        if model.len() < 5 {
            return Err(Error("packed PPR model needs its five dimensions"));
        }
        let terms = count(model[0])?;
        let predictors = count(model[1])?;
        let responses = count(model[2])?;
        let observations = count(model[3])?;
        let used = count(model[4])?;
        if used > terms || used > 0 && observations == 0 {
            return Err(Error("invalid packed PPR curve count"));
        }
        let means_end = add(responses, 5)?;
        let base = add(means_end, 1)?;
        if model.len() < base {
            return Err(Error("packed PPR model needs its means and response scale"));
        }
        let mut shape = Self {
            predictors,
            responses,
            observations,
            used,
            means_end,
            coefficients: base,
            curves: base,
            projections: base,
        };
        if used > 0 {
            shape.coefficients = add(base, mul(predictors, terms)?)?;
            shape.curves = add(shape.coefficients, mul(responses, terms)?)?;
            shape.projections = add(shape.curves, mul(observations, terms)?)?;
            let end = add(shape.projections, mul(observations, used)?)?;
            if model.len() < end {
                return Err(Error("packed PPR model curve storage is too short"));
            }
            if !model[..end].iter().all(|x| x.is_finite()) {
                return Err(Error("nonfinite packed PPR model"));
            }
        } else if !model[5..base].iter().all(|x| x.is_finite()) {
            return Err(Error("nonfinite packed PPR means or response scale"));
        }
        Ok(shape)
    }
    pub(crate) fn validate(
        self,
        np: usize,
        x: usize,
        y: usize,
        scratch: usize,
    ) -> Result<(), Error> {
        if y < mul(np, self.responses)? {
            return Err(Error("PPR prediction output is too short"));
        }
        if self.used > 0 {
            if x < mul(np, self.predictors)? {
                return Err(Error("PPR prediction input is too short"));
            }
            if scratch < mul(2, self.observations)? {
                return Err(Error("PPR sorting workspace is too short"));
            }
        }
        Ok(())
    }
    pub(crate) fn validate_projections(
        self,
        np: usize,
        x: &[f64],
        model: &[f64],
    ) -> Result<(), Error> {
        // An unordered projection cannot enter GNU's binary interpolation loop.
        // Check even overflow-generated NaN before sorting or output writes.
        for input in 0..np {
            for term in 0..self.used {
                let mut projection = 0.;
                for j in 0..self.predictors {
                    projection +=
                        model[self.means_end + 1 + term * self.predictors + j] * x[input + np * j];
                }
                if projection.is_nan() {
                    return Err(Error("unordered PPR prediction projection"));
                }
            }
        }
        Ok(())
    }
}
pub(crate) fn predict(
    np: usize,
    x: &[f64],
    model: &mut [f64],
    y: &mut [f64],
    scratch: &mut [f64],
) -> Result<(), Error> {
    let shape = Shape::read(model)?;
    shape.validate(np, x.len(), y.len(), scratch.len())?;
    shape.validate_projections(np, x, model)?;
    let mut stack = Vec::new();
    if shape.used > 0 {
        stack
            .try_reserve_exact(shape.observations.ilog2() as usize + 2)
            .map_err(|_| Error("PPR sorting stack allocation failed"))?;
        let n = shape.observations;
        let (permutation, original_curve) = scratch[..2 * n].split_at_mut(n);
        for term in 0..shape.used {
            let curve = shape.curves + term * n;
            let projection = shape.projections + term * n;
            for j in 0..n {
                permutation[j] = (j + 1) as f64 + 0.1;
                original_curve[j] = model[curve + j];
            }
            sort::sort(
                &mut model[projection..projection + n],
                permutation,
                &mut stack,
            );
            for j in 0..n {
                model[curve + j] = original_curve[permutation[j] as usize - 1];
            }
        }
    }
    let scale = model[shape.means_end];
    for input in 0..np {
        for response in 0..shape.responses {
            y[input + np * response] = 0.;
        }
        for term in 0..shape.used {
            let mut projection = 0.;
            for j in 0..shape.predictors {
                projection +=
                    model[shape.means_end + 1 + term * shape.predictors + j] * x[input + np * j];
            }
            let first = shape.projections + term * shape.observations;
            let curve = shape.curves + term * shape.observations;
            let last = shape.observations - 1;
            let value = if projection <= model[first] {
                model[curve]
            } else if projection >= model[first + last] {
                model[curve + last]
            } else {
                let mut low = 0;
                let mut high = shape.observations + 1;
                loop {
                    if low + 1 >= high {
                        let lo = low - 1;
                        let hi = high - 1;
                        break model[curve + lo]
                            + (model[curve + hi] - model[curve + lo])
                                * (projection - model[first + lo])
                                / (model[first + hi] - model[first + lo]);
                    }
                    let place = (low + high) / 2;
                    let t = model[first + place - 1];
                    if projection == t {
                        break model[curve + place - 1];
                    }
                    if projection < t {
                        high = place;
                    } else {
                        low = place;
                    }
                }
            };
            for response in 0..shape.responses {
                y[input + np * response] +=
                    model[shape.coefficients + term * shape.responses + response] * value;
            }
        }
        for response in 0..shape.responses {
            y[input + np * response] = scale * y[input + np * response] + model[5 + response];
        }
    }
    Ok(())
}
