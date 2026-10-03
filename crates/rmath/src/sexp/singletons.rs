#![forbid(unsafe_code)]
//! Thread-confined immutable values with genuine owned headers and payloads.

use super::super::ffi::{NodeBody, SEXP, SEXPTYPE, SexprecCore, Vecsxp};
use std::{cell::{Cell, RefCell}, rc::Rc};

struct Singleton {
    header: Cell<SexprecCore>,
    _logical: Option<Rc<Cell<i32>>>,
}

/// A lease owns the exact header allocation, independently of the TLS pool.
#[derive(Clone)]
pub(crate) struct SingletonLease(Rc<Singleton>);

impl std::fmt::Debug for SingletonLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SingletonLease").finish_non_exhaustive()
    }
}

impl SingletonLease {
    fn new(kind: SEXPTYPE, marked: bool, na: bool, logical: Option<i32>) -> Self {
        let logical = logical.map(|value| Rc::new(Cell::new(value)));
        let mut header = SexprecCore::new(kind);
        header.sxpinfo.set_mark(marked);
        header.sxpinfo.set_named(2);
        header.sxpinfo.set_gp(u16::from(na));
        header.sxpinfo.set_scalar(logical.is_some());
        if kind == SEXPTYPE::CHARSXP || logical.is_some() {
            let length = i64::from(logical.is_some());
            header.data = NodeBody::Vector(Vecsxp { length, truelength: length });
        }
        if let Some(payload) = &logical {
            header.gengc_next_node = payload.as_ptr().cast();
        }
        Self(Rc::new(Singleton { header: Cell::new(header), _logical: logical }))
    }

    pub(crate) fn projection(&self) -> SEXP {
        self.0.header.as_ptr()
    }

    pub(crate) fn snapshot(&self) -> SexprecCore {
        self.0.header.get()
    }
}

struct SingletonPool {
    values: [SingletonLease; 7],
}

/// Checked heap handles retain their sentinel bank so graph edges remain valid
/// when the TLS pool closes before the last handle is released.
#[derive(Clone)]
pub(crate) struct SingletonPoolLease(Rc<SingletonPool>);

impl std::fmt::Debug for SingletonPoolLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SingletonPoolLease").finish_non_exhaustive()
    }
}

impl SingletonPoolLease {
    fn new() -> Self {
        Self(Rc::new(SingletonPool { values: [
            SingletonLease::new(SEXPTYPE::NILSXP, false, false, None),
            SingletonLease::new(SEXPTYPE::SYMSXP, true, false, None),
            SingletonLease::new(SEXPTYPE::SYMSXP, true, false, None),
            SingletonLease::new(SEXPTYPE::SPECIALSXP, true, false, None),
            SingletonLease::new(SEXPTYPE::LGLSXP, false, false, Some(1)),
            SingletonLease::new(SEXPTYPE::LGLSXP, false, false, Some(0)),
            SingletonLease::new(SEXPTYPE::CHARSXP, false, true, None),
        ] }))
    }

    pub(crate) fn lease(&self, candidate: SEXP) -> Option<SingletonLease> {
        self.0.values.iter()
            .find(|value| value.projection() == candidate)
            .cloned()
    }

    pub(crate) fn nil(&self) -> SingletonLease {
        self.0.values[0].clone()
    }

    pub(crate) fn na_string_projection(&self) -> SEXP {
        self.0.values[6].projection()
    }

    pub(crate) fn canonical_projection(&self, candidate: SEXP) -> Option<SEXP> {
        self.lease(candidate).map(|value| value.projection())
    }

    pub(crate) fn snapshot(&self, candidate: SEXP) -> Option<SexprecCore> {
        self.lease(candidate).map(|value| value.snapshot())
    }
}

thread_local! {
    static POOL: RefCell<Option<SingletonPoolLease>> = const { RefCell::new(None) };
}

pub(super) fn pool() -> SingletonPoolLease {
    POOL.with(|slot| slot.borrow_mut().get_or_insert_with(SingletonPoolLease::new).clone())
}

fn current_pool() -> Option<SingletonPoolLease> {
    POOL.try_with(|slot| slot.borrow().clone()).ok().flatten()
}

pub(super) fn nil() -> SEXP { pool().0.values[0].projection() }
pub(super) fn unbound() -> SEXP { pool().0.values[1].projection() }
pub(super) fn missing() -> SEXP { pool().0.values[2].projection() }
pub(super) fn restart() -> SEXP { pool().0.values[3].projection() }
pub(super) fn logical(value: bool) -> SEXP { pool().0.values[if value { 4 } else { 5 }].projection() }
pub(super) fn na_string() -> SEXP { pool().0.values[6].projection() }

pub(super) fn current_na_string_projection() -> Option<SEXP> {
    current_pool().map(|pool| pool.na_string_projection())
}

pub(super) fn canonical_projection(candidate: SEXP) -> Option<SEXP> {
    current_pool()?.canonical_projection(candidate)
}

pub(super) fn snapshot(candidate: SEXP) -> Option<SexprecCore> {
    current_pool()?.snapshot(candidate)
}

#[cfg(test)]
pub(super) fn close_pool_for_test() {
    POOL.with(|slot| { slot.borrow_mut().take(); });
}
