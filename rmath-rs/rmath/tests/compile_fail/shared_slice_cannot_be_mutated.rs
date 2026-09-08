// The translated runtime is crate-private; embedding uses owned values.
//~ ERROR: module `sexp` is private
use rmath::sexp::{SEXPTYPE, memory::RArena};
fn main() {
    let mut arena = RArena::new();
    let x = arena.alloc_vector_sexp(SEXPTYPE::INTSXP, 1).unwrap();
    let view = x.clone().as_integer_slice().unwrap();
    x.set_integer_elt(0, 42); 
    assert_eq!(view[0], 42);
}
