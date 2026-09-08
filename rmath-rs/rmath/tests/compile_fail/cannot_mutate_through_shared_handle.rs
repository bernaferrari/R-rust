// The translated runtime is crate-private; embedding uses owned values.
//~ ERROR: module `sexp` is private
use rmath::sexp::memory::RArena;
use rmath::sexp::SEXPTYPE;

pub fn forbidden() {
    let mut arena = RArena::new();
    let sexp = arena
        .alloc_vector_sexp(SEXPTYPE::INTSXP, 3)
        .expect("arena allocation failed");

    // Taking the bulk view consumes the handle: the slice outlives any
    // further use of `sexp` as a value.
    let slice = sexp.as_integer_slice().expect("integer vector");

    // Mutating through the (still non-mut, shared) binding while the
    // slice view is live: rejected, `sexp` was moved into the view.
    sexp.set_integer_elt(0, 42); 

    // The view is provably live across the mutation window above.
    let _sum = slice[0] + slice[1] + slice[2];
}
