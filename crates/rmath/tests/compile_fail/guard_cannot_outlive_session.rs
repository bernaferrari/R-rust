// The translated runtime is crate-private; embedding uses owned values.
//~ ERROR: module `sexp` is private
use rmath::sexp::RSession;

fn main() {
    let value = {
        let session = RSession::new();
        session.global_env().unwrap()
    };
    drop(value);
}
