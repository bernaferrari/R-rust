use super::super::ffi::{R_xlen_t, SEXPTYPE};

/// Error returned by Rust-shaped SEXP accessors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SexpError {
    /// A raw pointer was null.
    NullPointer,
    /// A raw pointer was visibly not aligned for `SexprecCore`.
    MisalignedPointer { address: usize },
    /// The SEXP had the wrong R type for the requested operation.
    TypeMismatch {
        expected: &'static str,
        actual: SEXPTYPE,
    },
    /// An element index was outside the vector length.
    OutOfBounds { index: R_xlen_t, len: R_xlen_t },
    /// A requested pairlist argument was not present.
    MissingArgument { index: usize },
    /// Allocation returned a null pointer while building an object.
    AllocationFailed { object: &'static str },
    /// A vector-like object had no data buffer.
    MissingData { sexptype: SEXPTYPE },
    /// A string value was not valid UTF-8.
    InvalidUtf8,
}

impl std::fmt::Display for SexpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SexpError::NullPointer => write!(f, "SEXP pointer is null"),
            SexpError::MisalignedPointer { address } => {
                write!(f, "SEXP pointer {address:#x} is misaligned")
            }
            SexpError::TypeMismatch { expected, actual } => {
                write!(f, "expected {expected}, got {:?}", actual.0)
            }
            SexpError::OutOfBounds { index, len } => {
                write!(f, "index {index} is outside vector length {len}")
            }
            SexpError::MissingArgument { index } => {
                write!(f, "missing pairlist argument at index {index}")
            }
            SexpError::AllocationFailed { object } => write!(f, "failed to allocate {object}"),
            SexpError::MissingData { sexptype } => {
                write!(f, "SEXP {:?} has no data buffer", sexptype.0)
            }
            SexpError::InvalidUtf8 => write!(f, "CHARSXP bytes are not valid UTF-8"),
        }
    }
}

impl std::error::Error for SexpError {}

pub type SexpResult<T> = Result<T, SexpError>;
