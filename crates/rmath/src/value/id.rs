#![forbid(unsafe_code)]
//! Generational value handles.

/// An 8-byte `Copy` generational handle into the heap.
///
/// A handle is valid only while its slot carries the same generation; after
/// collection frees and reuses the slot the generation bumps and stale
/// handles are rejected rather than dereferenced.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ValueId {
    slot: u32,
    generation: u32,
}

impl Default for ValueId {
    fn default() -> Self {
        ValueId::NIL
    }
}

impl ValueId {
    /// Sentinel generation reserved for constants that never address the heap.
    const SENTINEL_GEN: u32 = u32::MAX;

    /// The nil object — R's `NULL`.
    pub const NIL: ValueId = ValueId::sentinel(0);
    /// The missing-argument marker.
    pub const MISSING: ValueId = ValueId::sentinel(1);
    /// The unbound-value marker inside environments.
    pub const UNBOUND: ValueId = ValueId::sentinel(2);
    /// The empty environment — the termination of every enclosure chain.
    pub const EMPTY_ENV: ValueId = ValueId::sentinel(3);
    /// The base environment sentinel; bound to a real heap object at boot.
    pub const BASE_ENV: ValueId = ValueId::sentinel(4);
    /// The global environment sentinel; bound to a real heap object per session.
    pub const GLOBAL_ENV: ValueId = ValueId::sentinel(5);

    const fn sentinel(tag: u32) -> ValueId {
        ValueId {
            slot: u32::MAX,
            generation: Self::SENTINEL_GEN - tag,
        }
    }

    pub(crate) const fn new(slot: u32, generation: u32) -> ValueId {
        ValueId { slot, generation }
    }

    /// True when this handle is a reserved constant rather than a heap address.
    pub fn is_sentinel(self) -> bool {
        self.slot == u32::MAX
    }

    pub(crate) fn slot_index(self) -> Option<usize> {
        if self.is_sentinel() {
            None
        } else {
            Some(self.slot as usize)
        }
    }

    pub(crate) fn generation(self) -> u32 {
        self.generation
    }
}

impl std::fmt::Debug for ValueId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_sentinel() {
            let name = match *self {
                ValueId::NIL => "NIL",
                ValueId::MISSING => "MISSING",
                ValueId::UNBOUND => "UNBOUND",
                ValueId::EMPTY_ENV => "EMPTY_ENV",
                ValueId::BASE_ENV => "BASE_ENV",
                ValueId::GLOBAL_ENV => "GLOBAL_ENV",
                _ => "SENTINEL?",
            };
            write!(f, "ValueId::{name}")
        } else {
            write!(f, "ValueId({}#{})", self.slot, self.generation)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentinels_are_distinct_and_not_heap_addresses() {
        let all = [
            ValueId::NIL,
            ValueId::MISSING,
            ValueId::UNBOUND,
            ValueId::EMPTY_ENV,
            ValueId::BASE_ENV,
            ValueId::GLOBAL_ENV,
        ];
        for (i, a) in all.iter().enumerate() {
            assert!(a.is_sentinel());
            assert_eq!(a.slot_index(), None);
            for b in all.iter().skip(i + 1) {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn value_id_is_copy_and_eight_bytes() {
        assert_eq!(std::mem::size_of::<ValueId>(), 8);
        fn assert_copy<T: Copy>() {}
        assert_copy::<ValueId>();
    }
}
