use crate::gc::Trace;
use crate::sexp::{Header, SEXPTYPE};

pub struct Environment {
    header: Header,
}

impl Environment {
    pub fn new() -> Self {
        Self {
            header: Header::new(SEXPTYPE::ENVSXP),
        }
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl Trace for Environment {
    unsafe fn trace(&mut self) {}
}
