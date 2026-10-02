#![forbid(unsafe_code)]
//! Typed, initialized vector storage with stable legacy projections.
//!
//! Alignment comes from typed chunks containing only contiguous elements.
//! The owning Rc is established before a raw pointer can be projected, so
//! transferring the owner cannot uniquely retag already published bytes.

use std::{alloc::Layout, cell::Cell, rc::Rc};

use super::ffi::{R_xlen_t, Rcomplex, SEXP, SEXPTYPE};

#[repr(C, align(8))]
struct Chunk<T, const N: usize> {
    values: [Cell<T>; N],
}

const POINTERS_PER_CHUNK: usize = 8 / std::mem::size_of::<SEXP>();
type ByteChunk = Chunk<u8, 8>;
type IntegerChunk = Chunk<i32, 2>;
type RealChunk = Chunk<f64, 1>;
type ComplexChunk = Chunk<Rcomplex, 1>;
type PointerChunk = Chunk<SEXP, POINTERS_PER_CHUNK>;

// Pointer arithmetic in the legacy bridge requires no padding between
// adjacent chunks. These are type/layout checks, not allocator assumptions.
const _: () = {
    assert!(std::mem::size_of::<ByteChunk>() == 8);
    assert!(std::mem::size_of::<IntegerChunk>() == 2 * std::mem::size_of::<i32>());
    assert!(std::mem::size_of::<RealChunk>() == std::mem::size_of::<f64>());
    assert!(std::mem::size_of::<ComplexChunk>() == std::mem::size_of::<Rcomplex>());
    assert!(
        std::mem::size_of::<PointerChunk>() == POINTERS_PER_CHUNK * std::mem::size_of::<SEXP>()
    );
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PayloadError {
    Allocation,
    InvalidLength,
    InvalidVectorType,
}

enum Storage {
    Bytes(Rc<[ByteChunk]>),
    Integers(Rc<[IntegerChunk]>),
    Reals(Rc<[RealChunk]>),
    Complex(Rc<[ComplexChunk]>),
    References(Rc<[PointerChunk]>),
}

/// Canonical ownership of one vector's initialized typed allocation.
/// Header sharing is tracked by the arena; this owner is moved, not rebuilt
/// from a pointer. Its layout accounts for logical bytes, excluding spare
/// elements in the final alignment chunk.
pub(crate) struct OwnedPayload {
    storage: Storage,
    length: usize,
    layout: Layout,
}

fn chunks<T, const N: usize>(
    elements: usize,
    mut initial: impl FnMut() -> T,
) -> Result<Rc<[Chunk<T, N>]>, PayloadError> {
    let count = elements.div_ceil(N);
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| PayloadError::Allocation)?;
    values.resize_with(count, || Chunk {
        values: std::array::from_fn(|_| Cell::new(initial())),
    });
    Ok(Rc::from(values.into_boxed_slice()))
}

fn logical_layout(elements: usize, element_size: usize) -> Result<Layout, PayloadError> {
    let bytes = elements
        .checked_mul(element_size)
        .ok_or(PayloadError::InvalidLength)?;
    Layout::from_size_align(bytes, 8).map_err(|_| PayloadError::InvalidLength)
}

impl OwnedPayload {
    /// Generic scratch storage remains byte-typed and initialized. Consumers
    /// interpret its bytes through their own audited size/alignment contract.
    pub(crate) fn zeroed_bytes(bytes: usize) -> Result<Self, PayloadError> {
        let layout = logical_layout(bytes, 1)?;
        Ok(Self {
            storage: Storage::Bytes(chunks(bytes, || 0)?),
            length: bytes,
            layout,
        })
    }

    pub(crate) fn zeroed_vector(kind: SEXPTYPE, length: R_xlen_t) -> Result<Self, PayloadError> {
        let length = usize::try_from(length).map_err(|_| PayloadError::InvalidLength)?;
        let (storage, layout) = match kind {
            SEXPTYPE::RAWSXP => {
                let layout = logical_layout(length, 1)?;
                (Storage::Bytes(chunks(length, || 0)?), layout)
            }
            SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP => {
                let layout = logical_layout(length, std::mem::size_of::<i32>())?;
                (Storage::Integers(chunks(length, || 0)?), layout)
            }
            SEXPTYPE::REALSXP => {
                let layout = logical_layout(length, std::mem::size_of::<f64>())?;
                (Storage::Reals(chunks(length, || 0.0)?), layout)
            }
            SEXPTYPE::CPLXSXP => {
                let layout = logical_layout(length, std::mem::size_of::<Rcomplex>())?;
                (Storage::Complex(chunks(length, Rcomplex::default)?), layout)
            }
            SEXPTYPE::STRSXP | SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP | SEXPTYPE::BCODESXP => {
                let layout = logical_layout(length, std::mem::size_of::<SEXP>())?;
                (
                    Storage::References(chunks(length, std::ptr::null_mut)?),
                    layout,
                )
            }
            _ => return Err(PayloadError::InvalidVectorType),
        };
        Ok(Self {
            storage,
            length,
            layout,
        })
    }

