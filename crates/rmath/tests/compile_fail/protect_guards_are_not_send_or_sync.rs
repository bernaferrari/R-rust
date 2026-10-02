// The translated runtime is crate-private; embedding uses owned values.
//~ ERROR: module `sexp` is private
// Automatic handles and their owning runtime remain thread-confined.
use rmath::sexp::{RSession, Sexp};

fn require_send<T: Send>() {}
fn require_sync<T: Sync>() {}

fn main() {
    let session = RSession::new();
    let value = session.global_env().unwrap();
    let _send: Box<dyn Send + '_> = Box::new(value);
    require_send::<Sexp<'static>>();
    require_sync::<Sexp<'static>>();
}
