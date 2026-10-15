/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2023   Torsten Hothorn
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program; if not, a copy is available at
 *  https://www.R-project.org/Licenses/
 */

/**
    Exact Distribution of Two-Sample Permutation Tests
    Streitberg and Rohmel Shift Algorithm

    Ported from r-source/src/library/stats/src/permdist.c
*/
use crate::sexp::accessors::{INTEGER_ELT, REAL, TYPEOF, XLENGTH};
use crate::sexp::constructors::Rf_allocVector3;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::protect::protect;

const TWO_SAMPLE_OVERFLOW: &str = "overflow error; cannot compute exact distribution";
const ONE_SAMPLE_OVERFLOW: &str = "overflow error: cannot compute exact distribution";

fn workspace(length: usize) -> Result<Vec<f64>, &'static str> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(length)
        .map_err(|_| "cannot allocate permutation distribution workspace")?;
    values.resize(length, 0.0);
    Ok(values)
}

fn normalized(mut values: Vec<f64>, error: &'static str) -> Result<Vec<f64>, &'static str> {
    let mut total = 0.0;
    for &value in &values {
        if !value.is_finite() {
            return Err(error);
        }
        total += value;
    }
    if !total.is_finite() || total == 0.0 {
        return Err(error);
    }
    for value in &mut values {
        *value /= total;
    }
    Ok(values)
}

/// Streitberg and Rohmel shift recurrence. All offsets address owned storage;
/// the C recurrence's negative intermediate `idx2` is never constructed.
fn two_sample(
    scores: &[usize],
    m: usize,
    mut interrupt: impl FnMut(),
) -> Result<Vec<f64>, &'static str> {
    if m > scores.len() {
        return Err("invalid permutation sample size");
    }
    let limit = scores[scores.len() - m..]
        .iter()
        .try_fold(0usize, |sum, &score| sum.checked_add(score))
        .ok_or(TWO_SAMPLE_OVERFLOW)?;
    let stride = limit.checked_add(1).ok_or(TWO_SAMPLE_OVERFLOW)?;
    let rows = m.checked_add(1).ok_or(TWO_SAMPLE_OVERFLOW)?;
    let mut counts = workspace(rows.checked_mul(stride).ok_or(TWO_SAMPLE_OVERFLOW)?)?;
    counts[0] = 1.0;
    let mut prefix = 0usize;
    let mut remaining = 10000;
    for (k, &score) in scores.iter().enumerate() {
        prefix = prefix.checked_add(score).ok_or(TWO_SAMPLE_OVERFLOW)?;
        let highest = limit.min(prefix);
        if score > highest {
            continue;
        }
        for i in (1..=m.min(k + 1)).rev() {
            let row = i * stride;
            let previous = (i - 1) * stride;
            for j in (score..=highest).rev() {
                remaining -= 1;
                if remaining == 0 {
                    interrupt();
                    remaining = 10000;
                }
                counts[row + j] += counts[previous + (j - score)];
            }
        }
    }
    // GNU excludes column zero for the independent two-sample distribution.
    let start = m * stride + 1;
    counts.drain(..start);
    normalized(counts, TWO_SAMPLE_OVERFLOW)
}

fn one_sample(scores: &[usize], mut interrupt: impl FnMut()) -> Result<Vec<f64>, &'static str> {
    let total = scores
        .iter()
        .try_fold(0usize, |sum, &score| sum.checked_add(score))
        .ok_or(ONE_SAMPLE_OVERFLOW)?;
    let mut counts = workspace(total.checked_add(1).ok_or(ONE_SAMPLE_OVERFLOW)?)?;
    counts[0] = 1.0;
    let mut prefix = 0usize;
    let mut remaining = 10000;
    for &score in scores {
        prefix += score; // bounded by the checked total
        for i in (score..=prefix).rev() {
            remaining -= 1;
            if remaining == 0 {
                interrupt();
                remaining = 10000;
            }
            counts[i] += counts[i - score];
        }
    }
    normalized(counts, ONE_SAMPLE_OVERFLOW)
}

fn permutation_error(message: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: message.to_owned(),
    })
}

