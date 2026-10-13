use super::*;

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

impl RResult {
    pub(crate) fn with_error_output(mut self) -> Self {
        if let RValue::Error(message) = &self.typed {
            let message = message.trim_end();
            self.output = if message.starts_with("Error") {
                message.to_string()
            } else {
                // Prefer the fully rendered top-level error text (with call
                // attribution, "Error in <call> : ...") when it was rendered
                // by this exact error — mirroring what Rscript prints on
                // stderr. The last-rendered check rejects stale renders left
                // in the error buffer by earlier, already-caught errors.
                let rendered = crate::mainutils::errors::try_last_rendered_message(message);
                match rendered.as_deref().map(str::trim_end) {
                    Some(rendered)
                        if rendered.starts_with("Error") && rendered.contains(message) =>
                    {
                        rendered.to_string()
                    }
                    _ => format!("Error: {message}"),
                }
                .trim_end()
                .to_string()
            };
        }
        self
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

impl From<SexpComplex> for RComplexValue {
    fn from(value: SexpComplex) -> Self {
        RComplexValue {
            real: value.real,
            imaginary: value.imaginary,
        }
    }
}

impl RValue {
    pub fn from_sexp(sexp: Sexp<'_>) -> Self {
        match sexp.clone().to_owned_value() {
            Ok(value) => RValue::from_owned_value(value),
            Err(_error) => RValue::Unsupported {
                type_name: format!("invalid {}", sexp.typeof_().0),
            },
        }
    }

    pub(crate) fn from_owned_value(value: SexpValue) -> Self {
        match value {
            SexpValue::Null => RValue::Null,
            SexpValue::Logical(value) => RValue::Logical(value),
            SexpValue::Integer(value) => RValue::Integer(value),
            SexpValue::Real(value) => RValue::Real(value),
            SexpValue::LogicalVector(values) => RValue::LogicalVector(values),
            SexpValue::IntegerVector(values) => RValue::IntegerVector(values),
            SexpValue::RealVector(values) => RValue::RealVector(values),
            SexpValue::StringVector(values) => RValue::StringVector(values),
            SexpValue::RawVector(values) => RValue::RawVector(values),
            SexpValue::ComplexVector(values) => RValue::ComplexVector(
                values
                    .into_iter()
                    .map(|value| value.map(RComplexValue::from))
                    .collect(),
            ),
            SexpValue::List(values) => {
                RValue::List(values.into_iter().map(RValue::from_owned_value).collect())
            }
            SexpValue::Attributed { value, metadata } => RValue::Attributed {
                value: Box::new(RValue::from_owned_value(*value)),
                metadata: RMetadata::from(metadata),
            },
            SexpValue::Unsupported { type_name } => RValue::Unsupported { type_name },
        }
    }

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

impl From<SexpAttribute> for RAttribute {
    fn from(attribute: SexpAttribute) -> Self {
        RAttribute {
            name: attribute.name,
            value: RValue::from_owned_value(attribute.value),
        }
    }
}

impl From<SexpMetadata> for RMetadata {
    fn from(metadata: SexpMetadata) -> Self {
        RMetadata {
            names: metadata.names,
            dim: metadata.dim,
            class: metadata.class,
            levels: metadata.levels,
            attributes: metadata
                .attributes
                .into_iter()
                .map(RAttribute::from)
                .collect(),
        }
    }
}

/// Supported mathematical functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RMathFunc {
    Abs,
    Sqrt,
    Ceil,
    Floor,
    Trunc,
    Exp,
    Log,
    Log2,
    Log10,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Gamma,
    Lgamma,
}

/// Supported Bessel function families.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RBesselFunc {
    J,
    Y,
    I,
    K,
}

// ---------------------------------------------------------------------------
// Free functions for simple operations (no session needed)
// ---------------------------------------------------------------------------

/// Compute `dnorm(x, mean, sd)`.
pub fn dnorm_free(x: f64, mean: f64, sd: f64) -> f64 {
    crate::dist::normal::dnorm(x, mean, sd, 0)
}

pub fn pnorm_free(x: f64, mean: f64, sd: f64) -> f64 {
    crate::dist::normal::pnorm(x, mean, sd, 1, 0)
}

pub fn qnorm_free(p: f64, mean: f64, sd: f64) -> f64 {
    crate::dist::normal::qnorm(p, mean, sd, 1, 0)
}
