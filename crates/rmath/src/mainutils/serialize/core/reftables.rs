use super::*;

// ---------------------------------------------------------------------------
// Internal hash table for write reference tracking
// ---------------------------------------------------------------------------

/// A simple hash table mapping SEXP pointers to reference indices.
/// Stored as a vector of (pointer, index) pairs with linear probing.
pub struct WriteHashTable {
    pub buckets: Vec<Vec<(usize, i32)>>,
    pub count: i32,
    pub(crate) roots: Vec<crate::sexp::protect::ProtectGuard<'static>>,
}

impl WriteHashTable {
    pub fn new() -> Self {
        WriteHashTable {
            buckets: vec![Vec::new(); HASHSIZE as usize],
            count: 0,
            roots: Vec::new(),
        }
    }

    pub fn add(&mut self, obj: SEXP) {
        let key = obj as usize;
        let pos = (key >> 2) % (HASHSIZE as usize);
        self.count += 1;
        self.buckets[pos].push((key, self.count));
        self.roots.push(unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect(obj) });
    }

    pub fn get(&self, item: SEXP) -> i32 {
        let key = item as usize;
        let pos = (key >> 2) % (HASHSIZE as usize);
        for &(k, v) in &self.buckets[pos] {
            if k == key {
                return v;
            }
        }
        0
    }
}

// ---------------------------------------------------------------------------
// Internal read reference table
// ---------------------------------------------------------------------------

/// A growable array for tracking deserialized reference objects.
pub struct ReadRefTable {
    pub entries: Vec<SEXP>,
    pub(crate) roots: Vec<crate::sexp::protect::ProtectGuard<'static>>,
}

impl ReadRefTable {
    pub fn new() -> Self {
        ReadRefTable {
            entries: Vec::with_capacity(INITIAL_REFREAD_TABLE_SIZE as usize),
            roots: Vec::new(),
        }
    }

    pub fn add(&mut self, value: SEXP) {
        self.roots.push(unsafe { /* SAFETY: fixture keeps its owner live; no Rust payload borrow overlaps this raw operation. */ protect(value) });
        self.entries.push(value);
    }

    pub fn get(&self, index: i32) -> Result<SEXP, String> {
        let i = index
            .checked_sub(1)
            .and_then(|i| usize::try_from(i).ok())
            .ok_or_else(|| "reference index out of range".to_string())?;
        if i >= self.entries.len() {
            Err("reference index out of range".into())
        } else {
            Ok(self.entries[i])
        }
    }
}
