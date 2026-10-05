//! Bounded, nonowning hints for immutable ASCII characters in this arena.
#![forbid(unsafe_code)]
use super::{NodeLink, PayloadLink, RArena, SEXP, SEXPTYPE};
use std::collections::HashMap;

const MAX_ENTRIES: usize = 4096;
const MAX_KEY_BYTES: usize = 256 * 1024;
const MAX_CHARACTER_BYTES: usize = 256;

#[derive(Clone, Copy)]
struct CachedCharacter {
    node: NodeLink,
    payload: PayloadLink,
}

#[derive(Default)]
pub(super) struct CharacterCache {
    entries: HashMap<Vec<u8>, CachedCharacter>,
    key_bytes: usize,
}

impl RArena {
    pub(super) fn cached_ascii_character(&mut self, bytes: &[u8]) -> Option<SEXP> {
        if bytes.len() > MAX_CHARACTER_BYTES || !bytes.is_ascii() {
            return None;
        }
        let cached = *self.character_cache.entries.get(bytes)?;
        // The hint grants no authority: resolve its original generation in
        // this backing, then verify the actual immutable shape and encoding.
        let live = (|| {
            let (projection, node) = self.backing.resolve_link(cached.node)?;
            let header = self.backing.node_snapshot(node.id())?;
            let payload = self.backing.node_payload(node.id())?;
            (header.sxpinfo.type_of() == SEXPTYPE::CHARSXP
                && !header.sxpinfo.alt()
                && !header.sxpinfo.obj()
                && header.sxpinfo.gp() == 1 << 6
                && header.attrib.is_null()
                && header.data.vector().length == bytes.len() as super::R_xlen_t
                && header.data.vector().truelength == 0
                && matches!(
                    header.data.vector().metadata,
                    super::super::ffi::VectorMetadata::None
                )
                && header.payload == cached.payload
                && payload.is_immutable()
                && payload.matches_header(&header))
            .then_some(projection)
        })();
        if live.is_none() {
            self.character_cache.entries.remove(bytes);
            self.character_cache.key_bytes -= bytes.len();
        }
        live
    }

    pub(super) fn cache_ascii_character(&mut self, bytes: &[u8], value: SEXP) {
        if bytes.len() > MAX_CHARACTER_BYTES || !bytes.is_ascii() {
            return;
        }
        let Some(node) = self.node_token(value) else {
            return;
        };
        let Some(link) = node.link() else { return };
        let Some(header) = self.backing.node_snapshot(node.id()) else {
            return;
        };
        if self.character_cache.entries.len() >= MAX_ENTRIES
            || self.character_cache.key_bytes + bytes.len() > MAX_KEY_BYTES
        {
            // Plain identities and byte keys cannot retain a graph or invoke
            // a destructor callback. A full hint table can safely start over.
            self.character_cache.entries.clear();
            self.character_cache.key_bytes = 0;
        }
        let mut key = Vec::new();
        if key.try_reserve_exact(bytes.len()).is_err()
            || self.character_cache.entries.try_reserve(1).is_err()
        {
            return; // Cache admission must never reject a completed allocation.
        }
        key.extend_from_slice(bytes);
        let previous = self.character_cache.entries.insert(
            key,
            CachedCharacter {
                node: link,
                payload: header.payload,
            },
        );
        if previous.is_none() {
            self.character_cache.key_bytes += bytes.len();
        }
    }
}
