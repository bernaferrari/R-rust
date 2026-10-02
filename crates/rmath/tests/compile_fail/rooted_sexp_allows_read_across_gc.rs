// The translated runtime is crate-private; embedding uses owned values.
//~ ERROR: module `sexp` is private
// A Clone handle roots its allocation automatically across collection.
use rmath::sexp::{RSession, SEXPTYPE};

fn main() {
    let session = RSession::new();
    let raw = session.with_arena(|arena| {
        arena.alloc_vector_sexp(SEXPTYPE::INTSXP, 3).unwrap().as_raw()
    }).unwrap();
    let value = session.sexp(raw).unwrap();
    let readback = value.clone();
    drop(value);
    session.gc();
    assert_eq!(readback.len(), 3);
    assert_eq!(readback.integer_elt(0), Some(0));
}
