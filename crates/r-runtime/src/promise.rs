use crate::gc::Trace;
use crate::sexp::{Header, SEXPTYPE};

pub struct Promise {
    header: Header,
}

impl Promise {
    pub fn new() -> Self {
        Self {
            header: Header::new(SEXPTYPE::PROMSXP),
        }
    }
}

impl Default for Promise {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl Trace for Promise {
    unsafe fn trace(&mut self) {}
}
