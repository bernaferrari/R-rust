//! Shared BFGS recurrence for the R optimizer and package numerical routines.

use super::{ACCTOL, RELTEST, STEPREDN};
use crate::sexp::owner::OwnerToken;
use libm::fabs;

pub(crate) trait BfgsObjective {
    fn value(&mut self, point: &mut [f64]) -> Result<f64, &'static str>;
    fn gradient(&mut self, point: &mut [f64], slope: &mut [f64]) -> Result<(), &'static str>;
}

pub(crate) struct BfgsSettings {
    pub max_iterations: i32,
    pub trace: bool,
    pub report_interval: i32,
    pub absolute_tolerance: f64,
    pub relative_tolerance: f64,
}

pub(crate) struct BfgsResult {
    pub value: f64,
    pub function_count: i32,
    pub gradient_count: i32,
    pub failure: i32,
}

const WORKSPACE_LIMIT: &str = "BFGS workspace exceeds the session memory limit";

fn zeros(len: usize) -> Result<Vec<f64>, &'static str> {
    let mut buffer = Vec::new();
    buffer.try_reserve_exact(len).map_err(|_| WORKSPACE_LIMIT)?;
    buffer.resize(len, 0.0);
    Ok(buffer)
}

fn row_start(row: usize) -> usize {
    row * (row + 1) / 2
}

