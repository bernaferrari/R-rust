//! Default GNU running-linear supersmoother, without mutable PPR COMMON state.
//! Non-default PPR spline methods and trace output remain separate interfaces.
#![forbid(unsafe_code)]
mod smooth;
use smooth::{Plan, smooth};
const SPANS: [f64; 3] = [0.05, 0.2, 0.5];
#[derive(Clone, Copy)]
pub(crate) struct Parameters {
    pub n: usize,
    pub periodic: i32,
    pub span: f64,
    pub alpha: f64,
}
#[derive(Clone, Copy)]
pub(crate) struct Input<'a> {
    pub x: &'a [f64],
    pub y: &'a [f64],
    pub weights: &'a [f64],
}
pub(crate) struct Output<'a> {
    pub smoothed: &'a mut [f64],
    pub scratch: &'a mut [f64],
    pub edf: &'a mut [f64],
}
#[derive(Debug)]
pub(crate) struct Error(&'static str);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for Error {}
#[derive(Clone, Copy)]
enum Branch {
    Constant,
    Fixed(Plan),
    Automatic(Plan),
}
impl Parameters {
    fn plan(self, input: Input<'_>, lengths: [usize; 3]) -> Result<Branch, Error> {
        let n = self.n;
        if n == 0 || n > i32::MAX as usize {
            return Err(Error(
                "supersmoother requires a positive representable observation count",
            ));
        }
        if [
            input.x.len(),
            input.y.len(),
            input.weights.len(),
            lengths[0],
        ]
        .iter()
        .any(|&l| l < n)
        {
            return Err(Error("short supersmoother observation or output buffer"));
        }
        if input.x[..n]
            .iter()
            .chain(&input.y[..n])
            .chain(&input.weights[..n])
            .any(|x| !x.is_finite())
        {
            return Err(Error("supersmoother observations must be finite"));
        }
        if input.x[n - 1] <= input.x[0] {
            return Ok(Branch::Constant);
        }
        if n < 4 {
            return Err(Error(
                "nonconstant supersmoother requires at least four observations",
            ));
        }
        if input.x[..n].windows(2).any(|w| w[0] > w[1]) {
            return Err(Error("supersmoother abscissae must be ordered"));
        }
        let mut i = n / 4 - 1;
        let mut j = 3 * (n / 4) - 1;
        while input.x[j] <= input.x[i] {
            if j < n - 1 {
                j += 1;
            }
            i = i.saturating_sub(1);
        }
        let scale = 1e-3 * (input.x[j] - input.x[i]);
        let variance = scale * scale;
        let periodic = self.periodic == 2 && input.x[0] >= 0. && input.x[n - 1] <= 1.;
        if !self.span.is_finite() {
            return Err(Error("supersmoother span must be finite"));
        }
        let width = if self.span > 0. {
            n
        } else {
            n.checked_mul(7)
                .ok_or(Error("supersmoother workspace overflow"))?
        };
        if width > isize::MAX as usize / size_of::<f64>() || lengths[1] < width {
            return Err(Error("short or unrepresentable supersmoother workspace"));
        }
        let plan = Plan::new(
            n,
            if self.span > 0. { self.span } else { SPANS[0] },
            periodic,
            variance,
            true,
        )
        .ok_or(Error(
            "supersmoother periodic span exceeds its observation buffer",
        ))?;
        if self.span > 0. {
            Ok(Branch::Fixed(plan))
        } else {
            if lengths[2] < 1 {
                return Err(Error("missing supersmoother effective degrees output"));
            }
            Ok(Branch::Automatic(plan))
        }
    }
    pub(crate) fn validate(self, input: Input<'_>, lengths: [usize; 3]) -> Result<(), Error> {
        self.plan(input, lengths).map(|_| ())
    }
}
pub(crate) fn filter(input: Input<'_>, p: Parameters, output: Output<'_>) -> Result<(), Error> {
    let branch = p.plan(
        input,
        [
            output.smoothed.len(),
            output.scratch.len(),
            output.edf.len(),
        ],
    )?;
    let n = p.n;
    let input = Input {
        x: &input.x[..n],
        y: &input.y[..n],
        weights: &input.weights[..n],
    };
    match branch {
        Branch::Constant => {
            let (mut sy, mut sw) = (0., 0.);
            for (&y, &w) in input.y.iter().zip(input.weights) {
                sy += y * w;
                sw += w;
            }
            output.smoothed[..n].fill(if sw > 0. { sy / sw } else { 0. });
        }
        Branch::Fixed(plan) => smooth(
            input,
            plan,
            &mut output.smoothed[..n],
            &mut output.scratch[..n],
        ),
        Branch::Automatic(base) => {
            // The sole allocation precedes every caller-visible write; no temporary raw loans.
            let mut h = Vec::new();
            h.try_reserve_exact(n)
                .map_err(|_| Error("supersmoother workspace allocation failed"))?;
            h.resize(n, 0.);
            let (sc, selected) = output.scratch[..7 * n].split_at_mut(6 * n);
            for (i, &span) in SPANS.iter().enumerate() {
                let pair = &mut sc[2 * i * n..2 * (i + 1) * n];
                let (fit, error) = pair.split_at_mut(n);
                let plan = Plan::new(n, span, base.periodic, base.variance, true)
                    .expect("default spans bounded by validated observation count");
                smooth(input, plan, fit, selected);
                let plan = Plan::new(n, SPANS[1], base.periodic, base.variance, false)
                    .expect("default span bounded");
                smooth(
                    Input {
                        x: input.x,
                        y: selected,
                        weights: input.weights,
                    },
                    plan,
                    error,
                    &mut h,
                );
            }
            for j in 0..n {
                let mut minimum = 1e20;
                for (i, &span) in SPANS.iter().enumerate() {
                    let error = sc[(2 * i + 1) * n + j];
                    if error < minimum {
                        minimum = error;
                        selected[j] = span;
                    }
                }
                if p.alpha > 0. && p.alpha <= 10. && minimum < sc[5 * n + j] && minimum > 0. {
                    selected[j] += (SPANS[2] - selected[j])
                        * (minimum / sc[5 * n + j]).max(1e-7).powf(10. - p.alpha);
                }
            }
            let plan = Plan::new(n, SPANS[1], base.periodic, base.variance, false)
                .expect("default span bounded");
            smooth(
                Input {
                    x: input.x,
                    y: selected,
                    weights: input.weights,
                },
                plan,
                &mut sc[n..2 * n],
                &mut h,
            );
            let (left, right) = sc.split_at_mut(3 * n);
            let (target, right) = right.split_at_mut(n);
            for j in 0..n {
                left[n + j] = left[n + j].clamp(SPANS[0], SPANS[2]);
                let f = left[n + j] - SPANS[1];
                target[j] = if f < 0. {
                    let f = -f / (SPANS[1] - SPANS[0]);
                    (1. - f) * left[2 * n + j] + f * left[j]
                } else {
                    let f = f / (SPANS[2] - SPANS[1]);
                    (1. - f) * left[2 * n + j] + f * right[j]
                };
            }
            let plan = Plan::new(n, SPANS[0], base.periodic, base.variance, false)
                .expect("default span bounded");
            smooth(
                Input {
                    x: input.x,
                    y: target,
                    weights: input.weights,
                },
                plan,
                &mut output.smoothed[..n],
                &mut h,
            );
            output.edf[0] = 0.;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn supsmu_safe_kernel_rejects_before_mutating_caller_outputs() {
        let x = [0., 1., 2., 3.];
        let y = [1., 2., 3., 4.];
        let weights = [1.; 4];
        for p in [
            Parameters {
                n: 0,
                periodic: 1,
                span: 0.,
                alpha: 0.,
            },
            Parameters {
                n: 3,
                periodic: 1,
                span: 0.,
                alpha: 0.,
            },
            Parameters {
                n: 4,
                periodic: 1,
                span: 0.,
                alpha: 0.,
            },
        ] {
            let (mut smo, mut scratch, mut edf) = ([9.; 4], [8.; 27], [7.; 1]);
            assert!(
                filter(
                    Input {
                        x: &x,
                        y: &y,
                        weights: &weights
                    },
                    p,
                    Output {
                        smoothed: &mut smo,
                        scratch: &mut scratch,
                        edf: &mut edf
                    }
                )
                .is_err()
            );
            assert_eq!(smo, [9.; 4]);
            assert_eq!(scratch, [8.; 27]);
            assert_eq!(edf, [7.; 1]);
        }
    }
}
