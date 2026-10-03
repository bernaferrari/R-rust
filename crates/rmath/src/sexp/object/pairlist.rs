use super::{SessionNodeFactory, Sexp, SexpError, SexpResult};
use crate::sexp::ffi::{EdgeField, NodeBody, SEXPTYPE};
#[cfg(test)]
use crate::sexp::globals::R_NilValue;

/// An iterator over pairlist (LISTSXP/LANGSXP) elements.
///
/// Yields each cons cell in the chain, stopping at `R_NilValue`.
/// Use [`Sexp::car()`] on each yielded item to access the value,
/// and [`Sexp::tag()`] to access the tag/name.
///
/// # Examples
///
/// ```text
/// use crate::sexp::{Sexp, PairlistIter};
/// use crate::sexp::builder::PairlistBuilder;
/// use crate::sexp::memory::RArena;
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
        let current = self.current.clone()?.clone();
        if current.clone().is_nil() {
            self.current = None;
            return None;
        }
        let item = current;
        self.current = item.clone().cdr().clone();
        Some(item)
    }
}

/// Builder for R pairlist chains.
///
/// R's evaluator has many paths that append tagged cons cells. Keeping that
/// mutation here avoids repeating head/tail pointer stitching throughout the
/// checked evaluator boundary while preserving the underlying LISTSXP shape.
pub(crate) struct PairlistBuilder<'a> {
    head: Option<Sexp<'a>>,
    tail: Option<Sexp<'a>>,
    factory: SessionNodeFactory<'a>,
}

impl<'a> PairlistBuilder<'a> {
    /// Start an incremental list in the active owner.
    ///
    /// # Safety
    /// The active owner must remain live for `'a`. Each append must run with
    /// that owner active, with no outstanding Rust payload borrow of a cell.
    pub(crate) unsafe fn new() -> Self {
        // SAFETY: caller supplies the active owner lifetime and excludes loans.
        let owner =
            unsafe { crate::sexp::owner::OwnerToken::current() }.expect("active pairlist owner");
        Self::new_in(owner)
    }

    pub(crate) fn new_in(owner: crate::sexp::owner::OwnerToken<'a>) -> Self {
        Self::from_factory(SessionNodeFactory::new(owner))
    }

    pub(crate) fn from_factory(factory: SessionNodeFactory<'a>) -> Self {
        Self {
            head: None,
            tail: None,
            factory,
        }
    }

