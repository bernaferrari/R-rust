use super::Sexp;

/// An iterator over pairlist (LISTSXP/LANGSXP) elements.
///
/// Yields each cons cell in the chain, stopping at `R_NilValue`.
/// Use [`Sexp::car()`] on each yielded item to access the value,
/// and [`Sexp::tag()`] to access the tag/name.
///
/// # Examples
///
/// ```
/// use rmath::sexp::{Sexp, PairlistIter, SEXPTYPE};
/// use rmath::sexp::memory::RArena;
///
/// let mut arena = RArena::new();
/// let list = arena.alloc_list_chain(3);
/// let sexp = arena.sexp(list).expect("list belongs to arena");
/// let items: Vec<_> = PairlistIter::new(sexp).collect();
/// assert_eq!(items.len(), 3);
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
