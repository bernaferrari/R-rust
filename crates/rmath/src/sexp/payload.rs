#![forbid(unsafe_code)]
//! Typed, initialized vector storage with stable numeric projections.
//!
//! Numeric alignment comes from typed chunks; graph references are exact
//! nonowning NodeLinks in their own aligned cells, never a pointer array.
//! The owning Rc is established before a raw pointer can be projected, so
//! transferring the owner cannot uniquely retag already published bytes.

use std::{alloc::Layout, cell::Cell, rc::Rc};

use super::ffi::{R_xlen_t, Rcomplex, SEXPTYPE};
use super::heap::NodeLink;

#[repr(C, align(8))]
struct Chunk<T, const N: usize> {
    values: [Cell<T>; N],
}

type ByteChunk = Chunk<u8, 8>;
type IntegerChunk = Chunk<i32, 2>;
type RealChunk = Chunk<f64, 1>;
type ComplexChunk = Chunk<Rcomplex, 1>;

// Pointer arithmetic in the legacy bridge requires no padding between
// adjacent chunks. These are type/layout checks, not allocator assumptions.
const _: () = {
    assert!(std::mem::size_of::<ByteChunk>() == 8);
    assert!(std::mem::size_of::<IntegerChunk>() == 2 * std::mem::size_of::<i32>());
    assert!(std::mem::size_of::<RealChunk>() == std::mem::size_of::<f64>());
    assert!(std::mem::size_of::<ComplexChunk>() == std::mem::size_of::<Rcomplex>());
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
    References(Rc<[Cell<NodeLink>]>),
}

/// A lease of the original canonical reference cells. Keeping this Rc alive
/// prevents allocator address reuse while a copied visitor is detached.
/// It is neither a rebuilt payload nor a second source of graph authority.
#[derive(Clone)]
pub(crate) struct ReferencePayloadLease(Rc<[Cell<NodeLink>]>);

impl ReferencePayloadLease {
    pub(crate) fn from_cells(cells: Rc<[Cell<NodeLink>]>) -> Self {
        Self(cells)
    }