    pub(crate) fn wrap(&self, pointer: crate::sexp::ffi::SEXP) -> SexpResult<Sexp<'a>> {
        self.factory.wrap(pointer)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.head.is_none()
    }

    pub(crate) fn push(&mut self, value: Sexp<'a>, tag: Option<Sexp<'a>>) -> SexpResult<()> {
        self.push_cell(value, tag).map(|_| ())
    }

    /// Append a rooted cell. The head retains the chain across subsequent
    /// allocations, and the new cell receives its root before the lend ends.
    pub(crate) fn push_cell(
        &mut self,
        value: Sexp<'a>,
        tag: Option<Sexp<'a>>,
    ) -> SexpResult<Sexp<'a>> {
        let nil = self.factory.nil();
        let cell = self
            .factory
            .pairlist_cell(&value, &nil, tag.as_ref().unwrap_or(&nil))?;
        if let Some(tail) = &self.tail {
            let allocation = tail.allocation()?;
            let heap = allocation.heap_identity();
            let header = heap
                .node_snapshot(allocation)
                .ok_or(SexpError::StaleAllocation)?;
            if !matches!(header.data, NodeBody::List(_))
                || !matches!(
                    header.sxpinfo.type_of(),
                    SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP | SEXPTYPE::DOTSXP
                )
            {
                return Err(SexpError::TypeMismatch {
                    expected: "pairlist tail",
                    actual: header.sxpinfo.type_of(),
                });
            }
            tail.check_child_owner(&cell)?;
            tail.remember_child(&cell)?;
            heap.set_edge(
                allocation,
                EdgeField::ListCdr,
                crate::sexp::heap::ReferenceChild::Node(cell.allocation()?),
            )
            .ok_or(SexpError::StaleAllocation)?;
        } else {
            self.head = Some(cell.clone());
        }
        self.tail = Some(cell.clone());
        Ok(cell)
    }

    pub(crate) fn finish(self) -> SexpResult<Sexp<'a>> {
        let head = self.head.unwrap_or_else(|| self.factory.nil());
        if !head.is_nil() {
            let header = head
                .allocation()?
                .heap_identity()
                .node_snapshot(head.allocation()?)
                .ok_or(SexpError::StaleAllocation)?;
            if !matches!(header.data, NodeBody::List(_))
                || !matches!(
                    header.sxpinfo.type_of(),
                    SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP | SEXPTYPE::DOTSXP
                )
            {
                return Err(SexpError::TypeMismatch {
                    expected: "pairlist head",
                    actual: header.sxpinfo.type_of(),
                });
            }
        }
        Ok(head)
    }

    pub(crate) fn finish_as_type(self, sexptype: SEXPTYPE) -> SexpResult<Sexp<'a>> {
        if !matches!(
            sexptype,
            SEXPTYPE::LISTSXP | SEXPTYPE::LANGSXP | SEXPTYPE::DOTSXP
        ) {
            return Err(SexpError::TypeMismatch {
                expected: "pairlist type",
                actual: sexptype,
            });
        }
        let head = self.finish()?;
        if !head.is_nil() {
            let allocation = head.allocation()?;
            let heap = allocation.heap_identity();
            let mut header = heap
                .node_snapshot(allocation)
                .ok_or(SexpError::StaleAllocation)?;
            header.sxpinfo.set_type(sexptype);
            heap.replace_node(allocation, header)
                .ok_or(SexpError::StaleAllocation)?;
        }
        Ok(head)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::accessors::{CAR, CDR, TAG};
    use crate::sexp::constructors::Rf_ScalarInteger;
    use crate::sexp::symbol::Rf_install;

    #[test]
    fn pairlist_builder_rejects_callback_shape_changes_without_overwriting_them() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();
        session.with_active(|| {
            let factory = session.owner_token().unwrap().node_factory();
            let value = factory.character("kept through callback").unwrap();
            let mut builder = PairlistBuilder::from_factory(factory.clone());
            let head = builder.push_cell(value.clone(), None).unwrap();
            let allocation = head.allocation().unwrap().clone();
            let inspected = allocation.clone();
            let changed = std::rc::Rc::new(std::cell::Cell::new(false));
            let observed = changed.clone();
            crate::sexp::gengc::register_gc_callback(Box::new(move |_| {
                if observed.replace(true) {
                    return;
                }
                crate::sexp::gengc::full_gc();
                let heap = inspected.heap_identity();
                let mut header = heap.node_snapshot(&inspected).unwrap();
                header.sxpinfo.set_type(SEXPTYPE::S4SXP);
                header.data = NodeBody::Other;
                heap.replace_node(&inspected, header).unwrap();
            }));
            session.with_active_in(|owner| unsafe {
                (*owner).memory_state.gc_force_gap = 1;
                (*owner).memory_state.gc_force_wait = 1;
            });
            assert!(matches!(
                builder.push(value.clone(), None),
                Err(SexpError::TypeMismatch {
                    expected: "pairlist tail",
                    actual: SEXPTYPE::S4SXP
                })
            ));
            assert!(changed.get());
            let header = allocation
                .heap_identity()
                .node_snapshot(&allocation)
                .unwrap();
            assert_eq!(header.sxpinfo.type_of(), SEXPTYPE::S4SXP);
            assert!(matches!(header.data, NodeBody::Other));
            assert!(matches!(
                builder.finish(),
                Err(SexpError::TypeMismatch {
                    expected: "pairlist head",
                    actual: SEXPTYPE::S4SXP
                })
            ));
            assert_eq!(value.try_as_string().unwrap(), "kept through callback");
            drop(head);
            drop(value);
            crate::sexp::gengc::full_gc();
            assert!(!allocation.is_live());
        });
    }

    #[test]
    fn pairlist_builder_preserves_order_and_tags() {
        let session = crate::sexp::session::RSession::new_for_gc_tests();

        let first = unsafe { Rf_ScalarInteger(1) };
        let second = unsafe { Rf_ScalarInteger(2) };
        let tag = unsafe { Rf_install(c"answer".as_ptr()) };
        let first_value = session.sexp(first).expect("first value belongs to session");
        let second_value = session
            .sexp(second)
            .expect("second value belongs to session");
        let tag_value = session.sexp(tag).expect("tag belongs to session");

        // SAFETY: session outlives the builder and no cell payload is borrowed.
        let mut builder = unsafe { PairlistBuilder::new() };
        builder.push(first_value, Some(tag_value)).unwrap();
        builder.push(second_value, None).unwrap();
        let list_owner = builder.finish().expect("pairlist");
        let list = list_owner.as_raw();

        unsafe {
            assert_eq!(CAR(list), first);
            assert_eq!(TAG(list), tag);
            assert_eq!(CAR(CDR(list)), second);
            assert_eq!(CDR(CDR(list)), R_NilValue());
        }
    }
}
