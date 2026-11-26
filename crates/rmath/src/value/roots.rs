#![forbid(unsafe_code)]
//! Unified rooting: scoped evaluation roots and stable host handles.

use super::id::ValueId;

/// A scoped stack of root frames for evaluator temporaries.
///
/// Frames are pushed/popped in strict LIFO order through RAII guards; every
/// handle inside a live frame is traced as a GC root. This replaces the
/// manual `PROTECT`/`UNPROTECT` protocol with compile-time enforced scoping.
#[derive(Debug, Default)]
pub struct RootStack {
    frames: Vec<Vec<ValueId>>,
}

impl RootStack {
    pub fn new() -> RootStack {
        RootStack::default()
    }

    /// Open a new root frame. The frame closes when the returned guard drops.
    pub fn frame(&mut self) -> RootFrame<'_> {
        self.frames.push(Vec::new());
        RootFrame { stack: self }
    }

    /// Rooted handles across all live frames, innermost-last ordering.
    pub(crate) fn roots(&self) -> impl Iterator<Item = ValueId> + '_ {
        self.frames.iter().flat_map(|frame| frame.iter().copied())
    }

    pub fn frame_depth(&self) -> usize {
        self.frames.len()
    }
}

/// RAII guard for one root frame. Root additional handles with [`protect`].
pub struct RootFrame<'a> {
    stack: &'a mut RootStack,
}

impl RootFrame<'_> {
    /// Add a handle to this frame, keeping it live until the frame closes.
    pub fn protect(&mut self, id: ValueId) -> ValueId {
        self.stack
            .frames
            .last_mut()
            .expect("frame guard outlives its frame")
            .push(id);
        id
    }
}

impl Drop for RootFrame<'_> {
    fn drop(&mut self) {
        let frame = self.stack.frames.pop();
        debug_assert!(frame.is_some(), "root frames must close in LIFO order");
    }
}

/// A stable token for a long-lived root. Copy-able and generation-checked:
/// releasing twice or after-table-reset is a no-op rather than a use-after-free.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct RootToken {
    index: u32,
    generation: u32,
}

/// Stable roots for host-held handles and long-lived runtime caches.
///
/// Unlike the LIFO stack, table entries persist independently until
/// explicitly released — this backs `ValueHandle`-style host references.
#[derive(Debug, Default)]
pub struct RootTable {
    entries: Vec<RootEntry>,
    free_head: Option<u32>,
}

#[derive(Debug)]
enum RootEntry {
    Occupied { generation: u32, id: ValueId },
    Free { generation: u32, next_free: Option<u32> },
}

impl RootTable {
    pub fn new() -> RootTable {
        RootTable::default()
    }

    /// Register a long-lived root; the returned token releases it.
    pub fn register(&mut self, id: ValueId) -> RootToken {
        match self.free_head {
            Some(index) => {
                let entry = &mut self.entries[index as usize];
                let (generation, next_free) = match entry {
                    RootEntry::Free {
                        generation,
                        next_free,
                    } => (*generation, *next_free),
                    RootEntry::Occupied { .. } => unreachable!(),
                };
                let generation = generation.wrapping_add(1).max(1);
                *entry = RootEntry::Occupied { generation, id };
                self.free_head = next_free;
                RootToken { index, generation }
            }
            None => {
                let index = self.entries.len() as u32;
                self.entries.push(RootEntry::Occupied { generation: 1, id });
                RootToken {
                    index,
                    generation: 1,
                }
            }
        }
    }

    /// The rooted handle for a valid token.
    pub fn get(&self, token: RootToken) -> Option<ValueId> {
        match self.entries.get(token.index as usize)? {
            RootEntry::Occupied { generation, id } if *generation == token.generation => Some(*id),
            _ => None,
        }
    }

    /// Release a root. Releasing twice is a harmless no-op.
    pub fn release(&mut self, token: RootToken) {
        let Some(entry) = self.entries.get_mut(token.index as usize) else {
            return;
        };
        let generation = match entry {
            RootEntry::Occupied { generation, .. } if *generation == token.generation => *generation,
            _ => return,
        };
        *entry = RootEntry::Free {
            generation,
            next_free: self.free_head,
        };
        self.free_head = Some(token.index);
    }

    pub(crate) fn roots(&self) -> impl Iterator<Item = ValueId> + '_ {
        self.entries.iter().filter_map(|entry| match entry {
            RootEntry::Occupied { id, .. } => Some(*id),
            RootEntry::Free { .. } => None,
        })
    }

    pub fn live_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e, RootEntry::Occupied { .. }))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_nest_and_release_in_order() {
        let mut stack = RootStack::new();
        let id = ValueId::new(7, 1);
        {
            let mut outer = stack.frame();
            outer.protect(id);
            assert_eq!(stack.frame_depth(), 1);
            {
                let mut inner = stack.frame();
                inner.protect(id);
                assert_eq!(stack.frame_depth(), 2);
                assert_eq!(stack.roots().count(), 2);
            }
            assert_eq!(stack.frame_depth(), 1);
        }
        assert_eq!(stack.frame_depth(), 0);
        assert_eq!(stack.roots().count(), 0);
    }

    #[test]
    fn table_tokens_validate_and_invalidate() {
        let mut table = RootTable::new();
        let id = ValueId::new(3, 1);
        let token = table.register(id);
        assert_eq!(table.get(token), Some(id));
        table.release(token);
        assert_eq!(table.get(token), None);
        table.release(token); // double release is a no-op
        let token2 = table.register(id);
        assert_ne!(token, token2);
        assert_eq!(table.get(token2), Some(id));
    }
}
