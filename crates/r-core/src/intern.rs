#![forbid(unsafe_code)]
//! String and symbol interning.

use std::collections::HashMap;
use std::rc::Rc;

/// Interned string handle — cheap to copy, compare, and store per element.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct InternedStr(pub u32);

/// Interned symbol handle.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct SymId(pub u32);

/// The interner owns one copy of each distinct string. Symbol and string
/// namespaces are separate because symbols compare by identity semantics
/// while character vectors carry encoding baggage in the full migration.
#[derive(Debug, Default)]
pub struct SymbolInterner {
    symbols: Vec<Rc<str>>,
    symbol_index: HashMap<Rc<str>, SymId>,
    strings: Vec<Rc<str>>,
    string_index: HashMap<Rc<str>, InternedStr>,
}

impl SymbolInterner {
    pub fn new() -> SymbolInterner {
        SymbolInterner::default()
    }

    pub fn intern_symbol(&mut self, name: &str) -> SymId {
        if let Some(id) = self.symbol_index.get(name) {
            return *id;
        }
        let rc: Rc<str> = Rc::from(name);
        let id = SymId(self.symbols.len() as u32);
        self.symbols.push(rc.clone());
        self.symbol_index.insert(rc, id);
        id
    }

    pub fn symbol_name(&self, id: SymId) -> &str {
        &self.symbols[id.0 as usize]
    }

    pub fn intern_string(&mut self, text: &str) -> InternedStr {
        if let Some(id) = self.string_index.get(text) {
            return *id;
        }
        let rc: Rc<str> = Rc::from(text);
        let id = InternedStr(self.strings.len() as u32);
        self.strings.push(rc.clone());
        self.string_index.insert(rc, id);
        id
    }

    pub fn string_text(&self, id: InternedStr) -> &str {
        &self.strings[id.0 as usize]
    }

    pub fn symbol_count(&self) -> usize {
        self.symbols.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_is_stable_and_deduplicating() {
        let mut interner = SymbolInterner::new();
        let a = interner.intern_symbol("alpha");
        let b = interner.intern_symbol("beta");
        assert_eq!(interner.intern_symbol("alpha"), a);
        assert_ne!(a, b);
        assert_eq!(interner.symbol_name(a), "alpha");
        assert_eq!(interner.symbol_count(), 2);

        let s = interner.intern_string("hello");
        assert_eq!(interner.intern_string("hello"), s);
        assert_eq!(interner.string_text(s), "hello");
    }
}
