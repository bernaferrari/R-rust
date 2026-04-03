use crate::gc::{Gc, Trace, WriteBarrier};
use crate::sexp::{Sexp, Tag};
use crate::symbol::Symbol;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use core::ptr::NonNull;

#[derive(Debug, Clone)]
pub struct Environment {
    parent: Option<Gc<Environment>>,
    frame: BTreeMap<Symbol, Sexp>,
    locked: bool,
    hash: bool,
}

unsafe impl Trace for Environment {
    fn trace(&self) {
        if let Some(parent) = &self.parent {
            parent.trace();
        }
        for (_sym, val) in self.frame.iter() {
            val.trace();
        }
    }
}

impl Environment {
    pub fn new(parent: Option<Gc<Environment>>) -> Self {
        Environment {
            parent,
            frame: BTreeMap::new(),
            locked: false,
            hash: false,
        }
    }

    pub fn parent(&self) -> Option<Gc<Environment>> {
        self.parent.clone()
    }

    pub fn get(&self, sym: Symbol) -> Option<Sexp> {
        match self.frame.get(&sym) {
            Some(val) => Some(val.clone()),
            None => self.parent.as_ref().and_then(|p| p.get(sym)),
        }
    }

    pub fn get_local(&self, sym: Symbol) -> Option<Sexp> {
        self.frame.get(&sym).cloned()
    }

    pub fn define(&mut self, sym: Symbol, value: Sexp) {
        debug_assert!(!self.locked);
        self.frame.insert(sym, value);
    }

    pub fn assign(&mut self, sym: Symbol, value: Sexp) -> Result<(), ()> {
        if self.frame.contains_key(&sym) {
            self.frame.insert(sym, value);
            Ok(())
        } else {
            match &mut self.parent {
                Some(parent) => Gc::get_mut(parent).assign(sym, value),
                None => Err(()),
            }
        }
    }

    pub fn remove(&mut self, sym: Symbol) -> bool {
        self.frame.remove(&sym).is_some()
    }

    pub fn exists(&self, sym: Symbol, inherit: bool) -> bool {
        if self.frame.contains_key(&sym) {
            true
        } else if inherit {
            self.parent
                .as_ref()
                .map_or(false, |p| p.exists(sym, inherit))
        } else {
            false
        }
    }

    pub fn lock(&mut self) {
        self.locked = true;
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    pub fn enable_hash(&mut self) {
        self.hash = true;
    }

    pub fn size(&self) -> usize {
        self.frame.len()
    }
}
