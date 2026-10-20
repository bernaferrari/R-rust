//! Node/vector layout constants, ArenaError, OwnedBuffer.

use super::*;

/// Full pages contain 4096 canonical headers; constrained byte budgets use
/// smaller admitted pages. Header bytes follow the actual SexprecCore layout.
pub(super) const NODE_PAGE_SIZE: usize = 4096;

const _: () = assert!(NODE_PAGE_SIZE % 64 == 0);

/// Byte size of one node for checking legacy projection address ranges.
pub(super) const NODE_BYTES: usize = std::mem::size_of::<SexprecCore>();

// ---------------------------------------------------------------------------
// Element sizes by SEXPTYPE
// ---------------------------------------------------------------------------

/// Get the element size in bytes for a vector SEXPTYPE.
/// Returns 0 for non-vector types.
pub fn sexp_elem_size(t: SEXPTYPE) -> usize {
    match t {
        SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP => std::mem::size_of::<i32>(),
        SEXPTYPE::REALSXP => std::mem::size_of::<f64>(),
        SEXPTYPE::CPLXSXP => std::mem::size_of::<Rcomplex>(),
        SEXPTYPE::STRSXP | SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP | SEXPTYPE::BCODESXP => {
            std::mem::size_of::<NodeLink>()
        }
        SEXPTYPE::RAWSXP => std::mem::size_of::<Rbyte>(),
        _ => 0,
    }
}

pub(super) const GC_TRIGGER_THRESHOLD: usize = 10_000;

pub(super) const GC_BYTE_THRESHOLD: usize = 64 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Arena budget and error types
// ---------------------------------------------------------------------------

/// Errors that can occur during arena allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArenaError {
    /// Allocation failed (e.g., out of memory).
    OutOfMemory,
    /// Request would exceed the arena's byte budget.
    ByteBudgetExceeded { limit: usize, requested: usize },
    /// Request would exceed the arena's node budget.
    NodeBudgetExceeded { limit: usize, requested: usize },
    /// Invalid vector length (negative or overflow).
    InvalidLength,
    /// The type requires a different header or a dedicated constructor.
    InvalidVectorType { sexptype: SEXPTYPE },
}

impl std::fmt::Display for ArenaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArenaError::OutOfMemory => write!(f, "arena out of memory"),
            ArenaError::ByteBudgetExceeded { limit, requested } => {
                write!(
                    f,
                    "arena byte budget exceeded: limit={limit}, requested={requested}"
                )
            }
            ArenaError::NodeBudgetExceeded { limit, requested } => {
                write!(
                    f,
                    "arena node budget exceeded: limit={limit}, requested={requested}"
                )
            }
            ArenaError::InvalidLength => write!(f, "invalid vector length"),
            ArenaError::InvalidVectorType { sexptype } => {
                write!(f, "invalid vector type: {}", sexptype.as_c_int())
            }
        }
    }
}

impl std::error::Error for ArenaError {}

/// Validate the union header, platform-sized length and full payload layout
/// before either allocator changes accounting or allocates memory.
pub(super) fn vector_layout(sexptype: SEXPTYPE, length: R_xlen_t) -> Result<Layout, ArenaError> {
    let elem_size = sexp_elem_size(sexptype);
    if elem_size == 0 {
        return Err(ArenaError::InvalidVectorType { sexptype });
    }
    let length = usize::try_from(length).map_err(|_| ArenaError::InvalidLength)?;
    let bytes = length
        .checked_mul(elem_size)
        .ok_or(ArenaError::InvalidLength)?;
    Layout::from_size_align(bytes, std::mem::align_of::<u64>())
        .map_err(|_| ArenaError::InvalidLength)
}

/// Fresh typed chunks have eight-byte alignment and retain the last complete
/// chunk, while logical vector shape still uses the unpadded layout above.
pub(super) fn vector_storage_layout(
    sexptype: SEXPTYPE,
    length: R_xlen_t,
) -> Result<Layout, ArenaError> {
    // u64 itself has four-byte alignment on i686, while every typed chunk
    // explicitly has eight-byte alignment on every supported target.
    let logical = vector_layout(sexptype, length)?;
    Layout::from_size_align(logical.size(), 8)
        .map(|layout| layout.pad_to_align())
        .map_err(|_| ArenaError::InvalidLength)
}

#[cfg(test)]
thread_local! {
    static BUFFER_ALLOCATION_ATTEMPTS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn buffer_allocation_attempts() -> usize {
    BUFFER_ALLOCATION_ATTEMPTS.with(Cell::get)
}

#[cfg(test)]
pub(super) fn note_buffer_allocation_attempt() {
    BUFFER_ALLOCATION_ATTEMPTS.with(|count| count.set(count.get() + 1));
}

/// Own initialized transient bytes with the requested layout and at least
/// eight-byte alignment. R_alloc is the verified consumer of this facade.
pub(crate) struct OwnedBuffer {
    allocation: OwnedPayload,
    layout: Layout,
}

impl OwnedBuffer {
    pub(crate) fn required_storage_bytes(layout: Layout) -> Option<usize> {
        if layout.size() == 0 || layout.align() > 8 {
            return None;
        }
        OwnedPayload::requested_byte_storage_bytes(layout.size()).ok()
    }
    pub(crate) fn zeroed(layout: Layout) -> Option<Self> {
        Self::required_storage_bytes(layout)?;
        #[cfg(test)]
        note_buffer_allocation_attempt();
        let allocation = OwnedPayload::zeroed_bytes(layout.size()).ok()?;
        Some(Self { allocation, layout })
    }

    pub(crate) fn as_ptr(&self) -> *mut u8 {
        self.allocation.as_ptr()
    }
    pub(crate) fn layout(&self) -> Layout {
        self.layout
    }
}

pub(super) fn zeroed_vector_payload(
    kind: SEXPTYPE,
    length: R_xlen_t,
) -> Result<OwnedPayload, ArenaError> {
    #[cfg(test)]
    if length > 0 {
        note_buffer_allocation_attempt();
    }
    OwnedPayload::zeroed_vector(kind, length).map_err(|error| match error {
        PayloadError::Allocation
        | PayloadError::IdentityExhausted
        | PayloadError::AliasedStorage => ArenaError::OutOfMemory,
        PayloadError::InvalidLength => ArenaError::InvalidLength,
        PayloadError::InvalidVectorType => ArenaError::InvalidVectorType { sexptype: kind },
    })
}
