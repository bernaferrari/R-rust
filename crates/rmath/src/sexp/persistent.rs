#![forbid(unsafe_code)]
//! Owned permanent headers share the arena's checked allocation identity.
//! Raw addresses are compatibility projections; Rust cells own every header.

use crate::sexp::{
    ffi::{SEXP, SEXPTYPE, SexprecCore, SexprecData, Vecsxp},
    heap::{CheckedNode, HeapError, HeapIdentity, NodePage},
    memory::{NodePageRegistration, register_node_page},
};
use std::{cell::Cell, collections::HashMap, rc::Rc};

enum PersistentPayload {
    Bytes(Rc<[Cell<u8>]>),
    Integers(Rc<[Cell<i32>]>),
    Reals(Rc<[Cell<f64>]>),
    Pointers(Rc<[Cell<SEXP>]>),
}
impl PersistentPayload {
    fn pointer(&self) -> SEXP {
        // The projection covers the whole shared interior-cell slice, so
        // native span reads retain provenance across all its elements.
        match self {
            Self::Bytes(values) => values.as_ptr().cast_mut().cast(),
            Self::Integers(values) => values.as_ptr().cast_mut().cast(),
            Self::Reals(values) => values.as_ptr().cast_mut().cast(),
            Self::Pointers(values) => values.as_ptr().cast_mut().cast(),
        }
    }
}
fn scalar_cells<T>(value: T) -> Result<Rc<[Cell<T>]>, HeapError> {
    let mut cells = Vec::new();
    cells
        .try_reserve_exact(1)
        .map_err(|_| HeapError::Allocation)?;
    cells.push(Cell::new(value));
    Ok(Rc::from(cells.into_boxed_slice()))
}
struct PersistentAllocation {
    _registration: NodePageRegistration,
    header: NodePage<SexprecCore>,
    // Fixed interior cells keep a native character projection stable without
    // handing Rust ownership of the bytes to a raw allocation or header.
    _payload: Option<PersistentPayload>,
}

pub(crate) struct PersistentHeap {
    identity: HeapIdentity,
    next_page: usize,
    nodes: HashMap<usize, PersistentAllocation>,
}
impl PersistentHeap {
    pub(crate) fn new(identity: HeapIdentity) -> Self {
        Self {
            identity,
            next_page: 0,
            nodes: HashMap::new(),
        }
    }
    fn allocate(
        &mut self,
        header: SexprecCore,
        payload: Option<PersistentPayload>,
    ) -> Result<SEXP, HeapError> {
        self.nodes
            .try_reserve(1)
            .map_err(|_| HeapError::Allocation)?;
        let next_page = self.next_page.checked_add(1).ok_or(HeapError::Allocation)?;
        let mut initial = Some(header);
        let page = NodePage::try_new(self.identity.clone(), self.next_page, 1, || {
            initial
                .take()
                .expect("one header per persistent allocation")
        })?;
        page.metadata().activate(0, true)?;
        let pointer = page.raw_slot(0).ok_or(HeapError::InvalidSlot)?;
        let registration = register_node_page(&page);
        self.nodes.insert(
            pointer as usize,
            PersistentAllocation {
                _registration: registration,
                header: page,
                _payload: payload,
            },
        );
        self.next_page = next_page;
        Ok(pointer)
    }
    pub(crate) fn allocate_header(&mut self, header: SexprecCore) -> Result<SEXP, HeapError> {
        self.allocate(header, None)
    }
    pub(crate) fn allocate_chars(&mut self, bytes: &[u8]) -> Result<SEXP, HeapError> {
        let length = i64::try_from(bytes.len()).map_err(|_| HeapError::Allocation)?;
        let capacity = bytes.len().checked_add(1).ok_or(HeapError::Allocation)?;
        let mut cells = Vec::new();
        cells
            .try_reserve_exact(capacity)
            .map_err(|_| HeapError::Allocation)?;
        cells.extend(bytes.iter().copied().map(Cell::new));
        cells.push(Cell::new(0));
        let cells: Rc<[Cell<u8>]> = Rc::from(cells.into_boxed_slice());
        let mut header = SexprecCore::new(SEXPTYPE::CHARSXP);
        header.data = SexprecData {
            vecsxp: Vecsxp {
                length,
                truelength: 0,
            },
        };
        let payload = PersistentPayload::Bytes(cells);
        header.gengc_next_node = payload.pointer();
        self.allocate(header, Some(payload))
    }
    fn allocate_vector(
        &mut self,
        kind: SEXPTYPE,
        payload: PersistentPayload,
    ) -> Result<SEXP, HeapError> {
        let mut header = SexprecCore::new_vector(kind, 1);
        header.gengc_next_node = payload.pointer();
        self.allocate(header, Some(payload))
    }
    pub(crate) fn allocate_integer(
        &mut self,
        value: i32,
        logical: bool,
    ) -> Result<SEXP, HeapError> {
        self.allocate_vector(
            if logical {
                SEXPTYPE::LGLSXP
            } else {
                SEXPTYPE::INTSXP
            },
            PersistentPayload::Integers(scalar_cells(value)?),
        )
    }
    pub(crate) fn allocate_real(&mut self, value: f64) -> Result<SEXP, HeapError> {
        self.allocate_vector(
            SEXPTYPE::REALSXP,
            PersistentPayload::Reals(scalar_cells(value)?),
        )
    }
    pub(crate) fn allocate_string(&mut self, value: SEXP) -> Result<SEXP, HeapError> {
        self.allocate_vector(
            SEXPTYPE::STRSXP,
            PersistentPayload::Pointers(scalar_cells(value)?),
        )
    }
    pub(crate) fn token(&self, pointer: SEXP) -> Option<CheckedNode> {
        self.nodes.get(&(pointer as usize))?.header.token(0)
    }
    /// Recover the allocation's own cell projection, rather than trusting the
    /// provenance of an address supplied by a native caller.
    pub(crate) fn canonical_projection(&self, pointer: SEXP) -> Option<SEXP> {
        let allocation = self.nodes.get(&(pointer as usize))?;
        allocation.header.token(0)?;
        allocation.header.raw_slot(0)
    }
    pub(crate) fn contains(&self, pointer: SEXP) -> bool {
        self.token(pointer).is_some()
    }
    pub(crate) fn remove(&mut self, pointer: SEXP) -> bool {
        self.nodes.remove(&(pointer as usize)).is_some()
    }
    pub(crate) fn projections(&self) -> impl Iterator<Item = SEXP> + '_ {
        self.nodes
            .values()
            .filter_map(|owned| owned.header.raw_slot(0))
    }
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.nodes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persistent_tokens_share_heap_and_expire_when_storage_is_removed() {
        let identity = HeapIdentity::new();
        let mut left = PersistentHeap::new(identity.clone());
        let mut right = PersistentHeap::new(identity);
        let first = left
            .allocate_header(SexprecCore::new(SEXPTYPE::LISTSXP))
            .unwrap();
        let token = left.token(first).unwrap();
        let second = right.allocate_chars(b"persistent").unwrap();
        let other = right.token(second).unwrap();
        assert!(token.same_heap(&other));
        assert_ne!(token, other);
        assert!(right.token(first).is_none());
        assert!(left.remove(first));
        assert!(!token.is_live());
        let next = left
            .allocate_header(SexprecCore::new(SEXPTYPE::LISTSXP))
            .unwrap();
        assert_ne!(token, left.token(next).unwrap());
        drop(right);
        assert!(!other.is_live());
    }
}
