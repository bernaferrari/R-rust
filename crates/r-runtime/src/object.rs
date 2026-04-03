use crate::gc::Trace;
use crate::sexp::{Header, SEXPTYPE};

/// Base trait for all GC-managed objects.
pub unsafe trait Object: Trace {
    /// Get the common object header.
    fn header(&self) -> &Header;

    /// Get the object's type tag.
    #[inline(always)]
    fn tag(&self) -> SEXPTYPE {
        self.header().tag()
    }

    /// Get the object's size in bytes.
    fn size(&self) -> usize;
}