pub(crate) fn minimize_bfgs(
    b: &mut [f64],
    mask: &[i32],
    objective: &mut dyn BfgsObjective,
    settings: BfgsSettings,
    owner: Option<OwnerToken<'_>>,
) -> Result<BfgsResult, &'static str> {
    let BfgsSettings {
        max_iterations: maxit,
        trace,
        report_interval: nreport,
        absolute_tolerance: abstol,
        relative_tolerance: reltol,
    } = settings;
    if maxit <= 0 {
        return Ok(BfgsResult {
            value: objective.value(b)?,
            function_count: 0,
            gradient_count: 0,
            failure: 0,
        });
    }
    if nreport <= 0 {
        return Err(r#"REPORT must be > 0 (method = "BFGS")"#);
    }
    if b.len() != mask.len() {
        return Err("invalid BFGS parameter mask length");
    }

    let n = mask.iter().filter(|&&entry| entry != 0).count();
    let triangle = n
        .checked_add(1)
        .and_then(|next| n.checked_mul(next))
        .ok_or(WORKSPACE_LIMIT)?
        / 2;
    let numeric_values = n
        .checked_mul(3)
        .and_then(|count| count.checked_add(b.len()))
        .and_then(|count| count.checked_add(triangle))
        .ok_or(WORKSPACE_LIMIT)?;
    let bytes = numeric_values
        .checked_mul(size_of::<f64>())
        .and_then(|bytes| bytes.checked_add(n.checked_mul(size_of::<usize>())?))
        .ok_or(WORKSPACE_LIMIT)?;
    let _workspace = owner
        .map(|owner| {
            owner
                .with_arena(|arena| arena.reserve_workspace(bytes))
                .map_err(|_| "BFGS requires the active session")?
                .ok_or(WORKSPACE_LIMIT)
        })
        .transpose()?;
    let mut l = Vec::new();
    l.try_reserve_exact(n).map_err(|_| WORKSPACE_LIMIT)?;
    l.extend(
        mask.iter()
            .enumerate()
            .filter_map(|(i, &entry)| (entry != 0).then_some(i)),
    );

    let mut storage = zeros(numeric_values)?;
    let (g, rest) = storage.split_at_mut(b.len());
    let (t, rest) = rest.split_at_mut(n);
    let (x, rest) = rest.split_at_mut(n);
    let (c, bmat) = rest.split_at_mut(n);

    let mut f = objective.value(b)?;
    if !f.is_finite() {
        return Err("initial value in 'vmmin' is not finite");
    }
    if trace {
        eprintln!("initial  value {} ", f);
    }
    let mut fmin = f;
    let mut funcount: i32 = 1;
    let mut gradcount: i32 = 1;
    objective.gradient(b, g)?;
    let mut iter: i32 = 1;
    let mut ilast = gradcount;

    loop {
        if ilast == gradcount {
            bmat.fill(0.0);
            for i in 0..n {
                bmat[row_start(i) + i] = 1.0;
            }
        }
        for i in 0..n {
            x[i] = b[l[i]];
            c[i] = g[l[i]];
        }
        let mut gradproj = 0.0_f64;
        // Keep GNU's fused scalar BFGS arithmetic explicit so native and
        // Wasm searches take the same rounding path, independent of the
        // compiler's implicit floating-point contraction policy.
        for i in 0..n {
            let mut s = 0.0_f64;
            for j in 0..=i {
                s = (-bmat[row_start(i) + j]).mul_add(g[l[j]], s);
            }
            for j in (i + 1)..n {
                s = (-bmat[row_start(j) + i]).mul_add(g[l[j]], s);
            }
            t[i] = s;
            gradproj = s.mul_add(g[l[i]], gradproj);
        }

        let mut count: usize;
        if gradproj < 0.0 {
            // search direction is downhill
            let mut steplength = 1.0_f64;
            let mut accpoint = false;
            loop {
                count = 0;
                for i in 0..n {
                    b[l[i]] = steplength.mul_add(t[i], x[i]);
                    if RELTEST + x[i] == RELTEST + b[l[i]] {
                        count += 1;
                    }
                }
                if count < n {
                    f = objective.value(b)?;
                    funcount = funcount.saturating_add(1);
                    accpoint = f.is_finite() && f <= (gradproj * steplength).mul_add(ACCTOL, fmin);
                    if !accpoint {
                        steplength *= STEPREDN;
                    }
                }
                if count == n || accpoint {
                    break;
                }
            }

            let enough = f > abstol && fabs(f - fmin) > reltol * (fabs(fmin) + reltol);
            if !enough {
                count = n;
                fmin = f;
            }
            if count < n {
                // making progress
                fmin = f;
                objective.gradient(b, g)?;
                gradcount += 1;
                iter += 1;
                let mut d1 = 0.0_f64;
                for i in 0..n {
                    t[i] *= steplength;
                    c[i] = g[l[i]] - c[i];
                    d1 = t[i].mul_add(c[i], d1);
                }
                if d1 > 0.0 {
                    let mut d2 = 0.0_f64;
                    for i in 0..n {
                        let mut s = 0.0_f64;
                        for j in 0..=i {
                            s = bmat[row_start(i) + j].mul_add(c[j], s);
                        }
                        for j in (i + 1)..n {
                            s = bmat[row_start(j) + i].mul_add(c[j], s);
                        }
                        x[i] = s;
                        d2 = s.mul_add(c[i], d2);
                    }
                    let d2 = 1.0 + d2 / d1;
                    for i in 0..n {
                        for j in 0..=i {
                            let update = (d2 * t[i]).mul_add(t[j], -(x[i] * t[j]));
                            bmat[row_start(i) + j] += (-t[i]).mul_add(x[j], update) / d1;
                        }
                    }
                } else {
                    ilast = gradcount;
                }
            } else {
                // no progress
                if ilast < gradcount {
                    count = 0;
                    ilast = gradcount;
                }
            }
        } else {
            // uphill search
            count = 0;
            if ilast == gradcount {
                count = n;
            } else {
                ilast = gradcount;
            }
        }

        if trace && iter % nreport == 0 {
            eprintln!("iter{:4} value {}", iter, f);
        }
        if iter >= maxit {
            break;
        }
        if (gradcount - ilast) as usize > 2 * n {
            ilast = gradcount;
        }

        // A curvature restart can happen while the search still makes
        // progress. GNU stops only when the actual no-progress count
        // reaches every active parameter and the gradient is unchanged.
        if count == n && ilast == gradcount {
            break;
        }
    }

    if trace {
        eprintln!("final  value {} ", fmin);
        if iter < maxit {
            eprintln!("converged");
        } else {
            eprintln!("stopped after {} iterations", iter);
        }
    }
    Ok(BfgsResult {
        value: fmin,
        function_count: funcount,
        gradient_count: gradcount,
        failure: i32::from(iter >= maxit),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{RSession, memory::ArenaBudget};

    struct Quadratic;

    impl BfgsObjective for Quadratic {
        fn value(&mut self, point: &mut [f64]) -> Result<f64, &'static str> {
            Ok(point.iter().map(|&x| x * x).sum())
        }

        fn gradient(&mut self, point: &mut [f64], slope: &mut [f64]) -> Result<(), &'static str> {
            for (output, x) in slope.iter_mut().zip(point) {
                *output = 2.0 * *x;
            }
            Ok(())
        }
    }

    fn settings() -> BfgsSettings {
        BfgsSettings {
            max_iterations: 100,
            trace: false,
            report_interval: 10,
            absolute_tolerance: f64::NEG_INFINITY,
            relative_tolerance: 1e-8,
        }
    }

    #[test]
    fn bfgs_denies_workspace_before_evaluation_and_retries_with_a_sparse_mask() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let base = owner
            .with_arena(|arena| arena.total_bytes_allocated())
            .unwrap();
        let limit = base + 1024;
        owner
            .with_arena(|arena| arena.set_budget(ArenaBudget::new(limit, 0)))
            .unwrap();
        let mut point = [2.0; 32];
        let mut mask = [1; 32];
        assert_eq!(
            minimize_bfgs(&mut point, &mask, &mut Quadratic, settings(), Some(owner)).err(),
            Some(WORKSPACE_LIMIT),
        );
        assert_eq!(point, [2.0; 32]);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );

        mask[1..].fill(0);
        let result =
            minimize_bfgs(&mut point, &mask, &mut Quadratic, settings(), Some(owner)).unwrap();
        assert_eq!(result.failure, 0);
        assert!((result.value - 124.0).abs() < 1e-8);
        assert!(point[0].abs() < 1e-6);
        assert_eq!(point[1..], [2.0; 31]);
        assert_eq!(
            owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
            0
        );
        assert_eq!(
            owner.with_arena(|arena| arena.budget().max_bytes).unwrap(),
            limit
        );
    }
}