    /// Character payloads include their terminating zero, initialized through
    /// typed cells without publishing a writable pointer during construction.
    pub(crate) fn characters(bytes: &[u8]) -> Result<Self, PayloadError> {
        let length = bytes
            .len()
            .checked_add(1)
            .ok_or(PayloadError::InvalidLength)?;
        let layout = logical_layout(length, 1)?;
        let values: Rc<[ByteChunk]> = chunks(length, || 0)?;
        for (index, byte) in bytes.iter().copied().enumerate() {
            values[index / 8].values[index % 8].set(byte);
        }
        Ok(Self {
            storage: Storage::Bytes(values),
            length,
            layout,
        })
    }

    pub(crate) fn layout(&self) -> Layout {
        self.layout
    }
    pub(crate) fn len(&self) -> usize {
        self.length
    }

    /// The projection spans the entire chunk slice, preserving provenance for
    /// legacy pointer arithmetic across all elements. No pointer is derived
    /// from a reference to only the first element's Cell.
    pub(crate) fn as_ptr(&self) -> *mut u8 {
        if self.length == 0 {
            return std::ptr::null_mut();
        }
        match &self.storage {
            Storage::Bytes(values) => values.as_ptr().cast::<u8>().cast_mut(),
            Storage::Integers(values) => values.as_ptr().cast::<u8>().cast_mut(),
            Storage::Reals(values) => values.as_ptr().cast::<u8>().cast_mut(),
            Storage::Complex(values) => values.as_ptr().cast::<u8>().cast_mut(),
            Storage::References(values) => values.as_ptr().cast::<u8>().cast_mut(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_payloads_preserve_alignment_lengths_and_zero_initialization() {
        for kind in [
            SEXPTYPE::RAWSXP,
            SEXPTYPE::INTSXP,
            SEXPTYPE::LGLSXP,
            SEXPTYPE::REALSXP,
            SEXPTYPE::CPLXSXP,
            SEXPTYPE::STRSXP,
            SEXPTYPE::VECSXP,
            SEXPTYPE::EXPRSXP,
            SEXPTYPE::BCODESXP,
        ] {
            for length in [0, 1, 2, 7, 8, 9, 65] {
                let payload = OwnedPayload::zeroed_vector(kind, length).unwrap();
                assert_eq!(payload.len(), length as usize);
                assert_eq!(payload.layout().align(), 8);
                assert_eq!(payload.as_ptr().is_null(), length == 0);
                assert_eq!(payload.as_ptr() as usize % 8, 0);
                match &payload.storage {
                    Storage::Bytes(values) => assert!(
                        values
                            .iter()
                            .all(|chunk| chunk.values.iter().all(|cell| cell.get() == 0))
                    ),
                    Storage::Integers(values) => assert!(
                        values
                            .iter()
                            .all(|chunk| chunk.values.iter().all(|cell| cell.get() == 0))
                    ),
                    Storage::Reals(values) => assert!(
                        values
                            .iter()
                            .all(|chunk| chunk.values.iter().all(|cell| cell.get() == 0.0))
                    ),
                    Storage::Complex(values) => assert!(values.iter().all(|chunk| {
                        chunk
                            .values
                            .iter()
                            .all(|cell| cell.get() == Rcomplex::default())
                    })),
                    Storage::References(values) => assert!(
                        values
                            .iter()
                            .all(|chunk| chunk.values.iter().all(|cell| cell.get().is_null()))
                    ),
                }
            }
        }
    }

    #[test]
    fn character_cells_copy_bytes_and_initialize_terminator_across_chunks() {
        for bytes in [b"".as_slice(), b"abcdefg", b"abcdefgh", b"abcdefghijk\0z"] {
            let payload = OwnedPayload::characters(bytes).unwrap();
            assert_eq!(payload.len(), bytes.len() + 1);
            assert_eq!(payload.layout().size(), bytes.len() + 1);
            let Storage::Bytes(values) = &payload.storage else {
                panic!("character storage")
            };
            for (index, expected) in bytes.iter().copied().chain([0]).enumerate() {
                assert_eq!(values[index / 8].values[index % 8].get(), expected);
            }
        }
    }

    #[test]
    fn invalid_lengths_and_nonvector_headers_are_rejected() {
        assert!(matches!(
            OwnedPayload::zeroed_vector(SEXPTYPE::INTSXP, -1),
            Err(PayloadError::InvalidLength)
        ));
        assert!(matches!(
            OwnedPayload::zeroed_vector(SEXPTYPE::REALSXP, i64::MAX),
            Err(PayloadError::InvalidLength)
        ));
        for kind in [
            SEXPTYPE::NILSXP,
            SEXPTYPE::SYMSXP,
            SEXPTYPE::CHARSXP,
            SEXPTYPE::LISTSXP,
        ] {
            assert!(matches!(
                OwnedPayload::zeroed_vector(kind, 1),
                Err(PayloadError::InvalidVectorType)
            ));
        }
        #[cfg(target_pointer_width = "32")]
        assert!(matches!(
            OwnedPayload::zeroed_vector(SEXPTYPE::RAWSXP, 1_i64 << 32),
            Err(PayloadError::InvalidLength)
        ));
    }
}
