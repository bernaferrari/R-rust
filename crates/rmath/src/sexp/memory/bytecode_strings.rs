#![forbid(unsafe_code)]
//! Weak hints for immutable scalar literals in imported executable code.

use super::{NodeBody, NodeLink, RArena, SEXP, SEXPTYPE, WorkspaceReservation};
use crate::sexp::ffi::VectorMetadata;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    ptr,
};

const SLOTS: usize = 8192;

#[derive(Clone, Copy)]
struct Hint {
    character: NodeLink,
    scalar: NodeLink,
}

#[derive(Default)]
pub(super) struct BytecodeStringCache {
    slots: Vec<Option<Hint>>,
    // Storage drops before its original-session reservation.
    _reservation: Option<WorkspaceReservation>,
}

fn slot(character: NodeLink) -> usize {
    let mut hash = DefaultHasher::new();
    character.hash(&mut hash);
    hash.finish() as usize & (SLOTS - 1)
}

impl RArena {
    fn cached_bytecode_string(&self, key: NodeLink) -> Option<SEXP> {
        let hint = self
            .bytecode_string_cache
            .slots
            .get(slot(key))
            .copied()
            .flatten()?;
        if hint.character != key {
            return None;
        }
        let (pointer, node) = self.backing.resolve_link(hint.scalar)?;
        let header = self.backing.node_snapshot(node.id())?;
        let NodeBody::Vector(vector) = header.data else {
            return None;
        };
        let payload = self.backing.node_payload(node.id())?;
        (header.sxpinfo.type_of() == SEXPTYPE::STRSXP
            && header.sxpinfo.named() == 3
            && header.sxpinfo.gp() == 0
            && !header.sxpinfo.obj()
            && !header.sxpinfo.alt()
            && header.attrib.is_null()
            && vector.length == 1
            && vector.truelength == 0
            && matches!(vector.metadata, VectorMetadata::None)
            && payload.is_immutable()
            && payload.matches_header(&header)
            && payload.reference_elt(0) == Some(key))
        .then_some(pointer)
    }

    pub(crate) fn immutable_bytecode_string(&mut self, character: SEXP) -> SEXP {
        if let Some(value) = self
            .heap_identity()
            .link_from_projection(character)
            .and_then(|key| self.cached_bytecode_string(key))
        {
            return value;
        }
        let scalar = self.alloc_vector(SEXPTYPE::STRSXP, 1);
        if scalar.is_null() || self.set_reference_element(scalar, 0, character).is_none() {
            return ptr::null_mut();
        }
        let node = self.node_token(scalar).expect("fresh scalar string");
        let mut header = self
            .backing
            .node_snapshot(node.id())
            .expect("fresh scalar header");
        header.sxpinfo.set_named(3);
        header.data.vector_mut().truelength = 0;
        self.heap_identity()
            .replace_node(&node, header)
            .expect("unchanged scalar shape");
        self.backing
            .node_payload(node.id())
            .expect("scalar cells")
            .make_immutable();

        if self.bytecode_string_cache.slots.is_empty() {
            let bytes = SLOTS * std::mem::size_of::<Option<Hint>>();
            if let Some(reservation) = self.reserve_workspace(bytes) {
                let mut slots = Vec::new();
                if slots.try_reserve_exact(SLOTS).is_ok() {
                    slots.resize_with(SLOTS, || None);
                    self.bytecode_string_cache = BytecodeStringCache {
                        slots,
                        _reservation: Some(reservation),
                    };
                }
            }
        }
        if let Some(key) = self.heap_identity().link_from_projection(character) {
            if let Some(entry) = self.bytecode_string_cache.slots.get_mut(slot(key)) {
                *entry = Some(Hint {
                    character: key,
                    scalar: node.link().expect("managed scalar"),
                });
            }
        }
        scalar
    }
}