unsafe fn scores(x: SEXP) -> Vec<usize> {
    unsafe {
        if x.is_null() || TYPEOF(x) != SEXPTYPE::INTSXP {
            permutation_error("permutation scores must be integer");
        }
        let n = i32::try_from(XLENGTH(x))
            .unwrap_or_else(|_| permutation_error("invalid permutation scores"));
        let mut scores = Vec::new();
        scores
            .try_reserve_exact(n as usize)
            .unwrap_or_else(|_| permutation_error("cannot allocate permutation score workspace"));
        for i in 0..n {
            scores.push(
                usize::try_from(INTEGER_ELT(x, i)).unwrap_or_else(|_| {
                    permutation_error("permutation scores must be nonnegative")
                }),
            );
        }
        scores
    }
}

unsafe fn publish(values: Vec<f64>) -> SEXP {
    unsafe {
        let length = i64::try_from(values.len())
            .unwrap_or_else(|_| permutation_error("permutation distribution is too large"));
        let result = Rf_allocVector3(SEXPTYPE::REALSXP, length);
        if !values.is_empty() {
            std::ptr::copy_nonoverlapping(values.as_ptr(), REAL(result), values.len());
        }
        result
    }
}

pub unsafe fn dpermdist2(x: SEXP, m: SEXP) -> SEXP {
    unsafe {
        let _x = protect(x);
        let _m = protect(m);
        let scores = scores(x);
        if m.is_null() || TYPEOF(m) != SEXPTYPE::INTSXP || XLENGTH(m) != 1 {
            permutation_error("invalid permutation sample size");
        }
        let m = usize::try_from(INTEGER_ELT(m, 0))
            .unwrap_or_else(|_| permutation_error("invalid permutation sample size"));
        let values = two_sample(&scores, m, || crate::main::errors::R_CheckUserInterrupt())
            .unwrap_or_else(|message| permutation_error(message));
        publish(values)
    }
}

pub unsafe fn dpermdist1(x: SEXP) -> SEXP {
    unsafe {
        let _x = protect(x);
        let values = one_sample(&scores(x), || crate::main::errors::R_CheckUserInterrupt())
            .unwrap_or_else(|message| permutation_error(message));
        publish(values)
    }
}

pub unsafe extern "C-unwind" fn c_dpermdist1(x: SEXP) -> SEXP {
    unsafe { dpermdist1(x) }
}
pub unsafe extern "C-unwind" fn c_dpermdist2(x: SEXP, m: SEXP) -> SEXP {
    unsafe { dpermdist2(x, m) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permutation_two_sample_matches_enumerated_tied_and_untied_ranks() {
        // Six subsets of size two, with sums 3,4,5,5,6,7.
        assert_eq!(
            two_sample(&[1, 2, 3, 4], 2, || {}).unwrap(),
            vec![
                0.0,
                0.0,
                1.0 / 6.0,
                1.0 / 6.0,
                2.0 / 6.0,
                1.0 / 6.0,
                1.0 / 6.0
            ]
        );
        assert_eq!(
            two_sample(&[1, 1, 3, 3], 2, || {}).unwrap(),
            vec![0.0, 1.0 / 6.0, 0.0, 4.0 / 6.0, 0.0, 1.0 / 6.0]
        );
        assert_eq!(two_sample(&[0, 1, 2], 1, || {}).unwrap(), vec![0.5, 0.5]);
        assert_eq!(
            two_sample(&[1, 2], 3, || {}),
            Err("invalid permutation sample size")
        );
    }

    #[test]
    fn permutation_one_sample_matches_subset_counts_and_zero_scores() {
        assert_eq!(
            one_sample(&[1, 2, 3], || {}).unwrap(),
            vec![0.125, 0.125, 0.125, 0.25, 0.125, 0.125, 0.125]
        );
        assert_eq!(
            one_sample(&[0, 1, 2, 3], || {}).unwrap(),
            one_sample(&[1, 2, 3], || {}).unwrap()
        );
        assert_eq!(one_sample(&[], || {}).unwrap(), vec![1.0]);
        assert_eq!(one_sample(&[usize::MAX], || {}), Err(ONE_SAMPLE_OVERFLOW));
    }

    #[test]
    fn permutation_owned_workspace_preserves_interrupts_and_overflow_errors() {
        let mut interrupts = 0;
        one_sample(&vec![1; 150], || interrupts += 1).unwrap();
        assert!(interrupts > 0);
        interrupts = 0;
        two_sample(&vec![1; 150], 75, || interrupts += 1).unwrap();
        assert!(interrupts > 0);
        assert_eq!(one_sample(&vec![0; 1024], || {}), Err(ONE_SAMPLE_OVERFLOW));
        assert_eq!(
            two_sample(&[usize::MAX], 1, || {}),
            Err(TWO_SAMPLE_OVERFLOW)
        );
    }
}
