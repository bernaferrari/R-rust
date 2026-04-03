use crate::gc::Trace;
use crate::sexp::{Header, SEXPTYPE};

/// Trait for GC-managed objects
pub unsafe trait Object: Trace {
    fn header(&self) -> &Header;
    fn size(&self) -> usize;
}

/// Stub implementation for ()
unsafe impl Trace for () {
    unsafe fn trace(&mut self) {}
}

unsafe impl Object for () {
    fn header(&self) -> &Header {
        static HEADER: Header = Header::new(SEXPTYPE::NILSXP);
        &HEADER
    }

    fn size(&self) -> usize {
        0
    }
}
