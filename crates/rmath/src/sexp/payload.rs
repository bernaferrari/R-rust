#![forbid(unsafe_code)]
//! Typed, initialized vector storage with stable numeric projections.
//!
//! Numeric alignment comes from typed chunks; graph references are exact
//! nonowning NodeLinks in their own aligned cells, never a pointer array.
//! The owning Rc is established before a raw pointer can be projected, so
//! transferring the owner cannot uniquely retag already published bytes.

use std::{
    alloc::Layout,
    cell::{Cell, RefCell},
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

use super::ffi::{NodeBody, R_xlen_t, Rcomplex, SEXPTYPE, SexprecCore};
use super::heap::{HeapIdentity, NodeLink};

static NEXT_PAYLOAD_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PayloadId(std::num::NonZeroU64);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PayloadLink(Option<PayloadId>);
impl PayloadLink {
    pub(crate) const EMPTY: Self = Self(None);
    pub(crate) fn is_empty(self) -> bool {
        self.0.is_none()
    }
    pub(crate) fn id(self) -> Option<PayloadId> {
        self.0
    }
}

struct PayloadCharge {
    total: Rc<Cell<usize>>,
    bytes: usize,
}
impl Drop for PayloadCharge {
    fn drop(&mut self) {
        self.total.set(
            self.total
                .get()
                .checked_sub(self.bytes)
                .expect("payload byte ledger balance"),
        );
    }
}
struct PayloadAllocation {
    id: PayloadId,
    payload: OwnedPayload,
    domain: RefCell<Option<HeapIdentity>>,
    charge: RefCell<Option<PayloadCharge>>,
    immutable: Cell<bool>,
}
/// Pins the actual typed arrays, their identity and their single byte charge.
#[derive(Clone)]
pub(crate) struct PayloadLease(Rc<PayloadAllocation>);
#[derive(Clone)]
pub(crate) struct WeakPayloadLease(std::rc::Weak<PayloadAllocation>);
impl WeakPayloadLease {
    pub(crate) fn upgrade(&self) -> Option<PayloadLease> {
        self.0.upgrade().map(PayloadLease)
    }
}
impl std::fmt::Debug for PayloadLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("PayloadLease").field(&self.id()).finish()
    }
}
impl PayloadLease {
    pub(crate) fn from_owned(payload: OwnedPayload) -> Result<Self, PayloadError> {
        let raw = NEXT_PAYLOAD_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(1)
            })
            .map_err(|_| PayloadError::IdentityExhausted)?;
        let id = PayloadId(std::num::NonZeroU64::new(raw).ok_or(PayloadError::IdentityExhausted)?);
        Ok(Self(Rc::new(PayloadAllocation {
            id,
            payload,
            domain: RefCell::new(None),
            charge: RefCell::new(None),
            immutable: Cell::new(false),
        })))
    }
    pub(crate) fn zeroed(kind: SEXPTYPE, length: R_xlen_t) -> Result<Self, PayloadError> {
        Self::from_owned(OwnedPayload::zeroed_vector(kind, length)?)
    }
    pub(crate) fn from_byte_cells(mut cells: Rc<[Cell<u8>]>) -> Result<Self, PayloadError> {
        // A later seal must cover every safe writer to these original cells.
        // Retained strong or weak aliases could otherwise bypass the lease.
        if Rc::get_mut(&mut cells).is_none() {
            return Err(PayloadError::AliasedStorage);
        }
        Self::from_owned(OwnedPayload::from_cells(
            Storage::ByteCells(cells),
            1,
            std::mem::align_of::<u8>(),
        )?)
    }
    pub(crate) fn from_integer_cells(cells: Rc<[Cell<i32>]>) -> Result<Self, PayloadError> {
        Self::from_owned(OwnedPayload::from_cells(
            Storage::IntegerCells(cells),
            4,
            std::mem::align_of::<i32>(),
        )?)
    }
    pub(crate) fn from_real_cells(cells: Rc<[Cell<f64>]>) -> Result<Self, PayloadError> {
        Self::from_owned(OwnedPayload::from_cells(
            Storage::RealCells(cells),
            8,
            std::mem::align_of::<f64>(),
        )?)
    }
    pub(crate) fn from_reference_cells(cells: Rc<[Cell<NodeLink>]>) -> Result<Self, PayloadError> {
        Self::from_owned(OwnedPayload::from_cells(
            Storage::References(cells),
            std::mem::size_of::<NodeLink>(),
            std::mem::align_of::<NodeLink>(),
        )?)
    }
    pub(crate) fn id(&self) -> PayloadId {
        self.0.id
    }
    pub(crate) fn downgrade(&self) -> WeakPayloadLease {
        WeakPayloadLease(Rc::downgrade(&self.0))
    }
    pub(crate) fn link(&self) -> PayloadLink {
        PayloadLink(Some(self.id()))
    }
    pub(crate) fn capacity(&self) -> usize {
        self.0.payload.len()
    }
    pub(crate) fn logical_bytes(&self) -> usize {
        self.0.payload.layout().size()
    }
    pub(crate) fn accepts_kind(&self, kind: SEXPTYPE) -> bool {
        self.0.payload.accepts_kind(kind)
    }
    pub(crate) fn matches_header(&self, header: &SexprecCore) -> bool {
        if !header.has_valid_shape()
            || header.payload != self.link()
            || !self.accepts_kind(header.sxpinfo.type_of())
        {
            return false;
        }
        let NodeBody::Vector(vector) = header.data else {
            return false;
        };
        let Ok(length) = usize::try_from(vector.length) else {
            return false;
        };
        if header.sxpinfo.type_of() == SEXPTYPE::CHARSXP {
            length < self.capacity() && self.byte_elt(length) == Some(0)
        } else {
            length <= self.capacity()
        }
    }
    pub(crate) fn same_allocation(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
    pub(crate) fn make_immutable(&self) {
        self.0.immutable.set(true);
    }
    pub(crate) fn is_immutable(&self) -> bool {
        self.0.immutable.get()
    }
    pub(crate) fn belongs_to(&self, heap: &HeapIdentity) -> bool {
        self.0
            .domain
            .borrow()
            .as_ref()
            .is_some_and(|domain| domain.same_domain(heap))
    }
    pub(crate) fn bind_to_heap(&self, heap: &HeapIdentity, total: &Rc<Cell<usize>>) -> Option<()> {
        let mut domain = self.0.domain.borrow_mut();
        if let Some(original) = domain.as_ref() {
            return original.same_domain(heap).then_some(());
        }
        // A sealed allocation may be reused in its original charged domain,
        // but an unbound singleton cannot become an unmetered heap payload.
        if self.0.immutable.get() {
            return None;
        }
        let bytes = self.logical_bytes();
        let next = total.get().checked_add(bytes)?;
        *self.0.charge.borrow_mut() = Some(PayloadCharge {
            total: total.clone(),
            bytes,
        });
        *domain = Some(heap.clone());
        total.set(next);
        Some(())
    }
    /// A projection of this pinned actual allocation only. Dereferencing it
    /// requires an audited numerical boundary with no conflicting loans or R
    /// callbacks; safe readers use the bounded copied element methods. An
    /// immutable lease permits only reads through this projection.
    pub(crate) fn native_projection(&self) -> *mut u8 {
        self.0.payload.as_ptr()
    }
    pub(crate) fn byte_elt(&self, index: usize) -> Option<u8> {
        self.0.payload.byte_cell(index).map(Cell::get)
    }
    pub(crate) fn integer_elt(&self, index: usize) -> Option<i32> {
        self.0.payload.integer_cell(index).map(Cell::get)
    }
    pub(crate) fn real_elt(&self, index: usize) -> Option<f64> {
        self.0.payload.real_cell(index).map(Cell::get)
    }
    pub(crate) fn complex_elt(&self, index: usize) -> Option<Rcomplex> {
        self.0.payload.complex_cell(index).map(Cell::get)
    }
    pub(crate) fn set_byte_elt(&self, index: usize, value: u8) -> Option<()> {
        if self.0.immutable.get() {
            return None;
        }
        self.0.payload.byte_cell(index)?.set(value);
        Some(())
    }
    pub(crate) fn set_integer_elt(&self, index: usize, value: i32) -> Option<()> {
        if self.0.immutable.get() {
            return None;
        }
        self.0.payload.integer_cell(index)?.set(value);
        Some(())
    }
    pub(crate) fn set_real_elt(&self, index: usize, value: f64) -> Option<()> {
        if self.0.immutable.get() {
            return None;
        }
        self.0.payload.real_cell(index)?.set(value);
        Some(())
    }
    pub(crate) fn set_complex_elt(&self, index: usize, value: Rcomplex) -> Option<()> {
        if self.0.immutable.get() {
            return None;
        }
        self.0.payload.complex_cell(index)?.set(value);
        Some(())
    }
    pub(crate) fn reference_elt(&self, index: usize) -> Option<NodeLink> {
        self.0.payload.reference_elt(index)
    }
    pub(crate) fn set_reference_elt(&self, index: usize, value: NodeLink) -> Option<()> {
        if self.0.immutable.get() {
            return None;
        }
        self.0.payload.set_reference_elt(index, value)
    }
    pub(crate) fn copy_references(&self, length: usize) -> Option<Vec<NodeLink>> {
        self.0.payload.copy_references(length)
    }
    pub(crate) fn reference_lease(&self) -> Option<ReferencePayloadLease> {
        let mut lease = self.0.payload.reference_lease()?;
        lease.owner = Some(self.clone());
        Some(lease)
    }
}

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
    AliasedStorage,
    InvalidLength,
    InvalidVectorType,
    IdentityExhausted,
}

