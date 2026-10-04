//! Checked, fallible GNU STL decomposition without raw buffer projections.
#![forbid(unsafe_code)]
mod helpers;

#[derive(Clone, Copy)]
pub(crate) struct Parameters {
    pub n: usize,
    pub period: usize,
    spans: [usize; 3],
    degrees: [i32; 3],
    jumps: [i32; 3],
    pub inner: usize,
    pub outer: usize,
}
#[derive(Clone, Copy)]
pub(crate) struct Lengths {
    pub y: usize,
    pub weights: usize,
    pub season: usize,
    pub trend: usize,
}
pub(crate) struct Output<'a> {
    pub weights: &'a mut [f64],
    pub season: &'a mut [f64],
    pub trend: &'a mut [f64],
}
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Error {
    Overflow,
    Allocation,
    Period,
    Jump,
    EmptyRobust,
    Short(&'static str, usize, usize),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Overflow => f.write_str("STL workspace size overflow"),
            Self::Allocation => f.write_str("cannot allocate STL workspace"),
            Self::Period => {
                f.write_str("STL needs at least one full period for positive inner iterations")
            }
            Self::Jump => f.write_str("STL smoothing jumps must be positive when used"),
            Self::EmptyRobust => f.write_str("STL robustness iterations require observations"),
            Self::Short(name, needed, actual) => {
                write!(f, "STL {name} needs {needed} elements, received {actual}")
            }
        }
    }
}
fn require(name: &'static str, actual: usize, needed: usize) -> Result<(), Error> {
    if actual < needed {
        Err(Error::Short(name, needed, actual))
    } else {
        Ok(())
    }
}
impl Parameters {
    pub fn new(
        n: usize,
        period: i32,
        spans: [i32; 3],
        degrees: [i32; 3],
        jumps: [i32; 3],
        inner: i32,
        outer: i32,
    ) -> Self {
        Self {
            n,
            period: period.max(2) as usize,
            spans: spans.map(|s| {
                let s = s.max(3) as usize;
                s + usize::from(s % 2 == 0)
            }),
            degrees,
            jumps,
            inner: inner.max(0) as usize,
            outer: outer.max(0) as usize,
        }
    }
    pub fn validate(self, lengths: Lengths) -> Result<usize, Error> {
        let width = self
            .period
            .checked_mul(2)
            .and_then(|p| self.n.checked_add(p))
            .ok_or(Error::Overflow)?;
        let work = width
            .checked_mul(5)
            .filter(|n| *n <= isize::MAX as usize / size_of::<f64>())
            .ok_or(Error::Overflow)?;
        if self.inner > 0 {
            if self.n < self.period {
                return Err(Error::Period);
            }
            if self.jumps.iter().any(|v| *v < 1) {
                return Err(Error::Jump);
            }
        }
        if self.outer > 0 && self.n == 0 {
            return Err(Error::EmptyRobust);
        }
        let reads = self.inner > 0 || self.outer > 0;
        require("input", lengths.y, if reads { self.n } else { 0 })?;
        require("season", lengths.season, if reads { self.n } else { 0 })?;
        require("weights", lengths.weights, self.n)?;
        require("trend", lengths.trend, self.n)?;
        Ok(work)
    }
}
pub(crate) fn filter(y: &[f64], p: Parameters, output: Output<'_>) -> Result<(), Error> {
    let size = p.validate(Lengths {
        y: y.len(),
        weights: output.weights.len(),
        season: output.season.len(),
        trend: output.trend.len(),
    })?;
    let mut work = Vec::new();
    work.try_reserve_exact(size)
        .map_err(|_| Error::Allocation)?;
    work.resize(size, 0.);
    // All indexing and allocation obligations are discharged before writes.
    let n = p.n;
    let trend = &mut output.trend[..n];
    let rw = &mut output.weights[..n];
    let reads = p.inner > 0 || p.outer > 0;
    let y = if reads { &y[..n] } else { &y[..0] };
    let season = if reads {
        &mut output.season[..n]
    } else {
        &mut output.season[..0]
    };
    trend.fill(0.);
    for iteration in 0..=p.outer {
        if p.inner > 0 {
            helpers::stlstp(
                y,
                p.period,
                p.spans[0],
                p.spans[1],
                p.spans[2],
                p.degrees[0],
                p.degrees[1],
                p.degrees[2],
                p.jumps[0] as usize,
                p.jumps[1] as usize,
                p.jumps[2] as usize,
                p.inner,
                iteration > 0,
                rw,
                season,
                trend,
                &mut work,
            );
        }
        if iteration == p.outer {
            break;
        }
        let width = n + 2 * p.period;
        let (fit, median) = work.split_at_mut(width);
        for i in 0..n {
            fit[i] = trend[i] + season[i];
        }
        helpers::stlrwt(y, &fit[..n], rw, &mut median[..n]);
    }
    if p.outer == 0 {
        rw.fill(1.);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stl_safe_kernel_rejects_malformed_workspaces_before_any_write() {
        let mut weights = [11.; 4];
        let mut season = [12.; 4];
        let mut trend = [13.; 4];
        let p = Parameters::new(usize::MAX, 2, [3; 3], [1; 3], [1; 3], 1, 0);
        assert_eq!(
            filter(
                &[],
                p,
                Output {
                    weights: &mut weights,
                    season: &mut season,
                    trend: &mut trend
                }
            ),
            Err(Error::Overflow)
        );
        let p = Parameters::new(4, 2, [3; 3], [1; 3], [1; 3], 1, 0);
        assert_eq!(
            filter(
                &[1.; 4],
                p,
                Output {
                    weights: &mut weights,
                    season: &mut season[..3],
                    trend: &mut trend
                }
            ),
            Err(Error::Short("season", 4, 3))
        );
        assert_eq!(weights, [11.; 4]);
        assert_eq!(season, [12.; 4]);
        assert_eq!(trend, [13.; 4]);
    }
}
