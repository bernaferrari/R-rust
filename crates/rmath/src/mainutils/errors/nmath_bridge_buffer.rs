//! One fallible NUL-terminated warning buffer charged to its captured heap.
#![forbid(unsafe_code)]
use crate::sexp::{
    memory::{MemoryLedger, WorkspaceReservation},
    object::SexpError,
};
use std::ffi::CStr;
pub(super) struct WarningBuffer {
    // Physical storage must disappear before original-ledger credit is released.
    bytes: Vec<u8>,
    _admission: WorkspaceReservation,
}
#[derive(Debug)]
pub(super) enum BufferError {
    AdmissionDenied,
    LengthOverflow,
    AllocationFailed,
}
impl BufferError {
    pub(super) fn into_sexp(self) -> SexpError {
        match self {
            Self::AdmissionDenied => SexpError::EvaluationFailed {
                message: "R allocation failed: memory or node budget exceeded".into(),
            },
            Self::LengthOverflow => SexpError::AllocationFailed {
                object: "nmath warning C-string length",
            },
            Self::AllocationFailed => SexpError::AllocationFailed {
                object: "nmath warning C-string workspace",
            },
        }
    }
}
fn required_bytes(length: usize) -> Result<usize, BufferError> {
    length
        .checked_add(1)
        .filter(|&n| n <= isize::MAX as usize)
        .ok_or(BufferError::LengthOverflow)
}
impl WarningBuffer {
    /// The entry boundary pins/checks authority and captures the original ledger.
    /// No arena loan, R allocation, callback or growth occurs in this copy.
    pub(super) fn copy(ledger: &MemoryLedger, message: &str) -> Result<Self, BufferError> {
        let message = message.strip_suffix('\n').unwrap_or(message);
        // Preserve CString::new(...).unwrap_or_default(): any embedded NUL
        // turns the entire message into the empty C string.
        let source = if message.as_bytes().contains(&0) {
            &[][..]
        } else {
            message.as_bytes()
        };
        let length = required_bytes(source.len())?;
        let mut admission = ledger.reserve(length).ok_or(BufferError::AdmissionDenied)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| BufferError::AllocationFailed)?;
        if bytes.capacity() < length {
            drop(bytes);
            return Err(BufferError::AllocationFailed);
        }
        if admission.try_resize(bytes.capacity()).is_none() {
            drop(bytes);
            return Err(BufferError::AdmissionDenied);
        }
        bytes.extend_from_slice(source);
        bytes.push(0);
        Ok(Self {
            bytes,
            _admission: admission,
        })
    }
    pub(super) fn as_c_str(&self) -> &CStr {
        CStr::from_bytes_with_nul(&self.bytes).expect("one checked terminal NUL")
    }
    #[cfg(test)]
    pub(super) fn capacity(&self) -> usize {
        self.bytes.capacity()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nmath_bridge_buffer_size_overflow_is_rejected() {
        assert!(required_bytes(usize::MAX).is_err());
        assert!(required_bytes(isize::MAX as usize).is_err());
        assert_eq!(required_bytes(0).unwrap(), 1);
    }
}
