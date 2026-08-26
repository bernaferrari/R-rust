use super::{Sexp, SexpError, SexpResult};
use crate::sexp::accessors::{SETCDR, SETTAG};
use crate::sexp::constructors::Rf_cons;
use crate::sexp::ffi::{SEXP, SEXPTYPE};
use crate::sexp::globals::R_NilValue;
use std::ptr;

/// An iterator over pairlist (LISTSXP/LANGSXP) elements.
///
/// Yields each cons cell in the chain, stopping at `R_NilValue`.
/// Use [`Sexp::car()`] on each yielded item to access the value,
/// and [`Sexp::tag()`] to access the tag/name.
///
/// # Examples
///
/// ```
/// use rmath::sexp::{Sexp, PairlistIter};
/// use rmath::sexp::builder::PairlistBuilder;
/// use rmath::sexp::memory::RArena;
///
/// let mut arena = RArena::new();
/// let first = Sexp::nil();
/// let second = Sexp::nil();
/// let sexp = PairlistBuilder::new()
///     .push_untagged_value(first)
///     .push_untagged_value(second)
///     .build_in(&mut arena)
///     .expect("pairlist allocation failed");
/// let items: Vec<_> = PairlistIter::new(sexp).collect();
/// assert_eq!(items.len(), 2);
/// ```
pub struct PairlistIter<'a> {
    current: Option<Sexp<'a>>,
}

impl<'a> PairlistIter<'a> {
    /// Create a new iterator starting from the given pairlist.
    pub fn new(list: Sexp<'a>) -> Self {
        PairlistIter {
            current: Some(list),
        }
    }
}

impl<'a> Iterator for PairlistIter<'a> {
    type Item = Sexp<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.current?;
        if current.is_nil() {
            self.current = None;
            return None;
        }
        let item = current;
        self.current = item.cdr();
        Some(item)
    }
}

/// Builder for R pairlist chains.
///
/// R's evaluator has many paths that append tagged cons cells. Keeping that
/// mutation here avoids repeating head/tail pointer stitching throughout the
/// safe-ish evaluator boundary while preserving the underlying LISTSXP shape.
pub(crate) struct PairlistBuilder {
    head: SEXP,
    tail: SEXP,
}

impl PairlistBuilder {
    pub(crate) fn new() -> Self {
        Self {
            head: ptr::null_mut(),
            tail: ptr::null_mut(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.head.is_null()
    }

    pub(crate) fn push<'a>(&mut self, value: Sexp<'a>, tag: Option<Sexp<'a>>) -> SexpResult<()> {
        let tag = tag.map(Sexp::as_raw).unwrap_or_else(ptr::null_mut);
        unsafe { self.append_raw(value.as_raw(), tag) }.map(|_| ())
    }

    /// Append a cell and return the raw cons cell so callers can protect it.
    ///
    /// Incremental list builders such as `evalList`/`promiseArgs` evaluate
    /// (and thus may run a collection) between appends; every cell built so
    /// far is reachable only from this builder's raw locals, so callers must
    /// protect each returned cell until the finished list is handed off.
    pub(crate) fn push_cell<'a>(
        &mut self,
        value: Sexp<'a>,
        tag: Option<Sexp<'a>>,
    ) -> SexpResult<SEXP> {
        let tag = tag.map(Sexp::as_raw).unwrap_or_else(ptr::null_mut);
        unsafe { self.append_raw(value.as_raw(), tag) }
    }

    unsafe fn append_raw(&mut self, value: SEXP, tag: SEXP) -> SexpResult<SEXP> {
        unsafe {
            let cell = Rf_cons(value, R_NilValue());
            if cell.is_null() {
                return Err(SexpError::AllocationFailed {
                    object: "pairlist cell",
                });
            }

            if !tag.is_null() {
                SETTAG(cell, tag);
            }

            if self.is_empty() {
                self.head = cell;
            } else {
                SETCDR(self.tail, cell);
            }
            self.tail = cell;
            Ok(cell)
        }
    }

    fn finish_raw(self) -> SEXP {
        if self.head.is_null() {
            unsafe { R_NilValue() }
        } else {
            self.head
        }
    }

    pub(crate) fn finish<'a>(self) -> SexpResult<Sexp<'a>> {
        Sexp::try_from_raw(self.finish_raw())
    }

    pub(crate) fn finish_as_type<'a>(self, sexptype: SEXPTYPE) -> SexpResult<Sexp<'a>> {
        unsafe {
            let head = self.finish_raw();
            if head != R_NilValue() {
                (*head).sxpinfo.set_type(sexptype);
            }
            Sexp::try_from_raw(head)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::accessors::{CAR, CDR, TAG};
    use crate::sexp::constructors::Rf_ScalarInteger;
    use crate::sexp::symbol::Rf_install;

    #[test]
    fn pairlist_builder_preserves_order_and_tags() {
        let session = crate::sexp::session::RSession::new();

        let first = unsafe { Rf_ScalarInteger(1) };
        let second = unsafe { Rf_ScalarInteger(2) };
        let tag = unsafe { Rf_install(c"answer".as_ptr()) };
        let first_value = session.sexp(first).expect("first value belongs to session");
        let second_value = session
            .sexp(second)
            .expect("second value belongs to session");
        let tag_value = session.sexp(tag).expect("tag belongs to session");

        let mut builder = PairlistBuilder::new();
        builder.push(first_value, Some(tag_value)).unwrap();
        builder.push(second_value, None).unwrap();
        let list = builder.finish().expect("pairlist").as_raw();

        unsafe {
            assert_eq!(CAR(list), first);
            assert_eq!(TAG(list), tag);
            assert_eq!(CAR(CDR(list)), second);
            assert_eq!(CDR(CDR(list)), R_NilValue());
        }
    }
}
