//! GNU Holt-Winters filtering over borrowed, disjoint Rust slices.
#![forbid(unsafe_code)]

#[derive(Clone, Copy)]
pub(crate) struct Parameters {
    pub(crate) alpha: f64,
    pub(crate) beta: f64,
    pub(crate) gamma: f64,
    pub(crate) start_time: usize,
    pub(crate) period: usize,
    pub(crate) additive: bool,
    pub(crate) trend: bool,
    pub(crate) seasonal: bool,
}

pub(crate) struct Initial<'a> {
    pub(crate) level: f64,
    pub(crate) trend: f64,
    pub(crate) season: &'a [f64],
}

pub(crate) struct Output<'a> {
    pub(crate) sse: &'a mut [f64],
    pub(crate) level: &'a mut [f64],
    pub(crate) trend: &'a mut [f64],
    pub(crate) season: &'a mut [f64],
}

#[derive(Clone, Copy)]
pub(crate) struct Lengths {
    pub(crate) x: usize,
    pub(crate) sse: usize,
    pub(crate) seed: usize,
    pub(crate) level: usize,
    pub(crate) trend: usize,
    pub(crate) season: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FilterError {
    InvalidStart,
    WorkspaceOverflow,
    ShortBuffer {
        name: &'static str,
        needed: usize,
        actual: usize,
    },
}

impl std::fmt::Display for FilterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidStart => f.write_str("HoltWinters start_time must be at least one"),
            Self::WorkspaceOverflow => f.write_str("HoltWinters workspace size overflow"),
            Self::ShortBuffer {
                name,
                needed,
                actual,
            } => write!(
                f,
                "HoltWinters {name} needs at least {needed} elements, received {actual}"
            ),
        }
    }
}

fn workspace(a: usize, b: usize) -> Result<usize, FilterError> {
    a.checked_add(b)
        .filter(|n| *n <= isize::MAX as usize / std::mem::size_of::<f64>())
        .ok_or(FilterError::WorkspaceOverflow)
}

fn require(name: &'static str, actual: usize, needed: usize) -> Result<(), FilterError> {
    if actual < needed {
        Err(FilterError::ShortBuffer {
            name,
            needed,
            actual,
        })
    } else {
        Ok(())
    }
}

impl Parameters {
    /// Check every indexing obligation before any output is written. A start
    /// past the input is a valid initialization-only call, as in GNU R.
    pub(crate) fn validate(self, lengths: Lengths) -> Result<(), FilterError> {
        let start = self
            .start_time
            .checked_sub(1)
            .ok_or(FilterError::InvalidStart)?;
        let steps = lengths.x.saturating_sub(start);
        require("SSE", lengths.sse, usize::from(steps > 0))?;
        require("level", lengths.level, workspace(steps, 1)?)?;
        // GNU's level recurrence reads the previous trend even when trend
        // updates and forecasting are disabled. Preserve that buffer contract.
        let trend = if self.trend {
            workspace(steps, 1)?
        } else {
            steps
        };
        require("trend", lengths.trend, trend)?;
        if self.seasonal {
            require("season", lengths.season, workspace(steps, self.period)?)?;
            require("season seed", lengths.seed, self.period)?;
        }
        Ok(())
    }
}

