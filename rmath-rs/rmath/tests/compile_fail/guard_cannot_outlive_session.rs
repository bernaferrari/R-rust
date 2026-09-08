// The translated runtime is crate-private; embedding uses owned values.
//~ ERROR: module `sexp` is private
use rmath::sexp::RSession;
use rmath::sexp::protect::protect_sexp;
fn main() {
    let guard = {
        let session = RSession::new();
        protect_sexp(session.global_env().unwrap()) 
    };
    drop(guard);
}
