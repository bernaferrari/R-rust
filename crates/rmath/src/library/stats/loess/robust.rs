//! Owned robustness weights and pseudovalues from GNU loessf.f.
//! The caller-visible permutation workspace uses the original partial selection
//! moves and tie order, rather than a complete or stable sort.
#![forbid(unsafe_code)]

fn select(
    values: &[f64],
    indices: &mut [usize],
    mut left: usize,
    mut right: usize,
    k: usize,
) -> Result<(), &'static str> {
    while left < right {
        let pivot = values[indices[k]];
        let mut i = left;
        let mut j = right;
        indices.swap(left, k);
        if pivot < values[indices[right]] {
            indices.swap(left, right);
        }
        while i < j {
            indices.swap(i, j);
            i += 1;
            j -= 1;
            while i <= right && values[indices[i]] < pivot {
                i += 1;
            }
            while pivot < values[indices[j]] {
                j = j.checked_sub(1).ok_or("invalid LOESS median partition")?;
            }
        }
        if values[indices[left]] == pivot {
            indices.swap(left, j);
        } else {
            j += 1;
            if j > right {
                return Err("invalid LOESS median partition");
            }
            indices.swap(right, j);
        }
        if j <= k {
            left = j + 1;
        }
        if k <= j {
            let Some(previous) = j.checked_sub(1) else {
                break;
            };
            right = previous;
        }
    }
    Ok(())
}

fn median_order(values: &[f64]) -> Result<Vec<usize>, &'static str> {
    let n = values.len();
    let mut indices: Vec<_> = (0..n).collect();
    if n == 0 {
        return Ok(indices);
    }
    let middle = n / 2;
    select(values, &mut indices, 0, n - 1, middle)?;
    if n % 2 == 0 {
        select(values, &mut indices, 0, middle - 1, middle - 1)?;
    }
    Ok(indices)
}

fn workspace(indices: &[usize]) -> Result<Vec<i32>, &'static str> {
    indices
        .iter()
        .map(|&i| i32::try_from(i + 1).map_err(|_| "LOESS permutation is too large"))
        .collect()
}

pub(crate) fn weights(residuals: &[f64]) -> Result<(Vec<f64>, Vec<i32>), &'static str> {
    let mut values: Vec<_> = residuals.iter().map(|r| r.abs()).collect();
    let indices = median_order(&values)?;
    let n = values.len();
    if n == 0 {
        return Ok((values, workspace(&indices)?));
    }
    let middle = n / 2;
    let cmad = if n % 2 == 0 {
        3.0 * (values[indices[middle]] + values[indices[middle - 1]])
    } else {
        6.0 * values[indices[middle]]
    };
    for value in &mut values {
        *value = if cmad < f64::MIN_POSITIVE {
            1.0
        } else if cmad * 0.999 < *value {
            0.0
        } else if cmad * 0.001 < *value {
            (1.0 - (*value / cmad).powi(2)).powi(2)
        } else {
            1.0
        };
    }
    Ok((values, workspace(&indices)?))
}

pub(crate) fn pseudovalues(
    y: &[f64],
    fitted: &[f64],
    weights: &[f64],
    robust: &[f64],
) -> Result<(Vec<f64>, Vec<i32>), &'static str> {
    let n = y.len();
    if [fitted.len(), weights.len(), robust.len()]
        .iter()
        .any(|&length| length != n)
    {
        return Err("invalid LOESS pseudovalue dimensions");
    }
    let mut values: Vec<_> = (0..n)
        .map(|i| (y[i] - fitted[i]).abs() * weights[i].sqrt())
        .collect();
    let indices = median_order(&values)?;
    if n == 0 {
        return Ok((values, workspace(&indices)?));
    }
    let middle = n / 2;
    let mad = if n % 2 == 0 {
        (values[indices[middle - 1]] + values[indices[middle]]) / 2.0
    } else {
        values[indices[middle]]
    };
    let c = (6.0 * mad).powi(2) / 5.0;
    for i in 0..n {
        values[i] = (1.0 - (y[i] - fitted[i]).powi(2) * weights[i] / c) * robust[i].sqrt();
    }
    // GNU reduces from the final observation to the first. The order matters
    // for the scale, including zero-MAD NaN propagation.
    let mut sum = values[n - 1];
    for i in (0..n - 1).rev() {
        sum = values[i] + sum;
    }
    let scale = n as f64 / sum;
    for i in 0..n {
        values[i] = fitted[i] + (scale * robust[i]) * (y[i] - fitted[i]);
    }
    Ok((values, workspace(&indices)?))
}

#[cfg(test)]
#[allow(clippy::excessive_precision)] // Exact printed values from the pinned GNU oracle.
mod tests {
    use super::*;

    #[test]
    fn loess_robust_workspaces_match_pinned_gnu_partial_selection_and_ties() {
        for (residuals, permutation, expected) in [
            (
                vec![0., 1., 2.],
                vec![1, 2, 3],
                vec![1., 0.94521604938271597, 0.79012345679012352],
            ),
            (
                vec![3., 1., 4., 1., 5., 9., 2., 6.],
                vec![7, 2, 4, 1, 3, 5, 6, 8],
                vec![
                    0.95960016659725111,
                    0.99546999449817708,
                    0.92875396568302304,
                    0.99546999449817708,
                    0.88983499673489952,
                    0.66638900458142447,
                    0.98194168067831811,
                    0.84339858392336531,
                ],
            ),
            (vec![0., 0., 0., 0.], vec![1, 4, 3, 2], vec![1.; 4]),
            (
                vec![2., 1.],
                vec![2, 1],
                vec![0.90367322054564858, 0.97546105776558467],
            ),
        ] {
            let (actual, work) = weights(&residuals).unwrap();
            assert_eq!(work, permutation);
            for (actual, expected) in actual.into_iter().zip(expected) {
                assert!((actual - expected).abs() < 1e-15, "{actual} != {expected}");
            }
        }
    }

    #[test]
    fn loess_pseudovalue_workspace_and_zero_mad_match_pinned_gnu() {
        let y = [3., 1., 4., 1., 5., 9., 2., 6.];
        let fitted = [2., 2., 3., 2., 4., 8., 1., 5.];
        let weights = [1., 4., 1., 4., 1., 4., 1., 4.];
        let robust = [1., 0.5, 1., 0.5, 1., 0.5, 1., 0.5];
        let (actual, work) = pseudovalues(&y, &fitted, &weights, &robust).unwrap();
        assert_eq!(work, [5, 1, 7, 3, 2, 8, 6, 4]);
        let expected = [
            3.3598188707226502,
            1.3200905646386749,
            4.3598188707226502,
            1.3200905646386749,
            5.3598188707226502,
            8.679909435361326,
            2.3598188707226502,
            5.6799094353613251,
        ];
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 2e-15, "{actual} != {expected}");
        }
        let (actual, work) = pseudovalues(&y, &y, &weights, &robust).unwrap();
        assert_eq!(work, [6, 1, 7, 8, 2, 5, 3, 4]);
        assert!(actual.iter().all(|value| value.is_nan()));
    }
}