pub(crate) fn filter(
    x: &[f64],
    parameters: Parameters,
    initial: Initial<'_>,
    output: Output<'_>,
) -> Result<(), FilterError> {
    parameters.validate(Lengths {
        x: x.len(),
        sse: output.sse.len(),
        seed: initial.season.len(),
        level: output.level.len(),
        trend: output.trend.len(),
        season: output.season.len(),
    })?;
    let Parameters {
        alpha,
        beta,
        gamma,
        start_time,
        period,
        additive,
        trend,
        seasonal,
    } = parameters;
    output.level[0] = initial.level;
    if trend {
        output.trend[0] = initial.trend;
    }
    if seasonal {
        output.season[..period].copy_from_slice(&initial.season[..period]);
    }
    let start = start_time - 1; // admitted above
    for (offset, &sample) in x.iter().skip(start).enumerate() {
        let i0 = offset + 1;
        let mut forecast = output.level[offset] + if trend { output.trend[offset] } else { 0.0 };
        let previous_season = if seasonal {
            output.season[offset]
        } else if additive {
            0.0
        } else {
            1.0
        };
        if additive {
            forecast += previous_season;
        } else {
            forecast *= previous_season;
        }
        let residual = sample - forecast;
        output.sse[0] += residual * residual;
        let deseasonalized = if additive {
            sample - previous_season
        } else {
            sample / previous_season
        };
        output.level[i0] =
            alpha * deseasonalized + (1.0 - alpha) * (output.level[offset] + output.trend[offset]);
        if trend {
            output.trend[i0] = beta * (output.level[i0] - output.level[offset])
                + (1.0 - beta) * output.trend[offset];
        }
        if seasonal {
            let update = if additive {
                sample - output.level[i0]
            } else {
                sample / output.level[i0]
            };
            output.season[offset + period] = gamma * update + (1.0 - gamma) * previous_season;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parameters() -> Parameters {
        Parameters {
            alpha: 0.3,
            beta: 0.1,
            gamma: 0.2,
            start_time: 1,
            period: 2,
            additive: true,
            trend: true,
            seasonal: true,
        }
    }

    #[test]
    fn holtwinters_checked_workspace_rejects_overflow_and_zero_start() {
        let lengths = Lengths {
            x: 4,
            sse: 1,
            seed: usize::MAX,
            level: 5,
            trend: 5,
            season: usize::MAX,
        };
        let mut p = parameters();
        p.period = usize::MAX;
        assert_eq!(p.validate(lengths), Err(FilterError::WorkspaceOverflow));
        p.period = 2;
        p.start_time = 0;
        assert_eq!(p.validate(lengths), Err(FilterError::InvalidStart));
    }

    #[test]
    fn holtwinters_short_workspace_leaves_all_outputs_unchanged() {
        let (mut sse, mut level, mut trend, mut season) = (7.0, [11.0; 5], [12.0; 5], [13.0; 5]);
        let result = filter(
            &[1.0; 4],
            parameters(),
            Initial {
                level: 2.0,
                trend: 3.0,
                season: &[1.0; 2],
            },
            Output {
                sse: std::slice::from_mut(&mut sse),
                level: &mut level,
                trend: &mut trend,
                season: &mut season,
            },
        );
        assert!(matches!(
            result,
            Err(FilterError::ShortBuffer { name: "season", .. })
        ));
        assert_eq!(
            (sse, level, trend, season),
            (7.0, [11.0; 5], [12.0; 5], [13.0; 5])
        );
    }

    #[test]
    fn holtwinters_initialization_only_allows_empty_disabled_workspaces() {
        let mut p = parameters();
        p.start_time = 9;
        p.trend = false;
        p.seasonal = false;
        let (mut sse, mut level) = (7.0, [11.0, 22.0]);
        filter(
            &[],
            p,
            Initial {
                level: 2.0,
                trend: 3.0,
                season: &[],
            },
            Output {
                sse: std::slice::from_mut(&mut sse),
                level: &mut level,
                trend: &mut [],
                season: &mut [],
            },
        )
        .unwrap();
        assert_eq!(sse, 7.0);
        assert_eq!(level, [2.0, 22.0]);
    }
}

#[cfg(test)]
mod sse_slice_tests {
    use super::*;
    #[test]
    fn holtwinters_sse_slice_requires_storage_only_for_actual_iterations_before_writes() {
        let mut p = Parameters {
            alpha: 0.3,
            beta: 0.,
            gamma: 0.,
            start_time: 1,
            period: 0,
            additive: true,
            trend: false,
            seasonal: false,
        };
        let mut level = [11., 12.];
        let mut trend = [13.];
        let error = filter(
            &[1.],
            p,
            Initial {
                level: 2.,
                trend: 0.,
                season: &[],
            },
            Output {
                sse: &mut [],
                level: &mut level,
                trend: &mut trend,
                season: &mut [],
            },
        )
        .unwrap_err();
        assert_eq!(
            error,
            FilterError::ShortBuffer {
                name: "SSE",
                needed: 1,
                actual: 0
            }
        );
        assert_eq!(level, [11., 12.]);
        assert_eq!(trend, [13.]);
        p.start_time = 2;
        filter(
            &[],
            p,
            Initial {
                level: 2.,
                trend: 0.,
                season: &[],
            },
            Output {
                sse: &mut [],
                level: &mut level,
                trend: &mut [],
                season: &mut [],
            },
        )
        .unwrap();
        assert_eq!(level, [2., 12.]);
    }
}
