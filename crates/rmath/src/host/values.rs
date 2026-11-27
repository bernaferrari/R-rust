#![warn(unused_unsafe)]

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Result of an R evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct RResult {
    /// Legacy numeric scalar view for existing simple callers.
    pub value: f64,
    /// Owned typed value for Android/FFI callers that should not parse output.
    pub typed: RValue,
    /// Actual console emission in chronological order, including automatic printing and warnings.
    pub output: String,
    /// Rscript stdout — print/cat/auto-print only.
    pub stdout: String,
    /// Rscript stderr — warnings and REprintf.
    pub stderr: String,
}

pub(crate) fn saturating_usize(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

/// Runtime state needed by Android hosts to wire package libraries and temp files.
#[derive(Debug, Clone, PartialEq)]
pub struct RRuntimeInfo {
    pub is_active: bool,
    pub library_paths: Vec<String>,
    pub temp_dir: String,
}

/// Snapshot of one session's arena allocator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RArenaStats {
    pub active_nodes: u64,
    pub free_nodes: u64,
    pub retained_bytes: u64,
    pub fragmentation_ratio: f64,
}

impl Default for RArenaStats {
    fn default() -> Self {
        Self {
            active_nodes: 0,
            free_nodes: 0,
            retained_bytes: 0,
            fragmentation_ratio: 0.0,
        }
    }
}

/// Host-owned resource limits for Android sessions.
///
/// A value of `0` means unlimited for that dimension, except
/// `max_eval_depth`, where `0` selects the evaluator default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RResourceLimits {
    pub max_eval_depth: u64,
    pub max_execution_time_ms: u64,
    pub max_alloc_bytes: u64,
    /// Maximum simultaneously live arena nodes. Garbage-collected slots may
    /// be reused; execution time and cancellation bound cumulative work.
    pub max_arena_nodes: u64,
}

impl Default for RResourceLimits {
    fn default() -> Self {
        RResourceLimits {
            max_eval_depth: 500,
            max_execution_time_ms: 0,
            max_alloc_bytes: 0,
            max_arena_nodes: 0,
        }
    }
}



/// Owned representation of evaluated R values suitable for FFI boundaries.
#[derive(Debug, Clone, PartialEq)]
pub enum RValue {
    Null,
    Logical(Option<bool>),
    Integer(Option<i32>),
    Real(Option<f64>),
    LogicalVector(Vec<Option<bool>>),
    IntegerVector(Vec<Option<i32>>),
    RealVector(Vec<Option<f64>>),
    StringVector(Vec<Option<String>>),
    RawVector(Vec<u8>),
    ComplexVector(Vec<Option<RComplexValue>>),
    List(Vec<RValue>),
    Attributed {
        value: Box<RValue>,
        metadata: RMetadata,
    },
    Unsupported {
        type_name: String,
    },
    Error(String),
}

/// Named owned R attribute for Android/UniFFI callers.
#[derive(Debug, Clone, PartialEq)]
pub struct RAttribute {
    pub name: String,
    pub value: RValue,
}

/// Owned R metadata for Android/UniFFI callers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RMetadata {
    pub names: Option<Vec<Option<String>>>,
    pub dim: Option<Vec<i32>>,
    pub class: Option<Vec<Option<String>>>,
    pub levels: Option<Vec<Option<String>>>,
    pub attributes: Vec<RAttribute>,
}

/// Owned complex number for Android/UniFFI callers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RComplexValue {
    pub real: f64,
    pub imaginary: f64,
}

impl RValue {
    pub(crate) fn numeric_scalar_value(&self) -> f64 {
        match self {
            RValue::Integer(Some(value)) => *value as f64,
            RValue::Integer(None) => f64::NAN,
            RValue::Real(Some(value)) => *value,
            RValue::Real(None) => f64::NAN,
            RValue::Logical(Some(true)) => 1.0,
            RValue::Logical(Some(false)) => 0.0,
            RValue::Logical(None) => f64::NAN,
            RValue::IntegerVector(values) => values
                .first()
                .and_then(|value| *value)
                .map(|value| value as f64)
                .unwrap_or(f64::NAN),
            RValue::RealVector(values) => {
                values.first().and_then(|value| *value).unwrap_or(f64::NAN)
            }
            RValue::LogicalVector(values) => match values.first().copied().flatten() {
                Some(true) => 1.0,
                Some(false) => 0.0,
                None => f64::NAN,
            },
            RValue::Attributed { value, .. } => value.numeric_scalar_value(),
            _ => 0.0,
        }
    }
}