enum Storage {
    Bytes(Rc<[ByteChunk]>),
    Integers(Rc<[IntegerChunk]>),
    Reals(Rc<[RealChunk]>),
    Complex(Rc<[ComplexChunk]>),
    References(Rc<[Cell<NodeLink>]>),
    ByteCells(Rc<[Cell<u8>]>),
    IntegerCells(Rc<[Cell<i32>]>),
    RealCells(Rc<[Cell<f64>]>),
}

/// A lease of the original canonical reference cells. Keeping this Rc alive
/// prevents allocator address reuse while a copied visitor is detached.
/// It is neither a rebuilt payload nor a second source of graph authority.
#[derive(Clone)]
pub(crate) struct ReferencePayloadLease {
    cells: Rc<[Cell<NodeLink>]>,
    owner: Option<PayloadLease>,
}

impl ReferencePayloadLease {
    pub(crate) fn from_cells(cells: Rc<[Cell<NodeLink>]>) -> Self {
        Self { cells, owner: None }
    }

    pub(crate) fn same_allocation(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.cells, &other.cells)
    }

    pub(crate) fn element(&self, index: usize) -> Option<NodeLink> {
        self.cells.get(index).map(Cell::get)
    }

    pub(crate) fn snapshot(&self, length: usize) -> Option<Vec<NodeLink>> {
        if length > self.cells.len() {
            return None;
        }
        Some(self.cells.iter().take(length).map(Cell::get).collect())
    }

    /// All bounds are checked before touching any canonical cell. Callers
    /// validate parent identity, original values and replacement links first.
    pub(crate) fn replace_sparse(&self, changes: &[(usize, NodeLink)]) -> Option<()> {
        if self
            .owner
            .as_ref()
            .is_some_and(|owner| owner.0.immutable.get())
            || changes.iter().any(|(index, _)| *index >= self.cells.len())
        {
            return None;
        }
        for (index, link) in changes {
            self.cells[*index].set(*link);
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
    fn from_cells(
        storage: Storage,
        element_size: usize,
        alignment: usize,
    ) -> Result<Self, PayloadError> {
        let length = match &storage {
            Storage::ByteCells(cells) => cells.len(),
            Storage::IntegerCells(cells) => cells.len(),
            Storage::RealCells(cells) => cells.len(),
            Storage::References(cells) => cells.len(),
            _ => return Err(PayloadError::InvalidVectorType),
        };
        let bytes = length
            .checked_mul(element_size)
            .ok_or(PayloadError::InvalidLength)?;
        let layout =
            Layout::from_size_align(bytes, alignment).map_err(|_| PayloadError::InvalidLength)?;
        Ok(Self {
            storage,
            length,
            layout,
        })
    }
    fn byte_cell(&self, index: usize) -> Option<&Cell<u8>> {
        if index >= self.length {
            return None;
        }
        match &self.storage {
            Storage::Bytes(cells) => Some(&cells.get(index / 8)?.values[index % 8]),
            Storage::ByteCells(cells) => cells.get(index),
            _ => None,
        }
    }
    fn integer_cell(&self, index: usize) -> Option<&Cell<i32>> {
        if index >= self.length {
            return None;
        }
        match &self.storage {
            Storage::Integers(cells) => Some(&cells.get(index / 2)?.values[index % 2]),
            Storage::IntegerCells(cells) => cells.get(index),
            _ => None,
        }
    }
    fn real_cell(&self, index: usize) -> Option<&Cell<f64>> {
        if index >= self.length {
            return None;
        }
        match &self.storage {
            Storage::Reals(cells) => Some(&cells.get(index)?.values[0]),
            Storage::RealCells(cells) => cells.get(index),
            _ => None,
        }
    }
    fn complex_cell(&self, index: usize) -> Option<&Cell<Rcomplex>> {
        if index >= self.length {
            return None;
        }
        match &self.storage {
            Storage::Complex(cells) => Some(&cells.get(index)?.values[0]),
            _ => None,
        }
    }
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

    /// The actual typed owner, rather than its projected address or byte size,
    /// decides which semantic element kinds can use this storage.
    pub(crate) fn accepts_kind(&self, kind: SEXPTYPE) -> bool {
        match &self.storage {
            Storage::Bytes(_) | Storage::ByteCells(_) => {
                matches!(kind, SEXPTYPE::RAWSXP | SEXPTYPE::CHARSXP)
            }
            Storage::Integers(_) | Storage::IntegerCells(_) => {
                matches!(kind, SEXPTYPE::LGLSXP | SEXPTYPE::INTSXP)
            }
            Storage::Reals(_) | Storage::RealCells(_) => kind == SEXPTYPE::REALSXP,
            Storage::Complex(_) => kind == SEXPTYPE::CPLXSXP,
            Storage::References(_) => matches!(
                kind,
                SEXPTYPE::STRSXP | SEXPTYPE::VECSXP | SEXPTYPE::EXPRSXP | SEXPTYPE::BCODESXP
            ),
        }
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
            Storage::ByteCells(values) => values.as_ptr().cast::<u8>().cast_mut(),
            Storage::IntegerCells(values) => values.as_ptr().cast::<u8>().cast_mut(),
            Storage::RealCells(values) => values.as_ptr().cast::<u8>().cast_mut(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_cell_ownership_rejects_safe_aliases_before_sealing() {
        let cells: Rc<[Cell<u8>]> = Rc::from([Cell::new(b'a'), Cell::new(0)]);
        assert!(matches!(
            PayloadLease::from_byte_cells(cells.clone()),
            Err(PayloadError::AliasedStorage)
        ));
        let weak = Rc::downgrade(&cells);
        assert!(matches!(
            PayloadLease::from_byte_cells(cells),
            Err(PayloadError::AliasedStorage)
        ));
        assert!(weak.upgrade().is_none());

        let cells: Rc<[Cell<u8>]> = Rc::from([Cell::new(b'a'), Cell::new(0)]);
        let projection = cells.as_ptr().cast_mut().cast::<u8>();
        let lease = PayloadLease::from_byte_cells(cells).unwrap();
        assert_eq!(lease.native_projection(), projection);
        let heap = HeapIdentity::new();
        let charge = Rc::new(Cell::new(0));
        lease.bind_to_heap(&heap, &charge).unwrap();
        lease.make_immutable();
        assert!(lease.is_immutable());
        assert!(lease.set_byte_elt(0, b'b').is_none());
        assert_eq!(lease.byte_elt(0), Some(b'a'));
        // Reattaching in the original domain cannot mint another charge.
        lease.bind_to_heap(&heap, &charge).unwrap();
        assert_eq!(charge.get(), 2);
        let foreign = Rc::new(Cell::new(0));
        assert!(lease.bind_to_heap(&HeapIdentity::new(), &foreign).is_none());
        assert_eq!(foreign.get(), 0);
        drop(lease);
        assert_eq!(charge.get(), 0);
    }

    #[test]
    fn cell_wrappers_keep_original_storage_and_immutable_leases_reject_writes() {
        let cells: Rc<[Cell<i32>]> = Rc::from([Cell::new(7), Cell::new(11)]);
        let lease = PayloadLease::from_integer_cells(cells.clone()).unwrap();
        assert_eq!(
            lease.native_projection(),
            cells.as_ptr().cast_mut().cast::<u8>()
        );
        lease.set_integer_elt(1, 29).unwrap();
        assert_eq!(cells[1].get(), 29);
        assert!(lease.real_elt(0).is_none());
        assert!(lease.set_integer_elt(2, 44).is_none());
        lease.make_immutable();
        assert!(lease.set_integer_elt(1, 44).is_none());
        assert_eq!(lease.integer_elt(1), Some(29));
        let heap = HeapIdentity::new();
        assert!(lease.bind_to_heap(&heap, &Rc::new(Cell::new(0))).is_none());
    }

    #[test]
    fn allocation_binding_is_unique_and_reference_lease_pins_its_charge() {
        let heap = HeapIdentity::new();
        let total = Rc::new(Cell::new(0));
        let lease = PayloadLease::zeroed(SEXPTYPE::VECSXP, 2).unwrap();
        lease.bind_to_heap(&heap, &total).unwrap();
        lease.bind_to_heap(&heap, &total).unwrap();
        let bytes = 2 * std::mem::size_of::<NodeLink>();
        assert_eq!(total.get(), bytes);
        let foreign = HeapIdentity::new();
        let other_total = Rc::new(Cell::new(0));
        assert!(lease.bind_to_heap(&foreign, &other_total).is_none());
        assert_eq!(other_total.get(), 0);
        let reference = lease.reference_lease().unwrap();
        let weak = lease.downgrade();
        drop(lease);
        assert!(weak.upgrade().is_some());
        assert_eq!(total.get(), bytes);
        drop(reference);
        assert!(weak.upgrade().is_none());
        assert_eq!(total.get(), 0);
    }

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
        let weak = Rc::downgrade(&lease.cells);
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
        let page = super::super::heap::NodePage::try_new(
            super::super::heap::HeapIdentity::new(),
            0,
            1,
            || (),
        )
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
                    Storage::ByteCells(_) | Storage::IntegerCells(_) | Storage::RealCells(_) => {
                        unreachable!("zeroed vector chunks")
                    }
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
