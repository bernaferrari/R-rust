use super::*;

mod consts;
mod flags;
#[cfg(test)]
#[path = "owned_collection_tests.rs"]
mod owned_collection_tests;
mod persist;
mod read_item;
mod reader;
mod reader_impl;
#[allow(unused_imports)]
pub(crate) use reader_impl::*; // tests.rs chains here via use super::*
mod reftables;
mod tests;
mod write_item;
mod writer;
pub use consts::*;
pub use flags::*;
pub use persist::*;
pub use read_item::*;
pub use reader::*;
pub use reftables::*;
pub use write_item::*;
pub use writer::*;

// Low-level test convenience; production entry points pass their already pinned owner.
#[cfg(test)]
impl WriteHashTable {
    pub fn new() -> Self {
        Self::for_owner(&unsafe { crate::sexp::owner::OwnerToken::current().unwrap() })
    }
}
#[cfg(test)]
impl ReadRefTable {
    pub fn new() -> Self {
        Self::for_owner(&unsafe { crate::sexp::owner::OwnerToken::current().unwrap() })
    }
}