    pub(crate) fn same_allocation(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    pub(crate) fn element(&self, index: usize) -> Option<NodeLink> {
        self.0.get(index).map(Cell::get)
    }

    pub(crate) fn snapshot(&self, length: usize) -> Option<Vec<NodeLink>> {
        if length > self.0.len() {
            return None;
        }
        Some(self.0.iter().take(length).map(Cell::get).collect())
    }

    /// All bounds are checked before touching any canonical cell. Callers
    /// validate parent identity, original values and replacement links first.
    pub(crate) fn replace_sparse(&self, changes: &[(usize, NodeLink)]) -> Option<()> {
        if changes.iter().any(|(index, _)| *index >= self.0.len()) {
            return None;
        }
        for (index, link) in changes {
            self.0[*index].set(*link);
        }
        Some(())
    }
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
                let layout = logical_layout(length, std::mem::size_of::<NodeLink>())?;
                let mut values = Vec::new();
                values
                    .try_reserve_exact(length)
                    .map_err(|_| PayloadError::Allocation)?;
                values.resize_with(length, || Cell::new(NodeLink::NULL));
                (
                    Storage::References(Rc::from(values.into_boxed_slice())),
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

    pub(crate) fn reference_capacity(&self) -> Option<usize> {
        matches!(self.storage, Storage::References(_)).then_some(self.length)
    }

    pub(crate) fn reference_elt(&self, index: usize) -> Option<NodeLink> {
        let Storage::References(chunks) = &self.storage else {
            return None;
        };
        (index < self.length).then(|| chunks[index].get())
    }

    pub(crate) fn set_reference_elt(&self, index: usize, value: NodeLink) -> Option<()> {
        self.replace_references(index, &[value])
    }

    /// Check the entire range before writing any cell, including arithmetic
    /// overflow and the logical end of the final alignment chunk.
    pub(crate) fn replace_references(&self, start: usize, values: &[NodeLink]) -> Option<()> {
        let Storage::References(chunks) = &self.storage else {
            return None;
        };
        if start.checked_add(values.len())? > self.length {
            return None;
        }
        for (index, value) in (start..).zip(values.iter().copied()) {
            chunks[index].set(value);
        }
        Some(())
    }

    pub(crate) fn reference_lease(&self) -> Option<ReferencePayloadLease> {
        let Storage::References(cells) = &self.storage else {
            return None;
        };
        Some(ReferencePayloadLease::from_cells(Rc::clone(cells)))
    }

    /// Copy graph edges from the actual typed cells. Header lengths cannot
    /// expose alignment padding or reinterpret a numeric/scratch allocation.
    pub(crate) fn copy_references(&self, length: usize) -> Option<Vec<NodeLink>> {
        let Storage::References(chunks) = &self.storage else {
            return None;
        };
        if length > self.length {
            return None;
        }
        Some(chunks.iter().take(length).map(Cell::get).collect())
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
    fn reference_cell_writes_are_typed_bounded_and_atomic_on_failure() {
        let payload = OwnedPayload::zeroed_vector(SEXPTYPE::VECSXP, 3).unwrap();
        let heap = super::super::heap::HeapIdentity::new();
        let page = super::super::heap::NodePage::try_new(heap, 0, 1, || ()).unwrap();
        page.metadata().activate(0, false).unwrap();
        let first = page.token(0).unwrap().link().unwrap();
        payload.set_reference_elt(2, first).unwrap();
        assert_eq!(payload.reference_elt(2), Some(first));
        assert!(payload.reference_elt(3).is_none());
        assert!(payload.set_reference_elt(3, first).is_none());
        assert!(payload.replace_references(2, &[first, first]).is_none());
        assert!(payload.replace_references(usize::MAX, &[first]).is_none());
        assert_eq!(
            payload.copy_references(3),
            Some(vec![NodeLink::NULL, NodeLink::NULL, first])
        );
        payload.replace_references(3, &[]).unwrap();
        assert!(payload.replace_references(4, &[]).is_none());
        for payload in [
            OwnedPayload::zeroed_bytes(24).unwrap(),
            OwnedPayload::zeroed_vector(SEXPTYPE::INTSXP, 3).unwrap(),
        ] {
            assert!(payload.reference_capacity().is_none());
            assert!(payload.reference_elt(0).is_none());
            assert!(payload.set_reference_elt(0, first).is_none());
            assert!(payload.replace_references(0, &[]).is_none());
        }
    }

    #[test]
    fn graph_snapshots_check_actual_storage_type_and_logical_bounds() {
        for length in [0, 1, 3, 8, 9] {
            let payload = OwnedPayload::zeroed_vector(SEXPTYPE::VECSXP, length).unwrap();
            assert_eq!(
                payload.copy_references(length as usize).unwrap().len(),
                length as usize
            );
            assert!(payload.copy_references(length as usize + 1).is_none());
            assert_eq!(payload.copy_references(0), Some(Vec::new()));
        }
        for kind in [SEXPTYPE::INTSXP, SEXPTYPE::REALSXP, SEXPTYPE::RAWSXP] {
            assert!(
                OwnedPayload::zeroed_vector(kind, 8)
                    .unwrap()
                    .copy_references(1)
                    .is_none()
            );
        }
        assert!(
            OwnedPayload::zeroed_bytes(64)
                .unwrap()
                .copy_references(1)
                .is_none()
        );
    }

    #[test]
    fn reference_lease_retains_original_cells_and_distinguishes_equal_capacity_allocations() {
        let original = OwnedPayload::zeroed_vector(SEXPTYPE::VECSXP, 2).unwrap();
        let lease = original.reference_lease().unwrap();
        let weak = Rc::downgrade(&lease.0);
        let same = original.reference_lease().unwrap();
        let different = OwnedPayload::zeroed_vector(SEXPTYPE::VECSXP, 2)
            .unwrap()
            .reference_lease()
            .unwrap();
        assert!(lease.same_allocation(&same));
        assert!(!lease.same_allocation(&different));
        drop(original);
        drop(same);
        assert!(weak.upgrade().is_some());
        assert_eq!(lease.snapshot(2), Some(vec![NodeLink::NULL; 2]));
        let page =
            super::super::heap::NodePage::try_new(super::super::heap::HeapIdentity::new(), 0, 1, || ())
                .unwrap();
        page.metadata().activate(0, false).unwrap();
        let nonnull = page.token(0).unwrap().link().unwrap();
        assert!(
            lease
                .replace_sparse(&[(0, nonnull), (2, NodeLink::NULL)])
                .is_none()
        );
        assert_eq!(lease.snapshot(2), Some(vec![NodeLink::NULL; 2]));
        drop(lease);
        assert!(weak.upgrade().is_none());
    }

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
                    Storage::References(values) => {
                        assert!(values.iter().all(|cell| cell.get().is_null()))
                    }
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
